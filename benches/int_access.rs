//! Benchmarks reading and writing an integer shared in different ways.
//! Compares all available concurrent cell types across various R+W thread combinations.

use std::hint::black_box;
use std::io::{self, Write};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use arc_swap::ArcSwap;
use arcshift::ArcShift;
use crossbeam_utils::thread;
use hazarc::{AtomicArc, Cache};
use lock_free_cell::{LockFreeCell, SpinCell};

fn test_run<R, W>(
    name: &str,
    read_threads: usize,
    write_threads: usize,
    iterations: usize,
    r: R,
    w: W,
) where
    R: Fn() -> usize + Sync + Send,
    W: Fn(usize) + Sync + Send,
{
    print!(
        "{:20} ({} + {}) x {}: ",
        name, read_threads, write_threads, iterations
    );
    io::stdout().flush().unwrap();
    let before = Instant::now();
    thread::scope(|scope| {
        for _ in 0..read_threads {
            scope.spawn(|_| {
                for _ in 0..iterations {
                    black_box(r());
                }
            });
        }
        for _ in 0..write_threads {
            scope.spawn(|_| {
                for i in 0..iterations {
                    black_box(w(i));
                }
            });
        }
    })
    .unwrap();
    let elapsed = before.elapsed();
    println!("{:?}", elapsed);
}

fn test_round<R, W>(name: &str, iterations: usize, r: R, w: W)
where
    R: Fn() -> usize + Sync + Send,
    W: Fn(usize) + Sync + Send,
{
    test_run(name, 1, 0, iterations, &r, &w);
    test_run(name, 2, 0, iterations, &r, &w);
    test_run(name, 4, 0, iterations, &r, &w);
    test_run(name, 8, 0, iterations, &r, &w);
    test_run(name, 1, 1, iterations, &r, &w);
    test_run(name, 2, 2, iterations, &r, &w);
    test_run(name, 4, 1, iterations, &r, &w);
    test_run(name, 4, 2, iterations, &r, &w);
    test_run(name, 4, 4, iterations, &r, &w);
    test_run(name, 8, 1, iterations, &r, &w);
    test_run(name, 8, 2, iterations, &r, &w);
    test_run(name, 8, 4, iterations, &r, &w);
    test_run(name, 0, 1, iterations, &r, &w);
    test_run(name, 0, 4, iterations, &r, &w);
}

const ITERATIONS: usize = 100_000;

fn main() {
    // --- Mutex ---
    let mutex = Mutex::new(42usize);
    test_round(
        "mutex",
        ITERATIONS,
        || *mutex.lock().unwrap(),
        |i| *mutex.lock().unwrap() = i,
    );

    // --- RwLock ---
    let lock = RwLock::new(42usize);
    test_round(
        "rw",
        ITERATIONS,
        || *lock.read().unwrap(),
        |i| *lock.write().unwrap() = i,
    );

    // --- ArcSwap load/store ---
    let arc = ArcSwap::from(Arc::new(42usize));
    test_round(
        "arc-load-store",
        ITERATIONS,
        || **arc.load(),
        |i| arc.store(Arc::new(i)),
    );

    // --- ArcSwap RCU ---
    test_round(
        "arc-rcu",
        ITERATIONS,
        || *arc.load_full(),
        |i| {
            arc.rcu(|_| Arc::new(i));
        },
    );

    // --- ArcShift ---
    // ArcShift::update takes &mut self, so we wrap in Mutex for multi-thread writes.
    let shift_base = ArcShift::new(42usize);
    let shift_r = shift_base.clone();
    let shift_w = Mutex::new(shift_base);
    test_round(
        "arcshift",
        ITERATIONS,
        || *shift_r.shared_get(),
        |i| shift_w.lock().unwrap().update(i),
    );

    // --- Hazarc AtomicArc ---
    let hazarc = AtomicArc::<usize>::new(Arc::new(42usize));
    test_round(
        "hazarc",
        ITERATIONS,
        || **hazarc.load(),
        |i| hazarc.store(Arc::new(i)),
    );

    // --- Hazarc Cache ---
    // Cache is thread-local, so each read thread needs its own.
    // We benchmark it in a custom loop since test_round's read closure is shared.
    println!("\n--- hazarc-cache (thread-local read cache) ---");
    let hazarc_for_cache = AtomicArc::<usize>::new(Arc::new(42usize));
    for &(rt, wt) in &[
        (1, 0),
        (2, 0),
        (4, 0),
        (8, 0),
        (1, 1),
        (4, 1),
        (4, 2),
        (4, 4),
        (8, 1),
        (8, 2),
        (8, 4),
        (0, 1),
        (0, 4),
    ] {
        print!(
            "{:20} ({} + {}) x {}: ",
            "hazarc-cache", rt, wt, ITERATIONS
        );
        io::stdout().flush().unwrap();
        let before = Instant::now();
        thread::scope(|scope| {
            for _ in 0..rt {
                scope.spawn(|_| {
                    let mut cache = Cache::new(&hazarc_for_cache);
                    for _ in 0..ITERATIONS {
                        black_box(**cache.load());
                    }
                });
            }
            for _ in 0..wt {
                scope.spawn(|_| {
                    for i in 0..ITERATIONS {
                        black_box(hazarc_for_cache.store(Arc::new(i)));
                    }
                });
            }
        })
        .unwrap();
        println!("{:?}", before.elapsed());
    }

    // --- LockFreeCell ---
    let cell = LockFreeCell::new(42usize);
    test_round(
        "lf-cell-rcu",
        ITERATIONS,
        || cell.read(|x| *x),
        |i| cell.store(i),
    );

    // --- SpinCell ---
    let spin = SpinCell::new(42usize);
    test_round(
        "spin-cell",
        ITERATIONS,
        || spin.read(|x| *x),
        |i| spin.write_discard(|x| *x = i),
    );
}
