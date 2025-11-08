# Abfall - Concurrent Tri-Color Tracing Garbage Collector

A concurrent mark-and-sweep garbage collector library for Rust using the tri-color marking algorithm.

## Features

- **Tri-Color Marking**: Uses white, gray, and black colors to track object reachability
- **Concurrent Collection**: Background thread performs garbage collection automatically
- **Thread-Safe**: Safe to use across multiple threads
- **Manual Control**: Option to disable automatic collection and trigger manually

## Architecture

### Tri-Color Algorithm

The garbage collector uses a tri-color marking scheme:

- **White**: Potentially unreachable objects (candidates for collection)
- **Gray**: Reachable objects that haven't been scanned yet
- **Black**: Reachable objects that have been fully scanned

### Mark and Sweep Phases

1. **Mark Phase**: Starting from root objects, the GC marks all reachable objects by:
   - Coloring all roots gray
   - Processing gray objects: mark as black and add references to gray queue
   - Continue until no gray objects remain

2. **Sweep Phase**: Reclaim memory from white (unmarked) objects

## Usage

### Basic Example

```rust
use abfall::{GcContext, GcPtr};
use std::sync::Arc;

// Create a new GC context with automatic background collection
let ctx = GcContext::new();

// Allocate objects on the GC heap
let value1 = ctx.allocate(42);
let value2 = ctx.allocate("Hello, GC!");
let value3 = ctx.allocate(vec![1, 2, 3, 4, 5]);

// Access values through smart pointers
println!("Value: {}", *value1);
println!("String: {}", *value2);
println!("Vector: {:?}", *value3);

// When pointers go out of scope, objects become unreachable
// and will be collected in the next GC cycle
```

### Manual Collection

```rust
use abfall::GcContext;
use std::time::Duration;

// Create context without background collection
let ctx = GcContext::with_options(false, Duration::from_millis(100));

let ptr = ctx.allocate(100);
drop(ptr); // Object is now unreachable

// Manually trigger collection
ctx.collect();
```

### Concurrent Usage

```rust
use abfall::GcContext;
use std::sync::Arc;
use std::thread;

let ctx = Arc::new(GcContext::new());
let mut handles = vec![];

for i in 0..10 {
    let ctx_clone = Arc::clone(&ctx);
    let handle = thread::spawn(move || {
        // Allocate from multiple threads
        let ptr = ctx_clone.allocate(i);
        println!("Thread {} allocated: {}", i, *ptr);
    });
    handles.push(handle);
}

for handle in handles {
    handle.join().unwrap();
}
```

## API

### `GcContext`

The main garbage collector context.

- `GcContext::new()` - Create with automatic background collection
- `GcContext::with_options(background: bool, interval: Duration)` - Create with custom options
- `allocate<T>(data: T) -> GcPtr<T>` - Allocate an object on the GC heap
- `collect()` - Manually trigger a collection cycle
- `bytes_allocated() -> usize` - Get current heap size
- `allocation_count() -> usize` - Get number of live allocations

### `GcPtr<T>`

Smart pointer to GC-managed memory.

- Implements `Deref` for transparent access
- Implements `Clone` - cloned pointers keep the object alive
- Implements `Send + Sync` (when T implements them)
- Automatically manages root set membership

## Implementation Details

### Memory Safety

- Uses `NonNull` pointers internally for safe null checks
- Atomic operations for thread-safe color updates
- Mutex-protected allocation tracking

### Root Set Management

Objects are considered roots when:
- At least one `GcPtr` points to them
- When all `GcPtr`s are dropped, the object leaves the root set
- Objects outside the root set become collection candidates

### Concurrent Collection

- Background thread wakes periodically to check if collection is needed
- Collection is triggered when heap size exceeds threshold
- Uses atomic flags to prevent concurrent collections
- Read barriers ensure consistency during concurrent marking

## Performance Characteristics

- **Allocation**: O(1) - simple bump allocation with lock
- **Collection Time**: O(n) where n is the number of live objects
- **Space Overhead**: Additional metadata per object (color, mark bit, root flag)
- **Pause Time**: Concurrent collection minimizes application pauses

## Limitations

- Currently doesn't support cyclic garbage (objects with circular references)
- No generational collection optimization
- Fixed collection threshold (1MB by default)
- Background thread cannot be gracefully stopped

## Future Improvements

- Generational collection for improved performance
- Incremental marking to reduce pause times
- Configurable collection thresholds
- Cycle detection for reference cycles
- Write barriers for more sophisticated concurrent collection

## Safety

This library uses `unsafe` code for manual memory management. The safety invariants are:

1. All allocated objects remain valid until explicitly deallocated
2. Objects in the root set are never collected
3. Atomic operations ensure thread-safe color transitions
4. Mutex protects allocation list during concurrent access

## License

This is a demonstration project for educational purposes.
