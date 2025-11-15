use std::sync::Arc;
use std::thread;
use std::time::Duration;

use abfall::{GcCell, GcContext, GcPtr, GcRoot, Trace, Tracer};

// Simple acyclic node type for graph tracing tests
struct Node {
    value: usize,
    next: Option<GcPtr<Node>>,
}

unsafe impl Trace for Node {
    fn trace(&self, tracer: &Tracer) {
        if let Some(n) = &self.next {
            tracer.mark(n);
        }
    }
}

#[test]
fn root_preservation_after_collection() {
    let ctx = GcContext::new();
    let keep = ctx.allocate(1234);
    for _ in 0..1000 {
        let _tmp = ctx.allocate(0usize);
    } // many temporaries
    ctx.heap().force_collect();
    assert_eq!(*keep, 1234);
}

#[test]
fn sweep_frees_memory() {
    let ctx = GcContext::new();
    let roots: Vec<_> = (0..10).map(|i| ctx.allocate(i)).collect();
    for _ in 0..500 {
        let _t = ctx.allocate(vec![0u8; 256]);
    } // allocate ~128KB
    let peak = ctx.heap().bytes_allocated();
    drop(roots); // drop roots so they become collectable
    ctx.heap().force_collect();
    let after = ctx.heap().bytes_allocated();
    assert!(
        after < peak / 2,
        "expected significant reclaim: after={}, peak={}",
        after,
        peak
    );
}

#[test]
fn threshold_triggers_collection() {
    use abfall::GcOptions;
    let mut opts = GcOptions::DEFAULT; // very low threshold to force collection quickly
    opts.min_threshold_bytes = 4 * 1024; // 4KB
    opts.threshold_percent = 10;
    let ctx = GcContext::with_options(opts);
    for _ in 0..200 {
        let _t = ctx.allocate([0u8; 64]);
    }
    // Background thread should run soon; wait a bit
    thread::sleep(Duration::from_millis(200));
    let after_bg = ctx.heap().bytes_allocated();
    ctx.heap().force_collect();
    let after_force = ctx.heap().bytes_allocated();
    assert!(
        after_force <= after_bg,
        "force_collect should not increase usage"
    );
}

#[test]
fn tracing_chain_keeps_all_nodes() {
    let ctx = GcContext::new();
    let mut prev: Option<GcRoot<Node>> = None;
    let head = ctx.allocate(Node {
        value: 0,
        next: None,
    });
    prev = Some(head.clone());
    for i in 1..100 {
        let n = ctx.allocate(Node {
            value: i,
            next: prev.map(|p| p.as_ptr()),
        });
        prev = Some(n);
    }
    ctx.heap().force_collect(); // all nodes reachable from last (prev)
    // Walk chain
    let mut count = 0;
    let mut cur = prev.unwrap();
    loop {
        count += 1;
        if let Some(next_ptr) = cur.next {
            // cur derefs to &Node via GcRoot
            cur = unsafe { next_ptr.root() }; // root next
        } else {
            break;
        }
    }
    assert_eq!(count, 100);
}

#[test]
fn concurrent_alloc_and_collect_no_race() {
    let ctx = GcContext::new();
    let heap = Arc::clone(ctx.heap());
    let mut handles = Vec::new();
    for _ in 0..4 {
        let heap_cl = Arc::clone(&heap);
        handles.push(thread::spawn(move || {
            let ctx = GcContext::with_heap(heap_cl);
            for i in 0..10_000 {
                let r = ctx.allocate(i);
                if i % 50 == 0 {
                    ctx.heap().collect();
                }
                if *r != i {
                    panic!("value corruption");
                }
            }
        }));
    }
    // Simultaneously trigger collections
    for _ in 0..50 {
        heap.collect();
        thread::yield_now();
    }
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn write_barrier_concurrent_mutation() {
    let ctx = GcContext::new();
    // Create many unrooted values
    let values: Vec<_> = (0..1000).map(|i| ctx.allocate(i).as_ptr()).collect();
    let cell_root = ctx.allocate(GcCell::new(values[0]));
    // Attempt to start a collection quickly by exceeding threshold
    for _ in 0..2000 {
        let _ = ctx.allocate([0u8; 16]);
    }
    // Background GC may be marking now; mutate concurrently
    let heap = Arc::clone(ctx.heap());
    let mut handles = Vec::new();
    for _ in 0..4 {
        let heap_cl = Arc::clone(&heap);
        let cell_ptr = cell_root.clone();
        let values_cl = values.clone();
        handles.push(thread::spawn(move || {
            let ctx = GcContext::with_heap(heap_cl);
            for v in values_cl {
                cell_ptr.set(v);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    // Force completion
    ctx.heap().force_collect();
    // The final pointer stored should still be valid
    let last = unsafe { cell_root.get().root() };
    assert_eq!(*last, 999);
}

#[test]
fn large_object_graph_survives_multiple_cycles() {
    let ctx = GcContext::new();
    let roots: Vec<_> = (0..100).map(|i| ctx.allocate(vec![i; 32])).collect();
    for cycle in 0..5 {
        for _ in 0..500 {
            let _t = ctx.allocate([0u8; 64]);
        }
        ctx.heap().force_collect();
        for (i, r) in roots.iter().enumerate() {
            assert_eq!(r[0], i);
        }
        println!("cycle {} ok", cycle);
    }
}
