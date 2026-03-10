use crate::soap::{self, SoapClient};

const CONNECTION_MANAGER: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";

pub async fn prepare_connection(
    client: &SoapClient,
    url: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let params = "\
<RemoteProtocolInfo>http-get:*:video/mp4:DLNA.ORG_OP=01;DLNA.ORG_FLAGS=01700000000000000000000000000000</RemoteProtocolInfo>\
<PeerConnectionManager></PeerConnectionManager>\
<PeerConnectionID>0</PeerConnectionID>\
<Direction>Input</Direction>";

    let body = soap::build_action(CONNECTION_MANAGER, "PrepareForConnection", params);
    soap::send(
        client,
        url,
        &soap::action_header(CONNECTION_MANAGER, "PrepareForConnection"),
        &body,
    )
    .await
    .map(|_| ())
}
