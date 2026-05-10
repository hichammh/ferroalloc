use crate::dwarf::Resolver;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Default, Clone, Serialize)]
pub struct LineStats {
    pub file: String,
    pub line: u32,
    pub function: String,
    pub alloc_count: u64,
    pub total_bytes: u64,
    /// Bytes currently live (allocated but not yet freed). Non-zero values indicate potential leaks.
    pub live_bytes: i64,
}

#[derive(Debug, Default)]
struct Inner {
    by_line: HashMap<(String, u32), LineStats>,
    /// Tracks live allocations keyed by pointer for dealloc matching.
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

    pub fn process(&self, event: &serde_json::Value, resolver: &Resolver) {
        let kind = event["kind"].as_str().unwrap_or("");
        let ptr = event["ptr"].as_u64().unwrap_or(0);
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
                entry.live_bytes += size as i64;
                g.live.insert(ptr, (file, line, size));
            }
            "dealloc" => {
                if let Some((f, l, s)) = g.live.remove(&ptr) {
                    let entry = g.by_line.entry((f, l)).or_default();
                    entry.live_bytes = (entry.live_bytes - s as i64).max(0);
                }
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

    fn make_event(kind: &str, ptr: u64, size: u64, frames: &[u64]) -> serde_json::Value {
        serde_json::json!({
            "kind": kind,
            "ptr": ptr,
            "size": size,
            "frames": frames,
        })
    }

    struct FakeResolver {
        file: String,
        line: u32,
        function: String,
    }

    impl FakeResolver {
        fn new(file: &str, line: u32, function: &str) -> Self {
            Self {
                file: file.to_string(),
                line,
                function: function.to_string(),
            }
        }
    }

    // A minimal stand-in for Resolver that always returns the same location
    fn process_with_loc(
        agg: &Aggregator,
        kind: &str,
        ptr: u64,
        size: usize,
        file: &str,
        line: u32,
        function: &str,
    ) {
        let mut g = agg.inner.lock().unwrap();
        let key = (file.to_string(), line);
        match kind {
            "alloc" => {
                let entry = g.by_line.entry(key.clone()).or_insert_with(|| LineStats {
                    file: file.to_string(),
                    line,
                    function: function.to_string(),
                    ..Default::default()
                });
                entry.alloc_count += 1;
                entry.total_bytes += size as u64;
                entry.live_bytes += size as i64;
                g.live.insert(ptr, (file.to_string(), line, size));
            }
            "dealloc" => {
                if let Some((f, l, s)) = g.live.remove(&ptr) {
                    let entry = g.by_line.entry((f, l)).or_default();
                    entry.live_bytes = (entry.live_bytes - s as i64).max(0);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn alloc_increments_counts() {
        let agg = Aggregator::new();
        process_with_loc(&agg, "alloc", 0x1000, 128, "main.rs", 10, "foo");
        process_with_loc(&agg, "alloc", 0x2000, 64, "main.rs", 10, "foo");

        let snap = agg.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].alloc_count, 2);
        assert_eq!(snap[0].total_bytes, 192);
        assert_eq!(snap[0].live_bytes, 192);
    }

    #[test]
    fn dealloc_reduces_live_bytes() {
        let agg = Aggregator::new();
        process_with_loc(&agg, "alloc", 0x1000, 128, "main.rs", 10, "foo");
        process_with_loc(&agg, "dealloc", 0x1000, 128, "main.rs", 10, "foo");

        let snap = agg.snapshot();
        assert_eq!(snap[0].live_bytes, 0);
        assert_eq!(snap[0].total_bytes, 128); // historical total unchanged
    }

    #[test]
    fn live_leaks_returns_unfreed_allocations() {
        let agg = Aggregator::new();
        process_with_loc(&agg, "alloc", 0x1000, 64, "main.rs", 5, "bar");
        process_with_loc(&agg, "alloc", 0x2000, 32, "main.rs", 5, "bar");
        process_with_loc(&agg, "dealloc", 0x1000, 64, "main.rs", 5, "bar");

        let leaks = agg.live_leaks();
        assert_eq!(leaks.len(), 1);
        assert_eq!(leaks[0].ptr, 0x2000);
        assert_eq!(leaks[0].size, 32);
    }

    #[test]
    fn reset_clears_all_data() {
        let agg = Aggregator::new();
        process_with_loc(&agg, "alloc", 0x1000, 64, "main.rs", 1, "baz");
        agg.reset();

        assert!(agg.snapshot().is_empty());
        assert!(agg.live_leaks().is_empty());
    }

    #[test]
    fn snapshot_sorted_by_total_bytes_desc() {
        let agg = Aggregator::new();
        process_with_loc(&agg, "alloc", 0x1000, 64, "a.rs", 1, "small");
        process_with_loc(&agg, "alloc", 0x2000, 1024, "b.rs", 2, "large");
        process_with_loc(&agg, "alloc", 0x3000, 256, "c.rs", 3, "medium");

        let snap = agg.snapshot();
        assert_eq!(snap[0].total_bytes, 1024);
        assert_eq!(snap[1].total_bytes, 256);
        assert_eq!(snap[2].total_bytes, 64);
    }
}
