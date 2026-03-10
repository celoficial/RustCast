use reqwest::Client;
use serde::Deserialize;
use serde_xml_rs::from_str;

use crate::discovery::ssdp::{rediscover_by_usn, SsdpDevice};

#[derive(Debug, Deserialize, Default)]
pub struct DeviceDescription {
    #[serde(rename = "device", default)]
    pub device: Device,
}

#[derive(Debug, Deserialize, Default)]
pub struct Device {
    #[serde(rename = "friendlyName", default)]
    pub friendly_name: String,

    #[serde(rename = "serviceList", default)]
    pub service_list: Option<ServiceList>,
}

#[derive(Debug, Deserialize)]
pub struct ServiceList {
    #[serde(rename = "service", default)]
    pub services: Vec<Service>,
}

#[derive(Debug, Deserialize)]
pub struct Service {
    #[serde(rename = "serviceType", default)]
    pub service_type: String,

    #[serde(rename = "controlURL", default)]
    pub control_url: String,
}

/// Parses a UPnP device description XML string.
pub fn parse_device_description(
    xml: &str,
) -> Result<DeviceDescription, Box<dyn std::error::Error>> {
    Ok(from_str(xml)?)
}

/// Extracts the base URL (scheme + host + port) from a full URL.
/// e.g. `"http://192.168.1.100:52235/description.xml"` → `"http://192.168.1.100:52235"`
pub fn extract_base_url(location: &str) -> String {
    if let Some(scheme_end) = location.find("://") {
        let after_scheme = &location[scheme_end + 3..];
        if let Some(path_start) = after_scheme.find('/') {
            return location[..scheme_end + 3 + path_start].to_string();
        }
    }
    location.to_string()
}

/// Finds a UPnP service control URL by matching the service type string.
/// Returns the absolute URL (base_url + controlURL), or None if not found.
pub fn find_control_url(
    desc: &DeviceDescription,
    service_type_fragment: &str,
    base_url: &str,
) -> Option<String> {
    let service_list = desc.device.service_list.as_ref()?;
    let service = service_list
        .services
        .iter()
        .find(|s| s.service_type.contains(service_type_fragment))?;
    let control_url = service.control_url.trim();
    if control_url.starts_with("http://") || control_url.starts_with("https://") {
        Some(control_url.to_string())
    } else {
        Some(format!("{}{}", base_url, control_url))
    }
}

/// Like `find_control_url` but falls back to `base_url + default_path` if the service
/// is not found in the device description. Eliminates the repeated unwrap_or_else pattern.
pub fn find_control_url_with_fallback(
    desc: &DeviceDescription,
    service_type_fragment: &str,
    base_url: &str,
    default_path: &str,
) -> String {
    find_control_url(desc, service_type_fragment, base_url)
        .unwrap_or_else(|| format!("{}{}", base_url, default_path))
}

/// Fetches and parses the device description XML from the given location URL.
pub async fn fetch_device_description(
    location: &str,
) -> Result<DeviceDescription, Box<dyn std::error::Error>> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let response = client.get(location).send().await?;
    if !response.status().is_success() {
        return Err(format!(
            "Error fetching device description from {}: HTTP {}",
            location,
            response.status()
        )
        .into());
    }
    let xml = response.text().await?;
    parse_device_description(&xml)
}

/// Attempts to rediscover a device by USN after it went offline, then re-fetches its
/// description and returns updated `(av_control_url, cm_control_url)` on success.
pub async fn reconnect_device(
    multicast_addr: &str,
    multicast_port: u16,
    usn: &str,
) -> Option<(String, String)> {
    let device: SsdpDevice = rediscover_by_usn(multicast_addr, multicast_port, usn).await?;
    let desc = fetch_device_description(&device.location).await.ok()?;
    let base = extract_base_url(&device.location);

    let av =
        find_control_url_with_fallback(&desc, "AVTransport", &base, "/upnp/control/AVTransport1");
    let cm = find_control_url_with_fallback(
        &desc,
        "ConnectionManager",
        &base,
        "/upnp/control/ConnectionManager1",
    );

    Some((av, cm))
}
