# Garbage Collector Improvement Plan

## Goals
1. ✅ Eliminate separate mark bit - use color atomically
2. ✅ Reduce lock contention
3. ✅ Use intrusive linked list for allocations
4. ✅ Add Trace trait for object graph traversal
5. Implement incremental tracing
6. Refine borrowing model (Rc/Arc-like)
7. Add GcCell for write barriers
8. **NEW:** Ensure allocation safety (objects rooted until linked)

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

## Phase 4: Incremental Marking

### 4.1 Work-Based Incremental Marking
- Track marking progress
- Limit work per increment (e.g., 100 objects)
- Interleave with mutator (application code)

### 4.2 Snapshot-At-Beginning (SATB)
- Record pointer updates during marking
- Use write barrier to maintain snapshot
- Ensures no objects are lost during concurrent marking

## Phase 5: Improved Borrowing Model & Write Barriers

### 5.1 Immutable-Only GcPtr
```rust
impl<T> GcPtr<T> {
    pub fn as_ref(&self) -> &T {
        // Only shared references
    }
    
    // No as_mut() or DerefMut
}
```

### 5.2 Interior Mutability with GcCell
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

### 5.3 Write Barrier Implementation
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

## Phase 6: Optimization

### 6.1 Benchmark performance
### 6.2 Optimize hot paths
### 6.3 Tune collection heuristics

## Implementation Status

### ✅ Completed
- **Step 1:** Core data structures (lock-free list, type-erased headers)
- **Step 2:** Trace trait system (graph traversal)

### 🔄 In Progress / Next
- **Step 3:** Allocation safety fixes (CRITICAL - addresses race conditions)
- **Step 4:** Incremental marking
- **Step 5:** Write barriers & GcCell
- **Step 6:** Optimization pass

## Current Branch Status
- Branch: `feat/improved-gc`
- Commits: 2 (clean history)
- Tests: All passing
- Ready for: Phase 3 (Allocation Safety)
