use reqwest::Client;
use serde::Deserialize;
use serde_xml_rs::from_str;

// Estruturas para parsing do XML
#[derive(Debug, Deserialize, Default)]
pub struct DeviceDescription {
    #[serde(rename = "device", default)]
    pub device: Device,
}

#[derive(Debug, Deserialize, Default)]
pub struct Device {
    #[serde(rename = "friendlyName", default)]
    pub friendly_name: String,

    #[serde(rename = "manufacturer", default)]
    pub manufacturer: String,

    #[serde(rename = "modelName", default)]
    pub model_name: String,

    #[serde(rename = "UDN", default)]
    pub udn: String,

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

    #[serde(rename = "SCPDURL", default)]
    pub scpd_url: String,
}

/// Parses XML into a DeviceDescription
pub fn parse_device_description(xml: &str) -> Result<DeviceDescription, Box<dyn std::error::Error>> {
    let description: DeviceDescription = from_str(xml)?;
    Ok(description)
}

/// Extracts the base URL (scheme + host + port) from a full URL.
/// e.g. "http://192.168.1.100:52235/description.xml" → "http://192.168.1.100:52235"
pub fn extract_base_url(location: &str) -> String {
    // Find the third slash (end of scheme://host:port)
    if let Some(scheme_end) = location.find("://") {
        let after_scheme = &location[scheme_end + 3..];
        if let Some(path_start) = after_scheme.find('/') {
            return location[..scheme_end + 3 + path_start].to_string();
        }
    }
    location.to_string()
}

/// Finds a UPnP service control URL by matching the service type string.
/// Returns the absolute URL (base_url + controlURL).
pub fn find_control_url(desc: &DeviceDescription, service_type_fragment: &str, base_url: &str) -> Option<String> {
    let service_list = desc.device.service_list.as_ref()?;
    let service = service_list.services.iter().find(|s| s.service_type.contains(service_type_fragment))?;
    let control_url = service.control_url.trim();
    if control_url.starts_with("http://") || control_url.starts_with("https://") {
        Some(control_url.to_string())
    } else {
        Some(format!("{}{}", base_url, control_url))
    }
}

/// Fetches and parses the device description XML without printing device info.
pub async fn fetch_device_description_quiet(location: &str) -> Result<DeviceDescription, Box<dyn std::error::Error>> {
    let client = Client::new();
    let response = client.get(location).send().await?;
    if !response.status().is_success() {
        return Err(format!("Error fetching device description. Status: {}", response.status()).into());
    }
    let xml = response.text().await?;
    parse_device_description(&xml)
}

/// Fetches and parses the device description XML from the given location URL.
pub async fn fetch_device_description(location: &str) -> Result<DeviceDescription, Box<dyn std::error::Error>> {
    println!("Fetching device description from: {}", location);

    let client = Client::new();
    let response = client.get(location).send().await?;

    if !response.status().is_success() {
        return Err(format!("Error fetching device description. Status: {}", response.status()).into());
    }

    let xml = response.text().await?;

    let description = parse_device_description(&xml)?;

    println!("\nDevice Information:");
    println!("Name: {}", description.device.friendly_name);
    println!("Manufacturer: {}", description.device.manufacturer);
    println!("Model: {}", description.device.model_name);
    println!("UDN: {}\n", description.device.udn);

    if let Some(service_list) = &description.device.service_list {
        println!("Available services:");
        for service in &service_list.services {
            println!(" - Type: {}", service.service_type);
            println!("   Control: {}", service.control_url);
            println!("   SCPD: {}\n", service.scpd_url);
        }
    }

    Ok(description)
}
