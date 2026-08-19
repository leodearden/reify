//! Task #5467 (PRD2 α, step-15(b)): `detect_underdetermined` must not
//! false-positive on an INSTANCE-PATH-keyed auto.
//!
//! Registered from `harness_engine.rs` with an explicit `#[path]` — see the
//! anti-re-accretion rationale there.
//!
//! # Why this is compiled from `.ri` source and not built with a builder
//!
//! The defect lives in a mismatch between TWO ID NAMESPACES: the compiler mints
//! an auto `ValueCellDecl` keyed by INSTANCE PATH at
//! `crates/reify-compiler/src/entity.rs:3025` (construction named-arg) and
//! `:3130` (sub-override), while a constraint's read of that same cell
//! normalises to the declaring TEMPLATE's id. A `TopologyTemplateBuilder`
//! fixture would only imitate the spelling `entity.rs` happens to emit today,
//! so it would keep passing if that spelling ever changed — pinning the
//! imitation instead of the fact. Compiling real source makes the COMPILER the
//! source of truth for both ids.
//!
//! # Why the assertion is a COUNT of `Underdetermined`, not "no errors"
//!
//! `W_UNDERDETERMINED` is a WARNING. The nearest existing e2e over the same
//! binding sites (`tests/auto_binding_sites_remaining_resolution.rs`) filters
//! with `errors_only(...)`, so two brand-new user-visible warnings passed
//! through it silently — which is precisely how this regression reached a
//! branch tip with a green suite.
//!
//! # Why the count assertion is not ENOUGH on its own (review #5467-12)
//!
//! Zero `Underdetermined` diagnostics is a NECESSARY but not a SUFFICIENT
//! signal, and asserting only the count green-lights the exact defect the
//! widening can introduce: layer 4 (`detect_underdetermined`) reads the FORWARD
//! walk while layer 1 (`filter_constraints_reading_autos`) reads the REVERSE
//! one, so widening one direction alone SUPPRESSES the warning without ever
//! solving the auto — strictly worse than the un-widened `main`, which at
//! least said so loudly. `no_underdetermined_for_either_instance_path_minting_site`
//! below also cannot reach the reverse direction at all, because it pins its
//! autos with a DIRECT read.
//!
//! `let_indirected_instance_path_autos_resolve_through_their_lets` therefore
//! carries the load-bearing half: the SAME two minting sites, pinned only
//! THROUGH a `let`, asserted on their RESOLVED VALUES. The zero-warning check
//! on that fixture is kept as a separate, secondary assertion
//! (`..._emit_no_underdetermined_warning`) so a regression reports which of the
//! two halves broke.
//!
//! # Why no connect-param site
//!
//! Deliberately excluded. `AllFourSites.__connector_0.gain` carries a SEPARATE,
//! PRE-EXISTING false positive (by design D5 the parent cannot name
//! `__connector_N`, so no constraint reads it under any spelling — true on
//! `main` too), and folding it in would make this count track two unrelated
//! defects at once. It is covered, and pinned as the expected baseline, by
//! `tests/auto_binding_sites_remaining_resolution.rs`.

use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_eval::{Engine, EvalResult};
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

/// Both instance-path minting sites side by side, each pinned by a constraint
/// that reads the SAME instance-path spelling the declaration uses.
///
/// Shape lifted from `examples/auto_binding_sites.ri` sites (1) and (2), minus
/// the connect-param site (see the module header).
const BOTH_MINTING_SITES: &str = r#"
structure Bearing {
    param bore : Length = 10mm
}

structure Bolt {
    param length : Length = 5mm
}

// (1) SUB-OVERRIDE site — `entity.rs:3130` mints the decl
//     `InstancePathAutos.b.bore`.
// (2) CONSTRUCTION named-arg site — `entity.rs:3025` mints the decl
//     `InstancePathAutos.bolt.length`.
structure InstancePathAutos {
    sub b : Bearing { bore = auto }
    constraint self.b.bore == 10mm

    sub bolt = Bolt(length: auto)
    constraint self.bolt.length == 10mm
}
"#;

/// Neither instance-path auto is free: each is pinned by a constraint reading
/// the same instance-path spelling. Zero `Underdetermined` diagnostics.
///
/// RED until step-17 makes `CellReadIndex::read_closure` a genuine SUPERSET.
/// Today the closure keeps only the NORMALISED spelling (`Bearing.bore`,
/// `Bolt.length`) while `detect_underdetermined` matches the RAW declaration id
/// (`InstancePathAutos.b.bore`, `InstancePathAutos.bolt.length`), so both are
/// reported free on every `reify check`.
#[test]
fn no_underdetermined_for_either_instance_path_minting_site() {
    let compiled = compile_source_with_stdlib(BOTH_MINTING_SITES);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "the instance-path fixture must compile without errors — this test \
         pins a DIAGNOSTIC COUNT, so a compile error would silently change \
         what is being counted; got {errors:#?}",
    );

    // `Engine::new(.., None)` is the literal `reify check` entry point:
    // `detect_underdetermined` runs OUTSIDE the `has_active_solver` gate, so no
    // amount of solver-side fixing can clear what it emits.
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&compiled);

    let under: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Underdetermined))
        .collect();

    assert_eq!(
        under.len(),
        0,
        "both autos here are pinned by a constraint reading the SAME \
         instance-path spelling the compiler used to declare them \
         (entity.rs:3130 sub-override, :3025 construction named-arg), so \
         NEITHER is underdetermined. Each diagnostic below is a false positive \
         caused by the read closure discarding the raw spelling; got {under:#?}",
    );
}

/// Tolerance for the resolved autos.
///
/// Each auto below is pinned by a single linear `Eq` residual with a unique
/// root, and `DimensionalSolver` accepts only a summed-squared-residual
/// `<= FEASIBILITY_THRESHOLD = 1e-12` (`crates/reify-constraints/src/solver.rs`),
/// which bounds each `|delta|` well under this. IMPLIED BY THE SOLVE SUCCEEDING,
/// not fitted to an observed run — a failure here is a convergence signal to
/// investigate, never an invitation to widen the constant.
const SOLVER_TOL: f64 = 1e-9;

/// The SAME two minting sites as [`BOTH_MINTING_SITES`], but with every auto
/// pinned ONLY THROUGH A `let`.
///
/// This is the shape that actually routes through `CellReadIndex::cells_reaching`
/// — the reverse direction of the index. The direct-read fixture above never
/// reaches it, because a constraint that names the auto itself is admitted by
/// layer 1's `auto_ids` disjunct without any reverse walk.
///
/// Each `let` has a unique root, so the assertions below are on exact values,
/// not on "some value was produced": `margin == 8mm` with `margin = bore - 2mm`
/// forces `bore = 10mm`; `slack == 9mm` with `slack = length - 1mm` forces
/// `length = 10mm`.
const BOTH_MINTING_SITES_LET_INDIRECTED: &str = r#"
structure Bearing {
    param bore : Length = 10mm
}

structure Bolt {
    param length : Length = 5mm
}

// (1) SUB-OVERRIDE site — `entity.rs:3130` mints the decl `LetIndirect.b.bore`.
// (2) CONSTRUCTION named-arg site — `entity.rs:3025` mints the decl
// `LetIndirect.bolt.length`.
structure LetIndirect {
    sub b : Bearing { bore = auto }
    let margin = self.b.bore - 2mm
    constraint margin == 8mm

    sub bolt = Bolt(length: auto)
    let slack = self.bolt.length - 1mm
    constraint slack == 9mm
}
"#;

/// Compile + eval `src` through the REAL `SolverRegistry::production()`, and
/// assert the eval produced zero `Severity::Error` diagnostics.
///
/// The error assertion is load-bearing: a non-converged solve surfaces as a
/// `constraints could not be satisfied` error, and the value assertions must
/// not be able to pass while the solve actually failed.
fn eval_through_production_registry(src: &str, what: &str) -> EvalResult {
    let compiled = compile_source_with_stdlib(src);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "the {what} source must compile without errors; got {errors:#?}",
    );

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(reify_constraints::SolverRegistry::production()));
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "the {what} eval must emit no Severity::Error diagnostics — a \
         non-converged solve surfaces here, and the value assertions must not \
         be able to pass while the solve failed; got {eval_errors:#?}",
    );

    result
}

/// The SI magnitude of a resolved `Scalar` cell, or a panic naming what was
/// actually there. An UNRESOLVED auto surfaces as `Value::Undef`, which is
/// precisely the silent failure this module must report legibly rather than let
/// a zero-diagnostic count wave through.
fn scalar_si(result: &EvalResult, id: &ValueCellId, what: &str) -> f64 {
    match result.values.get(id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "expected a resolved Scalar for {id:?} in the {what} eval; got \
             {other:?}. `Undef` here means the auto was never solved: the \
             let-indirected constraint that pins it was dropped by layer 1 \
             because `CellReadIndex::cells_reaching` keyed its reverse map on \
             the NORMALISED spelling only, while the seed is the compiler's RAW \
             instance-path declaration id",
        ),
    }
}

/// Every `Underdetermined`-coded diagnostic on this eval, matched by CODE
/// rather than by substring on the rendered `W_UNDERDETERMINED` text.
fn underdetermined(result: &EvalResult) -> Vec<&reify_core::Diagnostic> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Underdetermined))
        .collect()
}

/// THE load-bearing assertion for both instance-path minting sites: the autos
/// must RESOLVE, not merely stop warning.
///
/// RED before the reverse map is made additive (task #5467 step-19): probed
/// directly on the pre-fix branch tip, both cells came back `Value::Undef` with
/// ZERO diagnostics — silently wrong, where `main` was loudly right.
#[test]
fn let_indirected_instance_path_autos_resolve_through_their_lets() {
    let result =
        eval_through_production_registry(BOTH_MINTING_SITES_LET_INDIRECTED, "let-indirected");

    // (1) SUB-OVERRIDE site. `margin = bore - 2mm`, `margin == 8mm` => 10mm.
    let bore = scalar_si(
        &result,
        &ValueCellId::new("LetIndirect.b", "bore"),
        "let-indirected",
    );
    assert!(
        (bore - 0.010).abs() < SOLVER_TOL,
        "sub-override site (entity.rs:3130): `let margin = self.b.bore - 2mm` \
         with `constraint margin == 8mm` has the unique solution bore = 10mm; \
         got {bore} m (|delta| = {})",
        (bore - 0.010).abs(),
    );

    // (2) CONSTRUCTION named-arg site. `slack = length - 1mm`, `slack == 9mm`
    // => 10mm.
    let length = scalar_si(
        &result,
        &ValueCellId::new("LetIndirect.bolt", "length"),
        "let-indirected",
    );
    assert!(
        (length - 0.010).abs() < SOLVER_TOL,
        "construction named-arg site (entity.rs:3025): `let slack = \
         self.bolt.length - 1mm` with `constraint slack == 9mm` has the unique \
         solution length = 10mm; got {length} m (|delta| = {})",
        (length - 0.010).abs(),
    );

    // The dependent `let`s must be re-materialized to match, or the post-solve
    // write-back list (`build_dependent_cells` stage (d), which consumes the
    // same reverse walk) dropped them while the solve itself succeeded.
    let margin = scalar_si(
        &result,
        &ValueCellId::new("LetIndirect", "margin"),
        "let-indirected",
    );
    assert!(
        (margin - 0.008).abs() < SOLVER_TOL,
        "the dependent `let` must be re-materialized from the solved auto; \
         expected 8mm, got {margin} m",
    );
    let slack = scalar_si(
        &result,
        &ValueCellId::new("LetIndirect", "slack"),
        "let-indirected",
    );
    assert!(
        (slack - 0.009).abs() < SOLVER_TOL,
        "the dependent `let` must be re-materialized from the solved auto; \
         expected 9mm, got {slack} m",
    );
}

/// The SECOND, separate half of the let-indirected signal — kept apart from the
/// value assertions on purpose (see the module header): a fix that resolved the
/// autos while still printing `W_UNDERDETERMINED`, or that silenced the warning
/// without solving, must fail exactly one of the two and name which.
#[test]
fn let_indirected_instance_path_autos_emit_no_underdetermined_warning() {
    let result =
        eval_through_production_registry(BOTH_MINTING_SITES_LET_INDIRECTED, "let-indirected");

    let flagged = underdetermined(&result);
    assert!(
        flagged.is_empty(),
        "both autos are pinned through their `let`s, so neither may be \
         reported underdetermined; got {flagged:#?}",
    );
}
