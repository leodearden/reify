//! Engine-level tests for the `RepresentationWithin` assertion dispatch
//! interception (Determinacy γ, task-4199).
//!
//! # Non-OCCT tests (step-5 / step-6)
//!
//! Verify that `Engine::dispatch_constraints` correctly intercepts
//! `RepresentationWithin` constraint expressions, evaluates them against
//! `self.achieved_repr_tol` (injected via a test-instrumentation setter), and
//! weaves results back in caller (input) order.
//!
//! These tests use a non-kernel engine (no OCCT) so that the full pipeline
//! can be exercised in CI without a geometry backend.  The
//! `set_achieved_repr_tol_for_test` setter is the test-instrumentation seam
//! added alongside `set_capture_repr_tol` (engine_admin.rs).
//!
//! # OCCT-gated tests (step-7 / step-8)
//!
//! End-to-end tests that use a real OCCT kernel to tessellate curved geometry
//! and verify the full dispatch-interception + tessellation pipeline.
//! All OCCT-gated tests skip cleanly when OCCT is not available (stub mode).
//!
//! Pipeline under test: `set_capture_repr_tol(true)` →
//! `tessellate_realizations(&compiled)` → `check(&compiled)`.
//! The `tessellate_realizations` call populates `achieved_repr_tol`;
//! `check` calls `eval` (which does NOT clear the map) then
//! `dispatch_constraints`, which intercepts `RepresentationWithin` entries
//! and reads from the populated map.

use reify_core::ConstraintNodeId;
use reify_core::{ContentHash, DimensionVector, Type};
use reify_eval::graph::ConstraintNodeData;
use reify_eval::tolerance_combine::extract_output_tolerance_bound;
use reify_ir::{CompiledExpr, PersistentMap, Satisfaction};
use reify_test_support::{make_simple_engine, parse_and_compile};
use std::collections::BTreeMap;

// ── Shared DSL fixture ────────────────────────────────────────────────────────

/// A module with two constraints in the **same** template (`Checker`):
///
/// - Constraint index 0: `RepresentationWithin(subject, 1mm)` — the assertion.
///   Bound is `1mm = 1e-3 m` (built-in unit, no stdlib required).
/// - Constraint index 1: `w > 0.0` — an ordinary always-`Satisfied` predicate.
///
/// `MyGeom` supplies the named structure type for `subject`; it has no
/// geometry (non-kernel engine) so `subject.self` is Undef at eval time.
/// The type-name scan fallback in `eval_representation_within` resolves
/// the achieved-tol key from the struct name `"MyGeom"` → key
/// `"MyGeom#realization[0]"` in the injected map.
///
/// Both constraints live in the **same** template so they pass through a
/// **single** `dispatch_constraints` call — this exercises within-batch order
/// preservation when the interception peels constraint 0 and leaves
/// constraint 1 for the language-level checker.
///
/// Note: `mm` is a built-in length unit available without stdlib; `um`
/// (micrometer) requires stdlib and is intentionally avoided here so that
/// `parse_and_compile` (no stdlib) can be used.
const INTERCEPTION_SOURCE: &str = r#"
structure MyGeom {
    param x : Real = 1.0
}

// Checker carries BOTH a RepresentationWithin assertion (constraint index 0)
// AND an ordinary always-satisfied constraint (index 1) in a single template.
// Placing both constraints here exercises the within-batch order-preservation
// invariant of dispatch_constraints: the engine-side result for index 0 must
// appear before the checker-side result for index 1 in the returned list.
structure Checker {
    param subject : MyGeom
    param w : Real = 5.0
    constraint RepresentationWithin(subject, 1mm)
    constraint w > 0.0
}
"#;

// ── BT1: over-bound → Violated ────────────────────────────────────────────────

/// BT1: achieved value ABOVE the bound (5e-3 m > 1 mm = 1e-3 m) → `Violated`.
///
/// Also verifies:
/// - The ordinary constraint (`w > 0.0` with `w = 5.0`) is `Satisfied`.
/// - **Input-order preservation**: RepresentationWithin (constraint index 0)
///   appears before the ordinary constraint (index 1) in the result list,
///   proving `dispatch_constraints` weaves interception results back in the
///   original entry order.
///
/// RED until step-6 adds `set_achieved_repr_tol_for_test` and wires the
/// interception into `dispatch_constraints`.
#[test]
fn dispatch_interception_over_bound_yields_violated() {
    let compiled = parse_and_compile(INTERCEPTION_SOURCE);
    let mut engine = make_simple_engine();

    // Inject achieved_repr_tol via the test-instrumentation setter.
    // "MyGeom#realization[0]" = 5e-3 m > 1mm = 1e-3 m bound → must yield Violated.
    //
    // RED: `set_achieved_repr_tol_for_test` does not exist until step-6.
    let mut map = BTreeMap::new();
    map.insert("MyGeom#realization[0]".to_string(), 5e-3_f64);
    engine.set_achieved_repr_tol_for_test(map);

    let result = engine.check(&compiled);

    // Checker has two constraints → exactly 2 constraint results.
    assert_eq!(
        result.constraint_results.len(),
        2,
        "Checker has 2 constraints (RepresentationWithin + w>0) → 2 results; \
         got {:?}",
        result
            .constraint_results
            .iter()
            .map(|e| (&e.id, e.satisfaction))
            .collect::<Vec<_>>()
    );

    // ── RepresentationWithin (entity="Checker", index=0) ──────────────────────
    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Violated,
        "BT1: achieved 5e-3 m > bound 1mm (1e-3 m) → Violated"
    );

    // ── Ordinary constraint (entity="Checker", index=1) ───────────────────────
    let ord_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 1)
        .expect("must have Checker#constraint[1] (w > 0.0)");
    assert_eq!(
        ord_entry.satisfaction,
        Satisfaction::Satisfied,
        "w=5.0 > 0.0 → Satisfied (ordinary constraint unaffected by interception)"
    );

    // ── Input-order preservation ───────────────────────────────────────────────
    // The RepresentationWithin result (index 0) must appear at a LOWER position
    // in the output list than the ordinary result (index 1), matching the order
    // of entries in the dispatch batch.
    let rw_pos = result
        .constraint_results
        .iter()
        .position(|e| e.id.entity == "Checker" && e.id.index == 0)
        .unwrap();
    let ord_pos = result
        .constraint_results
        .iter()
        .position(|e| e.id.entity == "Checker" && e.id.index == 1)
        .unwrap();
    assert!(
        rw_pos < ord_pos,
        "BT1: RepresentationWithin (pos {rw_pos}) must precede the ordinary \
         constraint (pos {ord_pos}) — dispatch_constraints must preserve \
         within-batch input order even when interleaving engine-side and \
         checker-side results"
    );
}

// ── BT2: under-bound → Satisfied ─────────────────────────────────────────────

/// BT2: achieved value BELOW the bound (1e-9 m ≪ 1mm = 1e-3 m) → `Satisfied`.
///
/// RED until step-6.
#[test]
fn dispatch_interception_under_bound_yields_satisfied() {
    let compiled = parse_and_compile(INTERCEPTION_SOURCE);
    let mut engine = make_simple_engine();

    // 1e-9 m ≪ 1mm = 1e-3 m bound → Satisfied.
    let mut map = BTreeMap::new();
    map.insert("MyGeom#realization[0]".to_string(), 1e-9_f64);
    // RED: setter does not exist until step-6.
    engine.set_achieved_repr_tol_for_test(map);

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Satisfied,
        "BT2: achieved 1e-9 m < bound 1mm (1e-3 m) → Satisfied"
    );
}

// ── BT3: no entry → Indeterminate ────────────────────────────────────────────

/// BT3: no entry in `achieved_repr_tol` for the subject → `Indeterminate`.
///
/// C1 invariant: absent key ⇒ realization not run ⇒ never a false `Violated`.
///
/// RED until step-6.
#[test]
fn dispatch_interception_no_entry_yields_indeterminate() {
    let compiled = parse_and_compile(INTERCEPTION_SOURCE);
    let mut engine = make_simple_engine();

    // Empty map — no key matching "MyGeom#realization[*]" → Indeterminate.
    // RED: setter does not exist until step-6.
    engine.set_achieved_repr_tol_for_test(BTreeMap::new());

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Checker" && e.id.index == 0)
        .expect("must have Checker#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Indeterminate,
        "BT3 / C1: no achieved entry → Indeterminate (never a false Violated)"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// OCCT-gated end-to-end tests (step-7 / step-8)
// ═══════════════════════════════════════════════════════════════════════════════

// ── OCCT helpers ─────────────────────────────────────────────────────────────

/// Build a fresh `Engine` backed by a real OCCT kernel, mirroring the
/// `make_occt_engine` helper in `achieved_repr_tol.rs`.
///
/// Uses `OcctKernelHandle` directly (not `SingleKernelHolder`) so that
/// `measure_mesh_deviation` is reachable through the `&dyn GeometryKernel`
/// vtable — `SingleKernelHolder` defaults most optional methods to `None`.
fn make_occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

/// Compile `source` (no stdlib) and assert no error-severity diagnostics.
fn compile_no_errors(source: &str, name: &str) -> reify_compiler::CompiledModule {
    use reify_core::{ModulePath, Severity};
    let parsed = reify_syntax::parse(source, ModulePath::single(name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {name}: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors in {name}: {:#?}", errors);
    compiled
}

// ── Shared OCCT DSL fixture ───────────────────────────────────────────────────

/// DSL for the OCCT end-to-end tests.
///
/// - `Sphere`: a 1 m-radius sphere realized at `#precision(50mm)` (COARSE).
///   At 50 mm deflection the sampled chord deviation is on the order of
///   centimetres — far above `1mm` (1e-3 m) → **Violated** under tight bound.
/// - `SphereCheck`: carries `RepresentationWithin(subject, 1mm)` — bound = 1e-3 m.
///
/// BT6 uses this source verbatim (coarse → Violated).
/// BT7 replaces `#precision(50mm)` with `#precision(0.1mm)` so deviation < 1mm.
/// BT8 uses this source but skips `tessellate_realizations` → Indeterminate.
/// C4 uses a variant with `0mm` bound.
///
/// `mm` is a built-in length unit — no stdlib needed.
const OCCT_SOURCE_COARSE: &str = r#"
#precision(50mm)
structure Sphere {
    let r = sphere(1000mm)
}
structure SphereCheck {
    param subject : Sphere
    constraint RepresentationWithin(subject, 1mm)
}
"#;

/// Fine-precision variant: `#precision(0.1mm)` — sampled deviation ≪ 1mm →
/// used by BT7 to verify `Satisfied`.
const OCCT_SOURCE_FINE: &str = r#"
#precision(0.1mm)
structure Sphere {
    let r = sphere(1000mm)
}
structure SphereCheck {
    param subject : Sphere
    constraint RepresentationWithin(subject, 1mm)
}
"#;

/// Zero-bound variant: `RepresentationWithin(subject, 0mm)` — bound = 0.0 m.
/// With C4's zero-bound floor (PLANAR_FLOOR = 1e-5 m), a coarse sphere
/// (deviation ≫ 1e-5 m) is still **Violated**.
const OCCT_SOURCE_ZERO_BOUND: &str = r#"
#precision(50mm)
structure Sphere {
    let r = sphere(1000mm)
}
structure SphereCheck {
    param subject : Sphere
    constraint RepresentationWithin(subject, 0mm)
}
"#;

// ── BT6/C3: coarse sphere + tight bound → Violated ───────────────────────────

/// BT6 / C3: a sphere tessellated at coarse precision (50 mm deflection) has
/// sampled deviation >> 1 mm (1e-3 m), so `RepresentationWithin(subject, 1mm)`
/// is **Violated** after the full `tessellate_realizations → check` pipeline.
///
/// This is the headline "the assertion fires" test: non-zero bound (C3),
/// coarse subject, OCCT kernel present.
///
/// Pipeline: `set_capture_repr_tol(true)` → `tessellate_realizations` →
/// `check` → `dispatch_constraints` intercepts `RepresentationWithin` →
/// type-name scan resolves "Sphere#realization[0]" → achieved > 1mm → Violated.
#[test]
fn bt6_coarse_sphere_tight_bound_yields_violated() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping bt6_coarse_sphere_tight_bound_yields_violated: OCCT not available");
        return;
    }

    let compiled = compile_no_errors(OCCT_SOURCE_COARSE, "bt6_coarse");
    let mut engine = make_occt_engine();
    engine.set_capture_repr_tol(true);
    engine.tessellate_realizations(&compiled);

    // Verify the map was populated (BT6 pre-condition: OCCT measured something).
    let achieved = engine.achieved_repr_tol("Sphere#realization[0]").expect(
        "BT6: coarse sphere must have Some achieved_repr_tol after tessellate_realizations",
    );
    assert!(
        achieved > 1e-3,
        "BT6 pre-condition: coarse sphere deviation ({achieved:.3e} m) must exceed \
         the 1mm (1e-3 m) bound so the assertion fires"
    );

    // Run check: tessellate_realizations populated the map; check reads it.
    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "SphereCheck" && e.id.index == 0)
        .expect("must have SphereCheck#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Violated,
        "BT6 / C3: coarse sphere (deviation {achieved:.3e} m) > 1mm bound → Violated"
    );
}

// ── BT7/C3: fine sphere + tight bound → Satisfied ────────────────────────────

/// BT7 / C3: a sphere tessellated at fine precision (0.1 mm deflection) has
/// sampled deviation < 1 mm, so `RepresentationWithin(subject, 1mm)` is
/// **Satisfied** after the full pipeline.
#[test]
fn bt7_fine_sphere_tight_bound_yields_satisfied() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping bt7_fine_sphere_tight_bound_yields_satisfied: OCCT not available");
        return;
    }

    let compiled = compile_no_errors(OCCT_SOURCE_FINE, "bt7_fine");
    let mut engine = make_occt_engine();
    engine.set_capture_repr_tol(true);
    engine.tessellate_realizations(&compiled);

    let achieved = engine
        .achieved_repr_tol("Sphere#realization[0]")
        .expect("BT7: fine sphere must have Some achieved_repr_tol after tessellate_realizations");
    assert!(
        achieved < 1e-3,
        "BT7 pre-condition: fine sphere deviation ({achieved:.3e} m) must be below \
         the 1mm (1e-3 m) bound so the assertion passes"
    );

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "SphereCheck" && e.id.index == 0)
        .expect("must have SphereCheck#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Satisfied,
        "BT7 / C3: fine sphere (deviation {achieved:.3e} m) < 1mm bound → Satisfied"
    );
}

// ── BT8/C1: no tessellation → Indeterminate ──────────────────────────────────

/// BT8 / C1: when `tessellate_realizations` is NOT called (or
/// `set_capture_repr_tol` is NOT set to `true`), `achieved_repr_tol` stays
/// empty, and the assertion is **Indeterminate** (never a false Violated).
///
/// This verifies C1: absent key ⇒ realization not run ⇒ no assertion fire.
#[test]
fn bt8_no_tessellation_yields_indeterminate() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping bt8_no_tessellation_yields_indeterminate: OCCT not available");
        return;
    }

    let compiled = compile_no_errors(OCCT_SOURCE_COARSE, "bt8_no_tess");
    let mut engine = make_occt_engine();
    // Deliberately skip set_capture_repr_tol + tessellate_realizations
    // → achieved_repr_tol map stays empty.

    let result = engine.check(&compiled);

    // Map is empty → key absent → Indeterminate (C1: never a false Violated).
    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "SphereCheck" && e.id.index == 0)
        .expect("must have SphereCheck#constraint[0] (RepresentationWithin)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Indeterminate,
        "BT8 / C1: no tessellation → empty map → Indeterminate (never a false Violated)"
    );
}

// ── C4: zero bound on curved subject → Violated ──────────────────────────────

/// C4: `RepresentationWithin(subject, 0mm)` with a coarse curved sphere.
///
/// The zero-bound floor (PLANAR_FLOOR = 1e-5 m) is applied: `eff = 1e-5 m`.
/// A coarse sphere has deviation ≫ 1e-5 m, so the assertion is **Violated**.
/// This distinguishes planar (B1-validated ≤ 1e-5 m → Satisfied) from
/// curved (B2-validated ≫ 1e-5 m → Violated) under a zero bound.
#[test]
fn c4_zero_bound_coarse_sphere_yields_violated() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping c4_zero_bound_coarse_sphere_yields_violated: OCCT not available");
        return;
    }

    let compiled = compile_no_errors(OCCT_SOURCE_ZERO_BOUND, "c4_zero_bound");
    let mut engine = make_occt_engine();
    engine.set_capture_repr_tol(true);
    engine.tessellate_realizations(&compiled);

    let achieved = engine
        .achieved_repr_tol("Sphere#realization[0]")
        .expect("C4: coarse sphere must have Some achieved_repr_tol");
    // PLANAR_FLOOR = 1e-5 m; coarse sphere must be far above it.
    assert!(
        achieved > 1e-5,
        "C4 pre-condition: coarse sphere deviation ({achieved:.3e} m) must exceed \
         PLANAR_FLOOR (1e-5 m) so zero-bound → Violated"
    );

    let result = engine.check(&compiled);

    let rw_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "SphereCheck" && e.id.index == 0)
        .expect("must have SphereCheck#constraint[0] (RepresentationWithin, zero bound)");
    assert_eq!(
        rw_entry.satisfaction,
        Satisfaction::Violated,
        "C4: zero bound + coarse sphere (achieved {achieved:.3e} m >> PLANAR_FLOOR 1e-5 m) → Violated"
    );
}

// ── C2 regression: extract_output_tolerance_bound returns the bound ───────────

/// C2 regression: `extract_output_tolerance_bound` still returns the declared
/// bound from a `RepresentationWithin` constraint expression, unchanged by
/// the addition of the assertion path.
///
/// The same constraint expression BOTH drives the tessellation budget (via
/// `extract_output_tolerance_bound`) AND asserts post-realization (via
/// `eval_representation_within`).  This test pins the extractor's return value
/// so that refactoring the assertion path cannot silently break the budget.
///
/// Uses a synthetic `PersistentMap<ConstraintNodeId, ConstraintNodeData>` to
/// call `extract_output_tolerance_bound` directly (no OCCT needed).
#[test]
fn c2_extract_output_tolerance_bound_still_returns_declared_bound() {
    // Build a synthetic ConstraintNodeData carrying the canonical shape:
    // RepresentationWithin(ValueRef(subject.self):StructureRef("Sphere"), 1mm)
    // where 1mm = 1e-3 m (SI).
    let bound_si = 1e-3_f64; // 1mm in SI metres
    let subject_arg = CompiledExpr::value_ref(
        reify_core::ValueCellId::new("subject", "self"),
        Type::StructureRef("Sphere".to_string()),
    );
    let tol_arg = CompiledExpr::literal(
        reify_ir::Value::Scalar {
            si_value: bound_si,
            dimension: DimensionVector::LENGTH,
        },
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
    );
    let expr = CompiledExpr::user_function_call(
        "RepresentationWithin".to_string(),
        vec![subject_arg, tol_arg],
        Type::Bool,
    );

    let entity = "SphereCheck";
    let index = 0u32;
    let id = ConstraintNodeId::new(entity, index);
    let data = ConstraintNodeData {
        id: id.clone(),
        label: None,
        expr,
        content_hash: ContentHash::of_str(&format!("{}#constraint[{}]", entity, index)),
        optimized_target: None,
    };

    let mut constraints: PersistentMap<ConstraintNodeId, ConstraintNodeData> =
        PersistentMap::default();
    constraints.insert(id, data);

    // C2 regression: extract_output_tolerance_bound returns Some(1e-3) for
    // entity "SphereCheck" — the budget path is unaffected by the assertion path.
    let extracted = extract_output_tolerance_bound(&constraints, "SphereCheck");
    assert_eq!(
        extracted,
        Some(bound_si),
        "C2: extract_output_tolerance_bound must return the declared bound ({bound_si:.3e} m) \
         unchanged — the budget path must not be affected by the assertion interception"
    );

    // Also verify it returns None for an unrelated entity (gate 1 is still active).
    let not_found = extract_output_tolerance_bound(&constraints, "OtherEntity");
    assert_eq!(
        not_found, None,
        "C2: extract_output_tolerance_bound must return None for an unrelated entity"
    );
}
