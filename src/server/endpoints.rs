use hyper::{body::Bytes, Body, Request, Response, StatusCode};
use serde_json::json;
use std::io::SeekFrom;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::config::Config;
use crate::media::manager::list_media_files;
use crate::media::stream::get_mime_type;

/// Size of each chunk read from disk and sent over the network.
/// 256 KB balances disk I/O efficiency with renderer buffer granularity
/// and fits within a typical TCP send buffer.
const STREAM_CHUNK_SIZE: usize = 256 * 1024;

/// Number of pre-read chunks buffered between the disk reader and network
/// sender tasks. At 256 KB per chunk, 8 slots = 2 MB per stream — enough
/// to absorb disk latency spikes without wasting memory.
const READ_AHEAD_SLOTS: usize = 8;

/// DLNA content features header value.
/// DLNA.ORG_OP=01 — byte-range seeks supported, time-seek not supported.
/// DLNA.ORG_FLAGS bits: streaming-transfer-mode + interactive + background.
const DLNA_CONTENT_FEATURES: &str =
    "DLNA.ORG_OP=01;DLNA.ORG_FLAGS=01700000000000000000000000000000";

const SERVER_HEADER: &str = "RustCast/0.1 DLNA/1.5 UPnP/1.0";

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
        .header("EXT", "")
        .header("Server", SERVER_HEADER)
        .body(Body::from(xml))
        .unwrap())
}

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
    let (start_str, end_str) = stripped.split_once('-')?;

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

/// Serves a media file with full Range support and smooth streaming.
///
/// Architecture: two decoupled tasks connected by a bounded channel.
///
///   [disk reader task] --Bytes--> [mpsc channel (8 slots)] --> [sender task] --> [hyper body]
///
/// The disk reader stays up to READ_AHEAD_SLOTS chunks ahead of the network.
/// When the renderer disconnects (e.g. after a seek), the body sender fails,
/// the sender task drops the channel receiver, and the disk reader's next send
/// returns an error — both tasks clean up automatically without explicit cancellation.
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

    let metadata = match tokio::fs::metadata(&canonical).await {
        Ok(m) => m,
        Err(_) => return respond_internal_server_error("Error reading file metadata"),
    };
    let file_size = metadata.len();

    let mime_type = get_mime_type(canonical.to_str().unwrap_or(""));

    let range_header = req
        .headers()
        .get("range")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

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

    let content_length = end - start + 1;

    let mut file = match tokio::fs::File::open(&canonical).await {
        Ok(f) => f,
        Err(_) => return respond_internal_server_error("Error opening file"),
    };
    if start > 0 && file.seek(SeekFrom::Start(start)).await.is_err() {
        return respond_internal_server_error("Error seeking file");
    }

    // Bounded channel between disk reader and network sender.
    // Capacity = READ_AHEAD_SLOTS: disk reader stalls naturally when the channel
    // is full, preventing unbounded memory use.
    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<Bytes>(READ_AHEAD_SLOTS);

    // Disk reader task: reads STREAM_CHUNK_SIZE at a time and queues chunks.
    // Exits when EOF is reached, on a read error, or when chunk_tx.send fails
    // (meaning the sender task already exited due to a client disconnect).
    tokio::spawn(async move {
        let mut remaining = content_length;
        let mut buf = vec![0u8; STREAM_CHUNK_SIZE];
        while remaining > 0 {
            let to_read = (STREAM_CHUNK_SIZE as u64).min(remaining) as usize;
            match file.read(&mut buf[..to_read]).await {
                Ok(0) => break,
                Ok(n) => {
                    remaining -= n as u64;
                    let chunk = Bytes::copy_from_slice(&buf[..n]);
                    if chunk_tx.send(chunk).await.is_err() {
                        break; // sender task gone — client disconnected
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Network sender task: drains the channel and writes to the hyper body.
    // When send_data fails (client disconnected), drops chunk_rx which causes
    // the disk reader's next send to fail, stopping it immediately.
    let (mut body_sender, body) = Body::channel();
    tokio::spawn(async move {
        while let Some(chunk) = chunk_rx.recv().await {
            if body_sender.send_data(chunk).await.is_err() {
                break; // client disconnected
            }
        }
        // body_sender drops here, signalling end-of-body to hyper
    });

    // Build and return the response immediately — headers go out before the
    // first byte of the file has been read, so the renderer can start buffering.
    let response = Response::builder()
        .status(if is_partial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header("Content-Type", mime_type)
        .header("Content-Length", content_length.to_string())
        .header("Accept-Ranges", "bytes")
        .header("transferMode.dlna.org", "Streaming")
        .header("contentFeatures.dlna.org", DLNA_CONTENT_FEATURES)
        .header("EXT", "")
        .header("Server", SERVER_HEADER)
        .header(
            "Content-Disposition",
            format!("inline; filename=\"{}\"", media_name),
        );

    let response = if is_partial {
        response.header(
            "Content-Range",
            format!("bytes {}-{}/{}", start, end, file_size),
        )
    } else {
        response
    };

    Ok(response.body(body).unwrap())
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
