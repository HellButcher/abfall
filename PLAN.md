# Garbage Collector Improvement Plan

## Goals
1. Eliminate separate mark bit - use color atomically
2. Reduce lock contention
3. Use intrusive linked list for allocations
4. Add Trace trait for object graph traversal
5. Implement incremental tracing
6. Refine borrowing model (Rc/Arc-like)
7. Add GcCell for write barriers

## Phase 1: Core Data Structure Improvements

### 1.1 Unified Color-Mark System
- Remove separate `marked` AtomicBool from GcBox
- Use Color directly: White = unmarked, Black = marked
- Gray only used during active marking phase
- Atomic color transitions handle synchronization

### 1.2 Type-Erased Header + Intrusive Linked List
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

### 1.3 Lock-Free Allocation List
- Replace `Mutex<Vec<usize>>` with `AtomicPtr<GcHeader>`
- Use atomic operations for list insertion
- Mark phase traverses linked list without locks

## Phase 2: Trace Trait System

### 2.1 Trace Trait Definition
```rust
pub unsafe trait Trace {
    fn trace(&self, tracer: &mut Tracer);
}

// Marker for types with no GC pointers
pub unsafe trait NoTrace: Trace {}

// Auto-implement for primitives
unsafe impl NoTrace for i32 {}
unsafe impl NoTrace for String {}
// etc.

// Blanket impl for NoTrace types
unsafe impl<T: NoTrace> Trace for T {
    fn trace(&self, _tracer: &mut Tracer) {
        // Nothing to trace
    }
}
```

### 2.2 Tracer API
```rust
pub struct Tracer {
    gray_queue: Vec<*const GcHeader>,
}

impl Tracer {
    pub fn mark<T: Trace>(&mut self, ptr: &GcPtr<T>) {
        // Add to gray queue
    }
}
```

### 2.3 User-Defined Tracing
```rust
struct Node {
    value: i32,
    next: Option<GcPtr<Node>>,
}

unsafe impl Trace for Node {
    fn trace(&self, tracer: &mut Tracer) {
        if let Some(ref next) = self.next {
            tracer.mark(next);
        }
    }
}
```

## Phase 3: Incremental Marking

### 3.1 Work-Based Incremental Marking
- Track marking progress
- Limit work per increment (e.g., 100 objects)
- Interleave with mutator (application code)

### 3.2 Snapshot-At-Beginning (SATB)
- Record pointer updates during marking
- Use write barrier to maintain snapshot
- Ensures no objects are lost during concurrent marking

## Phase 4: Improved Borrowing Model

### 4.1 Immutable-Only GcPtr
```rust
impl<T> GcPtr<T> {
    pub fn as_ref(&self) -> &T {
        // Only shared references
    }
    
    // No as_mut() or DerefMut
}
```

### 4.2 Interior Mutability with GcCell
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

### 4.3 Write Barrier Implementation
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

### Step 4: Write Barriers & GcCell
1. Implement GcCell
2. Add write barrier tracking
3. Update marking to check barrier log
4. Remove DerefMut from GcPtr

### Step 5: Optimization
1. Benchmark performance
2. Optimize hot paths
3. Tune collection heuristics

## Next Actions
1. Create feature branch
2. Implement Step 1 (data structures)
3. Test thoroughly
4. Iterate on remaining steps
