//! Smart pointer types for GC-managed objects
//!
//! `GcPtr<T>` is a lightweight pointer (pointer-sized, Copy) to a GC object.
//! It does not implement Deref and does not manage root counts.
//!
//! `GcRoot<T>` is a rooted pointer that manages root counts and implements Deref
//! for access to the underlying value. Objects remain alive as long as at least
//! one `GcRoot` exists pointing to them.

use crate::gc_box::{GcBox, GcHeader};
use std::ops::Deref;
use std::ptr::NonNull;

/// Lightweight pointer to a GC-managed object
///
/// `GcPtr<T>` is a simple Copy pointer that does not manage root counts.
/// It does NOT implement Deref. To access the underlying value, you must
/// convert it to a `GcRoot` first (which increments the root count).
///
/// **Size**: GcPtr is pointer-sized (8 bytes on 64-bit).
/// 
/// Use `GcPtr` in data structures to reference other GC objects without
/// creating circular root references. Use `GcRoot` to keep objects alive.
#[repr(transparent)]
pub struct GcPtr<T: ?Sized>(NonNull<GcBox<T>>);

impl<T: ?Sized> GcPtr<T> {
    /// Create a GcPtr from a raw pointer (for internal use or future API)
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn new(ptr: NonNull<GcBox<T>>) -> Self {
        Self(ptr)
    }

    /// Convert this pointer to a rooted pointer
    ///
    /// Increments the root count, ensuring the object stays alive
    /// as long as the returned `GcRoot` exists.
    ///
    /// # Safety
    ///
    /// The pointer must be valid and point to a live GC object.
    #[inline]
    pub unsafe fn root(self) -> GcRoot<T> {
        unsafe {
            self.0.as_ref().header.inc_root();
            GcRoot(self)
        }
    }

    /// Get a raw pointer to the managed object
    ///
    /// # Safety
    ///
    /// The returned pointer is only valid as long as the object is reachable
    /// from some root.
    #[inline]
    pub fn as_ptr(&self) -> *const T {
        unsafe { &self.0.as_ref().data as *const T }
    }
    
    /// Get the header pointer for this object (internal use)
    #[inline]
    pub(crate) fn header_ptr(&self) -> *const GcHeader {
        unsafe { &self.0.as_ref().header as *const GcHeader }
    }
}

impl<T: ?Sized> Copy for GcPtr<T> {}
impl<T: ?Sized> Clone for GcPtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

unsafe impl<T: Send> Send for GcPtr<T> {}
unsafe impl<T: Sync> Sync for GcPtr<T> {}

/// Rooted pointer to a GC-managed object
///
/// `GcRoot<T>` is a rooted reference that keeps the object alive.
/// It implements `Deref` for transparent access to the underlying value.
/// Following Rc/Arc semantics, only provides shared references.
///
/// Objects are allocated in rooted state and `allocate()` returns `GcRoot`.
/// Clone a `GcRoot` to create another root. Drop all roots to make the object
/// eligible for collection.
#[repr(transparent)]
pub struct GcRoot<T: ?Sized>(GcPtr<T>);

impl<T: ?Sized> GcRoot<T> {
    /// Create a new GcRoot from a NonNull pointer
    ///
    /// Used internally during allocation. The object must already be allocated
    /// with root_count initialized to 1.
    /// 
    /// # Safety
    /// ptr must already have its root count initialized to 1.
    #[inline]
    pub(crate) unsafe fn new_from_nonnull(ptr: NonNull<GcBox<T>>) -> Self {
        Self(GcPtr(ptr))
    }

    /// Get the underlying GcPtr
    ///
    /// Use this to store non-rooting references in data structures.
    #[inline]
    pub fn as_ptr(&self) -> GcPtr<T> {
        self.0
    }
}

impl<T: ?Sized> Deref for GcRoot<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe { &self.0.0.as_ref().data }
    }
}

impl<T: ?Sized> Clone for GcRoot<T> {
    #[inline]
    fn clone(&self) -> Self {
        unsafe {
            self.0.0.as_ref().header.inc_root();
        }
        Self(self.0)
    }
}

impl<T: ?Sized> Drop for GcRoot<T> {
    fn drop(&mut self) {
        unsafe {
            self.0.0.as_ref().header.dec_root();
        }
    }
}

unsafe impl<T: Send> Send for GcRoot<T> {}
unsafe impl<T: Sync> Sync for GcRoot<T> {}

// GcPtr implements Trace - it marks itself as reachable
unsafe impl<T> crate::trace::Trace for GcPtr<T> {
    fn trace(&self, tracer: &mut crate::trace::Tracer) {
        tracer.mark(self);
    }
}
