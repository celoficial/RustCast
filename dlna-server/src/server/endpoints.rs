use hyper::{Body, Request, Response};
use serde_json::json;
use crate::config::Config;
use crate::media::manager::list_media_files;

/// Função para lidar com requisições HTTP
pub async fn handle_request(
    req: Request<Body>,
    config: &Config,
) -> Result<Response<Body>, hyper::Error> {
    match req.uri().path() {
        "/description.xml" => {
            // Responde com o XML de descrição
            let xml = r#"<?xml version="1.0"?>
                                <root xmlns="urn:schemas-upnp-org:device-1-0">
                                    <specVersion>
                                        <major>1</major>
                                        <minor>0</minor>
                                    </specVersion>
                                    <device>
                                        <deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>
                                        <friendlyName>Rust DLNA Server</friendlyName>
                                        <manufacturer>Rust Inc.</manufacturer>
                                        <manufacturerURL>http://www.rust-dlna.com</manufacturerURL>
                                        <modelName>DLNA Server v1</modelName>
                                        <modelDescription>A Rust-based DLNA Media Server</modelDescription>
                                        <modelURL>http://www.rust-dlna.com/models/server</modelURL>
                                        <UDN>uuid:12345678-1234-1234-1234-123456789abc</UDN>
                                    </device>
                                </root>"#;
            Ok(Response::new(Body::from(xml)))
        }

        "/media" => {
            let media_files = list_media_files(&config.media_directory);
            let json = json!(media_files); // Converte a lista de arquivos em JSON
            Ok(Response::new(Body::from(json.to_string())))
        }
        
        _ => Ok(Response::builder()
            .status(404)
            .body(Body::from("Not Found"))
            .unwrap()),
    }
}

