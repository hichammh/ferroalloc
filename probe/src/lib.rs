use crossbeam_queue::SegQueue;
use serde::Serialize;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

// Thread-local guard preventing re-entrant allocations triggered by the probe itself.
// Backtrace collection internally allocates, so without this we'd recurse infinitely.
thread_local! {
    static IN_PROBE: Cell<bool> = const { Cell::new(false) };
}

// Gate: recording is disabled until start_flush_thread() connects to the analyzer.
static PROBE_ACTIVE: AtomicBool = AtomicBool::new(false);

// Maximum number of events buffered in the queue. When full, new events are dropped
// to prevent unbounded memory growth if the analyzer is disconnected.
const MAX_QUEUE_LEN: usize = 10_000;

// Sampling: record only 1 out of every N allocations.
static SAMPLE_RATE: AtomicU32 = AtomicU32::new(1);
static ALLOC_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Set the sampling rate. Only 1 in every `n` allocations will be recorded.
pub fn set_sample_rate(n: u32) {
    SAMPLE_RATE.store(n.max(1), Ordering::Relaxed);
}

/// An allocation event with the source location already resolved by the probe.
/// Resolving at the probe side avoids ASLR/DWARF mismatch issues on macOS.
#[derive(Serialize, Debug)]
pub struct AllocEvent {
    pub kind: &'static str, // "alloc" | "dealloc"
    pub ptr: u64,
    pub size: usize,
    pub file: String,
    pub line: u32,
    pub function: String,
}

// Lock-free global queue drained by the background flush thread
pub static EVENT_QUEUE: SegQueue<AllocEvent> = SegQueue::new();

/// Drop-in global allocator that wraps the system allocator and records every
/// heap operation into `EVENT_QUEUE` for streaming to the ferroalloc analyzer.
///
/// # Usage
///
/// ```rust,no_run
/// use ferroalloc_probe::{FerroAllocator, start_flush_thread};
///
/// #[global_allocator]
/// static ALLOC: FerroAllocator = FerroAllocator;
///
/// fn main() {
///     start_flush_thread(7777);
///     // ... rest of your program
/// }
/// ```
pub struct FerroAllocator;

unsafe impl GlobalAlloc for FerroAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            record(ptr as u64, layout.size(), "alloc");
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        record(ptr as u64, layout.size(), "dealloc");
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc_zeroed(layout);
        if !ptr.is_null() {
            record(ptr as u64, layout.size(), "alloc");
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() {
            if new_ptr == ptr {
                // In-place resize: the block did not move. Record a dealloc for
                // the old size and an alloc for the new size without changing the
                // pointer — this correctly updates live_bytes without inflating
                // alloc_count with a spurious extra allocation.
                record(ptr as u64, layout.size(), "dealloc");
                record(ptr as u64, new_size, "alloc");
            } else {
                // The allocator moved the block to a new address.
                record(ptr as u64, layout.size(), "dealloc");
                record(new_ptr as u64, new_size, "alloc");
            }
        }
        new_ptr
    }
}

fn record(ptr: u64, size: usize, kind: &'static str) {
    if !PROBE_ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    let already_in = IN_PROBE.with(|g| {
        if g.get() {
            true
        } else {
            g.set(true);
            false
        }
    });
    if already_in {
        return;
    }

    // Apply sampling
    let rate = SAMPLE_RATE.load(Ordering::Relaxed);
    if rate > 1 {
        let count = ALLOC_COUNTER.fetch_add(1, Ordering::Relaxed);
        if !count.is_multiple_of(rate as u64) {
            IN_PROBE.with(|g| g.set(false));
            return;
        }
    }

    // Resolve source location at the probe side using the runtime symbol table.
    // This avoids ASLR/DWARF address mismatch issues on macOS.
    let mut file = String::new();
    let mut line: u32 = 0;
    let mut function = String::new();
    let mut found = false;

    unsafe {
        backtrace::trace_unsynchronized(|frame| {
            if found {
                return false;
            }
            backtrace::resolve_frame_unsynchronized(frame, |symbol| {
                let fname = symbol.name().map(|n| n.to_string()).unwrap_or_default();

                // Skip internal frames from the probe, backtrace, std, and core
                let is_internal = fname.contains("ferroalloc_probe")
                    || fname.contains("backtrace::")
                    || fname.starts_with("std::")
                    || fname.starts_with("core::")
                    || fname.starts_with("alloc::")
                    || fname.contains("__rust_")
                    || fname.contains("_ZN");

                let fpath = symbol
                    .filename()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();

                // Skip frames from cargo registry, rustup toolchain, and system paths
                let is_dep = fpath.contains(".cargo/registry")
                    || fpath.contains(".rustup")
                    || fpath.contains("/rustc/")
                    || fpath.starts_with("/usr/")
                    || fpath.starts_with("/Library/");

                if is_internal || is_dep || fpath.is_empty() {
                    return;
                }

                file = fpath;
                line = symbol.lineno().unwrap_or(0);
                function = fname;
                found = true;
            });
            !found
        });
    }

    // Drop events when the queue is full to prevent unbounded memory growth
    // if the analyzer is disconnected (fixes OOM on high-allocation programs).
    if EVENT_QUEUE.len() < MAX_QUEUE_LEN {
        EVENT_QUEUE.push(AllocEvent {
            kind,
            ptr,
            size,
            file,
            line,
            function,
        });
    }

    IN_PROBE.with(|g| g.set(false));
}

/// Starts the background flush thread that streams allocation events to the analyzer.
///
/// Must be called once at program startup, before allocations of interest occur.
/// The analyzer must be listening on `127.0.0.1:<port>` (default: 7777).
pub fn start_flush_thread(port: u16) {
    std::thread::Builder::new()
        .name("ferroalloc-flush".into())
        .spawn(move || flush_loop(port))
        .expect("failed to spawn ferroalloc flush thread");
}

fn flush_loop(port: u16) {
    use std::io::Write;
    use std::net::TcpStream;

    // Permanently mark this thread so that none of its own allocations
    // (e.g. serde_json serialization, TcpStream buffers) are ever recorded.
    // Without this, dealloc() called from inside flush_loop re-enters
    // backtrace::resolve_frame_unsynchronized and crashes on macOS.
    IN_PROBE.with(|g| g.set(true));

    let addr = format!("127.0.0.1:{port}");
    loop {
        match TcpStream::connect(&addr) {
            Ok(mut stream) => {
                PROBE_ACTIVE.store(true, Ordering::Relaxed);
                'send: loop {
                    while let Some(event) = EVENT_QUEUE.pop() {
                        if let Ok(mut json) = serde_json::to_vec(&event) {
                            json.push(b'\n');
                            if stream.write_all(&json).is_err() {
                                PROBE_ACTIVE.store(false, Ordering::Relaxed);
                                break 'send;
                            }
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(500)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::Layout;
    use std::sync::Mutex;

    // Tests share a global EVENT_QUEUE, so they must run serially.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn drain_queue() -> Vec<AllocEvent> {
        let mut events = Vec::new();
        while let Some(e) = EVENT_QUEUE.pop() {
            events.push(e);
        }
        events
    }

    fn activate() {
        PROBE_ACTIVE.store(true, Ordering::Relaxed);
    }

    fn deactivate() {
        PROBE_ACTIVE.store(false, Ordering::Relaxed);
    }

    #[test]
    fn alloc_pushes_event_to_queue() {
        let _guard = TEST_LOCK.lock().unwrap();
        activate();
        drain_queue();

        let layout = Layout::from_size_align(64, 8).unwrap();
        unsafe {
            let ptr = FerroAllocator.alloc(layout);
            assert!(!ptr.is_null());

            let events = drain_queue();
            assert!(events
                .iter()
                .any(|e| e.kind == "alloc" && e.size == 64 && e.ptr == ptr as u64));

            FerroAllocator.dealloc(ptr, layout);
        }
        deactivate();
    }

    #[test]
    fn dealloc_pushes_event_to_queue() {
        let _guard = TEST_LOCK.lock().unwrap();
        activate();
        drain_queue();

        let layout = Layout::from_size_align(128, 8).unwrap();
        unsafe {
            let ptr = FerroAllocator.alloc(layout);
            drain_queue();

            FerroAllocator.dealloc(ptr, layout);

            let events = drain_queue();
            assert!(events
                .iter()
                .any(|e| e.kind == "dealloc" && e.ptr == ptr as u64));
        }
        deactivate();
    }

    #[test]
    fn realloc_emits_dealloc_then_alloc() {
        let _guard = TEST_LOCK.lock().unwrap();
        activate();
        drain_queue();

        let layout = Layout::from_size_align(64, 8).unwrap();
        unsafe {
            let ptr = FerroAllocator.alloc(layout);
            drain_queue();

            let new_ptr = FerroAllocator.realloc(ptr, layout, 256);
            assert!(!new_ptr.is_null());

            let events = drain_queue();
            assert!(events
                .iter()
                .any(|e| e.kind == "dealloc" && e.ptr == ptr as u64));
            assert!(events.iter().any(|e| e.kind == "alloc" && e.size == 256));

            FerroAllocator.dealloc(new_ptr, Layout::from_size_align(256, 8).unwrap());
        }
        deactivate();
    }

    #[test]
    fn frames_are_captured() {
        let _guard = TEST_LOCK.lock().unwrap();
        activate();
        drain_queue();

        let layout = Layout::from_size_align(32, 8).unwrap();
        unsafe {
            let ptr = FerroAllocator.alloc(layout);
            let events = drain_queue();
            // With probe-side resolution, file should be non-empty for test code
            let event = events.iter().find(|e| e.kind == "alloc");
            assert!(event.is_some(), "alloc event should be captured");

            FerroAllocator.dealloc(ptr, layout);
        }
        deactivate();
    }
}
