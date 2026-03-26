use arc_swap::ArcSwap;
use arcshift::ArcShift;
use criterion::{Criterion, criterion_group, criterion_main};
use hazarc::{AtomicArc, Cache};
use lock_free_cell::{LockFreeCell, SpinCell};
use std::hint::{self, black_box};
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

// ============================================================================
// Contention helpers (adapted from hazarc benchmarks)
// ============================================================================

/// Benchmarks `read_op` while a background thread continuously writes via `write_op`.
/// Uses scoped threads and proper synchronization for accurate results.
fn bench_read_while_writing<S, W, R, O>(
    c: &mut Criterion,
    name: &str,
    shared: &S,
    write_op: W,
    read_op: R,
) where
    S: Sync,
    W: Fn(&S) + Send + Sync,
    R: Fn(&S) -> O,
{
    let started = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    thread::scope(|s| {
        s.spawn(|| {
            started.store(true, Relaxed);
            while !stop.load(Relaxed) {
                write_op(shared);
                for _ in 0..8 {
                    hint::spin_loop();
                }
            }
        });
        while !started.load(Relaxed) {
            hint::spin_loop();
        }
        c.bench_function(name, |b| b.iter(|| read_op(shared)));
        stop.store(true, Relaxed);
    });
}

/// Benchmarks `write_op` while `n_readers` threads continuously read via `read_op`.
/// Measures write latency under read contention.
fn bench_write_while_reading<S, W, R>(
    c: &mut Criterion,
    name: &str,
    shared: &S,
    n_readers: usize,
    read_op: R,
    write_op: W,
) where
    S: Sync,
    R: Fn(&S) + Send + Sync,
    W: Fn(&S),
{
    let started = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    thread::scope(|s| {
        for _ in 0..n_readers {
            s.spawn(|| {
                started.fetch_add(1, Relaxed);
                while !stop.load(Relaxed) {
                    read_op(shared);
                    for _ in 0..8 {
                        hint::spin_loop();
                    }
                }
            });
        }
        while started.load(Relaxed) != n_readers {
            hint::spin_loop();
        }
        c.bench_function(name, |b| b.iter(|| write_op(shared)));
        stop.store(true, Relaxed);
    });
}

/// Benchmarks `read_op` while a background thread also reads (read-read contention).
fn bench_read_contended<S, R, O>(c: &mut Criterion, name: &str, shared: &S, read_op: R)
where
    S: Sync,
    R: Fn(&S) -> O + Sync,
{
    let started = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    thread::scope(|s| {
        s.spawn(|| {
            started.store(true, Relaxed);
            while !stop.load(Relaxed) {
                black_box(read_op(shared));
            }
        });
        while !started.load(Relaxed) {
            hint::spin_loop();
        }
        c.bench_function(name, |b| b.iter(|| read_op(shared)));
        stop.store(true, Relaxed);
    });
}

// ============================================================================
// Mutex
// ============================================================================

fn mutex_read(c: &mut Criterion) {
    let m = Mutex::new(42u32);
    c.bench_function("mutex_read", |b| b.iter(|| *m.lock().unwrap()));
}

fn mutex_read_contended(c: &mut Criterion) {
    let m = Mutex::new(42u32);
    bench_read_contended(c, "mutex_read_contended", &m, |m| *m.lock().unwrap());
}

fn mutex_write(c: &mut Criterion) {
    let m = Mutex::new(42u32);
    c.bench_function("mutex_write", |b| b.iter(|| *m.lock().unwrap() = 43));
}

// ============================================================================
// RwLock
// ============================================================================

fn rwlock_read(c: &mut Criterion) {
    let rw = RwLock::new(42u32);
    c.bench_function("rwlock_read", |b| b.iter(|| *rw.read().unwrap()));
}

fn rwlock_read_contended(c: &mut Criterion) {
    let rw = RwLock::new(42u32);
    bench_read_contended(c, "rwlock_read_contended", &rw, |rw| *rw.read().unwrap());
}

fn rwlock_read_while_writing(c: &mut Criterion) {
    let rw = RwLock::new(42u32);
    bench_read_while_writing(
        c,
        "rwlock_read_while_writing",
        &rw,
        |rw| *rw.write().unwrap() = 43,
        |rw| *rw.read().unwrap(),
    );
}

fn rwlock_write(c: &mut Criterion) {
    let rw = RwLock::new(42u32);
    c.bench_function("rwlock_write", |b| b.iter(|| *rw.write().unwrap() = 43));
}

fn rwlock_write_contended_4r(c: &mut Criterion) {
    let rw = RwLock::new(42u32);
    bench_write_while_reading(
        c,
        "rwlock_write_contended_4r",
        &rw,
        4,
        |rw| { black_box(*rw.read().unwrap()); },
        |rw| *rw.write().unwrap() = 43,
    );
}

fn rwlock_write_contended_8r(c: &mut Criterion) {
    let rw = RwLock::new(42u32);
    bench_write_while_reading(
        c,
        "rwlock_write_contended_8r",
        &rw,
        8,
        |rw| { black_box(*rw.read().unwrap()); },
        |rw| *rw.write().unwrap() = 43,
    );
}

// ============================================================================
// ArcSwap
// ============================================================================

fn arcswap_load(c: &mut Criterion) {
    let a = ArcSwap::from_pointee(42u32);
    c.bench_function("arcswap_load", |b| {
        b.iter(|| black_box(*(*a.load()).deref()))
    });
}

fn arcswap_load_contended(c: &mut Criterion) {
    let a = ArcSwap::from_pointee(42u32);
    bench_read_contended(c, "arcswap_load_contended", &a, |a| {
        black_box(*(*a.load()).deref())
    });
}

fn arcswap_load_while_writing(c: &mut Criterion) {
    let a = ArcSwap::from_pointee(42u32);
    bench_read_while_writing(
        c,
        "arcswap_load_while_writing",
        &a,
        |a| a.store(Arc::new(43)),
        |a| black_box(*(*a.load()).deref()),
    );
}

fn arcswap_store(c: &mut Criterion) {
    let a = ArcSwap::from_pointee(42u32);
    c.bench_function("arcswap_store", |b| {
        b.iter(|| a.store(Arc::new(43)))
    });
}

fn arcswap_rcu(c: &mut Criterion) {
    let a = ArcSwap::from_pointee(42u32);
    c.bench_function("arcswap_rcu", |b| {
        b.iter(|| black_box(a.rcu(|x| **x + 43)))
    });
}

fn arcswap_store_contended_4r(c: &mut Criterion) {
    let a = ArcSwap::from_pointee(42u32);
    bench_write_while_reading(
        c,
        "arcswap_store_contended_4r",
        &a,
        4,
        |a| { black_box(*(*a.load()).deref()); },
        |a| a.store(Arc::new(43)),
    );
}

fn arcswap_store_contended_8r(c: &mut Criterion) {
    let a = ArcSwap::from_pointee(42u32);
    bench_write_while_reading(
        c,
        "arcswap_store_contended_8r",
        &a,
        8,
        |a| { black_box(*(*a.load()).deref()); },
        |a| a.store(Arc::new(43)),
    );
}

// ============================================================================
// ArcShift
// ============================================================================

fn arcshift_load(c: &mut Criterion) {
    let a = ArcShift::new(42u32);
    c.bench_function("arcshift_load", |b| b.iter(|| black_box(*a.shared_get())));
}

fn arcshift_load_contended(c: &mut Criterion) {
    let a = ArcShift::new(42u32);
    bench_read_contended(c, "arcshift_load_contended", &a, |a| {
        black_box(*a.shared_get())
    });
}

fn arcshift_update(c: &mut Criterion) {
    let a = Mutex::new(ArcShift::new(42u32));
    c.bench_function("arcshift_update", |b| {
        b.iter(|| {
            let mut a = a.lock().unwrap();
            let v = *a.shared_get() + 43;
            a.update(v);
        })
    });
}

// ============================================================================
// Hazarc AtomicArc
// ============================================================================

fn hazarc_load(c: &mut Criterion) {
    let a = AtomicArc::<u32>::new(Arc::new(42u32));
    c.bench_function("hazarc_load", |b| b.iter(|| black_box(**a.load())));
}

fn hazarc_load_contended(c: &mut Criterion) {
    let a = AtomicArc::<u32>::new(Arc::new(42u32));
    bench_read_contended(c, "hazarc_load_contended", &a, |a| black_box(**a.load()));
}

fn hazarc_load_while_writing(c: &mut Criterion) {
    let a = AtomicArc::<u32>::new(Arc::new(42u32));
    bench_read_while_writing(
        c,
        "hazarc_load_while_writing",
        &a,
        |a| a.store(Arc::new(43)),
        |a| black_box(**a.load()),
    );
}

fn hazarc_store(c: &mut Criterion) {
    let a = AtomicArc::<u32>::new(Arc::new(42u32));
    c.bench_function("hazarc_store", |b| b.iter(|| a.store(Arc::new(43))));
}

fn hazarc_store_contended_4r(c: &mut Criterion) {
    let a = AtomicArc::<u32>::new(Arc::new(42u32));
    bench_write_while_reading(
        c,
        "hazarc_store_contended_4r",
        &a,
        4,
        |a| { black_box(**a.load()); },
        |a| a.store(Arc::new(43)),
    );
}

fn hazarc_store_contended_8r(c: &mut Criterion) {
    let a = AtomicArc::<u32>::new(Arc::new(42u32));
    bench_write_while_reading(
        c,
        "hazarc_store_contended_8r",
        &a,
        8,
        |a| { black_box(**a.load()); },
        |a| a.store(Arc::new(43)),
    );
}

// --- Hazarc Cache ---

fn hazarc_cache_load(c: &mut Criterion) {
    let a = AtomicArc::<u32>::new(Arc::new(42u32));
    let mut cache = Cache::new(&a);
    c.bench_function("hazarc_cache_load", |b| {
        b.iter(|| black_box(**cache.load()))
    });
}

fn hazarc_cache_load_while_writing(c: &mut Criterion) {
    let a = AtomicArc::<u32>::new(Arc::new(42u32));
    let mut cache = Cache::new(&a);
    let started = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    thread::scope(|s| {
        s.spawn(|| {
            started.store(true, Relaxed);
            while !stop.load(Relaxed) {
                a.store(Arc::new(43));
                for _ in 0..8 {
                    hint::spin_loop();
                }
            }
        });
        while !started.load(Relaxed) {
            hint::spin_loop();
        }
        c.bench_function("hazarc_cache_load_while_writing", |b| {
            b.iter(|| black_box(**cache.load()))
        });
        stop.store(true, Relaxed);
    });
}

// ============================================================================
// LockFreeCell
// ============================================================================

fn lockfreecell_read(c: &mut Criterion) {
    let cell = LockFreeCell::new(42u32);
    c.bench_function("lockfreecell_read", |b| {
        b.iter(|| black_box(cell.read(|x| *x)))
    });
}

fn lockfreecell_read_contended(c: &mut Criterion) {
    let cell = LockFreeCell::new(42u32);
    bench_read_contended(c, "lockfreecell_read_contended", &cell, |c| {
        black_box(c.read(|x| *x))
    });
}

fn lockfreecell_read_while_writing(c: &mut Criterion) {
    let cell = LockFreeCell::new(42u32);
    bench_read_while_writing(
        c,
        "lockfreecell_read_while_writing",
        &cell,
        |c| c.write_discard(|x| x + 43),
        |c| black_box(c.read(|x| *x)),
    );
}

fn lockfreecell_store(c: &mut Criterion) {
    let cell = LockFreeCell::new(42u32);
    c.bench_function("lockfreecell_store", |b| {
        b.iter(|| cell.store(43))
    });
}

fn lockfreecell_write(c: &mut Criterion) {
    let cell = LockFreeCell::new(42u32);
    c.bench_function("lockfreecell_write", |b| {
        b.iter(|| cell.write_discard(|x| x + 43))
    });
}

fn lockfreecell_write_contended_4r(c: &mut Criterion) {
    let cell = LockFreeCell::new(42u32);
    bench_write_while_reading(
        c,
        "lockfreecell_write_contended_4r",
        &cell,
        4,
        |c| { black_box(c.read(|x| *x)); },
        |c| c.write_discard(|x| x + 43),
    );
}

fn lockfreecell_write_contended_8r(c: &mut Criterion) {
    let cell = LockFreeCell::new(42u32);
    bench_write_while_reading(
        c,
        "lockfreecell_write_contended_8r",
        &cell,
        8,
        |c| { black_box(c.read(|x| *x)); },
        |c| c.write_discard(|x| x + 43),
    );
}

// ============================================================================
// SpinCell
// ============================================================================

fn spincell_read(c: &mut Criterion) {
    let cell = SpinCell::new(42u32);
    c.bench_function("spincell_read", |b| {
        b.iter(|| black_box(cell.read(|x| *x)))
    });
}

fn spincell_read_contended(c: &mut Criterion) {
    let cell = SpinCell::new(42u32);
    bench_read_contended(c, "spincell_read_contended", &cell, |c| {
        black_box(c.read(|x| *x))
    });
}

fn spincell_read_while_writing(c: &mut Criterion) {
    let cell = SpinCell::new(42u32);
    bench_read_while_writing(
        c,
        "spincell_read_while_writing",
        &cell,
        |c| c.write_discard(|x| *x = 43),
        |c| black_box(c.read(|x| *x)),
    );
}

fn spincell_write(c: &mut Criterion) {
    let cell = SpinCell::new(42u32);
    c.bench_function("spincell_write", |b| {
        b.iter(|| cell.write_discard(|x| *x = 43))
    });
}

fn spincell_write_contended_4r(c: &mut Criterion) {
    let cell = SpinCell::new(42u32);
    bench_write_while_reading(
        c,
        "spincell_write_contended_4r",
        &cell,
        4,
        |c| { black_box(c.read(|x| *x)); },
        |c| c.write_discard(|x| *x = 43),
    );
}

fn spincell_write_contended_8r(c: &mut Criterion) {
    let cell = SpinCell::new(42u32);
    bench_write_while_reading(
        c,
        "spincell_write_contended_8r",
        &cell,
        8,
        |c| { black_box(c.read(|x| *x)); },
        |c| c.write_discard(|x| *x = 43),
    );
}

// ============================================================================

criterion_group!(
    benches,
    // Mutex
    mutex_read,
    mutex_read_contended,
    mutex_write,
    // RwLock
    rwlock_read,
    rwlock_read_contended,
    rwlock_read_while_writing,
    rwlock_write,
    rwlock_write_contended_4r,
    rwlock_write_contended_8r,
    // ArcSwap
    arcswap_load,
    arcswap_load_contended,
    arcswap_load_while_writing,
    arcswap_store,
    arcswap_rcu,
    arcswap_store_contended_4r,
    arcswap_store_contended_8r,
    // ArcShift
    arcshift_load,
    arcshift_load_contended,
    arcshift_update,
    // Hazarc
    hazarc_load,
    hazarc_load_contended,
    hazarc_load_while_writing,
    hazarc_store,
    hazarc_store_contended_4r,
    hazarc_store_contended_8r,
    hazarc_cache_load,
    hazarc_cache_load_while_writing,
    // LockFreeCell
    lockfreecell_read,
    lockfreecell_read_contended,
    lockfreecell_read_while_writing,
    lockfreecell_store,
    lockfreecell_write,
    lockfreecell_write_contended_4r,
    lockfreecell_write_contended_8r,
    // SpinCell
    spincell_read,
    spincell_read_contended,
    spincell_read_while_writing,
    spincell_write,
    spincell_write_contended_4r,
    spincell_write_contended_8r,
);

criterion_main!(benches);
