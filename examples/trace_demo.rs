use abfall::{GcContext, GcPtr, Trace, Tracer};
use std::sync::Arc;

// A simple linked list node
struct Node {
    value: i32,
    next: Option<GcPtr<Node>>,
}

// Implement Trace to tell the GC how to follow pointers
unsafe impl Trace for Node {
    fn trace(&self, tracer: &mut Tracer) {
        if let Some(ref next) = self.next {
            tracer.mark(next);
        }
    }
}

fn main() {
    println!("=== Trace Trait Demo: Linked List ===\n");
    
    let ctx = Arc::new(GcContext::with_options(false, std::time::Duration::from_secs(1)));
    
    // Create a linked list: 1 -> 2 -> 3
    // Note: We move the nodes, not clone them
    println!("Creating linked list: 1 -> 2 -> 3");
    let node1 = ctx.allocate(Node { 
        value: 1, 
        next: Some(ctx.allocate(Node {
            value: 2,
            next: Some(ctx.allocate(Node {
                value: 3,
                next: None
            }))
        }))
    });
    
    println!("Allocations: {}, Bytes: {}", ctx.allocation_count(), ctx.bytes_allocated());
    
    // Traverse the list
    println!("\nTraversing list:");
    let mut current = Some(node1.clone());
    while let Some(node) = current {
        println!("  Node value: {}", node.value);
        current = node.next.clone();
    }
    
    println!("\nAll nodes only reachable through node1");
    println!("Before collection: {} allocations", ctx.allocation_count());
    
    // Collection should NOT remove any nodes because node1 is still a root
    ctx.collect();
    
    println!("After collection: {} allocations", ctx.allocation_count());
    println!("Expected: 3 (all nodes still reachable through node1)");
    
    // Verify list is still intact
    println!("\nVerifying list is still intact:");
    let mut current = Some(node1.clone());
    while let Some(node) = current {
        println!("  Node value: {}", node.value);
        current = node.next.clone();
    }
    
    // Now drop the head
    println!("\nDropping node1 (head) - makes entire list unreachable");
    drop(node1);
    
    println!("Before collection: {} allocations", ctx.allocation_count());
    
    // Now all nodes should be collected
    ctx.collect();
    
    println!("After collection: {} allocations", ctx.allocation_count());
    println!("Expected: 0 (all nodes unreachable)");
    
    if ctx.allocation_count() == 0 {
        println!("\n✓ TEST PASSED: Trace-based collection works correctly!");
    } else {
        println!("\n✗ TEST FAILED: {} nodes not collected!", ctx.allocation_count());
    }
}
