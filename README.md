# ferroalloc

> Real-time Rust heap memory visualization directly in VS Code.

[![CI](https://github.com/hichammh/ferroalloc/actions/workflows/ci.yml/badge.svg)](https://github.com/hichammh/ferroalloc/actions/workflows/ci.yml)
[![VS Code Marketplace](https://img.shields.io/visual-studio-marketplace/v/hichammh.ferroalloc)](https://marketplace.visualstudio.com/items?itemName=hichammh.ferroalloc)
[![Installs](https://img.shields.io/visual-studio-marketplace/i/hichammh.ferroalloc)](https://marketplace.visualstudio.com/items?itemName=hichammh.ferroalloc)
[![ferroalloc-probe on crates.io](https://img.shields.io/crates/v/ferroalloc-probe)](https://crates.io/crates/ferroalloc-probe)

![ferroalloc demo](https://github.com/hichammh/ferroalloc/blob/main/vscode-extension/images/demo.gif?raw=true)

## What it does

Ferroalloc shows live heap memory stats inside VS Code as your Rust program runs:

- **CodeLens** — allocation count and total bytes above each line that allocates
- **Heatmap** — lines colored green → red by allocation volume
- **Leak detection** — lines with unfreed allocations flagged with ⚠
- **Snapshot diff** — compare memory state before and after a workload

## Quick Start

### 1 — Install the VS Code extension

Search **"ferroalloc"** in the VS Code Extensions panel, or:

```
ext install hichammh.ferroalloc
```

### 2 — Install the analyzer

```bash
cargo install ferroalloc-analyzer
```

### 3 — Add the probe to your Rust project

```toml
# Cargo.toml
[dependencies]
ferroalloc-probe = "0.1"

# Required: without debug symbols the probe resolves no file or line, and the
# extension has nothing to show. Cheap to enable, and it does not slow the build
# output down
[profile.release]
debug = true
```

```rust
// src/main.rs
use ferroalloc_probe::{FerroAllocator, start_flush_thread};

#[global_allocator]
static ALLOC: FerroAllocator = FerroAllocator;

fn main() {
    start_flush_thread(7777);
    // ... rest of your program
}
```

### 4 — Run

```bash
# Terminal 1 — start the analyzer
ferroalloc-analyzer

# Terminal 2 — run your program
cargo run
```

Then in VS Code: `Cmd+Shift+P` → **Ferroalloc: Start Memory Tracking**

## Architecture

```
Your Rust app                Analyzer              VS Code Extension
─────────────────            ────────────          ──────────────────
FerroAllocator               ferroalloc-           ferroalloc
  (GlobalAlloc)   ─TCP:7777─▶ analyzer   ─HTTP:7778─▶ extension
  captures every             aggregates            CodeLens + heatmap
  alloc/dealloc              by file:line          + leak panel
  resolves symbol
  at runtime
```

## Project structure

```
ferroalloc/
├── probe/             # ferroalloc-probe crate (add to your project)
├── analyzer/          # ferroalloc-analyzer binary (install on your machine)
└── vscode-extension/  # VS Code extension (install from marketplace)
```

## Troubleshooting

**Nothing appears in the editor.** Check the analyzer's counters:

```bash
curl http://127.0.0.1:7778/health
# {"status":"ok","events_received":8412,"events_resolved":8412,"events_dropped":0}
```

| What you see | What it means |
|---|---|
| `events_received: 0` | the probe never connected — is `start_flush_thread()` called, and is the analyzer running? |
| `received > 0`, `resolved: 0` | no debug symbols: add `[profile.release] debug = true` to your program's `Cargo.toml` and rebuild |
| `events_dropped > 0` | the program allocated faster than the analyzer could drain; counts are an over-estimate — use sampling below |

The status bar reports both cases on its own.

## Sampling

On allocation-heavy programs, record only a fraction of the blocks:

```rust
ferroalloc_probe::set_sample_rate(100); // 1 block in 100
```

The decision is taken from the block address, so a sampled allocation always has
its matching free recorded too — `live_bytes` stays meaningful and correct code is
never reported as leaking.

The flip side: an allocate-free loop that reuses one address is either fully
recorded or fully skipped, so the volume reduction is weaker than the rate on that
pattern. Correctness is unaffected.

## Configuration

| Setting | Default | Description |
|---|---|---|
| `ferroalloc.analyzerPort` | `7778` | HTTP port of the analyzer API |
| `ferroalloc.refreshIntervalMs` | `1000` | Poll interval in milliseconds |
| `ferroalloc.heatmapEnabled` | `true` | Enable/disable background heatmap |

## License

MIT
