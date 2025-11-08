//! Smart pointer type for GC-managed objects
//!
//! `GcPtr<T>` is a smart pointer that maintains a reference to an object
//! on the GC heap. As long as at least one `GcPtr` exists for an object,
//! it is considered a root and will not be collected.

use crate::heap::{GcBox, Heap};
use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::Arc;

/// Smart pointer to a GC-managed object
///
/// `GcPtr<T>` provides access to an object on the garbage collected heap.
/// The object remains alive as long as at least one `GcPtr` points to it.
///
/// Implements `Deref` for transparent access to the underlying value.
pub struct GcPtr<T> {
    ptr: NonNull<GcBox<T>>,
    heap: Arc<Heap>,
}

impl<T> GcPtr<T> {
    pub(crate) fn new(ptr: NonNull<GcBox<T>>, heap: Arc<Heap>) -> Self {
        unsafe {
            ptr.as_ref().set_root(true);
        }
        Self { ptr, heap }
    }

    /// Get a raw pointer to the managed object
    ///
    /// # Safety
    ///
    /// The returned pointer is only valid as long as this `GcPtr`
    /// or another `GcPtr` to the same object exists.
    pub fn as_ptr(&self) -> *const T {
        unsafe { self.ptr.as_ref().data() as *const T }
    }
}

impl<T> Deref for GcPtr<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.ptr.as_ref().data() }
    }
}

impl<T> Clone for GcPtr<T> {
    fn clone(&self) -> Self {
        unsafe {
            self.ptr.as_ref().set_root(true);
        }
        Self {
            ptr: self.ptr,
            heap: Arc::clone(&self.heap),
        }
    }
}

impl<T> Drop for GcPtr<T> {
    fn drop(&mut self) {
        // When a GcPtr is dropped, we don't immediately free it
        // Instead, we mark it as no longer a root
        // It will be collected during the next GC cycle if unreachable
        unsafe {
            self.ptr.as_ref().set_root(false);
        }
    }
}

unsafe impl<T: Send> Send for GcPtr<T> {}
unsafe impl<T: Sync> Sync for GcPtr<T> {}
