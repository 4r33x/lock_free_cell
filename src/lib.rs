pub mod sz;
pub mod sz2;
pub mod sz3;
pub mod tagged;
pub use sz::LockFreeCell;
pub use tagged::SpinCell;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        hint::black_box,
        sync::{Mutex, atomic::AtomicBool},
        time::{Duration, Instant},
    };
    use std::{sync::Arc, thread};
    type LockFree<T> = LockFreeCell<T>;
    const ITERS: usize = 1_000_000;
    #[test]
    fn mutex_bench() {
        let ac = Mutex::new(42u32);
        let start = Instant::now();
        let mut res = 0;
        for _ in 0..ITERS {
            res += *ac.lock().unwrap();
        }
        println!("mutex_bench: {:?} res: {res}", start.elapsed());
    }

    #[test]
    fn my_lock_free_cell_contended_bench() {
        let ac = Arc::new(LockFreeCell::new(42u32));
        let ac_clone = ac.clone();
        let terminate = Arc::new(AtomicBool::new(false));
        let t2 = terminate.clone();
        let j = thread::spawn(move || {
            while !t2.load(std::sync::atomic::Ordering::Relaxed) {
                black_box(ac_clone.read(|x| *x));
            }
        });
        thread::sleep(std::time::Duration::from_millis(100));
        let start = Instant::now();
        let mut res = 0;
        for _ in 0..ITERS {
            ac.read(|x| res += *x);
        }
        terminate.swap(true, std::sync::atomic::Ordering::Relaxed);
        j.join().unwrap();
        println!(
            "my_lock_free_cell_contended_bench: {:?}, res: {res}",
            start.elapsed()
        );
    }
    #[test]
    fn my_lock_free_cell_bench() {
        let ac: LockFreeCell<u32> = LockFreeCell::new(42u32);
        let start = Instant::now();
        let mut res = 0;
        for _ in 0..ITERS {
            ac.read(|x| res += *x);
        }
        println!("my_lock_free_cell_bench: {:?}, res: {res}", start.elapsed());
    }
    #[test]
    fn my_lock_free_cell_update() {
        let ac: LockFreeCell<u32> = LockFreeCell::new(42u32);
        let start = Instant::now();
        for _ in 0..ITERS {
            ac.write_discard(|x| x + 43)
        }
        println!("my_lock_free_cell_update: {:?}", start.elapsed());
    }
    #[test]
    fn basic_read() {
        let lock_free = LockFree::new(42);
        let result = lock_free.read(|x| *x);
        assert_eq!(result, 42);
    }

    #[test]
    fn basic_drop() {
        let lock_free = Arc::new(LockFree::new(42));
        let lock_free_c = lock_free.clone();
        let result = lock_free_c.read(|x| *x);
        assert_eq!(result, 42);
        let result = lock_free.read(|x| *x);
        assert_eq!(result, 42);
        drop(lock_free);
        let result = lock_free_c.read(|x| *x);
        assert_eq!(result, 42);
        drop(lock_free_c);
    }

    #[test]
    fn basic_read_write() {
        let lock_free = LockFree::new(42);

        let result = lock_free.read(|x| *x);
        assert_eq!(result, 42);

        lock_free.write_discard(|x| x + 100);

        let result = lock_free.read(|x| *x);
        assert_eq!(result, 142);
    }

    #[test]
    fn concurrent_readers() {
        let lock_free = Arc::new(LockFree::new(42));
        let mut handles = vec![];

        for _ in 0..5 {
            let lf = lock_free.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    let result = lf.read(|x| {
                        thread::sleep(Duration::from_micros(1));
                        *x
                    });
                    assert_eq!(result, 42);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn write_during_read() {
        let lock_free = Arc::new(LockFree::new(42));
        let lf_reader = lock_free.clone();
        let lf_writer = lock_free.clone();
        let lf_reader1 = lock_free.clone();
        let lf_writer1 = lock_free.clone();
        let lf_reader2 = lock_free.clone();
        let lf_writer2 = lock_free.clone();

        // Start a long-running read
        let read_handle = thread::spawn(move || {
            for _ in 0..10 {
                let read = lf_reader.read(|x| {
                    let val = *x;
                    thread::sleep(Duration::from_millis(50));

                    val
                });
                println!("Readed: {read}")
            }
        });
        let read_handle1 = thread::spawn(move || {
            for _ in 0..10 {
                let read = lf_reader1.read(|x| {
                    let val = *x;
                    thread::sleep(Duration::from_millis(50));

                    val
                });
                println!("Readed: {read}")
            }
        });
        let read_handle2 = thread::spawn(move || {
            for _ in 0..10 {
                let read = lf_reader2.read(|x| {
                    let val = *x;
                    thread::sleep(Duration::from_millis(50));

                    val
                });
                println!("Readed: {read}")
            }
        });

        thread::sleep(Duration::from_millis(10));

        let write_handle = thread::spawn(move || {
            for _ in 0..10 {
                lf_writer.write_discard(|x| *x + 100);
                thread::sleep(Duration::from_millis(10));
            }
        });
        let write_handle1 = thread::spawn(move || {
            for _ in 0..10 {
                lf_writer1.write_discard(|x| *x + 100);
                thread::sleep(Duration::from_millis(10));
            }
        });
        let write_handle2 = thread::spawn(move || {
            for _ in 0..10 {
                lf_writer2.write_discard(|x| *x + 100);
                thread::sleep(Duration::from_millis(10));
            }
        });

        read_handle.join().unwrap();
        write_handle.join().unwrap();
        read_handle1.join().unwrap();
        read_handle2.join().unwrap();
        write_handle1.join().unwrap();
        write_handle2.join().unwrap();

        let new_result = lock_free.read(|x| *x);
        assert_eq!(new_result, 3042);
    }
}
