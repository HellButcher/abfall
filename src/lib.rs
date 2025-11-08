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
//! // Allocate objects on the GC heap
//! let value = ctx.allocate(42);
//! let text = ctx.allocate("Hello, GC!");
//!
//! // Access through Deref
//! assert_eq!(*value, 42);
//! assert_eq!(*text, "Hello, GC!");
//! ```

mod gc;
mod color;
mod heap;
mod ptr;
mod trace;

pub use gc::{Gc, GcContext};
pub use ptr::GcPtr;
pub use trace::{Trace, NoTrace, Tracer};

#[cfg(test)]
mod tests {
    use super::*;

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
    fn concurrent_allocation() {
        use std::sync::Arc;
        use std::thread;

        let ctx = Arc::new(GcContext::new());
        let mut handles = vec![];

        for i in 0..4 {
            let ctx_clone = Arc::clone(&ctx);
            let handle = thread::spawn(move || {
                let _ptr = ctx_clone.allocate(i);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }
}
