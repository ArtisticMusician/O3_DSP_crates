    // A raw heap pointer to the contiguous block of uninitialized elements.
    // This satisfies LLVM pointer provenance rules during concurrent thread access.
    base_ptr: *mut MaybeUninit<T>,
    capacity: usize,
    mask: usize,
    layout: Layout, // Cached layout tracking to guarantee absolute compliance during Drop

    pub write_idx: CachePadded<AtomicUsize>,
    pub read_index: CachePadded<AtomicUsize>,
}unsafe impl<T: Send> Send for SpscRingBuffer<T> {}unsafe impl<T: Send> Sync for SpscRingBuffer<T> {}impl<T> SpscRingBuffer<T> {
    /// Creates a new heap-allocated bounded SPSC Ring Buffer.
    /// Capacity must be a power of two to optimize wrapping calculations via bitwise masking.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity.is_power_of_two(), "Buffer capacity must be a power of 2.");
        assert!(mem::size_of::<T>() > 0, "Zero-Sized Types are not supported.");

        // 1. Calculate a hardware-optimized layout forced to 64-byte alignment
        let raw_layout = Layout::array::<MaybeUninit<T>>(capacity)
            .expect("Valid memory layout computation failure.");
        let layout = raw_layout.align_to(64)
            .expect("Forced structural alignment mapping failure.");

        // 2. Claim memory directly from the global heap allocator
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

    /// Pushes a new item onto the ring buffer.
    /// Returns Err(item) if the buffer is full. Real-time safe.
    #[inline(always)]
    pub fn push(&self, item: T) -> Result<(), T> {
        let write = self.write_idx.value.load(Ordering::Relaxed);
        let read = self.read_index.value.load(Ordering::Acquire); // Synchronize with pop loop release

        // Ironclad Boundary Check: Bounded strictly by the Acquire fence to stop
        // speculative memory re-ordering on weakly ordered CPU cores.
        if write - read >= self.capacity {
            return Err(item);
        }

        let mask_idx = write & self.mask;
        
        unsafe {
            // Write directly to the raw pointer offset without alias tracking constraints
            let slot_ptr = self.base_ptr.add(mask_idx);
            slot_ptr.write(MaybeUninit::new(item));
        }

        // Release ordering guarantees the data write is visible before the index increments
        self.write_idx.value.store(write + 1, Ordering::Release);
        Ok(())
    }

    /// Pops an item off the ring buffer.
    /// Returns None if the buffer is empty. Real-time safe.
    #[inline(always)]
    pub fn pop(&self) -> Option<T> {
        let read = self.read_index.value.load(Ordering::Relaxed);
        let write = self.write_idx.value.load(Ordering::Acquire); // Synchronize with push loop release

        // Ironclad Boundary Check: Bounded strictly by the Acquire fence to stop
        // speculative memory re-ordering on weakly ordered CPU cores.
        if read == write {
            return Option::None;
        }

        let mask_idx = read & self.mask;
        
        let item = unsafe {
            let slot_ptr = self.base_ptr.add(mask_idx);
            // Read through provenance-compliant paths and unpack safely
            slot_ptr.read().assume_init()
        };

        // Release ordering notifies the producer thread that the slot is now empty
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
}impl<T> Drop for SpscRingBuffer<T> {
    fn drop(&mut self) {
        // Only empty remaining instances if the type actually contains destructor resources
        if mem::needs_drop::<T>() {
            let mut read = self.read_index.value.load(Ordering::Relaxed);
            let write = self.write_idx.value.load(Ordering::Relaxed);
            
            while read != write {
                let mask_idx = read & self.mask;
                unsafe {
                    let slot_ptr = self.base_ptr.add(mask_idx);
                    // Extract internal raw address tracking data directly to drop elements natively
                    ptr::drop_in_place((*slot_ptr).as_mut_ptr());
                }
                read += 1;
            }
        }
        
        // Deallocate the bare-metal layout memory block cleanly
        unsafe {
            dealloc(self.base_ptr as *mut u8, self.layout);
        }
    }
}