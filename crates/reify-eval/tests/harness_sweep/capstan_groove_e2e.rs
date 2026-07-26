//! End-to-end acceptance gate for the printer_v01 capstan's helical rope groove
//! (task #5454 — thread-hole δ, dogfood leaf 1; PRD
//! `docs/prds/v0_6/thread-hole-features.md` §6 row 11).
//!
//! Compiles the REAL design file `prj/printer_v01/dev_capstan.ri` through the
//! full source → parse → compile(stdlib+checked) → Engine(real
//! `OcctKernelHandle`) → tessellate pipeline and pins the stock-removal signal:
//! the volume the helical channel takes out of the drum blank must match the
//! Pappus prediction for a circular section of radius `groove_r` swept along a
//! helix, `ΔV ≈ π·groove_r²·L_helix` with
//! `L_helix = sqrt((2π·pitch_r·n)² + groove_len²)` and `n = groove_len / lead`.
//!
//! **No geometry number is hard-coded here.** Every input to the expected value
//! is read back out of the file's own evaluated cells (`pitch_r`, `groove_r`,
//! `lead`, `groove_len`), so a parameter edit moves the gate with the design
//! instead of going stale. That is also why the file exposes `blank_volume` /
//! `body_volume` as `volume()` cells (a legitimate stock-removal metric on a
//! machined part) rather than having the test recompute the blank's closed form
//! and thereby hard-code the blank tree's shape.
//!
//! Why the compile entry is `compile_with_stdlib_checked` and not the bare
//! `reify_compiler::compile` used by the sibling `helix_sweep_e2e` module: this
//! is a real design file, and it needs stdlib-resolved `pi`, `vec3`,
//! `transform3` and `orient_axis_angle`. `compile_with_stdlib_checked` is the
//! entry `reify eval` itself uses (crates/reify-cli/src/main.rs).
//!
//! This module lives inside the `harness_sweep` compile unit rather than as a
//! standalone `crates/reify-eval/tests/*.rs` binary: a new top-level test file
//! would fail `scripts/check-harness-baseline-registration.sh --from-git`
//! (the harness-layout baseline is a shrinking ratchet). `harness_sweep` is
//! also the thematically right home — it already carries #5342's
//! `helix_sweep_e2e`, whose `helix()` spine this design consumes.
//!
//! No other gate compiles anything under `prj/`, so this module is currently
//! also the only regression guard on `dev_capstan.ri` as a whole.

use reify_core::{DimensionVector, ModulePath, Severity, ValueCellId};
use reify_eval::TessellateResult;
use reify_ir::{Satisfaction, Value};
use std::f64::consts::PI;

/// The real design file under test, reached from this crate's manifest dir.
const DEV_CAPSTAN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../prj/printer_v01/dev_capstan.ri"
);

/// Relative tolerance on `ΔV` against the Pappus prediction (PRD §6 row 11).
///
/// The band is wide because the prediction is the *ideal* swept tube: the real
/// delta additionally loses the small segment of the cutter section that
/// emerges through the land surface (the groove mouth). `dev_capstan.ri` pins
/// that segment small via `constraint groove_mouth < groove_r * 0.2`, which is
/// what keeps this band reachable — a parameter edit that would falsify this
/// gate fails `reify check` first, with a local explanation.
const VOLUME_DELTA_REL_TOL: f64 = 0.15;

// ── Shared prologue ──────────────────────────────────────────────────────────

/// Load, parse, compile and tessellate `prj/printer_v01/dev_capstan.ri` with a
/// real OCCT kernel, asserting the pipeline is Error-diagnostic-free at every
/// stage. Callers must have already checked `reify_kernel_occt::OCCT_AVAILABLE`.
fn tessellate_dev_capstan() -> TessellateResult {
    let source = std::fs::read_to_string(DEV_CAPSTAN)
        .unwrap_or_else(|e| panic!("failed to read design file {DEV_CAPSTAN}: {e}"));

    // ---- Parse ----
    let parsed = reify_syntax::parse(&source, ModulePath::single("dev_capstan"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {DEV_CAPSTAN}: {:?}",
        parsed.errors
    );

    // ---- Compile (the CLI's entry: needs stdlib `pi` / `vec3` / `transform3`) ----
    let compiled = reify_compiler::compile_with_stdlib_checked(
        &parsed,
        &reify_constraints::SimpleConstraintChecker,
    );
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors in {DEV_CAPSTAN}: {compile_errors:#?}"
    );

    // ---- Tessellate with a real OCCT kernel via SingleKernelHolder ----
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(Box::new(planner)),
    );

    let result = engine.tessellate_realizations(&compiled);
    let geom_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        geom_errors.is_empty(),
        "unexpected geometry errors tessellating {DEV_CAPSTAN}: {geom_errors:#?}"
    );
    result
}

/// Read a `Value::Scalar` cell out of the tessellation's value map, asserting
/// its dimension, and return its SI value (metres / cubic metres).
fn scalar_cell(
    result: &TessellateResult,
    structure: &str,
    cell: &str,
    expected_dim: DimensionVector,
) -> f64 {
    let id = ValueCellId::new(structure, cell);
    match result.values.get(&id) {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension, expected_dim,
                "{structure}.{cell}: expected dimension {expected_dim:?}, got {dimension:?}"
            );
            *si_value
        }
        other => panic!(
            "{structure}.{cell} must be a Value::Scalar with dimension {expected_dim:?}, \
             got {other:?} — is the cell declared in {DEV_CAPSTAN}?"
        ),
    }
}

// ── PRD §6 row 11: the groove removes π·r²·L of stock ────────────────────────

/// The helical channel cut into the capstan drum must remove a volume matching
/// the swept-tube prediction `π·groove_r²·L_helix` within ±15 %, with every
/// input read from the design file's own cells.
///
/// This is the consumer-visible "the groove is modelled for real" signal: a
/// smooth core (or a groove that fails to break through the land) removes no
/// stock at all, and a half-round surface seat removes only half the tube.
#[test]
fn capstan_groove_volume_delta_matches_pi_r2_l() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let result = tessellate_dev_capstan();

    // ---- Read the design's own cells (SI: m, m³) ----
    let blank_volume = scalar_cell(&result, "Capstan", "blank_volume", DimensionVector::VOLUME);
    let body_volume = scalar_cell(&result, "Capstan", "body_volume", DimensionVector::VOLUME);
    let pitch_r = scalar_cell(&result, "Capstan", "pitch_r", DimensionVector::LENGTH);
    let groove_r = scalar_cell(&result, "Capstan", "groove_r", DimensionVector::LENGTH);
    let lead = scalar_cell(&result, "Capstan", "lead", DimensionVector::LENGTH);
    let groove_len = scalar_cell(&result, "Capstan", "groove_len", DimensionVector::LENGTH);

    // ---- Pappus: unroll the helix to get its arc length ----
    let turns = groove_len / lead;
    let l_helix = ((2.0 * PI * pitch_r * turns).powi(2) + groove_len.powi(2)).sqrt();
    let expected = PI * groove_r.powi(2) * l_helix;
    let delta = blank_volume - body_volume;

    assert!(
        body_volume > 0.0,
        "grooved drum body must have positive volume, got {body_volume:.6e} m³"
    );
    assert!(
        delta > 0.0,
        "the groove must REMOVE stock: blank {blank_volume:.6e} m³ - body \
         {body_volume:.6e} m³ = {delta:.6e} m³ (a smooth drum gives 0)"
    );

    let rel_err = (delta - expected).abs() / expected;
    assert!(
        rel_err < VOLUME_DELTA_REL_TOL,
        "groove stock removal off the swept-tube prediction: ΔV = {delta:.6e} m³, \
         expected π·groove_r²·L = {expected:.6e} m³ (rel err {:.2} %, tol {:.0} %); \
         L_helix = {l_helix:.6} m over {turns:.4} turns \
         (pitch_r = {pitch_r:.6} m, groove_r = {groove_r:.6} m, lead = {lead:.6} m, \
         groove_len = {groove_len:.6} m)",
        rel_err * 100.0,
        VOLUME_DELTA_REL_TOL * 100.0
    );
}

// ── The viewport shows ONE grooved drum, and the design still checks clean ───

/// Entity-path prefix of the capstan's surfaces.
///
/// `Capstan` is contained by the file's `CapstanDrive` assembly (`sub capstan =
/// Capstan()`), so it does not surface as a root template — its bodies come
/// back in the composed descendant form `CapstanDrive.capstan#realization[i]`
/// (sub-placement Phase B). The realization index `i` is the same slot the
/// value map reports for the corresponding `Capstan.<let>` cell.
const CAPSTAN_SURFACE_PREFIX: &str = "CapstanDrive.capstan#realization[";

/// Modelling the groove for real means composing the drum from several named
/// intermediate bodies (profile, spine, cutter, blank). Exactly one of them —
/// the finished `body` — may surface in the viewport; the construction
/// geometry must be realized-but-hidden, or the consumer sees a pile of stray
/// meshes instead of a grooved drum.
///
/// Also pins that the design still checks clean at its defaults, so the
/// `groove_mouth` / `land_r` constraints added alongside the groove cannot
/// silently regress (equivalent to `reify check` reporting "All constraints
/// satisfied").
#[test]
fn capstan_surfaces_only_the_finished_drum() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let result = tessellate_dev_capstan();

    // The realization slot backing `Capstan.body` — resolved from the value map,
    // so the test never hard-codes a realization index.
    let body_path = match result.values.get(&ValueCellId::new("Capstan", "body")) {
        Some(Value::GeometryHandle {
            realization_ref, ..
        }) => format!("{CAPSTAN_SURFACE_PREFIX}{}]", realization_ref.index),
        other => panic!("Capstan.body must be a realized Value::GeometryHandle, got {other:?}"),
    };

    let capstan_surfaces: Vec<_> = result
        .meshes
        .iter()
        .filter(|s| s.entity_path.starts_with(CAPSTAN_SURFACE_PREFIX))
        .collect();
    assert!(
        !capstan_surfaces.is_empty(),
        "expected Capstan root-template surfaces, got none; all surfaces: {:?}",
        result
            .meshes
            .iter()
            .map(|s| &s.entity_path)
            .collect::<Vec<_>>()
    );

    let visible: Vec<_> = capstan_surfaces
        .iter()
        .filter(|s| s.default_visible)
        .collect();
    assert_eq!(
        visible.len(),
        1,
        "exactly one Capstan surface may be visible by default (the finished drum); \
         got {} visible out of {} — the groove's construction geometry \
         (groove_profile / groove_path / groove_cutter / drum_blank) must be `aux`. \
         Capstan surfaces (path, default_visible): {:?}",
        visible.len(),
        capstan_surfaces.len(),
        capstan_surfaces
            .iter()
            .map(|s| (&s.entity_path, s.default_visible))
            .collect::<Vec<_>>()
    );

    let drum = visible[0];
    assert_eq!(
        drum.entity_path, body_path,
        "the one visible Capstan surface must be the finished `body`, not a \
         construction body"
    );
    assert!(
        !drum.mesh.vertices.is_empty(),
        "the visible grooved drum must have vertices"
    );
    assert!(
        !drum.mesh.indices.is_empty(),
        "the visible grooved drum must have triangles"
    );

    // The design still checks clean at its defaults (`reify check` equivalent).
    let violated: Vec<_> = result
        .constraint_results
        .iter()
        .filter(|c| c.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        violated.is_empty(),
        "{DEV_CAPSTAN} must satisfy every constraint at its defaults; violated: {violated:#?}"
    );
}
