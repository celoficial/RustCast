use std::path::Path;
use std::io::stdin;

mod config;
mod discovery;
mod server;
mod media;

use config::Config;
use discovery::discovery::discover_ssdp;
use discovery::device::fetch_device_description;
use server::http_server::start_http_server;
use media::manager::list_media_files;
use media::stream::stream_media;

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
                // let mut input = String::new();
                // stdin().read_line(&mut input)?;
                // let choice: usize = input.trim().parse().unwrap_or(0);

                // if choice == 0 {
                //     println!("Saindo...");
                //     return Ok(());
                // }

                // if let Some(selected_device) = devices.get(choice - 1) {
                //     if let Some(location) = selected_device.get("LOCATION") {
                //         println!("Você selecionou o dispositivo com LOCATION: {}", location);
                //         fetch_device_description(location).await?;
                //     } else {
                //         println!("Dispositivo selecionado não possui LOCATION.");
                //     }
                // } else {
                //     println!("Dispositivo inválido.");
                // }

                let mut input = String::new();
                stdin().read_line(&mut input)?;
                let device_choice: usize = input.trim().parse().unwrap_or(0);

                if device_choice == 0 {
                    println!("Saindo...");
                    return Ok(());
                }

                let selected_device = devices
                    .get(device_choice - 1)
                    .and_then(|device| device.get("LOCATION"))
                    .ok_or("Dispositivo inválido ou sem LOCATION.")?;

                // Obtém a descrição do dispositivo selecionado
                println!("Obtendo descrição do dispositivo...");
                if let Err(e) = fetch_device_description(selected_device).await {
                    eprintln!("Erro ao obter a descrição do dispositivo: {}", e);
                } else {
                    println!("Descrição do dispositivo obtida com sucesso.");
                }

                println!("Você selecionou o dispositivo: {}", selected_device);

                // Pergunta ao usuário qual arquivo de mídia deseja transmitir
                println!("\nEscolha um arquivo de mídia pelo número (ou digite '0' para sair):");
                input.clear();
                stdin().read_line(&mut input)?;
                let media_choice: usize = input.trim().parse().unwrap_or(0);

                if media_choice == 0 {
                    println!("Saindo...");
                    return Ok(());
                }

                let selected_media = media_files
                .get(media_choice - 1)
                .ok_or("Arquivo de mídia inválido.")?;

                println!("Você selecionou o arquivo de mídia: {}", selected_media.name);

                // Inicia o streaming para o dispositivo DLNA
                println!("Iniciando a transmissão para o dispositivo: {}", selected_device);
                stream_media(selected_device, selected_media).await?;

                println!("Transmissão concluída com sucesso!");
            }
        }
        Err(e) => {
            println!("Erro ao descobrir dispositivos SSDP: {}", e);
        }
    }

    // Aguarda o término do programa (Ctrl+C para finalizar)
    tokio::signal::ctrl_c().await?;
    println!("Encerrando o servidor HTTP...");

    // Cancela a tarefa do servidor HTTP
    //server_task.abort();

    Ok(())
}

