use hyper::{Body, Client, Method, Request};
use std::path::Path;

use super::manager::MediaFile;

/// Função para configurar a conexão (PrepareForConnection)
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
        .uri(format!("{device_url}/upnp/control/ConnectionManager1"))
        .header("SOAPACTION", r#""urn:schemas-upnp-org:service:ConnectionManager:1#PrepareForConnection""#)
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .body(Body::from(soap_body))?;

    let response = client.request(request).await?;

    if !response.status().is_success() {
        return Err(format!("Falha ao configurar conexão. Status: {}", response.status()).into());
    }

    println!("Conexão configurada com sucesso!");
    Ok(())
}

/// Função para transmitir a mídia
pub async fn stream_media(device_url: &str, media_file: &MediaFile) -> Result<(), Box<dyn std::error::Error>> {
    let file_path = Path::new(&media_file.path);

    if !file_path.exists() {
        return Err(format!("O arquivo '{}' não foi encontrado.", media_file.path).into());
    }

    let normalized_path = file_path.canonicalize()?;
    let cleaned_path = normalized_path.to_str().unwrap_or("").trim_start_matches(r"\\?\");
    println!("Iniciando transmissão de: {}", cleaned_path);

    prepare_connection(device_url).await?;

    let client = Client::new();

    let current_uri = format!("http://192.168.0.97:8080/media/{}", media_file.name); //TODO: Mudar para o IP e porta do servidor http no .env

    let soap_body = format!(
        r#"
        <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
          <s:Body>
            <u:SetAVTransportURI xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
              <InstanceID>0</InstanceID>
              <CurrentURI>{}</CurrentURI>
              <CurrentURIMetaData></CurrentURIMetaData>
            </u:SetAVTransportURI>
          </s:Body>
        </s:Envelope>
        "#,
        current_uri
    );

    let request = Request::builder()
    .method(Method::POST)
    .uri(format!("{device_url}/upnp/control/AVTransport1"))
    .header("SOAPACTION", r#""urn:schemas-upnp-org:service:AVTransport:1#SetAVTransportURI""#)
    .header("Content-Type", "text/xml; charset=\"utf-8\"")
    .body(Body::from(soap_body))?;

    let response = client.request(request).await?;
    if !response.status().is_success() {
        return Err(format!(
            "Falha ao configurar transporte. Status: {}",
            response.status()
        ).into());
    }

    println!("Transporte configurado com sucesso.");

    println!("Mídia configurada com sucesso! Iniciando reprodução...");

    Ok(())
}

