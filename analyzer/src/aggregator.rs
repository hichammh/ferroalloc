use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Debug counters exposed via `/health`.
#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct Counters {
    pub events_received: u64,
    pub events_resolved: u64,
    pub events_dropped: u64,
}

// Fields of the aggregator rather than globals: two aggregators (or two tests
// running in parallel) must not share one set of counters.
#[derive(Debug, Default)]
struct AtomicCounters {
    received: AtomicU64,
    resolved: AtomicU64,
    dropped: AtomicU64,
}

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
    counters: AtomicCounters,
}

impl Aggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a pre-resolved allocation event from the probe.
    /// The event JSON contains (kind, ptr, size, file, line, function).
    pub fn process(&self, event: &serde_json::Value) {
        let kind = event["kind"].as_str().unwrap_or("");

        // Control event, not an allocation: the probe reporting its losses.
        if kind == "dropped" {
            self.counters
                .dropped
                .store(event["count"].as_u64().unwrap_or(0), Ordering::Relaxed);
            return;
        }

        let ptr = event["ptr"].as_u64().unwrap_or(0);
        let size = event["size"].as_u64().unwrap_or(0) as usize;
        let file = event["file"].as_str().unwrap_or("").to_string();
        let line = event["line"].as_u64().unwrap_or(0) as u32;
        let function = event["function"].as_str().unwrap_or("").to_string();

        // Counted before the unattributable ones are discarded below. Counting
        // after made received == resolved unconditionally, which left /health
        // blind to the most common failure: a build without debug symbols, where
        // events do arrive but carry no file.
        self.counters.received.fetch_add(1, Ordering::Relaxed);

        if file.is_empty() {
            return;
        }

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
                self.counters.resolved.fetch_add(1, Ordering::Relaxed);
            }
            "dealloc" => {
                if let Some((f, l, s)) = g.live.remove(&ptr) {
                    let entry = g
                        .by_line
                        .entry((f.clone(), l))
                        .or_insert_with(|| LineStats {
                            file: f,
                            line: l,
                            function: function.clone(),
                            ..Default::default()
                        });
                    entry.live_bytes = (entry.live_bytes - s as i64).max(0);
                }
                self.counters.resolved.fetch_add(1, Ordering::Relaxed);
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

    /// Snapshot of the diagnostic counters served by `/health`.
    pub fn counters(&self) -> Counters {
        Counters {
            events_received: self.counters.received.load(Ordering::Relaxed),
            events_resolved: self.counters.resolved.load(Ordering::Relaxed),
            events_dropped: self.counters.dropped.load(Ordering::Relaxed),
        }
    }

    /// Clears all accumulated data (useful between debug sessions).
    pub fn reset(&self) {
        let mut g = self.inner.lock().unwrap();
        g.by_line.clear();
        g.live.clear();
        self.counters.received.store(0, Ordering::Relaxed);
        self.counters.resolved.store(0, Ordering::Relaxed);
        self.counters.dropped.store(0, Ordering::Relaxed);
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

    fn alloc_event(
        ptr: u64,
        size: usize,
        file: &str,
        line: u32,
        function: &str,
    ) -> serde_json::Value {
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
    fn unresolved_events_are_counted_as_received_but_not_resolved() {
        // The gap between the two counters is what tells the user their build has
        // no debug symbols, so an event with no file must still be counted.
        let agg = Aggregator::new();

        agg.process(&alloc_event(0x1000, 64, "", 0, ""));

        assert_eq!(agg.counters().events_received, 1);
        assert_eq!(agg.counters().events_resolved, 0);
        assert!(agg.snapshot().is_empty());
    }

    #[test]
    fn dropped_control_event_is_not_an_allocation() {
        let agg = Aggregator::new();

        agg.process(&serde_json::json!({ "kind": "dropped", "count": 4_096 }));

        assert_eq!(agg.counters().events_dropped, 4_096);
        assert_eq!(agg.counters().events_received, 0);
        assert!(agg.snapshot().is_empty());
    }

    #[test]
    fn dealloc_after_reset_does_not_create_a_phantom_line() {
        // A reset between the alloc and its dealloc used to insert a defaulted
        // LineStats — empty file, line 0 — straight into /snapshot.
        let agg = Aggregator::new();
        agg.process(&alloc_event(0x1000, 128, "main.rs", 10, "foo"));
        agg.process(&dealloc_event(0x1000, 128, "main.rs", 10));

        let snap = agg.snapshot();
        for entry in &snap {
            assert!(
                !entry.file.is_empty(),
                "snapshot entry with no file: {entry:?}"
            );
            assert_ne!(entry.line, 0, "snapshot entry with line 0: {entry:?}");
        }
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].file, "main.rs");
        assert_eq!(snap[0].line, 10);
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
