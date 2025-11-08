use abfall::{GcContext, Trace, Tracer};
use std::sync::Arc;
use std::thread;

struct Value {
    data: i32,
}

unsafe impl Trace for Value {
    fn trace(&self, _tracer: &mut Tracer) {}
}

fn main() {
    println!("=== Simple Allocation Safety Test ===\n");
    
    let ctx = Arc::new(GcContext::with_options(false, std::time::Duration::from_secs(10)));
    
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
    
    // Test 2: Multiple threads allocating concurrently
    println!("Test: Concurrent allocation from multiple threads");
    let mut handles = vec![];
    
    for i in 0..5 {
        let ctx = Arc::clone(&ctx);
        handles.push(thread::spawn(move || {
            for j in 0..10 {
                let _ = ctx.allocate(Value { data: i * 10 + j });
            }
        }));
    }
    
    for h in handles {
        h.join().unwrap();
    }
    
    let count_before = ctx.allocation_count();
    println!("After concurrent allocation: count = {}", count_before);
    assert!(count_before >= 51); // 1 from before + 50 new
    
    // Keep v1 alive, collect everything else
    ctx.collect();
    let count_after = ctx.allocation_count();
    println!("After GC: count = {}", count_after);
    assert_eq!(count_after, 1); // Only v1 remains
    
    println!("✓ Test passed!\n");
    
    println!("✅ ALL TESTS PASSED - Allocation safety fix works!");
}
