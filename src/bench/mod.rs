//! Microbenchmark entry points for the Rust -> JS call boundary.
//!
//! Everything here exists to put measured numbers behind the claims in `NAPI_OVERHEAD.md`, which
//! were derived by reading napi-rs's sources rather than by measuring anything. See
//! [`napi_call`] for the variant ladder and what each variant isolates.
//!
//! These are `#[napi]` exports and therefore ship in `index.d.ts`, exactly like the test-only
//! exports under [`crate::tests`]. They are driven by `benchmark/napi-call/run.js`.

pub mod napi_call;

/// A counting global allocator, compiled only under the `bench-alloc` feature so that ordinary
/// release builds pay nothing for it.
#[cfg(feature = "bench-alloc")]
pub mod counting_alloc;
