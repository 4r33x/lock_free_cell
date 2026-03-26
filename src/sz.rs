use crossbeam_utils::CachePadded;
use seize::{Guard, reclaim};
use std::{
    alloc::Layout,
    cell::Cell,
    mem::MaybeUninit,
    sync::atomic::{AtomicPtr, Ordering},
};

use seize::Collector;

const CACHE_SIZE: usize = 2;
type CacheEntry = (*mut u8, Layout);
const EMPTY_ENTRY: CacheEntry = (std::ptr::null_mut(), Layout::new::<()>());

thread_local! {
    static NODE_CACHE: Cell<[CacheEntry; CACHE_SIZE]> = const {
        Cell::new([EMPTY_ENTRY; CACHE_SIZE])
    };
}
const BATCH_SIZE: usize = 32;
const RO: Ordering = Ordering::Acquire;
const WO: Ordering = Ordering::Release;
pub struct LockFreeCell<T> {
    collector: Collector,
    head: CachePadded<AtomicPtr<Node<T>>>,
}
impl<T> Drop for LockFreeCell<T> {
    fn drop(&mut self) {
        let head = self.head.load(RO);
        unsafe { drop(Box::from_raw(head)) };
    }
}
struct Node<T> {
    value: T,
}
impl<T> Node<T> {
    #[inline]
    unsafe fn get<'a>(node: *mut Node<T>) -> &'a T {
        &unsafe { &*node }.value
    }
    #[inline]
    unsafe fn set(node: *mut Node<T>, value: T) {
        unsafe { &mut *node }.value = value;
    }
    #[inline]
    fn new_boxed(value: T) -> *mut Node<T> {
        Box::into_raw(Box::new(Self { value }))
    }
    #[inline]
    fn new_cached(value: T) -> *mut Node<T> {
        let layout = Layout::new::<Node<T>>();
        NODE_CACHE.with(|c| {
            let mut slots = c.get();
            for slot in &mut slots {
                if !slot.0.is_null() && slot.1 == layout {
                    let ptr = slot.0 as *mut Node<T>;
                    *slot = EMPTY_ENTRY;
                    c.set(slots);
                    unsafe { ptr.write(Node { value }) };
                    return ptr;
                }
            }
            Node::new_boxed(value)
        })
    }
}

unsafe fn cache_reclaim<T>(ptr: *mut Node<T>, _collector: &Collector) {
    unsafe { std::ptr::drop_in_place(ptr) };
    let layout = Layout::new::<Node<T>>();
    NODE_CACHE.with(|c| {
        let mut slots = c.get();
        for slot in &mut slots {
            if slot.0.is_null() {
                *slot = (ptr as *mut u8, layout);
                c.set(slots);
                return;
            }
        }
        // Cache full, deallocate
        unsafe { std::alloc::dealloc(ptr as *mut u8, layout) };
    });
}

unsafe impl<T: Send> Send for LockFreeCell<T> {}
unsafe impl<T: Send + Sync> Sync for LockFreeCell<T> {}

impl<T> LockFreeCell<T> {
    pub fn new(value: T) -> Self {
        Self {
            collector: Collector::new().batch_size(BATCH_SIZE),
            head: CachePadded::new(AtomicPtr::new(Node::new_boxed(value))),
        }
    }

    #[inline]
    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let guard = self.collector.enter();
        let head = guard.protect(&self.head, RO);
        f(unsafe { Node::get(head) })
    }

    /// Replace the value without reading the old one. Uses atomic swap (no CAS loop).
    /// Uses thread-local allocation cache to avoid Box::new on every call.
    #[inline]
    pub fn store(&self, value: T) {
        let new_ptr = Node::new_cached(value);
        let old = self.head.swap(new_ptr, WO);
        unsafe { self.collector.retire(old, cache_reclaim) };
    }

    pub fn write_discard(&self, f: impl Fn(&T) -> T) {
        let mut new_value = MaybeUninit::uninit();
        let mut set = false;
        let guard = self.collector.enter();
        loop {
            let head = guard.protect(&self.head, RO);
            let ptr = if set {
                let ptr = unsafe { *new_value.as_mut_ptr() };
                unsafe { Node::set(ptr, f(Node::get(head))) };
                ptr
            } else {
                new_value.write(unsafe { Node::new_boxed(f(Node::get(head))) });
                set = true;
                unsafe { *new_value.as_mut_ptr() }
            };

            if self
                .head
                .compare_exchange(head, ptr, WO, Ordering::Relaxed)
                .is_ok()
            {
                unsafe { self.collector.retire(head, reclaim::boxed) };
                break;
            };
        }
    }
}
