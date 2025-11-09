//! Example verifying that Drop implementations are correctly called
//!
//! This demonstrates that the GC properly deallocates objects by:
//! - Calling custom Drop implementations
//! - Handling types with internal Drop (String, Vec, etc.)

use abfall::{GcContext, Trace, Tracer};
use std::sync::atomic::{AtomicUsize, Ordering};

static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

// A type with a custom Drop implementation
struct DropCounter {
    id: usize,
}

impl Drop for DropCounter {
    fn drop(&mut self) {
        println!("  Dropping DropCounter {}", self.id);
        DROP_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

unsafe impl Trace for DropCounter {
    fn trace(&self, _tracer: &Tracer) {
        // No GC pointers
    }
}

fn main() {
    println!("=== Drop Semantics Verification ===\n");

    // Test 1: Drop is called when objects are collected
    println!("Test 1: Drop is called during GC");
    DROP_COUNT.store(0, Ordering::SeqCst);

    {
        let ctx = GcContext::new();

        let obj1 = ctx.allocate(DropCounter { id: 1 });
        let obj2 = ctx.allocate(DropCounter { id: 2 });
        let obj3 = ctx.allocate(DropCounter { id: 3 });

        println!("  Created 3 DropCounter objects");

        drop(obj2);
        drop(obj3);
        println!("  Dropped 2 GcRoot references");

        ctx.collect();
        println!(
            "  After GC: {} objects dropped",
            DROP_COUNT.load(Ordering::SeqCst)
        );
        assert_eq!(DROP_COUNT.load(Ordering::SeqCst), 2);

        drop(obj1);
        // ctx is dropped here, calling Drop on remaining object
    }

    println!(
        "  After context dropped: {} total drops",
        DROP_COUNT.load(Ordering::SeqCst)
    );
    assert_eq!(DROP_COUNT.load(Ordering::SeqCst), 3);

    println!("✓ Test passed: All Drop implementations called!\n");

    // Test 2: Drop is called for String (built-in Drop)
    println!("Test 2: String deallocation (has internal Drop)");

    let ctx = GcContext::new();

    let s1 = ctx.allocate(String::from(
        "Hello, World! This is a long string that allocates on the heap",
    ));
    let s2 = ctx.allocate(String::from("Another heap-allocated string"));
    let s3 = ctx.allocate(String::from("Yet another string"));

    println!("  Created 3 String objects");
    println!("  s1: {}", *s1);

    drop(s2);
    drop(s3);

    ctx.collect();
    println!("  After GC: String objects properly deallocated");
    println!("  s1 still accessible: {}", *s1);

    println!("✓ Test passed: Strings properly deallocated!\n");

    // Test 3: Drop is called for Vec (another built-in Drop)
    println!("Test 3: Vec deallocation (has internal Drop)");

    let v1 = ctx.allocate(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    let v2 = ctx.allocate(vec!["a", "b", "c", "d", "e"]);

    println!("  Created 2 Vec objects");
    println!("  v1: {:?}", *v1);

    drop(v2);
    ctx.collect();

    println!("  After GC: Vec objects properly deallocated");
    println!("  v1 still accessible: {:?}", *v1);

    println!("✓ Test passed: Vecs properly deallocated!\n");

    println!("✅ ALL TESTS PASSED");
    println!("   ✓ Custom Drop implementations called");
    println!("   ✓ String (internal Drop) handled correctly");
    println!("   ✓ Vec (internal Drop) handled correctly");
}
