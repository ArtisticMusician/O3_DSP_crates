use crate::frame_guard::FrameGuard;
use crate::MpmcRingBuffer;
use std::alloc::{alloc, dealloc, Layout};
use std::mem;
use std::ptr;

/// A completely allocation-free, 64-byte cache-line aligned frame arena pool for real-time threads.
/// Engineered for zero-copy initialization, absolute thread isolation, and pristine SIMD alignment.
pub struct FramePool<T> {
    base_ptr: *mut T,
    total_elements: usize,
    frame_size: usize,
    layout: Layout, // Cached layout tracking to guarantee absolute compliance during Drop
    pub(crate) available_indices: MpmcRingBuffer<usize>,
}

unsafe impl<T: Send> Send for FramePool<T> {}
unsafe impl<T: Send> Sync for FramePool<T> {}

impl<T> FramePool<T>
where
    T: Copy + Default,
{
    /// Instantiate completely on the UI / Main Initialization thread.
    /// Bypasses all standard container overhead to achieve direct, single-pass zero-copy heap allocation.
    pub fn new(max_frames: usize, frame_size: usize) -> Self {
        assert!(max_frames.is_power_of_two(), "Max frames must be a power of 2.");
        assert!(mem::size_of::<T>() > 0, "Zero-Sized Types are not supported.");

        let total_elements = max_frames * frame_size;

        // 1. Allocate index queue BEFORE the heap memory is claimed.
        // If this fails, no raw memory is left orphaned or leaked.
        let available_indices = MpmcRingBuffer::new(max_frames);
        for i in 0..max_frames {
            let _ = available_indices.push(i);
        }

        // 2. Compute a hardware-optimized layout forced to 64-byte alignment
        let raw_layout = Layout::array::<T>(total_elements)
            .expect("Valid memory layout computation failure.");
        
        // Force alignment to 64 bytes to guarantee fast SIMD vector memory alignment
        let layout = raw_layout.align_to(64)
            .expect("Forced structural alignment mapping failure.");

        // 3. Request the exact memory block directly from the global heap allocator
        let base_ptr = unsafe { alloc(layout) as *mut T };
        if base_ptr.is_null() {
            panic!("System OOM: Low-level FramePool raw allocation failed.");
        }

        // 4. Safely initialize memory slots using T::default() rather than bitwise zeroes
        // to fully respect custom type initialization states.
        let default_val = T::default();
        unsafe {
            for i in 0..total_elements {
                ptr::write(base_ptr.add(i), default_val);
            }
        }

        Self {
            base_ptr,
            total_elements,
            frame_size,
            layout,
            available_indices,
        }
    }

    /// Checkout a buffer segment cleanly using immutable shared access (`&self`).
    /// 100% Real-time safe. Zero locks, zero allocations, completely free of LLVM aliasing UB.
    #[inline]
    pub fn checkout(&self) -> Option<FrameGuard<'_, T>> {
        let slot_idx = self.available_indices.pop()?;
        let start_offset = slot_idx * self.frame_size;
        
        // Safety:
        // 1. `slot_idx` is uniquely leased to this thread by the atomic MPMC pop.
        // 2. The sub-slice is derived via offset calculation from an explicitly aligned raw pointer base.
        // 3. The lifetime of the returned slice is structurally bound to the FramePool lifetime.
        unsafe {
            let frame_ptr = self.base_ptr.add(start_offset);
            let slice = std::slice::from_raw_parts_mut(frame_ptr, self.frame_size);
            
            Some(FrameGuard {
                pool: self,
                slot_idx,
                slice,
            })
        }
    }
}

impl<T> Drop for FramePool<T> {
    fn drop(&mut self) {
        unsafe {
            // Drop individual elements inside the raw allocation block.
            // Note: Since T: Copy, the compiler recognizes this loop as a dead no-op 
            // and strips it completely from the final optimized release binary.
            for i in 0..self.total_elements {
                ptr::drop_in_place(self.base_ptr.add(i));
            }
            // Reclaim the memory block using the exact same Layout profile used during birth
            dealloc(self.base_ptr as *mut u8, self.layout);
        }
    }
}