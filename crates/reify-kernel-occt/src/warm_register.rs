//! PRD §5 B5 / I-3 (M-013 fix): static-init submission of a
//! [`WarmStartableRegistration`] declaring that `reify-kernel-occt` produces
//! warm-startable realization (geometry) state.
//!
//! Collected by `reify_types::WarmStartableRegistry::from_inventory()` at
//! scheduler init; consulted by the bidirectional coextension assertion in
//! `reify_runtime::assert_warm_startable_coextensive`.
//!
//! # Unconditional registration
//!
//! Both the `cfg(has_occt)` real `OcctKernel` (in `lib.rs`) and the
//! `cfg(not(has_occt))` stub `OcctKernel` (in `stubs.rs`) implement
//! [`reify_types::WarmStartable`] as the trait no-op (`warm_state` is `None`,
//! `with_warm_state` does nothing), so both build modes register alike. The
//! registration keeps `NodeKind::Realization` in the coextension registry,
//! which tracks kind presence, not impl quality. Mirrors the unconditional
//! submission pattern used by `reify-kernel-manifold/src/register.rs`.

use reify_ir::{NodeKind, WarmStartableRegistration};

inventory::submit! {
    WarmStartableRegistration { kind: NodeKind::Realization }
}
