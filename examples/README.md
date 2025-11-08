# Abfall Examples

This directory contains examples demonstrating various features of the Abfall garbage collector.

## Running Examples

Run any example with:
```bash
cargo run --example <name>
```

## Available Examples

### `demo.rs` - Basic Usage
A simple introduction to the Abfall GC showing:
- Basic allocation of GC objects
- How GC roots keep objects alive
- Manual garbage collection
- Memory pressure and automatic collection

**Run:** `cargo run --example demo`

### `trace_demo.rs` - Trace Trait
Demonstrates the `Trace` trait for tracking object references:
- Implementing `Trace` for custom types (linked list)
- How reachability preserves objects during collection
- How unreachable objects are collected

**Run:** `cargo run --example trace_demo`

### `vtable_drop_test.rs` - Drop Semantics
Verifies that Drop implementations are correctly called:
- Custom Drop implementations
- Types with internal Drop (String, Vec, etc.)
- Proper deallocation of heap memory

**Run:** `cargo run --example vtable_drop_test`

### `multi_threaded.rs` - Concurrent GC
Advanced example showing concurrent access to a shared GC heap:
- Multiple threads sharing the same GC heap
- Cross-thread references via `GcPtr` and `GcCell`
- Concurrent allocation and mutation
- Incremental garbage collection running concurrently
- Thread-local `GcContext` with shared heap

**Run:** `cargo run --example multi_threaded`

## Example Progression

We recommend exploring the examples in this order:

1. **demo.rs** - Start here to understand basic concepts
2. **trace_demo.rs** - Learn how to implement the Trace trait
3. **vtable_drop_test.rs** - Understand Drop semantics and verification
4. **multi_threaded.rs** - Explore advanced concurrent usage

## Key Concepts Demonstrated

- **GcContext**: Thread-local context for GC operations
- **GcRoot**: Rooted pointer that keeps objects alive
- **GcPtr**: Unrooted pointer for storage in GC objects
- **GcCell**: Interior mutability with write barrier support
- **Trace**: Trait for telling the GC how to follow references
- **Concurrent Collection**: Tri-color marking with incremental collection
