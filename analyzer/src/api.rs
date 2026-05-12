use crate::aggregator::Aggregator;
use crate::diff;
use crate::leak_report;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Response, Server};

/// Blocking HTTP API server — run this on a dedicated thread (not inside tokio).
///
/// Endpoints:
///   GET  /snapshot          — per-line allocation stats, sorted by total bytes desc
///   GET  /leaks             — raw unfreed allocations
///   GET  /leak-report       — leaks grouped by function (?min_bytes=N to filter)
///   POST /baseline          — save current snapshot as diff baseline
///   GET  /diff              — diff between saved baseline and current snapshot
///   POST /reset             — clear all accumulated data
///   GET  /health            — liveness probe
pub fn serve(port: u16, aggregator: Arc<Aggregator>) {
    let server = Server::http(format!("127.0.0.1:{port}"))
        .unwrap_or_else(|e| panic!("Cannot bind API port {port}: {e}"));

    // Baseline snapshot for diff comparisons
    let baseline: Arc<Mutex<Option<Vec<crate::aggregator::LineStats>>>> =
        Arc::new(Mutex::new(None));

    eprintln!("[ferroalloc] API listening on http://127.0.0.1:{port}");

    for request in server.incoming_requests() {
        let url = request.url().to_string();
        let (status, body) = match (request.method(), url.as_str()) {
            (Method::Get, "/snapshot") => {
                let json = serde_json::to_string(&aggregator.snapshot()).unwrap_or_default();
                (200, json)
            }
            (Method::Get, "/leaks") => {
                let json = serde_json::to_string(&aggregator.live_leaks()).unwrap_or_default();
                (200, json)
            }
            (Method::Get, u) if u.starts_with("/leak-report") => {
                let min_bytes = parse_query_param(u, "min_bytes").unwrap_or(0);
                let report = leak_report::build(&aggregator, min_bytes);
                let json = serde_json::to_string(&report).unwrap_or_default();
                (200, json)
            }
            (Method::Post, "/baseline") => {
                *baseline.lock().unwrap() = Some(aggregator.snapshot());
                (200, r#"{"status":"baseline saved"}"#.to_string())
            }
            (Method::Get, "/diff") => {
                let guard = baseline.lock().unwrap();
                match guard.as_ref() {
                    Some(base) => {
                        let current = aggregator.snapshot();
                        let d = diff::compute(base, &current);
                        let json = serde_json::to_string(&d).unwrap_or_default();
                        (200, json)
                    }
                    None => (
                        400,
                        r#"{"error":"no baseline set — POST /baseline first"}"#.to_string(),
                    ),
                }
            }
            (Method::Post, "/reset") => {
                aggregator.reset();
                *baseline.lock().unwrap() = None;
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

/// Extract a numeric query param value from a URL like `/path?foo=42`.
fn parse_query_param(url: &str, key: &str) -> Option<usize> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next()? == key {
            return parts.next()?.parse().ok();
        }
    }
    None
}
