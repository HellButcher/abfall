//! Heap management and object storage
//!
//! This module provides the heap structure that stores GC-managed objects
//! and implements the mark and sweep phases of garbage collection.

use crate::color::{AtomicColor, Color};
use crate::trace::{Trace, Tracer};
use std::alloc::Layout;
use std::ptr::{null_mut, NonNull};
use std::sync::atomic::{AtomicU8, AtomicPtr, AtomicUsize, Ordering};
use std::sync::Arc;

/// Send-safe wrapper for raw pointer queue
struct GrayQueue(Vec<*const GcHeader>);

unsafe impl Send for GrayQueue {}
unsafe impl Sync for GrayQueue {}

impl GrayQueue {
    fn new() -> Self {
        Self(Vec::new())
    }
    
    fn push(&mut self, ptr: *const GcHeader) {
        self.0.push(ptr);
    }
    
    fn pop(&mut self) -> Option<*const GcHeader> {
        self.0.pop()
    }
    
    fn clear(&mut self) {
        self.0.clear();
    }
    
    fn append(&mut self, other: &mut Vec<*const GcHeader>) {
        self.0.append(other);
    }
}

/// GC Phase for incremental collection
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcPhase {
    Idle = 0,
    Marking = 1,
    Sweeping = 2,
}

impl GcPhase {
    #[allow(dead_code)]
    fn from_u8(value: u8) -> Self {
        match value {
            0 => GcPhase::Idle,
            1 => GcPhase::Marking,
            2 => GcPhase::Sweeping,
            _ => GcPhase::Idle,
        }
    }
}

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
/// linked list, and implements the mark and sweep collection algorithm
/// with incremental marking support.
pub struct Heap {
    /// Head of the intrusive linked list of allocations
    head: AtomicPtr<GcHeader>,
    /// Total bytes currently allocated
    bytes_allocated: AtomicUsize,
    /// Collection threshold in bytes
    threshold: AtomicUsize,
    /// Current GC phase (for incremental collection)
    phase: AtomicU8,
    /// Gray queue for incremental marking
    gray_queue: parking_lot::Mutex<GrayQueue>,
}

impl Heap {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            head: AtomicPtr::new(null_mut()),
            bytes_allocated: AtomicUsize::new(0),
            threshold: AtomicUsize::new(1024 * 1024), // 1MB initial threshold
            phase: AtomicU8::new(GcPhase::Idle as u8),
            gray_queue: parking_lot::Mutex::new(GrayQueue::new()),
        })
    }
    
    /// Get the current GC phase
    #[allow(dead_code)]
    pub fn phase(&self) -> GcPhase {
        GcPhase::from_u8(self.phase.load(Ordering::Acquire))
    }
    
    /// Check if currently in marking phase (for write barriers)
    pub fn is_marking(&self) -> bool {
        self.phase.load(Ordering::Acquire) == GcPhase::Marking as u8
    }
    
    /// Mark an object as gray (write barrier support)
    /// 
    /// Used by write barriers to shade objects that are being written during marking.
    /// If the object is white, transitions it to gray and adds to the gray queue.
    pub fn mark_gray(&self, header: *const GcHeader) {
        if header.is_null() {
            return;
        }
        
        unsafe {
            let h = &*header;
            // Try to transition White -> Gray
            if h.color.compare_exchange(
                Color::White,
                Color::Gray,
                Ordering::AcqRel,
                Ordering::Acquire
            ).is_ok() {
                // Successfully transitioned - add to gray queue
                self.gray_queue.lock().push(header);
            }
            // If already gray or black, nothing to do
        }
    }
    
    /// Check if a collection should be triggered
    pub fn should_collect(&self) -> bool {
        self.bytes_allocated.load(Ordering::Relaxed) >= self.threshold.load(Ordering::Relaxed)
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

    /// Begin incremental marking phase
    pub fn begin_mark(&self) {
        // Transition to Marking phase
        self.phase.store(GcPhase::Marking as u8, Ordering::Release);
        
        // Initialize gray queue with roots
        let mut gray_queue = self.gray_queue.lock();
        gray_queue.clear();
        
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
    }
    
    /// Perform a bounded amount of incremental marking work
    /// 
    /// Returns true if marking is complete, false if more work remains
    pub fn do_mark_work(&self, work_budget: usize) -> bool {
        let mut gray_queue = self.gray_queue.lock();
        let mut work_done = 0;
        
        while work_done < work_budget {
            match gray_queue.pop() {
                Some(ptr) => {
                    unsafe {
                        let header = &*ptr;
                        
                        // Create a tracer and call the trace function from vtable
                        let mut tracer = Tracer::new();
                        (header.vtable.trace)(ptr, &mut tracer);
                        
                        // Merge tracer's gray queue
                        gray_queue.append(tracer.gray_queue_mut());
                        
                        // Mark as black
                        header.color.store(Color::Black, Ordering::Release);
                        
                        work_done += 1;
                    }
                }
                None => {
                    // No more work - marking is complete
                    return true;
                }
            }
        }
        
        // More work remains
        false
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
                gray_queue.append(tracer.gray_queue_mut());
                
                // Mark as black
                header.color.store(Color::Black, Ordering::Release);
            }
        }
    }

    /// Begin incremental sweep phase
    #[allow(dead_code)]
    pub fn begin_sweep(&self) {
        self.phase.store(GcPhase::Sweeping as u8, Ordering::Release);
    }

    pub fn sweep(&self) {
        // Set sweeping phase
        self.phase.store(GcPhase::Sweeping as u8, Ordering::Release);
        
        let mut freed = 0;
        
        unsafe {
            let mut current = self.head.load(Ordering::Acquire);
            let mut prev_next: *const AtomicPtr<GcHeader> = &self.head;
            
            while !current.is_null() {
                let header = &*current;
                let next = header.next.load(Ordering::Acquire);
                
                // Check if object should be collected (White and not a root)
                if header.color.load(Ordering::Acquire) == Color::White && !header.is_root() {
                    // Remove from list by updating previous node's next pointer
                    (*prev_next).store(next, Ordering::Release);
                    
                    // Get size from vtable and call drop function
                    let size = header.vtable.layout.size();
                    (header.vtable.drop)(current);  // Proper Drop via Box::from_raw!
                    freed += size;
                    
                    // Move to next, keeping same prev
                    current = next;
                } else {
                    // Reset color for next cycle
                    header.color.store(Color::White, Ordering::Release);
                    
                    // Move both forward
                    prev_next = &header.next;
                    current = next;
                }
            }
        }

        self.bytes_allocated.fetch_sub(freed, Ordering::Relaxed);
        
        // Transition back to Idle phase
        self.phase.store(GcPhase::Idle as u8, Ordering::Release);
    }
    
    /// Perform an incremental collection with bounded work per step
    pub fn collect_incremental(&self, work_per_step: usize) {
        // Begin marking
        self.begin_mark();
        
        // Do incremental marking work until complete
        while !self.do_mark_work(work_per_step) {
            // Small yield to allow allocation to proceed
            std::hint::spin_loop();
        }
        
        // Sweep (this sets phase and does cleanup)
        self.sweep();
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
