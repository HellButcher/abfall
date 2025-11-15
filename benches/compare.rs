use abfall::GcContext;
use criterion::{Criterion, criterion_group, criterion_main};
use std::time::Duration;

// Baseline: Abfall GC allocation + collection
fn bench_abfall_alloc(c: &mut Criterion) {
    c.bench_function("abfall_alloc_collect_50k", |b| {
        b.iter(|| {
            let ctx = GcContext::new();
            for i in 0..50_000 {
                let _ = ctx.allocate(i);
            }
            ctx.heap().force_collect();
        });
    });
}

mod dumpster_cmp {
    use super::*;
    use dumpster::sync::{Gc, collect};
    #[derive(dumpster_derive::Trace)]
    struct IntHolder(i32);
    pub fn bench_dumpster_alloc(c: &mut Criterion) {
        c.bench_function("dumpster_alloc_collect_50k", |b| {
            b.iter(|| {
                let mut vec = Vec::with_capacity(50_000);
                for i in 0..50_000 {
                    vec.push(Gc::new(IntHolder(i)));
                }
                drop(vec); // make unreachable
                collect();
            });
        });
    }
}

fn bench_compare(c: &mut Criterion) {
    bench_abfall_alloc(c);
    dumpster_cmp::bench_dumpster_alloc(c);
}

criterion_group! {
    name = compare;
    config = Criterion::default().measurement_time(Duration::from_secs(5));
    targets = bench_compare
}
criterion_main!(compare);
