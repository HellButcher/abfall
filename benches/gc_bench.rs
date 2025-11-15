use std::sync::Arc;
use std::thread;
use criterion::{criterion_group, criterion_main, Criterion, BatchSize};
use abfall::{GcContext, Heap, Trace, Tracer, GcPtr};

struct Node { value: usize, next: Option<GcPtr<Node>> }
unsafe impl Trace for Node { fn trace(&self, t: &Tracer) { if let Some(n)=&self.next { t.mark(n); } } }

fn bench_allocation(c: &mut Criterion) {
    c.bench_function("alloc_100k_ints", |b| {
        b.iter(|| {
            let ctx = GcContext::new();
            for i in 0..100_000 { let _ = ctx.allocate(i); }
            ctx.heap().force_collect();
        });
    });
}

fn bench_chain(c: &mut Criterion) {
    c.bench_function("alloc_trace_chain_10k", |b| {
        b.iter(|| {
            let ctx = GcContext::new();
            let mut prev = None;
            for i in 0..10_000 { let n = ctx.allocate(Node{ value:i, next:prev }); prev = Some(n.as_ptr()); }
            ctx.heap().force_collect();
        });
    });
}

fn bench_concurrent_alloc(c: &mut Criterion) {
    c.bench_function("concurrent_alloc", |b| {
        b.iter_batched(|| Heap::new(), |heap| {
            let threads: Vec<_> = (0..4).map(|t| {
                let heap_cl = Arc::clone(&heap);
                thread::spawn(move || {
                    let worker_ctx = GcContext::with_heap(heap_cl);
                    for i in 0..25_000 { let _ = worker_ctx.allocate(((t as u64)<<32 | i as u64) as u64); if i % 500 == 0 { worker_ctx.heap().collect(); } }
                })
            }).collect();
            for h in threads { h.join().unwrap(); }
            heap.force_collect();
        }, BatchSize::SmallInput);
    });
}

criterion_group!(gc, bench_allocation, bench_chain, bench_concurrent_alloc);
criterion_main!(gc);
