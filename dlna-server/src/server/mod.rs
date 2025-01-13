// src/server/mod.rs
use hyper::{Body, Request, Response, Server};
use hyper::service::{make_service_fn, service_fn};

async fn serve_description(_req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
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

pub async fn start_http_server(port: u16) {
    let addr = ([0, 0, 0, 0], port).into();
    println!("Iniciando servidor HTTP na porta {}", port);

    let make_svc = make_service_fn(|_conn| {
        async { Ok::<_, hyper::Error>(service_fn(serve_description)) }
    });

    let server = Server::bind(&addr).serve(make_svc);

    if let Err(e) = server.await {
        println!("Erro no servidor HTTP: {}", e);
    }
}
