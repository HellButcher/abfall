//! Garbage collector context and main API
//!
//! This module provides thread-local GC context management via `GcContext`.
//! Each thread has its own heap, accessed through a RAII guard.

use crate::Tracer;
use crate::heap::{GcOptions, Heap};
use crate::trace::Trace;
use std::cell::Cell;
use std::ops::Deref;
use std::pin::Pin;
use std::ptr;
use std::sync::Arc;

thread_local! {
    static CURRENT_CTX: Cell<*const GcContextInner> = const { Cell::new(ptr::null()) };
}

/// Set the current thread-local heap
fn set_current_context(ctx: &Pin<Box<GcContextInner>>) {
    let target_ptr: *const GcContextInner = ctx.as_ref().get_ref();
    CURRENT_CTX.with(|tls| {
        if !tls.get().is_null() {
            panic!("A GcContext is already set for this thread");
        }
        tls.set(target_ptr);
    });
}

fn reset_current_context(ctx: &Pin<Box<GcContextInner>>) {
    let target_ptr: *const GcContextInner = ctx.as_ref().get_ref();
    CURRENT_CTX.with(|tls| {
        if tls.get().addr() == target_ptr.addr() {
            tls.set(ptr::null());
        }
    });
}

pub(crate) fn with_current_tracer(f: impl FnOnce(&Tracer)) -> bool {
    CURRENT_CTX.with(|tls| {
        let ctx_ptr = tls.get();
        if ctx_ptr.is_null() {
            false
        } else {
            // SAFETY: ctx_ptr is valid as long as the GcContext is alive
            let ctx = unsafe { &*ctx_ptr };
            f(&ctx.local_gray);
            true
        }
    })
}

pub(crate) struct GcContextInner {
    heap: Arc<Heap>,
    local_gray: Tracer,
    shared: GcContextHeapShared,
    _marker: std::marker::PhantomData<*const ()>, // Makes GcContext !Send + !Sync
}

pub(crate) struct GcContextHeapShared {
    // TODO: fields that are shared with the Heap
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
pub struct GcContext(Pin<Box<GcContextInner>>);

impl Default for GcContext {
    fn default() -> Self {
        Self::new()
    }
}

impl GcContext {
    /// Create a new GC context and a new Heap for the current thread
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
        Self::with_heap(heap)
    }

    pub fn off() -> Self {
        let heap = Heap::off();
        Self::with_heap(heap)
    }

    /// Create a new GC context and a new Heap with custom options
    pub fn with_options(options: GcOptions) -> Self {
        let heap = Heap::with_options(options);
        Self::with_heap(heap)
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
        let inner = Box::pin(GcContextInner {
            heap,
            local_gray: Tracer::new(),
            shared: GcContextHeapShared {
                // Initialize shared fields
            },
            _marker: std::marker::PhantomData,
        });
        let inner_ref: &GcContextInner = inner.as_ref().get_ref();
        inner_ref.shared.register_with_heap(inner.heap.as_ref());
        set_current_context(&inner);
        GcContext(inner)
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
        self.0.heap.allocate(data)
    }

    /// Get reference to the underlying heap (for advanced use)
    pub fn heap(&self) -> &Arc<Heap> {
        &self.0.heap
    }
}

impl Drop for GcContext {
    fn drop(&mut self) {
        // Clear thread-local heap when context is dropped
        reset_current_context(&self.0);
        self.0.shared.unregister_from_heap(self.0.heap.as_ref());
    }
}

impl Deref for GcContext {
    type Target = Arc<Heap>;

    #[inline]
    fn deref(&self) -> &Arc<Heap> {
        &self.0.heap
    }
}
