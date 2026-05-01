//! Stub `OpenVdbKernel` — all operations return descriptive errors.
//!
//! # Design templates
//!
//! `crates/reify-kernel-occt/src/stubs.rs` — `OcctKernel` stub pattern
//! (`_private: ()` field, `new()` constructor, all-error trait impl).
//! `crates/reify-test-support/src/mocks.rs` — `FailingMockGeometryKernel`.
//!
//! # v0.2 scope
//!
//! Real OpenVDB FFI is deferred to a follow-up task. This stub exists so the
//! `inventory::submit!` in `register.rs` has a factory that compiles. When
//! the follow-up task lands, the factory can switch to the real impl behind
//! `cfg(has_openvdb)` without changing the registration shape.

/// Stub OpenVDB kernel — all operations return descriptive errors.
///
/// The `_private: ()` field prevents external construction without [`Self::new`],
/// matching the OCCT stub pattern in
/// `crates/reify-kernel-occt/src/stubs.rs:25-27`.
///
/// Trivially `Send + Sync` (no interior mutability, no raw pointers — no
/// `unsafe impl` needed; the auto-derived impls fire).
pub struct OpenVdbKernel {
    _private: (),
}

impl OpenVdbKernel {
    /// Construct a new stub `OpenVdbKernel`.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for OpenVdbKernel {
    fn default() -> Self {
        Self::new()
    }
}
