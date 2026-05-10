use crate::aggregator::Aggregator;
use std::io::Cursor;
use std::sync::Arc;
use tiny_http::{Header, Method, Response, Server};

/// Blocking HTTP API server — run this on a dedicated thread (not inside tokio).
///
/// Endpoints:
///   GET  /snapshot  — per-line allocation stats, sorted by total bytes desc
///   GET  /leaks     — allocations still live (not yet freed)
///   POST /reset     — clear all accumulated data
///   GET  /health    — liveness probe
pub fn serve(port: u16, aggregator: Arc<Aggregator>) {
    let server = Server::http(format!("127.0.0.1:{port}"))
        .unwrap_or_else(|e| panic!("Cannot bind API port {port}: {e}"));

    eprintln!("[ferroalloc] API listening on http://127.0.0.1:{port}");

    for request in server.incoming_requests() {
        let (status, body) = match (request.method(), request.url()) {
            (Method::Get, "/snapshot") => {
                let json = serde_json::to_string(&aggregator.snapshot()).unwrap_or_default();
                (200, json)
            }
            (Method::Get, "/leaks") => {
                let json = serde_json::to_string(&aggregator.live_leaks()).unwrap_or_default();
                (200, json)
            }
            (Method::Post, "/reset") => {
                aggregator.reset();
                (200, r#"{"status":"reset"}"#.to_string())
            }
            (Method::Get, "/health") => (200, r#"{"status":"ok"}"#.to_string()),
            _ => (404, r#"{"error":"not found"}"#.to_string()),
        };

        let len = body.len();
        let response = Response::new(
            tiny_http::StatusCode(status),
            vec![
                Header::from_bytes("Content-Type", "application/json").unwrap(),
                Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap(),
                Header::from_bytes("Content-Length", len.to_string().as_str()).unwrap(),
            ],
            Cursor::new(body),
            Some(len),
            None,
        );

        let _ = request.respond(response);
    }
}
