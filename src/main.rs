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
use media::manager::{list_media_files, MediaFile};
use media::stream::{
    get_transport_state, new_soap_client, pause_media, resume_media, seek_media, stop_media,
    stream_media,
};
use server::http_server::start_http_server;

/// Signal sent by the transport-state poll task to the control loop.
#[derive(Clone, PartialEq)]
enum PollSignal {
    Running,
    Paused,  // TV paused itself via remote
    Resumed, // TV resumed itself via remote
    Stopped, // playback ended naturally or TV pressed stop
    DeviceOffline,
}

/// What to do after a playlist ends or is stopped.
enum SessionAction {
    ReselectMedia,  // same device, pick new files
    ReselectDevice, // go back to device list
    Rescan,         // full SSDP rediscovery
    Quit,
}

/// Why a per-track control loop exited.
enum TrackExit {
    NextTrack,    // advance playlist ([n] or auto-advance)
    StopPlaylist, // stop here, show post-playlist menu ([s])
    Quit,         // exit the program ([q] or stdin closed)
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Looks for a `.srt` subtitle alongside the media file and returns its HTTP URL.
fn find_subtitle(media_file: &MediaFile, http_address: &str, http_port: u16) -> Option<String> {
    let path = Path::new(&media_file.path);
    let stem = path.file_stem()?.to_str()?;
    let dir = path.parent()?;
    for ext in &["srt", "SRT"] {
        let candidate = dir.join(format!("{}.{}", stem, ext));
        if candidate.exists() {
            let parent_rel = Path::new(&media_file.relative_path)
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let filename = candidate.file_name()?.to_string_lossy().to_string();
            let rel = if parent_rel.is_empty() {
                filename
            } else {
                format!("{}/{}", parent_rel, filename)
            };
            return Some(format!(
                "http://{}:{}/media/{}",
                http_address, http_port, rel
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
    let mut seen = HashSet::new();
    indices.retain(|x| seen.insert(*x));
    indices
}

/// Asks the user what to do after a playlist ends or is stopped.
async fn ask_what_next(
    stdin_rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
) -> SessionAction {
    loop {
        println!("\nWhat would you like to do?");
        println!("  [m] Select new media (same device)");
        println!("  [d] Choose a different device");
        println!("  [r] Rescan for devices");
        println!("  [q] Quit");
        let input = stdin_rx.recv().await.unwrap_or_default();
        match input.trim() {
            "m" | "M" => return SessionAction::ReselectMedia,
            "d" | "D" => return SessionAction::ReselectDevice,
            "r" | "R" => return SessionAction::Rescan,
            "q" | "Q" => return SessionAction::Quit,
            _ => println!("Enter m, d, r, or q."),
        }
    }
}

// ── main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env();
    println!("Starting {}...", config.friendly_name);

    if !Path::new(&config.media_directory).exists() {
        eprintln!(
            "Error: media directory '{}' does not exist.",
            config.media_directory
        );
        return Err("Invalid media directory".into());
    }

    let server_config = config.clone();
    let server_task = tokio::spawn(async move {
        start_http_server(server_config.http_port, server_config).await;
    });

    let advertiser_task = start_ssdp_advertiser(config.clone());

    // Single stdin reader — survives the entire session loop
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

    let soap_client = new_soap_client();

    // State carried across session iterations
    let mut need_device = true;
    let mut av_control_url = String::new();
    let mut cm_control_url = String::new();
    let mut selected_usn: Option<String> = None;

    // ── session loop ──────────────────────────────────────────────────────────
    'session: loop {
        // ── device phase ─────────────────────────────────────────────────────
        if need_device {
            let (devices, desc_results) = loop {
                println!("\nScanning for DLNA renderers...");

                let devices =
                    match discover_ssdp(&config.multicast_address, config.multicast_port).await {
                        Ok(d) => d,
                        Err(e) => {
                            eprintln!("Discovery error: {}", e);
                            Vec::new()
                        }
                    };

                if devices.is_empty() {
                    println!("No devices found.");
                    println!("  [r] Scan again   [q] Quit");
                    let input = stdin_rx.recv().await.unwrap_or_default();
                    match input.trim() {
                        "r" | "R" => continue,
                        _ => break 'session,
                    }
                }

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
                break (devices, desc_results);
            };

            println!("\nDevices found:");
            for (i, (device, desc)) in devices.iter().zip(desc_results.iter()).enumerate() {
                let location = device
                    .get("LOCATION")
                    .map(|s| s.as_str())
                    .unwrap_or("unknown");
                let name = match desc {
                    Some(Ok(d)) if !d.device.friendly_name.is_empty() => {
                        d.device.friendly_name.as_str()
                    }
                    _ => location,
                };
                println!("  {}) {}", i + 1, name);
            }

            println!("\nChoose a device (number) or [q] to quit:");
            let input = stdin_rx.recv().await.unwrap_or_default();
            if input.trim().eq_ignore_ascii_case("q") {
                break 'session;
            }
            let device_choice: usize = input.trim().parse().unwrap_or(0);
            if device_choice == 0 || device_choice > devices.len() {
                println!("Invalid choice — rescanning.");
                continue 'session;
            }

            let selected_device = &devices[device_choice - 1];
            let selected_location = match selected_device.get("LOCATION") {
                Some(l) => l.clone(),
                None => {
                    eprintln!("Selected device has no LOCATION — rescanning.");
                    continue 'session;
                }
            };

            selected_usn = selected_device.get("USN").cloned();

            let description = match desc_results.into_iter().nth(device_choice - 1) {
                Some(Some(Ok(desc))) => desc,
                _ => match fetch_device_description(&selected_location).await {
                    Ok(desc) => desc,
                    Err(e) => {
                        eprintln!("Error fetching device info: {} — rescanning.", e);
                        continue 'session;
                    }
                },
            };

            let base_url = extract_base_url(&selected_location);
            av_control_url = find_control_url(&description, "AVTransport", &base_url)
                .unwrap_or_else(|| format!("{}/upnp/control/AVTransport1", base_url));
            cm_control_url = find_control_url(&description, "ConnectionManager", &base_url)
                .unwrap_or_else(|| format!("{}/upnp/control/ConnectionManager1", base_url));

            let display_name = if description.device.friendly_name.is_empty() {
                selected_location.as_str()
            } else {
                description.device.friendly_name.as_str()
            };
            println!("Connected to: {}", display_name);
            need_device = false;
        }

        // ── media phase ───────────────────────────────────────────────────────
        let media_files = list_media_files(&config.media_directory);
        if media_files.is_empty() {
            println!("No media files found in '{}'.", config.media_directory);
            match ask_what_next(&mut stdin_rx).await {
                SessionAction::Quit => break 'session,
                SessionAction::ReselectMedia => continue 'session,
                SessionAction::ReselectDevice | SessionAction::Rescan => {
                    need_device = true;
                    continue 'session;
                }
            }
        }

        println!("\nMedia files:");
        for (i, file) in media_files.iter().enumerate() {
            println!("  {}) {}", i + 1, file.relative_path);
        }

        // Re-prompt on invalid selection instead of exiting
        let playlist: Vec<&MediaFile> = loop {
            println!("\nSelect (e.g. 1  1,3  2-4  all) or [q] to go back:");
            let input = stdin_rx.recv().await.unwrap_or_default();
            if input.trim().eq_ignore_ascii_case("q") {
                match ask_what_next(&mut stdin_rx).await {
                    SessionAction::Quit => break 'session,
                    SessionAction::ReselectMedia => continue 'session,
                    SessionAction::ReselectDevice | SessionAction::Rescan => {
                        need_device = true;
                        continue 'session;
                    }
                }
            }
            let indices = parse_selection(input.trim(), media_files.len());
            if indices.is_empty() {
                println!("Invalid selection — please try again.");
                continue;
            }
            break indices.iter().map(|&i| &media_files[i]).collect();
        };

        println!("\nPlaylist ({} file(s)):", playlist.len());
        for (i, file) in playlist.iter().enumerate() {
            println!("  {}) {}", i + 1, file.relative_path);
        }

        // ── playlist loop ─────────────────────────────────────────────────────
        let mut track_exit = TrackExit::NextTrack;

        'playlist: for (idx, media_file) in playlist.iter().enumerate() {
            println!(
                "\n[{}/{}] {}",
                idx + 1,
                playlist.len(),
                media_file.relative_path
            );

            let subtitle_url = find_subtitle(media_file, &config.http_address, config.http_port);
            if let Some(ref url) = subtitle_url {
                println!("  Subtitle: {}", url);
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
                eprintln!("Streaming error: {} — skipping.", e);
                continue 'playlist;
            }

            println!("Streaming.");
            println!("Controls: [p] Pause/Resume  [s] Stop & menu  [n] Next  [f] Seek  [q] Quit");

            // Poll task — detects state transitions and signals the control loop
            const OFFLINE_THRESHOLD: u32 = 3;
            let (poll_tx, mut poll_rx) = tokio::sync::watch::channel(PollSignal::Running);
            let poll_client = soap_client.clone();
            let av_url = av_control_url.clone();
            let poll_task = tokio::spawn(async move {
                let mut consecutive_errors: u32 = 0;
                let mut last_state = String::from("PLAYING");
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    match get_transport_state(&poll_client, &av_url).await {
                        Ok(state) if state == "STOPPED" => {
                            let _ = poll_tx.send(PollSignal::Stopped);
                            break;
                        }
                        Ok(state) => {
                            consecutive_errors = 0;
                            if state != last_state {
                                if state == "PAUSED_PLAYBACK" {
                                    let _ = poll_tx.send(PollSignal::Paused);
                                } else if state == "PLAYING" && last_state == "PAUSED_PLAYBACK" {
                                    let _ = poll_tx.send(PollSignal::Resumed);
                                }
                                last_state = state;
                            }
                        }
                        Err(e) => {
                            consecutive_errors += 1;
                            eprintln!(
                                "[poll] error ({}/{}): {}",
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

            // Control loop
            'control: loop {
                tokio::select! {
                    cmd = stdin_rx.recv() => {
                        let cmd = match cmd {
                            Some(c) => c,
                            None => {
                                stop_media(&soap_client, &av_control_url).await.ok();
                                poll_task.abort();
                                track_exit = TrackExit::Quit;
                                break 'control;
                            }
                        };
                        match cmd.trim() {
                            "p" => {
                                match get_transport_state(&soap_client, &av_control_url).await {
                                    Ok(s) if s == "PAUSED_PLAYBACK" => {
                                        match resume_media(&soap_client, &av_control_url).await {
                                            Ok(()) => println!("Resumed."),
                                            Err(e) => eprintln!("Resume failed: {}", e),
                                        }
                                    }
                                    Ok(s) if s == "PLAYING" => {
                                        match pause_media(&soap_client, &av_control_url).await {
                                            Ok(()) => println!("Paused."),
                                            Err(e) => eprintln!("Pause failed: {}", e),
                                        }
                                    }
                                    Ok(s) => eprintln!("Cannot pause/resume from state: {}", s),
                                    Err(e) => eprintln!("Could not query state: {}", e),
                                }
                            }
                            "s" => {
                                stop_media(&soap_client, &av_control_url).await.ok();
                                poll_task.abort();
                                track_exit = TrackExit::StopPlaylist;
                                break 'control;
                            }
                            "f" => {
                                println!("Seek position (HH:MM:SS):");
                                if let Some(pos) = stdin_rx.recv().await {
                                    match seek_media(
                                        &soap_client,
                                        &av_control_url,
                                        pos.trim(),
                                    )
                                    .await
                                    {
                                        Ok(()) => println!("Seeked to {}.", pos.trim()),
                                        Err(e) => eprintln!("Seek failed: {}", e),
                                    }
                                }
                            }
                            "n" => {
                                stop_media(&soap_client, &av_control_url).await.ok();
                                poll_task.abort();
                                track_exit = TrackExit::NextTrack;
                                break 'control;
                            }
                            "q" => {
                                stop_media(&soap_client, &av_control_url).await.ok();
                                poll_task.abort();
                                track_exit = TrackExit::Quit;
                                break 'control;
                            }
                            _ => println!(
                                "Controls: [p] Pause/Resume  [s] Stop & menu  [n] Next  [f] Seek  [q] Quit"
                            ),
                        }
                    }
                    changed = poll_rx.changed() => {
                        if changed.is_ok() {
                            match poll_rx.borrow().clone() {
                                PollSignal::Paused => println!("[TV] Paused."),
                                PollSignal::Resumed => println!("[TV] Resumed."),
                                PollSignal::Stopped => {
                                    println!("Playback finished.");
                                    poll_task.abort();
                                    track_exit = TrackExit::NextTrack;
                                    break 'control;
                                }
                                PollSignal::DeviceOffline => {
                                    println!("\nDevice went offline. Attempting rediscovery...");
                                    poll_task.abort();

                                    let new_loc = if let Some(ref usn) = selected_usn {
                                        rediscover_by_usn(
                                            &config.multicast_address,
                                            config.multicast_port,
                                            usn,
                                        )
                                        .await
                                        .and_then(|d| d.get("LOCATION").cloned())
                                    } else {
                                        None
                                    };

                                    match new_loc {
                                        Some(loc) => {
                                            match fetch_device_description_quiet(&loc).await {
                                                Ok(desc) => {
                                                    let base = extract_base_url(&loc);
                                                    av_control_url = find_control_url(
                                                        &desc,
                                                        "AVTransport",
                                                        &base,
                                                    )
                                                    .unwrap_or_else(|| {
                                                        format!(
                                                            "{}/upnp/control/AVTransport1",
                                                            base
                                                        )
                                                    });
                                                    cm_control_url = find_control_url(
                                                        &desc,
                                                        "ConnectionManager",
                                                        &base,
                                                    )
                                                    .unwrap_or_else(|| {
                                                        format!(
                                                            "{}/upnp/control/ConnectionManager1",
                                                            base
                                                        )
                                                    });
                                                    println!("Device rediscovered. Restarting track...");
                                                    track_exit = TrackExit::NextTrack;
                                                }
                                                Err(e) => {
                                                    eprintln!("Rediscovery failed: {}", e);
                                                    track_exit = TrackExit::Quit;
                                                }
                                            }
                                        }
                                        None => {
                                            eprintln!("Device not found after rediscovery.");
                                            track_exit = TrackExit::Quit;
                                        }
                                    }
                                    break 'control;
                                }
                                PollSignal::Running => {}
                            }
                        }
                    }
                }
            }

            // Decide what to do after the control loop exits
            match track_exit {
                TrackExit::NextTrack => {} // continue 'playlist
                TrackExit::StopPlaylist | TrackExit::Quit => break 'playlist,
            }
        }

        // ── post-playlist ─────────────────────────────────────────────────────
        if matches!(track_exit, TrackExit::Quit) {
            break 'session;
        }

        match ask_what_next(&mut stdin_rx).await {
            SessionAction::ReselectMedia => { /* need_device stays false */ }
            SessionAction::ReselectDevice => need_device = true,
            SessionAction::Rescan => need_device = true,
            SessionAction::Quit => break 'session,
        }
    }

    // ── shutdown ──────────────────────────────────────────────────────────────
    send_notify_byebye(&config).await;
    println!("Shutting down...");
    advertiser_task.abort();
    server_task.abort();
    Ok(())
}
