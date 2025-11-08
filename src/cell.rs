//! Interior mutability with write barriers for concurrent GC
//!
//! This module provides cells with write barriers for the tri-color marking algorithm:
//! - `GcPtrCell<T>`: Stores GcPtr with Dijkstra write barrier
//! - `GcRefCell<T>`: RefCell-like with Yuasa write barrier
//!
//! For `Copy` types (primitives, etc.), use `std::cell::Cell<T>` directly since
//! they cannot contain GC pointers and don't need write barriers.

use crate::gc::current_heap;
use crate::ptr::GcPtr;
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
pub struct GcPtrCell<T> {
    value: UnsafeCell<GcPtr<T>>,
}

impl<T> GcPtrCell<T> {
    pub fn new(value: GcPtr<T>) -> Self {
        Self {
            value: UnsafeCell::new(value),
        }
    }

    pub fn get(&self) -> GcPtr<T> {
        unsafe { (*self.value.get()).clone() }
    }

    /// Set the contained pointer with Dijkstra write barrier
    pub fn set(&self, new_value: GcPtr<T>) {
        // Get heap from thread-local context
        let heap = current_heap();
        
        // Dijkstra barrier: shade new pointer gray if marking
        if heap.is_marking() {
            heap.mark_gray(new_value.header_ptr());
        }
        
        unsafe {
            *self.value.get() = new_value;
        }
    }

    pub fn replace(&self, new_value: GcPtr<T>) -> GcPtr<T> {
        // Get heap from thread-local context
        let heap = current_heap();
        
        // Dijkstra barrier: shade new pointer gray if marking
        if heap.is_marking() {
            heap.mark_gray(new_value.header_ptr());
        }
        
        unsafe {
            std::ptr::replace(self.value.get(), new_value)
        }
    }

    pub fn swap(&self, other: &Self) {
        // Both pointers stay reachable through the cells, no barrier needed
        unsafe {
            std::ptr::swap(self.value.get(), other.value.get());
        }
    }
}

impl<T> std::fmt::Debug for GcPtrCell<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcPtrCell").finish_non_exhaustive()
    }
}

unsafe impl<T> Trace for GcPtrCell<T> {
    fn trace(&self, tracer: &mut Tracer) {
        unsafe {
            (*self.value.get()).trace(tracer);
        }
    }
}

unsafe impl<T: Send> Send for GcPtrCell<T> {}
unsafe impl<T: Sync> Sync for GcPtrCell<T> {}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::GcContext;

    #[test]
    fn test_gcptrcell_basic() {
        let ctx = GcContext::new();
        let value1 = ctx.allocate(10);
        let value2 = ctx.allocate(20);
        
        let cell = GcPtrCell::new(value1.clone());
        assert_eq!(*cell.get(), 10);
        
        cell.set(value2.clone());
        assert_eq!(*cell.get(), 20);
    }

    #[test]
    fn test_gcptrcell_write_barrier() {
        let ctx = GcContext::new();
        let value1 = ctx.allocate(10);
        let value2 = ctx.allocate(20);
        
        let cell_ptr = ctx.allocate(GcPtrCell::new(value1.clone()));
        
        // Start marking
        ctx.heap().begin_mark();
        assert!(ctx.heap().is_marking());
        
        // Mutation during marking - write barrier should shade value2
        let cell = unsafe { &*cell_ptr.as_ptr() };
        cell.set(value2.clone());
        
        // Complete marking
        while !ctx.heap().do_mark_work(10) {}
        
        // Both values should be reachable
        ctx.heap().sweep();
        assert_eq!(*value2, 20);
    }
}
