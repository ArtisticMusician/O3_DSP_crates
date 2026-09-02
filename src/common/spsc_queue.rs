use crate::common::cache_pad::CachePadded;
use std::alloc::{alloc, dealloc, Layout};
use std::mem::{self, MaybeUninit};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct SpscRingBuffer<T> {
    base_ptr: *mut MaybeUninit<T>,
    capacity: usize,
    mask: usize,
    layout: Layout,

    pub write_idx: CachePadded<AtomicUsize>,
    pub read_index: CachePadded<AtomicUsize>,
}

unsafe impl<T: Send> Send for SpscRingBuffer<T> {}
unsafe impl<T: Send> Sync for SpscRingBuffer<T> {}

impl<T> SpscRingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        assert!(
            capacity.is_power_of_two(),
            "Buffer capacity must be a power of 2."
        );
        assert!(
            mem::size_of::<T>() > 0,
            "Zero-Sized Types are not supported."
        );

        let raw_layout = Layout::array::<MaybeUninit<T>>(capacity)
            .expect("Valid memory layout computation failure.");
        let layout = raw_layout
            .align_to(64)
            .expect("Forced structural alignment mapping failure.");

        let base_ptr = unsafe { alloc(layout) as *mut MaybeUninit<T> };
        if base_ptr.is_null() {
            panic!("System OOM: Low-level SpscRingBuffer raw allocation failed.");
        }

        Self {
            base_ptr,
            capacity,
            mask: capacity - 1,
            layout,
            write_idx: CachePadded::new(AtomicUsize::new(0)),
            read_index: CachePadded::new(AtomicUsize::new(0)),
        }
    }

    #[inline(always)]
    pub fn push(&self, item: T) -> Result<(), T> {
        let write = self.write_idx.value.load(Ordering::Relaxed);
        let read = self.read_index.value.load(Ordering::Acquire);

        if write - read >= self.capacity {
            return Err(item);
        }

        let mask_idx = write & self.mask;

        unsafe {
            let slot_ptr = self.base_ptr.add(mask_idx);
            slot_ptr.write(MaybeUninit::new(item));
        }

        self.write_idx.value.store(write + 1, Ordering::Release);
        Ok(())
    }

    #[inline(always)]
    pub fn pop(&self) -> Option<T> {
        let read = self.read_index.value.load(Ordering::Relaxed);
        let write = self.write_idx.value.load(Ordering::Acquire);

        if read == write {
            return Option::None;
        }

        let mask_idx = read & self.mask;

        let item = unsafe {
            let slot_ptr = self.base_ptr.add(mask_idx);
            slot_ptr.read().assume_init()
        };

        self.read_index.value.store(read + 1, Ordering::Release);
        Some(item)
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        let write = self.write_idx.value.load(Ordering::Relaxed);
        let read = self.read_index.value.load(Ordering::Relaxed);
        write.saturating_sub(read)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> Drop for SpscRingBuffer<T> {
    fn drop(&mut self) {
        if mem::needs_drop::<T>() {
            let mut read = self.read_index.value.load(Ordering::Relaxed);
            let write = self.write_idx.value.load(Ordering::Relaxed);

            while read != write {
                let mask_idx = read & self.mask;
                unsafe {
                    let slot_ptr = self.base_ptr.add(mask_idx);
                    ptr::drop_in_place((*slot_ptr).as_mut_ptr());
                }
                read += 1;
            }
        }

        unsafe {
            dealloc(self.base_ptr as *mut u8, self.layout);
        }
    }
}
