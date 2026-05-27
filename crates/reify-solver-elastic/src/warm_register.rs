//! PRD §5 B5 / I-3 (M-013 fix): static-init submission of a
//! [`WarmStartableRegistration`] declaring that `reify-solver-elastic`
//! produces warm-startable compute (CG-solver) state.
//!
//! Collected by `reify_types::WarmStartableRegistry::from_inventory()` at
//! scheduler init; consulted by the bidirectional coextension assertion in
//! `reify_runtime::assert_warm_startable_coextensive`.
//!
//! # Presence-only registration
//!
//! The registration is presence-only (no factory) per the registry's
//! `HashSet<NodeKind>` shape. The actual producer-side donation/restoration
//! handshake stays on [`crate::CgWarmState`]'s `into_opaque_state` /
//! `from_opaque_state` methods, which the engine integration calls
//! directly. The registry only needs to know that `NodeKind::Compute` is
//! warm-startable; it does not need to construct producers.
//!
//! Mirrors the unconditional submission pattern used by
//! `reify-kernel-occt/src/warm_register.rs`.

use reify_ir::{NodeKind, WarmStartableRegistration};

inventory::submit! {
    WarmStartableRegistration { kind: NodeKind::Compute }
}
