use std::ops::{Deref, DerefMut};
use crate::memory::frame_pool::FramePool;

/// RAII handle that automatically returns the buffer block to the pool when dropped.
pub struct FrameGuard<'a, T> {
    pub(crate) pool: &'a FramePool<T>,
    pub(crate) slot_idx: usize,
    pub(crate) slice: &'a mut [T],
}

impl<'a, T> Deref for FrameGuard<'a, T> {
    type Target = [T];

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.slice
    }
}

impl<'a, T> DerefMut for FrameGuard<'a, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.slice
    }
}

impl<'a, T> Drop for FrameGuard<'a, T> {
    #[inline(always)]
    fn drop(&mut self) {
        // Return index to the lock-free queue
        let _ = self.pool.available_indices.push(self.slot_idx);
    }
}