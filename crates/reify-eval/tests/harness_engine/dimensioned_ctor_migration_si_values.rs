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

const TOTS_OPTIMAL_PTP_EXAMPLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/trajectory/tots_optimal_ptp.ri"
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

/// Read a list-valued field, panicking on a non-list.
fn list_field<'a>(data: &'a StructureInstanceData, name: &str) -> &'a [Value] {
    match field(data, name) {
        Value::List(items) => items,
        other => panic!(
            "{}.{name} must be a Value::List, got {other:?}",
            data.type_name
        ),
    }
}

/// Destructure a value as a `StructureInstance` of the expected `type_name`.
fn as_structure<'a>(v: &'a Value, expected_type: &str, what: &str) -> &'a StructureInstanceData {
    let Value::StructureInstance(data) = v else {
        panic!("{what} must be a {expected_type} StructureInstance; got {v:?}")
    };
    assert_eq!(
        data.type_name, expected_type,
        "{what} has wrong type_name: expected {expected_type:?}, got {:?}",
        data.type_name
    );
    data
}

/// Extract the SI magnitude of a dimensioned `Value::Scalar`, checking the
/// dimension. Used only for SOLVER-derived quantities, whose magnitude is then
/// compared under an explicit tolerance at the call site — literal-derived
/// values go through `assert_dimensioned` and compare exactly.
fn si_of(v: &Value, expected_dim: DimensionVector, what: &str) -> f64 {
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension, expected_dim,
                "{what}: wrong dimension — expected {expected_dim:?}, got {dimension:?}"
            );
            *si_value
        }
        other => panic!("{what}: expected a dimensioned Value::Scalar, got {other:?}"),
    }
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

// ── examples/trajectory/tots_optimal_ptp.ri ───────────────────────────────────

/// `TotsOptimalPtp.shaper`'s three actuator/kinematic limits are dimensioned,
/// and the TOTS solve's re-baselined output is pinned.
///
/// ## The two kinds of assertion here, and why they differ
///
/// The three LIMIT pins are literal-derived — a unit literal in a ctor arg
/// converted to SI at parse time — so they compare EXACTLY via
/// `assert_dimensioned`:
///
///   * `max_force` 1000 → `1000N` is MAGNITUDE-PRESERVING. The bare literal was
///     already read as SI newtons, so dimensioning it changes nothing numeric.
///   * `velocity_limit` 300 → `300mm/s` (SI 0.3) and `acceleration_limit`
///     5000 → `5000mm/s^2` (SI 5.0) are the INTENDED 1000× change ruled by Leo
///     in esc-5758-2 (option B). A bare literal at a `Scalar<Velocity>` slot was
///     being read as SI, i.e. 300 m/s on a desktop-printer-class mechanism; the
///     mm-scale values are what the design physically means.
///
/// The SOLVER pin is different in kind. Re-timing the move against limits that
/// are 1000× smaller moves the optimal second waypoint from
/// t = 0.07745966692423394 s to t ≈ 0.98 s — a 12.6× change that is the
/// INTENDED outcome of that same ruling, not a regression. A repo-wide sweep
/// (grep for `0.0774` across crates/ and gui/) found NO existing assertion
/// pinning the old value: `tots_optimal_ptp_example_tests.rs` is compile-only
/// and `auto_type_param_determinism_tests.rs` merely SKIP_SETs this file on
/// PerfBudget. So the ruled change would otherwise land completely unpinned;
/// this is a NEW pin, not a re-baselined one.
///
/// The 1e-3 absolute band is derived from a measured reference computation
/// (`reify eval` on a migrated copy, reproducing the value named in
/// esc-5758-2), NOT guessed: loose enough that SQP float jitter across
/// platforms cannot flake it, and ~1000× tighter than the 0.0774 → 0.98 shift
/// it exists to detect. Deliberately NOT exact float equality — this is a
/// solver output, not a literal.
///
/// Release-gated for the same reason as
/// `printer_print_envelope_e2e.rs::printer_print_envelope_eval_e2e`: this
/// example is already SKIP_SET on PerfBudget for its ~13.4 s compile, and the
/// eval adds a full SQP solve on top. The `ignore` reason is a perf gate, not a
/// blocker, so it carries no `#NNNN` cite (same as the printer's).
#[cfg_attr(debug_assertions, ignore = "heavy TOTS SQP solve; release-only")]
#[test]
fn tots_optimal_ptp_example_trajectory_limits_are_dimensioned() {
    let source = std::fs::read_to_string(TOTS_OPTIMAL_PTP_EXAMPLE)
        .expect("examples/trajectory/tots_optimal_ptp.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);
    assert_compile_clean(&compiled, "examples/trajectory/tots_optimal_ptp.ri");

    let mut engine = make_simple_engine();
    // Required so `input_shape` dispatches through the real TOTS trampoline
    // (which re-times the move via solve_tots) rather than the body-inline echo
    // fallback — see input_shape_eval_e2e.rs:294-296. Without it the `optimal`
    // pin below would silently assert against an un-re-timed profile.
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let result = engine.eval(&compiled);

    // ── (a) The dimensioned ctor args (literal-derived, exact) ───────────────
    let shaper = structure_cell(&result.values, "TotsOptimalPtp", "shaper", "TOTSShaper");

    assert_dimensioned(
        field(shaper, "velocity_limit"),
        0.3,
        DimensionVector::VELOCITY,
        "TotsOptimalPtp.shaper.velocity_limit",
    );
    assert_dimensioned(
        field(shaper, "acceleration_limit"),
        5.0,
        DimensionVector::ACCELERATION,
        "TotsOptimalPtp.shaper.acceleration_limit",
    );

    let limits = list_field(shaper, "actuator_limits");
    assert_eq!(
        limits.len(),
        1,
        "TotsOptimalPtp.shaper.actuator_limits should hold exactly one JointLimit, got {}",
        limits.len()
    );
    let jl = as_structure(
        &limits[0],
        "JointLimit",
        "TotsOptimalPtp.shaper.actuator_limits[0]",
    );
    assert_dimensioned(
        field(jl, "max_force"),
        1000.0,
        DimensionVector::FORCE,
        "TotsOptimalPtp.shaper.actuator_limits[0].max_force",
    );

    // `JointLimit.joint` is DIMENSIONLESS (`param joint : Real`,
    // stdlib/trajectory.ri) — it is a joint INDEX, not a physical quantity — so
    // it correctly stays a bare `Value::Real`. Pinned positively so a future
    // "dimension every bare literal" sweep cannot silently break it.
    match field(jl, "joint") {
        Value::Real(r) => assert_eq!(
            *r, 0.0,
            "TotsOptimalPtp.shaper.actuator_limits[0].joint should be 0.0, got {r}"
        ),
        other => panic!(
            "TotsOptimalPtp.shaper.actuator_limits[0].joint must stay a BARE Value::Real — \
             `JointLimit.joint` is a dimensionless joint index (`param joint : Real`, \
             stdlib/trajectory.ri), not a physical quantity. Got {other:?}."
        ),
    }

    // ── (b) The re-baselined solver output (solver-derived, tolerance) ───────
    let optimal = structure_cell(
        &result.values,
        "TotsOptimalPtp",
        "optimal",
        "PiecewisePolynomialProfile",
    );
    let waypoints = list_field(optimal, "waypoints");
    assert_eq!(
        waypoints.len(),
        2,
        "TotsOptimalPtp.optimal should hold exactly 2 waypoints, got {}",
        waypoints.len()
    );
    let wp1 = as_structure(&waypoints[1], "Waypoint", "TotsOptimalPtp.optimal.waypoints[1]");
    let t1 = si_of(
        field(wp1, "t"),
        DimensionVector::TIME,
        "TotsOptimalPtp.optimal.waypoints[1].t",
    );

    assert!(
        (t1 - 0.98).abs() < 1e-3,
        "TotsOptimalPtp.optimal.waypoints[1].t = {t1} s, expected 0.98 s ± 1e-3.\n\
         Pre-migration this was 0.07745966692423394 s, against limits read as SI \
         (300 m/s, 5000 m/s²). The 12.6× increase is the INTENDED outcome of Leo's \
         esc-5758-2 option-B ruling — the mm-scale limits (300 mm/s, 5000 mm/s²) are the \
         physically intended values — not a regression.\n\
         If t is back near 0.0774, the ctor args at tots_optimal_ptp.ri:78/:79 have been \
         re-bared. If t is wildly different or the cell is missing, the TOTS solve itself \
         regressed."
    );
}
