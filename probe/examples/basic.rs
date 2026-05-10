use ferroalloc_probe::{FerroAllocator, start_flush_thread};

#[global_allocator]
static ALLOC: FerroAllocator = FerroAllocator;

fn main() {
    // Connect to the analyzer before any tracked work begins
    start_flush_thread(7777);

    println!("Allocating some data...");

    let v: Vec<u8> = vec![0u8; 1024 * 1024]; // 1 MB
    println!("Allocated {} bytes", v.len());

    let s = String::from("ferroalloc probe example");
    println!("String: {s}");

    // v and s are dropped here — dealloc events are emitted automatically
    println!("Done. Check the analyzer for allocation data.");
}
