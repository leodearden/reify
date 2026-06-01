//! Drift guard: `reify_ir::geometry::KernelId` ≡ `reify_config::KernelId`.
//!
//! The two `KernelId` enums are an intentional, temporary duplication
//! (esc-4048-157). `reify-ir`'s B3 invariant forbids it from importing
//! `reify-config`, so a shared type cannot live in either crate today.
//! This guard is a **stopgap** until task B consolidates `KernelId` into
//! `reify-core`.
//!
//! ## What this file asserts
//!
//! 1. **Variant-set parity (compile-time, config side):** `config_to_ir`
//!    below is fully exhaustive over `reify_config::KernelId` (which is
//!    *not* `#[non_exhaustive]`).  Adding or removing a variant there
//!    breaks compilation immediately.
//!
//!    `reify_ir::KernelId` *is* `#[non_exhaustive]`, which prevents
//!    exhaustive external matching on that side.  Changes there are caught
//!    at runtime via `kernel_id_registry_names_match` (see below).
//!
//! 2. **Registry-name parity (runtime):** `kernel_id_registry_names_match`
//!    iterates the expected pairs and asserts
//!    `ir_variant.as_registry_name() == cfg_variant.to_string()` for every
//!    pair, plus checks the expected literal strings
//!    (`"fidget"`, `"gmsh"`, `"manifold"`, `"occt"`, `"openvdb"`).
//!    A count check against `reify_ir::geometry::KernelId::ALL` guards
//!    against the hand-written pairs array going stale.

use reify_config::KernelId as CfgKernelId;
use reify_ir::geometry::KernelId as IrKernelId;

// ---------------------------------------------------------------------------
// Bidirectional bridge
// ---------------------------------------------------------------------------

/// Map an IR-side `KernelId` to its config-side counterpart.
///
/// `reify_ir::KernelId` is `#[non_exhaustive]`; external crates must carry a
/// wildcard arm.  The wildcard panics with a clear message so that any future
/// variant added to `reify_ir::KernelId::ALL` (which drives the runtime test)
/// surfaces as a loud failure rather than silent wrong-path execution.
///
/// **Update this function** (and the `expected_pairs` array inside
/// `kernel_id_registry_names_match`) whenever a new variant is added to
/// `reify_ir::KernelId`.
fn ir_to_config(k: IrKernelId) -> CfgKernelId {
    match k {
        IrKernelId::Fidget => CfgKernelId::Fidget,
        IrKernelId::Gmsh => CfgKernelId::Gmsh,
        IrKernelId::Manifold => CfgKernelId::Manifold,
        IrKernelId::Occt => CfgKernelId::Occt,
        IrKernelId::OpenVdb => CfgKernelId::OpenVdb,
        k => panic!(
            "reify_ir::KernelId variant {:?} has no reify_config::KernelId counterpart; \
             update ir_to_config() and the expected_pairs array in \
             kernel_id_sync_guard.rs to keep the two enums in sync",
            k
        ),
    }
}

/// Map a config-side `KernelId` to its IR-side counterpart.
///
/// `reify_config::KernelId` is **not** `#[non_exhaustive]`, so this match is
/// fully exhaustive — no wildcard arm.  Adding or removing a variant in
/// `reify_config::KernelId` breaks compilation here, enforcing config-side
/// sync at compile time.  This function also serves as the **compile-time
/// exhaustiveness pin** for `reify_config::KernelId` — no separate pin is
/// needed.
///
/// This function is intentionally never called at runtime; its sole purpose
/// is to force a compile error when `reify_config::KernelId` gains or loses a
/// variant without a matching update here.
#[allow(dead_code)]
fn config_to_ir(k: CfgKernelId) -> IrKernelId {
    match k {
        CfgKernelId::Fidget => IrKernelId::Fidget,
        CfgKernelId::Gmsh => IrKernelId::Gmsh,
        CfgKernelId::Manifold => IrKernelId::Manifold,
        CfgKernelId::Occt => IrKernelId::Occt,
        CfgKernelId::OpenVdb => IrKernelId::OpenVdb,
    }
}

// ---------------------------------------------------------------------------
// Runtime registry-name parity test
// ---------------------------------------------------------------------------

/// Assert that every `reify_ir::KernelId` variant and its `reify_config`
/// counterpart agree on the canonical registry-name string.
///
/// The literal expected strings (`"fidget"` … `"openvdb"`) are the same
/// values pinned by each kernel adapter's `*_KERNEL_NAME` const (see
/// `crates/reify-kernel-*/src/register.rs`).
#[test]
fn kernel_id_registry_names_match() {
    // One entry per kernel variant.  This array is kept adjacent to the bridge
    // functions above; any stale entry is caught by the assertions below,
    // and missing config variants are caught at compile time by `config_to_ir`
    // (which is wildcard-free and doubles as the config-side exhaustiveness pin).
    let expected_pairs: [(IrKernelId, &str); 5] = [
        (IrKernelId::Fidget, "fidget"),
        (IrKernelId::Gmsh, "gmsh"),
        (IrKernelId::Manifold, "manifold"),
        (IrKernelId::Occt, "occt"),
        (IrKernelId::OpenVdb, "openvdb"),
    ];

    // Guard against expected_pairs going stale when IrKernelId::ALL grows.
    assert_eq!(
        IrKernelId::ALL.len(),
        expected_pairs.len(),
        "IrKernelId::ALL has {} variant(s) but expected_pairs has {}; \
         update kernel_id_sync_guard.rs (ir_to_config, config_to_ir, \
         and expected_pairs)",
        IrKernelId::ALL.len(),
        expected_pairs.len(),
    );

    for (ir_variant, literal) in expected_pairs {
        let cfg_variant = ir_to_config(ir_variant);

        // 1. reify-ir as_registry_name() == expected literal.
        //    Belt-and-suspenders: the IR side is also pinned by
        //    `reify-ir`'s own `registry_name_round_trips_exhaustively` test.
        assert_eq!(
            ir_variant.as_registry_name(),
            literal,
            "reify_ir::KernelId::{:?}.as_registry_name() == {:?} (expected {:?}); \
             update as_registry_name() in crates/reify-ir/src/geometry.rs",
            ir_variant,
            ir_variant.as_registry_name(),
            literal,
        );

        // 2. reify-config Display (to_string) == expected literal.
        //    Belt-and-suspenders: the config side is also pinned by
        //    `kernel_name_consistency` (config Display == adapter *_KERNEL_NAME consts).
        assert_eq!(
            cfg_variant.to_string().as_str(),
            literal,
            "reify_config::KernelId::{:?}.to_string() == {:?} (expected {:?}); \
             update Display impl in crates/reify-config/src/lib.rs",
            cfg_variant,
            cfg_variant.to_string(),
            literal,
        );

        // 3. Both sides agree with each other — the load-bearing cross-enum check.
        assert_eq!(
            ir_variant.as_registry_name(),
            cfg_variant.to_string().as_str(),
            "reify_ir::KernelId::{:?}.as_registry_name() ({:?}) != \
             reify_config::KernelId::{:?}.to_string() ({:?}): \
             registry-name drift detected between the two KernelId enums",
            ir_variant,
            ir_variant.as_registry_name(),
            cfg_variant,
            cfg_variant.to_string(),
        );
    }
}
