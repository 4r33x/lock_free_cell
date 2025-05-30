use nohash_hasher::{IsEnabled, NoHashHasher};
use seize::Guard;

use std::{
    any::{Any, TypeId},
    cell::UnsafeCell,
    collections::hash_map::Entry,
    hash::BuildHasherDefault,
    marker::PhantomData,
    mem::MaybeUninit,
    sync::atomic::{AtomicPtr, Ordering},
};
use std::{collections::HashMap, sync::atomic::AtomicUsize};

use seize::Collector;

const PRE_ALLOC_SIZE: usize = 16;
const BATCH: usize = 12;

#[derive(Eq, Hash, PartialEq, Clone, Copy)]
struct InstanceId {
    g_id: usize,
    t_id: TypeId,
}

impl IsEnabled for InstanceId {}

static GLOBAL_ID: AtomicUsize = AtomicUsize::new(0);
type InstanceIdHasher = BuildHasherDefault<NoHashHasher<InstanceId>>;
type Fm = HashMap<InstanceId, [Box<dyn Any>; PRE_ALLOC_SIZE], InstanceIdHasher>;

thread_local! {
    static THREAD_LOCAL_NODES: UnsafeCell<Fm> = UnsafeCell::new(Fm::default());
}

#[derive(Eq, Hash, PartialEq)]
struct PreAlloc<T> {
    id: InstanceId,
    _pd: PhantomData<T>,
}
impl<T: 'static> PreAlloc<T> {
    fn new() -> Self {
        Self {
            id: InstanceId {
                g_id: GLOBAL_ID.fetch_add(1, Ordering::Relaxed),
                t_id: TypeId::of::<T>(),
            },
            _pd: PhantomData,
        }
    }
    fn set(&self, value: T) -> *mut Node<T> {
        THREAD_LOCAL_NODES.with(|v| {
            let map = unsafe { &mut *v.get() };
            match map.entry(self.id) {
                Entry::Occupied(mut entry) => {
                    let nodes = entry.get_mut();
                    for node in nodes {
                        let node = node.downcast_mut::<Node<T>>().unwrap();
                        if node.write_locked.try_write() {
                            return unsafe { node.set(value) };
                        }
                    }
                    Node::new_raw(value)
                }
                Entry::Vacant(entry) => {
                    let mut a: [Box<dyn Any + 'static>; PRE_ALLOC_SIZE] =
                        std::array::from_fn(|_| Node::<T>::new_uninit_local() as Box<dyn Any>);
                    let ptr = unsafe { a[0].downcast_mut::<Node<T>>().unwrap().set(value) };
                    entry.insert(a);
                    ptr
                }
            }
        })
    }
}

enum WriteLock {
    Local(LockState),
    Boxed,
}
#[repr(u32)]
enum LockState {
    Available = 0,
    Writing = 1,
    ReadLocked = 2,
}
impl WriteLock {
    fn try_write(&mut self) -> bool {
        match self {
            WriteLock::Local(state) => match state {
                LockState::Available => {
                    *state = LockState::Writing;
                    true
                }
                _ => false,
            },
            WriteLock::Boxed => unreachable!(),
        }
    }
    fn read_lock(&mut self) {
        match self {
            WriteLock::Local(state) => *state = LockState::ReadLocked,
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

    fn new_uninit_local() -> Box<Node<T>> {
        Box::new(Self {
            write_locked: WriteLock::Local(LockState::Available),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        })
    }
    fn new_raw(value: T) -> *mut Node<T> {
        Box::into_raw(Box::new(Self {
            write_locked: WriteLock::Boxed,
            value: UnsafeCell::new(MaybeUninit::new(value)),
        }))
    }
    unsafe fn set(&mut self, value: T) -> *mut Node<T> {
        self.value.get_mut().write(value);
        self.write_locked.read_lock();
        self as *const Node<T> as *mut Node<T>
    }
}

unsafe impl<T: Send> Send for LockFreeCell<T> {}
unsafe impl<T: Send + Sync> Sync for LockFreeCell<T> {}

pub struct LockFreeCell<T> {
    pre_alloc: PreAlloc<T>,

    collector: Collector,

    head: AtomicPtr<Node<T>>,
}

impl<T: 'static> LockFreeCell<T> {
    pub fn new(value: T) -> Self {
        let pre_alloc = PreAlloc::new();
        let ptr = pre_alloc.set(value);
        Self {
            pre_alloc,
            collector: Collector::new().batch_size(BATCH),
            head: AtomicPtr::new(ptr),
        }
    }
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
                    let value = &mut *value;
                    match &mut value.write_locked {
                        WriteLock::Local(u) => {
                            *u = LockState::Available;
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
