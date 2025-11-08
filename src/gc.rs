//! Garbage collector context and main API
//!
//! This module provides the `GcContext` which manages the garbage collected heap
//! and provides allocation and collection capabilities.

use crate::heap::Heap;
use crate::ptr::GcPtr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// The main garbage collector context
///
/// `GcContext` manages a heap of garbage collected objects and provides
/// both automatic and manual memory management capabilities.
///
/// # Thread Safety
///
/// `GcContext` can be safely shared across threads using `Arc<GcContext>`.
/// Multiple threads can allocate objects concurrently.
///
/// # Collection Strategy
///
/// - Automatic: Background thread performs collection when heap exceeds threshold
/// - Manual: Caller explicitly triggers collection via `collect()`
pub struct GcContext {
    heap: Arc<Heap>,
    collecting: Arc<AtomicBool>,
    background_thread: Option<thread::JoinHandle<()>>,
}

impl GcContext {
    /// Create a new GC context with automatic background collection
    ///
    /// The background thread wakes every 100ms to check if collection is needed.
    ///
    /// # Example
    ///
    /// ```
    /// use abfall::GcContext;
    ///
    /// let ctx = GcContext::new();
    /// let value = ctx.allocate(42);
    /// ```
    pub fn new() -> Arc<Self> {
        Self::with_options(true, Duration::from_millis(100))
    }

    /// Create a GC context with custom options
    ///
    /// # Arguments
    ///
    /// * `background_collection` - If true, enables automatic background collection
    /// * `interval` - How often the background thread checks for collection
    ///
    /// # Example
    ///
    /// ```
    /// use abfall::GcContext;
    /// use std::time::Duration;
    ///
    /// // Create context without background collection
    /// let ctx = GcContext::with_options(false, Duration::from_millis(100));
    /// ```
    pub fn with_options(background_collection: bool, interval: Duration) -> Arc<Self> {
        let heap = Heap::new();
        let collecting = Arc::new(AtomicBool::new(false));

        let background_thread = if background_collection {
            let heap_clone = Arc::clone(&heap);
            let collecting_clone = Arc::clone(&collecting);
            
            Some(thread::spawn(move || {
                loop {
                    thread::sleep(interval);
                    
                    if heap_clone.should_collect() {
                        if let Ok(_) = collecting_clone.compare_exchange(
                            false,
                            true,
                            Ordering::Acquire,
                            Ordering::Relaxed,
                        ) {
                            heap_clone.mark_from_roots();
                            heap_clone.sweep();
                            collecting_clone.store(false, Ordering::Release);
                        }
                    }
                }
            }))
        } else {
            None
        };

        Arc::new(Self {
            heap,
            collecting,
            background_thread,
        })
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
    pub fn allocate<T>(self: &Arc<Self>, data: T) -> GcPtr<T> {
        let ptr = self.heap.allocate(data);
        GcPtr::new(ptr, Arc::clone(&self.heap))
    }

    /// Manually trigger a garbage collection cycle
    ///
    /// This performs a full mark-and-sweep collection. If a collection
    /// is already in progress, this call waits for it to complete.
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
        while self.collecting.compare_exchange(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed,
        ).is_err() {
            thread::yield_now();
        }

        self.heap.mark_from_roots();
        self.heap.sweep();
        
        self.collecting.store(false, Ordering::Release);
    }

    /// Force an immediate collection
    ///
    /// Alias for `collect()`.
    pub fn force_collect(&self) {
        self.collect();
    }

    /// Get the current number of bytes allocated on the heap
    pub fn bytes_allocated(&self) -> usize {
        self.heap.bytes_allocated()
    }

    /// Get the current number of live allocations
    pub fn allocation_count(&self) -> usize {
        self.heap.allocation_count()
    }

    /// Check if a collection is currently in progress
    pub fn is_collecting(&self) -> bool {
        self.collecting.load(Ordering::Relaxed)
    }
}

impl Drop for GcContext {
    fn drop(&mut self) {
        if let Some(handle) = self.background_thread.take() {
            // In a real implementation, we'd need a way to signal the thread to stop
            // For now, we just detach it
            drop(handle);
        }
    }
}

/// Helper struct for creating GC contexts
pub struct Gc;

impl Gc {
    /// Create a new GC context with default settings
    pub fn new_context() -> Arc<GcContext> {
        GcContext::new()
    }

    /// Create a new GC context with custom options
    pub fn new_context_with_options(background_collection: bool, interval: Duration) -> Arc<GcContext> {
        GcContext::with_options(background_collection, interval)
    }
}


