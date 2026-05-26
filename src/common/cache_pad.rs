use std::sync::atomic::{AtomicUsize, AtomicBool, AtomicU32};

/// A hardware-isolated, cross-platform atomic wrapper designed to prevent false sharing.
/// Uses the compiler's native alignment rules to automatically calculate optimal trailing padding size.
#[repr(align(64))]
pub struct CachePadded<T> {
    pub value: T,
}

impl<T> CachePadded<T> {
    pub const fn new(value: T) -> Self {
        Self { value }
    }
}

// =========================================================================
// Compile-Time Layout Protections (Validates across 32-bit and 64-bit targets)
// =========================================================================

// Verify that an AtomicUsize instance safely forces a complete 64-byte footprint
const _: () = assert!(std::mem::align_of::<CachePadded<AtomicUsize>>() == 64);
const _: () = assert!(std::mem::size_of::<CachePadded<AtomicUsize>>() == 64);

// Verify that smaller atomic types (like a 1-byte AtomicBool) are safely padded out to 64 bytes
const _: () = assert!(std::mem::align_of::<CachePadded<AtomicBool>>() == 64);
const _: () = assert!(std::mem::size_of::<CachePadded<AtomicBool>>() == 64);

// Verify that explicit 32-bit types are safely padded out to 64 bytes
const _: () = assert!(std::mem::align_of::<CachePadded<AtomicU32>>() == 64);
const _: () = assert!(std::mem::size_of::<CachePadded<AtomicU32>>() == 64);