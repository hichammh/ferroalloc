mod dwarf;
mod aggregator;
mod api;

use tokio::net::TcpListener;
use tokio::io::{AsyncBufReadExt, BufReader};
use std::sync::Arc;

const PROBE_PORT: u16 = 7777;
const API_PORT: u16 = 7778;

#[tokio::main]
async fn main() {
    let binary_path = std::env::args().nth(1)
        .expect("Usage: ferroalloc-analyzer <path-to-debug-binary>");

    let resolver = Arc::new(
        dwarf::Resolver::new(&binary_path).expect("Failed to load DWARF debug info")
    );

    let aggregator = Arc::new(aggregator::Aggregator::new());

    // Spawn the HTTP API server consumed by the VS Code extension
    let agg_api = Arc::clone(&aggregator);
    let res_api = Arc::clone(&resolver);
    tokio::spawn(async move {
        api::serve(API_PORT, agg_api, res_api).await;
    });

    let listener = TcpListener::bind(format!("127.0.0.1:{PROBE_PORT}"))
        .await
        .expect("Cannot bind probe port");

    eprintln!("[ferroalloc] Probe listener on :{PROBE_PORT} | API on :{API_PORT}");

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        let agg = Arc::clone(&aggregator);
        let res = Arc::clone(&resolver);

        tokio::spawn(async move {
            let reader = BufReader::new(socket);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
                    agg.process(&event, &res);
                }
            }
        });
    }
}
