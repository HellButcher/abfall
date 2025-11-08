//! Garbage collector context and main API
//!
//! This module provides thread-local GC context management via `GcContext`.
//! Each thread has its own heap, accessed through a RAII guard.

use crate::heap::Heap;
use crate::ptr::GcPtr;
use crate::trace::Trace;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::Arc;

thread_local! {
    static CURRENT_HEAP: RefCell<Option<Arc<Heap>>> = const { RefCell::new(None) };
}

/// Get the current thread-local heap
///
/// # Panics
/// Panics if no GcContext is active in this thread
pub(crate) fn current_heap() -> Arc<Heap> {
    CURRENT_HEAP.with(|h| {
        h.borrow().clone().expect("No GcContext active in this thread. Create a GcContext first.")
    })
}

/// Set the current thread-local heap
fn set_current_heap(heap: Option<Arc<Heap>>) {
    CURRENT_HEAP.with(|h| {
        *h.borrow_mut() = heap;
    });
}

/// RAII guard for GC context
///
/// While this guard is alive, the thread has an active GC context.
/// Dropping the guard clears the thread-local context.
///
/// # Example
///
/// ```
/// use abfall::GcContext;
///
/// let ctx = GcContext::new();
/// let value = ctx.allocate(42);
/// // ctx is dropped here, clearing thread-local context
/// ```
pub struct GcContext {
    heap: Arc<Heap>,
    _non_send_or_sync: PhantomData<*const ()>,
}

impl Default for GcContext {
    fn default() -> Self {
        Self::new()
    }
}

impl GcContext {
    /// Create a new GC context for the current thread
    ///
    /// Sets this as the active context for allocations in this thread.
    ///
    /// # Example
    ///
    /// ```
    /// use abfall::GcContext;
    ///
    /// let ctx = GcContext::new();
    /// let value = ctx.allocate(42);
    /// ```
    pub fn new() -> Self {
        let heap = Heap::new();
        // TODO: associate GcContext with Heap
        set_current_heap(Some(Arc::clone(&heap)));
        Self { heap, _non_send_or_sync: PhantomData }

    }

    /// Allocate an object on the GC heap
    ///
    /// Returns a `GcPtr` that keeps the object alive. When all pointers
    /// to an object are dropped, it becomes eligible for collection.
    ///
    /// # Example
    ///
    /// ```
    /// use abfall::GcContext;
    ///
    /// let ctx = GcContext::new();
    /// let number = ctx.allocate(42);
    /// let text = ctx.allocate("Hello");
    /// ```
    pub fn allocate<T: Trace>(&self, data: T) -> GcPtr<T> {
        let ptr = self.heap.allocate(data);
        GcPtr::new(ptr)
    }

    /// Manually trigger a garbage collection cycle
    ///
    /// This performs a full mark-and-sweep collection.
    ///
    /// # Example
    ///
    /// ```
    /// use abfall::GcContext;
    ///
    /// let ctx = GcContext::new();
    /// let ptr = ctx.allocate(100);
    /// drop(ptr);
    /// ctx.collect(); // Reclaim memory
    /// ```
    pub fn collect(&self) {
        self.heap.mark_from_roots();
        self.heap.sweep();
    }
    
    /// Perform an incremental collection with bounded work per step
    ///
    /// This allows the GC to perform work in small increments, reducing
    /// pause times. The `work_per_step` parameter controls how many objects
    /// are processed in each marking step.
    ///
    /// # Example
    ///
    /// ```
    /// use abfall::GcContext;
    ///
    /// let ctx = GcContext::new();
    /// let ptr = ctx.allocate(100);
    /// drop(ptr);
    /// ctx.collect_incremental(10); // Process 10 objects per step
    /// ```
    pub fn collect_incremental(&self, work_per_step: usize) {
        self.heap.collect_incremental(work_per_step);
    }

    /// Get reference to the underlying heap (for advanced use)
    pub fn heap(&self) -> &Arc<Heap> {
        &self.heap
    }
}

impl Drop for GcContext {
    fn drop(&mut self) {
        // Clear thread-local heap when context is dropped
        set_current_heap(None);
        // TODO: disassociate GcContext from Heap
    }
}
