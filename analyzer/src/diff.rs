use crate::aggregator::LineStats;
use serde::Serialize;

/// Represents the memory difference between two snapshots.
#[derive(Serialize)]
pub struct SnapshotDiff {
    /// Lines where allocation increased between baseline and current
    pub increased: Vec<DiffEntry>,
    /// Lines where allocation decreased (more frees than allocs since baseline)
    pub decreased: Vec<DiffEntry>,
    /// Lines that appear in current but not in baseline (new allocations)
    pub new_lines: Vec<DiffEntry>,
    /// Total byte delta (positive = more heap used)
    pub total_delta_bytes: i64,
}

#[derive(Serialize)]
pub struct DiffEntry {
    pub file: String,
    pub line: u32,
    pub function: String,
    pub delta_alloc_count: i64,
    pub delta_total_bytes: i64,
    pub delta_live_bytes: i64,
}

/// Compute the diff between a baseline snapshot and the current one.
pub fn compute(baseline: &[LineStats], current: &[LineStats]) -> SnapshotDiff {
    use std::collections::HashMap;

    let baseline_map: HashMap<(&str, u32), &LineStats> = baseline
        .iter()
        .map(|s| ((s.file.as_str(), s.line), s))
        .collect();

    let mut increased = Vec::new();
    let mut decreased = Vec::new();
    let mut new_lines = Vec::new();
    let mut total_delta: i64 = 0;

    for cur in current {
        let key = (cur.file.as_str(), cur.line);
        let entry = match baseline_map.get(&key) {
            Some(base) => {
                let delta_bytes = cur.total_bytes as i64 - base.total_bytes as i64;
                total_delta += delta_bytes;
                DiffEntry {
                    file: cur.file.clone(),
                    line: cur.line,
                    function: cur.function.clone(),
                    delta_alloc_count: cur.alloc_count as i64 - base.alloc_count as i64,
                    delta_total_bytes: delta_bytes,
                    delta_live_bytes: cur.live_bytes - base.live_bytes,
                }
            }
            None => {
                total_delta += cur.total_bytes as i64;
                let e = DiffEntry {
                    file: cur.file.clone(),
                    line: cur.line,
                    function: cur.function.clone(),
                    delta_alloc_count: cur.alloc_count as i64,
                    delta_total_bytes: cur.total_bytes as i64,
                    delta_live_bytes: cur.live_bytes,
                };
                new_lines.push(e);
                continue;
            }
        };

        if entry.delta_total_bytes > 0 {
            increased.push(entry);
        } else if entry.delta_total_bytes < 0 {
            decreased.push(entry);
        }
    }

    // Sort by absolute delta descending
    increased.sort_unstable_by_key(|e| std::cmp::Reverse(e.delta_total_bytes));
    decreased.sort_unstable_by_key(|e| e.delta_total_bytes);
    new_lines.sort_unstable_by_key(|e| std::cmp::Reverse(e.delta_total_bytes));

    SnapshotDiff {
        increased,
        decreased,
        new_lines,
        total_delta_bytes: total_delta,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregator::LineStats;

    fn stat(
        file: &str,
        line: u32,
        alloc_count: u64,
        total_bytes: u64,
        live_bytes: i64,
    ) -> LineStats {
        LineStats {
            file: file.to_string(),
            line,
            function: "fn".to_string(),
            alloc_count,
            total_bytes,
            live_bytes,
        }
    }

    #[test]
    fn detects_increased_allocation() {
        let baseline = vec![stat("a.rs", 1, 1, 100, 100)];
        let current = vec![stat("a.rs", 1, 3, 300, 200)];
        let diff = compute(&baseline, &current);
        assert_eq!(diff.increased.len(), 1);
        assert_eq!(diff.increased[0].delta_total_bytes, 200);
        assert_eq!(diff.total_delta_bytes, 200);
    }

    #[test]
    fn detects_new_lines() {
        let baseline = vec![];
        let current = vec![stat("b.rs", 5, 2, 64, 64)];
        let diff = compute(&baseline, &current);
        assert_eq!(diff.new_lines.len(), 1);
        assert_eq!(diff.new_lines[0].delta_total_bytes, 64);
    }

    #[test]
    fn detects_decreased_allocation() {
        let baseline = vec![stat("c.rs", 10, 5, 500, 500)];
        let current = vec![stat("c.rs", 10, 5, 200, 0)];
        let diff = compute(&baseline, &current);
        assert_eq!(diff.decreased.len(), 1);
        assert_eq!(diff.decreased[0].delta_total_bytes, -300);
        assert_eq!(diff.total_delta_bytes, -300);
    }

    #[test]
    fn empty_diff_when_no_change() {
        let baseline = vec![stat("d.rs", 1, 1, 128, 0)];
        let current = vec![stat("d.rs", 1, 1, 128, 0)];
        let diff = compute(&baseline, &current);
        assert!(diff.increased.is_empty());
        assert!(diff.decreased.is_empty());
        assert!(diff.new_lines.is_empty());
        assert_eq!(diff.total_delta_bytes, 0);
    }
}
