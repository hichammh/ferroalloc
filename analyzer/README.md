# ferroalloc-analyzer

The aggregator and HTTP API server for the [ferroalloc](https://marketplace.visualstudio.com/items?itemName=hichammh.ferroalloc)
VS Code extension.

It receives allocation events from `ferroalloc-probe` over TCP, aggregates them by
source location (file:line), and exposes a REST API consumed by the VS Code extension.

## Installation

```bash
cargo install ferroalloc-analyzer
```

## Usage

```bash
ferroalloc-analyzer
# Listening for probe on port 7777
# API server on port 7778
```

Then start your Rust program instrumented with `ferroalloc-probe`, and open your
project in VS Code with the [ferroalloc extension](https://marketplace.visualstudio.com/items?itemName=hichammh.ferroalloc) installed.

## API Endpoints

| Endpoint | Description |
|---|---|
| `GET /snapshot` | Current allocation stats per source line |
| `GET /leaks` | Lines with unreleased memory |
| `GET /leak-report?min_bytes=N` | Grouped leak report |
| `POST /baseline` | Save current snapshot as baseline |
| `GET /diff` | Compare current state to baseline |
| `POST /reset` | Clear all collected data |
| `GET /health` | Events received and resolved counters |

## License

MIT
