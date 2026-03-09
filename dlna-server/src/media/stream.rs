use super::manager::MediaFile;
use crate::config::Config;
use hyper::{Body, Client, Method, Request};
use std::time::Duration;
use tokio::time::timeout;

const SOAP_TIMEOUT: Duration = Duration::from_secs(10);

/// Escapes special XML characters to prevent injection.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Sends the PrepareForConnection SOAP action to the device's ConnectionManager.
pub async fn prepare_connection(cm_control_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let soap_body = r#"
      <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
        <s:Body>
          <u:PrepareForConnection xmlns:u="urn:schemas-upnp-org:service:ConnectionManager:1">
            <RemoteProtocolInfo>http-get:*:video/mp4:DLNA.ORG_OP=01;DLNA.ORG_FLAGS=01700000000000000000000000000000</RemoteProtocolInfo>
            <PeerConnectionManager></PeerConnectionManager>
            <PeerConnectionID>0</PeerConnectionID>
            <Direction>Input</Direction>
          </u:PrepareForConnection>
        </s:Body>
      </s:Envelope>
    "#;

    let client = Client::new();

    let request = Request::builder()
        .method(Method::POST)
        .uri(cm_control_url)
        .header(
            "SOAPACTION",
            r#""urn:schemas-upnp-org:service:ConnectionManager:1#PrepareForConnection""#,
        )
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .body(Body::from(soap_body))?;

    let response = timeout(SOAP_TIMEOUT, client.request(request))
        .await
        .map_err(|_| "PrepareForConnection timed out")??;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to configure connection. Status: {}",
            response.status()
        )
        .into());
    }

    println!("Connection configured successfully!");
    Ok(())
}

/// Internal helper: sends AVTransport Play with Speed=1.
async fn send_play(av_control_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let soap_body = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
    <s:Body>
        <u:Play xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
            <InstanceID>0</InstanceID>
            <Speed>1</Speed>
        </u:Play>
    </s:Body>
</s:Envelope>"#;

    let client = hyper::Client::new();
    let request = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(av_control_url)
        .header("Content-Type", "text/xml; charset=utf-8")
        .header(
            "SOAPAction",
            "\"urn:schemas-upnp-org:service:AVTransport:1#Play\"",
        )
        .body(hyper::Body::from(soap_body))?;

    let response = timeout(SOAP_TIMEOUT, client.request(request))
        .await
        .map_err(|_| "Play command timed out")??;
    let status = response.status();
    if !status.is_success() {
        let body_bytes = hyper::body::to_bytes(response.into_body()).await?;
        return Err(format!(
            "Failed to start playback. Status: {}. Response: {}",
            status,
            String::from_utf8_lossy(&body_bytes)
        )
        .into());
    }
    Ok(())
}

/// Streams a media file to the DLNA device using the discovered control URLs.
/// If `subtitle_url` is provided, it is embedded in the DIDL-Lite metadata so
/// renderers that support external subtitles can load them automatically.
pub async fn stream_media(
    config: &Config,
    av_control_url: &str,
    cm_control_url: &str,
    media_file: &MediaFile,
    subtitle_url: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let file_path = std::path::Path::new(&media_file.path);

    if !file_path.exists() {
        return Err(format!("The file '{}' was not found.", media_file.path).into());
    }

    println!("Starting streaming of: {}", media_file.path);

    // URL that the TV will use to fetch the media — use just the filename
    let media_url = format!(
        "http://{}:{}/media/{}",
        config.http_address, config.http_port, media_file.name
    );

    // PrepareForConnection is optional in DLNA; many renderers don't support it.
    // Log failures but don't abort streaming.
    if let Err(e) = prepare_connection(cm_control_url).await {
        eprintln!("PrepareForConnection skipped (not supported by device): {}", e);
    }

    let client = hyper::Client::new();

    let mime_type = get_mime_type(&media_file.path);

    // DIDL-Lite metadata — escape all values to prevent XML injection
    let title_escaped = xml_escape(&media_file.name);
    let url_escaped = xml_escape(&media_url);
    let mime_escaped = xml_escape(mime_type);

    // Subtitle elements: standard DLNA <res> + Samsung sec: extension for broader compatibility
    let (sec_ns, subtitle_elements) = match subtitle_url {
        Some(url) => {
            let esc = xml_escape(url);
            (
                r#" xmlns:sec="http://www.sec.co.kr/""#,
                format!(
                    r#"<res protocolInfo="http-get:*:text/srt:*">{}</res><sec:CaptionInfoEx sec:type="srt">{}</sec:CaptionInfoEx>"#,
                    esc, esc
                ),
            )
        }
        None => ("", String::new()),
    };

    let current_uri_metadata = format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"{}><item id="0" parentID="-1" restricted="1"><dc:title>{}</dc:title><res protocolInfo="http-get:*:{}:DLNA.ORG_OP=01">{}</res>{}</item></DIDL-Lite>"#,
        sec_ns, title_escaped, mime_escaped, url_escaped, subtitle_elements
    );

    // The metadata must be XML-escaped when embedded inside the SOAP body
    let metadata_escaped = xml_escape(&current_uri_metadata);

    let soap_body_set_uri = format!(
        r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
    <s:Body>
        <u:SetAVTransportURI xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
            <InstanceID>0</InstanceID>
            <CurrentURI>{}</CurrentURI>
            <CurrentURIMetaData>{}</CurrentURIMetaData>
        </u:SetAVTransportURI>
    </s:Body>
</s:Envelope>"#,
        xml_escape(&media_url),
        metadata_escaped
    );

    println!("Sending SetAVTransportURI command to {}", av_control_url);

    let request_set_uri = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(av_control_url)
        .header("Content-Type", "text/xml; charset=utf-8")
        .header(
            "SOAPAction",
            "\"urn:schemas-upnp-org:service:AVTransport:1#SetAVTransportURI\"",
        )
        .body(hyper::Body::from(soap_body_set_uri))?;

    let response_set_uri = timeout(SOAP_TIMEOUT, client.request(request_set_uri))
        .await
        .map_err(|_| "SetAVTransportURI timed out")??;
    let status_set_uri = response_set_uri.status();
    if !status_set_uri.is_success() {
        let body_bytes = hyper::body::to_bytes(response_set_uri.into_body()).await?;
        return Err(format!(
            "Failed to configure transport. Status: {}. Response: {}",
            status_set_uri,
            String::from_utf8_lossy(&body_bytes)
        )
        .into());
    }

    println!("Media configured successfully! Sending Play command...");
    send_play(av_control_url).await?;
    println!("Playback started successfully!");

    Ok(())
}

/// Resumes playback on the DLNA device.
pub async fn resume_media(av_control_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    send_play(av_control_url).await
}

/// Pauses playback on the DLNA device.
pub async fn pause_media(av_control_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let soap_body = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
    <s:Body>
        <u:Pause xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
            <InstanceID>0</InstanceID>
        </u:Pause>
    </s:Body>
</s:Envelope>"#;

    let client = hyper::Client::new();
    let request = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(av_control_url)
        .header("Content-Type", "text/xml; charset=utf-8")
        .header(
            "SOAPAction",
            "\"urn:schemas-upnp-org:service:AVTransport:1#Pause\"",
        )
        .body(hyper::Body::from(soap_body))?;

    let response = client.request(request).await?;
    let status = response.status();
    if !status.is_success() {
        let body_bytes = hyper::body::to_bytes(response.into_body()).await?;
        return Err(format!(
            "Failed to pause. Status: {}. Response: {}",
            status,
            String::from_utf8_lossy(&body_bytes)
        )
        .into());
    }
    Ok(())
}

/// Stops playback on the DLNA device.
pub async fn stop_media(av_control_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let soap_body = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
    <s:Body>
        <u:Stop xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
            <InstanceID>0</InstanceID>
        </u:Stop>
    </s:Body>
</s:Envelope>"#;

    let client = hyper::Client::new();
    let request = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(av_control_url)
        .header("Content-Type", "text/xml; charset=utf-8")
        .header(
            "SOAPAction",
            "\"urn:schemas-upnp-org:service:AVTransport:1#Stop\"",
        )
        .body(hyper::Body::from(soap_body))?;

    let response = client.request(request).await?;
    let status = response.status();
    if !status.is_success() {
        let body_bytes = hyper::body::to_bytes(response.into_body()).await?;
        return Err(format!(
            "Failed to stop. Status: {}. Response: {}",
            status,
            String::from_utf8_lossy(&body_bytes)
        )
        .into());
    }
    Ok(())
}

/// Seeks to a position in the current media. Position format: "HH:MM:SS"
pub async fn seek_media(
    av_control_url: &str,
    position: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let soap_body = format!(
        r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
    <s:Body>
        <u:Seek xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
            <InstanceID>0</InstanceID>
            <Unit>REL_TIME</Unit>
            <Target>{}</Target>
        </u:Seek>
    </s:Body>
</s:Envelope>"#,
        xml_escape(position)
    );

    let client = hyper::Client::new();
    let request = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(av_control_url)
        .header("Content-Type", "text/xml; charset=utf-8")
        .header(
            "SOAPAction",
            "\"urn:schemas-upnp-org:service:AVTransport:1#Seek\"",
        )
        .body(hyper::Body::from(soap_body))?;

    let response = client.request(request).await?;
    let status = response.status();
    if !status.is_success() {
        let body_bytes = hyper::body::to_bytes(response.into_body()).await?;
        return Err(format!(
            "Failed to seek. Status: {}. Response: {}",
            status,
            String::from_utf8_lossy(&body_bytes)
        )
        .into());
    }
    Ok(())
}

/// Gets the current transport state from the device.
/// Returns state string: "PLAYING", "PAUSED_PLAYBACK", "STOPPED", or "UNKNOWN".
pub async fn get_transport_state(
    av_control_url: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let soap_body = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
    <s:Body>
        <u:GetTransportInfo xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
            <InstanceID>0</InstanceID>
        </u:GetTransportInfo>
    </s:Body>
</s:Envelope>"#;

    let client = hyper::Client::new();
    let request = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(av_control_url)
        .header("Content-Type", "text/xml; charset=utf-8")
        .header(
            "SOAPAction",
            "\"urn:schemas-upnp-org:service:AVTransport:1#GetTransportInfo\"",
        )
        .body(hyper::Body::from(soap_body))?;

    let response = client.request(request).await?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Failed to get transport info. Status: {}", status).into());
    }

    let body_bytes = hyper::body::to_bytes(response.into_body()).await?;
    let body_str = String::from_utf8_lossy(&body_bytes);

    if let Some(start) = body_str.find("<CurrentTransportState>") {
        let after = &body_str[start + "<CurrentTransportState>".len()..];
        if let Some(end) = after.find("</CurrentTransportState>") {
            return Ok(after[..end].to_string());
        }
    }

    Ok("UNKNOWN".to_string())
}

pub fn get_mime_type(file_path: &str) -> &'static str {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext.to_lowercase().as_str() {
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "mp3" => "audio/mpeg",
        "srt" => "application/x-subrip",
        _ => "application/octet-stream",
    }
}
