use std::collections::HashSet;
use tokio::net::UdpSocket;
use tokio::time::{timeout, Duration, Instant};

/// A discovered DLNA MediaRenderer device.
#[derive(Debug, Clone)]
pub struct SsdpDevice {
    pub location: String,
    pub usn: String,
}

/// Extracts a case-insensitive HTTP header value from an SSDP response string.
fn header_value<'a>(response: &'a str, name: &str) -> Option<&'a str> {
    let prefix_len = name.len() + 1; // name + ':'
    response.lines().find_map(|line| {
        if line.len() > prefix_len && line[..prefix_len].eq_ignore_ascii_case(&format!("{}:", name))
        {
            Some(line[prefix_len..].trim())
        } else {
            None
        }
    })
}

/// Extracts the UUID portion from a USN value.
/// e.g. `"uuid:abc-123::urn:schemas-upnp-org:device:MediaRenderer:1"` → `"uuid:abc-123"`
fn usn_uuid(usn: &str) -> &str {
    usn.split("::").next().unwrap_or(usn)
}

/// Parses a raw SSDP response into an `SsdpDevice`.
/// Returns `None` if the response is not a MediaRenderer or has no LOCATION/USN.
fn parse_media_renderer(response: &str) -> Option<SsdpDevice> {
    if !response.contains("urn:schemas-upnp-org:device:MediaRenderer:1") {
        return None;
    }
    let location = header_value(response, "LOCATION")?.to_string();
    let usn = header_value(response, "USN")?.to_string();
    Some(SsdpDevice { location, usn })
}

/// Sends an SSDP M-SEARCH and collects all MediaRenderer responses within `total_timeout_secs`.
/// `mx_secs` controls the MX header value (max response delay requested from devices).
async fn msearch(
    multicast_addr: &str,
    multicast_port: u16,
    mx_secs: u8,
    total_timeout_secs: u64,
) -> Result<Vec<SsdpDevice>, Box<dyn std::error::Error>> {
    let multicast_address = format!("{multicast_addr}:{multicast_port}");
    let m_search = format!(
        "M-SEARCH * HTTP/1.1\r\n\
        HOST: {multicast_addr}:{multicast_port}\r\n\
        MAN: \"ssdp:discover\"\r\n\
        MX: {mx_secs}\r\n\
        ST: ssdp:all\r\n\
        \r\n"
    );

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.set_multicast_ttl_v4(4)?;
    socket
        .send_to(m_search.as_bytes(), &multicast_address)
        .await?;

    let mut devices: Vec<SsdpDevice> = Vec::new();
    let mut buf = [0u8; 4096];
    let deadline = Instant::now() + Duration::from_secs(total_timeout_secs);

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let wait = remaining.min(Duration::from_secs(1));

        match timeout(wait, socket.recv_from(&mut buf)).await {
            Ok(Ok((len, _))) => {
                let response = String::from_utf8_lossy(&buf[..len]);
                if let Some(device) = parse_media_renderer(&response) {
                    devices.push(device);
                }
            }
            Ok(Err(e)) => eprintln!("[ssdp] recv error: {}", e),
            Err(_) => {} // sub-timeout expiry, normal
        }
    }

    Ok(devices)
}

/// Discovers all DLNA MediaRenderer devices on the LAN via SSDP M-SEARCH.
/// Deduplicates results by USN UUID.
pub async fn discover_ssdp(
    multicast_addr: &str,
    multicast_port: u16,
) -> Result<Vec<SsdpDevice>, Box<dyn std::error::Error>> {
    let all = msearch(multicast_addr, multicast_port, 5, 5).await?;

    let mut seen = HashSet::new();
    let unique = all
        .into_iter()
        .filter(|d| seen.insert(usn_uuid(&d.usn).to_string()))
        .collect();

    Ok(unique)
}

/// Attempts to rediscover a specific device by its USN after it went offline.
/// Returns the device with an updated LOCATION if found within the timeout.
pub async fn rediscover_by_usn(
    multicast_addr: &str,
    multicast_port: u16,
    target_usn: &str,
) -> Option<SsdpDevice> {
    let target_uuid = usn_uuid(target_usn).to_string();
    let all = msearch(multicast_addr, multicast_port, 3, 5).await.ok()?;
    all.into_iter().find(|d| usn_uuid(&d.usn) == target_uuid)
}
