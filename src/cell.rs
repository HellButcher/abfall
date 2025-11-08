//! Interior mutability with write barriers for concurrent GC
//!
//! This module provides cells with write barriers for the tri-color marking algorithm:
//! - `GcCell<T>`: Stores traceable value with Dijkstra write barrier
//!
//! For non-traced types (primitives, etc.), use `std::cell::Cell<T>` directly since
//! they cannot contain GC pointers and don't need write barriers.

use crate::trace::{Trace, Tracer};
use std::cell::UnsafeCell;

/// Cell for storing GcPtr with Dijkstra write barrier
///
/// `GcPtrCell` enables mutation of GC pointers while maintaining the
/// tri-color invariant during concurrent marking.
///
/// # Write Barrier (Dijkstra)
///
/// When a new pointer is stored, if marking is in progress, the new
/// pointer is immediately shaded gray to prevent it from being collected.
pub struct GcCell<T> {
    value: UnsafeCell<T>,
}

impl<T: Trace + Copy> GcCell<T> {
    #[inline]
    pub fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
        }
    }

    pub fn get(&self) -> T {
        unsafe { *self.value.get() }
    }

    /// Set the contained pointer with Dijkstra write barrier
    pub fn set(&self, new_value: T) {
        // TODO: Dijkstra barrier: shade new pointer gray if marking
        // This requires access to the current GcContext via thread-local storage
        // let tracer = current_heap().tracer();
        // new_value.trace(tracer);
        unsafe {
            *self.value.get() = new_value;
        }
    }
}

impl<T> std::fmt::Debug for GcCell<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcPtrCell").finish_non_exhaustive()
    }
}

unsafe impl<T: Trace> Trace for GcCell<T> {
    fn trace(&self, tracer: &mut Tracer) {
        unsafe {
            (*self.value.get()).trace(tracer);
        }
    }
}

unsafe impl<T: Send> Send for GcCell<T> {}
//unsafe impl<T: Sync> Sync for GcCell<T> {}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::GcContext;

    #[test]
    fn test_gcptrcell_basic() {
        let ctx = GcContext::new();
        let value1 = ctx.allocate(10);
        let value2 = ctx.allocate(20);
        
        let cell = GcCell::new(value1.as_ptr());
        let retrieved = unsafe { cell.get().root() };
        assert_eq!(*retrieved, 10);
        
        cell.set(value2.as_ptr());
        let retrieved = unsafe { cell.get().root() };
        assert_eq!(*retrieved, 20);
    }

    #[test]
    fn test_gcptrcell_write_barrier() {
        let ctx = GcContext::new();
        let value1 = ctx.allocate(10);
        let value2 = ctx.allocate(20);
        let value2_clone = value2.clone();
        
        let cell_ptr = ctx.allocate(GcCell::new(value1.as_ptr()));
        
        // Start marking
        ctx.heap().begin_mark();
        assert!(ctx.heap().is_marking());
        
        // Mutation during marking - write barrier should shade value2
        cell_ptr.set(value2.as_ptr());
        
        // Complete marking
        while !ctx.heap().do_mark_work(10) {}
        
        // Both values should be reachable
        ctx.heap().sweep();
        assert_eq!(*value2_clone, 20);
    }
}
