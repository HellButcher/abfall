use criterion::{criterion_group, criterion_main, Criterion};
use std::time::Duration;
use abfall::GcContext;

// Baseline: Abfall GC allocation + collection
fn bench_abfall_alloc(c: &mut Criterion) {
    c.bench_function("abfall_alloc_collect_50k", |b| {
        b.iter(|| {
            let ctx = GcContext::new();
            for i in 0..50_000 { let _ = ctx.allocate(i); }
            ctx.heap().force_collect();
        });
    });
}

// Optional: Dumpster GC comparison (enable with --features compare_dumpster)
// NOTE: Add `dumpster` as dev-dependency and enable feature `compare_dumpster` to activate.
#[cfg(feature = "compare_dumpster")]
mod dumpster_cmp {
    use super::*;
    use dumpster::sync::{Gc, collect};
    #[derive(dumpster_derive::Trace)]
    struct IntHolder(i32);
    pub fn bench_dumpster_alloc(c: &mut Criterion) {
        c.bench_function("dumpster_alloc_collect_50k", |b| {
            b.iter(|| {
                let mut vec = Vec::with_capacity(50_000);
                for i in 0..50_000 { vec.push(Gc::new(IntHolder(i))); }
                drop(vec); // make unreachable
                collect();
            });
        });
    }
}

// Optional: cppgc via v8 crate (enable with --features compare_cppgc)
#[cfg(feature = "compare_cppgc")]
mod cppgc_cmp {
    use super::*;
    use std::sync::Once;
    use v8::{self, HandleScope, Isolate, CreateParams, Context, Local};

    static V8_INIT: Once = Once::new();
    fn init_v8() {
        V8_INIT.call_once(|| {
            let platform = v8::new_default_platform(0, false).make_shared();
            v8::V8::initialize_platform(platform);
            v8::V8::initialize();
        });
    }

    pub fn bench_cppgc_alloc(c: &mut Criterion) {
        c.bench_function("cppgc_alloc_collect_50k", |b| {
            b.iter(|| {
                init_v8();
                let mut isolate = Isolate::new(CreateParams::default());
                {
                    let mut scope = HandleScope::new(&mut isolate);
                    let context = Context::new(&mut scope);
                    let mut cs = v8::ContextScope::new(&mut scope, context);
                    for i in 0..50_000 {
                        let key = v8::String::new(&mut cs, "x").unwrap();
                        let val = v8::Integer::new(&mut cs, i);
                        let obj = v8::Object::new(&mut cs);
                        let _ = obj.set(&mut cs, key.into(), val.into());
                    }
                }
                isolate.low_memory_notification(); // GC hint only; request_garbage_collection_for_testing requires expose-gc flag
            });
        });
    }
}

fn bench_compare(c: &mut Criterion) {
    bench_abfall_alloc(c);
    #[cfg(feature = "compare_dumpster")]
    dumpster_cmp::bench_dumpster_alloc(c);
    #[cfg(feature = "compare_cppgc")]
    cppgc_cmp::bench_cppgc_alloc(c);
}

criterion_group!{
    name = compare;
    config = Criterion::default().measurement_time(Duration::from_secs(5));
    targets = bench_compare
}
criterion_main!(compare);
