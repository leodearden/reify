//! Library-level build() golden for the GHR-ζ terminal observable: building
//! `examples/spec-shape-physical.ri` through a real-OCCT `Engine` must yield a
//! `Bracket : Physical` whose `mass` and `centroid` value cells are real,
//! dimensioned numbers (NOT `undef`), matching the analytic expectations for a
//! `box(10mm, 20mm, 30mm)` steel block (task 3608, PRD
//! `docs/prds/v0_3/geometry-handle-runtime.md` §8 Phase 6).
//!
//! **Why a library (`BuildResult`) golden, not a CLI golden.** The user-facing
//! `reify eval` path runs `cmd_eval` (`Engine::eval`, no kernel) — it does not
//! dispatch geometry queries, so a `reify eval` golden would pin `undef`, not
//! real mass/centroid (see the escalation ruling on task 3608; the literal
//! `reify eval` mass/centroid narrative is deferred to a follow-up that wires
//! `cmd_eval` to a kernel-backed `build()`). This golden instead drives the
//! `build()` path directly, formats the resulting value cells with a
//! deterministic, FP-jitter-tolerant renderer, and commits the canonical text —
//! delivering the verification intent (real mass ≈ 0.0471 kg + analytic
//! centroid, committed and regression-locked) within the library boundary.
//!
//! The compile-clean assertion runs unconditionally so a grammar/compile
//! regression fails on every runner; the kernel build + golden comparison are
//! gated on `reify_kernel_occt::OCCT_AVAILABLE` and skip cleanly otherwise
//! (mirrors `geometry_query_kernel_dispatch.rs`). Regenerate the golden with
//! `REIFY_REGENERATE_GOLDEN=1`.

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

/// The committed terminal-observable fixture (pre-1): `Bracket : Physical` with
/// `param geometry = box(10mm, 20mm, 30mm)` and `param material =
/// Steel_AISI_1045()`. Kept purpose-free — the demanded-tolerance / cache-hit
/// narrative lives in `geometry_query_kernel_dispatch.rs`, not here.
const SPEC_SHAPE_PHYSICAL: &str = include_str!("../../../examples/spec-shape-physical.ri");

/// Deterministic SI-float renderer for the golden. Snaps |v| < 1e-9 to the
/// literal `0` (the centered-box centroid components are analytically zero but
/// carry sub-nanometre OCCT integration jitter — relative formatting of a
/// near-zero would be non-reproducible), and otherwise emits 6-significant-digit
/// scientific notation, which is far coarser than OCCT's analytic-primitive
/// determinism yet absorbs last-bit jitter so the exact-match golden stays
/// stable across runs.
fn fmt_si(v: f64) -> String {
    if v.abs() < 1e-9 {
        "0".to_string()
    } else {
        format!("{v:.6e}")
    }
}

/// Render one `Bracket.<member>` value cell into a canonical, deterministic
/// line body. `Scalar` shows `<si> [<dimension>]`; `Point` shows
/// `point(<si>, …)`; the geometry handle collapses to a stable placeholder
/// (NEVER the volatile session `kernel_handle`, which would make the golden
/// non-reproducible); a `Material` instance surfaces only its dimensioned
/// `density` field. `undef` is rendered verbatim so a dispatch regression
/// (mass/centroid reverting to `undef`) is visible in the golden diff.
fn render_cell(result: &reify_eval::BuildResult, member: &str) -> String {
    match result.values.get(&ValueCellId::new("Bracket", member)) {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => format!("{} [{}]", fmt_si(*si_value), dimension),
        Some(Value::Point(components)) => {
            let parts: Vec<String> = components
                .iter()
                .map(|c| match c {
                    Value::Scalar { si_value, .. } => fmt_si(*si_value),
                    other => format!("{other:?}"),
                })
                .collect();
            format!("point({})", parts.join(", "))
        }
        Some(Value::GeometryHandle { .. }) => "<geometry handle>".to_string(),
        Some(Value::StructureInstance(data)) => match data.fields.get("density") {
            Some(Value::Scalar {
                si_value,
                dimension,
            }) => format!("Material(density = {} [{}])", fmt_si(*si_value), dimension),
            other => format!("Material(density = {other:?})"),
        },
        Some(Value::Undef) => "undef".to_string(),
        Some(other) => format!("{other:?}"),
        None => "<absent>".to_string(),
    }
}

/// Read the runtime SI `density` (kg·m⁻³) from the evaluated `Bracket.material`
/// cell, so the analytic-mass tie tracks the real material constant rather than
/// a hardcoded literal (per the plan's "derive from runtime density" rule).
fn material_density_si(result: &reify_eval::BuildResult) -> f64 {
    match result.values.get(&ValueCellId::new("Bracket", "material")) {
        Some(Value::StructureInstance(data)) => match data.fields.get("density") {
            Some(Value::Scalar { si_value, .. }) => *si_value,
            other => panic!("Bracket.material.density should be Scalar, got {other:?}"),
        },
        other => panic!("Bracket.material should be StructureInstance, got {other:?}"),
    }
}

/// Build `examples/spec-shape-physical.ri` through a real-OCCT `Engine` and pin
/// the formatted `mass` / `centroid` / `material` / `geometry` cells to the
/// committed golden `tests/golden/spec_shape_physical.txt`.
///
/// Asserts (beyond the exact golden match): the golden carries a real, MASS-
/// dimensioned mass equal to `analytic_box_volume × runtime_density` (NOT
/// `undef`), the analytic centroid of the centered box, and the stable
/// `<geometry handle>` placeholder (never a raw session kernel handle).
#[test]
fn spec_shape_physical_build_golden() {
    let compiled = parse_and_compile_with_stdlib(SPEC_SHAPE_PHYSICAL);
    assert!(
        errors_only(&compiled).is_empty(),
        "fixture should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT golden: OCCT not available");
        return;
    }

    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let rendered = format!(
        "Bracket.geometry = {}\n\
         Bracket.material = {}\n\
         Bracket.mass     = {}\n\
         Bracket.centroid = {}\n",
        render_cell(&result, "geometry"),
        render_cell(&result, "material"),
        render_cell(&result, "mass"),
        render_cell(&result, "centroid"),
    );

    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/spec_shape_physical.txt");

    if std::env::var("REIFY_REGENERATE_GOLDEN").is_ok() {
        std::fs::write(&golden_path, &rendered).expect("failed to write golden file");
        return;
    }

    let expected = std::fs::read_to_string(&golden_path).expect(
        "golden crates/reify-eval/tests/golden/spec_shape_physical.txt missing; \
         run once with REIFY_REGENERATE_GOLDEN=1",
    );
    assert_eq!(
        rendered, expected,
        "spec-shape-physical build() value-cell rendering drifted from the golden; \
         re-run with REIFY_REGENERATE_GOLDEN=1 to update"
    );

    // ── Defence-in-depth on the committed golden (checked against `expected`,
    //    so it fires even if someone regenerated against a regressed build). ──

    // (a) Real, analytic mass — NOT undef. Tie to box volume × runtime density.
    let box_v = 0.010 * 0.020 * 0.030; // 6.0e-6 m³
    let density = material_density_si(&result);
    let expected_mass = fmt_si(box_v * density);
    assert!(
        expected.contains(&format!("Bracket.mass     = {expected_mass} [")),
        "golden must pin a real MASS-dimensioned mass equal to analytic \
         volume × runtime density ({expected_mass}); got:\n{expected}"
    );
    assert!(
        !expected.contains("Bracket.mass     = undef"),
        "golden mass must not be undef — geometry-query dispatch regressed; \
         re-run with REIFY_REGENERATE_GOLDEN=1 after fixing.\ngolden:\n{expected}"
    );

    // (b) Analytic centroid of the centered box → point(0, 0, 0).
    assert!(
        expected.contains("Bracket.centroid = point(0, 0, 0)"),
        "golden must pin the centered-box centroid point(0, 0, 0); got:\n{expected}"
    );

    // (c) Stable geometry placeholder — never a raw session kernel handle.
    assert!(
        expected.contains("Bracket.geometry = <geometry handle>"),
        "golden must use the stable <geometry handle> placeholder; got:\n{expected}"
    );
}
