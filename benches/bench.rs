use arc_swap::ArcSwap;
use arcshift::ArcShift;
use criterion::{Criterion, criterion_group, criterion_main};
use lock_free_cell::{LockFreeCell, SpinCell};
use std::sync::{Arc, Mutex, RwLock};
use std::{hint::black_box, ops::Deref};

fn mutex_bench(c: &mut Criterion) {
    let ac = Mutex::new(42u32);
    c.bench_function("mutex", |b| b.iter(|| ac.lock().unwrap()));
}
fn mutex_contended_bench(c: &mut Criterion) {
    let ac = Arc::new(Mutex::new(42u32));
    let ac_clone = ac.clone();
    std::thread::spawn(move || {
        loop {
            drop(black_box(ac_clone.lock()));
        }
    });
    c.bench_function("mutex_contended", |b| b.iter(|| ac.lock().unwrap()));
}

fn rwlock_contended_read_bench(c: &mut Criterion) {
    let ac = Arc::new(RwLock::new(42u32));
    let ac_clone = ac.clone();
    std::thread::spawn(move || {
        loop {
            drop(black_box(ac_clone.read()));
        }
    });
    c.bench_function("rwlock_contended_read", |b| {
        b.iter(|| {
            let guard = ac.read().unwrap();
            *guard
        })
    });
}

fn rwlock_write_bench(c: &mut Criterion) {
    let ac = RwLock::new(42u32);
    c.bench_function("rwlock_write", |b| {
        b.iter(|| {
            let mut guard = ac.write().unwrap();
            *guard = 43;
        })
    });
}

fn my_spincell_bench(c: &mut Criterion) {
    let ac = SpinCell::new(42u32);
    c.bench_function("my_spincell_get", |b| b.iter(|| ac.read(|x| *x)));
}
fn my_lockfreecell_bench(c: &mut Criterion) {
    let ac: LockFreeCell<u32> = LockFreeCell::new(42u32);
    c.bench_function("my_lockfreecell_get", |b| {
        b.iter(|| black_box(ac.read(|x| *x)))
    });
}

fn my_spincell_contended_bench(c: &mut Criterion) {
    let ac = Arc::new(SpinCell::new(42u32));
    let ac_clone = ac.clone();
    std::thread::spawn(move || {
        loop {
            black_box(ac_clone.read(|x| *x));
        }
    });
    c.bench_function("my_spincell_contended_get", |b| b.iter(|| ac.read(|x| *x)));
}
fn my_lockfreecell_contended_bench(c: &mut Criterion) {
    let ac = Arc::new(LockFreeCell::new(42u32));
    let ac_clone = ac.clone();
    std::thread::spawn(move || {
        loop {
            black_box(ac_clone.read(|x| *x));
        }
    });
    c.bench_function("my_lockfreecell_contended_get", |b| {
        b.iter(|| black_box(ac.read(|x| *x)))
    });
}
fn arcshift_update_bench(c: &mut Criterion) {
    let mut ac = ArcShift::new(42u32);
    c.bench_function("arcshift_update", |b| {
        b.iter(|| {
            ac.rcu(|x| *x + 43);
        })
    });
}

fn arcshift_contended_bench(c: &mut Criterion) {
    let mut ac_base = ArcShift::new(42u32);
    let ac = ac_base.clone();
    ac_base.update(43);
    std::thread::spawn(move || {
        loop {
            black_box(ac_base.shared_get());
        }
    });
    c.bench_function("arcshift_contended_get", |b| b.iter(|| *ac.shared_get()));
}

fn arcswap_contended_get(c: &mut Criterion) {
    let shared = Arc::new(ArcSwap::from_pointee(42));
    let shared2 = shared.clone();

    std::thread::spawn(move || {
        shared2.store(Arc::new(43));
        loop {
            black_box(*(*shared2.load()).deref());
        }
    });

    c.bench_function("arc_swap_contended_get", |b| {
        b.iter(|| {
            let arc = shared.load();
            let x: i32 = *(*arc).deref();
            black_box(x)
        })
    });
}

fn arcswap_update(c: &mut Criterion) {
    let ac = ArcSwap::from_pointee(42u32);
    c.bench_function("arc_swap_update", |b| {
        b.iter(|| black_box(ac.rcu(|x| **x + 43)))
    });
}
fn my_spincell_update(c: &mut Criterion) {
    let ac = SpinCell::new(42u32);
    c.bench_function("my_spincell_update", |b| {
        b.iter(|| ac.write_discard(|x| *x = 43))
    });
}
fn my_lockfreecell_update(c: &mut Criterion) {
    let ac: LockFreeCell<u32> = LockFreeCell::new(42u32);
    c.bench_function("my_lockfreecell_update", |b| {
        b.iter(|| ac.write_discard(|x| x + 43))
    });
}

criterion_group!(
    benches,
    my_lockfreecell_contended_bench,
    my_lockfreecell_update,
    my_lockfreecell_bench,
    mutex_bench,
    mutex_contended_bench,
    rwlock_write_bench,
    rwlock_contended_read_bench,
    arcswap_update,
    arcswap_contended_get,
    arcshift_update_bench,
    arcshift_contended_bench,
    my_spincell_update,
    my_spincell_bench,
    my_spincell_contended_bench,
);

criterion_main!(benches);
