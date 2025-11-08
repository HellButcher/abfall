//! Test GcPtrCell with write barriers and incremental collection

use abfall::{GcCell, GcContext, GcPtr, Trace, Tracer};

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
    target: GcCell<GcPtr<Counter>>,
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
        target: GcCell::new(counter_a.as_ptr()),
    });

    // Access the counter through rooting
    let target = unsafe { node.target.get().root() };
    println!("Node {} -> Counter {}", node.id, target.value);

    // Change node to point to counter_b
    println!("\nChanging node target to counter_b...");
    node.target.set(counter_b.as_ptr());
    let target = unsafe { node.target.get().root() };
    println!("Node {} -> Counter {}", node.id, target.value);

    // Now test write barrier during incremental GC
    println!("\n=== Testing Write Barrier During Incremental GC ===");
    
    // Start marking phase
    println!("Starting incremental GC (marking phase)...");
    ctx.begin_mark();
    assert!(ctx.is_marking());
    
    // Mutate during marking - write barrier should shade counter_c gray
    println!("Mutating node target to counter_c during marking...");
    println!("  Write barrier will shade counter_c gray");
    node.target.set(counter_c.as_ptr());
    
    // Complete marking
    println!("Completing marking...");
    while !ctx.do_mark_work(10) {}
    
    // Sweep
    println!("Sweeping...");
    ctx.sweep();
    
    // Verify counter_c is still alive
    println!("\nAfter GC:");
    let target = unsafe { node.target.get().root() };
    println!("  Node {} -> Counter {}", node.id, target.value);
    assert_eq!(target.value, 300);

    // Drop counter_a and counter_b, collect
    println!("\nDropping counter_a and counter_b, running GC...");
    drop(counter_a);
    drop(counter_b);
    ctx.collect();

    // counter_c should still be alive (pointed to by node)
    let target = unsafe { node.target.get().root() };
    println!("After GC: Node {} -> Counter {}", node.id, target.value);
    assert_eq!(target.value, 300);

    println!("\n✓ Write barrier test passed - tri-color invariant maintained!");
}
