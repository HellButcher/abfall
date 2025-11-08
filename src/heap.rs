//! Heap management and object storage
//!
//! This module provides the heap structure that stores GC-managed objects
//! and implements the mark and sweep phases of garbage collection.

use crate::color::{AtomicColor, Color};
use crate::trace::{Trace, Tracer};
use std::alloc::Layout;
use std::ptr::{null_mut, NonNull};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::Arc;

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

/// The garbage collected heap
///
/// Manages allocation and deallocation of GC objects using an intrusive
/// linked list, and implements the mark and sweep collection algorithm.
pub struct Heap {
    /// Head of the intrusive linked list of allocations
    head: AtomicPtr<GcHeader>,
    /// Total bytes currently allocated
    bytes_allocated: AtomicUsize,
    /// Collection threshold in bytes
    threshold: AtomicUsize,
}

impl Heap {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            head: AtomicPtr::new(null_mut()),
            bytes_allocated: AtomicUsize::new(0),
            threshold: AtomicUsize::new(1024 * 1024), // 1MB initial threshold
        })
    }

    pub fn allocate<T: Trace>(&self, data: T) -> NonNull<GcBox<T>> {
        let ptr = GcBox::new(data);
        let size = unsafe { (*ptr.as_ptr()).header.vtable.layout.size() };
        
        // Insert at head of linked list atomically
        let header_ptr = unsafe { &(*ptr.as_ptr()).header as *const GcHeader as *mut GcHeader };
        
        loop {
            let current_head = self.head.load(Ordering::Acquire);
            unsafe {
                (*header_ptr).next.store(current_head, Ordering::Relaxed);
            }
            
            if self.head.compare_exchange(
                current_head,
                header_ptr,
                Ordering::Release,
                Ordering::Acquire,
            ).is_ok() {
                break;
            }
        }
        
        self.bytes_allocated.fetch_add(size, Ordering::Relaxed);
        ptr
    }

    pub fn should_collect(&self) -> bool {
        self.bytes_allocated.load(Ordering::Relaxed) 
            >= self.threshold.load(Ordering::Relaxed)
    }

    pub fn mark_from_roots(&self) {
        let mut gray_queue: Vec<*const GcHeader> = Vec::new();

        // Walk the linked list to find roots
        let mut current = self.head.load(Ordering::Acquire);
        while !current.is_null() {
            unsafe {
                let header = &*current;
                if header.is_root() {
                    // Try to transition White -> Gray
                    if header.color.compare_exchange(
                        Color::White,
                        Color::Gray,
                        Ordering::AcqRel,
                        Ordering::Acquire
                    ).is_ok() {
                        gray_queue.push(current);
                    }
                }
                current = header.next.load(Ordering::Acquire);
            }
        }

        // Process gray queue
        while let Some(ptr) = gray_queue.pop() {
            unsafe {
                let header = &*ptr;
                
                // Create a tracer and call the trace function from vtable
                let mut tracer = Tracer::new();
                (header.vtable.trace)(ptr, &mut tracer);
                
                // Merge tracer's gray queue
                gray_queue.extend(tracer.gray_queue_mut().drain(..));
                
                // Mark as black
                header.color.store(Color::Black, Ordering::Release);
            }
        }
    }

    pub fn sweep(&self) {
        let mut freed = 0;
        let mut prev: *mut *mut GcHeader = &self.head as *const AtomicPtr<GcHeader> as *mut *mut GcHeader;
        
        unsafe {
            let mut current = self.head.load(Ordering::Acquire);
            
            while !current.is_null() {
                let header = &*current;
                let next = header.next.load(Ordering::Acquire);
                
                // Check if object should be collected (White and not a root)
                if header.color.load(Ordering::Acquire) == Color::White && !header.is_root() {
                    // Remove from list
                    if prev == &self.head as *const AtomicPtr<GcHeader> as *mut *mut GcHeader {
                        self.head.store(next, Ordering::Release);
                    } else {
                        (*(*prev)).next.store(next, Ordering::Release);
                    }
                    
                    // Get size from vtable and call drop function
                    let size = header.vtable.layout.size();
                    (header.vtable.drop)(current);  // Proper Drop via Box::from_raw!
                    freed += size;
                    
                    current = next;
                } else {
                    // Reset color for next cycle
                    header.color.store(Color::White, Ordering::Release);
                    prev = &header.next as *const AtomicPtr<GcHeader> as *mut *mut GcHeader;
                    current = next;
                }
            }
        }

        self.bytes_allocated.fetch_sub(freed, Ordering::Relaxed);
    }

    pub fn bytes_allocated(&self) -> usize {
        self.bytes_allocated.load(Ordering::Relaxed)
    }

    pub fn allocation_count(&self) -> usize {
        let mut count = 0;
        let mut current = self.head.load(Ordering::Acquire);
        
        while !current.is_null() {
            count += 1;
            unsafe {
                current = (*current).next.load(Ordering::Acquire);
            }
        }
        
        count
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        let mut current = self.head.load(Ordering::Acquire);
        
        while !current.is_null() {
            unsafe {
                let header = &*current;
                let next = header.next.load(Ordering::Acquire);
                
                // Use vtable drop for proper Drop semantics
                (header.vtable.drop)(current);
                
                current = next;
            }
        }
    }
}
