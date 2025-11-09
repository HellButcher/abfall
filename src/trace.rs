//! Trace trait for garbage collection
//!
//! This module provides the `Trace` trait that types must implement to participate
//! in garbage collection. The trait allows the GC to traverse object graphs and
//! mark reachable objects.

use crate::gc_box::GcHeader;
use std::{
    cell::UnsafeCell,
    collections::{BTreeSet, HashSet, VecDeque},
    convert::Infallible,
};

/// A tracer for marking reachable objects
///
/// Used during the mark phase to traverse the object graph.
/// Each thread can have its own tracer that accumulates gray objects,
/// which are then merged back to the shared gray queue.
pub struct Tracer(UnsafeCell<Vec<*const GcHeader>>);

impl Tracer {
    /// Create a new tracer without heap reference (for internal GC use)
    pub(crate) fn new() -> Self {
        Self(UnsafeCell::new(Vec::new()))
    }
    /// Append this tracer's accumulated work to a destination
    pub(crate) fn append_to(&self, dest: &mut Vec<*const GcHeader>) {
        dest.append(unsafe { &mut *self.0.get() });
    }

    /// Steal work from a list of gray objects
    pub(crate) fn steal_from(&self, mut num_items: usize, src: &mut Vec<*const GcHeader>) -> bool {
        if src.is_empty() || num_items == 0 {
            return false;
        }
        // move num_items from src to self
        while num_items > 0 {
            if let Some(item) = src.pop() {
                unsafe { &mut *self.0.get() }.push(item);
                num_items -= 1;
            } else {
                break;
            }
        }
        true
    }

    /// Pop a gray object from local work queue
    pub(crate) fn pop_work(&self) -> Option<*const GcHeader> {
        unsafe { &mut *self.0.get() }.pop()
    }

    pub(crate) fn has_work(&self) -> bool {
        !unsafe { &*self.0.get() }.is_empty()
    }

    /// Mark an object as reachable
    ///
    /// Adds the object to the gray queue for processing if it's currently white
    pub fn mark<T: Trace>(&self, ptr: &crate::GcPtr<T>) {
        let header_ptr = ptr.header_ptr();
        unsafe {
            let header = &*header_ptr;
            if T::NO_TRACE {
                // Immediately mark black if no tracing is needed
                header.color.mark_black();
            } else {
                self.mark_header(header);
            }
        }
    }

    pub(crate) fn mark_header(&self, header: &GcHeader) {
        if header.color.mark_white_to_gray() {
            // Enqueue for scanning
            unsafe { &mut *self.0.get() }.push(header);
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
/// use abfall::{Trace, Tracer, GcPtr, GcRoot};
///
/// struct Node {
///     value: i32,
///     next: Option<GcPtr<Node>>,
/// }
///
/// unsafe impl Trace for Node {
///     fn trace(&self, tracer: &Tracer) {
///         if let Some(ref next) = self.next {
///             tracer.mark(next);
///         }
///     }
/// }
/// ```
pub unsafe trait Trace {
    const NO_TRACE: bool = false;

    /// Trace all GC pointers in this object
    fn trace(&self, tracer: &Tracer);
}

macro_rules! impl_no_trace {
    ($(impl$([$($tt:tt)*])? for $ty:ty);* $(;)?) => {
        $(
            unsafe impl$(<$($tt)*>)? Trace for $ty {
                const NO_TRACE: bool = true;
                fn trace(&self, _tracer: &Tracer) {
                    // Nothing to trace
                }
            }
        )*
    };
}

impl_no_trace! {
    impl for ();
    impl for i8;
    impl for i16;
    impl for i32;
    impl for i64;
    impl for i128;
    impl for isize;
    impl for u8;
    impl for u16;
    impl for u32;
    impl for u64;
    impl for u128;
    impl for usize;
    impl for f32;
    impl for f64;
    impl for bool;
    impl for char;
    impl for String;
    impl for &str;
    impl for Infallible;
    impl[T] for std::marker::PhantomData<T>;
}

macro_rules! impl_trace_deref {
    ($(impl<$i:ident> for $ty:ty);* $(;)?) => {
        $(
            unsafe impl<$i: Trace> Trace for $ty {
                const NO_TRACE: bool = $i::NO_TRACE;
                fn trace(&self, tracer: &Tracer) {
                    $i::trace(self, tracer);
                }
            }
        )*
    };
}

impl_trace_deref! {
    impl<T> for Box<T>;
    impl<T> for std::rc::Rc<T>;
    impl<T> for std::sync::Arc<T>;
}

macro_rules! impl_trace_iterable {
    ($(impl<$i:ident> for $ty:ty);* $(;)?) => {
        $(
            unsafe impl<$i: Trace> Trace for $ty {
                const NO_TRACE: bool = $i::NO_TRACE;
                fn trace(&self, tracer: &Tracer) {
                    for item in self {
                        item.trace(tracer);
                    }
                }
            }
        )*
    };
}

impl_trace_iterable! {
    impl<T> for Vec<T>;
    impl<T> for VecDeque<T>;
    impl<T> for HashSet<T>;
    impl<T> for BTreeSet<T>;
}

macro_rules! impl_trace_map {
    ($(impl<$i:ident, $j:ident> for $ty:ty);* $(;)?) => {
        $(
            unsafe impl<$i: Trace,$j: Trace> Trace for $ty {
                const NO_TRACE: bool = $i::NO_TRACE && $j::NO_TRACE;
                fn trace(&self, tracer: &Tracer) {
                    for (k,v) in self.iter() {
                        k.trace(tracer);
                        v.trace(tracer);
                    }
                }
            }
        )*
    };
}

impl_trace_map! {
    impl<K,V> for std::collections::HashMap<K,V>;
    impl<K,V> for std::collections::BTreeMap<K,V>;
}

unsafe impl<T: Trace, E: Trace> Trace for Result<T, E> {
    const NO_TRACE: bool = T::NO_TRACE && E::NO_TRACE;
    fn trace(&self, tracer: &Tracer) {
        match self {
            Ok(value) => value.trace(tracer),
            Err(err) => err.trace(tracer),
        }
    }
}

unsafe impl<T: Trace> Trace for Option<T> {
    const NO_TRACE: bool = T::NO_TRACE;
    fn trace(&self, tracer: &Tracer) {
        if let Some(value) = self {
            value.trace(tracer);
        }
    }
}
unsafe impl<T: Trace, const N: usize> Trace for [T; N] {
    const NO_TRACE: bool = T::NO_TRACE;
    fn trace(&self, tracer: &Tracer) {
        for item in self {
            item.trace(tracer);
        }
    }
}
