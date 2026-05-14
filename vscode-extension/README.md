# ferroalloc — Rust Memory Lens

Real-time heap memory visualization for Rust directly in VS Code.  
See **which lines allocate the most**, detect leaks, and compare memory snapshots — without leaving your editor.

![ferroalloc demo](https://github.com/hichammh/ferroalloc/blob/main/vscode-extension/images/demo.gif?raw=true)

---

## Features

### CodeLens — live allocation stats per line
Every line that allocates heap memory shows an inline counter:

```
allocate_buffer: 142 allocs · 72.7 MB        ← CodeLens
fn allocate_buffer() -> Vec<u8> {
    vec![0u8; 512 * 1024]
}
```

The counter updates every second while your program runs.  
Lines with unreleased memory show a ⚠ warning icon.

### Heatmap — visual allocation pressure
Background colors range from green (low) to red (high) based on total bytes allocated per line.

### Leak detection
`Ferroalloc: Show Live Leaks` lists every source line that has allocated memory not yet freed, sorted by bytes leaked.

### Snapshot diff
1. Run a baseline workload → `Ferroalloc: Save Memory Baseline`
2. Run a second workload
3. `Ferroalloc: Show Diff Since Baseline` — shows which lines increased, decreased, or are new

### Status bar
A live byte counter in the status bar. Click to toggle tracking on/off.

---

## Quick Start

### 1 — Add the probe to your Rust project

```toml
# Cargo.toml
[dependencies]
ferroalloc-probe = "0.1"
```

```rust
// src/main.rs
use ferroalloc_probe::{FerroAllocator, start_flush_thread};

#[global_allocator]
static ALLOC: FerroAllocator = FerroAllocator;

fn main() {
    start_flush_thread(7777);   // connect to the analyzer on port 7777
    // ... rest of your program
}
```

### 2 — Run the analyzer

```bash
cargo install ferroalloc-analyzer
ferroalloc-analyzer
```

The analyzer listens for your program on port **7777** and exposes an HTTP API on port **7778** for the extension.

### 3 — Start tracking in VS Code

Open your Rust project, then:

```
Cmd+Shift+P  →  Ferroalloc: Start Memory Tracking
```

Run your program. CodeLens counters appear on allocating lines within seconds.

---

## Configuration

| Setting | Default | Description |
|---|---|---|
| `ferroalloc.analyzerPort` | `7778` | HTTP port of the analyzer API |
| `ferroalloc.refreshIntervalMs` | `1000` | Polling interval in milliseconds |
| `ferroalloc.heatmapEnabled` | `true` | Show green-to-red background heatmap |

---

## Commands

| Command | Description |
|---|---|
| `Ferroalloc: Start Memory Tracking` | Connect to analyzer, begin polling |
| `Ferroalloc: Stop Memory Tracking` | Stop polling, clear decorations |
| `Ferroalloc: Toggle Memory Tracking` | Start or stop |
| `Ferroalloc: Reset Collected Data` | Clear all data on the analyzer |
| `Ferroalloc: Show Live Leaks` | List lines with unreleased allocations |
| `Ferroalloc: Save Memory Baseline` | Snapshot current state for diffing |
| `Ferroalloc: Show Diff Since Baseline` | Compare current state to baseline |

---

## How it works

```
Your Rust app                Analyzer              VS Code Extension
─────────────────            ────────────          ──────────────────
FerroAllocator               ferroalloc-           ferroalloc
  (GlobalAlloc)   ─TCP─▶     analyzer      ─HTTP─▶ extension
  captures every             aggregates            CodeLens + heatmap
  alloc/dealloc              by file:line          + leak panel
  resolves symbol
  at runtime
```

The probe resolves source locations (file, line, function) **inside your process** using the runtime symbol table — this avoids ASLR/DWARF address mismatch issues on macOS.

---

## Requirements

- Rust toolchain (stable)
- `ferroalloc-analyzer` running locally (see Quick Start)
- VS Code 1.90+

---

## License

MIT
