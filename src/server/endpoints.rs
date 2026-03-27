use futures::stream;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full, StreamBody};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::{header, Request, Response, StatusCode};
use serde_json::json;
use std::convert::Infallible;
use std::io::SeekFrom;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::config::Config;
use crate::media::manager::get_mime_type;
use crate::media::manager::list_media_files;

/// Size of each chunk read from disk and sent over the network.
/// 256 KB balances disk I/O efficiency with renderer buffer granularity
/// and fits within a typical TCP send buffer.
const STREAM_CHUNK_SIZE: usize = 256 * 1024;

/// Number of pre-read chunks buffered between the disk reader and response stream.
/// At 256 KB per chunk, 8 slots = 2 MB per stream, enough to absorb disk
/// latency spikes without wasting memory.
const READ_AHEAD_SLOTS: usize = 8;

/// DLNA content features header value.
/// DLNA.ORG_OP=01 means byte-range seeks supported, time-seek not supported.
/// DLNA.ORG_FLAGS bits advertise streaming-transfer-mode and related support.
const DLNA_CONTENT_FEATURES: &str =
    "DLNA.ORG_OP=01;DLNA.ORG_FLAGS=01700000000000000000000000000000";
const SERVER_HEADER: &str = "RustCast/0.1 DLNA/1.5 UPnP/1.0";

type ResponseBody = BoxBody<Bytes, Infallible>;

fn full_body(body: impl Into<Bytes>) -> ResponseBody {
    Full::new(body.into()).boxed()
}

fn empty_body() -> ResponseBody {
    Empty::<Bytes>::new().boxed()
}

pub async fn handle_request(
    req: Request<Incoming>,
    config: &Config,
) -> Result<Response<ResponseBody>, Infallible> {
    let uri_path = req.uri().path().to_string();

    let response = match uri_path.as_str() {
        "/description.xml" => handle_description_request(config),
        "/media" => handle_media_list_request(config),
        _ => {
            if let Some(media_name) = uri_path.strip_prefix("/media/") {
                handle_media_file_request(&req, media_name, config).await
            } else {
                respond_not_found()
            }
        }
    };

    Ok(response)
}

fn handle_description_request(config: &Config) -> Response<ResponseBody> {
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

    Response::builder()
        .header("Content-Type", "text/xml; charset=utf-8")
        .header("EXT", "")
        .header("Server", SERVER_HEADER)
        .body(full_body(xml))
        .unwrap()
}

fn handle_media_list_request(config: &Config) -> Response<ResponseBody> {
    let media_files = list_media_files(&config.media_directory);
    let json = json!(media_files);

    Response::builder()
        .header("Content-Type", "application/json")
        .body(full_body(json.to_string()))
        .unwrap()
}

/// Parses a Range header like "bytes=X-Y" or "bytes=X-".
/// Returns (start, end) clamped to [0, file_size - 1], or None if invalid.
fn parse_range(range_str: &str, file_size: u64) -> Option<(u64, u64)> {
    if file_size == 0 {
        return None;
    }

    let stripped = range_str.strip_prefix("bytes=")?;
    let (start_str, end_str) = stripped.split_once('-')?;

    let (start, end) = if start_str.is_empty() {
        // Suffix range: bytes=-N means the last N bytes.
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

    if start > end {
        return None;
    }

    Some((start, end))
}

/// Serves a media file with full Range support and smooth streaming.
///
/// Architecture: a disk-reader task feeds a bounded channel, and the HTTP body
/// pulls chunks from that channel as the client consumes them.
///
///   [disk reader task] --Bytes--> [mpsc channel] --> [StreamBody] --> [hyper]
///
/// The disk reader stays up to READ_AHEAD_SLOTS chunks ahead of the network.
/// When the client disconnects, the body is dropped, the channel closes, and
/// the disk reader stops on its next send attempt.
async fn handle_media_file_request(
    req: &Request<Incoming>,
    media_name: &str,
    config: &Config,
) -> Response<ResponseBody> {
    // Security: canonicalize both paths and verify the file is within
    // the configured media directory.
    let base_canonical = match std::fs::canonicalize(&config.media_directory) {
        Ok(path) => path,
        Err(_) => return respond_not_found(),
    };

    let raw_path = Path::new(&config.media_directory).join(media_name);
    let canonical = match std::fs::canonicalize(&raw_path) {
        Ok(path) => path,
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
        Ok(metadata) => metadata,
        Err(_) => return respond_internal_server_error("Error reading file metadata"),
    };
    let file_size = metadata.len();

    let mime_type = get_mime_type(canonical.to_str().unwrap_or(""));

    let range_header: Option<String> = req
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    let (start, end, is_partial) = if let Some(range_str) = range_header.as_deref() {
        match parse_range(range_str, file_size) {
            Some((start, end)) => (start, end, true),
            None => {
                return Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header("Content-Range", format!("bytes */{}", file_size))
                    .body(empty_body())
                    .unwrap();
            }
        }
    } else {
        (0, file_size.saturating_sub(1), false)
    };

    let content_length = end.saturating_sub(start).saturating_add(1);

    let mut file = match tokio::fs::File::open(&canonical).await {
        Ok(file) => file,
        Err(_) => return respond_internal_server_error("Error opening file"),
    };
    if start > 0 && file.seek(SeekFrom::Start(start)).await.is_err() {
        return respond_internal_server_error("Error seeking file");
    }

    // Bounded channel between disk reader and response stream.
    // When the stream slows down, the disk reader naturally back-pressures here.
    let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel::<Bytes>(READ_AHEAD_SLOTS);

    // Disk reader task: reads STREAM_CHUNK_SIZE at a time and queues chunks.
    // Exits on EOF, read error, or when the HTTP body has been dropped.
    tokio::spawn(async move {
        let mut remaining = content_length;
        let mut buf = vec![0u8; STREAM_CHUNK_SIZE];
        while remaining > 0 {
            let to_read = (STREAM_CHUNK_SIZE as u64).min(remaining) as usize;
            match file.read(&mut buf[..to_read]).await {
                Ok(0) => break,
                Ok(read) => {
                    remaining -= read as u64;
                    let chunk = Bytes::copy_from_slice(&buf[..read]);
                    if chunk_tx.send(chunk).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // StreamBody turns the receiver into a hyper-compatible response body.
    // Headers can be returned immediately while file bytes are produced lazily.
    let body_stream = stream::unfold(chunk_rx, |mut chunk_rx| async move {
        chunk_rx
            .recv()
            .await
            .map(|chunk| (Ok::<Frame<Bytes>, Infallible>(Frame::data(chunk)), chunk_rx))
    });
    let body = StreamBody::new(body_stream).boxed();

    // Build and return the response immediately so the renderer can start
    // buffering before the whole file is read from disk.
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

    response.body(body).unwrap()
}

fn respond_not_found() -> Response<ResponseBody> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(full_body("Not Found"))
        .unwrap()
}

fn respond_bad_request() -> Response<ResponseBody> {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(full_body("Bad Request"))
        .unwrap()
}

fn respond_internal_server_error(message: &str) -> Response<ResponseBody> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(full_body(message.to_owned()))
        .unwrap()
}
