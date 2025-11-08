//! Heap management and object storage
//!
//! This module provides the heap structure that stores GC-managed objects
//! and implements the mark and sweep phases of garbage collection.

use crate::color::{AtomicColor, Color};
use std::alloc::{alloc, dealloc, Layout};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A garbage collected object with metadata
///
/// `GcBox` wraps a value with GC metadata including color, mark bit, and root status.
pub struct GcBox<T: ?Sized> {
    color: AtomicColor,
    marked: AtomicBool,
    root: AtomicBool,
    data: T,
}

impl<T> GcBox<T> {
    pub fn new(data: T) -> NonNull<GcBox<T>> {
        let layout = Layout::new::<GcBox<T>>();
        unsafe {
            let ptr = alloc(layout) as *mut GcBox<T>;
            if ptr.is_null() {
                panic!("Allocation failed");
            }
            ptr.write(GcBox {
                color: AtomicColor::new(Color::White),
                marked: AtomicBool::new(false),
                root: AtomicBool::new(true),
                data,
            });
            NonNull::new_unchecked(ptr)
        }
    }

    pub fn data(&self) -> &T {
        &self.data
    }

    #[allow(dead_code)]
    pub fn color(&self) -> Color {
        self.color.load(Ordering::Acquire)
    }

    pub fn set_color(&self, color: Color) {
        self.color.store(color, Ordering::Release);
    }

    pub fn is_marked(&self) -> bool {
        self.marked.load(Ordering::Acquire)
    }

    pub fn mark(&self) {
        self.marked.store(true, Ordering::Release);
    }

    pub fn unmark(&self) {
        self.marked.store(false, Ordering::Release);
    }

    pub fn is_root(&self) -> bool {
        self.root.load(Ordering::Acquire)
    }

    pub fn set_root(&self, is_root: bool) {
        self.root.store(is_root, Ordering::Release);
    }
}

/// The garbage collected heap
///
/// Manages allocation and deallocation of GC objects, and implements
/// the mark and sweep collection algorithm.
pub struct Heap {
    allocations: Mutex<Vec<usize>>,
    bytes_allocated: AtomicUsize,
    threshold: AtomicUsize,
}

impl Heap {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            allocations: Mutex::new(Vec::new()),
            bytes_allocated: AtomicUsize::new(0),
            threshold: AtomicUsize::new(1024 * 1024), // 1MB initial threshold
        })
    }

    pub fn allocate<T>(&self, data: T) -> NonNull<GcBox<T>> {
        let ptr = GcBox::new(data);
        let size = std::mem::size_of::<GcBox<T>>();
        
        self.allocations.lock().unwrap().push(ptr.as_ptr() as usize);
        self.bytes_allocated.fetch_add(size, Ordering::Relaxed);
        
        ptr
    }

    pub fn should_collect(&self) -> bool {
        self.bytes_allocated.load(Ordering::Relaxed) 
            >= self.threshold.load(Ordering::Relaxed)
    }

    pub fn sweep(&self) {
        let mut allocations = self.allocations.lock().unwrap();
        let mut i = 0;
        let mut freed = 0;

        while i < allocations.len() {
            let addr = allocations[i];
            unsafe {
                let ptr = addr as *mut GcBox<u8>;
                let gc_box = &*ptr;
                
                if !gc_box.is_marked() && !gc_box.is_root() {
                    let layout = Layout::for_value(gc_box);
                    freed += layout.size();
                    dealloc(ptr as *mut u8, layout);
                    allocations.swap_remove(i);
                } else {
                    gc_box.unmark();
                    gc_box.set_color(Color::White);
                    i += 1;
                }
            }
        }

        self.bytes_allocated.fetch_sub(freed, Ordering::Relaxed);
    }

    pub fn mark_from_roots(&self) {
        let allocations = self.allocations.lock().unwrap();
        let mut gray_queue: Vec<usize> = Vec::new();

        for &addr in allocations.iter() {
            unsafe {
                let ptr = addr as *mut GcBox<u8>;
                let gc_box = &*ptr;
                if gc_box.is_root() {
                    gc_box.set_color(Color::Gray);
                    gray_queue.push(addr);
                }
            }
        }

        while let Some(addr) = gray_queue.pop() {
            unsafe {
                let ptr = addr as *mut GcBox<u8>;
                let gc_box = &*ptr;
                gc_box.mark();
                gc_box.set_color(Color::Black);
            }
        }
    }

    pub fn bytes_allocated(&self) -> usize {
        self.bytes_allocated.load(Ordering::Relaxed)
    }

    pub fn allocation_count(&self) -> usize {
        self.allocations.lock().unwrap().len()
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        let allocations = self.allocations.lock().unwrap();
        for &addr in allocations.iter() {
            unsafe {
                let ptr = addr as *mut GcBox<u8>;
                let gc_box = &*ptr;
                let layout = Layout::for_value(gc_box);
                dealloc(ptr as *mut u8, layout);
            }
        }
    }
}
