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
use discovery::ssdp::discover_ssdp;
use media::manager::list_media_files;
use media::stream::{
    get_transport_state, pause_media, resume_media, seek_media, stop_media, stream_media,
};
use server::http_server::start_http_server;

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

    let selected_location = devices[device_choice - 1]
        .get("LOCATION")
        .ok_or("Selected device has no LOCATION")?
        .clone();

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
    let av_control_url = find_control_url(&description, "AVTransport", &base_url)
        .unwrap_or_else(|| format!("{}/upnp/control/AVTransport1", base_url));
    let cm_control_url = find_control_url(&description, "ConnectionManager", &base_url)
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

        // Poll transport state every 3s; signal when STOPPED
        let (stopped_tx, mut stopped_rx) = tokio::sync::watch::channel(false);
        let av_url = av_control_url.clone();
        let poll_task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                match get_transport_state(&av_url).await {
                    Ok(state) if state == "STOPPED" => {
                        let _ = stopped_tx.send(true);
                        break;
                    }
                    _ => {}
                }
            }
        });

        println!("Controls: [p] Pause/Resume  [s] Stop  [f] Seek  [n] Next  [q] Quit");
        let mut paused = false;

        'control: loop {
            tokio::select! {
                cmd = stdin_rx.recv() => {
                    let cmd = match cmd {
                        Some(c) => c,
                        None => {
                            // stdin closed — stop and exit
                            stop_media(&av_control_url).await.ok();
                            poll_task.abort();
                            quit = true;
                            break 'control;
                        }
                    };
                    match cmd.trim() {
                        "p" => {
                            if paused {
                                match resume_media(&av_control_url).await {
                                    Ok(()) => { paused = false; println!("Resumed."); }
                                    Err(e) => eprintln!("Resume failed: {}", e),
                                }
                            } else {
                                match pause_media(&av_control_url).await {
                                    Ok(()) => { paused = true; println!("Paused."); }
                                    Err(e) => eprintln!("Pause failed: {}", e),
                                }
                            }
                        }
                        "s" => {
                            stop_media(&av_control_url).await.ok();
                            poll_task.abort();
                            break 'control;
                        }
                        "f" => {
                            println!("Enter seek position (HH:MM:SS):");
                            if let Some(pos) = stdin_rx.recv().await {
                                let pos = pos.trim().to_string();
                                match seek_media(&av_control_url, &pos).await {
                                    Ok(()) => println!("Seeked to {}", pos),
                                    Err(e) => eprintln!("Seek failed: {}", e),
                                }
                            }
                        }
                        "n" => {
                            stop_media(&av_control_url).await.ok();
                            poll_task.abort();
                            break 'control;
                        }
                        "q" => {
                            stop_media(&av_control_url).await.ok();
                            poll_task.abort();
                            quit = true;
                            break 'control;
                        }
                        _ => {
                            println!("Controls: [p] Pause/Resume  [s] Stop  [f] Seek  [n] Next  [q] Quit");
                        }
                    }
                }
                changed = stopped_rx.changed() => {
                    if changed.is_ok() && *stopped_rx.borrow() {
                        println!("\nPlayback finished. Auto-advancing...");
                        poll_task.abort();
                        break 'control;
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
