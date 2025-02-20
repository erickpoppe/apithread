use hyper::{Body, Request, Response, Server, Method, StatusCode};
use hyper::service::{make_service_fn, service_fn};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::path::Path;
use tokio::net::UnixListener;
use tokio::sync::mpsc::{self, Sender};
use std::sync::Arc;

// Configuration structs (simplified)
#[derive(Serialize, Deserialize, Debug)]
struct VmConfig {
    memory_mb: u64,
    vcpu_count: u8,
}

// Commands to send to the VMM thread (simulated)
#[derive(Debug)]
enum VmmCommand {
    SetConfig(VmConfig),
    StartVm,
}

// API Thread struct
struct ApiThread {
    command_tx: Sender<VmmCommand>, // Channel to communicate with VMM thread
}

impl ApiThread {
    fn new(command_tx: Sender<VmmCommand>) -> Self {
        ApiThread { command_tx }
    }

    // Handle incoming HTTP requests
    async fn handle_request(
        &self,
        req: Request<Body>,
    ) -> Result<Response<Body>, hyper::Error> {
        match (req.method(), req.uri().path()) {
            // GET /status - Check API status
            (&Method::GET, "/status") => {
                Ok(Response::new(Body::from("API is running")))
            }

            // PUT /config - Configure VM (e.g., memory, vCPUs)
            (&Method::PUT, "/config") => {
                let body_bytes = hyper::body::to_bytes(req.into_body()).await?;
                let config: VmConfig = match serde_json::from_slice(&body_bytes) {
                    Ok(cfg) => cfg,
                    Err(_) => {
                        return Ok(Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(Body::from("Invalid config JSON"))
                            .unwrap());
                    }
                };
                // Send config to VMM thread
                self.command_tx
                    .send(VmmCommand::SetConfig(config))
                    .await
                    .map_err(|_| hyper::Error::from(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Failed to send command",
                    )))?;
                Ok(Response::new(Body::from("Config updated")))
            }

            // POST /start - Start the VM
            (&Method::POST, "/start") => {
                self.command_tx
                    .send(VmmCommand::StartVm)
                    .await
                    .map_err(|_| hyper::Error::from(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Failed to send start command",
                    )))?;
                Ok(Response::new(Body::from("VM start requested")))
            }

            // Unknown route
            _ => {
                Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("Not Found"))
                    .unwrap())
            }
        }
    }
}

// Main async function to run the API server
async fn run_api_server(api: Arc<ApiThread>, socket_path: &str) {
    // Remove socket if it exists
    let _ = std::fs::remove_file(socket_path);

    // Bind to Unix socket
    let listener = UnixListener::bind(socket_path).expect("Failed to bind Unix socket");
    let local_addr = listener.local_addr().unwrap();
    println!("API server listening on {:?}", local_addr);

    // Hyper service setup
    let make_svc = make_service_fn(move |_conn| {
        let api = api.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                api.handle_request(req)
            }))
        }
    });

    // Convert Tokio UnixListener to Hyper-compatible stream
    let incoming = hyper::server::accept::from_stream(tokio_stream::wrappers::UnixListenerStream::new(listener));
    let server = Server::builder(incoming).serve(make_svc);

    if let Err(e) = server.await {
        eprintln!("Server error: {}", e);
    }
}

#[tokio::main]
async fn main() {
    // Simulate VMM thread communication with a channel
    let (command_tx, mut command_rx) = mpsc::channel::<VmmCommand>(32);

    // Spawn a mock VMM thread to handle commands
    tokio::spawn(async move {
        while let Some(cmd) = command_rx.recv().await {
            match cmd {
                VmmCommand::SetConfig(config) => {
                    println!("VMM received config: {:?}", config);
                    // In Firecracker, this would update VM state
                }
                VmmCommand::StartVm => {
                    println!("VMM starting VM...");
                    // In Firecracker, this would kick off the vCPU threads
                }
            }
        }
    });

    // Create and run the API thread
    let api = Arc::new(ApiThread::new(command_tx));
    let socket_path = "/tmp/firecracker-api.sock";
    run_api_server(api, socket_path).await;
}
