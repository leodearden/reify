//! Real-OCCT end-to-end dispatch pins for the whole-handle geometry queries
//! `volume()` / `area()` / `centroid()` / `bounding_box()` on a
//! `Value::GeometryHandle` (task 3608, GHR-ζ; PRD
//! `docs/prds/v0_3/geometry-handle-runtime.md` §8 Phase 6).
//!
//! Each test compiles an inline DSL structure that realizes a primitive
//! (`box`/`sphere`/`cylinder`) and binds a geometry-query `let` over it, builds
//! the module through a real-OCCT `Engine`, and asserts the resulting value
//! cell is the correct typed `Value` (`Scalar<Volume>` / `Scalar<Area>` /
//! `Point3<Length>` / `BoundingBox`) within an analytic tolerance.
//!
//! The compile-clean assertion runs unconditionally so a grammar/compile
//! regression fails on every runner; the kernel build + numeric assertions are
//! gated on `reify_kernel_occt::OCCT_AVAILABLE` and skip cleanly otherwise
//! (mirrors `kernel_queries_distance_smoke.rs`).
//!
//! **Placement convention:** Reify's `box(w,h,d)` is CENTERED at the origin
//! (`occt_wrapper.cpp` `make_box` uses corner `(-w/2,-h/2,-d/2)`), so
//! `box(10mm,20mm,30mm)` has centroid `(0,0,0)` and bounding box
//! `min(-5,-10,-15)mm` / `max(5,10,15)mm`. Volume and surface area are
//! placement-invariant. (The plan's corner-at-origin premise was a documented
//! assumption to confirm; the centered convention is authoritative here and is
//! consistent with `examples/kernel_queries/distance_box_point.ri`.)

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, manufacturing_purpose, parse_and_compile_with_stdlib};

/// Compile `source` (asserting no error-severity diagnostics), then — if OCCT
/// is available — build it through a real-OCCT `Engine` and return the
/// `BuildResult`. Returns `None` when OCCT is unavailable, signalling the caller
/// to skip the numeric assertions.
fn compile_and_build_occt(source: &str) -> Option<reify_eval::BuildResult> {
    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "fixture should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return None;
    }

    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    Some(engine.build(&compiled, ExportFormat::Step))
}

/// Assert `value` is a `Value::Scalar` of dimension `dim` whose `si_value` is
/// within 1e-6 relative of `expected` (which must be non-zero).
fn assert_scalar_rel(value: Option<&Value>, dim: DimensionVector, expected: f64, what: &str) {
    match value {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension, dim,
                "{what}: expected dimension {dim:?}, got {dimension:?}"
            );
            let rel = (si_value - expected).abs() / expected.abs();
            assert!(
                rel < 1e-6,
                "{what}: si_value {si_value:.12} not within 1e-6 relative of \
                 {expected:.12} (rel={rel:.3e})"
            );
        }
        other => panic!("{what}: expected Value::Scalar{{{dim:?}}}, got {other:?}"),
    }
}

/// Assert `value` is a `Value::Point` of exactly 3 length-dimensioned scalar
/// components, each within `tol` ABSOLUTE (in metres) of `expected[i]`. Uses
/// absolute tolerance because the centered-box centroid components are zero
/// (relative tolerance is undefined at zero).
fn assert_point_abs(value: Option<&Value>, expected: [f64; 3], tol: f64, what: &str) {
    match value {
        Some(Value::Point(components)) => {
            assert_eq!(
                components.len(),
                3,
                "{what}: expected 3 Point components, got {}",
                components.len()
            );
            for (i, (comp, exp)) in components.iter().zip(expected.iter()).enumerate() {
                match comp {
                    Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert_eq!(
                            *dimension,
                            DimensionVector::LENGTH,
                            "{what}: component {i} dimension should be LENGTH, got {dimension:?}"
                        );
                        assert!(
                            (si_value - exp).abs() < tol,
                            "{what}: component {i} si_value {si_value:.12} not within {tol:.1e} \
                             (absolute) of {exp:.12}"
                        );
                    }
                    other => {
                        panic!("{what}: component {i} should be Scalar<Length>, got {other:?}")
                    }
                }
            }
        }
        other => panic!("{what}: expected Value::Point, got {other:?}"),
    }
}

/// Assert `value` is a `Value::BoundingBox` whose `min` and `max` corners are
/// each a `Point3<Length>` within `tol` ABSOLUTE (in metres) of `expected_min`
/// / `expected_max`. Delegates each corner to [`assert_point_abs`].
fn assert_bbox_abs(
    value: Option<&Value>,
    expected_min: [f64; 3],
    expected_max: [f64; 3],
    tol: f64,
    what: &str,
) {
    match value {
        Some(Value::BoundingBox { min, max }) => {
            assert_point_abs(
                Some(min.as_ref()),
                expected_min,
                tol,
                &format!("{what} min"),
            );
            assert_point_abs(
                Some(max.as_ref()),
                expected_max,
                tol,
                &format!("{what} max"),
            );
        }
        other => panic!("{what}: expected Value::BoundingBox, got {other:?}"),
    }
}

// ── volume() ────────────────────────────────────────────────────────────────

const VOLUME_SOURCE: &str = r#"
structure def VolBox {
    let body = box(10mm, 20mm, 30mm)
    let v = volume(body)
}
structure def VolSphere {
    let body = sphere(10mm)
    let v = volume(body)
}
structure def VolCyl {
    let body = cylinder(10mm, 20mm)
    let v = volume(body)
}
"#;

/// `volume(handle)` dispatches to OCCT and yields `Scalar<Volume>` for box,
/// sphere, and cylinder primitives, matching the analytic volumes:
///   - box(10,20,30)mm  → 0.010·0.020·0.030          = 6.0e-6 m³
///   - sphere(10mm)      → (4/3)π·0.010³              ≈ 4.18879e-6 m³
///   - cylinder(10,20)mm → π·0.010²·0.020             ≈ 6.28319e-6 m³
#[test]
fn volume_dispatch_box_sphere_cylinder() {
    let Some(result) = compile_and_build_occt(VOLUME_SOURCE) else {
        return;
    };

    let box_v = 0.010 * 0.020 * 0.030;
    let sphere_v = (4.0 / 3.0) * std::f64::consts::PI * 0.010_f64.powi(3);
    let cyl_v = std::f64::consts::PI * 0.010_f64.powi(2) * 0.020;

    assert_scalar_rel(
        result.values.get(&ValueCellId::new("VolBox", "v")),
        DimensionVector::VOLUME,
        box_v,
        "volume(box(10,20,30)mm)",
    );
    assert_scalar_rel(
        result.values.get(&ValueCellId::new("VolSphere", "v")),
        DimensionVector::VOLUME,
        sphere_v,
        "volume(sphere(10mm))",
    );
    assert_scalar_rel(
        result.values.get(&ValueCellId::new("VolCyl", "v")),
        DimensionVector::VOLUME,
        cyl_v,
        "volume(cylinder(10mm,20mm))",
    );
}

// ── area() ──────────────────────────────────────────────────────────────────

const AREA_SOURCE: &str = r#"
structure def AreaBox {
    let body = box(10mm, 20mm, 30mm)
    let a = area(body)
}
structure def AreaSphere {
    let body = sphere(10mm)
    let a = area(body)
}
structure def AreaCyl {
    let body = cylinder(10mm, 20mm)
    let a = area(body)
}
"#;

/// `area(handle)` dispatches to OCCT and yields `Scalar<Area>` for box, sphere,
/// and cylinder primitives, matching the analytic surface areas:
///   - box(10,20,30)mm  → 2(lw+lh+wh)        = 0.0022 m²
///   - sphere(10mm)      → 4π·0.010²          ≈ 1.256637e-3 m²
///   - cylinder(10,20)mm → 2πr·h + 2π·r²      ≈ 1.884956e-3 m²
#[test]
fn area_dispatch_box_sphere_cylinder() {
    let Some(result) = compile_and_build_occt(AREA_SOURCE) else {
        return;
    };

    let box_a = 2.0 * (0.010 * 0.020 + 0.010 * 0.030 + 0.020 * 0.030);
    let sphere_a = 4.0 * std::f64::consts::PI * 0.010_f64.powi(2);
    let cyl_a = 2.0 * std::f64::consts::PI * 0.010 * 0.020 // lateral
        + 2.0 * std::f64::consts::PI * 0.010_f64.powi(2); // two caps

    assert_scalar_rel(
        result.values.get(&ValueCellId::new("AreaBox", "a")),
        DimensionVector::AREA,
        box_a,
        "area(box(10,20,30)mm)",
    );
    assert_scalar_rel(
        result.values.get(&ValueCellId::new("AreaSphere", "a")),
        DimensionVector::AREA,
        sphere_a,
        "area(sphere(10mm))",
    );
    assert_scalar_rel(
        result.values.get(&ValueCellId::new("AreaCyl", "a")),
        DimensionVector::AREA,
        cyl_a,
        "area(cylinder(10mm,20mm))",
    );
}

// ── centroid() ────────────────────────────────────────────────────────────────

const CENTROID_SOURCE: &str = r#"
structure def CentroidBox {
    let body = box(10mm, 20mm, 30mm)
    let c = centroid(body)
}
"#;

/// `centroid(handle)` dispatches to OCCT and yields a `Point3<Length>`.
///
/// Reify's `box(w,h,d)` is CENTERED at the origin (`occt_wrapper.cpp`
/// `make_box` corner `(-w/2,-h/2,-d/2)`), so the centroid of
/// `box(10,20,30)mm` is `(0,0,0)` — NOT `(5,10,15)mm`. (The plan's
/// corner-at-origin premise was an assumption to confirm; the centered
/// convention is authoritative and matches `distance_box_point.ri`.)
#[test]
fn centroid_dispatch_box_is_origin() {
    let Some(result) = compile_and_build_occt(CENTROID_SOURCE) else {
        return;
    };

    assert_point_abs(
        result.values.get(&ValueCellId::new("CentroidBox", "c")),
        [0.0, 0.0, 0.0],
        1e-6,
        "centroid(box(10,20,30)mm)",
    );
}

// ── bounding_box() ────────────────────────────────────────────────────────────

const BBOX_SOURCE: &str = r#"
structure def BBoxBox {
    let body = box(10mm, 20mm, 30mm)
    let bb = bounding_box(body)
}
"#;

/// `bounding_box(handle)` dispatches to OCCT and yields a `Value::BoundingBox`
/// of two `Point3<Length>` corners.
///
/// Reify's `box(w,h,d)` is CENTERED at the origin (`occt_wrapper.cpp`
/// `make_box` corner `(-w/2,-h/2,-d/2)`), so `box(10,20,30)mm` spans
/// `min(-5,-10,-15)mm` / `max(5,10,15)mm` — NOT `min(0,0,0)`. (Corrects the
/// plan's corner-at-origin premise; consistent with the centroid pin and
/// `distance_box_point.ri`.)
#[test]
fn bounding_box_dispatch_box() {
    let Some(result) = compile_and_build_occt(BBOX_SOURCE) else {
        return;
    };

    assert_bbox_abs(
        result.values.get(&ValueCellId::new("BBoxBox", "bb")),
        [-0.005, -0.010, -0.015],
        [0.005, 0.010, 0.015],
        1e-6,
        "bounding_box(box(10,20,30)mm)",
    );
}

// ── nested fold: mass = volume(geometry) * material.density ────────────────────

/// Read the runtime `density` (SI kg·m⁻³) from a structure's evaluated
/// `material` StructureInstance cell. Lets the expected mass track the actual
/// material constant (`Steel_AISI_1045` → 7850) rather than a hardcoded literal,
/// per the plan's "derive from runtime density" robustness requirement.
fn material_density_si(result: &reify_eval::BuildResult, structure: &str) -> f64 {
    match result.values.get(&ValueCellId::new(structure, "material")) {
        Some(Value::StructureInstance(data)) => match data.fields.get("density") {
            Some(Value::Scalar { si_value, .. }) => *si_value,
            other => panic!("{structure}.material.density should be Scalar, got {other:?}"),
        },
        other => panic!("{structure}.material should be StructureInstance, got {other:?}"),
    }
}

/// The committed terminal-observable fixture (pre-1). `Bracket : Physical`
/// inherits `mass = volume(geometry) * material.density` (a BinOp whose nested
/// `volume()` leaf must fold) and `centroid = centroid(geometry)` (a direct
/// call, GREEN since step-6).
const SPEC_SHAPE_PHYSICAL: &str = include_str!("../../../examples/spec-shape-physical.ri");

/// End-to-end over `examples/spec-shape-physical.ri`: the `centroid` cell folds
/// (direct call) and the `mass` cell folds its NESTED `volume(geometry)` leaf so
/// `Scalar<Volume> * Scalar<Density>` recomputes to `Scalar<Mass>`.
///
/// RED until the nested-fold extension lands: `centroid` is already real, but
/// `mass` stays `Undef` because `try_eval_geometry_query` only matches a
/// default_expr that is *directly* a geometry-query call, not a BinOp
/// containing one.
#[test]
fn spec_shape_physical_mass_and_centroid() {
    let Some(result) = compile_and_build_occt(SPEC_SHAPE_PHYSICAL) else {
        return;
    };

    // (a) centroid = centroid(geometry) — centered box(10,20,30)mm → (0,0,0).
    assert_point_abs(
        result.values.get(&ValueCellId::new("Bracket", "centroid")),
        [0.0, 0.0, 0.0],
        1e-6,
        "Bracket.centroid",
    );

    // (b) mass = volume(geometry) * material.density — nested fold. Expected =
    // analytic box volume (6.0e-6 m³) × runtime density (Steel 7850 → ≈0.0471 kg).
    let box_v = 0.010 * 0.020 * 0.030;
    let density = material_density_si(&result, "Bracket");
    assert_scalar_rel(
        result.values.get(&ValueCellId::new("Bracket", "mass")),
        DimensionVector::MASS,
        box_v * density,
        "Bracket.mass",
    );
}

// ── realization-cache re-run: mass/centroid survive the cache-hit build path ──

/// The geometry-query post-process (`post_process_geometry_queries`) lives
/// INSIDE `Engine::run_post_processes`, which executes on EVERY build path —
/// including a `RealizationCache`-hit build where `execute_realization_ops`
/// short-circuits the per-op kernel dispatch. This test pins that invariant:
/// building the spec-shape-physical module twice in the SAME `Engine` (with a
/// demanded tolerance so the cache actually engages) serves the second build
/// entirely from the cache (`last_dispatch_count() == 0`, no geometry rebuild)
/// while `mass` and `centroid` retain their correct numeric values — they do
/// NOT revert to `undef`.
///
/// **Why inject a manufacturing purpose.** The `RealizationCache` is keyed by
/// `(entity, ReprKind::BRep, demanded_tol)` and only engages when
/// `demanded_tol = Some(..)`. A purpose-free build leaves `demanded_tol = None`,
/// so the cache never populates and `last_dispatch_count()` would be nonzero on
/// every build — defeating the test premise (escalation ruling, task 3608,
/// Option 2A). We inject `manufacturing_purpose("manufacturing", 1µm)` onto the
/// COMPILED module and activate it against `Bracket`, which is invisible to the
/// committed `examples/spec-shape-physical.ri` fixture (kept purpose-free for
/// the golden) yet sets `demanded_tol = Some(1e-6)` so the cache short-circuit
/// fires on the second build. Mirrors the cache-hit pattern in
/// `multi_handle_engine_dispatch::last_dispatch_count_zero_on_cache_hit_second_build`.
#[test]
fn mass_and_centroid_survive_realization_cache_hit() {
    let mut compiled = parse_and_compile_with_stdlib(SPEC_SHAPE_PHYSICAL);
    assert!(
        errors_only(&compiled).is_empty(),
        "fixture should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT cache-hit assertions: OCCT not available");
        return;
    }

    // Inject (without mutating the committed .ri) a manufacturing purpose
    // demanding 1µm so `demanded_tol = Some(1e-6)` and the RealizationCache
    // engages keyed at `(Bracket, BRep, 1e-6)`.
    compiled
        .compiled_purposes
        .push(manufacturing_purpose("manufacturing", 1e-6));

    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));

    // Canonical user flow: eval → activate_purpose → build.
    let _eval = engine.eval(&compiled);
    engine.activate_purpose("manufacturing", "Bracket");

    // First build: cold cache → the box realization dispatches at least once.
    let build1 = engine.build(&compiled, ExportFormat::Step);
    let dispatches_first = engine.last_dispatch_count();
    assert!(
        dispatches_first >= 1,
        "expected the first build() to dispatch the box realization at least \
         once (cold cache); got last_dispatch_count()={dispatches_first}"
    );

    let box_v = 0.010 * 0.020 * 0.030;
    let density = material_density_si(&build1, "Bracket");
    assert_scalar_rel(
        build1.values.get(&ValueCellId::new("Bracket", "mass")),
        DimensionVector::MASS,
        box_v * density,
        "Bracket.mass (first build)",
    );
    assert_point_abs(
        build1.values.get(&ValueCellId::new("Bracket", "centroid")),
        [0.0, 0.0, 0.0],
        1e-6,
        "Bracket.centroid (first build)",
    );

    // Re-activate the purpose so the second build sees the same
    // `demanded_tol = Some(1e-6)` that populated the cache — `build()`'s
    // internal eval() clears `active_purpose_bindings` (mirrors the same
    // re-activation in multi_handle_engine_dispatch.rs).
    engine.activate_purpose("manufacturing", "Bracket");

    // Second build: served entirely from the RealizationCache — the cache-hit
    // short-circuit returns before the per-op loop, so no geometry is rebuilt.
    let build2 = engine.build(&compiled, ExportFormat::Step);
    let dispatches_second = engine.last_dispatch_count();
    assert_eq!(
        dispatches_second, 0,
        "expected the second build() to be served entirely from the \
         RealizationCache (cache-hit short-circuit → zero dispatches); got \
         last_dispatch_count()={dispatches_second} (first build saw \
         {dispatches_first})"
    );

    // The load-bearing assertion: the geometry-query post-process ran on the
    // cache-hit path too, so mass/centroid are STILL their correct numeric
    // values — they did not revert to undef despite no realization rebuild.
    assert_scalar_rel(
        build2.values.get(&ValueCellId::new("Bracket", "mass")),
        DimensionVector::MASS,
        box_v * density,
        "Bracket.mass (cache-hit second build)",
    );
    assert_point_abs(
        build2.values.get(&ValueCellId::new("Bracket", "centroid")),
        [0.0, 0.0, 0.0],
        1e-6,
        "Bracket.centroid (cache-hit second build)",
    );
}

// ── cross-cell factoring: a dependent cell does NOT fold (known limitation) ───

const CROSS_CELL_SOURCE: &str = r#"
structure def CrossCellFactored {
    let body = box(10mm, 20mm, 30mm)
    let v = volume(body)
    let m = v * 2
}
"#;

/// Regression pin for the documented CROSS-CELL limitation (see the module note
/// above `try_eval_geometry_query` in `geometry_ops.rs`): the nested fold fires
/// only when the geometry-query call is lexically inside the cell's OWN
/// `default_expr`. Here `v = volume(body)` is a direct query cell that folds to
/// `Scalar<Volume>`, but `m = v * 2` is a `BinOp` over `ValueRef(v)` containing
/// no geometry-query leaf — so `try_eval_geometry_query` returns `None` for `m`,
/// and because the pure eval pass already evaluated `m` while `v` was `Undef`
/// (and the post-process never re-evaluates dependents), `m` silently stays
/// `Undef`. This is a plausible idiomatic factoring, so it is locked here to
/// keep the limitation regression-visible: if a future fixpoint re-eval resolves
/// it, this test breaks and must be updated to assert the folded value.
#[test]
fn cross_cell_factored_dependent_stays_undef() {
    let Some(result) = compile_and_build_occt(CROSS_CELL_SOURCE) else {
        return;
    };

    // `v = volume(body)` — DIRECT geometry-query cell → folds to Scalar<Volume>.
    assert_scalar_rel(
        result
            .values
            .get(&ValueCellId::new("CrossCellFactored", "v")),
        DimensionVector::VOLUME,
        0.010 * 0.020 * 0.030,
        "v = volume(box) (direct cell folds)",
    );

    // `m = v * 2` references `v` by ValueRef — no query leaf in its own expr —
    // so the dependent cell is NOT folded and stays Undef (documented limitation,
    // NOT the real Scalar<Volume> that a fixpoint re-eval would produce).
    let m = result
        .values
        .get(&ValueCellId::new("CrossCellFactored", "m"));
    assert!(
        matches!(m, None | Some(Value::Undef)),
        "cross-cell dependent `m` should stay Undef (known limitation: this pass \
         does not re-evaluate geometry-query dependents); got {m:?}"
    );
}
