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
//! That prediction is checked at two very different resolutions, because the
//! ideal swept tube is not quite what gets removed — the sliver of the cutter
//! section that emerges through the land surface (the groove mouth) was never
//! inside the blank to begin with:
//!   1. `PAPPUS_REL_TOL` — the ideal tube within ±15 %, PRD §6 row 11 as
//!      literally written. Coarse; this is the conformance statement.
//!   2. `SEATED_SECTION_REL_TOL` — the same prediction with the analytic
//!      circular-segment mouth loss subtracted, within ±2 %. This is the
//!      regression-sensitive gate, and it models the geometry it measures
//!      rather than leaning on a design constraint to keep the coarse
//!      prediction valid.
//!
//! **No geometry number is hard-coded here.** Every input to the expected value
//! is read back out of the file's own evaluated cells (`pitch_r`, `groove_r`,
//! `groove_mouth`, `lead`, `groove_len`), so a parameter edit moves the gate
//! with the design instead of going stale. That is also why the file exposes
//! `blank_volume` / `body_volume` as `volume()` cells (a legitimate
//! stock-removal metric on a machined part) rather than having the test
//! recompute the blank's closed form and thereby hard-code the blank tree's
//! shape.
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
use std::sync::OnceLock;

/// The real design file under test, reached from this crate's manifest dir.
const DEV_CAPSTAN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../prj/printer_v01/dev_capstan.ri"
);

/// The design entity whose cells and constraints this module gates.
const CAPSTAN_ENTITY: &str = "Capstan";

/// Relative tolerance on `ΔV` against the IDEAL swept tube `π·groove_r²·L`.
///
/// This is PRD §6 row 11's conformance band verbatim. It is deliberately coarse:
/// the ideal tube over-predicts by whatever the groove mouth cuts off, so the
/// band has to swallow that (2.17 % at the file's defaults). Regression
/// sensitivity comes from [`SEATED_SECTION_REL_TOL`] instead — do NOT tighten
/// this one to compensate, it would stop meaning "row 11".
const PAPPUS_REL_TOL: f64 = 0.15;

/// Relative tolerance on `ΔV` against the mouth-corrected prediction
/// `(π·groove_r² − A_segment)·L`.
///
/// With the emergent groove-mouth segment subtracted analytically, the residual
/// is only the second-order terms the closed form ignores — land-surface
/// curvature across the ~2.6 mm mouth chord (vs. the flat-chord segment model)
/// and the ~2.7° helix obliquity. Measured on the file's defaults: −0.30 %, so
/// 2 % leaves ~6× headroom while still catching the failure modes this module
/// claims to catch (a groove_r off by 7 % moves ΔV by ~14 %; a noticeably
/// shallower seat moves it further).
const SEATED_SECTION_REL_TOL: f64 = 0.02;

// ── Shared prologue ──────────────────────────────────────────────────────────

/// The tessellated design, computed once per test binary.
///
/// The full parse → compile → spawn-OCCT → sweep → boolean → tessellate pipeline
/// costs ~5 s and both tests in this module only ever *read* the result, so it is
/// memoized rather than run per test (same `OnceLock` caching idiom as
/// `crates/reify-eval/tests/auto_type_param_determinism_tests.rs`). Callers must
/// have already checked `reify_kernel_occt::OCCT_AVAILABLE`.
fn dev_capstan() -> &'static TessellateResult {
    static R: OnceLock<TessellateResult> = OnceLock::new();
    R.get_or_init(tessellate_dev_capstan)
}

/// Load, parse, compile and tessellate `prj/printer_v01/dev_capstan.ri` with a
/// real OCCT kernel, asserting the pipeline is Error-diagnostic-free at every
/// stage. Use [`dev_capstan`] rather than calling this directly.
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

/// Read a `Value::Scalar` cell of [`CAPSTAN_ENTITY`] out of the tessellation's
/// value map, asserting its dimension, and return its SI value (m / m³).
fn capstan_cell(result: &TessellateResult, cell: &str, expected_dim: DimensionVector) -> f64 {
    let id = ValueCellId::new(CAPSTAN_ENTITY, cell);
    match result.values.get(&id) {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension, expected_dim,
                "{CAPSTAN_ENTITY}.{cell}: expected dimension {expected_dim:?}, got {dimension:?}"
            );
            *si_value
        }
        other => panic!(
            "{CAPSTAN_ENTITY}.{cell} must be a Value::Scalar with dimension {expected_dim:?}, \
             got {other:?} — is the cell declared in {DEV_CAPSTAN}?"
        ),
    }
}

/// Area of the circular segment of a radius-`r` disc lying beyond a chord whose
/// sagitta (segment height) is `h`: `r²·acos((r−h)/r) − (r−h)·sqrt(2rh − h²)`.
///
/// Here `r` is the swept cutter's section radius and `h` is `groove_mouth`, so
/// this is the slice of the cutter section that pokes out through the land
/// surface as the groove mouth — stock that was never in the blank, and so is
/// never removed from it. Valid for `0 ≤ h ≤ r`, which the design's own
/// `0mm < groove_mouth < groove_r * 0.2` constraints keep it inside.
///
/// The chord is modelled flat while the land is really a cylinder; over the
/// ~2.6 mm mouth chord at a ~26.7 mm land radius that approximation is worth a
/// few tenths of a percent of `ΔV`, which is what [`SEATED_SECTION_REL_TOL`]
/// budgets for.
fn circular_segment_area(r: f64, h: f64) -> f64 {
    let apothem = r - h; // centre-to-chord distance
    r * r * (apothem / r).acos() - apothem * (2.0 * r * h - h * h).sqrt()
}

// ── PRD §6 row 11: the groove removes π·r²·L of stock ────────────────────────

/// The helical channel cut into the capstan drum must remove a volume matching
/// the swept-tube prediction `π·groove_r²·L_helix` within ±15 % (PRD §6 row 11),
/// and its mouth-corrected refinement within ±2 %, with every input read from
/// the design file's own cells.
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

    let result = dev_capstan();

    // ---- Read the design's own cells (SI: m, m³) ----
    let blank_volume = capstan_cell(result, "blank_volume", DimensionVector::VOLUME);
    let body_volume = capstan_cell(result, "body_volume", DimensionVector::VOLUME);
    let pitch_r = capstan_cell(result, "pitch_r", DimensionVector::LENGTH);
    let groove_r = capstan_cell(result, "groove_r", DimensionVector::LENGTH);
    let groove_mouth = capstan_cell(result, "groove_mouth", DimensionVector::LENGTH);
    let lead = capstan_cell(result, "lead", DimensionVector::LENGTH);
    let groove_len = capstan_cell(result, "groove_len", DimensionVector::LENGTH);

    // ---- Pappus: unroll the helix to get its arc length ----
    let turns = groove_len / lead;
    let l_helix = ((2.0 * PI * pitch_r * turns).powi(2) + groove_len.powi(2)).sqrt();
    let pappus = PI * groove_r.powi(2) * l_helix;
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

    // ---- (1) PRD §6 row 11, as literally written: the ideal swept tube ± 15 % ----
    let pappus_err = (delta - pappus).abs() / pappus;
    assert!(
        pappus_err < PAPPUS_REL_TOL,
        "groove stock removal off the swept-tube prediction: ΔV = {delta:.6e} m³, \
         expected π·groove_r²·L = {pappus:.6e} m³ (rel err {:.2} %, tol {:.0} %); \
         L_helix = {l_helix:.6} m over {turns:.4} turns \
         (pitch_r = {pitch_r:.6} m, groove_r = {groove_r:.6} m, lead = {lead:.6} m, \
         groove_len = {groove_len:.6} m)",
        pappus_err * 100.0,
        PAPPUS_REL_TOL * 100.0
    );

    // ---- (2) The sensitive gate: subtract the analytic groove-mouth segment ----
    // The mouth sliver of the cutter section sweeps through air, not stock, so it
    // is not part of ΔV. Modelling it here (instead of widening the band, or
    // constraining the design to keep the naive prediction valid) is what makes
    // this assertion tight enough to catch a real modelling regression.
    let mouth_segment = circular_segment_area(groove_r, groove_mouth);
    let seated = (PI * groove_r.powi(2) - mouth_segment) * l_helix;
    let seated_err = (delta - seated).abs() / seated;
    assert!(
        seated_err < SEATED_SECTION_REL_TOL,
        "groove stock removal off the mouth-corrected prediction: ΔV = {delta:.6e} m³, \
         expected (π·groove_r² − A_seg)·L = {seated:.6e} m³ (rel err {:.2} %, tol \
         {:.0} %); A_seg = {mouth_segment:.6e} m² is {:.2} % of the π·groove_r² = \
         {:.6e} m² section, from groove_mouth = {groove_mouth:.6} m on groove_r = \
         {groove_r:.6} m; L_helix = {l_helix:.6} m over {turns:.4} turns",
        seated_err * 100.0,
        SEATED_SECTION_REL_TOL * 100.0,
        100.0 * mouth_segment / (PI * groove_r.powi(2)),
        PI * groove_r.powi(2)
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

    let result = dev_capstan();

    // The realization slot backing `Capstan.body` — resolved from the value map,
    // so the test never hard-codes a realization index.
    let body_path = match result.values.get(&ValueCellId::new(CAPSTAN_ENTITY, "body")) {
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
        "expected surfaces under {CAPSTAN_SURFACE_PREFIX}, got none; all surfaces: {:?}",
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

    // ---- The design still checks clean at its defaults (`reify check` equivalent) ----
    // Filtering for `Violated` alone would be vacuously green two ways: an empty
    // `constraint_results` (nothing checked at all), and `Indeterminate` — which is
    // precisely what an undef input produces, i.e. the symptom of a geometry or
    // scalar cell failing to evaluate. So assert positively instead.
    assert!(
        !result.constraint_results.is_empty(),
        "no constraints were checked at all — every structure in {DEV_CAPSTAN} \
         declares some, so an empty result means the check never ran"
    );

    let capstan_constraints: Vec<_> = result
        .constraint_results
        .iter()
        .filter(|c| c.id.entity == CAPSTAN_ENTITY)
        .collect();
    assert!(
        !capstan_constraints.is_empty(),
        "expected constraint results for entity `{CAPSTAN_ENTITY}`, got none; \
         entities checked: {:?}",
        result
            .constraint_results
            .iter()
            .map(|c| &c.id.entity)
            .collect::<Vec<_>>()
    );

    // Strict for `Capstan`: all of its constraint inputs are defined on the happy
    // path, so anything other than `Satisfied` — Violated OR Indeterminate — is a
    // regression in the groove_mouth / land_r work this module gates.
    let unsatisfied: Vec<_> = capstan_constraints
        .iter()
        .filter(|c| c.satisfaction != Satisfaction::Satisfied)
        .collect();
    assert!(
        unsatisfied.is_empty(),
        "every `{CAPSTAN_ENTITY}` constraint must be Satisfied at the file's defaults \
         ({} of {} were not; Indeterminate means an input cell failed to evaluate): \
         {unsatisfied:#?}",
        unsatisfied.len(),
        capstan_constraints.len()
    );

    // File-wide, the weaker statement `reify check` makes: nothing is Violated.
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
