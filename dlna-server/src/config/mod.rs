// src/config/mod.rs
use dotenvy::dotenv;
use std::env;

#[derive(Clone)]
pub struct Config {
    pub http_address: String,
    pub http_port: u16,
    pub friendly_name: String,
    pub multicast_address: String,
    pub multicast_port: u16,
    pub media_directory: String,
}

impl Config {
    pub fn from_env() -> Self {
        dotenv().expect("Erro ao carregar o Arquivo .env");

        Config {
            http_address: env::var("HTTP_ADDRESS")
                .unwrap_or_else(|_| "localhost".to_string())
                .parse()
                .expect("HTTP_ADDRESS must be a valid address"),
            http_port: env::var("HTTP_PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .expect("HTTP_PORT must be a valid number"),
            friendly_name: env::var("DLNA_FRIENDLY_NAME")
                .unwrap_or_else(|_| "Rust DLNA Server".to_string())
                .parse()
                .expect("DLNA_FRIENDLY_NAME must be a valid string"),
            multicast_address: env::var("MULTICAST_ADDRESS")
                .unwrap_or_else(|_| "239.255.255.250".to_string())
                .parse()
                .expect("MULTICAST_ADDRESS must be a valid address"),
            multicast_port: env::var("MULTICAST_PORT")
                .unwrap_or_else(|_| "1900".to_string())
                .parse()
                .expect("MULTICAST_PORT must be a valid number"),
            media_directory: env::var("MEDIA_DIRECTORY")
                .unwrap_or_else(|_| "./media".to_string())
                .parse()
                .expect("MEDIA_DIRECTORY must be a valid path"),
        }
    }
}
