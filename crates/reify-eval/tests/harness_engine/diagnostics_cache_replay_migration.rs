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

// ─────────────────────────────────────────────────────────────────────────
// Shared-registry parity — the MassProperties PSD post-pass (λ's
// `MassPropertiesPsdDetector`, wired via `DetectorRegistry`) runs IDENTICALLY
// across eval / eval_cached / edit_check (step-5; wired by step-6).
// ─────────────────────────────────────────────────────────────────────────

/// A valid, kernel-free `MassProperties` model. `mp`'s inertia is the strictly
/// positive-definite diagonal `diag(0.10, 0.10, 0.10)` (all eigenvalues > 0 ⇒
/// PSD), so the MassProperties PSD post-pass leaves the cell intact and emits
/// no `DynamicsInertiaNotPSD` diagnostic on EVERY serve mode.
/// `MassProperties` / `point3` / `vec3` / `Frame3` resolve through the growing
/// prelude (cf. `examples/dynamics/closed_4bar_idyn.ri`), so no OCCT kernel is
/// needed — this exercises the detector's classification path directly, unlike
/// the OCCT-gated `body_mass_props(...)` fixtures in
/// `dynamics_body_mass_props.rs`.
///
/// A `touched : Bool` param (consumed by `flag`, so it is not an unused param)
/// gives the `edit_check` leg an editable cell. `mp` does NOT depend on
/// `touched`, so on the edit serve `mp` is carried forward from the baseline
/// snapshot (engine_edit.rs rebuilds the full `ValueMap` from
/// `new_snapshot.values`), yet the post-pass registry still re-scans it fresh —
/// exactly the cross-path invocation this fixture pins.
const VALID_MASS_PROPS_SRC: &str = r#"
structure MpModel {
    param touched : Bool = false
    let flag = touched
    let mp = MassProperties(mass: 1kg, com: point3(0mm, 0mm, 0mm), inertia: [[0.10, 0.0, 0.0], [0.0, 0.10, 0.0], [0.0, 0.0, 0.10]], origin: Frame3(origin: vec3(0mm, 0mm, 0mm), x_axis: vec3(0mm, 0mm, 0mm), y_axis: vec3(0mm, 0mm, 0mm), z_axis: vec3(0mm, 0mm, 0mm)))
}
"#;

/// Count `E_DynamicsInertiaNotPSD` diagnostics — the MassProperties PSD
/// post-pass's sole output code — in a diagnostics slice.
fn count_inertia_not_psd(diagnostics: &[Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DynamicsInertiaNotPSD))
        .count()
}

/// True iff `cell` holds an INTACT `MassProperties` `StructureInstance` — i.e.
/// the PSD post-pass did NOT replace it with `Value::Undef`.
fn mass_props_cell_intact(values: &reify_ir::ValueMap, cell: &ValueCellId) -> bool {
    matches!(
        values.get(cell),
        Some(Value::StructureInstance(d)) if d.type_name == "MassProperties"
    )
}

/// (step-5) The MassProperties PSD post-pass produces the SAME
/// MassProperties-cell result — intact cell + identical `DynamicsInertiaNotPSD`
/// diagnostic set — across `eval`, `eval_cached`, AND `edit_check`, proving no
/// path-divergence once λ's `DetectorRegistry` is wired into all three
/// (step-6).
///
/// **Characterization / no-path-divergence guard, green by construction.** On a
/// VALID (PSD) model every serve mode yields ZERO PSD diagnostics and an intact
/// cell, so this test cannot go RED on the registry wiring alone: a natural
/// non-PSD / malformed `MassProperties` inertia tensor is NOT producible via
/// `.ri` (physical inertia tensors are PSD by construction — plan
/// design-decision 5), so a POSITIVE end-to-end PSD-detection parity assertion
/// is not authorable in the harness. Positive non-PSD detection parity instead
/// rides on:
///   - step-6's inline-block deletion making λ's `MassPropertiesPsdDetector`
///     the SOLE PSD path on `eval` (pinned by the existing MassProperties PSD
///     eval tests `dynamics_body_mass_props.rs` / `structure_instance_e2e.rs`
///     staying green), and
///   - `detectors.rs`'s own cross-site unit coverage — the `site_eval` /
///     `site_eval_cached` / `site_edit_check` states in
///     `same_post_pass_state_yields_same_diagnostic_set` (detectors.rs:417-423)
///     — as the registry-runs-identically-per-path guarantee.
/// The representative `.ri`-authorable "cold-only post-pass now runs on the
/// warm / edit serve" RED signal is the annotation-args fixture in step-7.
#[test]
fn mass_properties_psd_registry_parity() {
    let mp_cell = ValueCellId::new("MpModel", "mp");

    // ── eval() baseline: intact MassProperties cell, zero PSD diagnostics ──
    let compiled = parse_and_compile_with_stdlib(VALID_MASS_PROPS_SRC);
    let mut engine = make_simple_engine();
    let cold = engine.eval(&compiled);
    assert!(
        mass_props_cell_intact(&cold.values, &mp_cell),
        "cold eval(): MpModel.mp must be an intact MassProperties instance; got \
         {:?} (diagnostics: {:?})",
        cold.values.get(&mp_cell),
        cold.diagnostics
    );
    let cold_psd = count_inertia_not_psd(&cold.diagnostics);
    assert_eq!(
        cold_psd, 0,
        "cold eval(): a PSD inertia tensor must emit no DynamicsInertiaNotPSD \
         diagnostic; got {:?}",
        cold.diagnostics
    );

    // ── eval_cached() parity (warm serve, SAME engine → real cache hit) ──
    let warm = engine.eval_cached(&compiled, VersionId(0));
    assert!(
        mass_props_cell_intact(&warm.eval_result.values, &mp_cell),
        "warm eval_cached(): MpModel.mp must stay an intact MassProperties \
         instance through the registry path; got {:?}",
        warm.eval_result.values.get(&mp_cell)
    );
    assert_eq!(
        count_inertia_not_psd(&warm.eval_result.diagnostics),
        cold_psd,
        "eval_cached() must produce the SAME MassProperties-cell PSD diagnostic \
         set as eval() (no path-divergence); got {:?}",
        warm.eval_result.diagnostics
    );

    // ── edit_check() parity (SEPARATE engine, prior eval() baseline) ──
    let edit_compiled = parse_and_compile_with_stdlib(VALID_MASS_PROPS_SRC);
    let mut edit_engine = make_simple_engine();
    let _ = edit_engine.eval(&edit_compiled); // establish the edit baseline
    let touched_id = ValueCellId::new("MpModel", "touched");
    let checked = edit_engine
        .edit_check(touched_id, Value::Bool(true))
        .expect("edit_check(touched, true) must succeed after a cold eval");
    assert!(
        mass_props_cell_intact(&checked.values, &mp_cell),
        "edit_check(): MpModel.mp must stay an intact MassProperties instance \
         through the registry path; got {:?}",
        checked.values.get(&mp_cell)
    );
    assert_eq!(
        count_inertia_not_psd(&checked.diagnostics),
        cold_psd,
        "edit_check() must produce the SAME MassProperties-cell PSD diagnostic \
         set as eval() (no path-divergence); got {:?}",
        checked.diagnostics
    );
}
