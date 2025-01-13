mod config;
mod discovery;
mod server;
mod utils;
mod media;

use config::Config;
use discovery::discovery::discover_ssdp;
use discovery::device::fetch_device_description;
use server::http_server::start_http_server;
use media::manager::list_media_files;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env();
    println!("Iniciando o servidor DLNA: {}", config.friendly_name);

    // Verifica se o diretório de mídia existe
    if !Path::new(&config.media_directory).exists() {
        eprintln!("Erro: O diretório de mídia configurado '{}' não existe.", config.media_directory);
        return Err("Diretório de mídia inválido".into());
    }

    // Lista os arquivos de mídia
    let media_files = list_media_files(&config.media_directory);
    if media_files.is_empty() {
        println!("Nenhum arquivo de mídia encontrado no diretório: {}", config.media_directory);
    } else {
        println!("Arquivos de mídia encontrados:");
        for file in &media_files {
            println!("- {}", file.name);
        }
    }

    // Clona a configuração para uso no servidor HTTP
    let server_config = config.clone();

    // Inicia o servidor HTTP em uma thread separada
    tokio::spawn(async move {
        println!("Iniciando servidor HTTP na porta: {}", server_config.http_port);
        start_http_server(server_config.http_port, server_config).await;
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


