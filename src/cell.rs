//! Interior mutability with write barriers for concurrent GC
//!
//! This module provides cells with write barriers for the tri-color marking algorithm:
//! - `GcPtrCell<T>`: Stores GcPtr with Dijkstra write barrier
//! - `GcRefCell<T>`: RefCell-like with Yuasa write barrier
//!
//! For `Copy` types (primitives, etc.), use `std::cell::Cell<T>` directly since
//! they cannot contain GC pointers and don't need write barriers.

use crate::ptr::GcPtr;
use crate::trace::{Trace, Tracer};
use crate::heap::Heap;
use std::cell::{Cell, UnsafeCell};
use std::sync::Arc;
use std::ops::{Deref, DerefMut};

/// Cell for storing GcPtr with Dijkstra write barrier
///
/// `GcPtrCell` enables mutation of GC pointers while maintaining the
/// tri-color invariant during concurrent marking.
///
/// # Write Barrier (Dijkstra)
///
/// When a new pointer is stored, if marking is in progress, the new
/// pointer is immediately shaded gray to prevent it from being collected.
pub struct GcPtrCell<T> {
    value: UnsafeCell<GcPtr<T>>,
    heap: Arc<Heap>,
}

impl<T> GcPtrCell<T> {
    pub fn new(value: GcPtr<T>, heap: Arc<Heap>) -> Self {
        Self {
            value: UnsafeCell::new(value),
            heap,
        }
    }

    pub fn get(&self) -> GcPtr<T> {
        unsafe { (*self.value.get()).clone() }
    }

    /// Set the contained pointer with Dijkstra write barrier
    pub fn set(&self, new_value: GcPtr<T>) {
        // Dijkstra barrier: shade new pointer gray if marking
        if self.heap.is_marking() {
            self.heap.mark_gray(new_value.header_ptr());
        }
        
        unsafe {
            *self.value.get() = new_value;
        }
    }

    pub fn replace(&self, new_value: GcPtr<T>) -> GcPtr<T> {
        // Dijkstra barrier: shade new pointer gray if marking
        if self.heap.is_marking() {
            self.heap.mark_gray(new_value.header_ptr());
        }
        
        unsafe {
            std::ptr::replace(self.value.get(), new_value)
        }
    }

    pub fn swap(&self, other: &Self) {
        // Both pointers stay reachable through the cells, no barrier needed
        unsafe {
            std::ptr::swap(self.value.get(), other.value.get());
        }
    }
}

impl<T> std::fmt::Debug for GcPtrCell<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcPtrCell").finish_non_exhaustive()
    }
}

unsafe impl<T> Trace for GcPtrCell<T> {
    fn trace(&self, tracer: &mut Tracer) {
        unsafe {
            (*self.value.get()).trace(tracer);
        }
    }
}

unsafe impl<T: Send> Send for GcPtrCell<T> {}
unsafe impl<T: Sync> Sync for GcPtrCell<T> {}

/// Borrow state for GcRefCell
#[derive(Copy, Clone, PartialEq, Eq)]
enum BorrowState {
    Unused,
    Shared(usize),
    Exclusive,
}

/// RefCell-like cell with Yuasa write barrier
///
/// `GcRefCell<T>` provides runtime borrow checking and ensures write barrier
/// semantics for mutations during GC marking.
///
/// # Write Barrier (Yuasa)
///
/// When a mutable borrow is released, if marking is in progress, the
/// modified value is traced to ensure any new pointers are marked.
pub struct GcRefCell<T> {
    value: UnsafeCell<T>,
    borrow: Cell<BorrowState>,
    heap: Arc<Heap>,
}

impl<T> GcRefCell<T> {
    pub fn new(value: T, heap: Arc<Heap>) -> Self {
        Self {
            value: UnsafeCell::new(value),
            borrow: Cell::new(BorrowState::Unused),
            heap,
        }
    }

    pub fn borrow(&self) -> GcRef<'_, T> {
        match self.borrow.get() {
            BorrowState::Exclusive => panic!("already mutably borrowed"),
            BorrowState::Shared(n) => {
                self.borrow.set(BorrowState::Shared(n + 1));
            }
            BorrowState::Unused => {
                self.borrow.set(BorrowState::Shared(1));
            }
        }
        GcRef { cell: self }
    }

    pub fn borrow_mut(&self) -> GcRefMut<'_, T>
    where
        T: Trace,
    {
        match self.borrow.get() {
            BorrowState::Unused => {
                self.borrow.set(BorrowState::Exclusive);
                GcRefMut { cell: self }
            }
            _ => panic!("already borrowed"),
        }
    }

    pub fn try_borrow(&self) -> Option<GcRef<'_, T>> {
        match self.borrow.get() {
            BorrowState::Exclusive => None,
            BorrowState::Shared(n) => {
                self.borrow.set(BorrowState::Shared(n + 1));
                Some(GcRef { cell: self })
            }
            BorrowState::Unused => {
                self.borrow.set(BorrowState::Shared(1));
                Some(GcRef { cell: self })
            }
        }
    }

    pub fn try_borrow_mut(&self) -> Option<GcRefMut<'_, T>>
    where
        T: Trace,
    {
        match self.borrow.get() {
            BorrowState::Unused => {
                self.borrow.set(BorrowState::Exclusive);
                Some(GcRefMut { cell: self })
            }
            _ => None,
        }
    }
}

impl<T> std::fmt::Debug for GcRefCell<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcRefCell").finish_non_exhaustive()
    }
}

unsafe impl<T: Trace> Trace for GcRefCell<T> {
    fn trace(&self, tracer: &mut Tracer) {
        unsafe {
            (*self.value.get()).trace(tracer);
        }
    }
}

unsafe impl<T: Send> Send for GcRefCell<T> {}
unsafe impl<T: Sync> Sync for GcRefCell<T> {}

/// Immutable borrow guard for GcRefCell
pub struct GcRef<'a, T> {
    cell: &'a GcRefCell<T>,
}

impl<T> Deref for GcRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.cell.value.get() }
    }
}

impl<T> Drop for GcRef<'_, T> {
    fn drop(&mut self) {
        match self.cell.borrow.get() {
            BorrowState::Shared(1) => self.cell.borrow.set(BorrowState::Unused),
            BorrowState::Shared(n) => self.cell.borrow.set(BorrowState::Shared(n - 1)),
            _ => unreachable!(),
        }
    }
}

/// Mutable borrow guard for GcRefCell with Yuasa write barrier
pub struct GcRefMut<'a, T: Trace> {
    cell: &'a GcRefCell<T>,
}

impl<T: Trace> Deref for GcRefMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.cell.value.get() }
    }
}

impl<T: Trace> DerefMut for GcRefMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.cell.value.get() }
    }
}

impl<T: Trace> Drop for GcRefMut<'_, T> {
    fn drop(&mut self) {
        // Yuasa write barrier: trace modified value if marking
        if self.cell.heap.is_marking() {
            let mut tracer = Tracer::new();
            unsafe {
                (*self.cell.value.get()).trace(&mut tracer);
            }
            
            // Process any gray objects found
            for ptr in tracer.gray_queue_mut().drain(..) {
                self.cell.heap.mark_gray(ptr);
            }
        }
        
        self.cell.borrow.set(BorrowState::Unused);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GcContext;

    #[test]
    fn test_gcptrcell_basic() {
        let ctx = GcContext::new();
        let value1 = ctx.allocate(10);
        let value2 = ctx.allocate(20);
        
        let cell = GcPtrCell::new(value1.clone(), Arc::clone(&ctx.heap));
        assert_eq!(*cell.get(), 10);
        
        cell.set(value2.clone());
        assert_eq!(*cell.get(), 20);
    }

    #[test]
    fn test_gcptrcell_write_barrier() {
        let ctx = GcContext::new();
        let value1 = ctx.allocate(10);
        let value2 = ctx.allocate(20);
        
        let cell_ptr = ctx.allocate(GcPtrCell::new(value1.clone(), Arc::clone(&ctx.heap)));
        
        // Start marking
        ctx.heap.begin_mark();
        assert!(ctx.heap.is_marking());
        
        // Mutation during marking - write barrier should shade value2
        let cell = unsafe { &*cell_ptr.as_ptr() };
        cell.set(value2.clone());
        
        // Complete marking
        while !ctx.heap.do_mark_work(10) {}
        
        // Both values should be reachable
        ctx.heap.sweep();
        assert_eq!(*value2, 20);
    }

    #[test]
    fn test_gcrefcell_basic() {
        let ctx = GcContext::new();
        let cell = GcRefCell::new(42, Arc::clone(&ctx.heap));
        
        {
            let borrowed = cell.borrow();
            assert_eq!(*borrowed, 42);
        }
        
        {
            let mut borrowed_mut = cell.borrow_mut();
            *borrowed_mut = 100;
        }
        
        let borrowed = cell.borrow();
        assert_eq!(*borrowed, 100);
    }

    #[test]
    fn test_gcrefcell_multiple_borrows() {
        let ctx = GcContext::new();
        let cell = GcRefCell::new(42, Arc::clone(&ctx.heap));
        
        let b1 = cell.borrow();
        let b2 = cell.borrow();
        assert_eq!(*b1, 42);
        assert_eq!(*b2, 42);
    }

    #[test]
    #[should_panic(expected = "already mutably borrowed")]
    fn test_gcrefcell_borrow_mut_conflict() {
        let ctx = GcContext::new();
        let cell = GcRefCell::new(42, Arc::clone(&ctx.heap));
        
        let _mut_borrow = cell.borrow_mut();
        let _borrow = cell.borrow(); // Should panic
    }

    #[test]
    #[should_panic(expected = "already borrowed")]
    fn test_gcrefcell_double_mut_borrow() {
        let ctx = GcContext::new();
        let cell = GcRefCell::new(42, Arc::clone(&ctx.heap));
        
        let _mut1 = cell.borrow_mut();
        let _mut2 = cell.borrow_mut(); // Should panic
    }
}
