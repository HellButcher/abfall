//! Example of sharing a GC heap across multiple threads
//!
//! This example demonstrates how to use the same GC heap from multiple threads
//! by cloning the heap Arc and creating a new GcContext in each thread.

use abfall::{GcContext, Trace, GcPtr};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// A simple tree node structure for demonstration
struct Node {
    value: i32,
    left: Option<GcPtr<Node>>,
    right: Option<GcPtr<Node>>,
}

unsafe impl Trace for Node {
    fn trace(&self, tracer: &mut abfall::Tracer) {
        if let Some(left) = self.left {
            tracer.mark(&left);
        }
        if let Some(right) = self.right {
            tracer.mark(&right);
        }
    }
}

fn main() {
    println!("=== Multi-threaded GC Example ===\n");

    // Create a GC context in the main thread
    let ctx = GcContext::new();
    
    // Get a reference to the heap to share with other threads
    let heap = Arc::clone(ctx.heap());
    
    println!("Main thread: Created GC context");
    println!("Initial allocations: {}", ctx.allocation_count());
    
    // Allocate some objects in the main thread
    let root1 = ctx.allocate(Node {
        value: 1,
        left: None,
        right: None,
    });
    
    let root2 = ctx.allocate(Node {
        value: 2,
        left: None,
        right: None,
    });
    
    println!("Main thread: Allocated {} objects", ctx.allocation_count());
    
    // Spawn multiple threads that share the same heap
    let mut handles = vec![];
    
    for thread_id in 0..3 {
        let heap_clone = Arc::clone(&heap);
        
        let handle = thread::spawn(move || {
            // Each thread creates its own GcContext using the shared heap
            let ctx = GcContext::with_heap(heap_clone);
            
            println!("Thread {}: Created GC context", thread_id);
            
            // Allocate objects in this thread
            let local_root = ctx.allocate(Node {
                value: 100 + thread_id,
                left: None,
                right: None,
            });
            
            println!(
                "Thread {}: Allocated node with value {}, total allocations: {}",
                thread_id,
                local_root.value,
                ctx.allocation_count()
            );
            
            // Do some work
            thread::sleep(Duration::from_millis(10));
            
            // Return some value to verify thread completion
            local_root.value
        });
        
        handles.push(handle);
    }
    
    // Wait for all threads to complete
    println!("\nWaiting for threads to complete...");
    for (i, handle) in handles.into_iter().enumerate() {
        match handle.join() {
            Ok(value) => println!("Thread {} finished with value: {}", i, value),
            Err(e) => println!("Thread {} panicked: {:?}", i, e),
        }
    }
    
    println!("\nAll threads completed!");
    println!("Total allocations after threads: {}", ctx.allocation_count());
    println!("Total bytes allocated: {}", ctx.bytes_allocated());
    
    // Access the roots we created in the main thread
    println!("\nMain thread roots still accessible:");
    println!("  root1.value = {}", root1.value);
    println!("  root2.value = {}", root2.value);
    
    // Trigger a collection
    println!("\nTriggering garbage collection...");
    ctx.collect();
    
    println!("Allocations after collection: {}", ctx.allocation_count());
    println!("Bytes after collection: {}", ctx.bytes_allocated());
    
    // The roots are still alive
    println!("\nRoots still alive after collection:");
    println!("  root1.value = {}", root1.value);
    println!("  root2.value = {}", root2.value);
    
    println!("\n=== Example Complete ===");
}
