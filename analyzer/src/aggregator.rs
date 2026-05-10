use std::collections::HashMap;
use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use crate::dwarf::Resolver;

#[derive(Debug, Default, Clone, Serialize)]
pub struct LineStats {
    pub file: String,
    pub line: u32,
    pub function: String,
    pub alloc_count: u64,
    pub total_bytes: u64,
    /// Bytes currently live (not yet freed). Used for leak detection.
    pub live_bytes: i64,
}

#[derive(Debug, Default)]
struct Inner {
    by_line: HashMap<(String, u32), LineStats>,
    /// Tracks live allocations keyed by pointer for dealloc matching.
    live: HashMap<u64, (String, u32, usize)>,
}

pub struct Aggregator {
    inner: Mutex<Inner>,
}

impl Aggregator {
    pub fn new() -> Self {
        Self { inner: Mutex::new(Inner::default()) }
    }

    pub fn process(&self, event: &serde_json::Value, resolver: &Resolver) {
        let kind = event["kind"].as_str().unwrap_or("");
        let ptr  = event["ptr"].as_u64().unwrap_or(0);
        let size = event["size"].as_u64().unwrap_or(0) as usize;
        let frames: Vec<u64> = event["frames"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
            .unwrap_or_default();

        // Walk the call stack; use the first frame that maps to user source code
        let loc = frames.iter().find_map(|&ip| resolver.resolve(ip));
        let (file, line, function) = match loc {
            Some(l) => (
                l.file.unwrap_or_default(),
                l.line.unwrap_or(0),
                l.function.unwrap_or_default(),
            ),
            None => return,
        };

        let mut g = self.inner.lock().unwrap();
        let key = (file.clone(), line);

        match kind {
            "alloc" => {
                let entry = g.by_line.entry(key.clone()).or_insert_with(|| LineStats {
                    file: file.clone(),
                    line,
                    function: function.clone(),
                    ..Default::default()
                });
                entry.alloc_count += 1;
                entry.total_bytes += size as u64;
                entry.live_bytes  += size as i64;

                g.live.insert(ptr, (file, line, size));
            }
            "dealloc" => {
                if let Some((f, l, s)) = g.live.remove(&ptr) {
                    let entry = g.by_line.entry((f, l)).or_default();
                    entry.live_bytes -= s as i64;
                    if entry.live_bytes < 0 {
                        entry.live_bytes = 0;
                    }
                }
            }
            _ => {}
        }
    }

    /// Returns a snapshot of all per-line statistics.
    pub fn snapshot(&self) -> Vec<LineStats> {
        self.inner.lock().unwrap().by_line.values().cloned().collect()
    }

    /// Returns all allocations that have not been freed yet (potential leaks).
    pub fn live_leaks(&self) -> Vec<(u64, String, u32, usize)> {
        self.inner.lock().unwrap()
            .live
            .iter()
            .map(|(&ptr, (f, l, s))| (ptr, f.clone(), *l, *s))
            .collect()
    }
}
