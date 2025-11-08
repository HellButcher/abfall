# GC Improvement Plan - Concurrent Tri-Color Mark & Sweep

## Status: Phase 8 - Architecture refinement & optimization

**Objective**: Refine GcPtr/GcRoot separation, optimize thread-local access, improve write barriers

### Current State (Phase 7 Complete ✅)
- ✅ Thread-local GC context via TLS
- ✅ RAII GcContext guard (non-Send/Sync)
- ✅ Shared Heap (Send+Sync)
- ✅ GcPtr/GcRoot separation started
- ✅ Pointer-sized GcPtr achieved
- ⚠️ GcCell needs write barrier implementation
- ⚠️ Examples/tests need updates for new API

### Phase 8 Goals
1. Complete GcPtr/GcRoot refactor
2. Implement proper write barriers in GcCell
3. Update all examples/tests
4. Performance validation

---

## Architecture Overview

### Current Structure

```rust
// 1. GC Object Layout (repr(C) for safety)
#[repr(C)]
struct GcBox<T: Trace> {
    header: GcHeader,  // Always at offset 0
    data: T,
}

// 2. Type-erased header
struct GcHeader {
    color: AtomicU8,              // Tri-color (White/Gray/Black)
    root_count: AtomicUsize,      // Root references (0 = eligible for GC)
    next: AtomicPtr<GcHeader>,    // Intrusive linked list
    vtable: &'static GcVTable,    // Type-erased ops
}

// 3. VTable for type erasure
struct GcVTable {
    layout: Layout,
    trace: unsafe fn(*const GcHeader, &mut Tracer),
    drop: unsafe fn(*mut GcHeader),
}

// 4. Heap (Send+Sync)
pub struct Heap {
    head: AtomicPtr<GcHeader>,           // Lock-free allocation list
    bytes_allocated: AtomicUsize,
    threshold: AtomicUsize,
    phase: AtomicU8,                     // Idle/Marking/Sweeping
    gray_queue: Mutex<GrayQueue>,        // For incremental marking
}

// 5. Thread-local context
thread_local! {
    static CURRENT_HEAP: RefCell<Option<Arc<Heap>>> = ...;
}

// 6. API (GcContext is !Send + !Sync)
pub struct GcContext {
    heap: Arc<Heap>,
    _marker: PhantomData<*const ()>,
}

// 7. Pointers
#[repr(transparent)]
pub struct GcPtr<T>(NonNull<GcBox<T>>);  // Copy, no Deref

#[repr(transparent)]
pub struct GcRoot<T>(GcPtr<T>);          // Deref, manages root_count
```

### Key Properties
- **Lock-free allocation**: CAS on head pointer
- **Type erasure**: VTable for trace/drop
- **Safety**: Objects start rooted (root_count=1)
- **Incremental**: Work budget system
- **Memory safe**: Box::into_raw/from_raw
- **Thread-local**: No heap pointer in GcPtr (8 bytes)
- **Shared heap**: Multiple threads via Arc<Heap>

---

## Implementation Phases

### Phase 1-4: Foundation ✅
- Intrusive linked list (lock-free)
- Tri-color marking with AtomicU8
- Trace trait for graph traversal
- Fixed allocation race: objects start rooted
- VTable system with Layout field
- Box-based allocation (proper Drop)
- #[repr(C)] for safe pointer casts

### Phase 5: Incremental Marking ✅
- Phase state machine (Idle/Marking/Sweeping)
- GrayQueue with Mutex (Send/Sync wrapper)
- Work budget for controlled pauses
- Fixed sweep() linked list corruption

**API**:
```rust
pub fn begin_mark(&self)
pub fn do_mark_work(&self, work_budget: usize) -> bool
pub fn collect_incremental(&self, work_per_step: usize)
```

### Phase 6: Write Barriers (Initial) ✅
- GcPtrCell with Dijkstra barrier
- GcRefCell with Yuasa barrier
- Heap API: `is_marking()`, `mark_gray()`

**Note**: Replaced with simpler GcCell in Phase 7, write barrier pending

### Phase 7: Thread-Local Context ✅
- Thread-local heap storage
- GcContext RAII guard
- Pointer-sized GcPtr
- GcPtr/GcRoot separation
- Shared heap support

### Phase 8: Write Barriers & Refinement 🔄
**Current work**:
- Implement write barrier in GcCell
- TLS helper for current heap access
- Update examples/tests
- Validation & testing

---

### Phase 7: Thread-Local Context ✅ COMPLETE

**Implemented**:
- ✅ Thread-local heap via TLS
- ✅ GcContext RAII guard (non-Send/Sync)
- ✅ Shared Arc<Heap> (Send+Sync)
- ✅ GcContext::with_heap() for sharing
- ✅ GcPtr now pointer-sized (no heap field)
- ✅ GcPtr/GcRoot separation begun

**Architecture**:
```rust
// Thread-local storage
thread_local! {
    static CURRENT_HEAP: RefCell<Option<Arc<Heap>>> = ...;
}

// Non-Send/Sync context (manages TLS)
pub struct GcContext {
    heap: Arc<Heap>,
    _marker: PhantomData<*const ()>,  // !Send + !Sync
}

// Pointer-sized GcPtr (Copy)
#[repr(transparent)]
pub struct GcPtr<T>(NonNull<GcBox<T>>);  // 8 bytes on 64-bit

// Rooted pointer (Deref, Drop)
#[repr(transparent)]
pub struct GcRoot<T>(GcPtr<T>);
```

**API Changes**:
- `allocate()` returns `GcRoot<T>` (already rooted)
- `GcPtr::root()` creates new root (inc root_count)
- `GcRoot::as_ptr()` gets unrooted GcPtr
- Only `GcRoot` implements `Deref`

---

### Phase 8: Write Barriers & Refinement 🔄 IN PROGRESS

**Current Issues**:
1. GcCell has no write barrier (TODO comments)
2. Needs access to current heap via TLS
3. Examples may need updates for GcPtr/GcRoot API
4. Write barrier strategy needs finalization

**Write Barrier Design**:
```rust
// Option A: GcCell for GcPtr (Dijkstra barrier)
impl<T> GcCell<GcPtr<T>> {
    pub fn set(&self, new: GcPtr<T>) {
        // Access current heap via TLS
        if let Some(heap) = get_current_heap() {
            if heap.is_marking() {
                heap.mark_gray(new.header_ptr());
            }
        }
        unsafe { *self.value.get() = new; }
    }
}

// Option B: Specialized GcPtrCell
pub struct GcPtrCell<T> {
    value: UnsafeCell<GcPtr<T>>,
}
// Always applies Dijkstra barrier when marking

// For non-GC types: use std::cell::Cell<T> directly
```

**Tasks**:
- [ ] Implement write barrier in GcCell or GcPtrCell
- [ ] Add TLS helper: `get_current_heap() -> Option<Arc<Heap>>`
- [ ] Update examples to use GcPtr/GcRoot correctly
- [ ] Validate write barrier correctness
- [ ] Performance testing

---

### Phase 9: Optimization 🔮 FUTURE

**Performance Goals** (Go-inspired):
- <1ms pause times for mark steps
- <10% CPU overhead for marking
- Support heaps up to 1GB

**Optimizations**:
- [ ] Background marking thread
- [ ] Pacer/trigger heuristics (heap growth-based)
- [ ] Hot path optimization (allocate, mark_gray)
- [ ] Benchmarking suite
- [ ] VTable caching (avoid leaking one per type)

**Go GC Features to Consider**:
- Pacer (trigger GC before heap doubles)
- Assist mechanism (mutator helps when pressure high)
- Concurrent sweep

---

## Key Design Decisions

### 1. Allocation Safety (Phase 3)
**Problem**: Race where object could be freed before first root created

```rust
// FIXED: Objects start rooted (root_count=1)
fn allocate<T>(data: T) -> GcRoot<T> {
    header.root_count.store(1, ...);  // Safe immediately
    GcRoot::new_from_nonnull(ptr)     // Already rooted
}
```

### 2. VTable System (Phase 4)
**Why Box instead of alloc API**:
- Proper Drop semantics automatically
- Layout stored in VTable once
- Type-erased drop_impl calls Box::from_raw
- No manual dealloc bookkeeping

### 3. Sweep Corruption Fix (Phase 5)
```rust
// FIXED: Simple and correct
let mut prev_next: *const AtomicPtr<GcHeader> = &self.head;
(*prev_next).store(next, ...);  // Works!
```

### 4. GcPtr/GcRoot Separation (Phase 7)
**Design rationale**:
- `GcPtr`: Lightweight Copy pointer, no Deref, no root management
- `GcRoot`: Rooted reference, Deref, manages root_count (like Rc/Arc)
- Enables storing GcPtr in data structures without circular roots
- Only 8 bytes per pointer (no heap field needed)

**API**:
```rust
let root = ctx.allocate(42);        // GcRoot<i32>
let ptr = root.as_ptr();            // GcPtr<i32> (Copy)
let root2 = unsafe { ptr.root() };  // Create new root
// GcPtr in structs: no circular root references
```

### 5. Thread-Local Context (Phase 7)
**Why TLS**:
- Pointer-sized GcPtr (no heap field)
- Separate heaps per thread doesn't make sense (mixing objects = unsafe)
- Shared heap via Arc<Heap> for multi-threading
- GcContext is !Send/!Sync (manages TLS)

**Pattern**:
```rust
// Thread 1
let ctx = GcContext::new();
let heap = Arc::clone(ctx.heap());

// Thread 2
thread::spawn(move || {
    let ctx2 = GcContext::with_heap(heap);
    // Shares same heap, different TLS context
});
```

### 6. Write Barrier Strategy (Phase 8)
**Dijkstra vs Yuasa**:
- Dijkstra: Shade new pointer gray (insertion barrier)
- Yuasa: Trace old value (deletion/snapshot barrier)

**Current approach**: Dijkstra for GcPtr updates
```rust
impl<T> GcCell<GcPtr<T>> {
    pub fn set(&self, new: GcPtr<T>) {
        if heap.is_marking() {
            heap.mark_gray(new.header_ptr());  // Dijkstra
        }
        unsafe { *self.value.get() = new; }
    }
}
```

### 7. Go GC Adaptations (Phase 5)
**Adopted**:
- ✅ Tri-color marking
- ✅ Phase state machine
- ✅ Work budget system
- ✅ Shared gray queue

**Different in Rust**:
- No stack scanning (GcPtr only)
- Simpler roots (no goroutine stacks)
- Type-safe Trace trait

**Deferred**:
- Write barriers (Phase 8)
- Pacer heuristics (Phase 9)
- Background thread (Phase 9)

---

## Testing & Examples

### Unit Tests (4 tests, all passing ✅)
- `basic_allocation` - Simple allocation/deref
- `allocation_and_collection` - GC reclaims memory
- `test_gcptrcell_basic` - GcCell operations
- `test_gcptrcell_write_barrier` - Write barrier behavior

### Doc Tests (8 tests, all passing ✅)
- GcContext API examples
- allocate(), collect(), collect_incremental()
- Thread-local and shared heap patterns

### Examples (11 examples)
1. `simple_safety.rs` - Basic allocation safety
2. `allocation_safety.rs` - Concurrent allocation patterns
3. `vtable_drop_test.rs` - Drop correctness validation
4. `trace_demo.rs` - Object graph traversal
5. `incremental_test.rs` - Incremental GC demonstration
6. `debug_trace.rs` - GC behavior debugging
7. `debug_incremental.rs` - Incremental marking debug
8. `demo.rs` - Complete API demo
9. `gccell_test.rs` - GcCell usage patterns
10. `concurrent_shared.rs` - Shared heap across threads
11. `multi_threaded.rs` - Multi-threaded GC example

### Status
- ✅ All tests passing
- ⚠️ Some examples may need updates for GcPtr/GcRoot API
- ⚠️ Write barrier implementation pending in GcCell

---

## Module Structure

```
src/
├── lib.rs           - Public API exports
├── gc.rs            - GcContext (thread-local API)
├── gc_box.rs        - GcBox, GcHeader, GcVTable
├── heap.rs          - Heap (allocation, mark, sweep)
├── ptr.rs           - GcPtr, GcRoot
├── color.rs         - Color enum, AtomicColor
├── trace.rs         - Trace trait, Tracer, NoTrace
└── cell.rs          - GcCell (write barriers)

examples/
├── simple_safety.rs          - Basic patterns
├── allocation_safety.rs      - Concurrent patterns
├── multi_threaded.rs         - Shared heap
├── incremental_test.rs       - Incremental GC
└── ... (7 more)
```

---

## Dependencies

```toml
[dependencies]
parking_lot = "0.12"  # Efficient Mutex for gray queue
```

**Why parking_lot**:
- Better performance than std::sync::Mutex
- Send+Sync wrapper for GrayQueue
- Required for incremental marking

---

## Next Steps (Phase 8)

### Immediate Tasks
1. **Implement write barrier in GcCell**
   - Add TLS helper: `get_current_heap()`
   - Apply Dijkstra barrier for GcPtr updates
   - Handle case when no context is active
   
2. **Update examples for GcPtr/GcRoot API**
   - Review which examples use GcPtr directly
   - Ensure root management is correct
   - Validate no UB from unrooted access

3. **Testing & Validation**
   - Test write barrier correctness
   - Concurrent mutation during marking
   - Multi-threaded scenarios
   
4. **Documentation**
   - Update API docs for GcPtr/GcRoot
   - Write barrier behavior
   - Thread-local context patterns

### Code Changes Needed

```rust
// gc.rs - Add TLS helper
pub(crate) fn get_current_heap() -> Option<Arc<Heap>> {
    CURRENT_HEAP.with(|h| h.borrow().clone())
}

// cell.rs - Implement write barrier
impl<T> GcCell<GcPtr<T>> {
    pub fn set(&self, new: GcPtr<T>) {
        if let Some(heap) = crate::gc::get_current_heap() {
            if heap.is_marking() {
                heap.mark_gray(new.header_ptr());
            }
        }
        unsafe { *self.value.get() = new; }
    }
}

// trace.rs - Implement Trace for GcCell
unsafe impl<T: Trace> Trace for GcCell<T> {
    fn trace(&self, tracer: &mut Tracer) {
        self.get().trace(tracer);
    }
}
```

---

## Performance Considerations

### Current Performance Characteristics
- **Allocation**: Lock-free CAS (fast path)
- **Mark**: Incremental with work budget
- **Sweep**: Stop-the-world (fast, single pass)
- **Write barrier**: Single atomic check + potential mark_gray

### Known Bottlenecks
- VTable leak (one per unique type used)
- Gray queue mutex contention
- Root count atomics (can be relaxed further)

### Future Optimizations (Phase 9)
- VTable caching/reuse
- Lock-free gray queue
- SIMD for color transitions
- Parallel marking
- Background collection thread

---

## Build & Run

```bash
# Build
cargo build

# Test
cargo test

# Clippy
cargo clippy

# Examples
cargo run --example demo
cargo run --example multi_threaded
cargo run --example incremental_test
```

---

## References

**Go GC**:
- [Go GC Guide](https://tip.golang.org/doc/gc-guide)
- [Go 1.5 Concurrent GC](https://go.dev/blog/go15gc)
- `runtime/mgc.go`, `runtime/mbarrier.go` (Go source)

**Papers**:
- Dijkstra et al., "On-the-Fly GC" (1978)
- Baker, "Real-time GC" (1978)
- Yuasa, Snapshot-at-Beginning algorithm

---

**Last Updated**: Phase 7 complete (Thread-Local Context)  
**Current Milestone**: Phase 8 (Write Barriers & Refinement)  
**Status**: ✅ All tests passing, write barrier implementation pending
