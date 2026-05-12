mod aggregator;
mod api;
mod diff;
mod leak_report;

use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;

const PROBE_PORT: u16 = 7777;
const API_PORT: u16 = 7778;

#[tokio::main]
async fn main() {
    let aggregator = Arc::new(aggregator::Aggregator::new());

    // Spawn the HTTP API server on a dedicated blocking thread
    let agg_api = Arc::clone(&aggregator);
    std::thread::spawn(move || api::serve(API_PORT, agg_api));

    let listener = TcpListener::bind(format!("127.0.0.1:{PROBE_PORT}"))
        .await
        .unwrap_or_else(|e| panic!("Cannot bind probe port {PROBE_PORT}: {e}"));

    eprintln!("[ferroalloc] Probe listener on 127.0.0.1:{PROBE_PORT}");
    eprintln!("[ferroalloc] API on http://127.0.0.1:{API_PORT}");

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        let agg = Arc::clone(&aggregator);

        tokio::spawn(async move {
            let reader = BufReader::new(socket);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
                    agg.process(&event);
                }
            }
        });
    }
}
