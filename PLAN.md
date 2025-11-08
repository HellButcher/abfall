# GC Improvement Plan - Concurrent Tri-Color Mark & Sweep

## Status: 5/7 Phases Complete (71%)

**Branch**: `feat/improved-gc` | **Commits**: 12 | **Tests**: ✓ All passing

### Completed ✅
1. Lock-free intrusive list + type-erased headers
2. Trace trait system for graph traversal
3. Allocation safety (root_count=1, no race)
4. VTable + Box allocation (proper Drop)
5. Incremental marking (Go GC-inspired)

### In Progress 🔄
6. Write barriers & GcCell (next)
7. Optimization & tuning

---

## Architecture Overview

### Core Structures
```rust
#[repr(C)]
struct GcBox<T: Trace> {
    header: GcHeader,
    data: T,
}

struct GcHeader {
    color: AtomicU8,           // Tri-color (White/Gray/Black)
    root_count: AtomicUsize,   // Root references
    next: AtomicPtr<GcHeader>, // Intrusive linked list
    vtable: &'static GcVTable, // Type-erased operations
}

struct GcVTable {
    layout: Layout,
    trace: unsafe fn(*const GcHeader, &mut Tracer),
    drop: unsafe fn(*mut GcHeader),
}

pub enum GcPhase {
    Idle = 0,
    Marking = 1,
    Sweeping = 2,
}
```

### Key Properties
- **Lock-free allocation**: CAS on head pointer
- **Type erasure**: VTable for trace/drop
- **Safety**: Objects start rooted (root_count=1)
- **Incremental**: Work budget system
- **Memory safe**: Box::into_raw/from_raw

---

## Phase Details

### Phase 1-4: Foundation ✅
**What was done**:
- Intrusive linked list (lock-free)
- Tri-color marking with AtomicU8
- Trace trait for graph traversal
- Fixed allocation race: objects start rooted
- VTable system with Layout field
- Box-based allocation (proper Drop)
- #[repr(C)] for safe pointer casts

**Key Commits**:
- Commits 1-3: Lock-free structures
- Commits 4-5: Trace trait
- Commits 6-7: Allocation safety
- Commits 8-10: VTable + Box

**Tests**: 7 unit tests, 3 examples

---

### Phase 5: Incremental Marking ✅

**Design**: Inspired by Go's concurrent GC
- Phase state machine (AtomicU8)
- GrayQueue with Mutex (Send/Sync wrapper)
- Work budget for controlled pauses

**API**:
```rust
// Initialize marking phase with roots
pub fn begin_mark(&self)

// Process bounded work (returns true if complete)
pub fn do_mark_work(&self, work_budget: usize) -> bool

// Full incremental collection
pub fn collect_incremental(&self, work_per_step: usize)
```

**Critical Bug Fixed**:
- sweep() had corrupted linked list manipulation
- Complex pointer-to-pointer logic caused misaligned pointers
- Fixed with simplified `*const AtomicPtr<GcHeader>` pattern
- Unified head/non-head node handling

**Results**:
- ✓ Incremental collection working correctly
- ✓ Test: 5 objects → 3 after GC (2 collected)
- ✓ Compatible with stop-the-world collect()

**Commit**: #11 (feat: implement incremental marking)

**New Dependency**: parking_lot (efficient Mutex)

---

### Phase 6: Write Barriers & GcCell 🔄 NEXT

**Goal**: Enable concurrent mutation during marking

**Design** (Go-inspired):
```rust
pub struct GcCell<T> {
    value: UnsafeCell<T>,
}

impl<T: Trace> GcCell<T> {
    pub fn set(&self, value: T) {
        // Write barrier: shade old value if marking
        if gc_phase() == Marking {
            // Yuasa: mark old value gray
            mark_gray(old_value);
        }
        // Store new value
        unsafe { *self.value.get() = value; }
    }
}
```

**Hybrid Barrier** (Dijkstra + Yuasa):
- Yuasa: Snapshot-at-beginning (shade old values)
- Dijkstra: Mark new values if black → white edge
- Go uses hybrid for concurrent safety

**Tasks**:
- [ ] Implement GcCell with write barrier
- [ ] Integrate with incremental marking
- [ ] Test concurrent mutation correctness
- [ ] Update Trace implementations

---

### Phase 7: Optimization 🔮 FUTURE

**Performance Goals** (Go-inspired):
- <1ms pause times for mark steps
- <10% CPU overhead for marking
- Support heaps up to 1GB

**Optimizations**:
- [ ] Background marking thread
- [ ] Pacer/trigger heuristics (heap growth-based)
- [ ] Hot path optimization
- [ ] Benchmarking suite

**Go GC Features to Consider**:
- Pacer (trigger GC before heap doubles)
- Assist mechanism (mutator helps when pressure high)
- Concurrent sweep

---

## Key Design Decisions

### Allocation Safety (Phase 3)
**Problem**: Race where object could be freed before first GcPtr created
```rust
// BEFORE (BROKEN):
let ptr = heap.allocate(data);    // root_count=0
let gc_ptr = GcPtr::new(ptr);     // increment root_count (TOO LATE!)
// GC could run here and free object!

// AFTER (FIXED):
fn allocate() {
    // Objects start rooted
    header.root_count.store(1, ...);  // Safe immediately
}
fn GcPtr::new() {
    // Don't increment - already rooted
}
```

### VTable System (Phase 4)
**Why Box instead of alloc API**:
- Proper Drop semantics automatically
- Layout stored in VTable once
- Type-erased drop_impl calls Box::from_raw
- No manual dealloc bookkeeping

### Sweep Corruption Fix (Phase 5)
**Problem**: Double dereference in list manipulation
```rust
// BEFORE: Complex and broken
let mut prev: *mut *mut GcHeader = ...;
(*(*prev)).next.store(next, ...);  // Crashes!

// AFTER: Simple and correct
let mut prev_next: *const AtomicPtr<GcHeader> = &self.head;
(*prev_next).store(next, ...);  // Works!
```

### Go GC Adaptations (Phase 5)
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
- Write barriers (Phase 6)
- Pacer heuristics (Phase 7)
- Background thread (Phase 7)

---

## Testing

**Unit Tests** (7):
- Allocation/deallocation
- Graph traversal
- Cycle collection
- Drop semantics

**Examples** (6):
- simple_safety.rs - Allocation safety
- allocation_safety.rs - Concurrent allocation
- vtable_drop_test.rs - Drop correctness
- trace_demo.rs - Object graphs
- incremental_test.rs - Incremental GC
- debug_trace.rs - GC behavior

**All passing** ✓

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

## Quick Reference

### Build & Test
```bash
cargo build
cargo test
cargo clippy
cargo run --example incremental_test
```

### Key Files
- `src/heap.rs` - Core GC implementation
- `src/gc.rs` - GcContext, GcPtr API
- `src/trace.rs` - Trace trait
- `examples/` - Usage demonstrations

### Dependencies
- `parking_lot` - Efficient Mutex for gray queue

---

**Last Updated**: Phase 5 complete (Incremental Marking)  
**Next Milestone**: Phase 6 (Write Barriers & GcCell)
