use crossbeam_utils::{Backoff, CachePadded};
use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};
use std::{marker::PhantomData, ptr::NonNull};
/// Assumes T has at least 8-byte alignment ⇒ 3 bits free
const PTR_MASK: usize = !0b111;
const READER_MASK: usize = 0b111;

#[repr(align(8))]
/// Reference-counted data with atomic ref count
struct RefCountedData<T> {
    data: UnsafeCell<T>,
}

impl<T> RefCountedData<T> {
    fn new(data: T) -> *mut Self {
        let v = Box::new(Self {
            data: UnsafeCell::new(data),
        });
        Box::into_raw(v)
    }
}

pub struct SpinCell<T> {
    inner: CachePadded<AtomicUsize>, // tagged pointer + reader count
    _pd: PhantomData<T>,
}

impl<T> SpinCell<T> {
    pub fn new(value: T) -> Self {
        let ptr = RefCountedData::new(value);
        debug_assert_eq!((ptr as usize) & READER_MASK, 0);
        debug_assert!(align_of::<Box<RefCountedData<T>>>() > 3);

        let addr = ptr as usize;
        Self {
            inner: CachePadded::new(AtomicUsize::new(addr)),
            _pd: PhantomData,
        }
    }
    #[inline(always)]
    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let backoff = Backoff::new();
        loop {
            let old = self.inner.load(Ordering::Acquire);
            let (addr, count) = Self::unpack(old);

            if count == READER_MASK {
                // std::hint::spin_loop();
                backoff.spin();
                continue;
            }

            let new = Self::pack(addr, count + 1);

            match self
                .inner
                .compare_exchange_weak(old, new, Ordering::Release, Ordering::Relaxed)
            {
                Ok(_) => {
                    let ptr = unsafe { NonNull::new_unchecked(addr as *mut RefCountedData<T>) };
                    let ref_data = unsafe { ptr.as_ref() };
                    let result = f(unsafe { ref_data.data.get().as_ref().unwrap() });
                    self.inner.fetch_sub(1, Ordering::Release);
                    return result;
                }
                Err(_) => {
                    // std::hint::spin_loop();
                    backoff.spin();
                    continue;
                }
            }
        }
    }
    #[inline(always)]
    fn unpack(value: usize) -> (usize, usize) {
        (value & PTR_MASK, value & READER_MASK)
    }
    #[inline(always)]
    fn pack(addr: usize, readers: usize) -> usize {
        debug_assert!(readers <= READER_MASK);
        debug_assert_eq!(addr & READER_MASK, 0);
        addr | readers
    }

    #[inline(always)]
    pub fn write_discard(&self, f: impl FnOnce(&mut T)) {
        let backoff = Backoff::new();
        loop {
            let current = self.inner.load(Ordering::Acquire);
            let (addr, readers) = Self::unpack(current);

            if readers != 0 {
                // std::hint::spin_loop();
                backoff.spin();
                continue;
            }

            let new = Self::pack(addr, READER_MASK);

            match self.inner.compare_exchange_weak(
                current,
                new,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    let mut ptr = unsafe { NonNull::new_unchecked(addr as *mut RefCountedData<T>) };
                    let data = unsafe { ptr.as_mut() };
                    f(data.data.get_mut());
                    self.inner.fetch_sub(7, Ordering::Release);
                    return;
                }
                Err(_) => {
                    // std::hint::spin_loop();
                    backoff.spin();
                    continue;
                }
            }
        }
    }
}

impl<T> Drop for SpinCell<T> {
    #[inline(always)]
    fn drop(&mut self) {
        let current = self.inner.load(Ordering::Acquire);
        let (addr, readers) = Self::unpack(current);
        debug_assert_eq!(readers, 0);
        let ptr = unsafe { NonNull::new_unchecked(addr as *mut RefCountedData<T>) };
        drop(unsafe { Box::from_raw(ptr.as_ptr()) })
    }
}

unsafe impl<T: Send> Send for SpinCell<T> {}
unsafe impl<T: Send + Sync> Sync for SpinCell<T> {}
