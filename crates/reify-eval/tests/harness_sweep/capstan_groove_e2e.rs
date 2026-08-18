//! End-to-end acceptance gate for the printer_v01 capstan's helical rope seat
//! (task #5454 — thread-hole δ, dogfood leaf 1; PRD
//! `docs/prds/v0_6/thread-hole-features.md` §6 row 11, re-spec'd by #5580).
//!
//! Compiles the REAL design file `prj/printer_v01/dev_capstan.ri` and pins
//! three things: two geometric properties of the rope seat cut into the drum
//! (1, 2 — through the full source → parse → compile(stdlib+checked) →
//! Engine(real `OcctKernelHandle`) → tessellate pipeline) and one design-level
//! relation between the drum and the fairlead that tracks it (3 — through the
//! kernel-free value-eval + constraint-check surface, so it runs everywhere).
//!
//! **1. The seat admits the rope (`capstan_seat_admits_the_rope_radially`).**
//! The seat is a HALF-ROUND: the swept section's arc centre sits ON the land
//! surface (`land_r == pitch_r`), so the mouth it opens is the section's full
//! width `2·groove_r == rope_dia` and the rope can be laid in radially. That
//! is the only depth that works. For a circular section of radius `groove_r`
//! centred at `pitch_r` under a land at `land_r`, the mouth chord is
//! `2·sqrt(groove_r² − (land_r − pitch_r)²)`, which is *maximised* at exactly
//! `land_r == pitch_r`: a shallower land closes the mouth over the rope, and a
//! deeper one closes it back under. A radially-admitting mouth is what
//! `docs/projects/printer_v01.md` requires — its service model is "Hours
//! (re-wind capstans)" and its departure tangent migrates axially across the
//! band every revolution, so the rope has to leave the seat radially at an
//! arbitrary mid-band position. #5580 retired the pre-existing `groove_mouth`
//! captive-channel knob for exactly that reason. Re-introducing a radial
//! cut-back of any depth necessarily moves `land_r` off `pitch_r`, which
//! collapses that chord below `rope_dia` — so the mouth assertion catches it on
//! mechanical grounds, whatever the parameter that produced it is called.
//!
//! **2. The seat removes the right stock
//! (`capstan_seat_volume_delta_matches_half_pi_r2_l`).** The volume the helical
//! seat takes out of the drum blank is checked at two very different
//! resolutions:
//!   1. `PAPPUS_REL_TOL` — the half-round swept solid
//!      `ΔV ≈ 0.5·π·groove_r²·L_helix`, with
//!      `L_helix = sqrt((2π·pitch_r·n)² + groove_len²)` and
//!      `n = groove_len / lead`, within ±15 %. That is PRD §6 row 11 as
//!      literally written (post-#5580). Coarse; this is the conformance
//!      statement, and it is a band rather than an equality because of the
//!      centroid effect below.
//!   2. `HALF_ROUND_REL_TOL` — a centroid-Pappus + end-lens closed form within
//!      ±3 %. This is the regression-sensitive gate. Two corrections make it
//!      sharp, and they pull in OPPOSITE directions:
//!        * the seated half-disc's area centroid lies `4·groove_r/(3π)`
//!          radially INBOARD of the spine, so it sweeps a shorter helix than
//!          the spine does — worth ≈ −5.3 % at the file's defaults;
//!        * the swept tube's two end caps are flat discs normal to the helix
//!          tangent, so a lens of it pokes past each end of the land cylinder
//!          — into the retaining flanges, which are solid stock all the way
//!          out to `flange_r`. The tube's radially OUTER half is therefore
//!          removed there too, even though the band term (seated half only)
//!          never counts it, so that outer half-lens has to be added back at
//!          each end. Over BOTH ends that is exactly ONE full-disc lens,
//!          ≈ +1.5 %.
//!
//!      Net ≈ −3.9 %: that is what band (1) has to swallow, and the entire
//!      reason it cannot be an equality.
//!
//! Why band (2) is ±3 % and not the ±2 % this module carried before #5580: the
//! old budget was written for a correction term worth ~2 % of the swept
//! section (the emergent mouth sliver of a submerged channel). Under a
//! half-round seat the correction is 50 % of the section, so that budget no
//! longer covers it and carrying it over would be a guessed threshold. A
//! pessimistic a-priori budget for the enlarged terms is ≲0.5 %, and the
//! residual this seat actually leaves, measured, is 0.0202 % — so ±3 % is
//! ≈ 148× the real residual while still catching a modelling regression (a
//! `groove_r` off by 7 % moves ΔV by ~14 %). See [`HALF_ROUND_REL_TOL`].
//!
//! **3. The shuttle covers the band the wrap migrates over
//! (`capstan_active_band_is_covered_by_the_fairlead_stroke`,
//! `capstan_drive_constrains_the_shuttle_to_cover_the_band`).** The drum's
//! `band` (the departure tangent's axial migration over the full feed) and its
//! `groove_len` (the whole grooved extent, band plus the dead anchor wraps) are
//! two DIFFERENT axial figures, and `Fairlead.stroke` is a third — the band
//! rounded up to a whole turn. The first test pins the coverage window
//! `band ≤ stroke ≤ band + lead` over the file's evaluated cells; the second
//! pins that `CapstanDrive` states the relation in the DSL itself, so a plain
//! `reify check` catches a divergence too. Neither reads geometry, so both go
//! through [`dev_capstan_checked`] (no kernel) rather than the OCCT fixture —
//! a gate whose whole point is "this must bite outside a full OCCT run" must
//! not itself be skipped when OCCT is absent.
//!
//! **No geometry number is hard-coded here.** Every input to the expected value
//! is read back out of the file's own evaluated cells (`rope_dia`, `pitch_r`,
//! `groove_r`, `land_r`, `lead`, `groove_len`), so a parameter edit moves the
//! gate with the design instead of going stale. That is also why the file
//! exposes `blank_volume` / `body_volume` as `volume()` cells (a legitimate
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
use reify_eval::{CheckResult, TessellateResult};
use reify_ir::{Satisfaction, Value, ValueMap};
use std::f64::consts::PI;
use std::sync::OnceLock;

/// The real design file under test, reached from this crate's manifest dir.
const DEV_CAPSTAN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../prj/printer_v01/dev_capstan.ri"
);

/// The design entity whose cells and constraints this module gates.
const CAPSTAN_ENTITY: &str = "Capstan";

/// The fairlead shuttle that has to track the capstan's migrating wrap band.
const FAIRLEAD_ENTITY: &str = "Fairlead";

/// The assembly holding both — and therefore the only scope in which the
/// cross-structure band↔stroke relation can be stated.
const CAPSTAN_DRIVE_ENTITY: &str = "CapstanDrive";

/// The `sub` name `CapstanDrive` binds the capstan under.
const CAPSTAN_SUB: &str = "capstan";

/// The `sub` name `CapstanDrive` binds the fairlead shuttle under.
const SHUTTLE_SUB: &str = "shuttle";

/// Entity path of a `CapstanDrive` sub-instance — `CapstanDrive.capstan`,
/// `CapstanDrive.shuttle`.
///
/// The value map carries BOTH key forms for a contained structure's scalar
/// cells: the bare template (`Capstan.band`, `Fairlead.stroke`) and the
/// instance-scoped composition below. They agree today only because parameter
/// overrides through `sub` are dropped (task 4147 — the design file's own
/// header records it); the DSL constraint `shuttle.stroke >= capstan.band`
/// resolves against the INSTANCE, so the gate reads the instance form and
/// separately asserts the two agree.
fn sub_entity(sub: &str) -> String {
    format!("{CAPSTAN_DRIVE_ENTITY}.{sub}")
}

/// Relative tolerance on `ΔV` against the ideal half-round swept solid
/// `0.5·π·groove_r²·L`, the spine-radius arc length.
///
/// This is PRD §6 row 11's conformance band verbatim (as re-spec'd by #5580 —
/// the reference value moved from `π·r²·L` to `0.5·π·r²·L`; the band width did
/// not). It is deliberately coarse: at the file's defaults the ideal
/// over-predicts the true seat by 3.87 % (measured), and the band has to
/// swallow that.
/// That 3.9 % is a NET of two opposite terms, not a single effect — sweeping
/// the section at the spine radius rather than at the seated half's area
/// centroid over-predicts by ≈ 5.3 %, while the end lenses that emerge into
/// the flanges are under-counted by ≈ 1.5 %. Regression sensitivity
/// comes from [`HALF_ROUND_REL_TOL`] instead — do NOT tighten this one to
/// compensate, it would stop meaning "row 11".
const PAPPUS_REL_TOL: f64 = 0.15;

/// Relative tolerance on `ΔV` against the centroid-Pappus + end-lens closed
/// form for a half-round seat (see the module docs for the derivation).
///
/// With the centroid shift and the end lenses both modelled, the residual is
/// only the second-order terms the closed form ignores — land-surface
/// curvature within the section plane (~0.006 %) and tessellation/`volume()`
/// resolution. **Measured on THIS geometry** (the half-round seat this constant
/// actually gates, at the file's defaults): the kernel reports ΔV =
/// 2.589140e-5 m³ against a prediction of 2.589662e-5 m³, i.e. the closed form
/// runs high by 0.0202 %. So 3 % is ≈ 148× the residual it has to cover — and
/// ≈ 6× even the deliberately pessimistic ≲0.5 % a-priori budget for the
/// enlarged correction terms, which is what sized it before the run. (Method
/// cross-check: the same three ingredients reproduce the pre-#5580
/// submerged-channel ΔV to −0.09 %. That geometry has a different seated area,
/// centroid and end lenses, so it validates the METHOD, not this number.)
///
/// It stays sharp enough for the failure modes this module claims to catch: a
/// `groove_r` off by 7 % moves ΔV by ~14 %, and a seat that reverts to a
/// submerged full tube moves it by ~100 %.
///
/// This deliberately replaces the pre-#5580 `SEATED_SECTION_REL_TOL = 0.02`
/// rather than inheriting it: that budget was sized for a correction term
/// worth 2 % of the swept section, and under a half-round seat the correction
/// is 50 % of it.
const HALF_ROUND_REL_TOL: f64 = 0.03;

/// Radial slack, as a fraction of `groove_r`, when reading the finished drum's
/// profile back out of its tessellated mesh.
///
/// Sized from the residual actually measured on this design, not guessed. At
/// the file's defaults the mesh reads back land = 24.000225 mm against a
/// `pitch_r` of 24 mm (0.2 µm out — vertices of a tessellated cylinder lie ON
/// the true circle, so only floating-point noise separates them), and a seat
/// bottom of 20.979 mm against a `pitch_r - groove_r` of 21 mm (21 µm out —
/// OCCT approximates the swept pipe surface with a B-spline, so the innermost
/// generator is only sampled). Worst residual = 0.70 % of `groove_r`.
///
/// 10 % of `groove_r` is 0.3 mm here: 14x that residual, and still an order of
/// magnitude tighter than every failure these assertions exist to catch — a
/// seat that never breaks through reads the land radius instead of the groove
/// bottom (3 mm out, 100 % of `groove_r`), and a land put back above the rope
/// centreline reads high against `pitch_r` by however far it was raised.
///
/// That second figure is a **measured negative control**, not a derivation:
/// reinstating the pre-#5580 submerged channel (`land_r = pitch_r + groove_r −
/// 0.3mm`) and re-running this test makes the mesh read `land_max` =
/// 26.700209 mm against `pitch_r` = 24 mm — 2.700 mm out, 90 % of `groove_r`,
/// 9x this band, caught. The same run is also why both assertions reference
/// `pitch_r` and not `land_r`: against `land_r` that submerged drum reads
/// 0.2 µm out and sails through, and its `seat_min` is unchanged at
/// 20.979 mm (the swept tube bottoms at `pitch_r − groove_r` whatever the land
/// does), so neither of the other two mesh comparisons would have noticed it
/// either. There is no regression this band could swallow that a tighter one
/// would catch.
const MESH_RADIAL_TOL_FRAC: f64 = 0.10;

// ── Shared prologue ──────────────────────────────────────────────────────────

/// The tessellated design, computed once per test binary.
///
/// The full parse → compile → spawn-OCCT → sweep → boolean → tessellate pipeline
/// costs ~5 s and every test in this module only ever *reads* the result, so it is
/// memoized rather than run per test (same `OnceLock` caching idiom as
/// `crates/reify-eval/tests/auto_type_param_determinism_tests.rs`). Callers must
/// have already checked `reify_kernel_occt::OCCT_AVAILABLE`.
fn dev_capstan() -> &'static TessellateResult {
    static R: OnceLock<TessellateResult> = OnceLock::new();
    R.get_or_init(tessellate_dev_capstan)
}

/// The design file's pure value-eval + constraint-check surface, computed once
/// per test binary — NO geometry kernel.
///
/// This is the `Engine::new(checker, None) + check()` path `reify check` itself
/// takes (crates/reify-cli/src/main.rs). The design-level gates (module doc 3)
/// need only scalar cells and `constraint_results`, both of which this surface
/// carries, so they run unconditionally instead of being skipped wherever OCCT
/// is unavailable — which matters most for
/// `capstan_drive_constrains_the_shuttle_to_cover_the_band`, whose entire claim
/// is that the relation bites *outside* a full OCCT run.
fn dev_capstan_checked() -> &'static CheckResult {
    static R: OnceLock<CheckResult> = OnceLock::new();
    R.get_or_init(check_dev_capstan)
}

/// Load, parse and compile `prj/printer_v01/dev_capstan.ri`, asserting both
/// stages are Error-diagnostic-free. Shared by the two fixtures.
fn compile_dev_capstan() -> reify_compiler::CompiledModule {
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
    compiled
}

/// Evaluate and constraint-check the design file with no kernel. Use
/// [`dev_capstan_checked`] rather than calling this directly.
///
/// Deliberately NOT asserted Error-diagnostic-free, unlike
/// [`tessellate_dev_capstan`]: the file's `blank_volume` / `body_volume` cells
/// call `volume()`, a geometry-consumer builtin that is only resolvable on the
/// build()/tessellate() path, so this surface reports exactly two
/// "`volume` could not be resolved" errors by construction. They are the
/// OCCT fixture's business — the tests reading THIS result assert on the cells
/// and constraint results they actually consume, each of which panics with a
/// pointed message if it failed to evaluate.
fn check_dev_capstan() -> CheckResult {
    let compiled = compile_dev_capstan();
    let mut engine = reify_eval::Engine::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        None,
    );
    engine.check(&compiled)
}

/// Load, parse, compile and tessellate `prj/printer_v01/dev_capstan.ri` with a
/// real OCCT kernel, asserting the pipeline is Error-diagnostic-free at every
/// stage. Use [`dev_capstan`] rather than calling this directly.
fn tessellate_dev_capstan() -> TessellateResult {
    let compiled = compile_dev_capstan();

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

/// Read a `Value::Scalar` cell of `entity` out of a value map, asserting its
/// dimension, and return its SI value (m / m³ / dimensionless).
///
/// Entity-parameterised so the cross-structure band↔stroke gate can read
/// [`FAIRLEAD_ENTITY`] cells — and the instance-scoped `CapstanDrive.shuttle`
/// form — through the same dimension-checked path (and the same "is the cell
/// declared in …?" hint) the capstan cells go through, rather than carrying a
/// second copy of the `Value::Scalar` match. Map-parameterised (rather than
/// taking a `&TessellateResult`) so the kernel-free [`dev_capstan_checked`]
/// surface reads its cells through the same helper.
///
/// Both structures are `sub`s of the file's `CapstanDrive` assembly, and the map
/// carries BOTH key forms for their scalar cells — the bare template
/// (`Fairlead.stroke`, exactly as `Capstan.rope_dia` is) and the instance-scoped
/// [`sub_entity`] composition (`CapstanDrive.shuttle.stroke`). Which one a
/// caller wants is a real choice, not a formality: see the (0) claim in
/// `capstan_active_band_is_covered_by_the_fairlead_stroke`.
fn entity_cell(
    values: &ValueMap,
    entity: &str,
    cell: &str,
    expected_dim: DimensionVector,
) -> f64 {
    let id = ValueCellId::new(entity, cell);
    match values.get(&id) {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension, expected_dim,
                "{entity}.{cell}: expected dimension {expected_dim:?}, got {dimension:?}"
            );
            *si_value
        }
        other => panic!(
            "{entity}.{cell} must be a Value::Scalar with dimension {expected_dim:?}, \
             got {other:?} — is the cell declared in {DEV_CAPSTAN}?"
        ),
    }
}

/// [`entity_cell`] fixed to [`CAPSTAN_ENTITY`] — the majority of this module's
/// reads.
fn capstan_cell(values: &ValueMap, cell: &str, expected_dim: DimensionVector) -> f64 {
    entity_cell(values, CAPSTAN_ENTITY, cell, expected_dim)
}

/// Read a dimensionless (`: Real`) cell of `entity` — a pure count such as
/// `active_turns` or `dead_total` — and return it.
///
/// Separate from [`entity_cell`] because the evaluator does NOT wrap a
/// dimensionless quantity in `Value::Scalar { dimension: DIMENSIONLESS }`; a
/// `: Real` cell comes back as a bare `Value::Real`. Both spellings are accepted
/// here anyway: they denote the same mathematical object, and this module's
/// assertions are about the DESIGN, not about which representation the evaluator
/// picks for a unitless number. A `Value::Scalar` carrying any real dimension is
/// still rejected — that would mean the cell had silently acquired units.
fn entity_real(values: &ValueMap, entity: &str, cell: &str) -> f64 {
    let id = ValueCellId::new(entity, cell);
    match values.get(&id) {
        Some(Value::Real(v)) => *v,
        Some(Value::Scalar {
            si_value,
            dimension,
        }) if *dimension == DimensionVector::DIMENSIONLESS => *si_value,
        other => panic!(
            "{entity}.{cell} must be a dimensionless real (a count of turns), i.e. a \
             `Value::Real` or a DIMENSIONLESS `Value::Scalar`, got {other:?} — is the \
             cell declared in {DEV_CAPSTAN}, and is it still `: Real`?"
        ),
    }
}

/// Arc length of a helix of radius `rho` making `turns` revolutions while
/// rising `rise` axially: `sqrt((2π·rho·turns)² + rise²)` — the hypotenuse of
/// the unrolled helix.
///
/// Needed at two different radii by the half-round closed form: at the spine
/// (`pitch_r`, for the coarse band and the end-lens obliquity factor) and at
/// the seated half-disc's area centroid (which lies inboard of the spine, and
/// therefore sweeps a measurably shorter path).
fn helix_arc_len(rho: f64, turns: f64, rise: f64) -> f64 {
    ((2.0 * PI * rho * turns).powi(2) + rise.powi(2)).sqrt()
}

/// The entity path of the surface backing `Capstan.body`, resolved through the
/// value map rather than hard-coded.
///
/// The realization index is whatever slot the evaluator assigned to the `body`
/// cell, so this never pins a literal index. Both consumers ([`finished_drum`]
/// and [`capstan_surfaces_only_the_finished_drum`]) go through here, so the
/// composed-descendant path form ([`CAPSTAN_SURFACE_PREFIX`], a sub-placement
/// Phase-B detail) is stated once.
fn capstan_body_path(result: &TessellateResult) -> String {
    match result.values.get(&ValueCellId::new(CAPSTAN_ENTITY, "body")) {
        Some(Value::GeometryHandle {
            realization_ref, ..
        }) => format!("{CAPSTAN_SURFACE_PREFIX}{}]", realization_ref.index),
        other => panic!("Capstan.body must be a realized Value::GeometryHandle, got {other:?}"),
    }
}

/// The tessellated surface backing `Capstan.body` — the finished grooved drum
/// exactly as the viewport receives it.
fn finished_drum(result: &TessellateResult) -> &reify_eval::MeshSurface {
    let body_path = capstan_body_path(result);
    result
        .meshes
        .iter()
        .find(|s| s.entity_path == body_path)
        .unwrap_or_else(|| {
            panic!(
                "no tessellated surface at `{body_path}` (the finished drum); all \
                 surfaces: {:?}",
                result
                    .meshes
                    .iter()
                    .map(|s| &s.entity_path)
                    .collect::<Vec<_>>()
            )
        })
}

// ── The seat must admit the rope radially ────────────────────────────────────

/// The rope seat cut into the drum must be a HALF-ROUND that a 6 mm rope can be
/// laid into radially, not a captive channel with a mouth narrower than the
/// rope it is supposed to carry.
///
/// Three claims, all computed from the design file's own cells:
///   1. the seat breaks through the land (`land_r < pitch_r + groove_r`) —
///      otherwise it is a buried tunnel and the drum renders smooth;
///   2. the mouth chord `2·sqrt(groove_r² − (land_r − pitch_r)²)` is at least
///      `rope_dia`. The general chord form is used deliberately rather than
///      asserting `land_r == pitch_r`: it catches a re-introduced depth offset
///      in EITHER direction, and it states the mechanical requirement rather
///      than one particular way of meeting it;
///   3. the drum the kernel actually produced HAS that seat — read back off
///      the finished mesh, not off the scalars. (1) and (2) are arithmetic
///      over four scalar cells, and would stay green for a sweep placed at the
///      wrong radius, a boolean that never breaks through, or a mouth that
///      never opens; only the volume gate would notice, and only in aggregate.
///      So this also checks the drum's radial profile inside the wrap band:
///      its outermost surface is the land, sitting ON the rope centreline
///      `pitch_r`, and it is cut all the way down to the groove bottom at
///      `pitch_r − groove_r`. Both references are `pitch_r`-derived on
///      purpose — `land_r` is the cell that parametrizes the land cylinder
///      itself, so a mesh-vs-`land_r` comparison moves in lockstep with the
///      design and asserts nothing.
#[test]
fn capstan_seat_admits_the_rope_radially() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let result = dev_capstan();

    let rope_dia = capstan_cell(&result.values, "rope_dia", DimensionVector::LENGTH);
    let pitch_r = capstan_cell(&result.values, "pitch_r", DimensionVector::LENGTH);
    let groove_r = capstan_cell(&result.values, "groove_r", DimensionVector::LENGTH);
    let land_r = capstan_cell(&result.values, "land_r", DimensionVector::LENGTH);

    // ---- (1) The seat breaks through the land ----
    assert!(
        land_r < pitch_r + groove_r,
        "the rope seat must break through the land surface, else it is a buried \
         tunnel and the drum renders smooth: land_r = {:.4} mm is at or beyond the \
         seat crest pitch_r + groove_r = {:.4} mm",
        land_r * 1e3,
        (pitch_r + groove_r) * 1e3
    );

    // ---- (2) The mouth admits the rope radially ----
    // Chord of the seat circle (centre at pitch_r, radius groove_r) cut by the
    // land cylinder at land_r. `max(0.0)` only fires when the seat lies wholly
    // clear of the land, a case (1) already rejects — it just keeps the failure
    // message numeric instead of NaN.
    //
    // The comparison carries a relative epsilon rather than being exact: at the
    // file's defaults the two sides are bit-for-bit equal (`2·groove_r ==
    // rope_dia` with `groove_r = rope_dia / 2`), so a bare `>=` sits at exactly
    // zero margin and one unit-conversion refactor away from a 1-ulp false red.
    // The epsilon is 1e-9 relative — nine orders below any real seat-depth
    // regression, which moves the chord by a fraction of a millimetre at least.
    let offset = land_r - pitch_r;
    let mouth = 2.0 * (groove_r.powi(2) - offset.powi(2)).max(0.0).sqrt();
    assert!(
        mouth >= rope_dia * (1.0 - 1e-9),
        "the rope seat's mouth must admit the rope RADIALLY: mouth chord = \
         {:.4} mm but rope_dia = {:.4} mm. The chord is \
         2·sqrt(groove_r² − (land_r − pitch_r)²) and is MAXIMISED only at \
         land_r == pitch_r (a true half-round seat, mouth == 2·groove_r); here \
         land_r − pitch_r = {:.4} mm, so a shallower land closes the mouth over \
         the rope and a deeper one closes it back under. \
         (pitch_r = {:.4} mm, groove_r = {:.4} mm, land_r = {:.4} mm)",
        mouth * 1e3,
        rope_dia * 1e3,
        offset * 1e3,
        pitch_r * 1e3,
        groove_r * 1e3,
        land_r * 1e3
    );

    // ---- (3) The drum the kernel produced really has that seat ----
    let groove_len = capstan_cell(&result.values, "groove_len", DimensionVector::LENGTH);
    let bore_r = capstan_cell(&result.values, "bore_r", DimensionVector::LENGTH);
    let drum = finished_drum(result);

    // Read the radial profile only well inside the wrap band: at the band ends
    // the seat emerges into the flanges, whose faces sit out at `flange_r` and
    // would dominate `land_max`.
    let band_half = 0.9 * groove_len / 2.0;
    // Inside the band the boundary is exactly three surfaces: the shaft bore at
    // `bore_r`, the land, and the seat between the groove bottom `pitch_r −
    // groove_r` and the land. Nothing lives between the bore and the groove
    // bottom, so a cut anywhere in that gap cleanly separates "bore" from "land
    // or seat". Take the midpoint of the two surfaces it separates rather than
    // an offset off one of them: the design's own `pitch_r − groove_r > bore_r`
    // wall constraint then guarantees `bore_r < bore_clear < pitch_r − groove_r`
    // for EVERY parameter set the file admits, so this cannot go stale inside
    // the design's legal space (`bore_r + groove_r`, the obvious spelling, is
    // already above the groove bottom at a legal `bore_dia = 40mm`).
    let bore_clear = 0.5 * (bore_r + (pitch_r - groove_r));
    let mut band_vertices = 0usize;
    let mut land_max = f64::MIN;
    let mut seat_min = f64::MAX;
    for v in drum.mesh.vertices.chunks_exact(3) {
        let (x, y, z) = (v[0] as f64, v[1] as f64, v[2] as f64);
        if z.abs() > band_half {
            continue;
        }
        band_vertices += 1;
        let r = x.hypot(y);
        land_max = land_max.max(r);
        if r > bore_clear {
            seat_min = seat_min.min(r);
        }
    }
    assert!(
        band_vertices > 0 && seat_min.is_finite(),
        "the finished drum has no mesh vertices in the wrap band |z| <= {:.4} mm \
         outside the bore (bore_clear = {:.4} mm): {band_vertices} band vertices \
         out of {} — the drum is not where the design says it is",
        band_half * 1e3,
        bore_clear * 1e3,
        drum.mesh.vertices.len() / 3
    );

    let tol = MESH_RADIAL_TOL_FRAC * groove_r;
    assert!(
        (land_max - pitch_r).abs() <= tol,
        "the drum's outermost surface inside the wrap band must be the land, and \
         the land must sit ON the rope centreline pitch_r = {:.4} mm — that is \
         what makes the seat a half-round — but the mesh reaches {:.4} mm (tol \
         {:.4} mm; the design's own land_r cell is {:.4} mm). The reference here \
         is pitch_r and NOT land_r deliberately: land_r is the very cell that \
         parametrizes the blank's land cylinder, so a mesh-vs-land_r check would \
         move in lockstep with any seat-depth edit and stay green. Against pitch_r \
         this is the design claim itself, and a land raised back above the \
         centreline (the pre-#5580 submerged channel) lands here.",
        pitch_r * 1e3,
        land_max * 1e3,
        tol * 1e3,
        land_r * 1e3
    );
    let groove_bottom = pitch_r - groove_r;
    assert!(
        (seat_min - groove_bottom).abs() <= tol,
        "the seat must be cut all the way down to the groove bottom \
         pitch_r − groove_r = {:.4} mm, but the drum's innermost non-bore \
         surface inside the wrap band is at {:.4} mm (tol {:.4} mm). A seat that \
         never breaks through leaves this at the land radius {:.4} mm; a seat cut \
         past the centreline drives it below.",
        groove_bottom * 1e3,
        seat_min * 1e3,
        tol * 1e3,
        land_r * 1e3
    );
}

// ── PRD §6 row 11: the seat removes 0.5·π·r²·L of stock ──────────────────────

/// The helical seat cut into the capstan drum must remove a volume matching the
/// half-round swept-solid prediction `0.5·π·groove_r²·L_helix` within ±15 %
/// (PRD §6 row 11 as re-spec'd by #5580), and its centroid-Pappus + end-lens
/// refinement within ±3 %, with every input read from the design file's own
/// cells.
///
/// This is the consumer-visible "the seat is modelled for real" signal: a
/// smooth core (or a seat that fails to break through the land) removes no
/// stock at all, and a submerged full-tube channel removes about twice as much.
#[test]
fn capstan_seat_volume_delta_matches_half_pi_r2_l() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let result = dev_capstan();

    // ---- Read the design's own cells (SI: m, m³) ----
    let blank_volume = capstan_cell(&result.values, "blank_volume", DimensionVector::VOLUME);
    let body_volume = capstan_cell(&result.values, "body_volume", DimensionVector::VOLUME);
    let pitch_r = capstan_cell(&result.values, "pitch_r", DimensionVector::LENGTH);
    let groove_r = capstan_cell(&result.values, "groove_r", DimensionVector::LENGTH);
    let land_r = capstan_cell(&result.values, "land_r", DimensionVector::LENGTH);
    let lead = capstan_cell(&result.values, "lead", DimensionVector::LENGTH);
    let groove_len = capstan_cell(&result.values, "groove_len", DimensionVector::LENGTH);

    // ---- Pappus: unroll the helix to get its arc length ----
    let turns = groove_len / lead;
    let l_helix = helix_arc_len(pitch_r, turns, groove_len);
    let pappus = 0.5 * PI * groove_r.powi(2) * l_helix;
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

    // ---- (1) PRD §6 row 11, as literally written: the ideal half-round ± 15 % ----
    let pappus_err = (delta - pappus).abs() / pappus;
    assert!(
        pappus_err < PAPPUS_REL_TOL,
        "seat stock removal off the half-round swept-solid prediction: ΔV = \
         {delta:.6e} m³, expected 0.5·π·groove_r²·L = {pappus:.6e} m³ (rel err \
         {:.2} %, tol {:.0} %); L_helix = {l_helix:.6} m over {turns:.4} turns \
         (pitch_r = {pitch_r:.6} m, groove_r = {groove_r:.6} m, lead = {lead:.6} m, \
         groove_len = {groove_len:.6} m). A result near 2x this is a submerged \
         full-tube channel, which #5580 retired.",
        pappus_err * 100.0,
        PAPPUS_REL_TOL * 100.0
    );

    // ---- (2) The sensitive gate: centroid-Pappus + end lenses ----
    // The closed form below is half-round-ONLY: it assumes the land plane passes
    // through the seat's arc centre, so exactly half the section is seated and
    // its centroid sits 4·groove_r/(3π) inboard of the spine. Guard that premise
    // rather than let the formula be silently applied to a different design.
    //
    // The window is `groove_r * 1e-6`, which is the SAME window `dev_capstan.ri`
    // sanctions in its own two-sided `land_r` band — deliberately, so that the
    // DSL and this gate agree on what counts as "land_r == pitch_r" and there is
    // no band of seat depths that `reify check` passes but this test panics on.
    // Both are pure fp slack: `let land_r = pitch_r` is a bit-exact assignment,
    // so the measured difference is 0.0 and the width only has to be positive.
    let half_round_premise_tol = groove_r * 1e-6;
    assert!(
        (land_r - pitch_r).abs() < half_round_premise_tol,
        "the half-round closed form below assumes land_r == pitch_r (the seat's \
         arc centre lies ON the land surface), but land_r = {:.6} m and pitch_r = \
         {:.6} m differ by {:.3e} m (window {:.3e} m, mirroring dev_capstan.ri's \
         own land_r band). A seat at any other depth needs a different seated \
         area AND a different centroid — re-derive the closed form rather than \
         widening the band.",
        land_r,
        pitch_r,
        (land_r - pitch_r).abs(),
        half_round_premise_tol
    );

    // Seated section = half the swept disc; its area centroid is 4r/(3π) inboard
    // of the spine, so it sweeps a shorter helix than the spine does.
    let a_seat = PI * groove_r.powi(2) / 2.0;
    let rho_c = pitch_r - 4.0 * groove_r / (3.0 * PI);
    let v_band = a_seat * helix_arc_len(rho_c, turns, groove_len);
    // Each end cap is a flat disc normal to the helix tangent, so a lens of the
    // swept tube overhangs the land cylinder's end plane at z = ±groove_len/2 —
    // into the retaining flange, which is solid stock all the way out to
    // flange_r. `v_band` above counts only the SEATED (radially inner) half of
    // the tube, so what is missing at each end is the OUTER half's lens,
    // (1/3)·groove_r³·cot α. Over BOTH ends that is exactly ONE full-disc lens,
    // (2/3)·groove_r³·cot α — (2/3)·groove_r³ being the first moment of a
    // half-disc about its diameter, and cot α the tangent's obliquity to the
    // axis. NOTE for anyone auditing the factor of 2 below: it is the two ends'
    // HALF-lenses, so the total is one full lens, NOT one lens per end. Doubling
    // it would bias the prediction by +1.5 %, which the ±3 % band would not
    // catch. cot α is written out rather than approximated by
    // csc α = l_helix/groove_len (worth +0.002 % of ΔV) — it costs nothing.
    let cot_alpha = (2.0 * PI * pitch_r * turns) / groove_len;
    let v_ends = 2.0 * (groove_r.powi(3) / 3.0) * cot_alpha;
    let v_pred = v_band + v_ends;

    let half_round_err = (delta - v_pred).abs() / v_pred;
    assert!(
        half_round_err < HALF_ROUND_REL_TOL,
        "seat stock removal off the centroid-Pappus + end-lens prediction: ΔV = \
         {delta:.6e} m³, expected {v_pred:.6e} m³ (rel err {:.2} %, tol {:.0} %). \
         Breakdown: A_seat = {a_seat:.6e} m² swept at the seated centroid radius \
         rho_c = {rho_c:.6} m (vs. spine pitch_r = {pitch_r:.6} m) gives v_band = \
         {v_band:.6e} m³ ({:.2} % of the prediction); the two end lenses add \
         v_ends = {v_ends:.6e} m³ ({:.2} %). L_helix = {l_helix:.6} m over \
         {turns:.4} turns, groove_r = {groove_r:.6} m, groove_len = \
         {groove_len:.6} m.",
        half_round_err * 100.0,
        HALF_ROUND_REL_TOL * 100.0,
        100.0 * v_band / v_pred,
        100.0 * v_ends / v_pred
    );
}

// ── The shuttle stroke covers the band the wrap migrates over ────────────────

/// Relative slack on the two exact band identities below.
///
/// `band == lead·active_turns` and `groove_len − band == lead·dead_total` are
/// *algebraic* identities given the file's own definitions (`groove_len = lead ·
/// (active_turns + dead_total)`), not empirical fits — the only thing separating
/// the two sides is floating-point association order in the evaluator. So this
/// is pure fp slack, and it is deliberately ~4 orders below the smallest
/// physically meaningful drift (a 1 µm move on a 60 mm band is 1.7e-5 relative).
const BAND_IDENTITY_REL_TOL: f64 = 1e-12;

/// The drum carries TWO distinct axial figures, and the fairlead shuttle has to
/// cover the smaller one.
///
///   * the **active band** (`band = lead · active_turns`) is how far the
///     departure tangent walks axially over the full per-axis `feed` — 60.35 mm
///     at the file's defaults. This is what the passive shuttle tracks;
///   * the **total grooved length** (`groove_len = lead · (active_turns +
///     dead_total)`) is the whole grooved extent, the band PLUS the dead
///     (anchor) wraps at each end — 88.35 mm. The shuttle never travels this.
///
/// Conflating them is exactly the drift this test exists to stop: those two
/// numbers and `Fairlead.stroke` (63 mm) read like three estimates of one
/// quantity and are not, which is how `docs/projects/printer_v01.md` came to
/// carry a stale "~80 mm over full travel" for the migration. The relation used
/// to live only in a comment; here it is stated over the file's evaluated cells.
///
/// Four claims, all read back from those cells:
///   1. `band` really is the ACTIVE migration — `lead · active_turns`. Guards
///      the definition against a later edit that quietly redefines it as the
///      total grooved extent;
///   2. `groove_len − band == lead · dead_total` — the decomposition the project
///      doc asserts in prose ("sized for the full per-axis feed … plus the base
///      wraps") stated over cells: 88.346 mm − 60.346 mm = 28.000 mm = 7 mm × 4;
///   3. `band < groove_len` strictly, i.e. the two figures have not collapsed
///      into one (they cannot while `dead_total > 0`);
///   4. the coverage relation itself: `band <= stroke <= band + lead`.
///
/// The upper bound in (4) is derived, not tuned to admit the observed 63 mm: the
/// stroke is the band rounded UP to a whole turn, `ceil(active_turns) · lead`,
/// and `ceil(x) · lead < x · lead + lead` holds identically for every `x`. A
/// bare lower bound would let the stroke grow without limit and still pass, so
/// it would not pin the design intent at all. Keeping both bounds derived is
/// what lets a future `lead` or `d_ratio` edit move this gate with the design
/// instead of going stale — the same principle as the module's "no geometry
/// number is hard-coded here".
#[test]
fn capstan_active_band_is_covered_by_the_fairlead_stroke() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let result = dev_capstan();

    let band = capstan_cell(&result.values, "band", DimensionVector::LENGTH);
    let lead = capstan_cell(&result.values, "lead", DimensionVector::LENGTH);
    let active_turns = entity_real(result, CAPSTAN_ENTITY, "active_turns");
    let dead_total = entity_real(result, CAPSTAN_ENTITY, "dead_total");
    let groove_len = capstan_cell(&result.values, "groove_len", DimensionVector::LENGTH);
    let stroke = entity_cell(result, FAIRLEAD_ENTITY, "stroke", DimensionVector::LENGTH);

    // ---- (1) `band` is the ACTIVE migration, not the total grooved extent ----
    let band_expected = lead * active_turns;
    assert!(
        (band - band_expected).abs() <= BAND_IDENTITY_REL_TOL * band_expected.abs(),
        "`{CAPSTAN_ENTITY}.band` must be the ACTIVE wrap-band migration, one `lead` \
         per active turn: lead · active_turns = {:.6} mm × {active_turns:.6} = \
         {:.6} mm, but the cell reads {:.6} mm. If this failed at ~{:.4} mm the \
         cell has been redefined as the TOTAL grooved extent (`groove_len`, which \
         also counts the {dead_total} dead anchor wraps) — those are different \
         lengths and only the active one is what the shuttle tracks.",
        lead * 1e3,
        band_expected * 1e3,
        band * 1e3,
        groove_len * 1e3
    );

    // ---- (2) The grooved length decomposes into band + dead wraps ----
    // An algebraic identity given `groove_len = lead · (active_turns +
    // dead_total)` and `band = lead · active_turns`, so only fp slack is needed;
    // it is asserted anyway because it is the statement the project doc makes in
    // prose, and it is what makes (1) and (3) mean what they say.
    let dead_extent = lead * dead_total;
    assert!(
        (groove_len - band - dead_extent).abs() <= BAND_IDENTITY_REL_TOL * dead_extent.abs(),
        "the drum's grooved length must decompose into the active band plus the \
         dead (anchor) wraps: groove_len − band = {:.6} mm − {:.6} mm = {:.6} mm, \
         but lead · dead_total = {:.6} mm × {dead_total} = {:.6} mm. This is the \
         \"sized for the full per-axis feed … plus the base wraps\" claim in \
         docs/projects/printer_v01.md, stated over the cells.",
        groove_len * 1e3,
        band * 1e3,
        (groove_len - band) * 1e3,
        lead * 1e3,
        dead_extent * 1e3
    );

    // ---- (3) The two axial figures have not collapsed into one ----
    assert!(
        band < groove_len,
        "the active band ({:.6} mm) must be strictly shorter than the total \
         grooved length ({:.6} mm) — they are different measurements of the drum \
         and the dead_total = {dead_total} anchor wraps are the difference. Equal \
         values mean the anchor wraps have been lost, which puts the rope's dead \
         turns under working tension.",
        band * 1e3,
        groove_len * 1e3
    );

    // ---- (4) The shuttle covers the band ----
    // Lower bound: a stroke short of the band leaves the fleet angle to open up
    // at one end of travel — the fairlead stops tracking and starts side-loading.
    // Upper bound: the stroke is the band rounded UP to a whole turn,
    // ceil(active_turns) · lead, and ceil(x) · lead < x · lead + lead for every
    // x. So the two bounds together say "one whole turn of margin, no more" —
    // neither is the literal 63 mm, which is deliberately absent from this
    // assertion so a lead/d_ratio edit moves the gate with the design.
    assert!(
        stroke >= band && stroke <= band + lead,
        "the fairlead shuttle's stroke must cover the capstan's band migration \
         and overshoot it by less than one whole turn: {FAIRLEAD_ENTITY}.stroke = \
         {:.6} mm against a band of {:.6} mm (lower bound) and band + lead = \
         {:.6} mm (upper bound). The stroke is the band rounded UP to a whole \
         turn, ceil(active_turns) · lead = ceil({active_turns:.6}) × {:.6} mm = \
         {:.6} mm; a stroke below the band means the shuttle runs out of travel \
         before the wrap band does (fleet angle opens, the fairlead side-loads \
         instead of guiding), and one above the upper bound is no longer that \
         rounding rule — it is an unexplained number.",
        stroke * 1e3,
        band * 1e3,
        (band + lead) * 1e3,
        lead * 1e3,
        active_turns.ceil() * lead * 1e3
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
/// constraints guarding the seat cannot silently regress (equivalent to `reify
/// check` reporting "All constraints satisfied"). Those are the two-sided
/// `land_r` band that pins the seat depth to `pitch_r` (and so subsumes
/// break-through), the groove-bottom-to-bore wall `pitch_r - groove_r >
/// bore_r`, the mouth-clearance lead `lead > groove_r * 2.0`, and `flange_r >
/// land_r`. That in-file band is what makes `reify check` — not just this
/// Rust gate — report a seat depth edited off `pitch_r`.
#[test]
fn capstan_surfaces_only_the_finished_drum() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let result = dev_capstan();

    // The realization slot backing `Capstan.body` — resolved from the value map,
    // so the test never hard-codes a realization index.
    let body_path = capstan_body_path(result);

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
    // regression in the rope-seat work this module gates.
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

// ── The design enforces the coverage itself, not only this gate ──────────────

/// The band↔stroke coverage must be a constraint the *design file* carries, so
/// that plain `reify check` reports a divergence — not only a full OCCT run of
/// this Rust module.
///
/// `capstan_active_band_is_covered_by_the_fairlead_stroke` above asserts the
/// same relation, but it only bites when this one test happens to run. An edit
/// to `Fairlead.stroke`, `Capstan.lead` or `d_ratio` that breaks the coverage
/// should be loud for anyone opening the file, which means it has to be stated
/// in the DSL.
///
/// Two claims:
///   1. `CapstanDrive` declares constraints at all. The relation is
///      cross-structure — it reads a cell of `capstan` against a cell of
///      `shuttle` — so the assembly that owns both `sub`s is the only scope it
///      CAN be stated in. Deriving `Fairlead.stroke` from the capstan instead
///      would need a parameter override through `sub shuttle = Fairlead(…)`,
///      which is precisely the override drop (task 4147) the design file's own
///      header records as not working: only `at` poses come through. So the
///      stroke stays a hand-set param and the assembly asserts it stays honest;
///   2. every one of those results is `Satisfied`.
///
/// (2) is asserted positively rather than as "nothing is `Violated`" — the same
/// reason `capstan_surfaces_only_the_finished_drum` gives for `Capstan`, and it
/// matters more here. A cross-sub field reference that fails to resolve
/// evaluates to `Indeterminate`, not `Violated`, so a `!= Violated` filter would
/// stay green on exactly the failure mode this constraint is most exposed to,
/// and the file-wide `violated.is_empty()` check in that test is likewise
/// vacuous on it. That combination — an empty result set and an indeterminate
/// one both reading as green — is the gap this test closes.
#[test]
fn capstan_drive_constrains_the_shuttle_to_cover_the_band() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let result = dev_capstan();

    // ---- (1) The assembly states the relation ----
    let drive_constraints: Vec<_> = result
        .constraint_results
        .iter()
        .filter(|c| c.id.entity == CAPSTAN_DRIVE_ENTITY)
        .collect();
    assert!(
        !drive_constraints.is_empty(),
        "`{CAPSTAN_DRIVE_ENTITY}` must carry the shuttle-covers-the-band constraint \
         (`shuttle.stroke >= capstan.band`), but it declares none. That relation \
         reads a `{CAPSTAN_ENTITY}` cell against a `{FAIRLEAD_ENTITY}` cell, so the \
         assembly owning both `sub`s is the ONLY scope it can live in: deriving \
         `{FAIRLEAD_ENTITY}.stroke` from the capstan would need a parameter \
         override through `sub shuttle = {FAIRLEAD_ENTITY}(…)`, and that is the \
         override drop (task 4147) {DEV_CAPSTAN}'s own header records as not \
         working — only `at` poses come through. Entities checked: {:?}",
        result
            .constraint_results
            .iter()
            .map(|c| &c.id.entity)
            .collect::<Vec<_>>()
    );

    // ---- (2) …and it holds at the file's defaults ----
    let unsatisfied: Vec<_> = drive_constraints
        .iter()
        .filter(|c| c.satisfaction != Satisfaction::Satisfied)
        .collect();
    assert!(
        unsatisfied.is_empty(),
        "every `{CAPSTAN_DRIVE_ENTITY}` constraint must be Satisfied at the file's \
         defaults ({} of {} were not). `Violated` means the shuttle's stroke no \
         longer covers the capstan's band migration — the fairlead runs out of \
         travel before the wrap band does. `Indeterminate` means something quite \
         different and is why this is asserted positively: it is what a cross-sub \
         field reference produces when it fails to EVALUATE, so the constraint is \
         present but checking nothing. Results: {unsatisfied:#?}",
        unsatisfied.len(),
        drive_constraints.len()
    );
}
