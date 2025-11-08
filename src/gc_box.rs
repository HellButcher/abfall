//! GC object layout and metadata
//!
//! This module defines the internal structure of garbage-collected objects,
//! including the header, vtable, and container.

use crate::color::{AtomicColor, Color};
use crate::trace::{Trace, Tracer};
use std::alloc::Layout;
use std::ptr::{null_mut, NonNull};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Type-erased virtual table for GC operations
///
/// This vtable contains all type-specific operations needed for GC,
/// stored statically to avoid per-object overhead.
pub struct GcVTable {
    /// Trace function for marking reachable objects
    pub trace: unsafe fn(*const GcHeader, &mut Tracer),
    
    /// Drop function - properly drops the object using Box::from_raw
    pub drop: unsafe fn(*mut GcHeader),
    
    /// Layout of the complete GcBox<T>
    pub layout: Layout,
}

impl GcVTable {
    /// Create a new vtable for type T
    pub fn new<T: Trace>() -> Self {
        unsafe fn trace_impl<T: Trace>(ptr: *const GcHeader, tracer: &mut Tracer) {
            unsafe {
                // Calculate GcBox pointer from header pointer using offset
                // SAFETY: GcBox is repr(C) so header is at offset 0
                let gc_box_ptr = (ptr as *const u8).sub(
                    std::mem::offset_of!(GcBox<T>, header)
                ) as *const GcBox<T>;
                
                let data = &(*gc_box_ptr).data;
                data.trace(tracer);
            }
        }
        
        unsafe fn drop_impl<T>(ptr: *mut GcHeader) {
            unsafe {
                // Calculate GcBox pointer from header pointer using offset
                // SAFETY: GcBox is repr(C) so header is at offset 0
                let gc_box_ptr = (ptr as *mut u8).sub(
                    std::mem::offset_of!(GcBox<T>, header)
                ) as *mut GcBox<T>;
                
                let _box = Box::from_raw(gc_box_ptr);
                // Box drops T here
            }
        }
        
        Self {
            trace: trace_impl::<T>,
            drop: drop_impl::<T>,
            layout: Layout::new::<GcBox<T>>(),
        }
    }
}

/// Type-erased header for all GC objects
///
/// This header is shared by all `GcBox<T>` instances and allows
/// uniform handling of objects in the allocation list.
pub struct GcHeader {
    /// Current color in the tri-color marking algorithm
    pub color: AtomicColor,
    /// Reference count for root pointers (0 = not a root)
    pub root_count: AtomicUsize,
    /// Next pointer in the intrusive linked list
    pub next: AtomicPtr<GcHeader>,
    /// Static vtable reference for type-erased operations
    pub vtable: &'static GcVTable,
}

impl GcHeader {
    pub fn new<T: Trace>() -> Self {
        // Leak a vtable for this type (in production, we'd cache these)
        let vtable = Box::leak(Box::new(GcVTable::new::<T>()));
        
        Self {
            color: AtomicColor::new(Color::White),
            root_count: AtomicUsize::new(1),  // Start at 1 - already rooted! (allocation safety)
            next: AtomicPtr::new(null_mut()),
            vtable,
        }
    }

    pub fn inc_root(&self) {
        self.root_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dec_root(&self) {
        self.root_count.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn is_root(&self) -> bool {
        self.root_count.load(Ordering::Relaxed) > 0
    }
}

/// A garbage collected object with metadata
///
/// `GcBox` wraps a value with GC metadata including color and root status.
/// 
/// SAFETY: repr(C) ensures that `header` is always at offset 0, making it
/// safe to cast between `*GcHeader` and `*GcBox<T>`.
#[repr(C)]
pub struct GcBox<T: ?Sized> {
    pub header: GcHeader,
    pub data: T,
}

impl<T: Trace> GcBox<T> {
    /// Allocate a new GcBox using Box (idiomatic Rust!)
    pub fn new(data: T) -> NonNull<GcBox<T>> {
        // Compile-time assertion: header must be at offset 0 due to repr(C)
        const _: () = assert!(std::mem::offset_of!(GcBox<()>, header) == 0);
        
        let gc_box = Box::new(GcBox {
            header: GcHeader::new::<T>(),
            data,
        });
        
        // Leak the box to get a raw pointer
        NonNull::from(Box::leak(gc_box))
    }
}
