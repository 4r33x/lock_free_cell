use crossbeam_utils::CachePadded;
use seize::Guard;
use std::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicPtr, AtomicU32, Ordering},
};

use seize::Collector;
const PRE_ALLOC_SIZE: usize = 16;
const BATCH: usize = 8;
struct PreAlloc<T> {
    array: [CachePadded<Node<T>>; PRE_ALLOC_SIZE],
}

impl<T> PreAlloc<T> {
    fn new() -> Self {
        Self {
            array: [const { CachePadded::new(Node::new_uninit()) }; PRE_ALLOC_SIZE],
        }
    }
    fn set(&self, value: T) -> *mut Node<T> {
        for node in self.array.iter() {
            if node.write_locked.try_write() {
                return unsafe { node.set(value) };
            }
        }
        Node::new_boxed(value)
    }
}
pub struct LockFreeCell<T> {
    // The collector for memory reclamation.
    collector: Collector,
    // The head of the stack.
    head: AtomicPtr<Node<T>>,
    pre_alloc: PreAlloc<T>,
}

enum WriteLock {
    Array(AtomicU32),
    Boxed,
}
#[repr(u32)]
enum LockState {
    Available = 0,
    Writing = 1,
    ReadLocked = 2,
}
impl WriteLock {
    fn try_write(&self) -> bool {
        match self {
            WriteLock::Array(atomic_u32) => atomic_u32
                .compare_exchange_weak(
                    LockState::Available as u32,
                    LockState::Writing as u32,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok(),
            WriteLock::Boxed => unreachable!(),
        }
    }
    fn read_lock(&self) {
        match self {
            WriteLock::Array(atomic_u32) => {
                atomic_u32.store(LockState::ReadLocked as u32, Ordering::Relaxed)
            }
            WriteLock::Boxed => unreachable!(),
        }
    }
}

struct Node<T> {
    write_locked: WriteLock,
    value: UnsafeCell<MaybeUninit<T>>,
}
impl<T> Node<T> {
    unsafe fn get<'a>(node: *mut Node<T>) -> &'a T {
        let node = unsafe { &*node };
        unsafe { &*(*node.value.get()).as_ptr() }
    }
    fn new_boxed(value: T) -> *mut Node<T> {
        Box::into_raw(Box::new(Self {
            write_locked: WriteLock::Boxed,
            value: UnsafeCell::new(MaybeUninit::new(value)),
        }))
    }
    const fn new_uninit() -> Self {
        Self {
            write_locked: WriteLock::Array(AtomicU32::new(LockState::Available as u32)),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
    unsafe fn set(&self, value: T) -> *mut Node<T> {
        unsafe { self.value.get().write(MaybeUninit::new(value)) };
        self.write_locked.read_lock();
        self as *const Node<T> as *mut Node<T>
    }
}

unsafe impl<T: Send> Send for LockFreeCell<T> {}
unsafe impl<T: Send + Sync> Sync for LockFreeCell<T> {}

impl<T> LockFreeCell<T> {
    pub fn new(value: T) -> Self {
        let pre_alloc = PreAlloc::new();
        let ptr = pre_alloc.set(value);
        Self {
            pre_alloc,
            collector: Collector::new().batch_size(BATCH),
            head: AtomicPtr::new(ptr),
        }
    }
}

impl<T> LockFreeCell<T> {
    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let guard = self.collector.enter();
        let head = guard.protect(&self.head, Ordering::Relaxed);
        f(unsafe { Node::get(head) })
    }

    pub fn write_discard(&self, f: impl FnOnce(&T) -> T) {
        let guard = self.collector.enter();
        let head = guard.protect(&self.head, Ordering::Relaxed);
        let new_val = unsafe { f(Node::get(head)) };
        let new_head = self.pre_alloc.set(new_val);
        self.head.swap(new_head, Ordering::Relaxed);
        drop(guard);
        unsafe {
            self.collector
                .retire(head, |value: *mut Node<T>, _collector: &Collector| {
                    // Safety: The value was allocated with `Box::new`.
                    match &value.read().write_locked {
                        WriteLock::Array(atomic_u32) => {
                            atomic_u32.store(LockState::Available as u32, Ordering::Relaxed);
                        }
                        WriteLock::Boxed => {
                            let value = Box::from_raw(value);
                            drop(value);
                        }
                    }
                })
        };
    }
}
