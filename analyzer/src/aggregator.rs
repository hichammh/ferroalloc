use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

// Debug counters exposed via /health
pub static EVENTS_RECEIVED: AtomicU64 = AtomicU64::new(0);
pub static EVENTS_RESOLVED: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default, Clone, Serialize)]
pub struct LineStats {
    pub file: String,
    pub line: u32,
    pub function: String,
    pub alloc_count: u64,
    pub total_bytes: u64,
    /// Bytes currently live (not yet freed). Non-zero = potential leak.
    pub live_bytes: i64,
}

#[derive(Debug, Default)]
struct Inner {
    by_line: HashMap<(String, u32), LineStats>,
    /// Live allocations keyed by pointer for dealloc matching.
    live: HashMap<u64, (String, u32, usize)>,
}

#[derive(Default)]
pub struct Aggregator {
    inner: Mutex<Inner>,
}

impl Aggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a pre-resolved allocation event from the probe.
    /// The event JSON contains (kind, ptr, size, file, line, function).
    pub fn process(&self, event: &serde_json::Value) {
        let kind = event["kind"].as_str().unwrap_or("");
        let ptr = event["ptr"].as_u64().unwrap_or(0);
        let size = event["size"].as_u64().unwrap_or(0) as usize;
        let file = event["file"].as_str().unwrap_or("").to_string();
        let line = event["line"].as_u64().unwrap_or(0) as u32;
        let function = event["function"].as_str().unwrap_or("").to_string();

        if file.is_empty() {
            return;
        }

        EVENTS_RECEIVED.fetch_add(1, Ordering::Relaxed);

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
                entry.live_bytes += size as i64;
                g.live.insert(ptr, (file, line, size));
                EVENTS_RESOLVED.fetch_add(1, Ordering::Relaxed);
            }
            "dealloc" => {
                if let Some((f, l, s)) = g.live.remove(&ptr) {
                    let entry = g.by_line.entry((f, l)).or_default();
                    entry.live_bytes = (entry.live_bytes - s as i64).max(0);
                }
                EVENTS_RESOLVED.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    /// Returns per-line stats sorted by total bytes allocated descending.
    pub fn snapshot(&self) -> Vec<LineStats> {
        let mut stats: Vec<_> = self
            .inner
            .lock()
            .unwrap()
            .by_line
            .values()
            .cloned()
            .collect();
        stats.sort_unstable_by_key(|s| std::cmp::Reverse(s.total_bytes));
        stats
    }

    /// Returns all allocations that have not been freed yet (potential leaks).
    pub fn live_leaks(&self) -> Vec<LeakEntry> {
        self.inner
            .lock()
            .unwrap()
            .live
            .iter()
            .map(|(&ptr, (f, l, s))| LeakEntry {
                ptr,
                file: f.clone(),
                line: *l,
                size: *s,
            })
            .collect()
    }

    /// Clears all accumulated data (useful between debug sessions).
    pub fn reset(&self) {
        let mut g = self.inner.lock().unwrap();
        g.by_line.clear();
        g.live.clear();
        EVENTS_RECEIVED.store(0, Ordering::Relaxed);
        EVENTS_RESOLVED.store(0, Ordering::Relaxed);
    }
}

#[derive(Serialize)]
pub struct LeakEntry {
    pub ptr: u64,
    pub file: String,
    pub line: u32,
    pub size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alloc_event(ptr: u64, size: usize, file: &str, line: u32, function: &str) -> serde_json::Value {
        serde_json::json!({
            "kind": "alloc",
            "ptr": ptr,
            "size": size,
            "file": file,
            "line": line,
            "function": function,
        })
    }

    fn dealloc_event(ptr: u64, size: usize, file: &str, line: u32) -> serde_json::Value {
        serde_json::json!({
            "kind": "dealloc",
            "ptr": ptr,
            "size": size,
            "file": file,
            "line": line,
            "function": "",
        })
    }

    #[test]
    fn alloc_increments_counts() {
        let agg = Aggregator::new();
        agg.process(&alloc_event(0x1000, 128, "main.rs", 10, "foo"));
        agg.process(&alloc_event(0x2000, 64, "main.rs", 10, "foo"));

        let snap = agg.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].alloc_count, 2);
        assert_eq!(snap[0].total_bytes, 192);
        assert_eq!(snap[0].live_bytes, 192);
    }

    #[test]
    fn dealloc_reduces_live_bytes() {
        let agg = Aggregator::new();
        agg.process(&alloc_event(0x1000, 128, "main.rs", 10, "foo"));
        agg.process(&dealloc_event(0x1000, 128, "main.rs", 10));

        let snap = agg.snapshot();
        assert_eq!(snap[0].live_bytes, 0);
        assert_eq!(snap[0].total_bytes, 128);
    }

    #[test]
    fn live_leaks_returns_unfreed_allocations() {
        let agg = Aggregator::new();
        agg.process(&alloc_event(0x1000, 64, "main.rs", 5, "bar"));
        agg.process(&alloc_event(0x2000, 32, "main.rs", 5, "bar"));
        agg.process(&dealloc_event(0x1000, 64, "main.rs", 5));

        let leaks = agg.live_leaks();
        assert_eq!(leaks.len(), 1);
        assert_eq!(leaks[0].ptr, 0x2000);
        assert_eq!(leaks[0].size, 32);
    }

    #[test]
    fn reset_clears_all_data() {
        let agg = Aggregator::new();
        agg.process(&alloc_event(0x1000, 64, "main.rs", 1, "baz"));
        agg.reset();

        assert!(agg.snapshot().is_empty());
        assert!(agg.live_leaks().is_empty());
    }

    #[test]
    fn snapshot_sorted_by_total_bytes_desc() {
        let agg = Aggregator::new();
        agg.process(&alloc_event(0x1000, 64, "a.rs", 1, "small"));
        agg.process(&alloc_event(0x2000, 1024, "b.rs", 2, "large"));
        agg.process(&alloc_event(0x3000, 256, "c.rs", 3, "medium"));

        let snap = agg.snapshot();
        assert_eq!(snap[0].total_bytes, 1024);
        assert_eq!(snap[1].total_bytes, 256);
        assert_eq!(snap[2].total_bytes, 64);
    }
}
