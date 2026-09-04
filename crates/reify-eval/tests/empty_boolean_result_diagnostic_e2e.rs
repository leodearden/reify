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

use reify_core::Severity;
use reify_ir::ExportFormat;
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

/// Collect the Error-severity diagnostic messages from a build.
fn error_messages(result: &reify_eval::BuildResult) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect()
}

/// Assert at least one Error diagnostic carries the kernel-error framing
/// (`"geometry error"`, from engine_build.rs), names the result as `"empty"`,
/// and names the user-level operation the designer wrote (`op_word`).
fn assert_empty_boolean_error(result: &reify_eval::BuildResult, op_word: &str) {
    let errors = error_messages(result);
    assert!(
        errors
            .iter()
            .any(|m| m.contains("geometry error") && m.contains("empty") && m.contains(op_word)),
        "expected an Error diagnostic containing \"geometry error\", \"empty\" and \
         {op_word:?}; got errors: {errors:?}"
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

// --- RED: empty boolean results must be rejected ---

/// `intersection` of two non-overlapping circles yields an empty compound,
/// which today flows into `extrude()` and produces a 0-face STEP with exit 0
/// and no diagnostics. The designer must instead see an error.
#[test]
fn disjoint_intersection_emits_error_diagnostic_not_silent_empty_solid() {
    let source = r#"structure P {
    let a = circle(5mm)
    let b = translate(circle(5mm), 100mm, 0mm, 0mm)
    let empty = intersection(a, b)
    let solid = extrude(empty, 10mm)
}"#;
    let Some(result) = build_with_occt(source) else {
        return;
    };
    assert_empty_boolean_error(&result, "intersection");
}

/// `difference` whose tool fully contains the target consumes it entirely.
/// Boxes are centred on the origin (`make_box` in occt_wrapper.cpp), so a
/// 50mm cube fully swallows a concentric 5mm cube.
#[test]
fn disjoint_difference_fully_consuming_target_emits_error_diagnostic() {
    let source = r#"structure P {
    let empty = difference(box(5mm, 5mm, 5mm), box(50mm, 50mm, 50mm))
}"#;
    let Some(result) = build_with_occt(source) else {
        return;
    };
    assert_empty_boolean_error(&result, "difference");
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
