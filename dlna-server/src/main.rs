use std::path::Path;
use std::io::stdin;

mod config;
mod discovery;
mod server;
mod media;

use config::Config;
use discovery::discovery::discover_ssdp;
use discovery::device::fetch_device_description;
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

    // Starts the HTTP server in a separate thread
    tokio::spawn(async move {
        start_http_server(server_config.http_port, server_config).await;
    });

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

                // Asks the user which device they want to use
                println!("\nChoose a device by number (or type '0' to exit):");

                let mut input = String::new();
                stdin().read_line(&mut input)?;
                let device_choice: usize = input.trim().parse().unwrap_or(0);

                if device_choice == 0 {
                    println!("Exiting...");
                    return Ok(());
                }

                let selected_device = devices
                    .get(device_choice - 1)
                    .and_then(|device| device.get("LOCATION"))
                    .ok_or("Invalid device or missing LOCATION.")?;

                // Fetches the description of the selected device
                if let Err(e) = fetch_device_description(selected_device).await {
                    eprintln!("Error fetching device description: {}", e);
                    return Ok(());
                }

                println!("You selected the device: {}", selected_device);

                // Lists media files
                let media_files = list_media_files(&config.media_directory);
                
                if media_files.is_empty() {
                    println!("No media files found in the directory: {}", config.media_directory);
                    return Ok(());
                } else {
                    println!("\nMedia files found:");
                    for (i, file) in media_files.iter().enumerate() {
                        println!("{}) {}", i + 1, file.name);
                    }
                    // Asks the user which media file they want to stream
                    println!("\nChoose a media file by number (or type '0' to exit):");
                }

                input.clear();
                stdin().read_line(&mut input)?;
                let media_choice: usize = input.trim().parse().unwrap_or(0);

                if media_choice == 0 {
                    println!("Exiting...");
                    return Ok(());
                }

                let selected_media = media_files
                .get(media_choice - 1)
                .ok_or("Invalid media file.")?;

                println!("You selected the media file: {}", selected_media.name);

                // Starts streaming to the DLNA device
                println!("Starting streaming to the device: {}", selected_device);

                let selected_device_cleaned = selected_device.trim_end_matches("/dmr").to_string();
                stream_media(&selected_device_cleaned, selected_media).await?;

                println!("Streaming completed successfully!");
            }
        }
        Err(e) => {
            println!("Error discovering SSDP devices: {}", e);
        }
    }

    // Waits for the program to terminate (Ctrl+C to exit)
    tokio::signal::ctrl_c().await?;
    println!("Shutting down the HTTP server...");

    // Cancels the HTTP server task
    //server_task.abort();

    Ok(())
}
