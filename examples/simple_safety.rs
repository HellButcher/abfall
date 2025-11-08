use abfall::{GcContext, Trace, Tracer};
use std::thread;

struct Value {
    data: i32,
}

unsafe impl Trace for Value {
    fn trace(&self, _tracer: &mut Tracer) {}
}

fn main() {
    println!("=== Simple Allocation Safety Test ===\n");
    
    let ctx = GcContext::new();
    
    // Test 1: Allocation safety - objects start rooted
    println!("Test: Objects are immediately rooted");
    let v1 = ctx.allocate(Value { data: 1 });
    let v2 = ctx.allocate(Value { data: 2 });
    let v3 = ctx.allocate(Value { data: 3 });
    
    println!("Allocated 3 objects: count = {}", ctx.allocation_count());
    assert_eq!(ctx.allocation_count(), 3);
    
    drop(v2);
    drop(v3);
    println!("Dropped 2 objects: count = {}", ctx.allocation_count());
    
    ctx.collect();
    println!("After GC: count = {}", ctx.allocation_count());
    assert_eq!(ctx.allocation_count(), 1);
    assert_eq!(v1.data, 1);
    
    println!("✓ Test passed!\n");
    
    // Test 2: Multiple threads with separate heaps
    println!("Test: Each thread has its own GC heap");
    let mut handles = vec![];
    
    for i in 0..5 {
        handles.push(thread::spawn(move || {
            let ctx = GcContext::new();
            for j in 0..10 {
                let _ = ctx.allocate(Value { data: i * 10 + j });
            }
            let count = ctx.allocation_count();
            println!("Thread {} allocated {} objects", i, count);
            assert_eq!(count, 10);
        }));
    }
    
    for h in handles {
        h.join().unwrap();
    }
    
    // Main thread's context still has 1 object
    let count_main = ctx.allocation_count();
    println!("Main thread: count = {}", count_main);
    assert_eq!(count_main, 1);
    
    println!("✓ Test passed!\n");
    
    println!("✅ ALL TESTS PASSED - Thread-local GC works!");
}
