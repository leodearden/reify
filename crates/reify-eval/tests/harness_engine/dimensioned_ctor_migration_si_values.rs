//! Value-layer SI-magnitude + dimension pins for the dimensioned-construction
//! migration (task 5758 — PRD `docs/prds/v0_6/dimensioned-construction-strictness.md`
//! §6.3 / §11 task β).
//!
//! β migrates the corpus's bare ctor arg-sites at dimensioned param slots
//! (`Scalar<Force>`, `Scalar<Velocity>`, `Scalar<Acceleration>`, `Density`,
//! `Pressure`) from bare `Real` literals to dimensioned unit literals, so that
//! γ — which promotes the dimensioned-Scalar diagnostic family to a Warning —
//! lands on a green corpus. β itself adds NO gate and asserts NO severity.
//!
//! ## Why these assertions do NOT go through an f64 helper
//!
//! `printer_print_envelope_e2e.rs:70`'s `num()` folds `Value::Real`,
//! `Value::Int` and `Value::Scalar` into one `f64`. A pin written on it passes
//! identically before and after this migration, so it is blind to precisely the
//! "dimensioned vs bare `Real`" property this task exists to establish. Every
//! assertion here therefore destructures `Value::Scalar { si_value, dimension }`
//! EXPLICITLY and compares `dimension` against a named `DimensionVector`
//! constant (`reify_core::dimension`), so a wrong-dimension misparse fails as
//! loudly as a wrong magnitude. This is what makes INV-SF-7
//! (parse-is-value-faithful) checkable rather than inferred from
//! compile-cleanliness — the insufficiency recorded as decompose-addendum D2.
//!
//! ## Why this module lives under `harness_engine/`
//!
//! reify-eval is one of the crates in `harness_layout_consolidatable_crates`
//! (tests/infra/harness-layout-lib.sh), so a NEW top-level
//! `crates/reify-eval/tests/*.rs` binary would be an anti-re-accretion violation
//! (scripts/check-harness-baseline-registration.sh, task #5300) unless
//! grandfathered into the shrinking baseline ratchet — and growing that ratchet
//! works against the C1 consolidation direction. Tasks #5056, #5196, #5045 and
//! #5360 made exactly this choice for exactly this reason.

use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_ir::{StructureInstanceData, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── Path constants ────────────────────────────────────────────────────────────
//
// Same `CARGO_MANIFEST_DIR`-relative resolution as
// `printer_print_envelope_e2e.rs:55`.

const GUI_LARGE_ASSEMBLY_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../gui/test/fixtures/large_assembly.ri"
);

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Assert that `v` is a dimensioned `Value::Scalar` with exactly `expected_si`
/// in SI base units and exactly `expected_dim`.
///
/// `si_value` is compared for exact equality on purpose: every value pinned
/// through this helper is LITERAL-derived (a unit literal in a `.ri` ctor arg
/// converted to SI at parse time), not solver-derived, so there is no float
/// jitter to tolerate. Solver-derived quantities get an explicit tolerance at
/// their own call site instead.
///
/// The panic messages name the observed variant so a bare-`Real` regression —
/// i.e. an un-migrated or re-bared ctor arg — reads clearly instead of as a
/// generic match failure.
fn assert_dimensioned(v: &Value, expected_si: f64, expected_dim: DimensionVector, what: &str) {
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension, expected_dim,
                "{what}: wrong dimension — expected {expected_dim:?}, got {dimension:?}. \
                 The ctor arg parsed as a dimensioned Scalar but carries the wrong unit."
            );
            assert_eq!(
                *si_value, expected_si,
                "{what}: wrong SI magnitude — expected {expected_si}, got {si_value}. \
                 These are literal-derived values; a change here means the migrated \
                 unit literal does not denote the same physical quantity."
            );
        }
        Value::Real(r) => panic!(
            "{what}: still a BARE Value::Real({r}) — expected a dimensioned \
             Value::Scalar {{ si_value: {expected_si}, dimension: {expected_dim:?} }}. \
             This ctor arg has not been migrated to a unit literal (task 5758 / PRD \
             docs/prds/v0_6/dimensioned-construction-strictness.md §11 β)."
        ),
        other => panic!(
            "{what}: expected a dimensioned Value::Scalar {{ si_value: {expected_si}, \
             dimension: {expected_dim:?} }}, got {other:?}"
        ),
    }
}

/// Read a named field out of a `StructureInstance`'s field map, panicking with
/// the available field names if it is absent.
fn field<'a>(data: &'a StructureInstanceData, name: &str) -> &'a Value {
    data.fields.get(&name.to_string()).unwrap_or_else(|| {
        let mut present: Vec<&String> = data.fields.iter().map(|(k, _)| k).collect();
        present.sort();
        panic!(
            "{}.{name} field missing; present fields: {present:?}",
            data.type_name
        )
    })
}

/// Fetch a value cell and destructure it as a `StructureInstance` of the
/// expected `type_name`.
fn structure_cell<'a>(
    values: &'a reify_ir::ValueMap,
    structure: &str,
    member: &str,
    expected_type: &str,
) -> &'a StructureInstanceData {
    let id = ValueCellId::new(structure, member);
    let val = values
        .get(&id)
        .unwrap_or_else(|| panic!("{structure}.{member} cell missing from eval result"));
    let Value::StructureInstance(data) = val else {
        panic!("{structure}.{member} must be a {expected_type} StructureInstance; got {val:?}")
    };
    assert_eq!(
        data.type_name, expected_type,
        "{structure}.{member} has wrong type_name: expected {expected_type:?}, got {:?}",
        data.type_name
    );
    data
}

/// Assert a compiled module produced no `Severity::Error` diagnostics.
fn assert_compile_clean(compiled: &reify_compiler::CompiledModule, what: &str) {
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "{what} should compile with no Error diagnostics; got:\n{errors:#?}"
    );
}

// ── gui/test/fixtures/large_assembly.ri ───────────────────────────────────────

/// The three `Material` ctors in `gui/test/fixtures/large_assembly.ri` construct
/// `density` / `youngs_modulus` from dimensioned unit literals, at SI magnitudes
/// numerically identical to the pre-migration bare-`Real` values.
///
/// This test is the FIRST cargo gate that file has ever had — it closes
/// decompose-addendum D1's "no compile gate behind this fixture" blind spot.
/// Before this, the fixture was reached only by `debug_server.rs` and the GUI
/// visual scenario in `gui/test/visual/assertions.ts`, neither of which runs
/// under `cargo test`.
///
/// Not release-gated: this is a kernel-free Value-layer eval that costs almost
/// nothing, and it is the only cargo coverage this file will have.
///
/// The magnitudes below are the CURRENT evaluated magnitudes, measured at
/// branch HEAD before any edit (task 5758 pre-1). The pin is deliberately
/// magnitude-PRESERVING: it fails only on the missing dimension, and will fail
/// loudly if the migration changes a number.
#[test]
fn gui_large_assembly_fixture_materials_are_dimensioned() {
    let source = std::fs::read_to_string(GUI_LARGE_ASSEMBLY_FIXTURE)
        .expect("gui/test/fixtures/large_assembly.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);
    assert_compile_clean(&compiled, "gui/test/fixtures/large_assembly.ri");

    // The zero-Error assertion is a COMPILE-clean pre-condition only (same as
    // `printer_print_envelope_e2e.rs`'s). Do NOT extend it to `result.diagnostics`:
    // this fixture's structures are `Physical`, so their `centroid` cells are
    // geometry-consumer builtins, and those legitimately emit Error-severity
    // `EvalUnresolved` on the pure value-eval surface ("geometry-consumer builtins
    // require a realized geometry kernel and are only resolvable on the
    // build()/tessellate() path"). That is a pre-existing structural property of
    // evaluating a Physical fixture without a kernel, unrelated to task 5758's
    // migration — asserting on it would make this pin fail for a reason it does
    // not own. The `material` cells this test reads are pure value-layer and
    // resolve fine.
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // (structure, density SI kg·m⁻³, youngs_modulus SI Pa)
    let expected: [(&str, f64, f64); 3] = [
        ("BoxPart", 7850.0, 200_000_000_000.0),
        ("TubePin", 7850.0, 200_000_000_000.0),
        ("BasePlate", 2700.0, 69_000_000_000.0),
    ];

    for (structure, density_si, youngs_si) in expected {
        let material = structure_cell(&result.values, structure, "material", "Material");

        assert_dimensioned(
            field(material, "density"),
            density_si,
            DimensionVector::MASS_DENSITY,
            &format!("{structure}.material.density"),
        );
        assert_dimensioned(
            field(material, "youngs_modulus"),
            youngs_si,
            DimensionVector::PRESSURE,
            &format!("{structure}.material.youngs_modulus"),
        );
    }
}
