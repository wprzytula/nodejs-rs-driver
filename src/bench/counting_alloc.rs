//! A global allocator that counts Rust-side allocations, so that "this call performs no
//! allocation" can be asserted exactly instead of estimated.
//!
//! # Why not DHAT / valgrind
//!
//! DHAT intercepts every `malloc` in the process, which under Node means V8's zone allocators, the
//! parser, ICU and libuv as well. The single 8-byte `Vec<napi_value>` that napi-rs's
//! `Function::call` allocates per invocation is a rounding error in that total. Counting only
//! *Rust* allocations is both exactly the claim we want to test and completely noise-free, and it
//! works in-process with no external tooling.
//!
//! # Caveats when reading the numbers
//!
//! The counters are process-global and monotonic, and the tokio runtime built in
//! [`crate::init`](crate) has worker threads of its own. Any measurement that spans a driver
//! operation is therefore only meaningful with no other driver work in flight; a per-call
//! measurement of the benchmark variants in [`super::napi_call`] is safe because those touch
//! nothing but the N-API boundary.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

/// Forwards every request to [`System`], bumping two relaxed counters on the way in.
///
/// Only allocating operations are counted: `dealloc` is pure forwarding, since the question being
/// asked is "how many allocations did this call perform", not "what was the peak footprint".
/// `realloc` counts as one allocation, which is what it costs.
pub struct CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

/// Returns `(allocation count, total bytes allocated)` since the last
/// [`bench_alloc_reset`] call.
///
/// Both are returned as `i64` because that is what N-API's number type maps to; neither is
/// expected to approach the range where that matters.
#[napi]
pub fn bench_alloc_counters() -> (i64, i64) {
    (
        ALLOC_COUNT.load(Ordering::Relaxed) as i64,
        ALLOC_BYTES.load(Ordering::Relaxed) as i64,
    )
}

/// Zeroes both counters. Note that the reset itself, and the N-API call carrying its result back,
/// may allocate - so always reset immediately before the region of interest and read immediately
/// after it, and calibrate the constant overhead by measuring an empty region.
#[napi]
pub fn bench_alloc_reset() {
    ALLOC_COUNT.store(0, Ordering::Relaxed);
    ALLOC_BYTES.store(0, Ordering::Relaxed);
}
