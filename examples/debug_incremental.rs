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
    
    let v1 = ctx.allocate(Value { data: 1 });
    let v2 = ctx.allocate(Value { data: 2 });
    
    println!("Allocated 2 objects, count: {}", ctx.allocation_count());
    
    drop(v2);
    println!("Dropped v2, count: {}", ctx.allocation_count());
    
    ctx.collect_incremental(10);
    
    println!("After GC, count: {}", ctx.allocation_count());
    println!("v1.data: {}", v1.data);
}
