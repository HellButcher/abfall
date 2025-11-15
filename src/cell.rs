//! Interior mutability with write barriers for concurrent GC
//!
//! This module provides cells with write barriers for the tri-color marking algorithm:
//! - `GcCell<T>`: Stores traceable value with write barrier
//!
//! For non-traced types (primitives, etc.), use `std::cell::Cell<T>` directly since
//! they cannot contain GC pointers and don't need write barriers.

use crate::{
    gc::with_current_context,
    trace::{Trace, Tracer},
};
use std::cell::UnsafeCell;

/// Cell for storing GC-traceable values with write barrier
///
/// `GcCell` enables mutation of values while maintaining the
/// tri-color invariant during concurrent marking.
///
/// # Write Barrier
///
/// When a value is stored during marking, the cell traces the new
/// value to ensure any GC pointers it contains are marked gray.
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

    /// Set the contained value with write barrier
    ///
    /// If marking is in progress, traces the new value to shade
    /// any GC pointers gray, preventing premature collection.
    pub fn set(&self, new_value: T) {
        // Dijkstra write barrier: shade new pointer gray
        // (To avoid race-conditions, we don't check is_marking here; overhead should be minimal)
        unsafe {
            let new_ref = &new_value;
            with_current_context(|ctx| {
                if ctx.heap.check_is_marking_and_increment_busy() {
                    // Trace new value to shade it gray
                    new_ref.trace(&ctx.local_gray);
                    ctx.heap.merge_work(&ctx.local_gray);
                    ctx.heap.decrement_busy_marking();
                }
            });
            *self.value.get() = new_value;
        }
    }
}

impl<T> std::fmt::Debug for GcCell<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcCell").finish_non_exhaustive()
    }
}

unsafe impl<T: Trace> Trace for GcCell<T> {
    fn trace(&self, tracer: &Tracer) {
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
        let ctx = GcContext::off();
        let value1 = ctx.allocate(10);
        let value2_unrooted = ctx.allocate(20).as_ptr();

        let cell_ptr = ctx.allocate(GcCell::new(value1.as_ptr()));

        let old_allocated = ctx.heap().bytes_allocated();

        // marking (partial marking step for test)
        ctx.heap().try_mark_full();

        assert!(
            unsafe { &*value2_unrooted.header_ptr() }.is_white(),
            "Value2 should still be white here"
        );

        // Mutation during marking - write barrier should shade value2
        cell_ptr.set(value2_unrooted);

        assert!(
            !unsafe { &*value2_unrooted.header_ptr() }.is_white(),
            "Value2 is now gray after write barrier"
        );

        // Finish with sweeping
        ctx.heap().sweep_and_finish();

        let new_allocated = ctx.heap().bytes_allocated();
        assert_eq!(
            new_allocated, old_allocated,
            "No allocations should be freed"
        );

        assert_eq!(unsafe { *value2_unrooted.as_ptr() }, 20);
    }
}
