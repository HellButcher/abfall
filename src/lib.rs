//! Abfall - A concurrent tri-color tracing mark and sweep garbage collector for Rust
//!
//! This library implements a concurrent garbage collector using the tri-color marking algorithm.
//! It provides automatic memory management with minimal pause times through concurrent collection.
//!
//! # Features
//!
//! - **Tri-Color Marking**: Objects are marked as white (unreachable), gray (reachable but unscanned),
//!   or black (reachable and scanned)
//! - **Concurrent Collection**: Background thread performs collection without stopping application
//! - **Thread-Safe**: Safe to use across multiple threads
//! - **Manual Control**: Option to disable automatic collection and trigger manually
//!
//! # Example
//!
//! ```
//! use abfall::GcContext;
//! use std::sync::Arc;
//!
//! // Create a GC context with automatic background collection
//! let ctx = GcContext::new();
//!
//! // Allocate objects on the GC heap (returns GcRoot)
//! let value = ctx.allocate(42);
//! let text = ctx.allocate("Hello, GC!");
//!
//! // Access through Deref
//! assert_eq!(*value, 42);
//! assert_eq!(*text, "Hello, GC!");
//! ```

mod cell;
mod color;
mod gc;
mod gc_box;
mod heap;
mod ptr;
mod trace;

pub use cell::GcCell;
pub use gc::GcContext;
pub use heap::{Heap, GcOptions};
pub use ptr::{GcPtr, GcRoot};
pub use trace::{Trace, Tracer};

#[cfg(test)]
mod tests {
    use crate::heap::GcOptions;

    use super::*;
    use std::time::Duration;

    #[test]
    fn basic_allocation() {
        let ctx = GcContext::new();
        let ptr = ctx.allocate(42);
        assert_eq!(*ptr, 42);
    }

    #[test]
    fn allocation_and_collection() {
        let ctx = GcContext::new();
        let ptr1 = ctx.allocate(100);
        let _ptr2 = ctx.allocate(200);
        drop(_ptr2);
        ctx.collect();
        assert_eq!(*ptr1, 100);
    }

    #[test]
    fn concurrent_collection() {
        use std::sync::Arc;
        use std::thread;

        // Create a heap with concurrent collection and low threshold
        let opts = heap::GcOptions {
            min_threshold_bytes: 1024,
            ..GcOptions::DEFAULT
        };
        let ctx = GcContext::with_options(opts);
        let heap = Arc::clone(ctx.heap());

        // Allocate some objects
        let roots: Vec<_> = (0..10).map(|i| ctx.allocate(i)).collect();

        let initial_bytes = heap.bytes_allocated();

        // Allocate and drop many objects in separate threads
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let heap = Arc::clone(&heap);
                thread::spawn(move || {
                    let ctx = GcContext::with_heap(heap);
                    for _ in 0..100 {
                        let _temp = ctx.allocate(vec![1, 2, 3, 4, 5]);
                        // Drop immediately - should be collected
                    }
                })
            })
            .collect();

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        let peak_bytes = heap.bytes_allocated();

        // Give background GC time to collect
        thread::sleep(Duration::from_millis(300));

        // Force a final collection
        heap.force_collect();

        // Verify roots are still alive
        for (i, root) in roots.iter().enumerate() {
            assert_eq!(**root, i);
        }

        // Verify memory was reclaimed (should be close to initial)
        let final_bytes = heap.bytes_allocated();
        println!(
            "Initial: {}, Peak: {}, Final: {}",
            initial_bytes, peak_bytes, final_bytes
        );

        // Final should be much less than peak (most temporary objects collected)
        assert!(
            final_bytes < peak_bytes / 2,
            "GC should have reclaimed significant memory"
        );
    }

    #[test]
    fn incremental_marking() {
        let ctx = GcContext::new();

        // Create a chain of objects
        let root = ctx.allocate(vec![1, 2, 3]);
        let _temp = ctx.allocate(vec![4, 5, 6]);

        // Manually trigger incremental marking
        ctx.heap().force_collect();

        // Root should still be alive
        assert_eq!(root.len(), 3);
    }
}
