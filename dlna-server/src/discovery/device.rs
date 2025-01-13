// use reqwest::Client;

// pub async fn fetch_device_description(location: &str) -> Result<(), Box<dyn std::error::Error>> {
//     println!("Obtendo descrição do dispositivo de: {}", location);

//     // Faz uma requisição HTTP para o endereço fornecido
//     let client = Client::new();
//     let response = client.get(location).send().await?;

//     if response.status().is_success() {
//         let xml = response.text().await?;
//         println!("Descrição do dispositivo obtida:\n{}", xml);

//         // Opcional: Parsear o XML
//         // Use crates como `quick-xml` ou `serde-xml-rs` para processar o XML
//     } else {
//         println!("Erro ao obter descrição do dispositivo. Status: {}", response.status());
//     }

//     Ok(())
// }

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

// Função para parsing
pub fn parse_device_description(xml: &str) -> Result<DeviceDescription, Box<dyn std::error::Error>> {
    let description: DeviceDescription = from_str(xml)?;
    Ok(description)
}

// Função para obter e processar a descrição do dispositivo
pub async fn fetch_device_description(location: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Obtendo descrição do dispositivo de: {}", location);

    let client = Client::new();
    let response = client.get(location).send().await?;

    if response.status().is_success() {
        let xml = response.text().await?;
        println!("Descrição do dispositivo obtida:\n{}", xml);

        // Parse do XML
        match parse_device_description(&xml) {
            Ok(description) => {
                println!("Informações do dispositivo:");
                println!("Nome: {}", description.device.friendly_name);
                println!("Fabricante: {}", description.device.manufacturer);
                println!("Modelo: {}", description.device.model_name);
                println!("UDN: {}", description.device.udn);

                if let Some(service_list) = description.device.service_list {
                    println!("Serviços disponíveis:");
                    for service in service_list.services {
                        println!(" - Tipo: {}", service.service_type);
                        println!("   Controle: {}", service.control_url);
                        println!("   SCPD: {}", service.scpd_url);
                    }
                }
            }
            Err(e) => {
                println!("Erro ao parsear o XML: {}", e);
            }
        }
    } else {
        println!("Erro ao obter descrição do dispositivo. Status: {}", response.status());
    }

    Ok(())
}

