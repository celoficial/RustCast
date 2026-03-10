use dotenvy::dotenv;
use std::env;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Config {
    pub http_address: String,
    pub http_port: u16,
    pub friendly_name: String,
    pub multicast_address: String,
    pub multicast_port: u16,
    pub media_directory: String,
    pub udn: String,
}

/// Detects the machine's outbound LAN IP by opening a UDP socket and checking
/// which local interface the OS would use to reach an external address.
/// No packet is actually sent.
fn detect_local_ip() -> String {
    let result = (|| -> Option<String> {
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.connect("8.8.8.8:80").ok()?;
        socket.local_addr().ok().map(|a| a.ip().to_string())
    })();

    match result {
        Some(ip) => ip,
        None => {
            eprintln!(
                "Warning: could not detect local LAN IP — falling back to 127.0.0.1. \
                DLNA renderers on other devices will not be able to reach media files. \
                Set the HTTP_ADDRESS environment variable to override."
            );
            "127.0.0.1".to_string()
        }
    }
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        dotenv().ok();

        let http_port: u16 = env::var("HTTP_PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse()
            .map_err(|_| "HTTP_PORT must be a number between 1 and 65535".to_string())?;

        if http_port == 0 {
            return Err("HTTP_PORT cannot be 0".to_string());
        }

        let multicast_port: u16 = env::var("MULTICAST_PORT")
            .unwrap_or_else(|_| "1900".to_string())
            .parse()
            .map_err(|_| "MULTICAST_PORT must be a number between 1 and 65535".to_string())?;

        let media_directory = env::var("MEDIA_DIRECTORY").unwrap_or_else(|_| "./media".to_string());
        if media_directory.trim().is_empty() {
            return Err("MEDIA_DIRECTORY cannot be empty".to_string());
        }

        let http_address = env::var("HTTP_ADDRESS").unwrap_or_else(|_| detect_local_ip());

        Ok(Config {
            http_address,
            http_port,
            friendly_name: env::var("DLNA_FRIENDLY_NAME")
                .unwrap_or_else(|_| "Rust DLNA Server".to_string()),
            multicast_address: env::var("MULTICAST_ADDRESS")
                .unwrap_or_else(|_| "239.255.255.250".to_string()),
            multicast_port,
            media_directory,
            udn: env::var("UDN").unwrap_or_else(|_| format!("uuid:{}", Uuid::new_v4())),
        })
    }
}
