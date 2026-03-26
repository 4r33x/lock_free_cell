// Adapted from https://github.com/wyfo/hazarc/blob/main/benches/comparison.rs
// Extended with: Mutex, ArcShift, LockFreeCell, SpinCell

use std::{
    array, hint,
    hint::black_box,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
        Arc, Barrier, Mutex, RwLock, RwLockReadGuard,
    },
    thread,
};

use arc_swap::{ArcSwap, ArcSwapOption, Guard};
use arcshift::ArcShift;
use divan::Bencher;
use hazarc::{domain::Domain, ArcBorrow, AtomicArc, AtomicOptionArc, Cache, DefaultDomain};
use lock_free_cell::{LockFreeCell, SpinCell};

// ============================================================================
// Payload type (56 bytes instead of usize)
// ============================================================================

#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[repr(C)]
struct Payload {
    a: [u64; 5],
    b: u128,
}

// ============================================================================
// Traits (from hazarc)
// ============================================================================

trait LoadBench: Default + Send + Sync {
    type Guard<'a>
    where
        Self: 'a;
    fn load(&self) -> Self::Guard<'_>;
    fn bench_load(b: Bencher, threads: bool) {
        let x = black_box(Self::default());
        drop(x.load());
        if threads {
            b.bench(|| drop(x.load()));
        } else {
            b.bench_local(|| drop(x.load()));
        }
    }
    fn bench_load_no_slot(b: Bencher) {
        let x = black_box(Self::default());
        let _guards = array::from_fn::<_, 8, _>(|_| x.load());
        b.bench_local(|| drop(x.load()));
    }
}

trait StoreBench: LoadBench + From<Arc<Payload>> {
    fn store(&self, arc: Arc<Payload>);
    fn bench_load_contended(b: Bencher, threads: bool) {
        let arc = Arc::new(Payload::default());
        let x = black_box(Self::from(arc.clone()));
        drop(x.load());
        let started = AtomicBool::new(false);
        let stop = AtomicBool::new(false);
        thread::scope(|s| {
            s.spawn(|| {
                started.store(true, Relaxed);
                while !stop.load(Relaxed) {
                    x.store(arc.clone());
                    for _ in 0..8 {
                        hint::spin_loop();
                    }
                }
            });
            while !started.load(Relaxed) {
                hint::spin_loop();
            }
            let load = || drop(x.load());
            if threads {
                b.bench(load);
            } else {
                b.bench_local(load);
            }
            stop.store(true, Relaxed);
        });
    }

    fn bench_store(b: Bencher, threads: usize) {
        let arc = Arc::new(Payload::default());
        let x = black_box(Self::from(arc.clone()));
        let barrier = Barrier::new(threads);
        thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| {
                    drop(x.load());
                    barrier.wait();
                });
            }
        });
        b.bench_local(|| x.store(arc.clone()));
    }

    fn bench_store_contended(b: Bencher, threads: usize) {
        let arc = Arc::new(Payload::default());
        let atomic_arc = black_box(Self::from(arc.clone()));
        let started = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| {
                    started.fetch_add(1, Relaxed);
                    while !stop.load(Relaxed) {
                        let _guard = atomic_arc.load();
                        for _ in 0..8 {
                            hint::spin_loop();
                        }
                    }
                });
            }
            while started.load(Relaxed) != threads {
                hint::spin_loop();
            }
            b.bench_local(|| atomic_arc.store(arc.clone()));
            stop.store(true, Relaxed);
        });
    }
}

// ============================================================================
// Trait impls: ArcSwap (from hazarc)
// ============================================================================

impl LoadBench for ArcSwap<Payload> {
    type Guard<'a> = Guard<Arc<Payload>>;
    fn load(&self) -> Self::Guard<'_> {
        self.load()
    }
}
impl StoreBench for ArcSwap<Payload> {
    fn store(&self, arc: Arc<Payload>) {
        self.store(arc);
    }
}
impl LoadBench for ArcSwapOption<Payload> {
    type Guard<'a> = Guard<Option<Arc<Payload>>>;
    fn load(&self) -> Self::Guard<'_> {
        self.load()
    }
}

// ============================================================================
// Trait impls: Hazarc (from hazarc)
// ============================================================================

impl<D: Domain> LoadBench for AtomicArc<Payload, D> {
    type Guard<'a> = ArcBorrow<Payload>;
    fn load(&self) -> Self::Guard<'_> {
        self.load()
    }
}
impl<D: Domain> StoreBench for AtomicArc<Payload, D> {
    fn store(&self, arc: Arc<Payload>) {
        self.store(arc);
    }
}
impl<D: Domain> LoadBench for AtomicOptionArc<Payload, D> {
    type Guard<'a> = Option<ArcBorrow<Payload>>;
    fn load(&self) -> Self::Guard<'_> {
        self.load()
    }
}

// ============================================================================
// Trait impls: RwLock (from hazarc)
// ============================================================================

impl LoadBench for RwLock<Arc<Payload>> {
    type Guard<'a> = RwLockReadGuard<'a, Arc<Payload>>;
    fn load(&self) -> Self::Guard<'_> {
        self.read().unwrap()
    }
}
impl StoreBench for RwLock<Arc<Payload>> {
    fn store(&self, arc: Arc<Payload>) {
        *self.write().unwrap() = arc;
    }
}

#[derive(Default)]
struct RwLockClone<T>(RwLock<T>);
impl LoadBench for RwLockClone<Arc<Payload>> {
    type Guard<'a> = Arc<Payload>;
    fn load(&self) -> Self::Guard<'_> {
        self.0.read().unwrap().clone()
    }
}
impl From<Arc<Payload>> for RwLockClone<Arc<Payload>> {
    fn from(arc: Arc<Payload>) -> Self {
        Self(RwLock::new(arc))
    }
}
impl StoreBench for RwLockClone<Arc<Payload>> {
    fn store(&self, arc: Arc<Payload>) {
        *self.0.write().unwrap() = arc;
    }
}

// ============================================================================
// Trait impls: Mutex (new)
// ============================================================================

impl LoadBench for Mutex<Arc<Payload>> {
    type Guard<'a> = std::sync::MutexGuard<'a, Arc<Payload>>;
    fn load(&self) -> Self::Guard<'_> {
        self.lock().unwrap()
    }
}
impl StoreBench for Mutex<Arc<Payload>> {
    fn store(&self, arc: Arc<Payload>) {
        *self.lock().unwrap() = arc;
    }
}

// ============================================================================
// Trait impls: ArcShift (new)
// ============================================================================

/// ArcShift wrapper that uses shared_get for reads and Mutex-wrapped update for writes.
/// ArcShift::update takes &mut self, so we need interior mutability for StoreBench.
struct ArcShiftBench {
    reader: ArcShift<Payload>,
    writer: Mutex<ArcShift<Payload>>,
}
impl Default for ArcShiftBench {
    fn default() -> Self {
        let w = ArcShift::new(Payload::default());
        let r = w.clone();
        Self {
            reader: r,
            writer: Mutex::new(w),
        }
    }
}
impl From<Arc<Payload>> for ArcShiftBench {
    fn from(arc: Arc<Payload>) -> Self {
        let w = ArcShift::new(*arc);
        let r = w.clone();
        Self {
            reader: r,
            writer: Mutex::new(w),
        }
    }
}
// ArcShift::shared_get returns ArcShiftLight which borrows ArcShift.
// We need an owned guard type, so we just return the value directly.
impl LoadBench for ArcShiftBench {
    type Guard<'a> = Payload;
    fn load(&self) -> Self::Guard<'_> {
        *self.reader.shared_get()
    }
}
impl StoreBench for ArcShiftBench {
    fn store(&self, arc: Arc<Payload>) {
        self.writer.lock().unwrap().update(*arc);
    }
}

// ============================================================================
// Trait impls: LockFreeCell (new)
// ============================================================================

/// LockFreeCell wrapper implementing LoadBench/StoreBench traits.
struct LockFreeCellBench(LockFreeCell<Payload>);
impl Default for LockFreeCellBench {
    fn default() -> Self {
        Self(LockFreeCell::new(Payload::default()))
    }
}
impl From<Arc<Payload>> for LockFreeCellBench {
    fn from(arc: Arc<Payload>) -> Self {
        Self(LockFreeCell::new(*arc))
    }
}
impl LoadBench for LockFreeCellBench {
    type Guard<'a> = Payload;
    fn load(&self) -> Self::Guard<'_> {
        self.0.read(|x| *x)
    }
}
impl StoreBench for LockFreeCellBench {
    fn store(&self, arc: Arc<Payload>) {
        self.0.store(*arc);
    }
}

// ============================================================================
// Trait impls: SpinCell (new)
// ============================================================================

/// SpinCell wrapper implementing LoadBench/StoreBench traits.
struct SpinCellBench(SpinCell<Payload>);
impl Default for SpinCellBench {
    fn default() -> Self {
        Self(SpinCell::new(Payload::default()))
    }
}
impl From<Arc<Payload>> for SpinCellBench {
    fn from(arc: Arc<Payload>) -> Self {
        Self(SpinCell::new(*arc))
    }
}
impl LoadBench for SpinCellBench {
    type Guard<'a> = Payload;
    fn load(&self) -> Self::Guard<'_> {
        self.0.read(|x| *x)
    }
}
impl StoreBench for SpinCellBench {
    fn store(&self, arc: Arc<Payload>) {
        let v = *arc;
        self.0.write_discard(|x| *x = v);
    }
}

// ============================================================================
// LoadSpin wrapper (from hazarc)
// ============================================================================

#[derive(Default)]
struct LoadSpin<T>(T);
impl<T: LoadBench> LoadBench for LoadSpin<T> {
    type Guard<'a>
        = T::Guard<'a>
    where
        Self: 'a;
    fn load(&self) -> Self::Guard<'_> {
        let guard = self.0.load();
        hint::spin_loop();
        guard
    }
}

// ============================================================================
// ArcSwap benchmarks (from hazarc)
// ============================================================================

#[divan::bench]
fn arcswap_load(b: Bencher) {
    ArcSwap::bench_load(b, false);
}
#[divan::bench]
fn arcswap_load_spin(b: Bencher) {
    LoadSpin::<ArcSwap<_>>::bench_load(b, false);
}
#[divan::bench]
fn arcswap_load_no_slot(b: Bencher) {
    ArcSwap::bench_load_no_slot(b);
}
#[divan::bench]
fn arcswap_load_no_slot_spin(b: Bencher) {
    LoadSpin::<ArcSwap<_>>::bench_load_no_slot(b);
}
#[divan::bench]
fn arcswap_load_none(b: Bencher) {
    ArcSwapOption::bench_load(b, false);
}
#[divan::bench]
fn arcswap_load_contended(b: Bencher) {
    ArcSwap::bench_load_contended(b, false);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn arcswap_store(b: Bencher, threads: usize) {
    ArcSwap::bench_store(b, threads);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn arcswap_store_contended(b: Bencher, threads: usize) {
    ArcSwap::bench_store_contended(b, threads);
}

// ============================================================================
// Hazarc benchmarks (from hazarc)
// ============================================================================

#[divan::bench]
fn hazarc_load(b: Bencher) {
    AtomicArc::<_, DefaultDomain>::bench_load(b, false);
}
#[divan::bench]
fn hazarc_load_spin(b: Bencher) {
    LoadSpin::<AtomicArc<_, DefaultDomain>>::bench_load(b, false);
}
#[divan::bench]
fn hazarc_load_no_slot(b: Bencher) {
    AtomicArc::<_, DefaultDomain>::bench_load_no_slot(b);
}
#[divan::bench]
fn hazarc_load_no_slot_spin(b: Bencher) {
    LoadSpin::<AtomicArc<_, DefaultDomain>>::bench_load_no_slot(b);
}
#[divan::bench]
fn hazarc_load_none(b: Bencher) {
    AtomicOptionArc::<_, DefaultDomain>::bench_load(b, false);
}
#[divan::bench]
fn hazarc_load_contended(b: Bencher) {
    AtomicArc::<_, DefaultDomain>::bench_load_contended(b, false);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn hazarc_store(b: Bencher, threads: usize) {
    AtomicArc::<_, DefaultDomain>::bench_store(b, threads);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn hazarc_store_contended(b: Bencher, threads: usize) {
    AtomicArc::<_, DefaultDomain>::bench_store_contended(b, threads);
}

// ============================================================================
// Hazarc Cache benchmarks (new)
// ============================================================================

#[divan::bench]
fn hazarc_cache_load(b: Bencher) {
    let a = AtomicArc::<Payload, DefaultDomain>::default();
    let mut cache = Cache::new(&a);
    b.bench_local(|| black_box(**cache.load()));
}

#[divan::bench]
fn hazarc_cache_load_contended(b: Bencher) {
    let arc = Arc::new(Payload::default());
    let a = AtomicArc::<Payload, DefaultDomain>::from(arc.clone());
    let mut cache = Cache::new(&a);
    let started = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    thread::scope(|s| {
        s.spawn(|| {
            started.store(true, Relaxed);
            while !stop.load(Relaxed) {
                a.store(arc.clone());
                for _ in 0..8 {
                    hint::spin_loop();
                }
            }
        });
        while !started.load(Relaxed) {
            hint::spin_loop();
        }
        b.bench_local(|| black_box(**cache.load()));
        stop.store(true, Relaxed);
    });
}

// ============================================================================
// RwLock benchmarks (from hazarc)
// ============================================================================

#[divan::bench(threads = [0, 1, 2, 4, 8, 16])]
fn rwlock_read(b: Bencher) {
    RwLock::bench_load(b, true);
}
#[divan::bench(threads = [0, 1, 2, 4, 8, 16])]
fn rwlock_read_spin(b: Bencher) {
    LoadSpin::<RwLock<_>>::bench_load(b, true);
}
#[divan::bench(threads = [0, 1, 2, 4, 8, 16])]
fn rwlock_read_clone(b: Bencher) {
    RwLockClone::bench_load(b, true);
}
#[divan::bench(threads = [0, 1, 2, 4, 8, 16])]
fn rwlock_read_clone_spin(b: Bencher) {
    LoadSpin::<RwLockClone<_>>::bench_load(b, true);
}
#[divan::bench(threads = [0, 1, 2, 4, 8, 16])]
fn rwlock_read_contended(b: Bencher) {
    RwLock::bench_load_contended(b, true);
}
#[divan::bench(threads = [0, 1, 2, 4, 8, 16])]
fn rwlock_read_contended_clone(b: Bencher) {
    RwLockClone::bench_load_contended(b, true);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn rwlock_write(b: Bencher, threads: usize) {
    RwLock::bench_store(b, threads);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn rwlock_write_contended(b: Bencher, threads: usize) {
    RwLock::bench_store_contended(b, threads);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn rwlock_write_contended_clone(b: Bencher, threads: usize) {
    RwLockClone::bench_store_contended(b, threads);
}

// ============================================================================
// Mutex benchmarks (new)
// ============================================================================

#[divan::bench(threads = [0, 1, 2, 4, 8, 16])]
fn mutex_read(b: Bencher) {
    Mutex::<Arc<Payload>>::bench_load(b, true);
}
#[divan::bench]
fn mutex_read_contended(b: Bencher) {
    Mutex::<Arc<Payload>>::bench_load_contended(b, false);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn mutex_write(b: Bencher, threads: usize) {
    Mutex::<Arc<Payload>>::bench_store(b, threads);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn mutex_write_contended(b: Bencher, threads: usize) {
    Mutex::<Arc<Payload>>::bench_store_contended(b, threads);
}

// ============================================================================
// ArcShift benchmarks (new)
// ============================================================================

#[divan::bench]
fn arcshift_load(b: Bencher) {
    ArcShiftBench::bench_load(b, false);
}
#[divan::bench]
fn arcshift_load_spin(b: Bencher) {
    LoadSpin::<ArcShiftBench>::bench_load(b, false);
}
#[divan::bench]
fn arcshift_load_contended(b: Bencher) {
    ArcShiftBench::bench_load_contended(b, false);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn arcshift_store(b: Bencher, threads: usize) {
    ArcShiftBench::bench_store(b, threads);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn arcshift_store_contended(b: Bencher, threads: usize) {
    ArcShiftBench::bench_store_contended(b, threads);
}

// ============================================================================
// LockFreeCell benchmarks (new)
// ============================================================================

#[divan::bench]
fn lockfreecell_load(b: Bencher) {
    LockFreeCellBench::bench_load(b, false);
}
#[divan::bench]
fn lockfreecell_load_spin(b: Bencher) {
    LoadSpin::<LockFreeCellBench>::bench_load(b, false);
}
#[divan::bench]
fn lockfreecell_load_contended(b: Bencher) {
    LockFreeCellBench::bench_load_contended(b, false);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn lockfreecell_store(b: Bencher, threads: usize) {
    LockFreeCellBench::bench_store(b, threads);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn lockfreecell_store_contended(b: Bencher, threads: usize) {
    LockFreeCellBench::bench_store_contended(b, threads);
}

// ============================================================================
// SpinCell benchmarks (new)
// ============================================================================

#[divan::bench]
fn spincell_load(b: Bencher) {
    SpinCellBench::bench_load(b, false);
}
#[divan::bench]
fn spincell_load_spin(b: Bencher) {
    LoadSpin::<SpinCellBench>::bench_load(b, false);
}
#[divan::bench]
fn spincell_load_contended(b: Bencher) {
    SpinCellBench::bench_load_contended(b, false);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn spincell_store(b: Bencher, threads: usize) {
    SpinCellBench::bench_store(b, threads);
}
#[divan::bench(args = [0, 1, 2, 4, 8, 16])]
fn spincell_store_contended(b: Bencher, threads: usize) {
    SpinCellBench::bench_store_contended(b, threads);
}

// ============================================================================

fn main() {
    divan::main();
}
