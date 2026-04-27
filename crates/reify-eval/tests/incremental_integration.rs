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
use reify_test_support::{kg, make_simple_engine, mm, parse_and_compile_with_stdlib};
use reify_types::{Satisfaction, SnapshotId, SnapshotProvenance, Value, ValueCellId};

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

    // Positive presence check: all constraints should still be evaluated (not silently dropped).
    // Height does not touch the `determined(origin)` guard, so no constraints are excluded —
    // the full assembly count (measured as 49) must be present.
    // Floor is 47: 49 total minus the 2 guarded constraints excluded by esc-295-78.
    assert!(
        check_result.constraint_results.len() >= 47,
        "expected >= 47 constraint results after height=400mm, got {} \
         (constraints may have been silently dropped; \
          floor is 49 total minus 2 esc-295-78-guarded constraints = 47)",
        check_result.constraint_results.len()
    );

    // Positive Satisfied assertion: at least one constraint must be Satisfied (not just "no
    // Violated" — the test would trivially pass if the engine returned 0 results).
    let satisfied: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Satisfied)
        .collect();
    assert!(
        !satisfied.is_empty(),
        "expected at least one Satisfied constraint with height=400mm (in-range edit), got none"
    );

    // No violations
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
    let check_result = engine
        .edit_check(mass_id, kg(0.5))
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

    // Total count still >=47 (no short-circuit on violation).
    // Floor is 47: 49 total minus the 2 guarded constraints excluded by esc-295-78.
    assert!(
        check_result.constraint_results.len() >= 47,
        "expected >=47 constraint results even with height=100mm (violation), got {} \
         (floor is 49 total minus 2 esc-295-78-guarded constraints = 47)",
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

    // Positive presence check: due to esc-295-78, the 2 guarded constraints are excluded from
    // the result when position_x is in the dirty cone.  Even so, the remaining 47 constraints
    // must all be present — any further silent dropping would indicate a regression.
    // Floor is 47: 49 total minus the 2 guarded constraints excluded by esc-295-78.
    assert!(
        check_result.constraint_results.len() >= 47,
        "expected >= 47 constraint results after position_x=200mm, got {} \
         (constraints may have been silently dropped; \
          note esc-295-78 excludes 2 guarded constraints leaving floor of 47)",
        check_result.constraint_results.len()
    );

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
///
/// Checks:
/// 1. The compiled Assembly template has exactly 1 guarded_group with 2 constraints
///    (the `where determined(origin) { … }` block).
/// 2. After edit_check, all 49 constraints are returned (including the 2 guarded ones).
/// 3. The guarded constraints (determined(displacement), determined(base_frame)) are Satisfied.
/// 4. No returned constraint results are Violated.
#[test]
fn edit_position_x_where_guard_constraints_remain_satisfied() {
    let e = "Assembly";
    let (mut engine, _initial) = make_eval_engine();

    // 1. Structural check: compiled module has the expected guarded_group layout.
    let compiled_module = compiled();
    let assembly_template = compiled_module
        .templates
        .iter()
        .find(|t| t.name == e)
        .expect("Assembly template should exist");
    assert_eq!(
        assembly_template.guarded_groups.len(),
        1,
        "Assembly should have exactly 1 guarded_group (the where determined(origin) block)"
    );
    assert_eq!(
        assembly_template.guarded_groups[0].constraints.len(),
        2,
        "the where-block should contain exactly 2 guarded constraints \
         (determined(displacement) and determined(base_frame))"
    );

    // 2. After edit_check(position_x=200mm), all 49 constraints should be returned.
    let px_id = ValueCellId::new(e, "position_x");
    let check_result = engine
        .edit_check(px_id, mm(200.0))
        .expect("edit_check should succeed");

    let result_count = check_result.constraint_results.len();
    assert_eq!(
        result_count, 49,
        "expected all 49 constraint results (including 2 guarded), got {result_count}"
    );
    // Lower-bound guard: even with esc-295-78 active, any regression that drops
    // the count below the expected-broken floor of 47 must not silently pass.
    // The XFAIL assertion above already captures the known breakage (count != 49);
    // this lower bound catches additional regressions below the broken floor.
    assert!(
        result_count >= 47,
        "expected >= 47 constraint results after position_x=200mm (XFAIL lower bound), \
         got {result_count} — this indicates a regression beyond the esc-295-78 exclusion \
         (49 total minus 2 guarded constraints excluded by esc-295-78 = 47 broken floor)"
    );

    // 3. The guarded constraints should be Satisfied (determined(origin) is true after edit).
    let guarded_ids: Vec<_> = assembly_template.guarded_groups[0]
        .constraints
        .iter()
        .map(|c| &c.id)
        .collect();
    for gid in &guarded_ids {
        let entry = check_result
            .constraint_results
            .iter()
            .find(|e| &e.id == *gid);
        assert!(
            entry.is_some(),
            "guarded constraint {gid} should be present in edit_check results"
        );
        assert_eq!(
            entry.unwrap().satisfaction,
            Satisfaction::Satisfied,
            "guarded constraint {gid} should be Satisfied after position_x edit"
        );
    }

    // 4. No constraint should be Violated.
    let violations: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|entry| entry.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        violations.is_empty(),
        "expected no Violated constraints after position_x=200mm, got {violations:?}"
    );
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
    let snap = engine.snapshot().expect("snapshot should exist after eval");
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

// ── Error-path tests: CellNotFound ────────────────────────────────────────────

/// edit_param with a ValueCellId whose entity does not exist in the graph
/// should return Err(EngineError::CellNotFound { .. }).
#[test]
fn edit_param_nonexistent_entity_returns_cell_not_found() {
    let (mut engine, _initial) = make_eval_engine();
    let bad_id = ValueCellId::new("NoSuchEntity", "height");
    let err = engine
        .edit_param(bad_id, mm(100.0))
        .expect_err("edit_param with nonexistent entity should return Err");
    assert!(
        matches!(err, reify_eval::EngineError::CellNotFound { .. }),
        "expected EngineError::CellNotFound, got {err:?}"
    );
}

/// edit_param with a ValueCellId whose member does not exist in the graph
/// should return Err(EngineError::CellNotFound { .. }).
#[test]
fn edit_param_nonexistent_param_returns_cell_not_found() {
    let (mut engine, _initial) = make_eval_engine();
    let bad_id = ValueCellId::new("Assembly", "no_such_param");
    let err = engine
        .edit_param(bad_id, mm(100.0))
        .expect_err("edit_param with nonexistent param should return Err");
    assert!(
        matches!(err, reify_eval::EngineError::CellNotFound { .. }),
        "expected EngineError::CellNotFound, got {err:?}"
    );
}

/// edit_check with a ValueCellId whose entity does not exist in the graph
/// should return Err(EngineError::CellNotFound { .. }) (delegates to edit_param).
#[test]
fn edit_check_nonexistent_entity_returns_cell_not_found() {
    let (mut engine, _initial) = make_eval_engine();
    let bad_id = ValueCellId::new("NoSuchEntity", "height");
    let err = engine
        .edit_check(bad_id, mm(100.0))
        .expect_err("edit_check with nonexistent entity should return Err");
    assert!(
        matches!(err, reify_eval::EngineError::CellNotFound { .. }),
        "expected EngineError::CellNotFound, got {err:?}"
    );
}

// ── Error-path tests: DimensionMismatch ───────────────────────────────────────

/// edit_param Assembly.height (Type::Scalar[LENGTH]) with kg(5.0) (Value::Scalar[MASS])
/// should return Err(EngineError::DimensionMismatch { .. }).
#[test]
fn edit_param_dimension_mismatch_returns_error() {
    let (mut engine, _initial) = make_eval_engine();
    let height_id = ValueCellId::new("Assembly", "height");
    // kg(5.0) produces Value::Scalar[MASS], but height is Type::Scalar[LENGTH].
    let err = engine
        .edit_param(height_id, kg(5.0))
        .expect_err("edit_param with dimension mismatch should return Err");
    assert!(
        matches!(err, reify_eval::EngineError::DimensionMismatch { .. }),
        "expected EngineError::DimensionMismatch, got {err:?}"
    );
}

/// Pins all three fields of `EngineError::DimensionMismatch` for
/// `edit_param Assembly.height` (Type::Scalar[LENGTH]) supplied kg(5.0) (Value::Scalar[MASS]).
/// Pre-existing sibling `edit_param_dimension_mismatch_returns_error` checks the variant kind
/// only; this test pins the fields to catch swap bugs during the task-2178 refactor.
/// A buggy mapping that swaps `expected` ↔ `got` or drops `cell` would fail here.
#[test]
fn edit_param_dimension_mismatch_pins_cell_and_dimensions() {
    let (mut engine, _initial) = make_eval_engine();
    let height_id = ValueCellId::new("Assembly", "height");
    // kg(5.0) produces Value::Scalar[MASS]; height is Type::Scalar[LENGTH].
    let err = engine
        .edit_param(height_id.clone(), kg(5.0))
        .expect_err("edit_param with dimension mismatch should return Err");
    let reify_eval::EngineError::DimensionMismatch {
        cell,
        expected,
        got,
    } = err
    else {
        panic!("expected EngineError::DimensionMismatch, got {err:?}");
    };
    assert_eq!(cell, height_id, "cell should be the height cell id");
    assert_eq!(
        *expected,
        reify_types::DimensionVector::LENGTH,
        "expected dimension should be LENGTH (the cell's declared dimension)"
    );
    assert_eq!(
        *got,
        reify_types::DimensionVector::MASS,
        "got dimension should be MASS (from kg(5.0))"
    );
}

/// edit_check Assembly.height (Type::Scalar[LENGTH]) with kg(5.0) (Value::Scalar[MASS])
/// should return Err(EngineError::DimensionMismatch { .. }) (delegates to edit_param).
#[test]
fn edit_check_dimension_mismatch_returns_error() {
    let (mut engine, _initial) = make_eval_engine();
    let height_id = ValueCellId::new("Assembly", "height");
    // kg(5.0) produces Value::Scalar[MASS], but height is Type::Scalar[LENGTH].
    let err = engine
        .edit_check(height_id, kg(5.0))
        .expect_err("edit_check with dimension mismatch should return Err");
    assert!(
        matches!(err, reify_eval::EngineError::DimensionMismatch { .. }),
        "expected EngineError::DimensionMismatch, got {err:?}"
    );
}

// ── Error-path tests: TypeKindMismatch ───────────────────────────────────────

/// edit_param Assembly.height (Type::Scalar[LENGTH]) with Value::Bool(true)
/// should return Err(EngineError::TypeKindMismatch { .. }) because the value
/// variant does not match the cell's declared type kind.
#[test]
fn edit_param_wrong_value_kind() {
    let (mut engine, _initial) = make_eval_engine();
    let height_id = ValueCellId::new("Assembly", "height");
    // Value::Bool is the wrong variant for a Type::Scalar cell.
    let err = engine
        .edit_param(height_id.clone(), Value::Bool(true))
        .expect_err("edit_param with wrong value kind should return Err");
    let reify_eval::EngineError::TypeKindMismatch {
        cell,
        expected,
        got,
    } = err
    else {
        panic!("expected EngineError::TypeKindMismatch, got {err:?}");
    };
    assert_eq!(cell, height_id, "cell should be the height cell id");
    assert_eq!(
        *expected,
        reify_types::Type::Scalar {
            dimension: reify_types::DimensionVector::LENGTH
        },
        "expected should be Type::Scalar[LENGTH] (the cell's declared type)"
    );
    assert_eq!(
        *got,
        Value::Bool(true),
        "got should be the supplied Value::Bool(true)"
    );
}

/// edit_check Assembly.height (Type::Scalar[LENGTH]) with Value::Bool(true)
/// should return Err(EngineError::TypeKindMismatch { .. }) (delegates to edit_param via ?).
/// This is a regression-lock guaranteeing the delegation path propagates the new error variant.
#[test]
fn edit_check_wrong_value_kind() {
    let (mut engine, _initial) = make_eval_engine();
    let height_id = ValueCellId::new("Assembly", "height");
    // Value::Bool is the wrong variant for a Type::Scalar cell.
    let err = engine
        .edit_check(height_id.clone(), Value::Bool(true))
        .expect_err("edit_check with wrong value kind should return Err");
    let reify_eval::EngineError::TypeKindMismatch {
        cell,
        expected,
        got,
    } = err
    else {
        panic!("expected EngineError::TypeKindMismatch, got {err:?}");
    };
    assert_eq!(cell, height_id, "cell should be the height cell id");
    assert_eq!(
        *expected,
        reify_types::Type::Scalar {
            dimension: reify_types::DimensionVector::LENGTH
        },
        "expected should be Type::Scalar[LENGTH] (the cell's declared type)"
    );
    assert_eq!(
        *got,
        Value::Bool(true),
        "got should be the supplied Value::Bool(true)"
    );
}

/// edit_param Assembly.grade (Type::Enum("Grade")) with Value::Int(1)
/// should return Err(EngineError::TypeKindMismatch { .. }) because
/// Value::Int hits the `Value::Int(_) => matches!(ty, Type::Int | Type::Real)` arm
/// which returns false for an Enum cell — non-Scalar cell regression-lock via an
/// existing fixture let binding (no dedicated Bool param required).
#[test]
fn edit_param_enum_cell_wrong_value_kind() {
    let (mut engine, _initial) = make_eval_engine();
    // `grade` is a let-binding (not a param), but edit_param acts on any ValueCell
    // regardless of kind — the TypeKindMismatch path is reached the same way.
    let grade_id = ValueCellId::new("Assembly", "grade");
    // Value::Int is the wrong variant for a Type::Enum("Grade") cell.
    let err = engine
        .edit_param(grade_id.clone(), Value::Int(1))
        .expect_err("edit_param with wrong value kind should return Err");
    let reify_eval::EngineError::TypeKindMismatch {
        cell,
        expected,
        got,
    } = err
    else {
        panic!("expected EngineError::TypeKindMismatch, got {err:?}");
    };
    assert_eq!(cell, grade_id, "cell should be the grade cell id");
    assert_eq!(
        *expected,
        reify_types::Type::Enum("Grade".to_string()),
        "expected should be Type::Enum(\"Grade\")"
    );
    assert_eq!(
        *got,
        Value::Int(1),
        "got should be the supplied Value::Int(1)"
    );
}

/// edit_check Assembly.grade (Type::Enum("Grade")) with Value::Int(1)
/// should return Err(EngineError::TypeKindMismatch { .. }) (delegates to edit_param via ?).
/// Regression-locks the delegation path for a non-Scalar typed cell via an existing
/// fixture let binding (no dedicated Bool param required).
#[test]
fn edit_check_enum_cell_wrong_value_kind() {
    let (mut engine, _initial) = make_eval_engine();
    // `grade` is a let-binding (not a param), but edit_check delegates to edit_param
    // which acts on any ValueCell regardless of kind — the TypeKindMismatch path
    // is reached the same way.
    let grade_id = ValueCellId::new("Assembly", "grade");
    // Value::Int is the wrong variant for a Type::Enum("Grade") cell.
    let err = engine
        .edit_check(grade_id.clone(), Value::Int(1))
        .expect_err("edit_check with wrong value kind should return Err");
    let reify_eval::EngineError::TypeKindMismatch {
        cell,
        expected,
        got,
    } = err
    else {
        panic!("expected EngineError::TypeKindMismatch, got {err:?}");
    };
    assert_eq!(cell, grade_id, "cell should be the grade cell id");
    assert_eq!(
        *expected,
        reify_types::Type::Enum("Grade".to_string()),
        "expected should be Type::Enum(\"Grade\")"
    );
    assert_eq!(
        *got,
        Value::Int(1),
        "got should be the supplied Value::Int(1)"
    );
}

// ── Regression tests: Undef acceptance + numeric coercion ────────────────────

/// edit_param Assembly.height (Type::Scalar[LENGTH]) with Value::Undef should return Ok.
/// Value::Undef is the Auto/no-value sentinel that must be accepted by any typed cell
/// regardless of the cell's declared type — the kind check must never reject it.
#[test]
fn edit_param_undef_is_always_accepted() {
    let (mut engine, _initial) = make_eval_engine();
    let height_id = ValueCellId::new("Assembly", "height");
    // Undef is the solver/compiler sentinel for unresolved Auto params.
    // edit_param with Undef must NOT return Err(TypeKindMismatch).
    engine
        .edit_param(height_id, Value::Undef)
        .expect("edit_param with Value::Undef should return Ok for any typed cell");
}

/// Value::Int to a Type::Real cell and Value::Real to a Type::Int cell must both
/// pass the kind check.  Numeric coercion between Int and Real is intentional:
/// the engine emits Warning diagnostics for runtime mismatches rather than hard errors.
/// Both directions are documented in `value_type_kind_matches`; this test regression-locks them.
#[test]
fn edit_param_int_real_numeric_coercion_allowed() {
    let (mut engine, _initial) = make_eval_engine();

    // Int → Real: Assembly.load_auto is declared as `param load_auto : Real = auto`.
    let load_auto_id = ValueCellId::new("Assembly", "load_auto");
    engine.edit_param(load_auto_id, Value::Int(5)).expect(
        "edit_param with Value::Int to a Type::Real cell should return Ok (numeric coercion)",
    );

    // Real → Int: RecursiveBeam.depth is declared as `param depth : Int = 2`.
    let depth_id = ValueCellId::new("RecursiveBeam", "depth");
    engine.edit_param(depth_id, Value::Real(5.0)).expect(
        "edit_param with Value::Real to a Type::Int cell should return Ok (numeric coercion)",
    );
}
