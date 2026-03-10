use futures::future::join_all;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};

mod config;
mod discovery;
mod dlna;
mod media;
mod server;
mod soap;
mod tui;

use config::Config;
use discovery::advertise::{send_notify_byebye, start_ssdp_advertiser};
use discovery::device::{
    extract_base_url, fetch_device_description, find_control_url_with_fallback, reconnect_device,
};
use discovery::health::{spawn_poll_task, PollSignal};
use discovery::ssdp::discover_ssdp;
use discovery::ssdp::SsdpDevice;
use media::finder::find_subtitle;
use media::manager::{list_media_files, MediaFile};
use media::stream::stream_media;
use server::http_server::start_http_server;
use soap::new_soap_client;
use tui::{ask_what_next, parse_selection, SessionAction, TrackExit};

// ── main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env().unwrap_or_else(|e| {
        eprintln!("Configuration error: {}", e);
        std::process::exit(1);
    });
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
    let mut selected_usn = String::new();

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
                    let loc = d.location.clone();
                    async move { fetch_device_description(&loc).await }
                });
                let desc_results = join_all(desc_futures).await;
                break (devices, desc_results);
            };

            println!("\nDevices found:");
            for (i, (device, desc)) in devices.iter().zip(desc_results.iter()).enumerate() {
                let name = match desc {
                    Ok(d) if !d.device.friendly_name.is_empty() => d.device.friendly_name.as_str(),
                    _ => device.location.as_str(),
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

            let selected_device: &SsdpDevice = &devices[device_choice - 1];
            selected_usn = selected_device.usn.clone();

            let description = match desc_results.into_iter().nth(device_choice - 1) {
                Some(Ok(desc)) => desc,
                _ => match fetch_device_description(&selected_device.location).await {
                    Ok(desc) => desc,
                    Err(e) => {
                        eprintln!("Error fetching device info: {} — rescanning.", e);
                        continue 'session;
                    }
                },
            };

            let base_url = extract_base_url(&selected_device.location);
            av_control_url = find_control_url_with_fallback(
                &description,
                "AVTransport",
                &base_url,
                "/upnp/control/AVTransport1",
            );
            cm_control_url = find_control_url_with_fallback(
                &description,
                "ConnectionManager",
                &base_url,
                "/upnp/control/ConnectionManager1",
            );

            let display_name = if description.device.friendly_name.is_empty() {
                selected_device.location.as_str()
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

            let (poll_task, mut poll_rx) =
                spawn_poll_task(soap_client.clone(), av_control_url.clone());

            // Control loop
            'control: loop {
                tokio::select! {
                    cmd = stdin_rx.recv() => {
                        let cmd = match cmd {
                            Some(c) => c,
                            None => {
                                dlna::av_transport::stop(&soap_client, &av_control_url).await.ok();
                                poll_task.abort();
                                track_exit = TrackExit::Quit;
                                break 'control;
                            }
                        };
                        match cmd.trim() {
                            "p" => {
                                match dlna::av_transport::get_transport_state(&soap_client, &av_control_url).await {
                                    Ok(s) if s == "PAUSED_PLAYBACK" => {
                                        match dlna::av_transport::play(&soap_client, &av_control_url).await {
                                            Ok(()) => println!("Resumed."),
                                            Err(e) => eprintln!("Resume failed: {}", e),
                                        }
                                    }
                                    Ok(s) if s == "PLAYING" => {
                                        match dlna::av_transport::pause(&soap_client, &av_control_url).await {
                                            Ok(()) => println!("Paused."),
                                            Err(e) => eprintln!("Pause failed: {}", e),
                                        }
                                    }
                                    Ok(s) => eprintln!("Cannot pause/resume from state: {}", s),
                                    Err(e) => eprintln!("Could not query state: {}", e),
                                }
                            }
                            "s" => {
                                dlna::av_transport::stop(&soap_client, &av_control_url).await.ok();
                                poll_task.abort();
                                track_exit = TrackExit::StopPlaylist;
                                break 'control;
                            }
                            "f" => {
                                println!("Seek position (HH:MM:SS):");
                                if let Some(pos) = stdin_rx.recv().await {
                                    match dlna::av_transport::seek(
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
                                dlna::av_transport::stop(&soap_client, &av_control_url).await.ok();
                                poll_task.abort();
                                track_exit = TrackExit::NextTrack;
                                break 'control;
                            }
                            "q" => {
                                dlna::av_transport::stop(&soap_client, &av_control_url).await.ok();
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

                                    match reconnect_device(
                                        &config.multicast_address,
                                        config.multicast_port,
                                        &selected_usn,
                                    )
                                    .await
                                    {
                                        Some((av, cm)) => {
                                            av_control_url = av;
                                            cm_control_url = cm;
                                            println!("Device rediscovered. Restarting track...");
                                            track_exit = TrackExit::NextTrack;
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
