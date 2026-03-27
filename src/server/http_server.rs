use crate::config::Config;
use crate::server::endpoints::handle_request;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::Request;
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

pub async fn start_http_server(port: u16, config: Config) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("Starting HTTP server on port {}", port);

    let listener = match TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("Failed to bind HTTP server: {}", err);
            return;
        }
    };

    // Share config across all accepted connections without cloning the payload.
    let shared_config = Arc::new(config);

    loop {
        let (stream, peer_addr) = match listener.accept().await {
            Ok(connection) => connection,
            Err(err) => {
                eprintln!("Failed to accept HTTP connection: {}", err);
                continue;
            }
        };

        if let Err(err) = stream.set_nodelay(true) {
            eprintln!("Failed to enable TCP_NODELAY for {}: {}", peer_addr, err);
        }

        let io = TokioIo::new(stream);
        let config = Arc::clone(&shared_config);

        tokio::spawn(async move {
            // Clone the Arc per request so the handler can borrow config safely.
            let service = service_fn(move |req: Request<Incoming>| {
                let config = Arc::clone(&config);
                async move { handle_request(req, &config).await }
            });

            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                eprintln!("HTTP connection error ({}): {}", peer_addr, err);
            }
        });
    }
}
