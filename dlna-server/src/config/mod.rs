// src/config/mod.rs
use dotenvy::dotenv;
use std::env;
use uuid::Uuid;

#[derive(Clone)]
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
    let socket =
        std::net::UdpSocket::bind("0.0.0.0:0").expect("Failed to bind UDP socket for IP detection");
    socket
        .connect("8.8.8.8:80")
        .expect("Failed to connect UDP socket for IP detection");
    socket
        .local_addr()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

impl Config {
    pub fn from_env() -> Self {
        dotenv().ok();

        Config {
            http_address: detect_local_ip(),
            http_port: env::var("HTTP_PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .expect("HTTP_PORT must be a valid number"),
            friendly_name: env::var("DLNA_FRIENDLY_NAME")
                .unwrap_or_else(|_| "Rust DLNA Server".to_string()),
            multicast_address: env::var("MULTICAST_ADDRESS")
                .unwrap_or_else(|_| "239.255.255.250".to_string()),
            multicast_port: env::var("MULTICAST_PORT")
                .unwrap_or_else(|_| "1900".to_string())
                .parse()
                .expect("MULTICAST_PORT must be a valid number"),
            media_directory: env::var("MEDIA_DIRECTORY").unwrap_or_else(|_| "./media".to_string()),
            udn: env::var("UDN").unwrap_or_else(|_| format!("uuid:{}", Uuid::new_v4())),
        }
    }
}
