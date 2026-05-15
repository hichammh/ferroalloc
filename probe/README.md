# ferroalloc-probe

A drop-in `GlobalAlloc` wrapper that streams every heap allocation and deallocation
to the [ferroalloc](https://marketplace.visualstudio.com/items?itemName=hichammh.ferroalloc)
VS Code extension for real-time memory visualization.

## Usage

```toml
# Cargo.toml
[dependencies]
ferroalloc-probe = "0.1"
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

## License

MIT
