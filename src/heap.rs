//! Heap management and object storage
//!
//! This module provides the heap structure that stores GC-managed objects
//! and implements the mark and sweep phases of garbage collection.

use crate::gc::GcContextHeapShared;
use crate::gc_box::{GcBox, GcHeader};
use crate::ptr::GcRoot;
use crate::trace::{Trace, Tracer};
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Send-safe wrapper for raw pointer queue
struct GrayQueue(Vec<*const GcHeader>);

unsafe impl Send for GrayQueue {}
unsafe impl Sync for GrayQueue {}

impl GrayQueue {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn pop(&mut self) -> Option<*const GcHeader> {
        self.0.pop()
    }
}

/// Send-safe list of threads associated with the heap
struct ThreadList(Vec<*const GcContextHeapShared>);

unsafe impl Send for ThreadList {}
unsafe impl Sync for ThreadList {}

impl ThreadList {
    #[inline]
    const fn new() -> Self {
        Self(Vec::new())
    }

    fn add(&mut self, thread: *const GcContextHeapShared) {
        self.0.push(thread);
    }

    fn remove(&mut self, thread: *const GcContextHeapShared) {
        if let Some(i) = self.0.iter().position(|&t| t.addr() == thread.addr()) {
            self.0.swap_remove(i);
        }
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
    /// Gray queue for incremental marking
    gray_queue: parking_lot::Mutex<GrayQueue>,
    /// Associated Threads
    threads: parking_lot::RwLock<ThreadList>,
}

impl Heap {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            head: AtomicPtr::new(null_mut()),
            bytes_allocated: AtomicUsize::new(0),
            threshold: AtomicUsize::new(1024 * 1024), // 1MB initial threshold
            gray_queue: parking_lot::Mutex::new(GrayQueue::new()),
            threads: parking_lot::RwLock::new(ThreadList::new()),
        })
    }

    pub fn with_options(_concurrent: bool, _collection_interval: std::time::Duration) -> Arc<Self> {
        // For now, just use the same configuration as new()
        // TODO: Add support for configuring concurrent collection and collection interval
        Self::new()
    }

    pub fn allocate<T: Trace>(&self, data: T) -> GcRoot<T> {
        let ptr = GcBox::new(data);
        let size = unsafe { (*ptr.as_ptr()).header.vtable.layout.size() };

        // Insert at head of linked list atomically
        let header_ptr = unsafe { &(*ptr.as_ptr()).header as *const GcHeader as *mut GcHeader };

        loop {
            let current_head = self.head.load(Ordering::Acquire);
            unsafe {
                (*header_ptr).next.store(current_head, Ordering::Relaxed);
            }

            if self
                .head
                .compare_exchange(
                    current_head,
                    header_ptr,
                    Ordering::Release,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                break;
            }
        }

        self.bytes_allocated.fetch_add(size, Ordering::Relaxed);

        // Return as GcRoot (already rooted with root_count = 1)
        unsafe { GcRoot::new_from_nonnull(ptr) }
    }

    pub fn force_collect(&self) {
        self.mark();
        self.sweep();
    }

    pub fn collect(&self) {
        if self.should_collect() {
            self.collect();
            // TODO: update threshold based on current usage
        }
    }

    /// Check if a collection should be triggered
    fn should_collect(&self) -> bool {
        self.bytes_allocated.load(Ordering::Relaxed) >= self.threshold.load(Ordering::Relaxed)
    }

    /// Perform a bounded amount of incremental marking work
    ///
    /// Returns true if marking is complete, false if more work remains
    pub fn mark_incremental(&self, work_budget: usize) -> bool {
        let mut gray_queue = self.gray_queue.lock();
        let mut work_done = 0;

        let tracer = Tracer::new();
        // TODO: merge global gray and task local queues to tracer
        while work_done < work_budget {
            match gray_queue.pop() {
                Some(ptr) => {
                    unsafe {
                        let header = &*ptr;

                        (header.vtable.trace)(ptr, &tracer);

                        // Merge tracer's gray queue
                        tracer.append_to(&mut gray_queue.0);

                        // Mark as black
                        header.color.mark_black();

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

    fn do_mark_work_full(&self, tracer: &Tracer) {
        let mut gray_queue = self.gray_queue.lock();

        while let Some(ptr) = gray_queue.pop() {
            unsafe {
                let header = &*ptr;

                (header.vtable.trace)(ptr, tracer);

                // Merge tracer's gray queue
                tracer.append_to(&mut gray_queue.0);

                // Mark as black
                header.color.mark_black();
            }
        }
    }

    fn do_mark_roots(&self, tracer: &Tracer) {
        // Walk the linked list to find roots
        let mut current = self.head.load(Ordering::Acquire);
        while !current.is_null() {
            unsafe {
                let header = &*current;
                if header.is_root() {
                    tracer.mark_header(header);
                }
                current = header.next.load(Ordering::Acquire);
            }
        }

        let mut gray_queue = self.gray_queue.lock();
        tracer.append_to(&mut gray_queue.0);
    }

    pub fn mark(&self) {
        let tracer = Tracer::new();

        self.do_mark_roots(&tracer);

        // Process gray queue
        self.do_mark_work_full(&tracer);
    }

    pub fn sweep(&self) {
        let mut freed = 0;

        unsafe {
            let mut current = self.head.load(Ordering::Acquire);
            let mut prev_next: *const AtomicPtr<GcHeader> = &self.head;

            while !current.is_null() {
                let header = &*current;
                let next = header.next.load(Ordering::Acquire);

                // Check if object should be collected
                if header.is_white() {
                    // Remove from list by updating previous node's next pointer
                    (*prev_next).store(next, Ordering::Release);

                    // Get size from vtable and call drop function
                    let size = header.vtable.layout.size();
                    (header.vtable.drop)(current); // Proper Drop via Box::from_raw!
                    freed += size;

                    // Move to next, keeping same prev
                    current = next;
                } else {
                    // Reset color for next cycle
                    header.color.reset_white();

                    // Move both forward
                    prev_next = &header.next;
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

impl GcContextHeapShared {
    pub(crate) fn register_with_heap(&self, heap: &Heap) {
        let mut threads = heap.threads.write();
        threads.add(self as *const _);
    }

    pub(crate) fn unregister_from_heap(&self, heap: &Heap) {
        let mut threads = heap.threads.write();
        threads.remove(self as *const _);
    }
}
