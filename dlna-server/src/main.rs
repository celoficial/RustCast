// src/main.rs
mod config;
mod discovery;
mod server;
mod utils;

use config::Config;
use discovery::discovery::discover_ssdp;
use discovery::device::fetch_device_description;
use server::start_http_server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env();
    println!("Iniciando o servidor DLNA: {}", config.friendly_name);

    // Inicia o servidor HTTP em uma thread separada
    tokio::spawn(async move {
        start_http_server(config.http_port).await;
    });

    // Chama a função de descoberta de dispositivos
    match discover_ssdp(&config.multicast_address, config.multicast_port).await {
        Ok(devices) => {
            if devices.is_empty() {
                println!("Nenhum dispositivo encontrado.");
            } else {
                println!("\nDispositivos MediaRenderer encontrados:");
                for (i, device) in devices.iter().enumerate() {
                    if let Some(location) = device.get("LOCATION") {
                        println!("{}) {}", i + 1, location);
                    } else {
                        println!("{}) Dispositivo sem LOCATION.", i + 1);
                    }
                }

                // Pergunta ao usuário qual dispositivo ele quer usar
                println!("\nEscolha um dispositivo pelo número (ou digite '0' para sair):");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                let choice: usize = input.trim().parse().unwrap_or(0);

                if choice == 0 {
                    println!("Saindo...");
                    return Ok(());
                }

                if let Some(selected_device) = devices.get(choice - 1) {
                    if let Some(location) = selected_device.get("LOCATION") {
                        println!("Você selecionou o dispositivo com LOCATION: {}", location);
                        fetch_device_description(location).await?;
                    } else {
                        println!("Dispositivo selecionado não possui LOCATION.");
                    }
                } else {
                    println!("Dispositivo inválido.");
                }
            }
        }
        Err(e) => {
            println!("Erro ao descobrir dispositivos SSDP: {}", e);
        }
    }

    Ok(())
}

