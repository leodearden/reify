//! Task μ (#5062) — end-to-end done-criteria fixtures for replaying cached
//! per-cell diagnostics on fast-path (cache-hit) serves and wiring λ's shared
//! post-pass `DetectorRegistry` into every serve mode.
//!
//! PRD `docs/prds/v0_6/eval-cell-commit-substrate.md` §2.7 / §3 P4 / §7 B1,B5 /
//! §10 Q2, invariant INV-EVAL-3 ("each diagnostic has exactly one owner per
//! serve — replayed XOR freshly-pushed, never both").
//!
//! The substrate this file exercises:
//!
//! - κ (#5042) shipped `NodeCache.diagnostics` (the per-cell replay side-table)
//!   + `NodeCache::new_with_diagnostics` + a Clone that PRESERVES it.
//! - μ (this task) feeds that field on the write path
//!   (`CacheStore::set_node_diagnostics`), replays it at `eval_cached`'s
//!   clean-serve arms, and wires λ's `DetectorRegistry`/annotation-args
//!   post-pass into `eval` / `eval_cached` / `edit_check` uniformly.
//!
//! Two diagnostic classes, disjoint by construction (this is the
//! double-emission guard):
//!   1. Per-cell eval-time runtime diagnostics (`W_FIELD_OUT_OF_BOUNDS`,
//!      `W_FIELD_SAMPLED_INVALID_CONFIG`) — captured per-cell, stored in
//!      `NodeCache.diagnostics`, replayed on cache-hit serves.
//!   2. Post-pass detector diagnostics (MassProperties PSD registry,
//!      annotation-args materialization) — always freshly produced on every
//!      serve mode, never stored per-cell.
//!
//! Mirrors δ's `edit_param_cell_commit_migration.rs` harness idiom:
//! `.ri` source constants + `parse_and_compile_with_stdlib` / `compile_source`
//! + `make_simple_engine`, filtering `d.severity == Warning && d.code ==
//! Some(DiagnosticCode::FieldOutOfBounds)`.
#![allow(dead_code, unused_imports)]

use reify_core::{Diagnostic, DiagnosticCode, Severity, ValueCellId, VersionId};
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{
    compile_source, make_engine, make_simple_engine, parse_and_compile,
    parse_and_compile_with_stdlib,
};

// ─────────────────────────────────────────────────────────────────────────
// Shared `.ri` source constants (prerequisite pre-1).
// ─────────────────────────────────────────────────────────────────────────

/// Out-of-bounds sampled-field source (copied from `field_eval_tests.rs`'s
/// `sample_sampled_field_out_of_bounds_returns_undef_and_emits_warning_once`).
///
/// The field `f` is a `RegularGrid1` sampled over `[0.0m, 1.0m]`; all three
/// `let` cells (`oob_a`/`oob_b`/`oob_c`) query outside those bounds, so
/// `sample()` returns `Value::Undef` and emits exactly one
/// `W_FIELD_OUT_OF_BOUNDS` warning per session (suppressed after the first by
/// the `AtomicBool` on the `SampledField` value, sampled.rs). The warning
/// fires during whichever OOB cell evaluates first; on a warm `eval_cached`
/// serve every `let` cell is served CLEAN (cache-reuse arm) so the sample
/// never re-runs — replay from `NodeCache.diagnostics` is the only way to
/// resurface it (the μ done-criterion B1).
const OOB_SRC: &str = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(1.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0] } }

structure S {
    let oob_a = sample(f, 5.0m)
    let oob_b = sample(f, 10.0m)
    let oob_c = sample(f, 2.0m)
}
"#;

/// Guarded out-of-bounds source (mirrors δ Fixture B's `OOB_GUARD_SRC`): the
/// ACTIVE member of a `where cond { .. }` group samples field `f` out of
/// bounds. `cond` starts `false` (member inactive — no warning at cold eval),
/// and `edit_param(cond, true)` activates + freshly evaluates the sampling
/// cell, so the `W_FIELD_OUT_OF_BOUNDS` warning surfaces on the edit path.
/// Drives the `edit_check` leg of the B5 done-criterion.
const OOB_GUARD_SRC: &str = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(1.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0] } }

structure S {
    param cond : Bool = false
    where cond {
        let oob = sample(f, 5.0m)
    }
}
"#;

/// Annotation-args materialization FAILURE source (copied from
/// `engine_eval_commit_migration.rs`'s `ANNOTATION_ARGS_FAILURE_SRC`):
/// `@test_eval(1.0 > 0.0)` yields a `Bool` against the schema's expected
/// `Real`, so the annotation-args materialization post-pass replaces `BadS.it`
/// with `Value::Undef` and emits one `AnnotationEvalFailed` diagnostic.
///
/// `@test_eval` is a globally-registered test-only annotation schema (one
/// `AtMaterialization` arg named `value: Real`); it requires the non-stdlib
/// `parse_and_compile` + `make_engine` helpers (NOT the `_with_stdlib`
/// variants). This is the representative "cold-only post-pass now runs on the
/// warm/edit serve" parity signal (a natural non-PSD MassProperties instance
/// is not producible via `.ri`, per the plan's registry-parity note).
const ANNOTATION_ARGS_FAILURE_SRC: &str = r#"
@test_eval(1.0 > 0.0) structure def BadAnnoItem {
    param dummy : Real = 0
}
structure BadS {
    let it = BadAnnoItem()
}
"#;
