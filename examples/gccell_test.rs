//! Test GcPtrCell with write barriers and incremental collection

use abfall::{GcPtrCell, GcContext, Trace, Tracer};
use std::sync::Arc;

struct Counter {
    value: i32,
}

unsafe impl Trace for Counter {
    fn trace(&self, _tracer: &mut Tracer) {
        // No GC pointers
    }
}

struct Node {
    id: i32,
    target: GcPtrCell<Counter>,
}

unsafe impl Trace for Node {
    fn trace(&self, tracer: &mut Tracer) {
        self.target.trace(tracer);
    }
}

fn main() {
    println!("=== GcPtrCell with Write Barriers Test ===\n");

    let ctx = GcContext::new();

    // Create counters
    println!("Creating counters...");
    let counter_a = ctx.allocate(Counter { value: 100 });
    let counter_b = ctx.allocate(Counter { value: 200 });
    let counter_c = ctx.allocate(Counter { value: 300 });

    // Create node pointing to counter_a
    let node = ctx.allocate(Node {
        id: 1,
        target: GcPtrCell::new(counter_a.clone(), Arc::clone(ctx.heap())),
    });

    println!("Node {} -> Counter {}", node.id, node.target.get().value);

    // Change node to point to counter_b
    println!("\nChanging node target to counter_b...");
    node.target.set(counter_b.clone());
    println!("Node {} -> Counter {}", node.id, node.target.get().value);

    // Now test write barrier during incremental GC
    println!("\n=== Testing Write Barrier During Incremental GC ===");
    
    // Start marking phase
    println!("Starting incremental GC (marking phase)...");
    ctx.heap().begin_mark();
    assert!(ctx.heap().is_marking());
    
    // Mutate during marking - write barrier should shade counter_c gray
    println!("Mutating node target to counter_c during marking...");
    println!("  Write barrier will shade counter_c gray");
    node.target.set(counter_c.clone());
    
    // Complete marking
    println!("Completing marking...");
    while !ctx.heap().do_mark_work(10) {}
    
    // Sweep
    println!("Sweeping...");
    ctx.heap().sweep();
    
    // Verify counter_c is still alive
    println!("\nAfter GC:");
    println!("  Node {} -> Counter {}", node.id, node.target.get().value);
    assert_eq!(node.target.get().value, 300);

    // Drop counter_a and counter_b, collect
    println!("\nDropping counter_a and counter_b, running GC...");
    drop(counter_a);
    drop(counter_b);
    ctx.collect();

    // counter_c should still be alive (pointed to by node)
    println!("After GC: Node {} -> Counter {}", node.id, node.target.get().value);
    assert_eq!(node.target.get().value, 300);

    println!("\n✓ Write barrier test passed - tri-color invariant maintained!");
}
