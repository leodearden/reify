//! End-to-end, through `.ri` source: where an empty OCCT boolean result is
//! legal, and where it is refused (task 5318, ruling of 2026-09-08 /
//! esc-5318-7).
//!
//! `BRepAlgoAPI_Common` on disjoint operands — and `BRepAlgoAPI_Cut` whose
//! tool fully consumes its target — report `IsDone() == true` and hand back an
//! **empty `TopoDS_Compound`**. That is a LEGAL kernel value, not a failure:
//! `examples/tolerancing/gdt_oracle_inside.ri` DESIGNS on one, where an empty
//! cut IS the "inside" verdict and `volume()` of it is exactly 0.0. So the
//! guard belongs at the consumers that mint an artifact and cannot mint one
//! from nothing — never at the boolean producer. That consumer list, and the
//! stated reason for every deliberate exclusion, lives ONCE at
//! `reject_empty_input_shape` in `reify-kernel-occt/cpp/occt_wrapper.cpp` and
//! is pinned at kernel level by
//! `harness_occt::empty_shape_consumer_guard_integration`; this file is the
//! designer-facing half of the same boundary.
//!
//! Nothing upstream sees the emptiness: an empty compound is not `IsNull()`,
//! so `get_shape`'s null check in `reify-kernel-occt/src/lib.rs` cannot see
//! it, and `brep_kind_of_shape` (same file, :660) classifies it `Compound` —
//! a well-formed handle that flows downstream like any other.
//!
//! The verdict is therefore the CLI path's, not the design's; one `.ri` can be
//! both silent and fatal:
//!   `reify check` → `Engine::realize_for_check`  — writes nothing, SILENT
//!   `reify eval`  → `Engine::realize_for_check`  — writes nothing, SILENT
//!   `reify build` → `Engine::build(.., Step)`    — WRITES, so it FAILS on an
//!                                                  empty artifact
//! One helper below per path; why the two artifact-free ones keep distinct
//! names is on [`eval_with_occt`].
//!
//! Nine tests in four roles. LEGALITY PINS: two realize an empty boolean with
//! no consumer downstream and must be silent, and two assert the ratified
//! GD&T INSIDE verdict (`pokeout` exactly 0.0 m³) — one per artifact-free CLI
//! path, because the protected gate `cli_gdt_integration_gate.rs:163` runs
//! `reify eval` and nothing here covered it. CONSUMER GUARD: two build-path
//! failures — an empty profile fed to `extrude`, and an empty compound as a
//! design's only product body. FALSE-POSITIVE CONTROLS: three booleans that
//! must keep succeeding, holding "the operands do not touch" apart from "the
//! result is empty".
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
/// This is not a convenience: it is the path `reify check` actually takes for
/// a geometry-bearing module (`cmd_check` selects `realize_for_check` at
/// `reify-cli/src/main.rs`:1118). `reify check` ONLY — assert the `reify eval`
/// path through [`eval_with_occt`] below even though the two currently reach
/// the same entry point, because they are separately changeable CLI commands
/// and conflating them is what let the protected gate
/// `cli_gdt_integration_gate.rs:163` go red unobserved.
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

/// Compile `source` and drive it through the engine entry point `reify eval`
/// uses — `cmd_eval`'s `module_has_geometry` branch
/// (`reify-cli/src/main.rs`:2130-2146).
///
/// MIRROR, and deliberately so: `cmd_eval` is a CLI function this crate cannot
/// call, so the terminal call reached below must be kept identical to the
/// terminal call there. Whoever changes one changes both; this doc comment and
/// the line reference above are the link.
///
/// `reify eval` is the THIRD path, and its distinctness is exactly what this
/// file previously got wrong — it had helpers for two paths and pins for two
/// verdicts, and the third path was the one the protected gate runs:
///   `reify check` → `Engine::realize_for_check`  — writes nothing, must be SILENT
///   `reify eval`  → `Engine::realize_for_check`  — writes nothing, must be SILENT
///   `reify build` → `Engine::build(.., Step)`    — WRITES an artifact, must FAIL on an empty one
///
/// `cmd_eval` reached `build()` until task 5318 step-10, for the
/// post-processes that resolve the geometry-query value cells; it discarded
/// `geometry_output` under its own comment "reify eval is a value inspector
/// only" but still REPORTED that walk's export-only diagnostics, and exits
/// non-zero on any of them. `realize_for_check` runs the same post-processes
/// and skips only the export walk, so the value cells are unaffected. The two
/// artifact-free paths therefore CONVERGED on one entry point rather than
/// coinciding by accident — hence the delegation below instead of a re-spelt
/// construction. The names stay distinct because the two CLI commands are
/// independently changeable: if either ever diverges, this is where it shows.
fn eval_with_occt(source: &str) -> Option<reify_eval::BuildResult> {
    realize_with_occt(source)
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

/// Assert at least one Error diagnostic contains every word in `words`.
///
/// Deliberately does NOT assert a diagnostic COUNT: a design that both feeds an
/// empty shape to a consumer AND leaves no exportable product body legitimately
/// reports both failures, and a count assertion would fail for the right
/// behaviour.
fn assert_error_diagnostic_mentions(result: &reify_eval::BuildResult, words: &[&str], what: &str) {
    let errors = error_messages(result);
    assert!(
        errors.iter().any(|m| words.iter().all(|w| m.contains(w))),
        "{what}: expected an Error diagnostic containing all of {words:?}; got: {errors:?}"
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
///
/// PAIRED with `empty_boolean_as_the_only_body_fails_the_build_with_a_diagnostic`
/// below: SAME `.ri`, opposite verdicts. `realize_for_check` (what `reify
/// check` does) must stay silent; `build` (what `reify build` does) must fail,
/// because building means writing an artifact and there is no artifact to
/// write. The two together ARE the ruling's boundary — change one and you must
/// look at the other.
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
/// `actual` (X range −4.95mm..5.05mm) lies fully inside `zone` (±5.1mm), so
/// `difference(actual, zone)` is an empty compound and `volume(...)` of it is
/// EXACTLY 0.0 m³ — the boolean oracle's INSIDE verdict, which the gate
/// compares against `pokeout < 1e-9 m³`.
///
/// Shared by the two artifact-free CLI paths' pins below, which must agree on
/// it: one `.ri`, one expected verdict, asserted through two entry points.
const GDT_INSIDE_ORACLE_PIN: &str = r#"structure GdtOracleInsidePin {
    let nominal = box(10mm, 10mm, 10mm)
    let actual = translate(nominal, 0.05mm, 0mm, 0mm)
    let zone = box(10.2mm, 10.2mm, 10.2mm)
    let diff = difference(actual, zone)
    let pokeout = volume(diff)
}"#;

/// Assert the ratified INSIDE verdict for [`GDT_INSIDE_ORACLE_PIN`]: no Error
/// diagnostic at all, and `pokeout` resolved to EXACTLY 0.0 m³.
///
/// `assert_eq!` against 0.0 is exact and matches how
/// `harness_occt::boolean_result_normalization_integration:951-953` asserts the
/// same quantity — BRepGProp over zero faces sums to exactly 0.0. Resolving the
/// cell at all is half the assertion: a path that went silent by skipping the
/// post-processes would leave `pokeout` absent, and that must fail here too.
fn assert_gdt_inside_oracle_verdict(result: &reify_eval::BuildResult, what: &str) {
    assert_silent(result, what);

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
                "{what}: pokeout should be Scalar<Volume>, got dimension {dimension:?}"
            );
            assert_eq!(
                *si_value, 0.0,
                "{what}: pokeout must be exactly 0.0 m³ (the INSIDE verdict), got {si_value:?}"
            );
        }
        other => panic!("{what}: pokeout should be Value::Scalar<Volume>, got {other:?}"),
    }
}

/// `reify check`'s path over the ratified GD&T "inside" design.
///
/// This is the reason the empty-result guard lives at the CONSUMERS and not at
/// the boolean producer: a producer-side guard turns this design's answer into
/// an error and fails the gate.
#[test]
fn gdt_inside_oracle_pokeout_is_exactly_zero_and_silent() {
    let Some(result) = realize_with_occt(GDT_INSIDE_ORACLE_PIN) else {
        return;
    };
    assert_gdt_inside_oracle_verdict(&result, "gdt inside oracle, check path");
}

/// `reify eval`'s path over the SAME design, and the sibling the file was
/// missing: [`realize_with_occt`] above is `reify check` ONLY, so nothing here
/// covered the path the protected gate actually runs
/// (`cli_gdt_integration_gate.rs:163 b5_oracle_inside_oracles_agree` shells out
/// to `reify eval`, not `reify check`). The gate went red while every local pin
/// stayed green, which is the definition of a vacuous mirror.
///
/// Same verdict as the check-path pin, for the same reason: `reify eval`
/// writes no artifact, so an artifact-writing error has nothing to report on.
#[test]
fn gdt_inside_oracle_is_silent_on_the_eval_path() {
    let Some(result) = eval_with_occt(GDT_INSIDE_ORACLE_PIN) else {
        return;
    };
    assert_gdt_inside_oracle_verdict(&result, "gdt inside oracle, eval path");
}

// --- CONSUMER GUARD: a consumer that cannot accept an empty shape ---

/// The 2026-08-20 repro. `intersection` of two disjoint coplanar circles is a
/// legal empty result (see the legality pins above); feeding it to `extrude`
/// is not, because `extrude` mints a body and there is nothing to mint one
/// from. Today that reaches `BRepPrimAPI_MakePrism` unchallenged and the build
/// writes a header-only STEP with zero diagnostics and exit 0.
///
/// Uses `build_with_occt`: this IS a build-path defect.
#[test]
fn disjoint_intersection_then_extrude_emits_error_diagnostic() {
    let source = r#"structure P {
    let a = circle(5mm)
    let b = translate(circle(5mm), 100mm, 0mm, 0mm)
    let empty = intersection(a, b)
    let solid = extrude(empty, 10mm)
}"#;
    let Some(result) = build_with_occt(source) else {
        return;
    };
    assert_error_diagnostic_mentions(
        &result,
        &["geometry error", "empty", "profile"],
        "disjoint intersection then extrude",
    );
    assert!(
        result.geometry_output.is_none(),
        "a design whose only body failed to realize must emit no geometry, got {:?} bytes",
        result.geometry_output.as_ref().map(|o| o.len())
    );
}

/// The build-path counterpart of
/// `difference_fully_consuming_target_alone_is_legal_and_silent` above: SAME
/// `.ri`, opposite verdict. No extrude here, so the sweep guard is NOT what
/// fires — the empty compound is the design's ONLY product body, and Phase-B's
/// `product_bodies.len() == 1` arm (engine_build.rs:4945-4978) hands it
/// straight to `export_with_options`. Exporting it writes a header-only STEP
/// and exits 0, which is a phantom artifact.
///
/// This is the test that distinguishes the export guard from the sweep guards;
/// without it the export half has no designer-facing evidence.
#[test]
fn empty_boolean_as_the_only_body_fails_the_build_with_a_diagnostic() {
    let source = r#"structure P {
    let empty = difference(box(5mm, 5mm, 5mm), box(50mm, 50mm, 50mm))
}"#;
    let Some(result) = build_with_occt(source) else {
        return;
    };
    assert_error_diagnostic_mentions(
        &result,
        &["export error", "empty"],
        "empty boolean as the only product body",
    );
    assert!(
        result.geometry_output.is_none(),
        "a failed export must yield no geometry, got {:?} bytes",
        result.geometry_output.as_ref().map(|o| o.len())
    );
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
