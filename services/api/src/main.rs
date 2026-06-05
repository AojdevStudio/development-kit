//! Binary entrypoint for the cloud API. Binds the router from `api::app` and
//! serves it. The acceptance criterion "API responds 200 on a health route" is
//! proven by the library test (`tests/health.rs`); this binary is the runnable
//! dev server.

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = std::env::var("API_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8787);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("api listening on http://{addr}");
    axum::serve(listener, api::app()).await?;
    Ok(())
}
