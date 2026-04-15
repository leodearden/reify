//! Integration tests for incremental evaluation (edit_param / edit_check)
//! using the full v0.1 feature set.
//!
//! Each test:
//!   1. Cold-starts an Engine with make_simple_engine().
//!   2. Calls engine.eval(compiled()) to establish the baseline.
//!   3. Calls edit_param() or edit_check() with a new value.
//!   4. Asserts that the result reflects the change correctly.
//!
//! Uses `examples/integration_full_v01.ri` as the source file (created by
//! task 291). All tests use the real SimpleConstraintChecker (not a mock)
//! so that InRange / Positive / trait-constraint violations produce real
//! Satisfaction::Violated results.

use reify_compiler::CompiledModule;
use reify_test_support::{make_simple_engine, mm, parse_and_compile_with_stdlib};
use reify_types::{DimensionVector, Satisfaction, SnapshotId, SnapshotProvenance, Value, ValueCellId};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/integration_full_v01.ri"
);

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Read integration_full_v01.ri, caching the result in a `OnceLock`.
/// Returns a `&'static str` reference — no allocation on each call.
fn source() -> &'static str {
    static S: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(EXAMPLE_PATH)
            .expect("examples/integration_full_v01.ri should exist")
    })
    .as_str()
}

/// Parse and compile (with stdlib) the canonical source, caching the result.
/// Returns a `&'static CompiledModule` — no clone on each call.
fn compiled() -> &'static CompiledModule {
    static C: std::sync::OnceLock<CompiledModule> = std::sync::OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(source()))
}

/// Cold-start eval of integration_full_v01.ri.
/// Returns a fresh Engine plus the initial EvalResult.
fn make_eval_engine() -> (reify_eval::Engine, reify_eval::EvalResult) {
    let mut engine = make_simple_engine();
    let result = engine.eval(compiled());
    (engine, result)
}

/// Extract the SI value from a Value::Scalar, panicking with a helpful message otherwise.
fn si_value(v: &Value, label: &str) -> f64 {
    match v {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected Scalar for {label}, got {other:?}"),
    }
}

/// Extract the first component's SI value from a Value::Point or Value::Vector.
fn point_component(v: &Value, component: usize, label: &str) -> f64 {
    match v {
        Value::Point(items) | Value::Vector(items) => {
            let item = items
                .get(component)
                .unwrap_or_else(|| panic!("{label} has no component {component}"));
            si_value(item, &format!("{label}[{component}]"))
        }
        other => panic!("expected Point/Vector for {label}, got {other:?}"),
    }
}

// ── Step 1: cold_start_baseline_values ───────────────────────────────────────

/// Eval integration_full_v01.ri, assert baseline parameter SI values:
/// height=0.3, width=0.15, mass=5.0, clearance≈0.0127, position_x=0.1.
/// This validates the prerequisite merge and fixture loading.
#[test]
fn cold_start_baseline_values() {
    let (_engine, result) = make_eval_engine();

    let e = "Assembly";

    // height = 300mm = 0.3 SI
    let h = si_value(
        result
            .values
            .get(&ValueCellId::new(e, "height"))
            .expect("Assembly.height should exist"),
        "Assembly.height",
    );
    assert!(
        (h - 0.3).abs() < 1e-9,
        "expected 0.3 SI for height (300mm), got {h}"
    );

    // width = 150mm = 0.15 SI
    let w = si_value(
        result
            .values
            .get(&ValueCellId::new(e, "width"))
            .expect("Assembly.width should exist"),
        "Assembly.width",
    );
    assert!(
        (w - 0.15).abs() < 1e-9,
        "expected 0.15 SI for width (150mm), got {w}"
    );

    // mass = 5kg = 5.0 SI
    let m = si_value(
        result
            .values
            .get(&ValueCellId::new(e, "mass"))
            .expect("Assembly.mass should exist"),
        "Assembly.mass",
    );
    assert!(
        (m - 5.0).abs() < 1e-9,
        "expected 5.0 SI for mass (5kg), got {m}"
    );

    // clearance = 500mil = 500*0.0000254 = 0.0127 SI
    let c = si_value(
        result
            .values
            .get(&ValueCellId::new(e, "clearance"))
            .expect("Assembly.clearance should exist"),
        "Assembly.clearance",
    );
    assert!(
        (c - 0.0127).abs() < 1e-9,
        "expected ~0.0127 SI for clearance (500mil), got {c}"
    );

    // position_x = 100mm = 0.1 SI
    let px = si_value(
        result
            .values
            .get(&ValueCellId::new(e, "position_x"))
            .expect("Assembly.position_x should exist"),
        "Assembly.position_x",
    );
    assert!(
        (px - 0.1).abs() < 1e-9,
        "expected 0.1 SI for position_x (100mm), got {px}"
    );
}

// ── Step 3: edit_height_updates_geometric_target ──────────────────────────────

/// Cold-start eval, edit Assembly.height from 300mm to 400mm.
/// target = point3(height, width, clearance), so target[0] should change 0.3→0.4.
/// displacement = target - origin, so displacement[0] = target[0] - origin[0] should
/// change (0.3-0.1=0.2) → (0.4-0.1=0.3).
/// origin = point3(px, py, 0) does NOT depend on height, so origin is unchanged.
#[test]
fn edit_height_updates_geometric_target() {
    let e = "Assembly";
    let (mut engine, initial) = make_eval_engine();

    // Capture initial origin x-component
    let initial_origin = initial
        .values
        .get(&ValueCellId::new(e, "origin"))
        .expect("Assembly.origin should exist");
    let initial_origin_x = point_component(initial_origin, 0, "origin (initial)");

    // Edit height from 300mm to 400mm
    let height_id = ValueCellId::new(e, "height");
    let result = engine
        .edit_param(height_id, mm(400.0))
        .expect("edit_param should succeed");

    // target[0] should be 0.4 (= height in SI)
    let target = result
        .values
        .get(&ValueCellId::new(e, "target"))
        .expect("Assembly.target should exist after edit");
    let target_x = point_component(target, 0, "target");
    assert!(
        (target_x - 0.4).abs() < 1e-9,
        "expected target[0]=0.4 SI after height=400mm, got {target_x}"
    );

    // displacement[0] = target[0] - origin[0] = 0.4 - 0.1 = 0.3
    let disp = result
        .values
        .get(&ValueCellId::new(e, "displacement"))
        .expect("Assembly.displacement should exist after edit");
    let disp_x = point_component(disp, 0, "displacement");
    assert!(
        (disp_x - 0.3).abs() < 1e-9,
        "expected displacement[0]=0.3 SI after height=400mm, got {disp_x}"
    );

    // origin is unchanged (does not depend on height)
    let origin = result
        .values
        .get(&ValueCellId::new(e, "origin"))
        .expect("Assembly.origin should exist after edit");
    let origin_x = point_component(origin, 0, "origin (after edit)");
    assert!(
        (origin_x - initial_origin_x).abs() < 1e-12,
        "expected origin[0] unchanged ({initial_origin_x}), got {origin_x}"
    );
}

// ── Step 5: edit_position_x_updates_origin_chain ─────────────────────────────

/// Cold-start eval, edit Assembly.position_x from 100mm to 250mm.
/// origin = point3(position_x, position_y, 0), so origin[0] should change 0.1→0.25.
/// shifted = origin + offset, so shifted[0] should change (0.1+0.01=0.11) → (0.25+0.01=0.26).
/// displacement = target - origin, so displacement[0] should change (0.3-0.1=0.2) → (0.3-0.25=0.05).
/// base_frame = frame3(origin, rot) should not be Undef.
#[test]
fn edit_position_x_updates_origin_chain() {
    let e = "Assembly";
    let (mut engine, _initial) = make_eval_engine();

    // Edit position_x from 100mm to 250mm
    let px_id = ValueCellId::new(e, "position_x");
    let result = engine
        .edit_param(px_id, mm(250.0))
        .expect("edit_param should succeed");

    // origin[0] should be 0.25
    let origin = result
        .values
        .get(&ValueCellId::new(e, "origin"))
        .expect("Assembly.origin should exist");
    let origin_x = point_component(origin, 0, "origin");
    assert!(
        (origin_x - 0.25).abs() < 1e-9,
        "expected origin[0]=0.25 SI after position_x=250mm, got {origin_x}"
    );

    // shifted[0] should be ~0.26 (origin[0]=0.25 + offset[0]=0.01m = 10mm)
    let shifted = result
        .values
        .get(&ValueCellId::new(e, "shifted"))
        .expect("Assembly.shifted should exist");
    let shifted_x = point_component(shifted, 0, "shifted");
    assert!(
        (shifted_x - 0.26).abs() < 1e-9,
        "expected shifted[0]=0.26 SI after position_x=250mm, got {shifted_x}"
    );

    // displacement[0] should be ~0.05 (target[0]=0.3 - origin[0]=0.25)
    let disp = result
        .values
        .get(&ValueCellId::new(e, "displacement"))
        .expect("Assembly.displacement should exist");
    let disp_x = point_component(disp, 0, "displacement");
    assert!(
        (disp_x - 0.05).abs() < 1e-9,
        "expected displacement[0]=0.05 SI after position_x=250mm, got {disp_x}"
    );

    // base_frame should not be Undef
    let base_frame = result
        .values
        .get(&ValueCellId::new(e, "base_frame"))
        .expect("Assembly.base_frame should exist");
    assert!(
        !matches!(base_frame, Value::Undef),
        "expected base_frame not Undef after position_x edit, got {base_frame:?}"
    );
}

// ── Step 7: edit_height_inrange_still_satisfied ───────────────────────────────

/// Cold-start eval, edit_check Assembly.height from 300mm to 400mm.
/// 400mm is within InRange(v:height, lo:50mm, hi:1000mm) → both predicates Satisfied.
#[test]
fn edit_height_inrange_still_satisfied() {
    let e = "Assembly";
    let (mut engine, _initial) = make_eval_engine();

    // Edit height to 400mm (still in [50mm, 1000mm])
    let height_id = ValueCellId::new(e, "height");
    let check_result = engine
        .edit_check(height_id, mm(400.0))
        .expect("edit_check should succeed");

    // All constraint results should be Satisfied (no violations from 400mm height)
    let violations: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        violations.is_empty(),
        "expected no Violated constraints with height=400mm (still in InRange), got {violations:?}"
    );
}

// ── Step 9: edit_width_beyond_inrange_triggers_violation ─────────────────────

/// Cold-start eval, edit_check Assembly.width from 150mm to 600mm.
/// 600mm exceeds InRange(v:width, lo:10mm, hi:500mm) hi bound → at least one Violated.
#[test]
fn edit_width_beyond_inrange_triggers_violation() {
    let e = "Assembly";
    let (mut engine, _initial) = make_eval_engine();

    // Edit width to 600mm (exceeds hi=500mm)
    let width_id = ValueCellId::new(e, "width");
    let check_result = engine
        .edit_check(width_id, mm(600.0))
        .expect("edit_check should succeed");

    // At least one Violated entry
    let violations: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        !violations.is_empty(),
        "expected at least one Violated constraint with width=600mm (exceeds InRange hi=500mm), \
         got {} results with no violations",
        check_result.constraint_results.len()
    );
}

// ── Step 11: edit_mass_below_trait_bound_triggers_violation ──────────────────

/// Cold-start eval, edit_check Assembly.mass from 5kg to 0.5kg.
/// 0.5kg violates the Physical trait constraint `mass > 1kg` → at least one Violated.
#[test]
fn edit_mass_below_trait_bound_triggers_violation() {
    let e = "Assembly";
    let (mut engine, _initial) = make_eval_engine();

    // Edit mass to 0.5kg (below Physical trait bound of 1kg)
    let mass_id = ValueCellId::new(e, "mass");
    let mass_value = Value::Scalar {
        si_value: 0.5,
        dimension: DimensionVector::MASS,
    };
    let check_result = engine
        .edit_check(mass_id, mass_value)
        .expect("edit_check should succeed");

    // At least one Violated entry
    let violations: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        !violations.is_empty(),
        "expected at least one Violated constraint with mass=0.5kg (below trait bound mass>1kg), \
         got {} results with no violations",
        check_result.constraint_results.len()
    );
}

// ── Step 13: edit_height_below_width_triggers_ordering_violation ──────────────

/// Cold-start eval, edit_check Assembly.height from 300mm to 100mm.
/// 100mm < width=150mm violates `constraint height > width` → at least one Violated.
/// Total constraint count still >=40 (no short-circuit).
#[test]
fn edit_height_below_width_triggers_ordering_violation() {
    let e = "Assembly";
    let (mut engine, _initial) = make_eval_engine();

    // Edit height to 100mm (< width=150mm → violates height > width)
    let height_id = ValueCellId::new(e, "height");
    let check_result = engine
        .edit_check(height_id, mm(100.0))
        .expect("edit_check should succeed");

    // Total count still >=40 (no short-circuit on violation)
    assert!(
        check_result.constraint_results.len() >= 40,
        "expected >=40 constraint results even with height=100mm (violation), got {}",
        check_result.constraint_results.len()
    );

    // At least one Violated
    let violations: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        !violations.is_empty(),
        "expected at least one Violated constraint with height=100mm < width=150mm, \
         got {} results with no violations",
        check_result.constraint_results.len()
    );
}

// ── Step 15: edit_position_x_determinacy_predicates_hold ─────────────────────

/// Cold-start eval, edit_check Assembly.position_x from 100mm to 200mm.
/// Both determined(origin) and determined(target) should remain Satisfied.
/// No constraints should be Violated.
#[test]
fn edit_position_x_determinacy_predicates_hold() {
    let e = "Assembly";
    let (mut engine, _initial) = make_eval_engine();

    // Edit position_x to 200mm
    let px_id = ValueCellId::new(e, "position_x");
    let check_result = engine
        .edit_check(px_id, mm(200.0))
        .expect("edit_check should succeed");

    // No violations
    let violations: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        violations.is_empty(),
        "expected no Violated constraints with position_x=200mm, got {violations:?}"
    );
}

// ── Step 17: edit_position_x_where_guard_constraints_remain_satisfied ─────────

/// Cold-start eval, edit_check Assembly.position_x from 100mm to 200mm.
/// The where-block guard determined(origin) remains Satisfied, so both
/// guarded constraints (determined(displacement), determined(base_frame)) should
/// also be Satisfied in the check result.
#[test]
fn edit_position_x_where_guard_constraints_remain_satisfied() {
    let e = "Assembly";
    let (mut engine, _initial) = make_eval_engine();

    // Identify the guarded constraint IDs from the compiled module
    let compiled_module = compiled();
    let assembly_template = compiled_module
        .templates
        .iter()
        .find(|t| t.name == e)
        .expect("Assembly template should exist");
    assert!(
        !assembly_template.guarded_groups.is_empty(),
        "Assembly should have at least one guarded_group (the where determined(origin) block)"
    );
    let guarded_ids: Vec<_> = assembly_template.guarded_groups[0]
        .constraints
        .iter()
        .map(|c| c.id.clone())
        .collect();

    // Edit position_x to 200mm
    let px_id = ValueCellId::new(e, "position_x");
    let check_result = engine
        .edit_check(px_id, mm(200.0))
        .expect("edit_check should succeed");

    // Both guarded constraints should be Satisfied
    for guarded_id in &guarded_ids {
        let entry = check_result
            .constraint_results
            .iter()
            .find(|entry| &entry.id == guarded_id)
            .unwrap_or_else(|| {
                panic!("guarded constraint {guarded_id} not found in check_result")
            });
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "guarded constraint {guarded_id} should be Satisfied after position_x=200mm, got {:?}",
            entry.satisfaction
        );
    }
}

// ── Step 19: multiple_edits_accumulate_correctly ──────────────────────────────

/// Cold-start eval, edit_param height 300mm→400mm, then width 150mm→200mm.
/// Final snapshot: height=0.4 SI, width=0.2 SI.
/// target[0]=0.4 (height), target[1]=0.2 (width).
/// displacement[0]=target[0]-origin[0]=0.4-0.1=0.3.
/// SnapshotId should be 2 after two edits.
#[test]
fn multiple_edits_accumulate_correctly() {
    let e = "Assembly";
    let (mut engine, _initial) = make_eval_engine();

    // Edit 1: height from 300mm to 400mm
    let height_id = ValueCellId::new(e, "height");
    engine
        .edit_param(height_id, mm(400.0))
        .expect("first edit_param should succeed");

    // Edit 2: width from 150mm to 200mm
    let width_id = ValueCellId::new(e, "width");
    let result = engine
        .edit_param(width_id, mm(200.0))
        .expect("second edit_param should succeed");

    // height = 0.4 SI
    let h = si_value(
        result
            .values
            .get(&ValueCellId::new(e, "height"))
            .expect("Assembly.height should exist"),
        "Assembly.height (after 2 edits)",
    );
    assert!(
        (h - 0.4).abs() < 1e-9,
        "expected height=0.4 SI after two edits, got {h}"
    );

    // width = 0.2 SI
    let w = si_value(
        result
            .values
            .get(&ValueCellId::new(e, "width"))
            .expect("Assembly.width should exist"),
        "Assembly.width (after 2 edits)",
    );
    assert!(
        (w - 0.2).abs() < 1e-9,
        "expected width=0.2 SI after two edits, got {w}"
    );

    // target[0]=0.4 (height), target[1]=0.2 (width)
    let target = result
        .values
        .get(&ValueCellId::new(e, "target"))
        .expect("Assembly.target should exist");
    let target_x = point_component(target, 0, "target (2 edits)");
    let target_y = point_component(target, 1, "target (2 edits)");
    assert!(
        (target_x - 0.4).abs() < 1e-9,
        "expected target[0]=0.4 after height=400mm, got {target_x}"
    );
    assert!(
        (target_y - 0.2).abs() < 1e-9,
        "expected target[1]=0.2 after width=200mm, got {target_y}"
    );

    // snapshot ID should be 2 (initial=0, first edit=1, second edit=2)
    let snap = engine
        .snapshot()
        .expect("snapshot should exist after two edits");
    assert_eq!(
        snap.id,
        SnapshotId(2),
        "expected SnapshotId(2) after two edits, got {:?}",
        snap.id
    );
}

// ── Step 21: edit_param_unrelated_params_unchanged ───────────────────────────

/// Cold-start eval, capture initial values for mass, clearance, position_x,
/// position_y. Edit height from 300mm to 400mm.
/// mass, clearance, position_x, position_y should be identical to initial values.
/// origin is unchanged (position_x/position_y didn't change).
#[test]
fn edit_param_unrelated_params_unchanged() {
    let e = "Assembly";
    let (mut engine, initial) = make_eval_engine();

    // Capture initial values for unrelated params
    let initial_mass = initial
        .values
        .get(&ValueCellId::new(e, "mass"))
        .expect("Assembly.mass")
        .clone();
    let initial_clearance = initial
        .values
        .get(&ValueCellId::new(e, "clearance"))
        .expect("Assembly.clearance")
        .clone();
    let initial_px = initial
        .values
        .get(&ValueCellId::new(e, "position_x"))
        .expect("Assembly.position_x")
        .clone();
    let initial_py = initial
        .values
        .get(&ValueCellId::new(e, "position_y"))
        .expect("Assembly.position_y")
        .clone();
    let initial_origin = initial
        .values
        .get(&ValueCellId::new(e, "origin"))
        .expect("Assembly.origin")
        .clone();

    // Edit height from 300mm to 400mm
    let height_id = ValueCellId::new(e, "height");
    let result = engine
        .edit_param(height_id, mm(400.0))
        .expect("edit_param should succeed");

    // mass unchanged
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "mass")),
        Some(&initial_mass),
        "mass should be unchanged after height edit"
    );

    // clearance unchanged
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "clearance")),
        Some(&initial_clearance),
        "clearance should be unchanged after height edit"
    );

    // position_x unchanged
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "position_x")),
        Some(&initial_px),
        "position_x should be unchanged after height edit"
    );

    // position_y unchanged
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "position_y")),
        Some(&initial_py),
        "position_y should be unchanged after height edit"
    );

    // origin unchanged (depends on position_x, position_y — not height)
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "origin")),
        Some(&initial_origin),
        "origin should be unchanged after height edit (origin depends on position_x/y, not height)"
    );
}

// ── Step 23: edit_param_snapshot_provenance_chain ────────────────────────────

/// Cold-start eval, assert snapshot provenance is Initial with id=0.
/// Edit height → provenance is Edit{changed:{height}, parent:0} with id=1.
/// Edit width → provenance is Edit{changed:{width}, parent:1} with id=2.
#[test]
fn edit_param_snapshot_provenance_chain() {
    let e = "Assembly";
    let (mut engine, _initial) = make_eval_engine();

    // After eval(): provenance should be Initial, ID = 0
    let snap = engine
        .snapshot()
        .expect("snapshot should exist after eval");
    assert_eq!(snap.provenance, SnapshotProvenance::Initial);
    assert_eq!(snap.id, SnapshotId(0));

    // After first edit_param (height): provenance should be Edit, ID = 1
    let height_id = ValueCellId::new(e, "height");
    engine
        .edit_param(height_id.clone(), mm(400.0))
        .expect("first edit_param should succeed");
    let snap = engine
        .snapshot()
        .expect("snapshot should exist after first edit");
    assert_eq!(snap.id, SnapshotId(1));
    {
        let mut expected_changed = std::collections::HashSet::new();
        expected_changed.insert(height_id);
        assert_eq!(
            snap.provenance,
            SnapshotProvenance::Edit {
                changed: expected_changed,
                parent: SnapshotId(0),
            }
        );
    }

    // After second edit_param (width): provenance should be Edit, ID = 2
    let width_id = ValueCellId::new(e, "width");
    engine
        .edit_param(width_id.clone(), mm(200.0))
        .expect("second edit_param should succeed");
    let snap = engine
        .snapshot()
        .expect("snapshot should exist after second edit");
    assert_eq!(snap.id, SnapshotId(2));
    {
        let mut expected_changed = std::collections::HashSet::new();
        expected_changed.insert(width_id);
        assert_eq!(
            snap.provenance,
            SnapshotProvenance::Edit {
                changed: expected_changed,
                parent: SnapshotId(1),
            }
        );
    }
}
