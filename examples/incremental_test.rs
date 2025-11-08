use abfall::{GcContext, Trace, Tracer};
use std::sync::Arc;

struct Value {
    data: i32,
}

unsafe impl Trace for Value {
    fn trace(&self, _tracer: &mut Tracer) {}
}

fn main() {
    println!("=== Incremental Marking Test ===\n");
    
    let ctx = Arc::new(GcContext::with_options(false, std::time::Duration::from_secs(100)));
    
    // Allocate several objects
    let v1 = ctx.allocate(Value { data: 1 });
    let v2 = ctx.allocate(Value { data: 2 });
    let v3 = ctx.allocate(Value { data: 3 });
    let v4 = ctx.allocate(Value { data: 4 });
    let v5 = ctx.allocate(Value { data: 5 });
    
    println!("Allocated 5 objects");
    assert_eq!(ctx.allocation_count(), 5);
    
    // Drop some objects
    drop(v2);
    drop(v4);
    
    println!("Dropped 2 objects");
    
    // Use incremental collection
    println!("Running incremental collection with work budget of 2 objects per step...");
    ctx.collect_incremental(2);
    
    println!("After incremental GC: {} objects remain", ctx.allocation_count());
    assert_eq!(ctx.allocation_count(), 3);
    
    // Verify remaining objects are still valid
    assert_eq!(v1.data, 1);
    assert_eq!(v3.data, 3);
    assert_eq!(v5.data, 5);
    
    println!("✓ Test passed: Incremental marking works!");
    println!("  - Objects collected correctly");
    println!("  - Remaining objects still accessible");
}
