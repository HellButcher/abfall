use abfall::{GcContext, Trace, Tracer};
use std::sync::Arc;

struct Value {
    data: i32,
}

unsafe impl Trace for Value {
    fn trace(&self, _tracer: &mut Tracer) {}
}

fn main() {
    let ctx = Arc::new(GcContext::with_options(false, std::time::Duration::from_secs(100)));
    
    println!("=== Tracing GC Behavior ===\n");
    
    let v1 = ctx.allocate(Value { data: 1 });
    let v2 = ctx.allocate(Value { data: 2 });
    let v3 = ctx.allocate(Value { data: 3 });
    
    println!("After allocation: {} objects", ctx.allocation_count());
    
    // Drop v2
    println!("Dropping v2...");
    drop(v2);
    
    // Try regular collection first
    println!("\nTrying regular collect()...");
    ctx.collect();
    println!("After regular collect: {} objects", ctx.allocation_count());
    
    println!("\nRemaining objects: v1={}, v3={}", v1.data, v3.data);
}
