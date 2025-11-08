# Garbage Collector Improvement Plan

## Goals
1. ✅ Eliminate separate mark bit - use color atomically
2. ✅ Reduce lock contention
3. ✅ Use intrusive linked list for allocations
4. ✅ Add Trace trait for object graph traversal
5. Implement incremental/concurrent marking (inspired by Go GC)
6. Refine borrowing model (Rc/Arc-like)
7. Add GcCell for write barriers
8. **NEW:** Ensure allocation safety (objects rooted until linked)
9. **NEW:** Use Box allocation + VTable for proper Drop semantics
10. **NEW:** Apply Go GC design patterns (pacer, write barriers, phase tracking)

## Phase 1: Core Data Structure Improvements ✅ COMPLETE

### 1.1 Unified Color-Mark System ✅
- ✅ Remove separate `marked` AtomicBool from GcBox
- ✅ Use Color directly: White = unmarked, Black = marked
- ✅ Gray only used during active marking phase
- ✅ Atomic color transitions handle synchronization

### 1.2 Type-Erased Header + Intrusive Linked List ✅
```rust
// Type-erased header for all GC objects
struct GcHeader {
    color: AtomicColor,
    root_count: AtomicUsize,  // Reference count for roots
    next: AtomicPtr<GcHeader>, // Intrusive linked list
    trace_fn: unsafe fn(*const GcHeader), // Type-erased trace function
}

// GcBox now includes header + data
struct GcBox<T> {
    header: GcHeader,
    data: T,
}
```

Benefits:
- No Vec allocation list (lock-free traversal)
- Type erasure allows uniform handling
- Each object knows how to trace itself

### 1.3 Lock-Free Allocation List ✅
- ✅ Replace `Mutex<Vec<usize>>` with `AtomicPtr<GcHeader>`
- ✅ Use atomic operations for list insertion
- ✅ Mark phase traverses linked list without locks

### 1.4 Allocation Safety (NEW - CRITICAL)

**Problem:** Race condition between allocation and linking
```rust
// Thread A allocates object
let obj = ctx.allocate(data);  // obj has root_count=1

// Thread B runs GC between allocation and linking
// If marking starts here, obj is White and root_count=0
// BEFORE GcPtr::new increments root_count!
ctx.collect();  // Could collect obj before it's linked!

// Thread A links object (TOO LATE)
node.next = Some(obj);
```

**Solution Options:**

**Option 1: Allocate as Black (during marking)**
- New objects allocated during marking start as Black
- Prevents premature collection
- Requires tracking GC phase state

**Option 2: Start with root_count=1 (CURRENT)**
- Objects start rooted (root_count=0, then GcPtr::new increments to 1)
- Safe because GcPtr is created atomically with allocation
- **ISSUE:** Current code has objects start at root_count=0, then increment
  - This creates a tiny window where root_count=0!
  - Fix: Initialize root_count=1 in GcHeader::new()

**Option 3: Allocate as Gray**
- New objects start Gray during collection
- Forces them to be processed in current cycle
- Most conservative approach

**Recommendation: Option 2 (with fix)**
```rust
impl GcHeader {
    pub fn new<T>(...) -> Self {
        Self {
            root_count: AtomicUsize::new(1),  // Start rooted!
            // ...
        }
    }
}

// In GcPtr::new, we DON'T increment (already at 1)
// In GcPtr::clone, we DO increment
// In GcPtr::drop, we DO decrement
```

**Action Items:**
- [ ] Review current initialization logic
- [ ] Ensure no race window between allocation and rooting
- [ ] Add tests for concurrent allocation during collection
- [ ] Document allocation safety guarantees

## Phase 2: Trace Trait System ✅ COMPLETE

### 2.1 Trace Trait Definition ✅
- ✅ Trace trait for types participating in GC
- ✅ NoTrace marker for types without GC pointers
- ✅ Blanket implementations for primitives and std types

### 2.2 Tracer API ✅
- ✅ Tracer manages gray queue during marking
- ✅ Atomic color transitions (White -> Gray -> Black)
- ✅ Type-safe marking through trait bounds

### 2.3 User-Defined Tracing ✅
- ✅ Users implement Trace for custom types
- ✅ Type-erased trace functions in GcHeader
- ✅ Zero-cost for NoTrace types

**Known Issues:**
- Some complex clone/drop scenarios may leave objects uncollected
- Needs investigation and comprehensive testing

## Phase 3: Allocation Safety Review & Fixes

**Priority: HIGH - Prevent race conditions**

### 3.1 Current State Analysis
```rust
// Current (PROBLEMATIC):
GcHeader::new() initializes root_count=0
GcPtr::new() increments to 1
// Race window exists between these!

unsafe impl Trace for Node {
    fn trace(&self, tracer: &mut Tracer) {
        if let Some(ref next) = self.next {
            tracer.mark(next);
        }
    }
}
```

// Fixed (SAFE):
GcHeader::new() initializes root_count=1  // Already rooted!
GcPtr::new() does NOT increment (already at 1)
GcPtr::clone() DOES increment
GcPtr::drop() DOES decrement
```

### 3.2 Fix Implementation Steps
1. Change GcHeader::new() to start with root_count=1
2. Update GcPtr::new() to NOT call inc_root() 
3. Verify GcPtr::clone() calls inc_root() ✓
4. Verify GcPtr::drop() calls dec_root() ✓

### 3.3 Testing Strategy
- [ ] Test concurrent allocation during collection
- [ ] Test object allocation in tight loop with concurrent GC
- [ ] Stress test with many threads allocating
- [ ] Verify no premature collection

### 3.4 Alternative: Black Allocation During Marking
**Future consideration for incremental GC:**
- Track GC phase (Idle, Marking, Sweeping)
- Allocate as Black during Marking phase
- Allocate as White during Idle/Sweeping
- Provides stronger guarantees for incremental collection

## Phase 4: VTable-Based Type Erasure & Box Allocation

**Priority: HIGH - Fix memory management architecture**

### 4.1 Problem: Current Manual Memory Management

**Issues with current approach:**
```rust
// Current (PROBLEMATIC):
impl<T> GcBox<T> {
    pub fn new(...) -> NonNull<GcBox<T>> {
        let layout = Layout::new::<GcBox<T>>();
        let ptr = alloc(layout) as *mut GcBox<T>;  // Manual alloc
        ptr.write(GcBox { ... });
    }
}

// During sweep:
let layout = Layout::for_value(&*current);  // Recompute layout!
dealloc(current as *mut u8, layout);        // Manual dealloc
// Problem: No Drop::drop() called! Memory leak for types with Drop
```

**Problems:**
1. Using raw `alloc`/`dealloc` bypasses Rust's drop semantics
2. Must recompute `Layout` during deallocation (expensive)
3. Types with `Drop` implementations leak resources
4. No type safety guarantees

### 4.2 Solution: VTable + Box Allocation

**VTable Design:**
```rust
/// Type-erased vtable for GC operations
pub struct GcVTable {
    /// Trace function for marking
    pub trace: unsafe fn(*const GcHeader, &mut Vec<*const GcHeader>),
    /// Drop function - uses Box::from_raw
    pub drop: unsafe fn(*mut GcHeader),
    /// Size for statistics
    pub size: usize,
    /// Alignment requirement
    pub align: usize,
}

impl GcVTable {
    pub const fn new<T: Trace>() -> &'static Self {
        &GcVTable {
            trace: trace_impl::<T>,
            drop: drop_impl::<T>,
            size: std::mem::size_of::<GcBox<T>>(),
            align: std::mem::align_of::<GcBox<T>>(),
        }
    }
}

unsafe fn drop_impl<T>(ptr: *mut GcHeader) {
    let gc_box_ptr = ptr as *mut GcBox<T>;
    let _box = Box::from_raw(gc_box_ptr);
    // Box::drop automatically called!
}
```

**Benefits:**
- Proper Drop semantics
- No Layout recomputation
- Type-safe deallocation
- Cached size information

### 4.3 Implementation Steps

1. [ ] Design GcVTable struct
2. [ ] Update GcHeader with vtable field
3. [ ] Rewrite GcBox::new using Box::new + leak
4. [ ] Update sweep to use vtable.drop
5. [ ] Test with Drop types (String, Vec, etc.)

## Phase 5: Incremental/Concurrent Marking (Inspired by Go GC)

**Design Reference: Go's Concurrent Mark & Sweep**

Go's GC provides excellent inspiration for concurrent collection with minimal pauses.
Key insights from Go's design that apply to our implementation:

### 5.1 Go GC Architecture Overview

**Go's Tri-Color Marking:**
- White: Potentially unreachable (not yet scanned)
- Grey: Reachable but not yet scanned (work queue)
- Black: Reachable and fully scanned
- Sound familiar? We already have this! ✓

**Go's Collection Phases:**
1. **Sweep Termination**: Finish any outstanding sweep work
2. **Mark Phase**: Concurrent marking with write barriers
3. **Mark Termination**: Stop-the-world to complete marking
4. **Sweep Phase**: Concurrent sweeping

**Key Innovations from Go:**
- Write barriers enable concurrent marking
- Pacer controls GC based on heap growth
- Stack scanning during STW windows
- Hybrid barrier (Dijkstra + Yuasa)

### 5.2 Applying Go's Design to Rust

**Phase State Machine:**
```rust
pub enum GcPhase {
    Idle,           // No GC in progress
    Marking,        // Concurrent marking active
    MarkTermination,// Final mark pass (brief pause)
    Sweeping,       // Concurrent sweep
}

pub struct GcState {
    phase: AtomicU8,  // Current GC phase
    work_available: AtomicBool,
    bytes_marked: AtomicUsize,
    bytes_allocated_since_gc: AtomicUsize,
}
```

**Concurrent Marking (inspired by Go):**
```rust
// Mark work can be split across multiple calls
pub fn do_mark_work(&self, work_budget: usize) -> bool {
    let mut work_done = 0;
    
    while work_done < work_budget {
        match self.gray_queue.try_pop() {
            Some(obj) => {
                unsafe { ((*obj).vtable.trace)(obj, &mut self.gray_queue); }
                (*obj).color.store(Color::Black, Ordering::Release);
                work_done += 1;
            }
            None => return true, // Marking complete
        }
    }
    
    false // More work remains
}
```

### 5.3 Write Barriers (Go's Hybrid Barrier Approach)

**Go uses a hybrid barrier combining:**
- **Dijkstra barrier**: shade on pointer write
- **Yuasa barrier**: shade old value before overwrite

**Our Implementation:**
```rust
pub struct WriteBarrier {
    gc_phase: Arc<AtomicU8>,
}

impl WriteBarrier {
    /// Called before updating a pointer field
    pub fn record_write<T>(&self, old_value: Option<&GcPtr<T>>) {
        // Only barrier during marking phase
        if self.gc_phase.load(Ordering::Relaxed) == GcPhase::Marking as u8 {
            // Shade the old value (Yuasa-style)
            if let Some(old) = old_value {
                let header = old.header_ptr();
                unsafe {
                    if (*header).color.load(Ordering::Acquire) == Color::White {
                        (*header).color.store(Color::Gray, Ordering::Release);
                        // Add to gray queue for rescanning
                    }
                }
            }
        }
    }
}
```

### 5.4 Pacer/Trigger Heuristics (Go-style)

**Go's pacer aims to:**
- Finish marking before heap doubles
- Smooth GC CPU usage over time
- Minimize pause times

**Our Implementation:**
```rust
pub struct GcPacer {
    /// Target heap size to trigger GC
    goal: AtomicUsize,
    /// Heap size at last GC
    marked_heap_size: AtomicUsize,
    /// Growth factor before triggering (e.g., 1.0 = 100% growth)
    gc_percent: f64,
}

impl GcPacer {
    pub fn should_trigger_gc(&self, current_heap: usize) -> bool {
        let goal = self.goal.load(Ordering::Relaxed);
        current_heap >= goal
    }
    
    pub fn update_after_gc(&self, marked_bytes: usize) {
        self.marked_heap_size.store(marked_bytes, Ordering::Relaxed);
        // Next GC triggers at marked_size * (1 + gc_percent)
        let goal = marked_bytes + (marked_bytes as f64 * self.gc_percent) as usize;
        self.goal.store(goal, Ordering::Relaxed);
    }
}
```

### 5.5 Implementation Roadmap

**Step 1: Add Phase Tracking**
- [ ] Implement GcPhase enum and state machine
- [ ] Track current phase atomically
- [ ] Update allocation to check phase

**Step 2: Split Mark Phase**
- [ ] Convert mark phase to incremental work units
- [ ] Implement work budget system
- [ ] Add gray queue management for concurrent access

**Step 3: Implement Write Barriers**
- [ ] Add write barrier to GcCell
- [ ] Implement Yuasa-style old value shading
- [ ] Test barrier correctness with concurrent mutations

**Step 4: Concurrent Sweep**
- [ ] Make sweep interruptible
- [ ] Allow allocation during sweep (different color space)
- [ ] Handle sweep/allocate races

**Step 5: Pacer Integration**
- [ ] Implement GcPacer with Go-style heuristics
- [ ] Tune gc_percent parameter (start with 100%)
- [ ] Add metrics for pause times

**Step 6: Background Goroutine (Thread) Support**
- [ ] Dedicated GC thread for background marking
- [ ] Assist mechanism: mutator helps when heap pressure high
- [ ] Coordinate with application threads

### 5.6 Key Differences from Go

**What we can adopt:**
- ✓ Tri-color marking (already have!)
- ✓ Concurrent marking with write barriers
- ✓ Phase-based state machine
- ✓ Pacer heuristics for triggering
- ✓ Incremental work budget system

**What's different in Rust:**
- **No stack scanning**: Rust has no GC pointers on stack (only GcPtr)
- **Simpler roots**: Only GcPtr root_count, not stack roots
- **Type safety**: Trace trait ensures correctness at compile time
- **No finalizers**: Drop is deterministic, not GC-dependent

### 5.7 Performance Goals (Go-inspired)

**Go achieves:**
- Sub-millisecond pause times
- ~25% CPU overhead for GC
- Scales to multi-GB heaps

**Our targets:**
- <1ms pause times for mark termination
- <10% CPU overhead for concurrent marking
- Support heaps up to 1GB efficiently

### 5.8 References & Further Reading

**Go GC Documentation:**
- [Go GC Guide](https://tip.golang.org/doc/gc-guide)
- [Go 1.5 Concurrent GC Design](https://go.dev/blog/go15gc)
- [Getting to Go: The Journey of Go's GC](https://go.dev/blog/ismmkeynote)

**Academic Papers:**
- "On-the-Fly Garbage Collection" (Dijkstra et al., 1978)
- "Real-time Garbage Collection" (Baker, 1978)
- Yuasa's Snapshot-at-Beginning algorithm

**Implementation Insights:**
- Study `runtime/mgc.go` in Go source
- Write barrier implementation in `runtime/mbarrier.go`
- Pacer logic in `runtime/mgcpacer.go`

## Phase 6: Improved Borrowing Model & Write Barriers

### 6.1 Immutable-Only GcPtr
```rust
impl<T> GcPtr<T> {
    pub fn as_ref(&self) -> &T {
        // Only shared references
    }
    
    // No as_mut() or DerefMut
}
```

### 6.2 Interior Mutability with GcCell
```rust
pub struct GcCell<T> {
    value: UnsafeCell<T>,
    barrier: WriteBarrier,
}

impl<T> GcCell<T> {
    pub fn set(&self, value: T) {
        // Write barrier before update
        self.barrier.record_write();
        unsafe { *self.value.get() = value; }
    }
    
    pub fn get(&self) -> T where T: Copy {
        unsafe { *self.value.get() }
    }
}

// Usage
struct Node {
    value: i32,
    next: GcCell<Option<GcPtr<Node>>>,
}
```

### 6.3 Write Barrier Implementation
```rust
struct WriteBarrier {
    // Track writes during marking
    // Add modified objects to gray queue
}
```

## Implementation Order

### Step 1: Refactor Data Structures
1. Create GcHeader with intrusive list
2. Update GcBox to include header
3. Replace Vec allocation list with AtomicPtr
4. Update mark/sweep to use linked list
5. Remove separate mark bit

### Step 2: Add Trace Trait
1. Define Trace and NoTrace traits
2. Implement for primitives and std types
3. Create Tracer API
4. Update mark phase to call trace functions

### Step 3: Incremental Marking
1. Add marking state machine
2. Implement work budgets
3. Add incremental mark API

## Phase 7: Optimization

### 7.1 Benchmark performance
### 7.2 Optimize hot paths  
### 7.3 Tune collection heuristics

## Implementation Status

### ✅ Completed
- **Phase 1:** Core data structures (lock-free list, type-erased headers)
- **Phase 2:** Trace trait system (graph traversal)

### 🔄 Next Priority (Critical)
- **Phase 3:** Allocation safety fixes (race conditions)
- **Phase 4:** VTable + Box allocation (proper Drop semantics)

### 📚 Planned (Go GC-Inspired)
- **Phase 5:** Incremental/concurrent marking
  - Phase state machine (Idle/Marking/Sweeping)
  - Work-based incremental marking
  - Write barriers (hybrid Dijkstra + Yuasa)
  - Pacer/trigger heuristics
  - Background marking thread
- **Phase 6:** Write barriers & GcCell integration
- **Phase 7:** Optimization and tuning

### 📖 Research References Added
- Go GC guide and blog posts
- Academic papers (Dijkstra, Baker, Yuasa)
- Go runtime source code references

## Current Branch Status
- Branch: `feat/improved-gc`
- Commits: 4 (clean history)
- Tests: All passing
- Ready for: Phase 3 (Allocation Safety) + Phase 4 (VTable)
- Research: Go GC design study for Phase 5

## Key Insights from Go GC Design

**What makes Go's GC successful:**
1. **Concurrent marking**: Work happens while app runs
2. **Write barriers**: Maintain invariants during concurrent mutation
3. **Pacer**: Smart triggering based on heap growth
4. **Incremental work**: Bounded pause times
5. **Simplicity**: No generational complexity, predictable behavior

**Direct applicability to our GC:**
- ✅ Already have tri-color marking
- ✅ Can implement similar write barriers in GcCell
- ✅ Pacer logic is straightforward to port
- ✅ Work budget system fits our architecture
- ✅ Simpler than Go (no stack scanning needed!)

**Our advantages in Rust:**
- Type system enforces Trace correctness
- No runtime stack scanning needed
- GcPtr provides clear root set
- Deterministic Drop (not GC-dependent)
- Can leverage Rust's Send/Sync for safe concurrency
