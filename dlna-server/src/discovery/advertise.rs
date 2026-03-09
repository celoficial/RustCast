use crate::config::Config;
use tokio::net::UdpSocket;
use tokio::time::{interval, Duration};

const NOTIFY_INTERVAL_SECS: u64 = 30;
const CACHE_MAX_AGE: u32 = 1800;

/// Builds the three NOTIFY ssdp:alive messages required for a UPnP MediaServer:
///   1. upnp:rootdevice
///   2. uuid:<UDN>
///   3. urn:schemas-upnp-org:device:MediaServer:1
fn build_alive_messages(location: &str, udn: &str, multicast_host: &str) -> Vec<String> {
    let entries = [
        ("upnp:rootdevice", format!("{}::upnp:rootdevice", udn)),
        (udn, udn.to_string()),
        (
            "urn:schemas-upnp-org:device:MediaServer:1",
            format!("{}::urn:schemas-upnp-org:device:MediaServer:1", udn),
        ),
    ];
    entries
        .iter()
        .map(|(nt, usn)| {
            format!(
                "NOTIFY * HTTP/1.1\r\n\
         HOST: {}\r\n\
         CACHE-CONTROL: max-age={}\r\n\
         LOCATION: {}\r\n\
         NT: {}\r\n\
         NTS: ssdp:alive\r\n\
         SERVER: Rust/1.0 UPnP/1.0 RustCast/0.1\r\n\
         USN: {}\r\n\
         \r\n",
                multicast_host, CACHE_MAX_AGE, location, nt, usn
            )
        })
        .collect()
}

/// Builds the three NOTIFY ssdp:byebye messages to announce the server is going offline.
fn build_byebye_messages(udn: &str, multicast_host: &str) -> Vec<String> {
    let entries = [
        ("upnp:rootdevice", format!("{}::upnp:rootdevice", udn)),
        (udn, udn.to_string()),
        (
            "urn:schemas-upnp-org:device:MediaServer:1",
            format!("{}::urn:schemas-upnp-org:device:MediaServer:1", udn),
        ),
    ];
    entries
        .iter()
        .map(|(nt, usn)| {
            format!(
                "NOTIFY * HTTP/1.1\r\n\
         HOST: {}\r\n\
         NT: {}\r\n\
         NTS: ssdp:byebye\r\n\
         USN: {}\r\n\
         \r\n",
                multicast_host, nt, usn
            )
        })
        .collect()
}

async fn send_messages(socket: &UdpSocket, messages: &[String], target: &str) {
    for msg in messages {
        if let Err(e) = socket.send_to(msg.as_bytes(), target).await {
            eprintln!("SSDP advertiser: send failed: {}", e);
        }
    }
}

/// Sends ssdp:byebye for all three notification types. Called on shutdown.
pub async fn send_notify_byebye(config: &Config) {
    let target = format!("{}:{}", config.multicast_address, config.multicast_port);
    let messages = build_byebye_messages(&config.udn, &target);
    match UdpSocket::bind("0.0.0.0:0").await {
        Ok(socket) => {
            send_messages(&socket, &messages, &target).await;
            println!("SSDP: sent ssdp:byebye");
        }
        Err(e) => eprintln!("SSDP byebye: failed to bind socket: {}", e),
    }
}

/// Spawns a background task that announces this server on the LAN via SSDP NOTIFY.
/// Sends ssdp:alive immediately on start, then every 30 seconds.
pub fn start_ssdp_advertiser(config: Config) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let location = format!(
            "http://{}:{}/description.xml",
            config.http_address, config.http_port
        );
        let target = format!("{}:{}", config.multicast_address, config.multicast_port);
        let alive = build_alive_messages(&location, &config.udn, &target);

        let socket = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("SSDP advertiser: failed to bind socket: {}", e);
                return;
            }
        };

        send_messages(&socket, &alive, &target).await;
        println!(
            "SSDP advertiser started — announcing as '{}'",
            config.friendly_name
        );

        // Consume the first immediate tick so the interval starts from now
        let mut tick = interval(Duration::from_secs(NOTIFY_INTERVAL_SECS));
        tick.tick().await;

        loop {
            tick.tick().await;
            send_messages(&socket, &alive, &target).await;
        }
    })
}
