use hyper::Client;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

pub const SOAP_TIMEOUT: Duration = Duration::from_secs(10);

/// Shared hyper client — one instance per session, reuses connection pool.
pub type SoapClient = Arc<Client<hyper::client::HttpConnector>>;

pub fn new_soap_client() -> SoapClient {
    Arc::new(Client::new())
}

/// Builds a SOAP envelope for a UPnP action call.
/// `service_urn` — e.g. `"urn:schemas-upnp-org:service:AVTransport:1"`
/// `method`      — e.g. `"Play"`
/// `params`      — inner XML, already escaped where necessary
pub fn build_action(service_urn: &str, method: &str, params: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?>\n\
<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\n\
    <s:Body>\n\
        <u:{0} xmlns:u=\"{1}\">\n\
            {2}\n\
        </u:{0}>\n\
    </s:Body>\n\
</s:Envelope>",
        method, service_urn, params
    )
}

/// Builds the quoted SOAPAction header value for a UPnP action.
/// e.g. `action_header("urn:...:AVTransport:1", "Play")` → `"\"urn:...:AVTransport:1#Play\""`
pub fn action_header(service_urn: &str, method: &str) -> String {
    format!("\"{}#{}\"", service_urn, method)
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Sends a SOAP POST and returns the response body on success.
/// Returns Err on timeout, HTTP errors, or network failures.
pub async fn send(
    client: &SoapClient,
    url: &str,
    action: &str,
    body: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let request = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(url)
        .header("Content-Type", "text/xml; charset=utf-8")
        .header("SOAPAction", action)
        .body(hyper::Body::from(body.to_owned()))?;

    let response = timeout(SOAP_TIMEOUT, client.request(request))
        .await
        .map_err(|_| format!("SOAP request to {} timed out", url))??;

    let status = response.status();
    let bytes = hyper::body::to_bytes(response.into_body()).await?;

    if !status.is_success() {
        return Err(format!(
            "SOAP {} from {}: {}",
            status,
            url,
            String::from_utf8_lossy(&bytes)
        )
        .into());
    }

    String::from_utf8(bytes.into()).map_err(|e| e.into())
}
