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
//! `cfg(not(has_occt))` stub `OcctKernel` (in `stubs.rs`) impl
//! [`reify_types::WarmStartable`], so the registration is meaningful in both
//! build modes. Mirrors the unconditional submission pattern used by
//! `reify-kernel-manifold/src/register.rs`. The registry tracks kind presence,
//! not impl quality.

use reify_types::{NodeKind, WarmStartableRegistration};

inventory::submit! {
    WarmStartableRegistration { kind: NodeKind::Realization }
}
