use crate::aggregator::Aggregator;
use crate::dwarf::Resolver;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Minimal HTTP/1.1 server exposing the aggregated data to the VS Code extension.
/// Endpoints:
///   GET /snapshot  — per-line allocation stats
///   GET /leaks     — allocations still live (not yet freed)
///   GET /health    — liveness probe
pub async fn serve(port: u16, aggregator: Arc<Aggregator>, _resolver: Arc<Resolver>) {
    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();

    loop {
        let (mut socket, _) = listener.accept().await.unwrap();
        let agg = Arc::clone(&aggregator);

        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let request = String::from_utf8_lossy(&buf);

            let (status, body) = if request.starts_with("GET /snapshot") {
                let json = serde_json::to_string(&agg.snapshot()).unwrap_or_default();
                ("200 OK", json)
            } else if request.starts_with("GET /leaks") {
                let leaks: Vec<_> = agg.live_leaks().into_iter().map(|(ptr, f, l, s)| {
                    serde_json::json!({ "ptr": ptr, "file": f, "line": l, "size": s })
                }).collect();
                ("200 OK", serde_json::to_string(&leaks).unwrap_or_default())
            } else if request.starts_with("GET /health") {
                ("200 OK", r#"{"status":"ok"}"#.to_string())
            } else {
                ("404 Not Found", r#"{"error":"not found"}"#.to_string())
            };

            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });
    }
}
