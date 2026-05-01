//! Stub `FidgetKernel` — scaffold for v0.2 task 2644.
//!
//! # Design templates
//!
//! `crates/reify-kernel-occt/src/stubs.rs` — `OcctKernel` stub pattern
//! (`_private: ()` field, `new()` constructor, all-error trait impl).
//! `crates/reify-test-support/src/mocks.rs:889` — `FailingMockGeometryKernel`.
//!
//! # v0.2 scope
//!
//! Real Fidget Rust JIT FFI is deferred to a follow-up task. The `GeometryKernel`
//! impl (all-error stub) arrives in step-4; this pre-1 scaffold provides only
//! the struct and constructors so the crate compiles.

/// Stub Fidget kernel — scaffold for v0.2 multi-kernel registration.
///
/// The `_private: ()` field prevents external construction without [`Self::new`],
/// matching the OCCT stub pattern in
/// `crates/reify-kernel-occt/src/stubs.rs:25-27`.
///
/// Trivially `Send + Sync` (no interior mutability, no raw pointers — no
/// `unsafe impl` needed; the auto-derived impls fire).
pub struct FidgetKernel {
    _private: (),
}

impl FidgetKernel {
    /// Construct a new stub `FidgetKernel`.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for FidgetKernel {
    fn default() -> Self {
        Self::new()
    }
}
