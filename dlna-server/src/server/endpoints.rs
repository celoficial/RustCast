use hyper::{Body, Request, Response, StatusCode};
use serde_json::json;
use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::media::manager::list_media_files;

pub async fn handle_request(
    req: Request<Body>,
    config: &Config,
) -> Result<Response<Body>, hyper::Error> {
    let uri_path = req.uri().path();

    match uri_path {
        "/description.xml" => handle_description_request(),
        "/media" => handle_media_list_request(config),
        _ => {
            if let Some(media_name) = uri_path.strip_prefix("/media/") {
                handle_media_file_request(media_name, config)
            } else {
                respond_not_found()
            }
        }
    }
}

/// Handles the `/description.xml` endpoint
fn handle_description_request() -> Result<Response<Body>, hyper::Error> {
    let xml = r#"<?xml version="1.0"?>
    <root xmlns="urn:schemas-upnp-org:device-1-0">
        <specVersion>
            <major>1</major>
            <minor>0</minor>
        </specVersion>
        <device>
            <deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>
            <friendlyName>RustCast Server</friendlyName>
            <manufacturer>Understake</manufacturer>
            <manufacturerURL>https://github.com/celoficial</manufacturerURL>
            <modelName>DLNA Server v1</modelName>
            <modelDescription>A Rust-based DLNA Media Server</modelDescription>
            <modelURL>https://github.com/celoficial/RustCast</modelURL>
            <UDN>uuid:12345678-1234-1234-1234-123456789abc</UDN>
        </device>
    </root>"#;

    Ok(Response::new(Body::from(xml)))
}

/// Handles the `/media` endpoint
fn handle_media_list_request(config: &Config) -> Result<Response<Body>, hyper::Error> {
    let media_files = list_media_files(&config.media_directory);
    let json = json!(media_files);

    Ok(Response::new(Body::from(json.to_string())))
}

/// Handles the `/media/{media_name}` endpoint
fn handle_media_file_request(
    media_name: &str,
    config: &Config,
) -> Result<Response<Body>, hyper::Error> {
    let file_path = Path::new(&config.media_directory).join(media_name);

    // Replace \\ with / in file_path
    let file_path2 = file_path.display().to_string().replace("\\", "/");
    let file_path = Path::new(&file_path2);

    if file_path.exists() && file_path.is_file() {
        // Determines the MIME type
        let mime_type = match file_path.extension().and_then(|ext| ext.to_str()) {
            Some("mp4") => "video/mp4",
            Some("mkv") => "video/x-matroska",
            Some("avi") => "video/x-msvideo",
            Some("mp3") => "audio/mpeg",
            Some("srt") => "application/x-subrip",
            _ => "application/octet-stream",
        };

        // Sends the file as a response
        match fs::read(&file_path) {
            Ok(content) => {
                Ok(Response::builder()
                .header("Content-Type", mime_type)
                .header(
                    "Content-Disposition",
                    format!("inline; filename=\"{}\"", media_name),
                )
                .body(Body::from(content))
                .unwrap())
            },
            Err(_) => respond_internal_server_error("Error reading the file"),
        }
    } else {
        println!("File not found: {:?}", file_path);
        respond_not_found()
    }
}

/// Responds with 404 - Not Found
fn respond_not_found() -> Result<Response<Body>, hyper::Error> {
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("Not Found"))
        .unwrap())
}

/// Responds with 500 - Internal Server Error
fn respond_internal_server_error(
    message: &str,
) -> Result<Response<Body>, hyper::Error> {
    Ok(Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::from(message.to_string())) // Converts to String
        .unwrap())
}
