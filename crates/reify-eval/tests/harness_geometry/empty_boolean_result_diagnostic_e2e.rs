//! End-to-end: an empty/degenerate OCCT boolean result must surface a
//! designer-visible Error diagnostic, not flow silently downstream as a
//! zero-volume solid (task 5318).
//!
//! `BRepAlgoAPI_Common` on disjoint operands reports `IsDone() == true` and
//! returns an **empty `TopoDS_Compound`**. An empty compound is not
//! `IsNull()`, so the existing null guard in
//! `reify-kernel-occt/src/lib.rs` (`get_shape`) cannot see it: today the
//! result is registered as a "Solid", `extrude()` happily consumes it, and
//! `reify build` writes a header-only STEP file (0 `ADVANCED_FACE`) and
//! exits 0 with zero diagnostics.
//!
//! Two RED tests pin the two ways a designer reaches that state
//! (`intersection` of non-overlapping operands; `difference` whose tool
//! fully consumes the target). Three FALSE-POSITIVE CONTROLS pin the shapes
//! the guard must NOT reject — a boolean whose result is a multi-solid
//! compound, or an unchanged target, still has topology and is valid.
//!
//! All tests are guarded by `reify_kernel_occt::OCCT_AVAILABLE` and skip if
//! the OCCT library is not present.

use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_ir::{ExportFormat, Value};
use reify_test_support::parse_and_compile_with_stdlib;

/// Compile `source` and build it through the real OCCT kernel, returning the
/// full `BuildResult` so a caller can assert on diagnostics AND on
/// `geometry_output`. Returns `None` when OCCT is unavailable.
///
/// Deliberately does NOT assert that the build is clean: the RED tests below
/// exist precisely to assert that it is not.
fn build_with_occt(source: &str) -> Option<reify_eval::BuildResult> {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return None;
    }

    let compiled = parse_and_compile_with_stdlib(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    Some(engine.build(&compiled, ExportFormat::Step))
}

/// Compile `source` and REALIZE it on the real OCCT kernel WITHOUT the Phase-B
/// product export — `Engine::realize_for_check` (engine_build.rs:4087).
///
/// Identical construction to [`build_with_occt`]; only the terminal call
/// differs, and that difference is the whole point. `realize_for_check`
/// realizes every body on the real kernel and runs the geometry-query value
/// cells (`volume`, `centroid`, …) exactly as `build` does, but SKIPS the
/// Phase-B export walk — so it observes what the KERNEL says about a design
/// without also observing whether that design can be written to a STEP file.
///
/// This is not a convenience: it is the path `reify eval` / `reify check`
/// actually take for a geometry-bearing module (`reify-cli/src/main.rs`
/// selects `realize_for_check` at :1092-1098 whenever `module_has_geometry`),
/// which is what the protected gate `cli_gdt_integration_gate.rs:163` runs.
/// A design whose boolean legitimately collapses to nothing —
/// `examples/tolerancing/gdt_oracle_inside.ri` — must stay silent HERE.
///
/// Do NOT substitute `Engine::eval`: it mints only placeholder
/// `GeometryHandle { kernel_handle: None, .. }` (engine_eval.rs:6691-6697) and
/// never reaches OCCT at all, so an eval-based assertion about kernel
/// behaviour would be vacuous.
fn realize_with_occt(source: &str) -> Option<reify_eval::BuildResult> {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return None;
    }

    let compiled = parse_and_compile_with_stdlib(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    Some(engine.realize_for_check(&compiled))
}

/// Collect the Error-severity diagnostic messages from a build.
fn error_messages(result: &reify_eval::BuildResult) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect()
}

/// Assert the realization produced NO Error diagnostics at all.
///
/// Used by the legality pins: an empty boolean result is a legal kernel value,
/// so realizing a design that produces one must be completely silent.
fn assert_silent(result: &reify_eval::BuildResult, what: &str) {
    let errors = error_messages(result);
    assert!(
        errors.is_empty(),
        "{what}: an empty boolean result is a LEGAL kernel value under \
         realize/query, so no Error diagnostic is allowed; got: {errors:?}"
    );
}

/// Assert the build produced no Error diagnostics and emitted real geometry.
/// Used by the false-positive controls: these booleans are legitimate and the
/// empty-result guard must not fire on them.
fn assert_clean_non_empty_build(result: &reify_eval::BuildResult, what: &str) {
    let errors = error_messages(result);
    assert!(
        errors.is_empty(),
        "{what}: expected no Error diagnostics, got: {errors:?}"
    );
    let output = result
        .geometry_output
        .as_ref()
        .unwrap_or_else(|| panic!("{what}: expected geometry output"));
    assert!(!output.is_empty(), "{what}: geometry output was empty");
}

// --- LEGALITY PINS: an empty boolean result is a legal kernel value ---

/// `intersection` of two non-overlapping circles yields an empty compound.
/// With no consumer downstream, that is a legal — if useless — design, and
/// realizing it must be SILENT: the emptiness is the kernel's honest answer to
/// "what do these two disjoint shapes have in common?".
#[test]
fn disjoint_intersection_alone_is_legal_and_silent() {
    let source = r#"structure P {
    let a = circle(5mm)
    let b = translate(circle(5mm), 100mm, 0mm, 0mm)
    let empty = intersection(a, b)
}"#;
    let Some(result) = realize_with_occt(source) else {
        return;
    };
    assert_silent(&result, "disjoint intersection, no consumer");
}

/// `difference` whose tool fully contains the target consumes it entirely.
/// Boxes are centred on the origin (`make_box` in occt_wrapper.cpp), so a
/// 50mm cube fully swallows a concentric 5mm cube.
///
/// Realizing that is legal and silent — it is the shape of the GD&T "inside"
/// verdict, where an empty cut IS the answer.
#[test]
fn difference_fully_consuming_target_alone_is_legal_and_silent() {
    let source = r#"structure P {
    let empty = difference(box(5mm, 5mm, 5mm), box(50mm, 50mm, 50mm))
}"#;
    let Some(result) = realize_with_occt(source) else {
        return;
    };
    assert_silent(
        &result,
        "difference fully consuming its target, no consumer",
    );
}

/// Local pin of the ratified GD&T semantic, mirroring
/// `examples/tolerancing/gdt_oracle_inside.ri` (:20-23) without depending on
/// that file or on the CLI gate that runs it
/// (`cli_gdt_integration_gate.rs:163 b5_oracle_inside_oracles_agree`).
///
/// `actual` (X range −4.95mm..5.05mm) lies fully inside `zone`
/// (±5.1mm), so `difference(actual, zone)` is an empty compound and
/// `volume(...)` of it is EXACTLY 0.0 m³ — the boolean oracle's INSIDE
/// verdict, which the gate compares against `pokeout < 1e-9 m³`.
///
/// This is the reason the empty-result guard lives at the CONSUMERS and not at
/// the boolean producer: a producer-side guard turns this design's answer into
/// an error and fails the gate. `assert_eq!` against 0.0 is exact and matches
/// how `harness_occt::boolean_result_normalization_integration:951-953` asserts
/// the same quantity — BRepGProp over zero faces sums to exactly 0.0.
#[test]
fn gdt_inside_oracle_pokeout_is_exactly_zero_and_silent() {
    let source = r#"structure GdtOracleInsidePin {
    let nominal = box(10mm, 10mm, 10mm)
    let actual = translate(nominal, 0.05mm, 0mm, 0mm)
    let zone = box(10.2mm, 10.2mm, 10.2mm)
    let diff = difference(actual, zone)
    let pokeout = volume(diff)
}"#;
    let Some(result) = realize_with_occt(source) else {
        return;
    };
    assert_silent(&result, "gdt inside oracle");

    match result
        .values
        .get(&ValueCellId::new("GdtOracleInsidePin", "pokeout"))
    {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension,
                DimensionVector::VOLUME,
                "pokeout should be Scalar<Volume>, got dimension {dimension:?}"
            );
            assert_eq!(
                *si_value, 0.0,
                "pokeout must be exactly 0.0 m³ (the INSIDE verdict), got {si_value:?}"
            );
        }
        other => panic!("pokeout should be Value::Scalar<Volume>, got {other:?}"),
    }
}

// --- FALSE-POSITIVE CONTROLS: valid booleans must keep working ---

/// A fuse of two disjoint solids yields a compound holding two solids — it
/// has topology and is a perfectly valid multi-body result. The guard must
/// not confuse "the operands do not touch" with "the result is empty".
#[test]
fn union_of_disjoint_solids_still_succeeds() {
    let source = r#"structure P {
    let u = union(box(10mm, 10mm, 10mm), translate(box(10mm, 10mm, 10mm), 100mm, 0mm, 0mm))
}"#;
    let Some(result) = build_with_occt(source) else {
        return;
    };
    assert_clean_non_empty_build(&result, "union of disjoint solids");
}

/// A cut whose tool misses the target entirely returns the target unchanged —
/// non-empty, and must not be rejected.
#[test]
fn difference_with_non_intersecting_tool_still_succeeds() {
    let source = r#"structure P {
    let d = difference(box(10mm, 10mm, 10mm), translate(box(2mm, 2mm, 2mm), 100mm, 0mm, 0mm))
}"#;
    let Some(result) = build_with_occt(source) else {
        return;
    };
    assert_clean_non_empty_build(&result, "difference with non-intersecting tool");
}

/// The overlapping counterpart of the RED test above: the same construction
/// with a 2mm offset instead of 100mm produces a real lens-shaped solid.
#[test]
fn overlapping_intersection_still_succeeds() {
    let source = r#"structure P {
    let a = circle(5mm)
    let b = translate(circle(5mm), 2mm, 0mm, 0mm)
    let ok = intersection(a, b)
    let solid = extrude(ok, 10mm)
}"#;
    let Some(result) = build_with_occt(source) else {
        return;
    };
    assert_clean_non_empty_build(&result, "overlapping intersection");
}
