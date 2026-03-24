use std::path::Path;

mod config;
mod discovery;
mod dlna;
mod media;
mod server;
mod soap;
mod tui;

use config::Config;
use discovery::advertise::{send_notify_byebye, start_ssdp_advertiser};
use media::manager::list_media_files;
use server::http_server::start_http_server;
use soap::new_soap_client;
use tui::TerminalGuard;

// ── main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env().unwrap_or_else(|e| {
        eprintln!("Configuration error: {}", e);
        std::process::exit(1);
    });

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

    let soap_client = new_soap_client();

    // Restore terminal on panic
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        crossterm::terminal::disable_raw_mode().ok();
        crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen).ok();
        hook(info);
    }));

    let media_files = list_media_files(&config.media_directory);

    let mut terminal = TerminalGuard::new()?;
    tui::run_app(&mut terminal, media_files, config.clone(), soap_client).await?;
    drop(terminal);

    // ── shutdown ──────────────────────────────────────────────────────────────
    send_notify_byebye(&config).await;
    advertiser_task.abort();
    server_task.abort();
    Ok(())
}
