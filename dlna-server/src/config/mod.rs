// src/config/mod.rs
use dotenvy::dotenv;
use std::env;

pub struct Config {
    pub http_port: u16,
    pub friendly_name: String,
    pub multicast_address: String,
    pub multicast_port: u16,
}

impl Config {
    pub fn from_env() -> Self {
        dotenv().ok();

        Config {
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
        }
    }
}