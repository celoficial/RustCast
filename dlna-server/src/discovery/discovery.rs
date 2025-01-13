use tokio::net::UdpSocket;
use tokio::time::{Instant, Duration, timeout};
use std::collections::HashMap;

pub async fn discover_ssdp(multicast_addr: &str, multicast_port: u16) -> Result<Vec<HashMap<String, String>>, Box<dyn std::error::Error>> {
    let multicast_address = format!("{multicast_addr}:{multicast_port}");
    let m_search = format!(
        "M-SEARCH * HTTP/1.1\r\n\
        HOST: 239.255.255.250:1900\r\n\
        MAN: \"ssdp:discover\"\r\n\
        MX: 5\r\n\
        ST: ssdp:all\r\n\
        \r\n"
    );

    let socket = UdpSocket::bind("192.168.0.97:0").await?;
    socket.set_multicast_ttl_v4(4)?;
    println!("Socket UDP criado na porta local: {:?}", socket.local_addr());

    // Envia a mensagem M-SEARCH
    println!("Enviando solicitação SSDP para {multicast_address}...");
    socket.send_to(m_search.as_bytes(), &multicast_address).await?;
    println!("Solicitação SSDP enviada com sucesso!");

    let mut devices: Vec<HashMap<String, String>> = Vec::new();
    let mut buf = [0u8; 1024];

    let start_time = Instant::now();
    let timeout_duration = Duration::from_secs(10);

    // Loop de recebimento com timeout global
    while start_time.elapsed() < timeout_duration {
        // Timeout para cada operação de recebimento
        match timeout(Duration::from_secs(1), socket.recv_from(&mut buf)).await {
            Ok(Ok((len, addr))) => {
                let response = String::from_utf8_lossy(&buf[..len]);
                println!("Resposta SSDP recebida de {} com {} bytes", addr, len);

                if response.contains("urn:schemas-upnp-org:device:MediaRenderer:1") {
                    let mut device_info = HashMap::new();

                    if let Some(location) = response.lines().find(|line| line.starts_with("LOCATION:")) {
                        device_info.insert("LOCATION".to_string(), location["LOCATION:".len()..].trim().to_string());
                    }

                    if let Some(usn) = response.lines().find(|line| line.starts_with("USN:")) {
                        device_info.insert("USN".to_string(), usn["USN:".len()..].trim().to_string());
                    }

                    if let Some(friendly_name) = response.lines().find(|line| line.starts_with("ST:")) {
                        device_info.insert("ST".to_string(), friendly_name["ST:".len()..].trim().to_string());
                    }

                    if !devices.iter().any(|d| d.get("USN") == device_info.get("USN")) {
                        println!("Novo dispositivo encontrado: {:?}", device_info);
                        devices.push(device_info);
                    }
                }
            }
            Ok(Err(e)) => {
                println!("Erro ao receber resposta SSDP: {}", e);
            }
            Err(_) => {
                println!("Timeout ao esperar por respostas SSDP.");
            }
        }
    }

    println!("Saindo da função discover_ssdp.");
    Ok(devices)
}