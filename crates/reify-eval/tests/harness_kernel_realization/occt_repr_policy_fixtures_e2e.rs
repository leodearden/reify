//! BT-ζ2 / BT-ζ3 (task 8669; PRD docs/prds/v0_6/occt-crate-internals.md §5):
//! the consumer-side reading of the OCCT result-repr policy, built through
//! `Engine::build` with a real OCCT kernel.
//!
//! A translated profile is still a face, so `centroid` measures the face; a
//! translated disjoint union is still a compound and a swept wire is a shell,
//! so `fillet` rejects both at its Solid-input precondition rather than
//! filleting a multi-body aggregate or failing deep inside OCCT.

use reify_core::{DimensionVector, ValueCellId};
use reify_ir::Value;

use super::fixture_scaffolding::compile_and_build_with_occt;

const SOLID_INPUT_GUARD: &str = "local-feature operation requires a BRepKind::Solid input shape";

const CENTROID_FIXTURE: &str =
    "tests/prd-gate/fixtures/occt_internals_centroid_translated_profile.ri";
const DISJOINT_UNION_FIXTURE: &str =
    "tests/prd-gate/fixtures/occt_internals_fillet_translated_disjoint_union.ri";
const EXTRUDED_WIRE_FIXTURE: &str =
    "tests/prd-gate/fixtures/occt_internals_fillet_extruded_wire.ri";

/// Build the repo-relative `fixture` with real OCCT; `None` when OCCT is absent.
fn build(fixture: &str) -> Option<reify_eval::BuildResult> {
    let path = format!("{}/../../{fixture}", env!("CARGO_MANIFEST_DIR"));
    compile_and_build_with_occt(&path, fixture)
}

fn cell<'r>(result: &'r reify_eval::BuildResult, name: &str) -> Option<&'r Value> {
    result.values.get(&ValueCellId::new("Probe", name))
}

/// `Probe.<name>` as the SI coordinates of a point of three LENGTH scalars.
#[track_caller]
fn point_cell(result: &reify_eval::BuildResult, name: &str) -> [f64; 3] {
    let coordinate = |v: &Value| match v {
        Value::Scalar {
            si_value,
            dimension,
        } if *dimension == DimensionVector::LENGTH => *si_value,
        other => panic!("Probe.{name} coordinate must be a LENGTH scalar, got {other:?}"),
    };
    match cell(result, name) {
        Some(Value::Point(items)) if items.len() == 3 => [
            coordinate(&items[0]),
            coordinate(&items[1]),
            coordinate(&items[2]),
        ],
        other => panic!("Probe.{name} must be a 3-component Value::Point, got {other:?}"),
    }
}

#[track_caller]
fn assert_point_near(actual: [f64; 3], expected: [f64; 3], what: &str) {
    // The planar-face centroid is closed-form; c_plain measures ~3.5e-19 m off.
    const TOLERANCE_M: f64 = 1e-9;
    for axis in 0..3 {
        assert!(
            (actual[axis] - expected[axis]).abs() < TOLERANCE_M,
            "{what} should be {expected:?} within {TOLERANCE_M:e} m, got {actual:?}"
        );
    }
}

#[track_caller]
fn assert_undef(result: &reify_eval::BuildResult, name: &str) {
    assert_eq!(
        cell(result, name),
        Some(&Value::Undef),
        "Probe.{name} must be undef; diagnostics: {:#?}",
        result.diagnostics
    );
}

fn diagnostics_containing(result: &reify_eval::BuildResult, needle: &str) -> usize {
    result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains(needle))
        .count()
}

#[test]
fn centroid_of_a_translated_profile_is_the_moved_face_centroid() {
    let Some(result) = build(CENTROID_FIXTURE) else {
        return;
    };
    assert_point_near(point_cell(&result, "c_plain"), [0.0; 3], "Probe.c_plain");
    assert_point_near(
        point_cell(&result, "c_moved"),
        [0.01, 0.0, 0.0],
        "Probe.c_moved",
    );
}

#[test]
fn fillet_rejects_a_disjoint_union_whether_or_not_it_was_translated() {
    let Some(result) = build(DISJOINT_UNION_FIXTURE) else {
        return;
    };
    assert_undef(&result, "v_direct");
    assert_undef(&result, "v_moved");
    let rejected_fillets = 2;
    assert_eq!(
        diagnostics_containing(&result, &format!("{SOLID_INPUT_GUARD}, got Compound")),
        rejected_fillets,
        "each of the two fillets (direct and translated) must be rejected as a \
         Compound; diagnostics: {:#?}",
        result.diagnostics
    );
}

#[test]
fn fillet_rejects_an_extruded_wire_at_its_solid_input_precondition() {
    let Some(result) = build(EXTRUDED_WIRE_FIXTURE) else {
        return;
    };
    assert_undef(&result, "vf");
    assert!(
        diagnostics_containing(&result, SOLID_INPUT_GUARD) > 0,
        "the fillet of a swept wire must be rejected by the Solid-input guard; \
         diagnostics: {:#?}",
        result.diagnostics
    );
    assert_eq!(
        diagnostics_containing(&result, "There are no suitable edges"),
        0,
        "the guard must reject the swept wire before OCCT's fillet builder runs; \
         diagnostics: {:#?}",
        result.diagnostics
    );
}
