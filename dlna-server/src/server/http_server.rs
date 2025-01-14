// src/server/mod.rs
use hyper::{Body, Request, Server};
use hyper::service::{make_service_fn, service_fn};
use crate::server::endpoints::handle_request;
use crate::config::Config;
use std::sync::Arc;

pub async fn start_http_server(port: u16, config: Config) {
    let addr = ([0, 0, 0, 0], port).into();
    println!("Starting HTTP server on port {}", port);

    // Uses Arc to allow safe sharing of `config`
    let shared_config = Arc::new(config);

    let make_svc = make_service_fn(move |_conn| {
        // Clones the Arc for each connection
        let config = Arc::clone(&shared_config);

        async move {
            Ok::<_, hyper::Error>(service_fn(move |req: Request<Body>| {
                // Clones the Arc for use in the handler
                let config = Arc::clone(&config);
                async move { handle_request(req, &config).await }
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    if let Err(e) = server.await {
        println!("HTTP server error: {}", e);
    }
}
