use backtrace::Backtrace;
use crossbeam_queue::SegQueue;
use serde::Serialize;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

// Thread-local guard preventing re-entrant allocations triggered by the probe itself.
// Backtrace collection internally allocates, so without this we'd recurse infinitely.
// A Cell<bool> is sufficient — no cross-thread synchronisation needed by design.
thread_local! {
    static IN_PROBE: Cell<bool> = const { Cell::new(false) };
}

// Sampling: record only 1 out of every N allocations.
// N=1 means record all (default). Set via `set_sample_rate()`.
static SAMPLE_RATE: AtomicU32 = AtomicU32::new(1);
static ALLOC_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Set the sampling rate. Only 1 in every `n` allocations will be recorded.
/// Use `n = 1` to record all (default). Higher values reduce overhead on hot paths.
pub fn set_sample_rate(n: u32) {
    SAMPLE_RATE.store(n.max(1), Ordering::Relaxed);
}

#[derive(Serialize, Debug)]
pub struct AllocEvent {
    pub kind: &'static str, // "alloc" | "dealloc"
    pub ptr: u64,
    pub size: usize,
    pub frames: Vec<u64>, // raw instruction pointers, resolved by the analyzer
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
            record(ptr as u64, layout.size(), "dealloc");
            record(new_ptr as u64, new_size, "alloc");
        }
        new_ptr
    }
}

fn record(ptr: u64, size: usize, kind: &'static str) {
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

    // Apply sampling: skip this event if the counter is not a multiple of SAMPLE_RATE
    let rate = SAMPLE_RATE.load(Ordering::Relaxed);
    if rate > 1 {
        let count = ALLOC_COUNTER.fetch_add(1, Ordering::Relaxed);
        if !count.is_multiple_of(rate as u64) {
            IN_PROBE.with(|g| g.set(false));
            return;
        }
    }

    let bt = Backtrace::new_unresolved();
    let frames: Vec<u64> = bt.frames().iter().map(|f| f.ip() as u64).take(32).collect();

    EVENT_QUEUE.push(AllocEvent {
        kind,
        ptr,
        size,
        frames,
    });

    IN_PROBE.with(|g| g.set(false));
}

/// Starts the background flush thread that streams allocation events to the analyzer.
///
/// Must be called once at program startup, before allocations of interest occur.
/// The analyzer must be listening on `127.0.0.1:<port>` (default: 7777).
/// If the analyzer is not yet up, the thread retries the connection every 500 ms.
pub fn start_flush_thread(port: u16) {
    std::thread::Builder::new()
        .name("ferroalloc-flush".into())
        .spawn(move || flush_loop(port))
        .expect("failed to spawn ferroalloc flush thread");
}

fn flush_loop(port: u16) {
    use std::io::Write;
    use std::net::TcpStream;

    let addr = format!("127.0.0.1:{port}");
    loop {
        match TcpStream::connect(&addr) {
            Ok(mut stream) => loop {
                while let Some(event) = EVENT_QUEUE.pop() {
                    if let Ok(mut json) = serde_json::to_vec(&event) {
                        json.push(b'\n');
                        if stream.write_all(&json).is_err() {
                            break; // reconnect on next outer iteration
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            },
            // Analyzer not ready yet — keep retrying
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(500)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::Layout;
    use std::sync::Mutex;

    // Tests share a global EVENT_QUEUE, so they must run serially to avoid
    // one test consuming events that belong to another.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn drain_queue() -> Vec<AllocEvent> {
        let mut events = Vec::new();
        while let Some(e) = EVENT_QUEUE.pop() {
            events.push(e);
        }
        events
    }

    #[test]
    fn alloc_pushes_event_to_queue() {
        let _guard = TEST_LOCK.lock().unwrap();
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
    }

    #[test]
    fn dealloc_pushes_event_to_queue() {
        let _guard = TEST_LOCK.lock().unwrap();
        drain_queue();

        let layout = Layout::from_size_align(128, 8).unwrap();
        unsafe {
            let ptr = FerroAllocator.alloc(layout);
            drain_queue(); // discard the alloc event

            FerroAllocator.dealloc(ptr, layout);

            let events = drain_queue();
            assert!(events
                .iter()
                .any(|e| e.kind == "dealloc" && e.ptr == ptr as u64));
        }
    }

    #[test]
    fn realloc_emits_dealloc_then_alloc() {
        let _guard = TEST_LOCK.lock().unwrap();
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
    }

    #[test]
    fn frames_are_captured() {
        let _guard = TEST_LOCK.lock().unwrap();
        drain_queue();

        let layout = Layout::from_size_align(32, 8).unwrap();
        unsafe {
            let ptr = FerroAllocator.alloc(layout);
            let events = drain_queue();
            let event = events.iter().find(|e| e.kind == "alloc").unwrap();
            assert!(
                !event.frames.is_empty(),
                "backtrace frames should be captured"
            );
            FerroAllocator.dealloc(ptr, layout);
        }
    }
}
