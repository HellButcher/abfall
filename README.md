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

## License

[license]: #license

This project is licensed under either of

* MIT license ([LICENSE-MIT] or <http://opensource.org/licenses/MIT>)
* Apache License, Version 2.0, ([LICENSE-APACHE] or <http://www.apache.org/licenses/LICENSE-2.0>)

at your option

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

[LICENSE-MIT]: LICENSE-MIT
[LICENSE-APACHE]: LICENSE-APACHE

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the project by you shall be dual licensed as above,
without additional terms or conditions.

