//! These are very minimal benchmarks ‒ reading and writing an integer shared in
//! different ways. You can compare the times and see the characteristics.

use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use std::{
    hint::black_box,
    io::{self, Write},
};

use arc_swap::ArcSwap;

use crossbeam_utils::thread;
use lock_free_cell::LockFreeCell;

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
    test_run(name, 4, 1, iterations, &r, &w);
    test_run(name, 4, 2, iterations, &r, &w);
    test_run(name, 4, 4, iterations, &r, &w);
    test_run(name, 8, 1, iterations, &r, &w);
    test_run(name, 8, 2, iterations, &r, &w);
    test_run(name, 8, 4, iterations, &r, &w);
    test_run(name, 0, 1, iterations, &r, &w);
    test_run(name, 0, 4, iterations, &r, &w);
}

fn main() {
    let lock = RwLock::new(42);
    test_round(
        "rw",
        100_000,
        || *lock.read().unwrap(),
        |i| *lock.write().unwrap() = i,
    );

    let arc = ArcSwap::from(Arc::new(42));
    test_round(
        "arc-load-store",
        100_000,
        || **arc.load(),
        |i| arc.store(Arc::new(i)),
    );
    test_round(
        "arc-rcu",
        100_000,
        || *arc.load_full(),
        |i| {
            arc.rcu(|_| Arc::new(i));
        },
    );
    let cell = LockFreeCell::new(32);

    test_round(
        "cell-rcu",
        100_000,
        || cell.read(|x| *x),
        |i| {
            cell.write_discard(|_| i);
        },
    );
}
