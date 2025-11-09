//! Basic demo of the Abfall garbage collector
//!
//! This example demonstrates:
//! - Basic allocation of GC objects
//! - Manual garbage collection
//! - How GC roots keep objects alive
//! - Memory pressure and collection behavior

use abfall::GcContext;

fn main() {
    println!("=== Abfall Garbage Collector Demo ===\n");

    // Example 1: Basic allocation and usage
    println!("Example 1: Basic Allocation");
    basic_allocation();
    println!();

    // Example 2: Manual collection
    println!("Example 2: Manual Collection");
    manual_collection();
    println!();

    // Example 3: Memory pressure
    println!("Example 3: Memory Pressure and Collection");
    memory_pressure();
    println!();
}

fn basic_allocation() {
    let ctx = GcContext::new();

    let number = ctx.allocate(42);
    let text = ctx.allocate("Hello, World!");
    let vector = ctx.allocate(vec![1, 2, 3, 4, 5]);

    println!("  Number: {}", *number);
    println!("  Text: {}", *text);
    println!("  Vector: {:?}", *vector);
    println!("  Allocations: {}", ctx.heap().allocation_count());
}

fn manual_collection() {
    let ctx = GcContext::new();

    println!("  Allocating 5 objects...");
    let ptr1 = ctx.allocate(1);
    let _ptr2 = ctx.allocate(2);
    let _ptr3 = ctx.allocate(3);
    let _ptr4 = ctx.allocate(4);
    let _ptr5 = ctx.allocate(5);

    println!(
        "  Before collection: {} allocations, {} bytes",
        ctx.heap().allocation_count(),
        ctx.heap().bytes_allocated()
    );

    // Drop some pointers
    drop(_ptr2);
    drop(_ptr3);
    drop(_ptr4);
    drop(_ptr5);

    println!(
        "  After drops (before GC): {} allocations",
        ctx.heap().allocation_count()
    );

    // Manually trigger collection
    ctx.collect();

    println!(
        "  After collection: {} allocations, {} bytes",
        ctx.heap().allocation_count(),
        ctx.heap().bytes_allocated()
    );
    println!("  ptr1 still alive: {}", *ptr1);
}

fn memory_pressure() {
    let ctx = GcContext::new();

    println!("  Allocating many objects...");
    let mut live_objects = Vec::new();

    // Allocate 1000 objects
    for i in 0..1000 {
        let ptr = ctx.allocate(vec![i; 100]); // Each allocation is substantial
        if i % 100 == 0 {
            live_objects.push(ptr); // Keep some alive
        }
    }

    println!(
        "  Before collection: {} allocations, {} bytes",
        ctx.heap().allocation_count(),
        ctx.heap().bytes_allocated()
    );

    // Force collection
    ctx.collect();

    println!(
        "  After collection: {} allocations, {} bytes",
        ctx.heap().allocation_count(),
        ctx.heap().bytes_allocated()
    );
    println!("  Live objects kept: {}", live_objects.len());
}
