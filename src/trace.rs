//! Trace trait for garbage collection
//!
//! This module provides the `Trace` trait that types must implement to participate
//! in garbage collection. The trait allows the GC to traverse object graphs and
//! mark reachable objects.

use crate::heap::{GcHeader, Heap};
use std::sync::{Arc, Weak};

/// A tracer for marking reachable objects
///
/// Used during the mark phase to traverse the object graph
pub struct Tracer {
    #[allow(dead_code)]
    heap: Option<Weak<Heap>>,
    gray_queue: Vec<*const GcHeader>,
}

impl Tracer {
    /// Create a new tracer without heap reference (for internal GC use)
    pub(crate) fn new() -> Self {
        Self {
            heap: None,
            gray_queue: Vec::new(),
        }
    }
    
    /// Create a new tracer with heap reference (for write barriers)
    #[allow(dead_code)]
    pub(crate) fn with_heap(heap: Arc<Heap>) -> Self {
        Self {
            heap: Some(Arc::downgrade(&heap)),
            gray_queue: Vec::new(),
        }
    }

    pub(crate) fn gray_queue_mut(&mut self) -> &mut Vec<*const GcHeader> {
        &mut self.gray_queue
    }
    
    #[allow(dead_code)]
    pub(crate) fn heap(&self) -> Option<Arc<Heap>> {
        self.heap.as_ref().and_then(|w| w.upgrade())
    }

    /// Mark an object as reachable
    ///
    /// Adds the object to the gray queue for processing if it's currently white
    pub fn mark<T>(&mut self, ptr: &crate::GcPtr<T>) {
        use crate::color::Color;
        
        let header_ptr = ptr.header_ptr();
        unsafe {
            let header = &*header_ptr;
            // Try to transition White -> Gray
            if header.color.compare_exchange(
                Color::White,
                Color::Gray,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire
            ).is_ok() {
                self.gray_queue.push(header_ptr);
            }
        }
    }
}

/// Trait for types that can be traced by the garbage collector
///
/// # Safety
///
/// Implementations must call `tracer.mark()` on all `GcPtr` fields.
/// Failing to trace all GC pointers will result in premature collection
/// and use-after-free bugs.
///
/// # Example
///
/// ```
/// use abfall::{Trace, Tracer, GcPtr};
///
/// struct Node {
///     value: i32,
///     next: Option<GcPtr<Node>>,
/// }
///
/// unsafe impl Trace for Node {
///     fn trace(&self, tracer: &mut Tracer) {
///         if let Some(ref next) = self.next {
///             tracer.mark(next);
///         }
///     }
/// }
/// ```
pub unsafe trait Trace {
    /// Trace all GC pointers in this object
    fn trace(&self, tracer: &mut Tracer);
}

/// Marker trait for types that contain no GC pointers
///
/// Types implementing `NoTrace` have a default no-op trace implementation.
///
/// # Safety
///
/// Only implement this for types that contain no `GcPtr` fields.
pub unsafe trait NoTrace: Trace {}

// Blanket implementation for NoTrace types
unsafe impl<T: NoTrace> Trace for T {
    #[inline]
    fn trace(&self, _tracer: &mut Tracer) {
        // Nothing to trace
    }
}

// Implement NoTrace for common primitive types
unsafe impl NoTrace for i8 {}
unsafe impl NoTrace for i16 {}
unsafe impl NoTrace for i32 {}
unsafe impl NoTrace for i64 {}
unsafe impl NoTrace for i128 {}
unsafe impl NoTrace for isize {}

unsafe impl NoTrace for u8 {}
unsafe impl NoTrace for u16 {}
unsafe impl NoTrace for u32 {}
unsafe impl NoTrace for u64 {}
unsafe impl NoTrace for u128 {}
unsafe impl NoTrace for usize {}

unsafe impl NoTrace for f32 {}
unsafe impl NoTrace for f64 {}

unsafe impl NoTrace for bool {}
unsafe impl NoTrace for char {}

unsafe impl NoTrace for String {}
unsafe impl NoTrace for &str {}

// Vec is NoTrace if its element type is NoTrace
unsafe impl<T: NoTrace> NoTrace for Vec<T> {}

// Option<T> implements Trace for any T: Trace
unsafe impl<T: Trace> Trace for Option<T> {
    fn trace(&self, tracer: &mut Tracer) {
        if let Some(value) = self {
            value.trace(tracer);
        }
    }
}

// Result is NoTrace if both types are NoTrace  
unsafe impl<T: NoTrace, E: NoTrace> NoTrace for Result<T, E> {}

// Tuples are NoTrace if all elements are NoTrace
unsafe impl<T: NoTrace> NoTrace for (T,) {}
unsafe impl<T1: NoTrace, T2: NoTrace> NoTrace for (T1, T2) {}
unsafe impl<T1: NoTrace, T2: NoTrace, T3: NoTrace> NoTrace for (T1, T2, T3) {}
unsafe impl<T1: NoTrace, T2: NoTrace, T3: NoTrace, T4: NoTrace> NoTrace for (T1, T2, T3, T4) {}

// Arrays are NoTrace if the element type is NoTrace
unsafe impl<T: NoTrace, const N: usize> NoTrace for [T; N] {}
