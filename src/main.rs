use futures::future::join_all;
use std::collections::HashSet;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};

mod config;
mod discovery;
mod media;
mod server;

use config::Config;
use discovery::advertise::{send_notify_byebye, start_ssdp_advertiser};
use discovery::device::{
    extract_base_url, fetch_device_description, fetch_device_description_quiet, find_control_url,
};
use discovery::ssdp::{discover_ssdp, rediscover_by_usn};
use media::manager::list_media_files;
use media::stream::{
    get_transport_state, new_soap_client, pause_media, resume_media, seek_media, stop_media,
    stream_media,
};
use server::http_server::start_http_server;

/// Signal sent by the transport-state polling task to the control loop.
#[derive(Clone, PartialEq)]
enum PollSignal {
    Running,
    Stopped,
    DeviceOffline,
}

/// Looks for a `.srt` subtitle file alongside the media file (same base name).
/// Returns the HTTP URL the renderer should use to fetch it, or None if not found.
fn find_subtitle(media_path: &str, http_address: &str, http_port: u16) -> Option<String> {
    let path = std::path::Path::new(media_path);
    let stem = path.file_stem()?.to_str()?;
    let dir = path.parent()?;
    for ext in &["srt", "SRT"] {
        let candidate = dir.join(format!("{}.{}", stem, ext));
        if candidate.exists() {
            let filename = candidate.file_name()?.to_str()?.to_string();
            return Some(format!(
                "http://{}:{}/media/{}",
                http_address, http_port, filename
            ));
        }
    }
    None
}

/// Parses a multi-select string into 0-based indices.
/// Supports: "1", "1,3,5", "2-4", "all"
fn parse_selection(input: &str, count: usize) -> Vec<usize> {
    let input = input.trim().to_lowercase();
    if input == "all" {
        return (0..count).collect();
    }
    let mut indices = Vec::new();
    for part in input.split(',') {
        let part = part.trim();
        if let Some(dash_pos) = part.find('-') {
            let start: usize = part[..dash_pos].trim().parse().unwrap_or(0);
            let end: usize = part[dash_pos + 1..].trim().parse().unwrap_or(0);
            if start > 0 && end >= start {
                for i in start..=end {
                    if i <= count {
                        indices.push(i - 1);
                    }
                }
            }
        } else if let Ok(n) = part.parse::<usize>() {
            if n > 0 && n <= count {
                indices.push(n - 1);
            }
        }
    }
    // Deduplicate while preserving order
    let mut seen = HashSet::new();
    indices.retain(|x| seen.insert(*x));
    indices
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env();
    println!("Starting the {} server", config.friendly_name);

    if !Path::new(&config.media_directory).exists() {
        eprintln!(
            "Error: The configured media directory '{}' does not exist.",
            config.media_directory
        );
        return Err("Invalid media directory".into());
    }

    let server_config = config.clone();
    let server_task = tokio::spawn(async move {
        start_http_server(server_config.http_port, server_config).await;
    });

    let advertiser_task = start_ssdp_advertiser(config.clone());

    // Spawn a stdin reader task that forwards lines to a channel.
    // This prevents dropped futures from losing buffered input.
    let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        let stdin = BufReader::new(tokio::io::stdin());
        let mut lines = stdin.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if stdin_tx.send(line).is_err() {
                break;
            }
        }
    });

    let devices = match discover_ssdp(&config.multicast_address, config.multicast_port).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error discovering SSDP devices: {}", e);
            server_task.abort();
            return Ok(());
        }
    };

    if devices.is_empty() {
        println!("No devices found.");
        server_task.abort();
        return Ok(());
    }

    // Fetch all device descriptions in parallel to show friendly names
    let desc_futures = devices.iter().map(|d| {
        let loc = d.get("LOCATION").cloned();
        async move {
            match loc {
                Some(l) => Some(fetch_device_description_quiet(&l).await),
                None => None,
            }
        }
    });
    let desc_results = join_all(desc_futures).await;

    println!("\nMediaRenderer devices found:");
    for (i, (device, desc_result)) in devices.iter().zip(desc_results.iter()).enumerate() {
        let location = device
            .get("LOCATION")
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        let name = match desc_result {
            Some(Ok(desc)) if !desc.device.friendly_name.is_empty() => {
                desc.device.friendly_name.as_str()
            }
            _ => location,
        };
        println!("{}) {}  ({})", i + 1, name, location);
    }

    println!("\nChoose a device by number (or '0' to exit):");
    let input = stdin_rx.recv().await.unwrap_or_default();
    let device_choice: usize = input.trim().parse().unwrap_or(0);

    if device_choice == 0 || device_choice > devices.len() {
        println!("Exiting...");
        server_task.abort();
        return Ok(());
    }

    let selected_device = &devices[device_choice - 1];
    let selected_location = selected_device
        .get("LOCATION")
        .ok_or("Selected device has no LOCATION")?
        .clone();

    // Retain USN so we can rediscover this device if it goes offline
    let selected_usn = selected_device.get("USN").cloned();

    // Reuse the already-fetched description; fall back to a fresh fetch if it failed
    let description = match desc_results.into_iter().nth(device_choice - 1) {
        Some(Some(Ok(desc))) => desc,
        _ => match fetch_device_description(&selected_location).await {
            Ok(desc) => desc,
            Err(e) => {
                eprintln!("Error fetching device description: {}", e);
                server_task.abort();
                return Ok(());
            }
        },
    };

    let base_url = extract_base_url(&selected_location);
    let mut av_control_url = find_control_url(&description, "AVTransport", &base_url)
        .unwrap_or_else(|| format!("{}/upnp/control/AVTransport1", base_url));
    let mut cm_control_url = find_control_url(&description, "ConnectionManager", &base_url)
        .unwrap_or_else(|| format!("{}/upnp/control/ConnectionManager1", base_url));

    let display_name = if description.device.friendly_name.is_empty() {
        selected_location.as_str()
    } else {
        description.device.friendly_name.as_str()
    };
    println!("Connected to: {}", display_name);

    // List media files
    let media_files = list_media_files(&config.media_directory);
    if media_files.is_empty() {
        println!(
            "No media files found in the directory: {}",
            config.media_directory
        );
        server_task.abort();
        return Ok(());
    }

    println!("\nMedia files found:");
    for (i, file) in media_files.iter().enumerate() {
        println!("{}) {}", i + 1, file.name);
    }

    println!("\nSelect media (e.g. 1  1,3  2-4  all) or '0' to exit:");
    let input = stdin_rx.recv().await.unwrap_or_default();
    if input.trim() == "0" {
        println!("Exiting...");
        server_task.abort();
        return Ok(());
    }

    let selected_indices = parse_selection(input.trim(), media_files.len());
    if selected_indices.is_empty() {
        println!("No valid selection. Exiting...");
        server_task.abort();
        return Ok(());
    }

    let playlist: Vec<&_> = selected_indices.iter().map(|&i| &media_files[i]).collect();
    println!("\nPlaylist ({} file(s)):", playlist.len());
    for (i, file) in playlist.iter().enumerate() {
        println!("  {}) {}", i + 1, file.name);
    }

    // Single shared SOAP client — connection pool is reused across all calls
    let soap_client = new_soap_client();

    let mut quit = false;
    for (idx, media_file) in playlist.iter().enumerate() {
        if quit {
            break;
        }

        println!(
            "\n[{}/{}] Starting: {}",
            idx + 1,
            playlist.len(),
            media_file.name
        );

        let subtitle_url = find_subtitle(&media_file.path, &config.http_address, config.http_port);
        if let Some(ref url) = subtitle_url {
            println!("Subtitle found: {}", url);
        }

        if let Err(e) = stream_media(
            &soap_client,
            &config,
            &av_control_url,
            &cm_control_url,
            media_file,
            subtitle_url.as_deref(),
        )
        .await
        {
            eprintln!("Streaming error: {}", e);
            continue;
        }

        println!("Streaming started!");

        // Poll transport state every 3s.
        // After OFFLINE_THRESHOLD consecutive failures, signals DeviceOffline.
        const OFFLINE_THRESHOLD: u32 = 3;
        let (poll_tx, mut poll_rx) = tokio::sync::watch::channel(PollSignal::Running);
        let poll_client = soap_client.clone();
        let av_url = av_control_url.clone();
        let poll_task = tokio::spawn(async move {
            let mut consecutive_errors: u32 = 0;
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                match get_transport_state(&poll_client, &av_url).await {
                    Ok(state) if state == "STOPPED" => {
                        let _ = poll_tx.send(PollSignal::Stopped);
                        break;
                    }
                    Ok(_) => {
                        consecutive_errors = 0;
                    }
                    Err(e) => {
                        consecutive_errors += 1;
                        eprintln!(
                            "[poll] transport query failed ({}/{}): {}",
                            consecutive_errors, OFFLINE_THRESHOLD, e
                        );
                        if consecutive_errors >= OFFLINE_THRESHOLD {
                            let _ = poll_tx.send(PollSignal::DeviceOffline);
                            break;
                        }
                    }
                }
            }
        });

        println!("Controls: [p] Pause/Resume  [s] Stop  [f] Seek  [n] Next  [q] Quit");

        'control: loop {
            tokio::select! {
                cmd = stdin_rx.recv() => {
                    let cmd = match cmd {
                        Some(c) => c,
                        None => {
                            // stdin closed — stop and exit
                            stop_media(&soap_client, &av_control_url).await.ok();
                            poll_task.abort();
                            quit = true;
                            break 'control;
                        }
                    };
                    match cmd.trim() {
                        "p" => {
                            // Query actual renderer state before toggling to avoid desync
                            match get_transport_state(&soap_client, &av_control_url).await {
                                Ok(state) if state == "PAUSED_PLAYBACK" => {
                                    match resume_media(&soap_client, &av_control_url).await {
                                        Ok(()) => println!("Resumed."),
                                        Err(e) => eprintln!("Resume failed: {}", e),
                                    }
                                }
                                Ok(state) if state == "PLAYING" => {
                                    match pause_media(&soap_client, &av_control_url).await {
                                        Ok(()) => println!("Paused."),
                                        Err(e) => eprintln!("Pause failed: {}", e),
                                    }
                                }
                                Ok(state) => eprintln!("Cannot pause/resume from state: {}", state),
                                Err(e) => eprintln!("Could not query transport state: {}", e),
                            }
                        }
                        "s" => {
                            stop_media(&soap_client, &av_control_url).await.ok();
                            poll_task.abort();
                            break 'control;
                        }
                        "f" => {
                            println!("Enter seek position (HH:MM:SS):");
                            if let Some(pos) = stdin_rx.recv().await {
                                let pos = pos.trim().to_string();
                                match seek_media(&soap_client, &av_control_url, &pos).await {
                                    Ok(()) => println!("Seeked to {}", pos),
                                    Err(e) => eprintln!("Seek failed: {}", e),
                                }
                            }
                        }
                        "n" => {
                            stop_media(&soap_client, &av_control_url).await.ok();
                            poll_task.abort();
                            break 'control;
                        }
                        "q" => {
                            stop_media(&soap_client, &av_control_url).await.ok();
                            poll_task.abort();
                            quit = true;
                            break 'control;
                        }
                        _ => {
                            println!("Controls: [p] Pause/Resume  [s] Stop  [f] Seek  [n] Next  [q] Quit");
                        }
                    }
                }
                changed = poll_rx.changed() => {
                    if changed.is_ok() {
                        match poll_rx.borrow().clone() {
                            PollSignal::Stopped => {
                                println!("\nPlayback finished. Auto-advancing...");
                                poll_task.abort();
                                break 'control;
                            }
                            PollSignal::DeviceOffline => {
                                println!("\nDevice went offline. Attempting rediscovery...");
                                poll_task.abort();

                                let reconnected = if let Some(ref usn) = selected_usn {
                                    match rediscover_by_usn(
                                        &config.multicast_address,
                                        config.multicast_port,
                                        usn,
                                    )
                                    .await
                                    {
                                        Some(device) => device,
                                        None => {
                                            eprintln!("Device not found after rediscovery. Stopping.");
                                            quit = true;
                                            break 'control;
                                        }
                                    }
                                } else {
                                    eprintln!("No USN available for rediscovery. Stopping.");
                                    quit = true;
                                    break 'control;
                                };

                                let new_location = match reconnected.get("LOCATION") {
                                    Some(l) => l.clone(),
                                    None => {
                                        eprintln!("Rediscovered device has no LOCATION. Stopping.");
                                        quit = true;
                                        break 'control;
                                    }
                                };

                                match fetch_device_description_quiet(&new_location).await {
                                    Ok(desc) => {
                                        let new_base = extract_base_url(&new_location);
                                        av_control_url = find_control_url(&desc, "AVTransport", &new_base)
                                            .unwrap_or_else(|| format!("{}/upnp/control/AVTransport1", new_base));
                                        cm_control_url = find_control_url(&desc, "ConnectionManager", &new_base)
                                            .unwrap_or_else(|| format!("{}/upnp/control/ConnectionManager1", new_base));
                                        println!("Device rediscovered. Restarting current track...");
                                        // Break to let the outer playlist loop re-stream the current file
                                        break 'control;
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to fetch description after rediscovery: {}. Stopping.", e);
                                        quit = true;
                                        break 'control;
                                    }
                                }
                            }
                            PollSignal::Running => {}
                        }
                    }
                }
            }
        }
    }

    if !quit {
        println!("\nPlaylist complete. Press Ctrl+C to stop the server.");
        tokio::signal::ctrl_c().await?;
    }

    send_notify_byebye(&config).await;
    println!("Shutting down...");
    advertiser_task.abort();
    server_task.abort();
    Ok(())
}
