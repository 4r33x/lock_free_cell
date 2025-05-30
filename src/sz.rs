use crossbeam_utils::CachePadded;
use seize::{Guard, reclaim};
use std::{
    mem::MaybeUninit,
    sync::atomic::{AtomicPtr, Ordering},
};

use seize::Collector;
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
    unsafe fn get<'a>(node: *mut Node<T>) -> &'a T {
        &unsafe { &*node }.value
    }
    unsafe fn set(node: *mut Node<T>, value: T) {
        unsafe { &mut *node }.value = value;
    }
    // fn new_owned_box(value: T) -> Box<Node<T>> {
    //     Box::new(Self { value })
    // }

    fn new_boxed(value: T) -> *mut Node<T> {
        Box::into_raw(Box::new(Self { value }))
    }
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
    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let guard = self.collector.enter();
        let head = guard.protect(&self.head, RO);
        f(unsafe { Node::get(head) })
    }

    pub fn write_discard(&self, f: impl Fn(&T) -> T) {
        let mut new_value = MaybeUninit::uninit();
        let mut set = false;
        loop {
            let guard = self.collector.enter();
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
