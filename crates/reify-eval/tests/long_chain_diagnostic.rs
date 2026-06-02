//! Integration-level smoke for task 2646's long-chain diagnostic API surface.
//!
//! This file exercises every public item that `crates/reify-eval/src/lib.rs`
//! re-exports from `dispatcher.rs` for the long-chain warning subsystem. It is
//! deliberately a separate `tests/` integration file (not a `#[cfg(test)] mod`
//! inside the crate) so it can ONLY see what the crate actually re-exports —
//! a symbol that's `pub` inside `dispatcher.rs` but missing from the lib.rs
//! re-export line will fail to compile here, locking the public surface.
//!
//! Purpose, per the task plan: pin the lib.rs re-export contract so a
//! downstream caller (the eventual engine/kernel-registry timing-loop
//! consumer) can rely on the entire long-chain primitive set being
//! discoverable through the crate root, not buried inside the private
//! module path.

use std::time::Duration;

use reify_core::{DiagnosticCode, Severity};
use reify_eval::{
    DispatchPlan, LONG_CHAIN_DEFAULT_THRESHOLD_MS, LONG_CHAIN_MIN_STAGES,
    LONG_CHAIN_THRESHOLD_ENV_VAR, is_long_chain_realization, long_chain_diagnostic,
    long_chain_threshold_from_env, long_chain_threshold_from_env_value,
};
use reify_ir::{KernelId, ReprKind};

/// Smoke-test every long-chain item re-exported through the crate root.
///
/// A compile-time discovery check — if `lib.rs` drops one of the new
/// symbols from its `pub use dispatcher::{...}` line, this file fails to
/// build, surfacing the regression at the public-surface boundary rather
/// than at the eventual downstream consumer.
#[test]
fn lib_re_exports_long_chain_api() {
    // Constants — pin the re-exported literal values so a typo or rename
    // in either the const decl or the re-export line is caught here.
    assert_eq!(LONG_CHAIN_DEFAULT_THRESHOLD_MS, 500);
    assert_eq!(
        LONG_CHAIN_THRESHOLD_ENV_VAR,
        "REIFY_LONG_CHAIN_THRESHOLD_MS"
    );
    assert_eq!(LONG_CHAIN_MIN_STAGES, 2);

    // Build a 3-stage plan that trips both gates: stage-count > 2 and
    // elapsed > threshold. Confirms `DispatchPlan` is constructable through
    // the re-export and that `is_long_chain_realization` accepts it.
    let plan = DispatchPlan {
        kernel: "manifold".to_string(),
        conversions: vec![
            (KernelId::Fidget, ReprKind::BRep, ReprKind::Mesh),
            (KernelId::Gmsh, ReprKind::Mesh, ReprKind::Sdf),
            (KernelId::Manifold, ReprKind::Sdf, ReprKind::Voxel),
        ],
    };
    let threshold = Duration::from_millis(500);
    let elapsed = Duration::from_millis(750);

    assert!(
        is_long_chain_realization(&plan, elapsed, threshold),
        "is_long_chain_realization must be re-exported and gate-passing for 3 stages + 750ms",
    );

    // Builder produces Some(diag) with the expected severity + code when
    // both gates pass.
    let diag = long_chain_diagnostic(&plan, elapsed, threshold)
        .expect("long_chain_diagnostic must be re-exported and emit when both gates pass");
    assert_eq!(diag.severity, Severity::Warning);
    assert_eq!(diag.code, Some(DiagnosticCode::LongChainRealization));

    // Env-var resolver: the seam (`Option<&str>`) and the production
    // wrapper (`std::env::var`-driven) must both be re-exported.
    assert_eq!(
        long_chain_threshold_from_env_value(None),
        Duration::from_millis(LONG_CHAIN_DEFAULT_THRESHOLD_MS),
        "seam re-export must resolve unset to default",
    );
    // Production wrapper smoke: prove the symbol is callable through the
    // crate root. We deliberately do NOT assert anything about the
    // returned `Duration` because the env state isn't test-controlled —
    // a developer running with `REIFY_LONG_CHAIN_THRESHOLD_MS=0` (an
    // explicit override, however unusual) would fail an `> ZERO`
    // assertion despite no regression. The deterministic parser branches
    // (None / "" / parseable / unparseable) are pinned by the seam tests
    // in `dispatcher.rs::tests`; this call only verifies that the
    // wrapper compiles and runs without panicking.
    let _resolved = long_chain_threshold_from_env();
}
