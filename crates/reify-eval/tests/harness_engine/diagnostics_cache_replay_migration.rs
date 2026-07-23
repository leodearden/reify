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

// ─────────────────────────────────────────────────────────────────────────
// B1 / B5 — per-cell runtime diagnostics replayed on cache-hit serves (step-3).
// ─────────────────────────────────────────────────────────────────────────

/// Count the `W_FIELD_OUT_OF_BOUNDS` warnings in a diagnostics slice (the μ
/// done-criteria filter: `Severity::Warning && code == FieldOutOfBounds`).
fn count_field_oob_warnings(diagnostics: &[Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.code == Some(DiagnosticCode::FieldOutOfBounds)
        })
        .count()
}

/// B1 (PRD §7 B1): a `W_FIELD_OUT_OF_BOUNDS` warning surfaced by a cold
/// `eval()` must RESURFACE on a subsequent warm `eval_cached()` cache-hit serve
/// of the SAME engine — replayed from `NodeCache.diagnostics`, not swallowed.
///
/// The swallow is structural: the warning fires at most once per field per
/// session (AtomicBool on the `SampledField` value), and on the warm serve the
/// consuming `let` cell is served CLEAN (fast-path / cache-reuse arm, no
/// re-`eval_expr`), so the sample never re-runs and even a re-sample would be
/// AtomicBool-suppressed. Replay from `NodeCache.diagnostics` is the ONLY way
/// to resurface it.
///
/// RED on base: `eval_cached` swallows the warning (count 0 on the warm serve).
/// GREEN after step-4 wires per-cell capture+store + clean-serve replay.
#[test]
fn field_oob_warning_resurfaces_on_repeated_reify_check() {
    let compiled = parse_and_compile_with_stdlib(OOB_SRC);
    let mut engine = make_simple_engine();

    // Cold eval — baseline: exactly one OOB warning fires.
    let cold = engine.eval(&compiled);
    assert_eq!(
        count_field_oob_warnings(&cold.diagnostics),
        1,
        "cold eval() must emit exactly one W_FIELD_OUT_OF_BOUNDS warning \
         (baseline); got: {:?}",
        cold.diagnostics
    );

    // Warm serve — the let cell(s) are served CLEAN via the fast-path /
    // cache-reuse arm (basis_version == VersionId(0), the version the first
    // eval() recorded entries at — the codebase's established warm-serve idiom,
    // cf. solver_pragma_dispatch.rs).
    let warm = engine.eval_cached(&compiled, VersionId(0));
    assert_eq!(
        count_field_oob_warnings(&warm.eval_result.diagnostics),
        1,
        "warm eval_cached() must REPLAY the W_FIELD_OUT_OF_BOUNDS warning from \
         NodeCache.diagnostics exactly once (RED on base: swallowed → 0); \
         got: {:?}",
        warm.eval_result.diagnostics
    );
}

/// B5 (PRD §7 B5): the same `W_FIELD_OUT_OF_BOUNDS` warning is emitted EXACTLY
/// ONCE in each of `eval`, `eval_cached`, AND `edit_check` — proving the
/// replayed-XOR-fresh double-emission guard holds across every serve mode.
///
/// - `eval` / `eval_cached` legs (unguarded OOB source, one engine): cold
///   `eval()` fires it fresh (1); warm `eval_cached()` REPLAYS it (1) — never
///   both on one serve, never doubled.
/// - `edit_check` leg (guarded OOB source, SEPARATE engine — the once-per-
///   session AtomicBool is per-`SampledField`-value, so a fresh emission needs
///   a fresh field value, i.e. a separate engine): cold `eval()` leaves the
///   guarded member inactive (0), then `edit_check(cond, true)` activates +
///   FRESHLY evaluates the sampling cell (1). Mirrors δ Fixture B.
///
/// RED on base: the `eval_cached` leg swallows the warning (0). GREEN after
/// step-4. The `edit_check` leg is already GREEN (δ) — B5 pins that it stays a
/// single, non-doubled emission alongside the newly-replayed warm serve.
#[test]
fn field_oob_warning_emitted_exactly_once_across_serve_modes() {
    // ── eval + eval_cached legs (unguarded, shared engine) ──
    let compiled = parse_and_compile_with_stdlib(OOB_SRC);
    let mut engine = make_simple_engine();

    let eval_result = engine.eval(&compiled);
    assert_eq!(
        count_field_oob_warnings(&eval_result.diagnostics),
        1,
        "eval() serve mode: exactly one OOB warning; got: {:?}",
        eval_result.diagnostics
    );

    let cached = engine.eval_cached(&compiled, VersionId(0));
    assert_eq!(
        count_field_oob_warnings(&cached.eval_result.diagnostics),
        1,
        "eval_cached() serve mode: exactly one OOB warning (replayed — not \
         swallowed, not doubled); got: {:?}",
        cached.eval_result.diagnostics
    );

    // ── edit_check leg (guarded, separate engine) ──
    let guard_compiled = parse_and_compile_with_stdlib(OOB_GUARD_SRC);
    let mut guard_engine = make_simple_engine();

    // Cold eval: cond=false → the guarded member is inactive, so no warning.
    let cold_guard = guard_engine.eval(&guard_compiled);
    assert_eq!(
        count_field_oob_warnings(&cold_guard.diagnostics),
        0,
        "cold eval() of the guarded source: inactive member emits no warning; \
         got: {:?}",
        cold_guard.diagnostics
    );

    // edit_check activates cond → the sampling cell is freshly evaluated and
    // the warning surfaces exactly once (fresh, via the drain).
    let cond_id = ValueCellId::new("S", "cond");
    let checked = guard_engine
        .edit_check(cond_id, Value::Bool(true))
        .expect("edit_check(cond, true) must succeed");
    assert_eq!(
        count_field_oob_warnings(&checked.diagnostics),
        1,
        "edit_check() serve mode: exactly one OOB warning (fresh, activated \
         member); got: {:?}",
        checked.diagnostics
    );
}
