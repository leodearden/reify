//! Real-OCCT end-to-end pin test for `intersects(Geometry, Geometry) -> Bool`
//! (task 3612, PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 KGQ-γ).
//!
//! The fixture `examples/kernel_queries/intersects_smoke.ri` contains:
//!
//! ```ri
//! structure def IntersectsSmoke {
//!     let a         = box(10mm, 10mm, 10mm)
//!     let b_overlap = translate(box(10mm, 10mm, 10mm), 5mm, 0mm, 0mm)
//!     let b_far     = translate(box(10mm, 10mm, 10mm), 100mm, 0mm, 0mm)
//!     let overlapping = intersects(a, b_overlap)
//!     let apart       = intersects(a, b_far)
//! }
//! ```
//!
//! Box geometry: `a` is 10 mm × 10 mm × 10 mm centred at origin ⟹ spans ±5 mm
//! (±0.005 m in SI).  `b_overlap` translated 5 mm in X spans 0..10 mm in X —
//! positive-volume overlap with `a` (0..5 mm in X), so BRep min distance = 0.0 →
//! `intersects` = `true`.  `b_far` translated 100 mm in X spans 95..105 mm in X
//! — ~90 mm face gap from `a`, so BRep min distance ≈ 0.09 m > 0.0 →
//! `intersects` = `false`.
//!
//! Dispatch route (task 3612 design decision): routes through
//! `GeometryQuery::Distance{from,to}` classifying `d <= 0.0 → Bool`, identical
//! to the shipped `shapes_intersect` adapter
//! (`reify-kernel-occt/src/lib.rs:770`) and the `interferes_with` helper
//! (`geometry_ops.rs:1601`).
//!
//! The compilation check runs unconditionally so a grammar or compile regression
//! fails on every runner. The kernel build + assertion is gated on
//! `reify_kernel_occt::OCCT_AVAILABLE` — skips cleanly on runners without OCCT.
//!
//! Modelled on `crates/reify-eval/tests/harness_kernel_realization/kernel_queries_contains.rs` (Bool
//! assertion pattern) and `crates/reify-eval/tests/harness_kernel_realization/kernel_queries_distance_smoke.rs`
//! (unconditional compile check + OCCT-gated value assertions).
//!
//! The read/compile/skip-without-OCCT scaffolding this test runs on lives in
//! the neutral sibling `fixture_scaffolding` module, not here: it is also used
//! by `best_practices_clearance_oracle` (task #5982), and a shared helper owned
//! by whichever pin test happened to define it first would make narrowing or
//! deleting THIS test silently break an unrelated one.
//!
//! # Second pin: full containment (task #6269)
//!
//! This module also carries `intersects_and_distance_detect_full_containment`,
//! which pins that the same query pair answers FULL CONTAINMENT correctly: a
//! solid strictly nested inside another — no boundary contact anywhere —
//! reports `intersects = true` and `distance` EXACTLY `0.0`, not a positive
//! gap. That is currently-unpinned real behaviour, so a regression toward the
//! intuitive-but-wrong "nesting looks like a clear gap" answer would otherwise
//! land silently and quietly falsify the `constraint not fouls` idiom that
//! `examples/best_practices/clearance_oracle.ri` teaches.
//!
//! Measured on this branch with `reify eval` before the pin was written
//! (`outer` = 20 mm box, `inner` = 10 mm box, both centred ⟹ 5 mm clear on
//! every face; `disjoint` = 10 mm box translated 50 mm in X ⟹ 35 mm face gap):
//!
//! ```text
//! nested_hit = true    nested_gap = 0 m       (intersects/distance(outer, inner))
//! nested_hit_rev = true nested_gap_rev = 0 m  (arguments swapped)
//! apart_hit = false    apart_gap = 0.035 m    (disjoint control)
//! ```
//!
//! The zero is exact, not a rounded epsilon — probed in-language on the same
//! source: `g == 0mm` → true, `g > 0mm` → false, and `g * 1e12` → `0 m` (a
//! 1e-16 epsilon scaled by 1e12 would have printed 1e-4). Mechanism: OCCT's
//! `BRepExtrema_DistShapeShape` classifies solid containment explicitly via
//! `SolidTreatment`/`InnerSolution()` ("True if one of the shapes is a solid
//! and the other shape is completely or partially inside the solid") and
//! returns a literal 0.0 rather than a computed near-zero extremum — which is
//! what makes the `dist.Value() <= 0.0` classification in the wrapper's
//! `shapes_intersect` (`reify-kernel-occt/cpp/occt_wrapper.cpp:1906`, "non-
//! negative by construction") report `true` here.
//!
//! Unlike the KGQ-γ pin above, that test is driven by an INLINE source
//! (`CONTAINMENT_SOURCE`) rather than a corpus `.ri`: its geometry is the
//! SUBJECT of the assertion, not an exemplar for human readers, so shipping it
//! under `examples/` would make a test-only artifact user-facing and enrol it
//! in every corpus sweep for no reader benefit. Appending cells to
//! `intersects_smoke.ri` instead would mix the KGQ-γ overlap/apart/undef
//! contract with an unrelated containment contract in one fixture. The inline
//! source needs no `module` line: the missing-module diagnostic is a WARNING,
//! which `errors_only` filters out.

use reify_core::ValueCellId;
use reify_ir::Value;

use super::fixture_scaffolding::{build_source_with_occt, compile_and_build_with_occt};

const INTERSECTS_SMOKE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/intersects_smoke.ri"
);

/// Inline source for `intersects_and_distance_detect_full_containment` below.
/// Deliberately test-owned rather than a corpus `.ri` — see the module header.
///
/// `box()` is CENTRED, so `outer` spans ±10 mm and `inner` spans ±5 mm:
/// strictly nested, 5 mm clear on every face, NO boundary contact anywhere.
/// `disjoint` spans 45..55 mm in X — a 35 mm face gap from `outer`'s +10 mm
/// face — and is the control that proves the query surface is genuinely
/// answering rather than defaulting: without it, a kernel that reported
/// "touching" for every pair would still satisfy the containment assertions.
/// Both argument orders are present because OCCT's `SolidTreatment` arm
/// inspects one shape as "the solid" and the other as "the contained", so an
/// asymmetric regression is possible; asserting both is free.
///
/// Every operand is LET-BOUND, as the ARG SHAPE contract requires: an inline
/// call argument falls through `resolve_geometry_handle_arg` to `Value::Undef`
/// with NO diagnostic, which would make every assertion below vacuous.
const CONTAINMENT_SOURCE: &str = r#"
structure def ContainmentPin {
    let outer    = box(20mm, 20mm, 20mm)
    let inner    = box(10mm, 10mm, 10mm)
    let disjoint = translate(box(10mm, 10mm, 10mm), 50mm, 0mm, 0mm)

    let nested_hit     = intersects(outer, inner)
    let nested_gap     = distance(outer, inner)
    let nested_hit_rev = intersects(inner, outer)
    let nested_gap_rev = distance(inner, outer)

    let apart_hit = intersects(outer, disjoint)
    let apart_gap = distance(outer, disjoint)
}
"#;

/// Pins the user-observable signal for KGQ-γ: `intersects(Geometry, Geometry)`
/// on two 10 mm boxes must evaluate to `Value::Bool(true)` when the boxes have
/// positive-volume overlap, and `Value::Bool(false)` when they are well apart.
///
/// Dispatches via `GeometryQuery::Distance` classified `d <= 0.0`:
/// - `overlapping`: BRep distance = 0.0 (touching/overlapping) → `true`.
/// - `apart`: BRep distance ≈ 0.09 m (90 mm face gap) → `false`.
///
/// Skips cleanly (via early return) when OCCT is not available.
#[test]
fn intersects_smoke_evals_expected_booleans() {
    let Some(result) = compile_and_build_with_occt(
        INTERSECTS_SMOKE_PATH,
        "examples/kernel_queries/intersects_smoke.ri",
    ) else {
        return;
    };

    // Helper: assert a Bool cell on IntersectsSmoke equals the expected value.
    let assert_bool = |cell_name: &str, expected: bool| {
        let cell = ValueCellId::new("IntersectsSmoke", cell_name);
        let actual = result.values.get(&cell);
        assert_eq!(
            actual,
            Some(&Value::Bool(expected)),
            "IntersectsSmoke.{cell_name} should be Value::Bool({expected}), got: {actual:?}"
        );
    };

    // b_overlap translated 5mm in X → overlaps a (both span ±5mm centred at origin)
    // by 5mm in X → BRep min distance = 0.0 → intersects = true.
    assert_bool("overlapping", true);

    // b_far translated 100mm in X → spans 95..105mm in X, ~90mm face gap from a
    // → BRep min distance ≈ 0.09m > 0.0 → intersects = false.
    assert_bool("apart", false);

    // Pin §4 invariant #1: an inline geometry arg (CompiledExprKind::FunctionCall,
    // not ValueRef) is rejected by resolve_geometry_handle_arg → dispatch arm
    // short-circuits (returns None) → cell stays at its compiled default
    // (None or Value::Undef — never a Bool).  The short-circuit happens before
    // any kernel call, but engine.build() still requires OCCT for the other
    // geometry cells in this fixture, so the assertion lives here.
    let undef_cell = ValueCellId::new("IntersectsSmoke", "undef_inline");
    let undef_actual = result.values.get(&undef_cell);
    assert!(
        matches!(undef_actual, None | Some(&Value::Undef)),
        "IntersectsSmoke.undef_inline must be None or Value::Undef (inline arg \
         falls through resolve_geometry_handle_arg per §4 invariant #1), \
         got: {undef_actual:?}"
    );
}

/// Pins that `intersects` / `distance` DO detect full containment, in both
/// argument orders, with a disjoint control in the same source.
///
/// A solid strictly nested inside another — 5 mm clear on every face, no
/// boundary contact — reports `intersects = true` and `distance` EXACTLY
/// `0.0`, so `constraint not fouls` (the idiom
/// `examples/best_practices/clearance_oracle.ri` teaches) covers NESTING as
/// well as boundary-crossing overlap. Nothing in the repo pinned this before
/// task #6269, so a regression toward the intuitive-but-wrong "a nested solid
/// is 5 mm clear" answer would have landed silently.
///
/// Skips cleanly (via early return) when OCCT is not available.
///
/// RED until `build_source_with_occt` exists.
#[test]
fn intersects_and_distance_detect_full_containment() {
    let Some(result) =
        build_source_with_occt(CONTAINMENT_SOURCE, "containment pin (inline source)")
    else {
        return;
    };

    // Same closure shape as `intersects_smoke_evals_expected_booleans` above,
    // so both tests in this module fail with visually consistent output.
    let assert_bool = |cell_name: &str, expected: bool| {
        let cell = ValueCellId::new("ContainmentPin", cell_name);
        let actual = result.values.get(&cell);
        assert_eq!(
            actual,
            Some(&Value::Bool(expected)),
            "ContainmentPin.{cell_name} should be Value::Bool({expected}), got: {actual:?}"
        );
    };

    // Assert an exactly-zero LENGTH. NOT a tolerance: OCCT's
    // `SolidTreatment`/`InnerSolution()` path classifies containment and
    // returns a literal 0.0 rather than a computed near-zero extremum,
    // confirmed in-language on this branch (`g == 0mm` → true, `g > 0mm` →
    // false, `g * 1e12` → 0 m). An epsilon here would be strictly weaker and
    // would blur the very distinction this pin exists to make: "contained"
    // (0.0) vs "clear by the nesting margin" (a positive gap).
    let assert_exactly_zero_length = |cell_name: &str| {
        let cell = ValueCellId::new("ContainmentPin", cell_name);
        let actual = result.values.get(&cell);
        match actual {
            Some(Value::Scalar {
                si_value,
                dimension,
            }) if *dimension == reify_core::DimensionVector::LENGTH => {
                assert_eq!(
                    *si_value, 0.0,
                    "ContainmentPin.{cell_name} si_value should be EXACTLY 0.0 \
                     (full containment, not a positive nesting gap), got {si_value:.17e}"
                );
            }
            other => panic!(
                "ContainmentPin.{cell_name} should be Value::Scalar{{LENGTH, 0.0}}, got: {other:?}"
            ),
        }
    };

    // inner (±5mm) is strictly inside outer (±10mm) with no boundary contact,
    // yet the pair still reads as fouling with a zero gap — both orders.
    assert_bool("nested_hit", true);
    assert_exactly_zero_length("nested_gap");
    assert_bool("nested_hit_rev", true);
    assert_exactly_zero_length("nested_gap_rev");

    // Disjoint control, in the SAME source so a regression toward the imagined
    // blind spot (a positive nested distance / `false`) fails loudly alongside
    // the proof that this query surface discriminates at all.
    assert_bool("apart_hit", false);

    let apart_gap_cell = ValueCellId::new("ContainmentPin", "apart_gap");
    let apart_gap_actual = result.values.get(&apart_gap_cell);
    match apart_gap_actual {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) if *dimension == reify_core::DimensionVector::LENGTH => {
            // outer's +X face sits at 0.010 m and disjoint's -X face at
            // 0.050 - 0.005 = 0.045 m, so the gap is 0.035 m. Epsilon
            // rationale (unlike the nested case, this IS a tolerance — but a
            // derived one, not a tuned one): both closest features are
            // axis-aligned PLANAR faces at coordinates exactly representable in
            // decimal, so the extremum is a difference of two doubles carrying
            // ~1e-17 representation error. 1e-9 sits 8 orders above that noise
            // floor and 7 orders below the 35 mm signal.
            let expected = 0.035_f64;
            let epsilon = 1e-9;
            assert!(
                (si_value - expected).abs() < epsilon,
                "ContainmentPin.apart_gap si_value should be 0.035 (35 mm face gap), \
                 got {si_value:.15} (delta {delta:.3e})",
                delta = (si_value - expected).abs()
            );
        }
        other => panic!(
            "ContainmentPin.apart_gap should be Value::Scalar{{LENGTH, ≈0.035}}, got: {other:?}"
        ),
    }
}
