//! Heap management and object storage
//!
//! This module provides the heap structure that stores GC-managed objects
//! and implements the mark and sweep phases of garbage collection.

use crate::color::{AtomicColor, Color};
use std::alloc::{alloc, dealloc, Layout};
use std::ptr::{null_mut, NonNull};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::Arc;

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
    /// Type-erased trace function
    pub trace_fn: unsafe fn(*const GcHeader, &mut Vec<*const GcHeader>),
}

impl GcHeader {
    pub fn new<T>(trace_fn: unsafe fn(*const GcHeader, &mut Vec<*const GcHeader>)) -> Self {
        Self {
            color: AtomicColor::new(Color::White),
            root_count: AtomicUsize::new(1),  // Start at 1 - already rooted! (allocation safety)
            next: AtomicPtr::new(null_mut()),
            trace_fn,
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
pub struct GcBox<T: ?Sized> {
    pub header: GcHeader,
    pub data: T,
}

impl<T> GcBox<T> {
    pub fn new(data: T, trace_fn: unsafe fn(*const GcHeader, &mut Vec<*const GcHeader>)) -> NonNull<GcBox<T>> {
        // TODO: use Box::into_raw for allocation
        let layout = Layout::new::<GcBox<T>>();
        unsafe {
            let ptr = alloc(layout) as *mut GcBox<T>;
            if ptr.is_null() {
                panic!("Allocation failed");
            }
            ptr.write(GcBox {
                header: GcHeader::new::<T>(trace_fn),
                data,
            });
            NonNull::new_unchecked(ptr)
        }
    }

    pub fn data(&self) -> &T {
        &self.data
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

    pub fn allocate<T>(&self, data: T, trace_fn: unsafe fn(*const GcHeader, &mut Vec<*const GcHeader>)) -> NonNull<GcBox<T>> {
        let ptr = GcBox::new(data, trace_fn);
        let size = std::mem::size_of::<GcBox<T>>();
        
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
                
                // Call the trace function to find children
                (header.trace_fn)(ptr, &mut gray_queue);
                
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
                    
                    // Calculate layout and free
                    let layout = Layout::for_value(&*current);
                    freed += layout.size();
                    dealloc(current as *mut u8, layout);
                    
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
                let layout = Layout::for_value(&*current);
                dealloc(current as *mut u8, layout);
                current = next;
            }
        }
    }
}
