//! Example demonstrating concurrent access to a shared GC heap from multiple threads
//!
//! This example shows how multiple threads can share the same GC heap by:
//! 1. Creating a GcContext in the main thread
//! 2. Cloning the heap Arc to share with worker threads
//! 3. Each worker thread sets up its own thread-local context pointing to the shared heap
//! 4. All threads allocate and mutate GC objects concurrently
//! 5. The GC runs concurrently with allocations

use abfall::{GcCell, GcContext, GcPtr, Trace, Tracer};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

// A simple node structure that can link to other nodes
#[derive(Debug)]
struct Node {
    id: usize,
    value: i32,
    next: GcCell<Option<GcPtr<Node>>>,
}

unsafe impl Trace for Node {
    fn trace(&self, tracer: &Tracer) {
        self.next.trace(tracer);
    }
}

fn main() {
    println!("=== Multi-threaded GC Example ===\n");

    // Create the main GC context
    let ctx = GcContext::new();
    println!("Created shared GC heap");

    // Get a reference to the heap to share with threads
    let heap = ctx.heap().clone();

    // Allocate a shared root node
    let root = ctx.allocate(Node {
        id: 0,
        value: 0,
        next: GcCell::new(None),
    });
    println!("Created root node (id=0)\n");

    // Number of worker threads
    const NUM_THREADS: usize = 4;
    const ITERATIONS: usize = 100;

    // Barrier to synchronize thread start (worker threads + monitor thread + main thread)
    let barrier = Arc::new(Barrier::new(NUM_THREADS + 2));

    // Spawn worker threads
    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|thread_id| {
            let heap = heap.clone();
            let root = root.clone();
            let barrier = barrier.clone();

            thread::spawn(move || {
                // Set up thread-local GC context with shared heap
                let ctx = GcContext::with_heap(heap);

                println!("Thread {} ready", thread_id);
                barrier.wait();

                let mut nodes = Vec::new();

                // Allocate nodes and link them
                for i in 0..ITERATIONS {
                    // Allocate a new node
                    let node = ctx.allocate(Node {
                        id: thread_id * 1000 + i,
                        value: (thread_id * 100 + i) as i32,
                        next: GcCell::new(None),
                    });

                    // Occasionally link to root (creates cross-thread references)
                    if i % 10 == 0 {
                        let current_next = root.next.get();
                        root.next.set(Some(node.as_ptr()));
                        node.next.set(current_next);

                        println!(
                            "Thread {} linked node {} (value={}) to root",
                            thread_id, node.id, node.value
                        );
                    }

                    nodes.push(node);

                    // Small delay to simulate work
                    if i % 20 == 0 {
                        thread::sleep(Duration::from_micros(10));
                    }
                }

                println!(
                    "Thread {} finished allocating {} nodes",
                    thread_id, ITERATIONS
                );

                println!("Thread {} ready for phase 2", thread_id);
                barrier.wait();

                drop(nodes); // Drop local references to allow collection
                root.next.set(None); // Clear root links
            })
        })
        .collect();

    // Monitor and trigger GC periodically
    let monitor_handle = {
        let heap = heap.clone();
        let barrier = barrier.clone();
        thread::spawn(move || {
            println!("Monitor Thread ready");
            barrier.wait();

            let collect = || {
                for round in 0..20 {
                    thread::sleep(Duration::from_millis(5));

                    let bytes = heap.bytes_allocated();
                    let count = heap.allocation_count();

                    println!(
                        "[Monitor] Round {}: {} allocations, {} bytes",
                        round, count, bytes
                    );

                    // Trigger incremental GC
                    if bytes > 1024 * 10 {
                        println!("[Monitor] Triggering GC...");
                        heap.collect();

                        let bytes_after = heap.bytes_allocated();
                        let count_after = heap.allocation_count();
                        println!(
                            "[Monitor] After GC: {} allocations, {} bytes",
                            count_after, bytes_after
                        );
                    }
                }
            };

            collect();

            println!("Monitor Thread ready for phase 2");
            barrier.wait();

            collect();
        })
    };

    // Wait for all threads to be ready
    barrier.wait();
    println!("\nAll threads started!\n");

    // Wait for all threads to be ready
    barrier.wait();
    println!("\nAll threads in phase 2!\n");

    // Wait for worker threads
    for (i, handle) in handles.into_iter().enumerate() {
        handle.join().unwrap();
        println!("Thread {} joined", i);
    }

    monitor_handle.join().unwrap();
    println!("\nMonitor thread joined");

    // Final statistics
    println!("\n=== Final Statistics ===");
    println!("Allocations: {}", ctx.heap().allocation_count());
    println!("Bytes allocated: {}", ctx.heap().bytes_allocated());

    // Perform final full collection
    println!("\nPerforming final collection...");
    ctx.collect();

    println!("After collection:");
    println!("  Allocations: {}", ctx.heap().allocation_count());
    println!("  Bytes allocated: {}", ctx.heap().bytes_allocated());

    // Traverse from root to count reachable nodes
    let mut reachable = 0;
    let mut current = root.next.get();
    let mut visited = std::collections::HashSet::new();

    while let Some(node_ptr) = current {
        // Root the pointer to access fields
        let node = unsafe { node_ptr.root() };
        let node_id = node.id;
        if visited.contains(&node_id) {
            break; // Cycle detected
        }
        visited.insert(node_id);
        reachable += 1;
        current = node.next.get();
    }

    println!("\nReachable nodes from root: {}", reachable);
    println!("\n=== Example Complete ===");
}
