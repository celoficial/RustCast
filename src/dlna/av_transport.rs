use crate::soap::{self, xml_escape, SoapClient};

const AV_TRANSPORT: &str = "urn:schemas-upnp-org:service:AVTransport:1";

pub async fn set_uri(
    client: &SoapClient,
    url: &str,
    media_url: &str,
    metadata_escaped: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let params = format!(
        "<InstanceID>0</InstanceID>\
<CurrentURI>{}</CurrentURI>\
<CurrentURIMetaData>{}</CurrentURIMetaData>",
        xml_escape(media_url),
        metadata_escaped
    );
    let body = soap::build_action(AV_TRANSPORT, "SetAVTransportURI", &params);
    soap::send(
        client,
        url,
        &soap::action_header(AV_TRANSPORT, "SetAVTransportURI"),
        &body,
    )
    .await
    .map(|_| ())
}

pub async fn play(client: &SoapClient, url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let body = soap::build_action(
        AV_TRANSPORT,
        "Play",
        "<InstanceID>0</InstanceID><Speed>1</Speed>",
    );
    soap::send(
        client,
        url,
        &soap::action_header(AV_TRANSPORT, "Play"),
        &body,
    )
    .await
    .map(|_| ())
}

pub async fn pause(client: &SoapClient, url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let body = soap::build_action(AV_TRANSPORT, "Pause", "<InstanceID>0</InstanceID>");
    soap::send(
        client,
        url,
        &soap::action_header(AV_TRANSPORT, "Pause"),
        &body,
    )
    .await
    .map(|_| ())
}

pub async fn stop(client: &SoapClient, url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let body = soap::build_action(AV_TRANSPORT, "Stop", "<InstanceID>0</InstanceID>");
    soap::send(
        client,
        url,
        &soap::action_header(AV_TRANSPORT, "Stop"),
        &body,
    )
    .await
    .map(|_| ())
}

pub async fn seek(
    client: &SoapClient,
    url: &str,
    position: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let params = format!(
        "<InstanceID>0</InstanceID><Unit>REL_TIME</Unit><Target>{}</Target>",
        xml_escape(position)
    );
    let body = soap::build_action(AV_TRANSPORT, "Seek", &params);
    soap::send(
        client,
        url,
        &soap::action_header(AV_TRANSPORT, "Seek"),
        &body,
    )
    .await
    .map(|_| ())
}

pub async fn get_transport_state(
    client: &SoapClient,
    url: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let body = soap::build_action(
        AV_TRANSPORT,
        "GetTransportInfo",
        "<InstanceID>0</InstanceID>",
    );
    let response = soap::send(
        client,
        url,
        &soap::action_header(AV_TRANSPORT, "GetTransportInfo"),
        &body,
    )
    .await?;

    if let Some(start) = response.find("<CurrentTransportState>") {
        let after = &response[start + "<CurrentTransportState>".len()..];
        if let Some(end) = after.find("</CurrentTransportState>") {
            return Ok(after[..end].to_string());
        }
    }

    Ok("UNKNOWN".to_string())
}
