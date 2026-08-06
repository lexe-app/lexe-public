//! Basic heap allocator instrumentation for SGX enclaves.
//!
//! Non-SGX binaries should use something like `jemalloc`, which tracks many
//! more useful stats more efficiently. The current SGX allocator is just
//! `SpinMutex<Dlmalloc<_>>`, which is pretty awful for normal multi-threaded
//! workloads.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(target_env = "sgx")]
unsafe extern "C" {
    /// A link-time constant for the fixed SGX enclave `heap-size` defined in
    /// e.g. `node/Cargo.toml`.
    ///
    /// See:
    /// - <https://github.com/rust-lang/rust/blob/main/library/std/src/sys/pal/sgx/abi/mem.rs#L18>
    /// - <https://github.com/rust-lang/rust/blob/main/library/std/src/sys/pal/sgx/abi/entry.S#L48>
    static HEAP_SIZE: usize;
}

/// Rust's [`System`] global allocator instrumented with allocation counters.
///
/// The counters track requested bytes, excluding allocator metadata, alignment
/// padding, and fragmentation. All instances contribute to the same counters;
/// install exactly one as the process global allocator.
pub struct InstrumentedSystemAllocator {
    _private: (),
}

/// Allocator metrics
struct Counters {
    current: AtomicUsize,
    max: AtomicUsize,
}

/// Global allocator metrics counters
static COUNTERS: Counters = Counters::new();

/// A snapshot of the global allocator's requested-byte counters.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct AllocatorStats {
    /// Current live bytes allocated on the heap.
    pub current_bytes: usize,
    /// The fixed SGX heap size, or `None` outside SGX.
    pub heap_size: Option<usize>,
    /// The max number of live bytes ever allocated during this process' life.
    pub max_bytes: usize,
}

/// Returns a snapshot of the allocator stats.
pub fn stats() -> AllocatorStats {
    let (current_bytes, max_bytes) = COUNTERS.snapshot();

    AllocatorStats {
        current_bytes,
        heap_size: heap_size(),
        max_bytes,
    }
}

/// The available memory we can heap allocate in.
fn heap_size() -> Option<usize> {
    cfg_if::cfg_if! {
        if #[cfg(target_env = "sgx")] {
            // SAFETY: `HEAP_SIZE` is a link-time constant for the SGX target.
            Some(unsafe { HEAP_SIZE })
        } else {
            None
        }
    }
}

// --- impl InstrumentedSystemAllocator --- //

impl InstrumentedSystemAllocator {
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

unsafe impl GlobalAlloc for InstrumentedSystemAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller upholds `GlobalAlloc::alloc`'s contract.
        let ptr = unsafe { System.alloc(layout) };
        COUNTERS.record_allocation(ptr, layout.size());
        ptr
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller upholds `GlobalAlloc::alloc_zeroed`'s contract.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        COUNTERS.record_allocation(ptr, layout.size());
        ptr
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: The caller upholds `GlobalAlloc::dealloc`'s contract.
        unsafe { System.dealloc(ptr, layout) };
        COUNTERS.remove(layout.size());
    }

    #[inline]
    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> *mut u8 {
        // SAFETY: The caller upholds `GlobalAlloc::realloc`'s contract.
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        COUNTERS.record_reallocation(new_ptr, layout.size(), new_size);
        new_ptr
    }
}

// --- impl Counters --- //

impl Counters {
    const fn new() -> Self {
        Self {
            current: AtomicUsize::new(0),
            max: AtomicUsize::new(0),
        }
    }

    #[inline]
    fn record_allocation(&self, ptr: *mut u8, size: usize) {
        if !ptr.is_null() {
            self.add(size);
        }
    }

    #[inline]
    fn record_reallocation(
        &self,
        ptr: *mut u8,
        old_size: usize,
        new_size: usize,
    ) {
        if ptr.is_null() {
            return;
        }

        if new_size > old_size {
            self.add(new_size - old_size);
        } else if new_size < old_size {
            self.remove(old_size - new_size);
        }
    }

    #[inline]
    fn add(&self, size: usize) {
        let current = self.current.fetch_add(size, Ordering::Relaxed) + size;
        self.max.fetch_max(current, Ordering::Relaxed);
    }

    #[inline]
    fn remove(&self, size: usize) {
        self.current.fetch_sub(size, Ordering::Relaxed);
    }

    fn snapshot(&self) -> (usize, usize) {
        let current = self.current.load(Ordering::Relaxed);
        // `add` updates `current` first, so include an in-flight max update.
        let max = self.max.load(Ordering::Relaxed).max(current);
        (current, max)
    }
}

#[cfg(test)]
mod tests {
    use std::ptr::{self, NonNull};

    use super::*;

    #[test]
    fn tracks_allocation_lifecycle() {
        let counters = Counters::new();
        let ptr = NonNull::<u8>::dangling().as_ptr();

        counters.record_allocation(ptr::null_mut(), 100);
        assert_eq!(counters.snapshot(), (0, 0));

        counters.record_allocation(ptr, 20);
        counters.record_allocation(ptr, 30);
        assert_eq!(counters.snapshot(), (50, 50));

        counters.remove(20);
        assert_eq!(counters.snapshot(), (30, 50));

        counters.record_reallocation(ptr::null_mut(), 30, 100);
        assert_eq!(counters.snapshot(), (30, 50));

        counters.record_reallocation(ptr, 30, 60);
        assert_eq!(counters.snapshot(), (60, 60));

        counters.record_reallocation(ptr, 60, 10);
        assert_eq!(counters.snapshot(), (10, 60));

        counters.remove(10);
        assert_eq!(counters.snapshot(), (0, 60));
    }
}
