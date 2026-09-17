# ferroalloc-probe

A drop-in `GlobalAlloc` wrapper that streams every heap allocation and deallocation
to the [ferroalloc](https://marketplace.visualstudio.com/items?itemName=hichammh.ferroalloc)
VS Code extension for real-time memory visualization.

## Usage

```toml
# Cargo.toml
[dependencies]
ferroalloc-probe = "0.1"

# Required: source locations come from your binary's debug symbols. Without them
# the probe resolves no file or line and the extension shows nothing.
[profile.release]
debug = true
```

```rust
use ferroalloc_probe::{FerroAllocator, start_flush_thread};

#[global_allocator]
static ALLOC: FerroAllocator = FerroAllocator;

fn main() {
    start_flush_thread(7777); // connect to ferroalloc-analyzer
    // ... rest of your program
}
```

Then run `ferroalloc-analyzer` and open your project in VS Code with the
[ferroalloc extension](https://marketplace.visualstudio.com/items?itemName=hichammh.ferroalloc) installed.

## How it works

`FerroAllocator` wraps the system allocator and records every `alloc`/`dealloc` call.
Source locations (file, line, function) are resolved at runtime inside your process
using the OS symbol table — this avoids ASLR/DWARF mismatch issues on macOS.

Events are pushed into a lock-free queue and streamed over TCP to `ferroalloc-analyzer`.

The queue is bounded. When it fills up, `alloc` events are dropped but `dealloc`
events keep being recorded a while longer: dropping the free of an allocation we
did record would turn a released block into a leak that never disappears. The
number of dropped events is reported to the analyzer — read it back with
`events_dropped()`, or on `GET /health` — so the UI can flag the data as
incomplete rather than presenting wrong numbers.

## Sampling

```rust
ferroalloc_probe::set_sample_rate(100); // record 1 block in 100
```

The decision is derived from the block address, so `alloc` and `dealloc` of the
same block always agree. Sampling them independently would keep `live_bytes`
permanently high and report every correct program as leaking.

Because the decision follows the address, a tight allocate-then-free loop that
keeps reusing the same address is either fully recorded or fully skipped — the
thinning factor is weaker on that pattern than the rate suggests. Correctness is
never affected; only the volume reduction is.

## License

MIT
