use abfall::{GcContext, Trace, Tracer};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

// Simple type for testing
struct Value {
    data: i32,
}

unsafe impl Trace for Value {
    fn trace(&self, _tracer: &mut Tracer) {
        // No GC pointers to trace
    }
}

fn main() {
    println!("=== Allocation Safety Test: Concurrent Allocation During GC ===\n");
    
    test_concurrent_allocation_basic();
    test_allocation_during_collection();
    test_high_contention_allocation();
    
    println!("\n✅ ALL ALLOCATION SAFETY TESTS PASSED!");
}

/// Test basic concurrent allocation
fn test_concurrent_allocation_basic() {
    println!("Test 1: Basic concurrent allocation");
    
    let ctx = GcContext::with_options(false, Duration::from_secs(10));
    let heap = Arc::clone(ctx.heap());
    let mut handles = vec![];
    
    for i in 0..10 {
        let heap_clone = Arc::clone(&heap);
        let handle = thread::spawn(move || {
            let ctx = GcContext::with_heap(heap_clone);
            for j in 0..100 {
                let _val = ctx.allocate(Value { data: i * 100 + j });
            }
        });
        handles.push(handle);
    }
    
    for handle in handles {
        handle.join().unwrap();
    }
    
    println!("  Allocated: {} objects", ctx.allocation_count());
    assert_eq!(ctx.allocation_count(), 1000);
    println!("  ✓ Passed\n");
}

/// Test allocation happening DURING an active collection
fn test_allocation_during_collection() {
    println!("Test 2: Allocation during active collection");
    
    let ctx = GcContext::with_options(false, Duration::from_secs(10));
    
    // Pre-allocate some objects
    for i in 0..100 {
        let _val = ctx.allocate(Value { data: i });
    }
    
    let barrier = Arc::new(Barrier::new(3));
    let heap = Arc::clone(ctx.heap());
    
    // Thread 1: Start collection
    let heap1 = Arc::clone(&heap);
    let barrier1 = Arc::clone(&barrier);
    let h1 = thread::spawn(move || {
        let ctx1 = GcContext::with_heap(heap1);
        barrier1.wait(); // Synchronize start
        ctx1.collect();
    });
    
    // Thread 2: Allocate during collection
    let heap2 = Arc::clone(&heap);
    let barrier2 = Arc::clone(&barrier);
    let h2 = thread::spawn(move || {
        let ctx2 = GcContext::with_heap(heap2);
        barrier2.wait(); // Synchronize start
        thread::sleep(Duration::from_micros(100)); // Let collection start
        
        // These allocations happen DURING collection
        let mut live_objects = vec![];
        for i in 0..50 {
            live_objects.push(ctx2.allocate(Value { data: 1000 + i }));
        }
        live_objects // Keep them alive
    });
    
    barrier.wait(); // Start both threads
    
    h1.join().unwrap();
    let live_objects = h2.join().unwrap();
    
    // Verify objects allocated during collection are still alive
    println!("  Objects kept alive: {}", live_objects.len());
    assert_eq!(live_objects.len(), 50);
    
    // Access them to verify they're valid
    for (i, obj) in live_objects.iter().enumerate() {
        assert_eq!(obj.data, 1000 + i as i32);
    }
    
    println!("  ✓ Passed: Objects allocated during collection are safe\n");
}

/// High contention test - many threads allocating rapidly  
fn test_high_contention_allocation() {
    println!("Test 3: High contention allocation (no concurrent GC)");
    
    let ctx = GcContext::with_options(false, Duration::from_secs(100));
    let heap = Arc::clone(ctx.heap());
    let barrier = Arc::new(Barrier::new(10));
    let mut handles = vec![];
    
    // 10 allocator threads
    for thread_id in 0..10 {
        let heap_clone = Arc::clone(&heap);
        let barrier_clone = Arc::clone(&barrier);
        
        let handle = thread::spawn(move || {
            let ctx = GcContext::with_heap(heap_clone);
            barrier_clone.wait(); // Synchronize start
            
            let mut live = vec![];
            for i in 0..20 {
                let val = ctx.allocate(Value { 
                    data: thread_id * 1000 + i 
                });
                
                if i % 5 == 0 {
                    live.push(val); // Keep some alive
                }
            }
            live
        });
        handles.push(handle);
    }
    
    // Wait for all threads and KEEP the objects alive
    let mut all_kept_objects = vec![];
    for handle in handles {
        let kept = handle.join().unwrap();
        all_kept_objects.extend(kept);
    }
    
    let total_kept = all_kept_objects.len();
    println!("  Total objects kept alive: {}", total_kept);
    println!("  Total allocations in heap: {}", ctx.allocation_count());
    
    // We kept 4 objects per thread (10 threads = 40 objects)
    assert_eq!(ctx.allocation_count(), 200); // 10 threads * 20 each
    assert_eq!(total_kept, 40); // 10 threads * 4 kept each
    
    // Verify objects are valid (test allocation safety - no corruption)
    assert_eq!(all_kept_objects[0].data, 0); // First from thread 0
    assert_eq!(all_kept_objects[1].data, 5); // Second from thread 0
    
    // Now drop most objects and collect
    all_kept_objects.truncate(10); // Keep only 10
    ctx.collect();
    
    println!("  After collection: {} allocations", ctx.allocation_count());
    assert_eq!(ctx.allocation_count(), 10);
    
    println!("  ✓ Passed: No races in concurrent allocation\n");
}
