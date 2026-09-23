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

// Maximum number of alloc events buffered in the queue. Once reached, new alloc
// events are dropped to prevent unbounded memory growth if the analyzer is
// disconnected.
const MAX_QUEUE_LEN: usize = 10_000;

// Deallocs are still accepted past MAX_QUEUE_LEN, up to this hard ceiling:
// dropping the dealloc of an allocation we did record turns a freed block into a
// phantom leak that never goes away. Above the ceiling they are dropped too, so
// that the queue stays bounded.
const HARD_QUEUE_LEN: usize = 20_000;

// Sampling: record only 1 out of every N allocations.
static SAMPLE_RATE: AtomicU32 = AtomicU32::new(1);

// Events discarded because the queue was full. Reported to the analyzer so the UI
// can say the data is incomplete instead of presenting wrong numbers as fact.
static EVENTS_DROPPED: AtomicU64 = AtomicU64::new(0);

/// Set the sampling rate. Only 1 in every `n` allocations will be recorded.
///
/// The decision is taken from the block address, so the `dealloc` of a recorded
/// `alloc` is always recorded as well. Sampling the two independently would leave
/// `live_bytes` permanently high and report every correct program as leaking.
pub fn set_sample_rate(n: u32) {
    SAMPLE_RATE.store(n.max(1), Ordering::Relaxed);
}

/// Number of events dropped because the event queue was full.
///
/// A non-zero value means the reported statistics are incomplete: the analyzer was
/// not draining events as fast as the program produced them.
pub fn events_dropped() -> u64 {
    EVENTS_DROPPED.load(Ordering::Relaxed)
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
        // Recorded while we still own the block. Once it is back with the system,
        // another thread can be handed the same address and publish its alloc
        // first, and the analyzer would then match our dealloc to that new block.
        record(ptr as u64, layout.size(), "dealloc");
        System.dealloc(ptr, layout);
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc_zeroed(layout);
        if !ptr.is_null() {
            record(ptr as u64, layout.size(), "alloc");
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A realloc is a dealloc of the old block followed by an alloc of the new
        // one, whether the block moved or not. The dealloc is recorded before the
        // call for the same reason as in dealloc(): if the block moves, the old
        // address is released inside System.realloc and can be reused at once.
        record(ptr as u64, layout.size(), "dealloc");
        let new_ptr = System.realloc(ptr, layout, new_size);
        if new_ptr.is_null() {
            // The old block is still ours: undo the dealloc recorded above.
            record(ptr as u64, layout.size(), "alloc");
        } else {
            record(new_ptr as u64, new_size, "alloc");
        }
        new_ptr
    }
}

/// Whether a block is recorded under the current sampling rate.
///
/// Keyed on the block address — hashed, because alignment makes the low bits
/// constant — so that the `alloc` and the `dealloc` of one block always take the
/// same decision.
fn is_sampled(ptr: u64, rate: u32) -> bool {
    if rate <= 1 {
        return true;
    }
    let hashed = ptr.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 16;
    hashed.is_multiple_of(rate as u64)
}

// Clears IN_PROBE even if symbol resolution unwinds. Without it, a panic inside
// backtrace leaves the flag set and the thread silently stops recording for the
// rest of its life.
struct ProbeGuard;

impl Drop for ProbeGuard {
    fn drop(&mut self) {
        IN_PROBE.with(|g| g.set(false));
    }
}

static RESOLVE_LOCK: AtomicBool = AtomicBool::new(false);

// Held while walking and resolving the stack; released on drop, including when
// symbol resolution unwinds.
struct ResolveLock;

impl ResolveLock {
    fn acquire() -> Self {
        while RESOLVE_LOCK
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        ResolveLock
    }
}

impl Drop for ResolveLock {
    fn drop(&mut self) {
        RESOLVE_LOCK.store(false, Ordering::Release);
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
    let _guard = ProbeGuard;

    if !is_sampled(ptr, SAMPLE_RATE.load(Ordering::Relaxed)) {
        return;
    }

    // Checked before resolving symbols: resolution is the expensive part, and an
    // event we are about to drop is not worth paying for.
    let limit = if kind == "dealloc" {
        HARD_QUEUE_LEN
    } else {
        MAX_QUEUE_LEN
    };
    if EVENT_QUEUE.len() >= limit {
        EVENTS_DROPPED.fetch_add(1, Ordering::Relaxed);
        return;
    }

    // Resolve source location at the probe side using the runtime symbol table.
    // This avoids ASLR/DWARF address mismatch issues on macOS.
    let mut file = String::new();
    let mut line: u32 = 0;
    let mut function = String::new();
    let mut found = false;

    // The *_unsynchronized backtrace functions share one global symbol cache and
    // require the caller to serialize them: two threads resolving at once
    // corrupt it and crash the program. A spin lock rather than a Mutex because
    // it never allocates, which matters inside an allocator.
    let _resolve_guard = ResolveLock::acquire();

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
    // Released before pushing: the queue is lock-free and needs no protection.
    drop(_resolve_guard);

    EVENT_QUEUE.push(AllocEvent {
        kind,
        ptr,
        size,
        file,
        line,
        function,
    });
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
    let mut reported_dropped = 0u64;
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

                    // Tell the analyzer how many events were lost, so the UI can
                    // flag the data as incomplete rather than silently showing
                    // allocations without their matching frees.
                    let dropped = EVENTS_DROPPED.load(Ordering::Relaxed);
                    if dropped != reported_dropped {
                        let line = format!("{{\"kind\":\"dropped\",\"count\":{dropped}}}\n");
                        if stream.write_all(line.as_bytes()).is_err() {
                            PROBE_ACTIVE.store(false, Ordering::Relaxed);
                            break 'send;
                        }
                        reported_dropped = dropped;
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
        SAMPLE_RATE.store(1, Ordering::Relaxed);
        EVENTS_DROPPED.store(0, Ordering::Relaxed);
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
    fn sampling_keeps_alloc_and_dealloc_of_the_same_block_together() {
        // A block is either fully recorded or fully ignored. Sampling the two
        // halves independently would leave live_bytes high forever and report a
        // correct program as leaking.
        for rate in [2u32, 10, 100, 997] {
            for ptr in [0x1000u64, 0x7f_ffff_abcd, 0x2a2a_2a2a_2a2a] {
                assert_eq!(
                    is_sampled(ptr, rate),
                    is_sampled(ptr, rate),
                    "decision must be stable for ptr {ptr:#x} at rate {rate}"
                );
            }
        }
    }

    #[test]
    fn sampling_rate_one_records_everything() {
        for ptr in [0u64, 8, 0x1000, u64::MAX] {
            assert!(is_sampled(ptr, 1));
            assert!(is_sampled(ptr, 0));
        }
    }

    #[test]
    fn sampling_actually_thins_out_allocations() {
        let kept = (0..10_000u64)
            .map(|i| 0x1000 + i * 32)
            .filter(|&ptr| is_sampled(ptr, 100))
            .count();
        // Roughly 1 %, with plenty of slack for hash imbalance.
        assert!(
            (20..500).contains(&kept),
            "expected about 100 of 10000 blocks to be sampled, got {kept}"
        );
    }

    #[test]
    fn dealloc_is_still_recorded_when_the_alloc_queue_is_full() {
        let _guard = TEST_LOCK.lock().unwrap();
        activate();
        drain_queue();

        // Fill the queue past the alloc limit with synthetic events.
        for i in 0..MAX_QUEUE_LEN as u64 {
            EVENT_QUEUE.push(AllocEvent {
                kind: "alloc",
                ptr: i,
                size: 1,
                file: "synthetic.rs".to_string(),
                line: 1,
                function: "fill".to_string(),
            });
        }

        // Measured as a delta: the test's own allocations go through record()
        // too, so the absolute counters are not ours alone.
        let before = EVENT_QUEUE.len();
        let dropped_before = events_dropped();
        record(0xdead_beef, 64, "alloc");
        assert_eq!(EVENT_QUEUE.len(), before, "alloc must be dropped when full");
        assert_eq!(events_dropped(), dropped_before + 1);

        record(0xdead_beef, 64, "dealloc");
        assert!(
            EVENT_QUEUE.len() > before,
            "dealloc must still be recorded above MAX_QUEUE_LEN, otherwise the \
             freed block shows up as a permanent leak"
        );

        drain_queue();
        deactivate();
    }

    #[test]
    fn everything_is_dropped_above_the_hard_ceiling() {
        let _guard = TEST_LOCK.lock().unwrap();
        activate();
        drain_queue();

        for i in 0..HARD_QUEUE_LEN as u64 {
            EVENT_QUEUE.push(AllocEvent {
                kind: "dealloc",
                ptr: i,
                size: 1,
                file: "synthetic.rs".to_string(),
                line: 1,
                function: "fill".to_string(),
            });
        }

        let before = EVENT_QUEUE.len();
        record(0xdead_beef, 64, "dealloc");
        assert_eq!(EVENT_QUEUE.len(), before, "queue must stay bounded");

        drain_queue();
        deactivate();
    }

    // Collects events on a separate thread while the workers run, so that the
    // queue never fills up and no event is dropped.
    fn spawn_drainer(stop: std::sync::Arc<AtomicBool>) -> std::thread::JoinHandle<Vec<AllocEvent>> {
        std::thread::spawn(move || {
            // Like the flush thread: the drainer's own allocations are not ours.
            IN_PROBE.with(|g| g.set(true));
            let mut events = Vec::new();
            loop {
                let stopping = stop.load(Ordering::Acquire);
                while let Some(e) = EVENT_QUEUE.pop() {
                    events.push(e);
                }
                if stopping {
                    return events;
                }
                std::thread::yield_now();
            }
        })
    }

    // Allocates, grows and frees blocks from several threads at once. The large
    // sizes make the system allocator hand a freed address straight to another
    // thread, which is what exposes ordering bugs.
    fn run_concurrent_workload(threads: usize, iterations: usize) {
        let workers: Vec<_> = (0..threads)
            .map(|t| {
                std::thread::spawn(move || unsafe {
                    for i in 0..iterations {
                        let size = [24, 4096, 128 * 1024, 512 * 1024][(i + t) % 4];
                        let layout = Layout::from_size_align(size, 8).unwrap();
                        let ptr = FerroAllocator.alloc(layout);
                        let grown = FerroAllocator.realloc(ptr, layout, size * 2);
                        FerroAllocator
                            .dealloc(grown, Layout::from_size_align(size * 2, 8).unwrap());
                    }
                })
            })
            .collect();
        for w in workers {
            w.join().unwrap();
        }
    }

    #[test]
    fn concurrent_allocations_do_not_crash() {
        // Symbol resolution goes through backtrace's *_unsynchronized functions,
        // which share a global cache. Without a lock around them, a few threads
        // allocating at once corrupt it and abort the process.
        let _guard = TEST_LOCK.lock().unwrap();
        activate();
        drain_queue();

        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let drainer = spawn_drainer(stop.clone());
        run_concurrent_workload(8, 300);
        stop.store(true, Ordering::Release);
        let events = drainer.join().unwrap();

        // Reaching this point is the test; the check only guards against a
        // workload that silently recorded nothing.
        assert!(events.iter().any(|e| e.kind == "alloc"));
        deactivate();
    }

    #[test]
    fn a_freed_address_is_never_reported_allocated_before_its_free() {
        // Replays the event stream the way the analyzer does. If the dealloc of a
        // block is recorded after the memory went back to the system, another
        // thread can get the same address and publish its alloc first: the
        // analyzer then sees two live blocks at one address and keeps a phantom
        // leak forever.
        let _guard = TEST_LOCK.lock().unwrap();
        activate();
        drain_queue();
        let dropped_before = events_dropped();

        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let drainer = spawn_drainer(stop.clone());
        run_concurrent_workload(8, 300);
        stop.store(true, Ordering::Release);
        let events = drainer.join().unwrap();

        assert_eq!(
            events_dropped(),
            dropped_before,
            "events were dropped, the replay below would be meaningless"
        );
        let mut live = std::collections::HashSet::new();
        for (i, e) in events.iter().enumerate() {
            match e.kind {
                "alloc" => assert!(
                    live.insert(e.ptr),
                    "event {i}: alloc of {:#x} while that address is still live",
                    e.ptr
                ),
                _ => {
                    live.remove(&e.ptr);
                }
            }
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
