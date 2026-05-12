use crate::aggregator::{Aggregator, LeakEntry};
use serde::Serialize;
use std::collections::HashMap;

/// Leak report grouped by function, sorted by total leaked bytes descending.
#[derive(Serialize)]
pub struct LeakReport {
    pub total_leaked_bytes: usize,
    pub total_leak_count: usize,
    pub groups: Vec<LeakGroup>,
}

#[derive(Serialize)]
pub struct LeakGroup {
    pub function: String,
    pub file: String,
    pub leak_count: usize,
    pub leaked_bytes: usize,
    pub entries: Vec<LeakEntry>,
}

/// Build a grouped leak report from the aggregator's live allocations.
/// Only includes entries where `size >= min_bytes` (default 0 = all).
pub fn build(aggregator: &Aggregator, min_bytes: usize) -> LeakReport {
    let leaks: Vec<LeakEntry> = aggregator
        .live_leaks()
        .into_iter()
        .filter(|l| l.size >= min_bytes)
        .collect();

    // Group by (file, function) — we use file as a proxy for function grouping
    let mut groups: HashMap<String, LeakGroup> = HashMap::new();

    for leak in leaks {
        let key = format!("{}:{}", leak.file, leak.line);
        let group = groups.entry(key).or_insert_with(|| LeakGroup {
            function: String::new(),
            file: leak.file.clone(),
            leak_count: 0,
            leaked_bytes: 0,
            entries: Vec::new(),
        });
        group.leak_count += 1;
        group.leaked_bytes += leak.size;
        group.entries.push(leak);
    }

    let mut groups: Vec<LeakGroup> = groups.into_values().collect();
    groups.sort_unstable_by_key(|g| std::cmp::Reverse(g.leaked_bytes));

    let total_leaked_bytes: usize = groups.iter().map(|g| g.leaked_bytes).sum();
    let total_leak_count: usize = groups.iter().map(|g| g.leak_count).sum();

    LeakReport {
        total_leaked_bytes,
        total_leak_count,
        groups,
    }
}
