//! Garbage collector context and main API
//!
//! This module provides thread-local GC context management via `GcContext`.
//! Each thread has its own heap, accessed through a RAII guard.

use crate::heap::Heap;
use crate::trace::Trace;
use std::cell::RefCell;
use std::sync::Arc;

thread_local! {
    static CURRENT_HEAP: RefCell<Option<Arc<Heap>>> = const { RefCell::new(None) };
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
/// GcContext is not Send or Sync because it manages a thread-local variable.
/// To share a heap across threads, clone the underlying heap and create a new
/// GcContext in each thread.
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
    _marker: std::marker::PhantomData<*const ()>, // Makes GcContext !Send + !Sync
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
        set_current_heap(Some(Arc::clone(&heap)));
        Self {
            heap,
            _marker: std::marker::PhantomData,
        }
    }

    /// Create a new GC context with custom options
    pub fn with_options(concurrent: bool, collection_interval: std::time::Duration) -> Self {
        let heap = Heap::with_options(concurrent, collection_interval);
        set_current_heap(Some(Arc::clone(&heap)));
        Self {
            heap,
            _marker: std::marker::PhantomData,
        }
    }

    /// Create a new GC context for the current thread using a shared heap
    ///
    /// This allows multiple threads to share the same underlying heap,
    /// each with its own thread-local context.
    ///
    /// # Example
    ///
    /// ```
    /// use abfall::GcContext;
    /// use std::sync::Arc;
    /// use std::thread;
    ///
    /// let ctx = GcContext::new();
    /// let heap = Arc::clone(ctx.heap());
    ///
    /// let handle = thread::spawn(move || {
    ///     let ctx2 = GcContext::with_heap(heap);
    ///     let value = ctx2.allocate(42);
    ///     *value
    /// });
    ///
    /// let result = handle.join().unwrap();
    /// ```
    pub fn with_heap(heap: Arc<Heap>) -> Self {
        set_current_heap(Some(Arc::clone(&heap)));
        Self {
            heap,
            _marker: std::marker::PhantomData,
        }
    }

    /// Allocate an object on the GC heap
    ///
    /// Returns a `GcRoot` that keeps the object alive. The object is allocated
    /// in rooted state (root_count = 1). When all roots are dropped, it becomes
    /// eligible for collection.
    ///
    /// # Example
    ///
    /// ```
    /// use abfall::GcContext;
    ///
    /// let ctx = GcContext::new();
    /// let number = ctx.allocate(42);
    /// let text = ctx.allocate("Hello");
    /// assert_eq!(*number, 42);
    /// ```
    pub fn allocate<T: Trace>(&self, data: T) -> crate::GcRoot<T> {
        self.heap.allocate(data)
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

    /// Get the number of currently allocated objects
    pub fn allocation_count(&self) -> usize {
        self.heap.allocation_count()
    }

    /// Get the number of bytes currently allocated
    pub fn bytes_allocated(&self) -> usize {
        self.heap.bytes_allocated()
    }

    /// Check if a collection should be triggered
    pub fn should_collect(&self) -> bool {
        self.heap.should_collect()
    }

    /// Begin the mark phase of garbage collection
    pub fn begin_mark(&self) {
        self.heap.begin_mark()
    }

    /// Perform a bounded amount of marking work
    /// Returns true if marking is complete
    pub fn do_mark_work(&self, work_budget: usize) -> bool {
        self.heap.do_mark_work(work_budget)
    }

    /// Check if currently in marking phase
    pub fn is_marking(&self) -> bool {
        self.heap.is_marking()
    }

    /// Perform the sweep phase of garbage collection
    pub fn sweep(&self) {
        self.heap.sweep()
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
    }
}
