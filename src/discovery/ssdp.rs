use std::collections::HashMap;
use tokio::net::UdpSocket;
use tokio::time::{timeout, Duration, Instant};

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
/// e.g. "uuid:abc-123::urn:schemas-upnp-org:device:MediaRenderer:1" → "uuid:abc-123"
fn usn_uuid(usn: &str) -> &str {
    usn.split("::").next().unwrap_or(usn)
}

/// Parses a raw SSDP response into a device info map (LOCATION, USN, ST).
/// Returns None if the response is not a MediaRenderer or has no USN.
fn parse_media_renderer(response: &str) -> Option<HashMap<String, String>> {
    if !response.contains("urn:schemas-upnp-org:device:MediaRenderer:1") {
        return None;
    }
    let mut device_info = HashMap::new();
    if let Some(location) = header_value(response, "LOCATION") {
        device_info.insert("LOCATION".to_string(), location.to_string());
    }
    if let Some(usn) = header_value(response, "USN") {
        device_info.insert("USN".to_string(), usn.to_string());
    }
    if let Some(st) = header_value(response, "ST") {
        device_info.insert("ST".to_string(), st.to_string());
    }
    if device_info.contains_key("USN") {
        Some(device_info)
    } else {
        None
    }
}

pub async fn discover_ssdp(
    multicast_addr: &str,
    multicast_port: u16,
) -> Result<Vec<HashMap<String, String>>, Box<dyn std::error::Error>> {
    let multicast_address = format!("{multicast_addr}:{multicast_port}");
    let m_search = format!(
        "M-SEARCH * HTTP/1.1\r\n\
        HOST: {multicast_addr}:{multicast_port}\r\n\
        MAN: \"ssdp:discover\"\r\n\
        MX: 5\r\n\
        ST: ssdp:all\r\n\
        \r\n"
    );

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.set_multicast_ttl_v4(4)?;
    socket
        .send_to(m_search.as_bytes(), &multicast_address)
        .await?;

    let mut devices: Vec<HashMap<String, String>> = Vec::new();
    let mut buf = [0u8; 4096];

    let start_time = Instant::now();
    let timeout_duration = Duration::from_secs(5);

    while start_time.elapsed() < timeout_duration {
        match timeout(Duration::from_secs(1), socket.recv_from(&mut buf)).await {
            Ok(Ok((len, _))) => {
                let response = String::from_utf8_lossy(&buf[..len]);
                if let Some(device_info) = parse_media_renderer(&response) {
                    if !devices
                        .iter()
                        .any(|d| d.get("USN") == device_info.get("USN"))
                    {
                        devices.push(device_info);
                    }
                }
            }
            Ok(Err(_)) => {}
            Err(_) => {}
        }
    }
    Ok(devices)
}

/// Attempts to rediscover a specific device by its USN (UUID portion).
/// Returns the new device info map (with updated LOCATION) if found within the timeout.
pub async fn rediscover_by_usn(
    multicast_addr: &str,
    multicast_port: u16,
    target_usn: &str,
) -> Option<HashMap<String, String>> {
    let target_uuid = usn_uuid(target_usn);

    let multicast_address = format!("{multicast_addr}:{multicast_port}");
    let m_search = format!(
        "M-SEARCH * HTTP/1.1\r\n\
        HOST: {multicast_addr}:{multicast_port}\r\n\
        MAN: \"ssdp:discover\"\r\n\
        MX: 3\r\n\
        ST: ssdp:all\r\n\
        \r\n"
    );

    let socket = UdpSocket::bind("0.0.0.0:0").await.ok()?;
    socket.set_multicast_ttl_v4(4).ok()?;
    socket
        .send_to(m_search.as_bytes(), &multicast_address)
        .await
        .ok()?;

    let mut buf = [0u8; 4096];
    let start_time = Instant::now();
    let timeout_duration = Duration::from_secs(5);

    while start_time.elapsed() < timeout_duration {
        match timeout(Duration::from_secs(1), socket.recv_from(&mut buf)).await {
            Ok(Ok((len, _))) => {
                let response = String::from_utf8_lossy(&buf[..len]);
                if let Some(device_info) = parse_media_renderer(&response) {
                    if let Some(usn) = device_info.get("USN") {
                        if usn_uuid(usn) == target_uuid {
                            return Some(device_info);
                        }
                    }
                }
            }
            Ok(Err(_)) => {}
            Err(_) => {}
        }
    }
    None
}
