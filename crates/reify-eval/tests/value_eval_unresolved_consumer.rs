//! R1a positive-signal integration tests for E_EVAL_UNRESOLVED at geometry
//! typed-consumption sites (task #4651).
//!
//! ## What these tests pin
//!
//! On the pure value-eval surface (`Engine::eval` / `eval_cached` / `check`,
//! kernel-less), a geometry **consumer** cell (one whose `default_expr` is a
//! FunctionCall to a recognised consumer name such as `adjacent_faces` or
//! `normal`) that stays at `Value::Undef` must emit exactly one
//! `DiagnosticCode::EvalUnresolved` at `Severity::Error` with a non-empty
//! label span.
//!
//! ## TDD arc
//!
//! **Step-3 (RED):** positive-signal eval + check tests — FAIL until step-4
//! wires `detect_unresolved_geometry_consumers` into `eval()`.
//!
//! **Step-4 (GREEN):** after wiring into `eval()`, step-3 tests pass.
//!
//! **Step-5 (RED):** `eval_cached` parity + editor-incompleteness guard tests.
//!
//! **Step-6 (GREEN):** after wiring into `eval_cached()`, step-5 tests pass.
//!
//! **Step-7 (RED/GREEN):** build-path scope-guard — GREEN as soon as step-4/6
//! correctly scope the detector to the eval surface only.
//!
//! ## Fixture source
//!
//! The `CONSUMER_SRC` structure contains two consumer cells:
//! - `neighbors = adjacent_faces(b, top_face)` — relational topology query
//! - `face_n    = normal(b, pt)`               — surface-normal query
//!
//! Both are geometry-typed builtins that require a realized kernel handle and
//! therefore remain `Value::Undef` on the pure value-eval surface.  The
//! construction / leaf-selector cells (`b`, `top_face`, `pt`) do NOT fire.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, Severity};
use reify_eval::Engine;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

/// Fixture source: a structure with two geometry consumer cells.
///
/// **Consumer cells** (should emit EvalUnresolved on kernel-less eval):
/// - `neighbors` — `adjacent_faces(b, top_face)`
/// - `face_n`    — `normal(b, pt)`
///
/// **Non-consumer cells** (should NOT emit EvalUnresolved):
/// - `b`         — `box(...)` constructor → symbolic GeometryHandle
/// - `zdir`      — `vec3(...)` → dimensionless Vec3
/// - `tol`       — `1deg` angle literal
/// - `top_face`  — `single(faces_by_normal(...))` → Undef (not a consumer)
/// - `pt`        — `point3(...)` → Point<Length>
const CONSUMER_SRC: &str = r#"structure def ConsumerTest {
    let b        = box(10mm, 20mm, 30mm)
    let zdir     = vec3(0.0, 0.0, 1.0)
    let tol      = 1deg
    let top_face = single(faces_by_normal(b, zdir, tol))
    let neighbors = adjacent_faces(b, top_face)
    let pt       = point3(0mm, 0mm, 5mm)
    let face_n   = normal(b, pt)
}"#;

// ─────────────────────────────────────────────────────────────────────────────
// Step-3 tests: positive signal — eval() and check()
// ─────────────────────────────────────────────────────────────────────────────

/// SIGNAL — `Engine::eval` (kernel-less) must emit two `EvalUnresolved` errors,
/// one for each geometry consumer cell (`neighbors`, `face_n`), with:
/// - `code == Some(DiagnosticCode::EvalUnresolved)`
/// - `severity == Severity::Error`
/// - at least one label with a non-empty span (from the `let`-decl byte range)
///
/// **RED** until step-4 wires `detect_unresolved_geometry_consumers` into
/// `eval()`.  Currently both consumer cells stay at `Value::Undef` silently.
#[test]
fn eval_emits_eval_unresolved_for_consumer_cells() {
    let compiled = parse_and_compile_with_stdlib(CONSUMER_SRC);

    // The fixture must compile cleanly — no Error-severity compile diagnostics.
    let compile_errors = errors_only(&compiled);
    assert!(
        compile_errors.is_empty(),
        "CONSUMER_SRC must compile with no errors; got: {:#?}",
        compile_errors
    );

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result = engine.eval(&compiled);

    // Collect only EvalUnresolved diagnostics at Error severity.
    let eval_unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::EvalUnresolved) && d.severity == Severity::Error
        })
        .collect();

    assert_eq!(
        eval_unresolved.len(),
        2,
        "expected exactly 2 EvalUnresolved errors (one for `neighbors`, one for `face_n`); \
         got {} — full diagnostics: {:#?}",
        eval_unresolved.len(),
        result.diagnostics
    );

    // Each EvalUnresolved diagnostic must carry at least one non-empty label span.
    for diag in &eval_unresolved {
        assert!(
            !diag.labels.is_empty(),
            "EvalUnresolved diagnostic must carry at least one label; got: {diag:#?}"
        );
        let primary_span = diag.labels[0].span;
        assert!(
            !primary_span.is_empty(),
            "EvalUnresolved diagnostic's primary label must have a non-empty span \
             (should be the let-decl byte range); got empty span. Diagnostic: {diag:#?}"
        );
    }

    // The consumer cells themselves must be Value::Undef — confirming the
    // detector did NOT synthesise a false Value.
    let neighbors_id = reify_core::ValueCellId::new("ConsumerTest", "neighbors");
    let face_n_id = reify_core::ValueCellId::new("ConsumerTest", "face_n");
    assert_eq!(
        result.values.get(&neighbors_id),
        Some(&Value::Undef),
        "`neighbors` must remain Value::Undef in kernel-less eval"
    );
    assert_eq!(
        result.values.get(&face_n_id),
        Some(&Value::Undef),
        "`face_n` must remain Value::Undef in kernel-less eval"
    );
}

/// WITNESS — `Engine::check` (= `eval` + post-solve constraint pass) must
/// surface the same `EvalUnresolved` errors in `CheckResult.diagnostics`.
///
/// This pins the `reify check` CLI witness surface: a plain geometry module
/// with consumer cells emits EvalUnresolved through `Engine::check()`.
///
/// **RED** until step-4 wires the detector into `eval()` (check() calls
/// eval() internally, so wiring eval() is sufficient).
#[test]
fn check_surfaces_eval_unresolved_for_consumer_cells() {
    let compiled = parse_and_compile_with_stdlib(CONSUMER_SRC);
    assert!(
        errors_only(&compiled).is_empty(),
        "CONSUMER_SRC must compile cleanly"
    );

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result = engine.check(&compiled);

    let eval_unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::EvalUnresolved) && d.severity == Severity::Error
        })
        .collect();

    assert!(
        !eval_unresolved.is_empty(),
        "check() must surface at least one EvalUnresolved error for consumer cells; \
         got none — full diagnostics: {:#?}",
        result.diagnostics
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-5 tests: eval_cached parity + editor-incompleteness guard
// ─────────────────────────────────────────────────────────────────────────────

/// PARITY — `Engine::eval_cached` must emit the same `EvalUnresolved` errors
/// as `eval()` for the consumer fixture.
///
/// **RED** until step-6 wires `detect_unresolved_geometry_consumers` into
/// `eval_cached()`.
#[test]
fn eval_cached_emits_eval_unresolved_parity_with_eval() {
    use reify_core::VersionId;

    let compiled = parse_and_compile_with_stdlib(CONSUMER_SRC);
    assert!(errors_only(&compiled).is_empty(), "CONSUMER_SRC must compile cleanly");

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result = engine.eval_cached(&compiled, VersionId(1));

    let eval_unresolved: Vec<_> = result
        .eval_result
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::EvalUnresolved) && d.severity == Severity::Error
        })
        .collect();

    assert_eq!(
        eval_unresolved.len(),
        2,
        "eval_cached() must emit 2 EvalUnresolved errors (parity with eval()); \
         got {} — full diagnostics: {:#?}",
        eval_unresolved.len(),
        result.eval_result.diagnostics
    );
}

/// EDITOR-INCOMPLETENESS GUARD (DD-4) — a source containing ONLY construction
/// and leaf-selector sites with NO consumer must produce ZERO EvalUnresolved
/// diagnostics from both `eval()` and `eval_cached()`.
///
/// A bare `let top_face = single(faces_by_normal(b, zdir, tol))` or
/// `let all_f = faces(b)` are editor-incompleteness states (the user has not
/// yet called a consumer), not errors.
///
/// This test must remain **GREEN throughout** — it is a scope guard that
/// verifies the allow-list does not accidentally fire on construction sites.
const CONSTRUCTION_ONLY_SRC: &str = r#"structure def ConstructionOnly {
    let b      = box(10mm, 20mm, 30mm)
    let zdir   = vec3(0.0, 0.0, 1.0)
    let tol    = 1deg
    let top_face = single(faces_by_normal(b, zdir, tol))
    let all_f  = faces(b)
}"#;

#[test]
fn eval_no_unresolved_error_for_construction_only_source() {
    use reify_core::VersionId;

    let compiled = parse_and_compile_with_stdlib(CONSTRUCTION_ONLY_SRC);
    assert!(
        errors_only(&compiled).is_empty(),
        "CONSTRUCTION_ONLY_SRC must compile cleanly"
    );

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);

    // eval() path
    let eval_result = engine.eval(&compiled);
    let unresolved_eval: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::EvalUnresolved))
        .collect();
    assert!(
        unresolved_eval.is_empty(),
        "eval() must emit ZERO EvalUnresolved for construction-only source; \
         got: {:#?}",
        unresolved_eval
    );

    // eval_cached() path (same module, next version)
    let cached_result = engine.eval_cached(&compiled, VersionId(2));
    let unresolved_cached: Vec<_> = cached_result
        .eval_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::EvalUnresolved))
        .collect();
    assert!(
        unresolved_cached.is_empty(),
        "eval_cached() must emit ZERO EvalUnresolved for construction-only source; \
         got: {:#?}",
        unresolved_cached
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-7 test: build-path scope guard + §6 no-double-fire
// ─────────────────────────────────────────────────────────────────────────────

/// BUILD-PATH SCOPE GUARD — `engine.build()` on the consumer source must emit
/// **ZERO** `EvalUnresolved` diagnostics.
///
/// `engine.build()` drives realisation through `run_unified_pass` (in
/// `engine_fixpoint`) and NEVER calls `eval()` / `eval_cached()`, so the
/// value-eval detector (`detect_unresolved_geometry_consumers`) is unreachable
/// on the build path by construction.
///
/// A plain consumer idiom (no constraints / auto-params) does not trigger
/// `run_unified_pass`'s own `EvalUnresolved` emission either, so the total
/// EvalUnresolved count is 0 — the disjointness invariant (PRD §6).
///
/// This test is GREEN as soon as step-4/6 land the correctly-scoped detector.
/// It will turn RED only if a future refactor accidentally routes build()
/// through eval() or eval_cached().
#[test]
fn build_path_emits_zero_eval_unresolved_for_consumer_source() {
    let compiled = parse_and_compile_with_stdlib(CONSUMER_SRC);
    assert!(errors_only(&compiled).is_empty(), "CONSUMER_SRC must compile cleanly");

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result = engine.build(&compiled, ExportFormat::Step);

    let eval_unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::EvalUnresolved))
        .collect();

    assert!(
        eval_unresolved.is_empty(),
        "build() must emit ZERO EvalUnresolved (detector is eval-surface-only, \
         not reachable on the build path); got: {:#?}",
        eval_unresolved
    );
}
