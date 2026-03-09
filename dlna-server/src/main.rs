use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};

mod config;
mod discovery;
mod server;
mod media;

use config::Config;
use discovery::discovery::discover_ssdp;
use discovery::device::{extract_base_url, fetch_device_description, find_control_url};
use server::http_server::start_http_server;
use media::manager::list_media_files;
use media::stream::stream_media;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env();
    println!("Starting the {} server", config.friendly_name);

    // Checks if the media directory exists
    if !Path::new(&config.media_directory).exists() {
        eprintln!("Error: The configured media directory '{}' does not exist.", config.media_directory);
        return Err("Invalid media directory".into());
    }

    // Clones the configuration for use in the HTTP server
    let server_config = config.clone();

    // Starts the HTTP server in a separate task — save handle for proper shutdown
    let server_task = tokio::spawn(async move {
        start_http_server(server_config.http_port, server_config).await;
    });

    // Async stdin reader
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    // Calls the device discovery function
    match discover_ssdp(&config.multicast_address, config.multicast_port).await {
        Ok(devices) => {
            if devices.is_empty() {
                println!("No devices found.");
            } else {
                println!("\nMediaRenderer devices found:");
                for (i, device) in devices.iter().enumerate() {
                    if let Some(location) = device.get("LOCATION") {
                        println!("{}) {}", i + 1, location);
                    } else {
                        println!("{}) Device without LOCATION.", i + 1);
                    }
                }

                println!("\nChoose a device by number (or type '0' to exit):");
                let input = lines.next_line().await?.unwrap_or_default();
                let device_choice: usize = input.trim().parse().unwrap_or(0);

                if device_choice == 0 {
                    println!("Exiting...");
                    server_task.abort();
                    return Ok(());
                }

                let selected_location = devices
                    .get(device_choice - 1)
                    .and_then(|device| device.get("LOCATION"))
                    .ok_or("Invalid device or missing LOCATION.")?
                    .clone();

                // Fetch device description and extract service control URLs
                let description = match fetch_device_description(&selected_location).await {
                    Ok(desc) => desc,
                    Err(e) => {
                        eprintln!("Error fetching device description: {}", e);
                        server_task.abort();
                        return Ok(());
                    }
                };

                let base_url = extract_base_url(&selected_location);

                let av_control_url = find_control_url(&description, "AVTransport", &base_url)
                    .unwrap_or_else(|| format!("{}/upnp/control/AVTransport1", base_url));

                let cm_control_url = find_control_url(&description, "ConnectionManager", &base_url)
                    .unwrap_or_else(|| format!("{}/upnp/control/ConnectionManager1", base_url));

                println!("You selected the device: {}", selected_location);
                println!("AVTransport URL: {}", av_control_url);
                println!("ConnectionManager URL: {}", cm_control_url);

                // Lists media files
                let media_files = list_media_files(&config.media_directory);

                if media_files.is_empty() {
                    println!("No media files found in the directory: {}", config.media_directory);
                    server_task.abort();
                    return Ok(());
                }

                println!("\nMedia files found:");
                for (i, file) in media_files.iter().enumerate() {
                    println!("{}) {}", i + 1, file.name);
                }

                println!("\nChoose a media file by number (or type '0' to exit):");
                let input = lines.next_line().await?.unwrap_or_default();
                let media_choice: usize = input.trim().parse().unwrap_or(0);

                if media_choice == 0 {
                    println!("Exiting...");
                    server_task.abort();
                    return Ok(());
                }

                let selected_media = media_files
                    .get(media_choice - 1)
                    .ok_or("Invalid media file.")?;

                println!("You selected the media file: {}", selected_media.name);
                println!("Starting streaming to the device...");

                stream_media(&config, &av_control_url, &cm_control_url, selected_media).await?;

                println!("Streaming started successfully!");
            }
        }
        Err(e) => {
            eprintln!("Error discovering SSDP devices: {}", e);
        }
    }

    // Wait for Ctrl+C before shutting down the HTTP server
    println!("Press Ctrl+C to stop the server...");
    tokio::signal::ctrl_c().await?;
    println!("Shutting down the HTTP server...");
    server_task.abort();

    Ok(())
}
