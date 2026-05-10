use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, Ordering};
use crossbeam_queue::SegQueue;
use backtrace::Backtrace;
use serde::Serialize;

// Prevents re-entrant allocations triggered by the probe itself (e.g. inside backtrace)
static IN_PROBE: AtomicBool = AtomicBool::new(false);

#[derive(Serialize)]
pub struct AllocEvent {
    pub kind: &'static str,  // "alloc" | "dealloc"
    pub ptr: u64,
    pub size: usize,
    pub frames: Vec<u64>,    // raw instruction pointers, resolved by the analyzer
}

// Lock-free global queue drained by the background flush thread
pub static EVENT_QUEUE: SegQueue<AllocEvent> = SegQueue::new();

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
    // Acquire the guard; bail out if already inside the probe to avoid infinite recursion
    if IN_PROBE.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        return;
    }

    let bt = Backtrace::new_unresolved();
    let frames: Vec<u64> = bt
        .frames()
        .iter()
        .map(|f| f.ip() as u64)
        .take(32)
        .collect();

    EVENT_QUEUE.push(AllocEvent { kind, ptr, size, frames });

    IN_PROBE.store(false, Ordering::Release);
}

/// Starts the background flush thread that streams allocation events to the analyzer.
/// Must be called once at program startup before any allocations of interest occur.
/// The analyzer is expected to be listening on `127.0.0.1:<port>` (default: 7777).
pub fn start_flush_thread(port: u16) {
    std::thread::spawn(move || {
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
                                break;
                            }
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                },
                // Analyzer not ready yet — keep retrying
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(500)),
            }
        }
    });
}
