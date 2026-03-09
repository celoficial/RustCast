use std::collections::HashMap;
use tokio::net::UdpSocket;
use tokio::time::{timeout, Duration, Instant};

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
    println!(
        "UDP socket created on local port: {:?}",
        socket.local_addr()
    );

    // Sends the M-SEARCH message
    println!("Sending SSDP request to {multicast_address}...");
    socket
        .send_to(m_search.as_bytes(), &multicast_address)
        .await?;
    println!("SSDP request sent successfully!");

    let mut devices: Vec<HashMap<String, String>> = Vec::new();
    let mut buf = [0u8; 4096];

    let start_time = Instant::now();
    let timeout_duration = Duration::from_secs(5);

    // Receive loop with global timeout
    while start_time.elapsed() < timeout_duration {
        // Timeout for each receive operation
        match timeout(Duration::from_secs(1), socket.recv_from(&mut buf)).await {
            Ok(Ok((len, _))) => {
                let response = String::from_utf8_lossy(&buf[..len]);

                if response.contains("urn:schemas-upnp-org:device:MediaRenderer:1") {
                    let mut device_info = HashMap::new();

                    // HTTP headers are case-insensitive; match accordingly
                    fn header_value<'a>(response: &'a str, name: &str) -> Option<&'a str> {
                        let prefix_len = name.len() + 1; // name + ':'
                        response.lines().find_map(|line| {
                            if line.len() > prefix_len
                                && line[..prefix_len].eq_ignore_ascii_case(&format!("{}:", name))
                            {
                                Some(line[prefix_len..].trim())
                            } else {
                                None
                            }
                        })
                    }

                    if let Some(location) = header_value(&response, "LOCATION") {
                        device_info.insert("LOCATION".to_string(), location.to_string());
                    }

                    if let Some(usn) = header_value(&response, "USN") {
                        device_info.insert("USN".to_string(), usn.to_string());
                    }

                    if let Some(st) = header_value(&response, "ST") {
                        device_info.insert("ST".to_string(), st.to_string());
                    }

                    if !devices
                        .iter()
                        .any(|d| d.get("USN") == device_info.get("USN"))
                    {
                        println!("New device found: {:?}", device_info);
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
