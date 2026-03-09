use hyper::{Body, Client, Method, Request};
use crate::config::Config;
use super::manager::MediaFile;

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
        .header("SOAPACTION", r#""urn:schemas-upnp-org:service:ConnectionManager:1#PrepareForConnection""#)
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .body(Body::from(soap_body))?;

    let response = client.request(request).await?;

    if !response.status().is_success() {
        return Err(format!("Failed to configure connection. Status: {}", response.status()).into());
    }

    println!("Connection configured successfully!");
    Ok(())
}

/// Streams a media file to the DLNA device using the discovered control URLs.
pub async fn stream_media(
    config: &Config,
    av_control_url: &str,
    cm_control_url: &str,
    media_file: &MediaFile,
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

    // PrepareForConnection using the discovered ConnectionManager URL
    prepare_connection(cm_control_url).await?;

    let client = hyper::Client::new();

    let mime_type = get_mime_type(&media_file.path);

    // DIDL-Lite metadata — escape all values to prevent XML injection
    let title_escaped = xml_escape(&media_file.name);
    let url_escaped = xml_escape(&media_url);
    let mime_escaped = xml_escape(mime_type);

    let current_uri_metadata = format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="0" parentID="-1" restricted="1"><dc:title>{}</dc:title><res protocolInfo="http-get:*:{}:DLNA.ORG_OP=01">{}</res></item></DIDL-Lite>"#,
        title_escaped, mime_escaped, url_escaped
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
        xml_escape(&media_url), metadata_escaped
    );

    println!("Sending SetAVTransportURI command to {}", av_control_url);

    let request_set_uri = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(av_control_url)
        .header("Content-Type", "text/xml; charset=utf-8")
        .header("SOAPAction", "\"urn:schemas-upnp-org:service:AVTransport:1#SetAVTransportURI\"")
        .body(hyper::Body::from(soap_body_set_uri))?;

    let response_set_uri = client.request(request_set_uri).await?;
    let status_set_uri = response_set_uri.status();
    if !status_set_uri.is_success() {
        let body_bytes = hyper::body::to_bytes(response_set_uri.into_body()).await?;
        return Err(format!(
            "Failed to configure transport. Status: {}. Response: {}",
            status_set_uri,
            String::from_utf8_lossy(&body_bytes)
        ).into());
    }

    println!("Media configured successfully! Sending Play command...");

    let soap_body_play = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
    <s:Body>
        <u:Play xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
            <InstanceID>0</InstanceID>
            <Speed>1</Speed>
        </u:Play>
    </s:Body>
</s:Envelope>"#;

    let request_play = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(av_control_url)
        .header("Content-Type", "text/xml; charset=utf-8")
        .header("SOAPAction", "\"urn:schemas-upnp-org:service:AVTransport:1#Play\"")
        .body(hyper::Body::from(soap_body_play))?;

    let response_play = client.request(request_play).await?;
    let status_play = response_play.status();
    if !status_play.is_success() {
        let body_bytes = hyper::body::to_bytes(response_play.into_body()).await?;
        return Err(format!(
            "Failed to start playback. Status: {}. Response: {}",
            status_play,
            String::from_utf8_lossy(&body_bytes)
        ).into());
    }

    println!("Playback started successfully!");

    Ok(())
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
