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

## Configuration

| Setting | Default | Description |
|---|---|---|
| `ferroalloc.analyzerPort` | `7778` | HTTP port of the analyzer API |
| `ferroalloc.refreshIntervalMs` | `1000` | Poll interval in milliseconds |
| `ferroalloc.heatmapEnabled` | `true` | Enable/disable background heatmap |

## License

MIT
