//! End-to-end test for the composed perforated-plate / sieve workload
//! (task 5213, step-9): `difference(plate, union_all(linear_pattern_2d, linear_pattern_2d))`.
//!
//! This is the eval-level analog of the task's observable signal — the
//! bread-and-butter workload the single-pass n-ary fuse (steps 1-8) accelerates.
//! It drives the four-pattern single-pass realizer through the FULL compile →
//! eval → OCCT build path on the shipped `examples/perforated_plate.ri` fixture,
//! and locks the realized solid's volume against an independent reference.
//!
//! ## Structure (mirrors `apply_transform_e2e.rs`'s OCCT-acceptance precedent)
//!
//! `Engine::build()` serializes geometry to STEP bytes and does NOT expose the
//! post-build kernel handle for queries (see the extended note in
//! `apply_transform_e2e.rs::apply_transform_occt_acceptance`). So geometry
//! CORRECTNESS is asserted the sanctioned way — by reconstructing the same
//! geometry directly on a fresh `OcctKernel` and querying it. The three tests
//! give layered coverage:
//!
//!   1. [`perforated_plate_compiles_and_evals_clean`] — loads the fixture,
//!      compiles it (no error diagnostics) and evaluates its value cells via the
//!      reify-test-support harness (`compile_source_named` / `make_engine` /
//!      `assert_no_eval_errors`). Needs no OCCT; this is also the RED trigger
//!      before the fixture exists (`fs::read_to_string` fails).
//!   2. [`perforated_plate_builds_to_step_occt`] — the composed workload realized
//!      end-to-end: compile the fixture → `Engine::build` with a real OCCT kernel
//!      → non-empty STEP output with no error diagnostics. Proves the
//!      `difference(plate, union_all(pattern, pattern))` path realizes through the
//!      single-pass fuse without error.
//!   3. [`perforated_plate_volume_and_topology_occt`] — reconstructs the SAME
//!      plate the fixture describes (in SI metres) directly on `OcctKernel` and
//!      asserts the realized volume equals `block − N·(hole ∩ block)` (exact for
//!      disjoint through-holes) and that the result is a valid single connected
//!      manifold body.
//!
//! ## Why the perforated result is NOT asserted `IsWatertight`
//!
//! `difference()` (`BRepAlgoAPI_Cut`) with a multi-solid tool returns a COMPOUND
//! wrapping the (single) perforated solid. OCCT's `IsWatertight` / `IsClosed`
//! guard on shape type — only SOLID / COMPSOLID / SHELL qualify (see
//! `occt_wrapper.cpp` `is_watertight`) — so both report `false` on the COMPOUND
//! even though the body is geometrically closed. The pinnable,
//! shape-type-independent witnesses are `IsManifold` + `IsOrientable` +
//! `IsConnected` (all true), matching the convention
//! `pattern_differential_integration.rs` and `conformance_integration.rs` already
//! follow: they leave the version/shape-type-dependent `IsWatertight` bool
//! unpinned for compound-wrapped multi-solid results. The solid plate BEFORE
//! perforation IS asserted watertight, as the "proper closed solid" anchor.

use std::fs;

use reify_core::Severity;
use reify_ir::{ExportFormat, GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;
use reify_test_support::{assert_no_eval_errors, compile_source_named, make_engine};

/// Absolute path to the shipped perforated-plate fixture.
const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/perforated_plate.ri"
);

/// Load the fixture source, panicking with a clear message if it is absent (the
/// RED state before step-10 authors it).
fn load_fixture() -> String {
    fs::read_to_string(FIXTURE_PATH)
        .unwrap_or_else(|e| panic!("{FIXTURE_PATH} should exist (authored in step-10): {e}"))
}

/// Return the Error-severity diagnostics from a slice, for precise failure text.
fn error_msgs(diags: &[reify_core::Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect()
}

// ── SI dimensions of the perforated plate (mirror examples/perforated_plate.ri) ──
//
// The fixture is authored in millimetres; the kernel works in SI metres, so a
// `40mm` plate is `0.04 m`. These constants reconstruct the identical geometry
// directly on `OcctKernel` (Part 2) so the realized volume can be queried — the
// value cannot be read back from `Engine::build`'s STEP bytes.
const PLATE_SIZE: f64 = 0.040; // square plate footprint (40 mm)
const THICKNESS: f64 = 0.004; // plate thickness (4 mm)
const HOLE_RADIUS: f64 = 0.002; // perforation radius (2 mm)
const HOLE_HEIGHT: f64 = THICKNESS * 3.0; // cylinder taller than the plate (12 mm)
const HOLE_Z: f64 = THICKNESS * -1.5; // shift so the hole fully pierces (-6 mm)
const GRID_SPACING: f64 = 0.010; // 10 mm pitch, both grids
/// Grid A: 3×3 with base instance at (−10 mm, −10 mm) ⇒ centres at {−10,0,10} mm².
const GRID_A_BASE: f64 = -0.010;
/// Grid B: 2×2 with base instance at (−5 mm, −5 mm) ⇒ centres at {−5,5} mm²,
/// interleaved into grid A's gaps (nearest A↔B centre distance √2·5 mm > Ø4 mm,
/// so every hole is disjoint — the removed volume is exactly N × (hole ∩ block)).
const GRID_B_BASE: f64 = -0.005;
/// Total perforations across both grids (3×3 + 2×2).
const HOLE_COUNT: f64 = 9.0 + 4.0;

/// Relative tolerance for the volume comparison. OCCT boolean volume numerics on
/// these axis-aligned cuts are exact to far within 1e-6.
const REL_TOL: f64 = 1e-6;

fn v(x: f64) -> Value {
    Value::Real(x)
}

fn volume_of(kernel: &mut OcctKernel, id: GeometryHandleId) -> f64 {
    let vol = kernel
        .query(&GeometryQuery::Volume(id))
        .expect("volume query should succeed")
        .as_f64()
        .expect("volume should be numeric");
    assert!(vol > 0.0, "volume must be positive, got {vol}");
    vol
}

fn query_bool(kernel: &OcctKernel, q: GeometryQuery, ctx: &str) -> bool {
    match kernel.query(&q) {
        Ok(Value::Bool(b)) => b,
        other => panic!("{ctx}: expected Ok(Bool(_)), got {other:?}"),
    }
}

/// Translate `src` by `(dx, dy, dz)` metres, returning a fresh handle id.
fn translated(kernel: &mut OcctKernel, src: GeometryHandleId, d: [f64; 3]) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Translate {
            target: src,
            dx: d[0],
            dy: d[1],
            dz: d[2],
        })
        .expect("translate should succeed")
        .id
}

/// Build one hole grid: a through-hole cylinder placed at `(base, base, HOLE_Z)`,
/// replicated as a `count × count` grid at `GRID_SPACING` pitch — the single-pass
/// `linear_pattern_2d` realizer under test.
fn hole_grid(kernel: &mut OcctKernel, base: f64, count: usize) -> GeometryHandleId {
    let cyl = kernel
        .execute(&GeometryOp::Cylinder {
            radius: v(HOLE_RADIUS),
            height: v(HOLE_HEIGHT),
        })
        .expect("cylinder should succeed")
        .id;
    let placed = translated(kernel, cyl, [base, base, HOLE_Z]);
    kernel
        .execute(&GeometryOp::LinearPattern2D {
            target: placed,
            direction1: [1.0, 0.0, 0.0],
            count1: count,
            spacing1: v(GRID_SPACING),
            direction2: [0.0, 1.0, 0.0],
            count2: count,
            spacing2: v(GRID_SPACING),
        })
        .expect("linear_pattern_2d should succeed")
        .id
}

// ── 1. compile + value-eval (no OCCT; RED trigger before the fixture exists) ─────

/// The fixture parses, compiles with no error diagnostics, and its value cells
/// evaluate cleanly under a kernel-less engine (the reify-test-support harness the
/// plan names). RED before step-10: [`load_fixture`] panics on the missing file.
#[test]
fn perforated_plate_compiles_and_evals_clean() {
    let source = load_fixture();
    let compiled = compile_source_named(&source, "perforated_plate");
    let errs = error_msgs(&compiled.diagnostics);
    assert!(
        errs.is_empty(),
        "perforated_plate.ri should compile with no errors, got: {errs:?}"
    );

    let mut engine = make_engine();
    let result = engine.eval(&compiled);
    assert_no_eval_errors(&result);
    assert!(
        !result.values.is_empty(),
        "eval should produce non-empty value cells for perforated_plate.ri"
    );
}

// ── 2. composed workload realized end-to-end through OCCT (STEP export) ───────────

/// Part 1: compile the fixture and build it with a real OCCT kernel — the composed
/// `difference(plate, union_all(pattern, pattern))` must realize to non-empty STEP
/// output with no error diagnostics, exercising the single-pass pattern fuse via
/// the full engine build path.
#[test]
fn perforated_plate_builds_to_step_occt() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping perforated_plate_builds_to_step_occt: OCCT not available");
        return;
    }

    let source = load_fixture();
    let compiled = compile_source_named(&source, "perforated_plate");
    let compile_errs = error_msgs(&compiled.diagnostics);
    assert!(
        compile_errs.is_empty(),
        "perforated_plate.ri should compile cleanly, got: {compile_errs:?}"
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let build_errs = error_msgs(&result.diagnostics);
    assert!(
        build_errs.is_empty(),
        "perforated_plate.ri should build with no error diagnostics, got: {build_errs:?}"
    );

    let output = result
        .geometry_output
        .expect("the perforated plate must produce STEP geometry output (non-Undef)");
    assert!(!output.is_empty(), "STEP output must be non-empty");
    let step = String::from_utf8(output).expect("STEP should be valid UTF-8");
    assert!(
        step.contains("ISO-10303-21"),
        "STEP output should carry the ISO-10303-21 header"
    );
}

// ── 3. realized-solid volume + topology correctness (direct OCCT reconstruction) ──

/// Part 2: reconstruct the SAME plate the fixture describes directly on
/// `OcctKernel` (since `Engine::build` exposes no post-build handle) and assert:
///
///   - the perforated volume equals `block − N·(hole ∩ block)` within `REL_TOL`,
///     where the single-hole cut volume is measured independently via OCCT
///     intersection (analytically a clean through-hole cylinder segment, and the
///     N disjoint holes each remove exactly that);
///   - the perforated body is a valid single connected manifold solid
///     (`IsManifold` ∧ `IsOrientable` ∧ `IsConnected`); the un-perforated plate is
///     itself watertight. (`IsWatertight` on the perforated body is intentionally
///     NOT pinned — see the module note: the `difference` result is a COMPOUND, a
///     shape type OCCT's watertight guard rejects by construction.)
#[test]
fn perforated_plate_volume_and_topology_occt() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping perforated_plate_volume_and_topology_occt: OCCT not available");
        return;
    }

    let mut kernel = OcctKernel::new();

    // The solid plate — a proper closed solid (watertight anchor).
    let plate = kernel
        .execute(&GeometryOp::Box {
            width: v(PLATE_SIZE),
            height: v(PLATE_SIZE),
            depth: v(THICKNESS),
        })
        .expect("plate box should succeed")
        .id;
    let plate_vol = volume_of(&mut kernel, plate);
    assert!(
        query_bool(&kernel, GeometryQuery::IsWatertight(plate), "plate"),
        "the un-perforated solid plate must be watertight"
    );

    // Two interleaved through-hole grids fused into one holes body (single-pass).
    let grid_a = hole_grid(&mut kernel, GRID_A_BASE, 3);
    let grid_b = hole_grid(&mut kernel, GRID_B_BASE, 2);
    let holes = kernel
        .execute(&GeometryOp::Union {
            left: grid_a,
            right: grid_b,
        })
        .expect("union_all of the two hole grids should succeed")
        .id;

    // Independent reference for the removed volume: one placed hole ∩ block, via
    // OCCT — a clean through-hole cylinder segment. All holes are disjoint and
    // identically positioned in Z, so the total removed volume is N × this.
    let ref_cyl = kernel
        .execute(&GeometryOp::Cylinder {
            radius: v(HOLE_RADIUS),
            height: v(HOLE_HEIGHT),
        })
        .expect("reference cylinder should succeed")
        .id;
    let ref_cyl = translated(&mut kernel, ref_cyl, [GRID_A_BASE, GRID_A_BASE, HOLE_Z]);
    let hole_cut = kernel
        .execute(&GeometryOp::Intersection {
            left: ref_cyl,
            right: plate,
        })
        .expect("hole ∩ block reference should succeed")
        .id;
    let hole_cut_vol = volume_of(&mut kernel, hole_cut);

    // The composed perforated plate.
    let perforated = kernel
        .execute(&GeometryOp::Difference {
            left: plate,
            right: holes,
        })
        .expect("difference(plate, holes) should succeed")
        .id;
    let perforated_vol = volume_of(&mut kernel, perforated);

    let expected = plate_vol - HOLE_COUNT * hole_cut_vol;
    let rel_err = (perforated_vol - expected).abs() / expected.abs().max(1.0);
    assert!(
        rel_err <= REL_TOL,
        "perforated volume {perforated_vol:.12} m³ must equal block − {HOLE_COUNT}·(hole∩block) \
         = {expected:.12} m³ (block={plate_vol:.12}, hole_cut={hole_cut_vol:.12}), rel_err={rel_err:.3e}"
    );

    // Valid single connected manifold body (the pinnable closed-solid witnesses).
    assert!(
        query_bool(&kernel, GeometryQuery::IsManifold(perforated), "perforated"),
        "the perforated plate must be manifold"
    );
    assert!(
        query_bool(&kernel, GeometryQuery::IsOrientable(perforated), "perforated"),
        "the perforated plate must be consistently orientable"
    );
    assert!(
        query_bool(&kernel, GeometryQuery::IsConnected(perforated), "perforated"),
        "the perforated plate must be a single connected body"
    );
}
