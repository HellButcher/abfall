//! Example demonstrating the Trace trait for tracking object references
//!
//! This example shows how the GC uses the Trace trait to follow pointers
//! and determine which objects are reachable. It demonstrates:
//! - Implementing Trace for custom types
//! - How reachability preserves objects during collection
//! - How unreachable objects are collected

use abfall::{GcContext, GcPtr, Trace, Tracer};

// A simple linked list node
struct Node {
    value: i32,
    next: Option<GcPtr<Node>>,
}

// Implement Trace to tell the GC how to follow pointers
unsafe impl Trace for Node {
    fn trace(&self, tracer: &Tracer) {
        if let Some(ref next) = self.next {
            tracer.mark(next);
        }
    }
}

fn main() {
    println!("=== Trace Trait Demo: Linked List ===\n");

    let ctx = GcContext::new();

    // Create a linked list: 1 -> 2 -> 3
    println!("Creating linked list: 1 -> 2 -> 3");

    let node1 = {
        let node3 = ctx.allocate(Node {
            value: 3,
            next: None,
        });
        let node2 = ctx.allocate(Node {
            value: 2,
            next: Some(node3.as_ptr()),
        });
        ctx.allocate(Node {
            value: 1,
            next: Some(node2.as_ptr()),
        })
        // node2 and node3 GcRoots are dropped here, but they're still reachable via node1
    };

    println!(
        "Allocations: {}, Bytes: {}",
        ctx.allocation_count(),
        ctx.bytes_allocated()
    );

    // Traverse the list
    println!("\nTraversing list:");
    let mut current = Some(node1.as_ptr());
    while let Some(ptr) = current {
        // Root the pointer to access it
        let node = unsafe { ptr.root() };
        println!("  Node value: {}", node.value);
        current = node.next;
    }

    println!("\nAll nodes only reachable through node1");
    println!("Before collection: {} allocations", ctx.allocation_count());

    // Collection should NOT remove any nodes because node1 is still a root
    ctx.collect();

    println!("After collection: {} allocations", ctx.allocation_count());
    println!("Expected: 3 (all nodes still reachable through node1)");

    // Verify list is still intact
    println!("\nVerifying list is still intact:");
    let mut current = Some(node1.as_ptr());
    while let Some(ptr) = current {
        let node = unsafe { ptr.root() };
        println!("  Node value: {}", node.value);
        current = node.next;
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
        println!("\n✓ Trace-based collection works correctly!");
    } else {
        println!(
            "\n✗ TEST FAILED: {} nodes not collected!",
            ctx.allocation_count()
        );
    }
}
