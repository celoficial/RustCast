use hyper::{Body, Request, Response, StatusCode};
use serde_json::json;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use std::io::SeekFrom;

use crate::config::Config;
use crate::media::manager::list_media_files;
use crate::media::stream::get_mime_type;

pub async fn handle_request(
    req: Request<Body>,
    config: &Config,
) -> Result<Response<Body>, hyper::Error> {
    let uri_path = req.uri().path().to_string();

    match uri_path.as_str() {
        "/description.xml" => handle_description_request(config),
        "/media" => handle_media_list_request(config),
        _ => {
            if let Some(media_name) = uri_path.strip_prefix("/media/") {
                let media_name = media_name.to_string();
                handle_media_file_request(&req, &media_name, config).await
            } else {
                respond_not_found()
            }
        }
    }
}

/// Handles the `/description.xml` endpoint
fn handle_description_request(config: &Config) -> Result<Response<Body>, hyper::Error> {
    let xml = format!(
        r#"<?xml version="1.0"?>
    <root xmlns="urn:schemas-upnp-org:device-1-0">
        <specVersion>
            <major>1</major>
            <minor>0</minor>
        </specVersion>
        <device>
            <deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>
            <friendlyName>{}</friendlyName>
            <manufacturer>Understake</manufacturer>
            <manufacturerURL>https://github.com/celoficial</manufacturerURL>
            <modelName>DLNA Server v1</modelName>
            <modelDescription>A Rust-based DLNA Media Server</modelDescription>
            <modelURL>https://github.com/celoficial/RustCast</modelURL>
            <UDN>{}</UDN>
        </device>
    </root>"#,
        config.friendly_name, config.udn
    );

    Ok(Response::builder()
        .header("Content-Type", "text/xml; charset=utf-8")
        .body(Body::from(xml))
        .unwrap())
}

/// Handles the `/media` endpoint
fn handle_media_list_request(config: &Config) -> Result<Response<Body>, hyper::Error> {
    let media_files = list_media_files(&config.media_directory);
    let json = json!(media_files);

    Ok(Response::builder()
        .header("Content-Type", "application/json")
        .body(Body::from(json.to_string()))
        .unwrap())
}

/// Parses a Range header value like "bytes=X-Y" or "bytes=X-".
/// Returns (start, end) clamped to [0, file_size-1], or None if invalid/unsatisfiable.
fn parse_range(range_str: &str, file_size: u64) -> Option<(u64, u64)> {
    let stripped = range_str.strip_prefix("bytes=")?;
    let mut parts = stripped.splitn(2, '-');
    let start_str = parts.next()?;
    let end_str = parts.next()?;

    let (start, end) = if start_str.is_empty() {
        // Suffix range: bytes=-N → last N bytes
        let suffix: u64 = end_str.parse().ok()?;
        let start = file_size.saturating_sub(suffix);
        (start, file_size - 1)
    } else {
        let start: u64 = start_str.parse().ok()?;
        let end: u64 = if end_str.is_empty() {
            file_size - 1
        } else {
            end_str.parse::<u64>().ok()?.min(file_size - 1)
        };
        (start, end)
    };

    if start > end || file_size == 0 {
        return None;
    }

    Some((start, end))
}

/// Handles the `/media/{media_name}` endpoint with Range support.
async fn handle_media_file_request(
    req: &Request<Body>,
    media_name: &str,
    config: &Config,
) -> Result<Response<Body>, hyper::Error> {
    // Security: canonicalize both paths and verify the file is within the media directory
    let base_canonical = match std::fs::canonicalize(&config.media_directory) {
        Ok(p) => p,
        Err(_) => return respond_not_found(),
    };

    let raw_path = Path::new(&config.media_directory).join(media_name);
    let canonical = match std::fs::canonicalize(&raw_path) {
        Ok(p) => p,
        Err(_) => {
            println!("File not found or path error: {:?}", raw_path);
            return respond_not_found();
        }
    };

    if !canonical.starts_with(&base_canonical) {
        println!("Path traversal attempt blocked: {:?}", canonical);
        return respond_bad_request();
    }

    if !canonical.is_file() {
        return respond_not_found();
    }

    // Get file size
    let metadata = match tokio::fs::metadata(&canonical).await {
        Ok(m) => m,
        Err(_) => return respond_internal_server_error("Error reading file metadata"),
    };
    let file_size = metadata.len();

    let mime_type = get_mime_type(canonical.to_str().unwrap_or(""));

    // Parse Range header
    let range_header = req.headers().get("range").and_then(|v| v.to_str().ok()).map(str::to_string);

    let (start, end, is_partial) = if let Some(ref range_str) = range_header {
        match parse_range(range_str, file_size) {
            Some((s, e)) => (s, e, true),
            None => {
                return Ok(Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header("Content-Range", format!("bytes */{}", file_size))
                    .body(Body::empty())
                    .unwrap());
            }
        }
    } else {
        (0, file_size.saturating_sub(1), false)
    };

    // Read the requested byte range from the file
    let content = match read_file_range(&canonical, start, end).await {
        Ok(bytes) => bytes,
        Err(_) => return respond_internal_server_error("Error reading the file"),
    };

    let content_length = content.len();

    if is_partial {
        Ok(Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header("Content-Type", mime_type)
            .header("Content-Length", content_length.to_string())
            .header("Content-Range", format!("bytes {}-{}/{}", start, end, file_size))
            .header("Accept-Ranges", "bytes")
            .header("Content-Disposition", format!("inline; filename=\"{}\"", media_name))
            .body(Body::from(content))
            .unwrap())
    } else {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", mime_type)
            .header("Content-Length", content_length.to_string())
            .header("Accept-Ranges", "bytes")
            .header("Content-Disposition", format!("inline; filename=\"{}\"", media_name))
            .body(Body::from(content))
            .unwrap())
    }
}

async fn read_file_range(path: &Path, start: u64, end: u64) -> std::io::Result<Vec<u8>> {
    let mut file = tokio::fs::File::open(path).await?;
    file.seek(SeekFrom::Start(start)).await?;
    let length = (end - start + 1) as usize;
    let mut buf = Vec::with_capacity(length);
    file.take(length as u64).read_to_end(&mut buf).await?;
    Ok(buf)
}

fn respond_not_found() -> Result<Response<Body>, hyper::Error> {
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("Not Found"))
        .unwrap())
}

fn respond_bad_request() -> Result<Response<Body>, hyper::Error> {
    Ok(Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(Body::from("Bad Request"))
        .unwrap())
}

fn respond_internal_server_error(message: &str) -> Result<Response<Body>, hyper::Error> {
    Ok(Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::from(message.to_string()))
        .unwrap())
}
