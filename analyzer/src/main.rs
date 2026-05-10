mod aggregator;
mod api;
mod dwarf;

use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;

const PROBE_PORT: u16 = 7777;
const API_PORT: u16 = 7778;

#[tokio::main]
async fn main() {
    let binary_path = std::env::args()
        .nth(1)
        .expect("Usage: ferroalloc-analyzer <path-to-debug-binary>");

    let resolver = Arc::new(
        dwarf::Resolver::new(&binary_path)
            .unwrap_or_else(|e| panic!("Failed to load DWARF debug info: {e}")),
    );

    let aggregator = Arc::new(aggregator::Aggregator::new());

    // Spawn the HTTP API server on a dedicated blocking thread (tiny_http is synchronous)
    let agg_api = Arc::clone(&aggregator);
    std::thread::spawn(move || api::serve(API_PORT, agg_api));

    // Listen for probe events over TCP
    let listener = TcpListener::bind(format!("127.0.0.1:{PROBE_PORT}"))
        .await
        .unwrap_or_else(|e| panic!("Cannot bind probe port {PROBE_PORT}: {e}"));

    eprintln!("[ferroalloc] Probe listener on 127.0.0.1:{PROBE_PORT}");

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
