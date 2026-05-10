# Ferroalloc

> Real-time Rust heap memory visualization directly in VS Code — no code changes required.

![CI](https://github.com/hichammh/ferroalloc/actions/workflows/ci.yml/badge.svg)

## What it does

Ferroalloc instruments your Rust program's allocator at compile time and displays live memory stats
inside the editor as you debug:

- **CodeLens** — allocation count, total bytes, and live bytes above each line that allocates
- **Heatmap** — lines colored green → red by allocation volume
- **Leak detection** — lines with unfreed allocations are highlighted with a warning glyph

No source modifications needed. The probe is injected transparently via `RUSTFLAGS`.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│  Your Rust binary (debug build)                     │
│  ┌──────────────────────────────────────────────┐   │
│  │  ferroalloc-probe   (GlobalAlloc wrapper)    │   │
│  │  Lock-free queue → flush thread → TCP :7777  │   │
│  └──────────────────────────────────────────────┘   │
└──────────────────────┬──────────────────────────────┘
                       │ newline-delimited JSON events
          ┌────────────▼────────────┐
          │  ferroalloc-analyzer    │
          │  DWARF resolver (gimli) │
          │  Per-line aggregation   │
          │  HTTP API  → TCP :7778  │
          └────────────┬────────────┘
                       │ JSON (poll every 1 s)
          ┌────────────▼────────────┐
          │  VS Code extension      │
          │  CodeLens provider      │
          │  Heatmap decorator      │
          └─────────────────────────┘
```

## Getting started

### 1. Build the analyzer

```bash
cargo build --release -p ferroalloc-analyzer
```

### 2. Build your Rust project with the probe

```bash
RUSTFLAGS="--extern ferroalloc_probe=target/debug/libferroalloc_probe.rlib" \
  cargo build
```

> The probe registers itself as the global allocator. If your project already uses a custom
> allocator (e.g. `jemalloc`), see the [Custom allocator guide](docs/custom-allocator.md).

### 3. Start the analyzer

```bash
./target/release/ferroalloc-analyzer ./target/debug/your-binary
```

### 4. Run your binary

The probe automatically connects to the analyzer on `127.0.0.1:7777`.

### 5. Open VS Code

Run the **Ferroalloc: Start Memory Tracking** command (`Ctrl+Shift+P`).
CodeLens items and heatmap decorations will appear as your program runs.

## VS Code commands

| Command | Description |
|---|---|
| `Ferroalloc: Start Memory Tracking` | Begin polling the analyzer |
| `Ferroalloc: Stop Memory Tracking` | Stop polling |
| `Ferroalloc: Show Live Leaks` | List allocations not yet freed |

## Configuration

| Setting | Default | Description |
|---|---|---|
| `ferroalloc.analyzerPort` | `7778` | Port the analyzer API listens on |
| `ferroalloc.refreshIntervalMs` | `1000` | Poll interval in milliseconds |
| `ferroalloc.heatmapEnabled` | `true` | Enable/disable background heatmap |

## Project structure

```
ferroalloc/
├── probe/              # Rust crate — GlobalAlloc wrapper + IPC flush
├── analyzer/           # Rust binary — DWARF resolver + HTTP API
│   └── src/
│       ├── main.rs
│       ├── dwarf.rs    # addr2line / gimli integration
│       ├── aggregator.rs
│       └── api.rs
├── vscode-extension/   # TypeScript VS Code extension
│   └── src/
│       ├── extension.ts
│       ├── analyzerClient.ts
│       ├── codelens.ts
│       └── heatmap.ts
└── .github/workflows/ci.yml
```

## License

MIT
