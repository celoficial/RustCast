use hyper::{Body, Client, Method, Request};
use crate::config::Config;

use super::manager::MediaFile;

// Function to set up the connection (PrepareForConnection)
pub async fn prepare_connection(device_url: &str) -> Result<(), Box<dyn std::error::Error>> {
  let soap_body = format!(
      r#"
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
      "#
  );

  let client = Client::new();
  
  let request = Request::builder()
      .method(Method::POST)
      .uri(format!("{device_url}/drm/upnp/control/ConnectionManager1"))
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

// Function to stream media
pub async fn stream_media(device_url: &str, media_file: &MediaFile) -> Result<(), Box<dyn std::error::Error>> {
  let config = Config::from_env();
  let file_path = std::path::Path::new(&media_file.path);

  if !file_path.exists() {
      return Err(format!("The file '{}' was not found.", media_file.path).into());
  }

  let normalized_path = file_path.canonicalize()?;
  let cleaned_path = normalized_path.to_str().unwrap_or("").trim_start_matches(r"\\?\");

  println!("Starting streaming of: {}", normalized_path.display());

  // URL where the file is available to the TV
  let media_url = format!("http://{}:{}/media/{}", config.http_address, config.http_port, cleaned_path);

  // Adds the prepare_connection step
  prepare_connection(device_url).await?;

  let client = hyper::Client::new();

  //let av_transport_url = "http://192.168.0.109:9197/upnp/control/AVTransport1";

  // URI metadata (optional, but required by some TVs)
  let current_uri_metadata = format!(
      r#"
      <DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/">
          <item id="0" parentID="-1" restricted="1">
              <dc:title>{}</dc:title>
              <res protocolInfo="http-get:*:video/mp4:DLNA.ORG_OP=01">{}</res>
          </item>
      </DIDL-Lite>
      "#,
      media_file.name, media_url
  );

  let soap_body_set_uri = format!(
      r#"
      <?xml version="1.0"?>
      <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
          <s:Body>
              <u:SetAVTransportURI xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
                  <InstanceID>0</InstanceID>
                  <CurrentURI>{}</CurrentURI>
                  <CurrentURIMetaData>{}</CurrentURIMetaData>
              </u:SetAVTransportURI>
          </s:Body>
      </s:Envelope>
      "#,
      media_url, current_uri_metadata
  );

  println!("Sending SetAVTransportURI command to {}/upnp/control/AVTransport1", device_url);

  let request_set_uri = hyper::Request::builder()
      .method(hyper::Method::POST)
      .uri(format!("{}/upnp/control/AVTransport1", device_url))
      .header("Content-Type", "text/xml; charset=utf-8")
      .header("SOAPAction", "\"urn:schemas-upnp-org:service:AVTransport:1#SetAVTransportURI\"")
      .body(hyper::Body::from(soap_body_set_uri))?;

  let response_set_uri = client.request(request_set_uri).await?;
  if !response_set_uri.status().is_success() {
      let body_bytes = hyper::body::to_bytes(response_set_uri.into_body()).await?;
      return Err(format!(
          "Failed to configure transport. Status: {}. Response: {}",
          "400 hardcoded",
          String::from_utf8_lossy(&body_bytes)
      ).into());
  }

  println!("Media configured successfully! Sending Play command...");

  // Adds the Play command
  let soap_body_play = r#"
  <?xml version="1.0"?>
  <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
      <s:Body>
          <u:Play xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
              <InstanceID>0</InstanceID>
              <Speed>1</Speed>
          </u:Play>
      </s:Body>
  </s:Envelope>
  "#;

  let request_play = hyper::Request::builder()
      .method(hyper::Method::POST)
      .uri(format!("{}/upnp/control/AVTransport1", device_url))
      .header("Content-Type", "text/xml; charset=utf-8")
      .header("SOAPAction", "\"urn:schemas-upnp-org:service:AVTransport:1#Play\"")
      .body(hyper::Body::from(soap_body_play))?;

  let response_play = client.request(request_play).await?;
  if !response_play.status().is_success() {
      let body_bytes = hyper::body::to_bytes(response_play.into_body()).await?;
      return Err(format!(
          "Failed to start playback. Status: {}. Response: {}",
          "400 hardcoded",
          String::from_utf8_lossy(&body_bytes)
      ).into());
  }

  println!("Playback started successfully!");

  Ok(())
}
