//! End-to-end acceptance gate for the printer_v01 capstan's helical rope seat
//! (task #5454 — thread-hole δ, dogfood leaf 1; PRD
//! `docs/prds/v0_6/thread-hole-features.md` §6 row 11, re-spec'd by #5580).
//!
//! Compiles the REAL design file `prj/printer_v01/dev_capstan.ri` through the
//! full source → parse → compile(stdlib+checked) → Engine(real
//! `OcctKernelHandle`) → tessellate pipeline and pins three properties of the
//! rope seat cut into the drum.
//!
//! Properties 1 and 2 read meshes and `volume()` cells, so they need a live
//! OCCT kernel and skip without one. Property 3 is scalar-only and runs
//! unguarded off a second, kernel-free evaluation of the same compiled module
//! ([`dev_capstan_scalars`]) — it is the only slip-fit gate here, and a
//! stub-degraded OCCT is silent in this repo (CLAUDE.md "Native deps"; #6343),
//! so it must not skip with the others.
//!
//! **1. The seat admits the rope (`capstan_seat_admits_the_rope_radially`).**
//! The seat is a HALF-ROUND: the swept section's arc centre sits ON the land
//! surface (`land_r == seat_c`, the arc-centre radius [`seat_arc_centre`]
//! derives), so the mouth it opens is the section's full width `2·groove_r`
//! and the rope can be laid in radially. For a circular section of radius
//! `groove_r` centred at `seat_c` under a land at `land_r`, the mouth chord is
//! `2·sqrt(groove_r² − (land_r − seat_c)²)`, which is *maximised* at exactly
//! `land_r == seat_c`: a shallower land closes the mouth over the rope, and a
//! deeper one closes it back under. Under the DIN 15061 arc that maximum is
//! `2·groove_r = 1.06·rope_dia`, so the mouth clears the rope by 6 %. A
//! radially-admitting mouth is what
//! `docs/projects/printer_v01.md` requires — its service model is "Hours
//! (re-wind capstans)" and its departure tangent migrates axially across the
//! band every revolution, so the rope has to leave the seat radially at an
//! arbitrary mid-band position. #5580 retired the pre-existing `groove_mouth`
//! captive-channel knob for exactly that reason.
//!
//! Note what MECHANICALLY retires here, and what the assertion nonetheless
//! still pins. With an equal-radii seat the arc centre was the ONLY land radius
//! admitting the rope at all. An oversize arc opens a whole BAND of admitting
//! land radii, `|land_r − seat_c| ≤ sqrt(groove_r² − (rope_dia/2)²)` — a
//! half-width of 1.055 mm at the defaults, so ≈ 2.11 mm wide — and #5580's "a
//! half-round is the only depth that admits the rope" reasoning retires with
//! it: `land_r == seat_c` is now a design CHOICE (for the maximised mouth and
//! for the closed form's half-seated premise), not a necessity, and the
//! necessity argument must not be reinstated.
//!
//! The assertion does NOT admit that whole band, though, and is not meant to.
//! It demands the chord's MAXIMUM: `MIN_MOUTH_CLEARANCE_FRAC` is derived from
//! the same DIN ratio the design multiplies into `groove_r`, so the required
//! `rope_dia·1.06` is bit-identically `2·groove_r`, attained only at
//! `land_r == seat_c`. With the 1e-9 relative slack that leaves a window of
//! `|land_r − seat_c| ≤ groove_r·sqrt(2e-9)` = 4.5e-5·`groove_r` = 0.14 µm —
//! i.e. the mouth assertion pins the seat depth, via the DIN clearance margin
//! rather than via a bare `land_r == seat_c`. That is safe rather than brittle
//! because `dev_capstan.ri` sets `land_r = seat_c` bit-exactly and its own
//! two-sided band sanctions only a `groove_r·1e-6` window, so no seat depth the
//! file admits can trip it. The volume gate's premise guard below is the same
//! `groove_r·1e-6` window and is therefore the TIGHTER, independent restatement
//! of the same pin (3.18 nm vs 0.14 µm here). A deliberate in-band cut-back —
//! which the oversize arc now makes mechanically legal — would have to move all
//! three together: this assertion, that premise guard, and the in-file band.
//!
//! **2. The seat removes the right stock
//! (`capstan_seat_volume_delta_matches_half_pi_r2_l`).** The volume the helical
//! seat takes out of the drum blank is checked at two very different
//! resolutions:
//!   1. `PAPPUS_REL_TOL` — the half-round swept solid
//!      `ΔV ≈ 0.5·π·groove_r²·L_helix`, with
//!      `L_helix = sqrt((2π·seat_c·n)² + groove_len²)` and
//!      `n = groove_len / lead`, within ±15 %. That is PRD §6 row 11 as
//!      literally written (post-#5580). Coarse; this is the conformance
//!      statement, and it is a band rather than an equality because of the
//!      centroid effect below.
//!   2. `HALF_ROUND_REL_TOL` — a centroid-Pappus + end-lens closed form within
//!      ±3 %. This is the regression-sensitive gate. Two corrections make it
//!      sharp, and they pull in OPPOSITE directions:
//!        * the seated half-disc's area centroid lies `4·groove_r/(3π)`
//!          radially INBOARD of the spine, so it sweeps a shorter helix than
//!          the spine does — worth ≈ −5.6 % at the file's defaults;
//!        * the swept tube's two end caps are flat discs normal to the helix
//!          tangent, so a lens of it pokes past each end of the land cylinder
//!          — into the retaining flanges, which are solid stock all the way
//!          out to `flange_r`. The tube's radially OUTER half is therefore
//!          removed there too, even though the band term (seated half only)
//!          never counts it, so that outer half-lens has to be added back at
//!          each end. Over BOTH ends that is exactly ONE full-disc lens,
//!          ≈ +1.5 %.
//!
//!      Net ≈ −4.0 %: that is what band (1) has to swallow, and the entire
//!      reason it cannot be an equality.
//!
//! **3. The seat arc conforms to DIN 15061
//! (`capstan_seat_arc_is_din_15061_oversize`).** `groove_r = 0.53·rope_dia` —
//! an OVERSIZE arc, not a zero-clearance slip fit, which is what buys the
//! per-side anti-pinch clearance at the rope's widest section — and with the
//! design's own `seat_c` cell equal to this module's recomputation of it,
//! which is what keeps the SEATED rope's centreline on the D/d circle
//! `pitch_r` and the seat bottom on that rope's own underside. This is the
//! module's only non-lockstep pin on the arc ratio itself: see
//! [`DIN_15061_SEAT_RATIO`]. Gates (1) and (2) are both parametrized by the
//! file's own `groove_r` and would stay green through a revert to a slip fit,
//! so this one is not redundant with them.
//!
//! Why band (2) is ±3 % and not the ±2 % this module carried before #5580: the
//! old budget was written for a correction term worth ~2 % of the swept
//! section (the emergent mouth sliver of a submerged channel). Under a
//! half-round seat the correction is 50 % of the section, so that budget no
//! longer covers it and carrying it over would be a guessed threshold. A
//! pessimistic a-priori budget for the enlarged terms is ≲0.5 %, and the
//! residual this seat actually leaves, measured, is 0.0321 % — so ±3 % is
//! ≈ 93× the real residual while still catching a modelling regression (a
//! `groove_r` off by 7 % moves ΔV by ~14 %). See [`HALF_ROUND_REL_TOL`].
//!
//! **No geometry number is hard-coded here.** Every input to the expected value
//! is read back out of the file's own evaluated cells (`rope_dia`, `pitch_r`,
//! `groove_r`, `land_r`, `lead`, `groove_len`), so a parameter edit moves the
//! gate with the design instead of going stale. The one derived radius the
//! closed form and the mesh checks hang off — the seat's arc centre `seat_c` —
//! is RECOMPUTED here from three of those cells by [`seat_arc_centre`] rather
//! than read back from a design cell of its own: a mesh-vs-design-cell
//! comparison moves in lockstep with the design and asserts nothing (the same
//! reason [`MESH_RADIAL_TOL_FRAC`]'s measured negative control gives for not
//! referencing `land_r`). The design's `seat_c` cell is read in exactly ONE
//! place — gate (3)'s claim (2) — and only to be checked AGAINST that
//! recomputation, which pins the two derivations against each other rather than
//! moving with either. That is also why the file
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
use reify_eval::TessellateResult;
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

/// DIN 15061 rope-drum seat arc radius as a fraction of rope diameter:
/// `r_groove = 0.53·d`. External standard, so it is the REFERENCE and not a
/// design cell.
///
/// **This is the one geometry number deliberately hard-coded in this module**,
/// against the header's "no geometry number is hard-coded here" rule. It is the
/// same narrow exception PRD §6 row 9 takes for the published ISO 286 IT value,
/// and for the same reason: conformance to an EXTERNAL standard makes the
/// standard's number the reference, and reading the design's own
/// `seat_arc_ratio` cell back out would assert nothing but that the file equals
/// itself.
///
/// It is load-bearing rather than stylistic. Both of this module's other gates
/// are parametrized by the file's own `groove_r`, so a silent revert to a
/// slip-fit `groove_r = rope_dia/2` leaves BOTH green: the volume gate's closed
/// form would simply predict the smaller ΔV, and the mesh gate's land reference
/// [`seat_arc_centre`] would simply move back onto `pitch_r`.
/// [`capstan_seat_arc_is_din_15061_oversize`] is the only assertion here that
/// catches it.
const DIN_15061_SEAT_RATIO: f64 = 0.53;

/// Fractional clearance the DIN ratio buys at the seat mouth, straight out of
/// it: the mouth is the section's full width `2·r_groove = 2·0.53·d = 1.06·d`,
/// so the rope clears it by 6 % of `rope_dia`.
const MIN_MOUTH_CLEARANCE_FRAC: f64 = 2.0 * DIN_15061_SEAT_RATIO - 1.0;

/// Relative tolerance on `ΔV` against the ideal half-round swept solid
/// `0.5·π·groove_r²·L`, the spine-radius arc length.
///
/// This is PRD §6 row 11's conformance band verbatim (as re-spec'd by #5580 —
/// the reference value moved from `π·r²·L` to `0.5·π·r²·L`; the band width did
/// not, and #5683 did not move it either — only the meaning of `r` and the
/// radius `L` is measured at). It is deliberately coarse: at the file's
/// defaults the ideal over-predicts the true seat by 4.07 % (measured on the
/// DIN 15061 oversize seat; it was 3.87 % on the equal-radii seat #5683
/// replaced), and the band has to swallow that.
/// That 4.1 % is a NET of two opposite terms, not a single effect — sweeping
/// the section at the spine radius rather than at the seated half's area
/// centroid over-predicts by ≈ 5.57 %, while the end lenses that emerge into
/// the flanges are under-counted by ≈ 1.53 %. Both shares are quoted against
/// the coarse ideal, so they sum to the closed form's ≈ −4.04 % offset from it
/// (the remaining 0.03 % is the measured residual — see
/// [`HALF_ROUND_REL_TOL`]). The centroid term grew with the oversize arc
/// because `4·groove_r/(3π)` grows faster than `seat_c` does. Regression
/// sensitivity comes from [`HALF_ROUND_REL_TOL`] instead — do NOT tighten this
/// one to compensate, it would stop meaning "row 11".
const PAPPUS_REL_TOL: f64 = 0.15;

/// Relative tolerance on `ΔV` against the centroid-Pappus + end-lens closed
/// form for a half-round seat (see the module docs for the derivation).
///
/// With the centroid shift and the end lenses both modelled, the residual is
/// only the second-order terms the closed form ignores — land-surface
/// curvature within the section plane and tessellation/`volume()` resolution.
/// **Measured on THIS geometry** (the DIN 15061 oversize seat this constant
/// actually gates, at the file's defaults): the kernel reports ΔV =
/// 2.924801e-5 m³ against a prediction of 2.925740e-5 m³, i.e. the closed form
/// runs high by 0.0321 %. So 3 % is ≈ 93× the residual it has to cover — and
/// still ≈ 6× the deliberately pessimistic ≲0.5 % a-priori budget for the
/// enlarged correction terms, which the measured residual in turn sits ≈ 16×
/// inside.
///
/// The band was NOT re-sized for #5683's oversize arc; the a-priori basis for
/// keeping it was that the dominant ignored term — land-surface curvature
/// within the section plane, ~0.006 % on the equal-radii seat — scales as
/// `(R_s/C)²`, i.e. `(3.18/24.18)² / (3/24)² = 1.107×` → ~0.0066 %, with
/// tessellation/`volume()` resolution unchanged, so the residual was expected
/// to stay ≈ 0.02 %. It came in at 0.0321 %, i.e. 1.59× the pre-#5683
/// residual rather than the 1.107× that curvature scaling alone accounts for —
/// so the estimate was directionally right and conservative in magnitude, but
/// it is not the whole story. At ≈ 93× inside the band that gap is recorded
/// rather than acted on; it would matter if a future arc ratio pushed the
/// residual toward the ≲0.5 % budget. (Method cross-check: the same three
/// ingredients reproduce the pre-#5580 submerged-channel ΔV to −0.09 %. That
/// geometry has a different seated area, centroid and end lenses, so it
/// validates the METHOD, not this number.)
///
/// It stays sharp enough for the failure modes this module claims to catch: a
/// `groove_r` off by 7 % moves ΔV by ~14 %, and a seat that reverts to a
/// submerged full tube moves it by ~100 %.
///
/// **What this band does NOT catch: a revert to a slip-fit
/// `groove_r = rope_dia/2`.** The closed form is parametrized by the file's own
/// `groove_r`, so it would simply predict the correspondingly smaller ΔV and
/// stay green. That is [`capstan_seat_arc_is_din_15061_oversize`]'s job, via
/// the externally-pinned [`DIN_15061_SEAT_RATIO`].
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
/// the file's defaults the mesh reads back land = 24.180070 mm against a
/// [`seat_arc_centre`] of 24.180 mm (0.07 µm out — vertices of a tessellated
/// cylinder lie ON the true circle, so only floating-point noise separates
/// them), and a seat bottom of 20.989 mm against a `pitch_r - rope_dia/2` of
/// 21 mm (11 µm out — OCCT approximates the swept pipe surface with a
/// B-spline, so the innermost generator is only sampled). Worst residual =
/// 0.34 % of `groove_r`. (Both figures re-measured on #5683's oversize seat;
/// they were 0.2 µm / 21 µm, worst 0.70 %, on the equal-radii seat.)
///
/// 10 % of `groove_r` is 0.318 mm here: 29x that residual, and still an order
/// of magnitude tighter than every failure these assertions exist to catch — a
/// seat that never breaks through reads the land radius instead of the groove
/// bottom (3.18 mm out, 100 % of `groove_r`), and a land put back above the
/// seat's arc centre reads high against `seat_c` by however far it was raised.
///
/// That second figure rests on a **measured negative control** — but one taken
/// on the PRE-#5683 equal-radii geometry, not re-run here, so it is carried
/// with its provenance stated rather than as a measurement of this seat.
/// #5580 reinstated the pre-#5580 submerged channel (`land_r = pitch_r +
/// groove_r − 0.3mm`) and re-ran this test: the mesh read `land_max` =
/// 26.700209 mm against an arc centre of 24 mm — 2.700 mm out, 90 % of
/// `groove_r`, 9x this band, caught. Nothing in that result depends on the arc
/// ratio — what carries is the FRACTION of `groove_r` the control is out by,
/// not the absolute offset, since both this band and the offset scale with
/// `groove_r` (the equivalent submerged land here would sit at `seat_c +
/// groove_r − 0.3mm` = 27.06 mm, i.e. 2.880 mm = `groove_r − 0.3mm` out, again
/// 90 % of `groove_r` and 9x this band) — so the conclusion carries even
/// though the number was not re-taken. That run is also why both assertions
/// reference recomputed radii and not `land_r`: against `land_r` the submerged
/// drum reads 0.2 µm out and sails through, and its `seat_min` is unchanged
/// (the swept tube bottoms at `seat_c − groove_r` whatever the land does), so
/// neither of the other two mesh comparisons would have noticed it either.
/// There is no regression this band could swallow that a tighter one would
/// catch.
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

/// The design's evaluated cells WITHOUT any geometry kernel, computed once per
/// test binary.
///
/// Everything hanging off [`dev_capstan`] needs a live OCCT kernel and therefore
/// skips wholesale where OCCT is absent or degraded to stubs — a documented,
/// silent failure mode in this repo (CLAUDE.md "Native deps"; #6343). That is
/// the right trade for the gates that read meshes or `volume()` cells, but
/// [`capstan_seat_arc_is_din_15061_oversize`] is pure scalar arithmetic over
/// `rope_dia` / `pitch_r` / `groove_r` / `seat_c`, and by
/// [`DIN_15061_SEAT_RATIO`]'s argument it is the ONLY assertion in this module
/// that catches a revert to a slip-fit `groove_r = rope_dia/2`. It must not be
/// the thing that goes quiet on a kernel-less machine, so it reads its cells
/// from here instead.
///
/// `Engine::new(checker, None)` evaluates with no planner at all (the pattern in
/// `crates/reify-eval/tests/auto_binding_sites_remaining_resolution.rs`). The
/// geometry and `volume()` cells cannot resolve that way, so eval diagnostics
/// are deliberately NOT asserted clean here and this map is good for SCALAR
/// cells only — the OCCT path is what gates the rest. Parse and compile ARE
/// asserted clean, in the shared [`compile_dev_capstan`].
fn dev_capstan_scalars() -> &'static ValueMap {
    static V: OnceLock<ValueMap> = OnceLock::new();
    V.get_or_init(|| {
        let compiled = compile_dev_capstan();
        let mut engine = reify_eval::Engine::new(
            Box::new(reify_constraints::SimpleConstraintChecker),
            None,
        );
        engine.eval(&compiled).values
    })
}

/// Read, parse and compile `prj/printer_v01/dev_capstan.ri`, asserting both
/// stages are Error-diagnostic-free.
///
/// Shared by the two memoized entry points ([`dev_capstan`]'s tessellation and
/// [`dev_capstan_scalars`]'s kernel-free evaluation) so they cannot drift onto
/// different sources or different compile entries.
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

/// Tessellate `prj/printer_v01/dev_capstan.ri` with a real OCCT kernel,
/// asserting the pipeline is Error-diagnostic-free at every stage. Use
/// [`dev_capstan`] rather than calling this directly.
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

/// Read a `Value::Scalar` cell of [`CAPSTAN_ENTITY`] out of an evaluated value
/// map, asserting its dimension, and return its SI value (m / m³).
///
/// Takes the `ValueMap` rather than the `TessellateResult` so the kernel-free
/// [`dev_capstan_scalars`] map can be read through the same accessor.
fn capstan_cell(values: &ValueMap, cell: &str, expected_dim: DimensionVector) -> f64 {
    let id = ValueCellId::new(CAPSTAN_ENTITY, cell);
    match values.get(&id) {
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

/// Arc length of a helix of radius `rho` making `turns` revolutions while
/// rising `rise` axially: `sqrt((2π·rho·turns)² + rise²)` — the hypotenuse of
/// the unrolled helix.
///
/// Needed at two different radii by the half-round closed form: at the spine
/// (`seat_c`, the seat's arc centre — see [`seat_arc_centre`] — for the coarse
/// band, and the radius the end-lens obliquity factor is likewise taken at) and
/// at the seated half-disc's area centroid (which lies inboard of the spine,
/// and therefore sweeps a measurably shorter path). `pitch_r` is the ROPE's
/// centreline and is not the spine of anything the closed form sweeps.
fn helix_arc_len(rho: f64, turns: f64, rise: f64) -> f64 {
    ((2.0 * PI * rho * turns).powi(2) + rise.powi(2)).sqrt()
}

/// Radius at which the rope seat's arc centre must sit for the SEATED rope's
/// centreline to land on the D/d circle `pitch_r`.
///
/// Under tension the rope is pressed radially inward and bottoms out in its
/// seat, so its centre lies `groove_r - rope_dia/2` INBOARD of the arc centre;
/// the arc centre is therefore that much outboard of `pitch_r`. Derived from
/// the design intent and recomputed here from three independent cells —
/// deliberately NOT read back from a `seat_c` cell, which would move in
/// lockstep with the design and assert nothing.
///
/// This is the radius the seat's geometry is actually anchored on: the swept
/// section is centred here, the land plane passes through here, and the closed
/// form's spine runs here. With an equal-radii seat (`groove_r == rope_dia/2`)
/// it collapses onto `pitch_r` exactly, which is why #5580 could write
/// `pitch_r` throughout without the distinction mattering.
fn seat_arc_centre(pitch_r: f64, groove_r: f64, rope_dia: f64) -> f64 {
    pitch_r + groove_r - rope_dia / 2.0
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
///   1. the seat breaks through the land (`land_r < seat_c + groove_r`) —
///      otherwise it is a buried tunnel and the drum renders smooth;
///   2. the mouth chord `2·sqrt(groove_r² − (land_r − seat_c)²)` clears the
///      rope by DIN 15061's margin — at least
///      `rope_dia·(1 + MIN_MOUTH_CLEARANCE_FRAC)`, i.e. `1.06·rope_dia`. The
///      general chord form is used deliberately rather than asserting
///      `land_r == seat_c`: it catches a re-introduced depth offset in EITHER
///      direction, and it states the mechanical requirement rather than one
///      particular way of meeting it. Because `1.06·rope_dia` IS that chord's
///      maximum `2·groove_r`, it still pins the depth — to 0.14 µm at the
///      defaults — it just routes the pin through the DIN clearance margin.
///      See the module header;
///   3. the drum the kernel actually produced HAS that seat — read back off
///      the finished mesh, not off the scalars. (1) and (2) are arithmetic
///      over four scalar cells, and would stay green for a sweep placed at the
///      wrong radius, a boolean that never breaks through, or a mouth that
///      never opens; only the volume gate would notice, and only in aggregate.
///      So this also checks the drum's radial profile inside the wrap band:
///      its outermost surface is the land, sitting ON the seat's arc centre
///      `seat_c`, and it is cut all the way down to the groove bottom — which
///      is the SEATED ROPE'S OWN UNDERSIDE, `pitch_r − rope_dia/2`, a
///      statement that holds whatever the seat's arc radius is and does not
///      mention `groove_r` at all. Neither reference is a design cell on
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
    let seat_c = seat_arc_centre(pitch_r, groove_r, rope_dia);

    // ---- (1) The seat breaks through the land ----
    assert!(
        land_r < seat_c + groove_r,
        "the rope seat must break through the land surface, else it is a buried \
         tunnel and the drum renders smooth: land_r = {:.4} mm is at or beyond the \
         seat crest seat_c + groove_r = {:.4} mm",
        land_r * 1e3,
        (seat_c + groove_r) * 1e3
    );

    // ---- (2) The mouth admits the rope radially, WITH clearance ----
    // Chord of the seat circle (centre at seat_c, radius groove_r) cut by the
    // land cylinder at land_r. `max(0.0)` only fires when the seat lies wholly
    // clear of the land, a case (1) already rejects — it just keeps the failure
    // message numeric instead of NaN.
    //
    // The chord is still maximised at `land_r == seat_c`, but that maximum is no
    // longer merely `rope_dia`: a DIN 15061 arc makes it `2·groove_r =
    // 1.06·rope_dia`, so this asserts a strict 6 % clearance rather than #5580's
    // bare admission.
    //
    // MECHANICALLY the admitting band is now wide — any land within a half-width
    // `sqrt(groove_r² − (rope_dia/2)²) = 1.055 mm` of the arc centre still passes
    // a rope, so #5580's "a half-round is the ONLY depth that admits the rope"
    // reasoning retires here and must not be reinstated. This ASSERTION does not
    // admit that band: `min_mouth` is bit-identically `2·groove_r` (see below),
    // the chord's maximum, so it holds only within `groove_r·sqrt(2e-9)` =
    // 0.14 µm of `land_r == seat_c`. That is deliberate — the design pins
    // `land_r = seat_c` bit-exactly — and it is the LOOSER of the two depth pins
    // here; the volume gate's premise guard restates it at `groove_r·1e-6`
    // (3.18 nm). Do not weaken that guard on the belief that this one is
    // permissive about depth, and do not introduce an in-band cut-back without
    // moving both plus dev_capstan.ri's own band.
    //
    // The comparison keeps a relative epsilon: the tie MOVED, it did not vanish.
    // `MIN_MOUTH_CLEARANCE_FRAC` is derived from the same DIN ratio the design
    // multiplies into `groove_r`, so at the file's defaults the two sides are
    // still bit-for-bit equal — measured 0 ulps apart, now at 1.06·rope_dia
    // instead of at 1.00·rope_dia. That exactness is also luck of the particular
    // `rope_dia`, not structural. A bare `>=` would therefore sit at exactly
    // zero margin and one rounding difference away from a 1-ulp false red,
    // exactly as it did before. 1e-9 relative is nine orders below any real
    // seat-depth regression, which moves the chord by a fraction of a millimetre
    // at least.
    let offset = land_r - seat_c;
    let mouth = 2.0 * (groove_r.powi(2) - offset.powi(2)).max(0.0).sqrt();
    let min_mouth = rope_dia * (1.0 + MIN_MOUTH_CLEARANCE_FRAC);
    assert!(
        mouth >= min_mouth * (1.0 - 1e-9),
        "the rope seat's mouth must admit the rope RADIALLY with DIN 15061 \
         clearance: mouth chord = {:.4} mm but the required minimum is \
         rope_dia·(1 + {:.2} %) = {:.4} mm (rope_dia = {:.4} mm). The chord is \
         2·sqrt(groove_r² − (land_r − seat_c)²), maximised at land_r == seat_c \
         (the land plane through the section's arc centre) where it equals \
         2·groove_r; here land_r − seat_c = {:.4} mm, so a shallower land closes \
         the mouth over the rope and a deeper one closes it back under. A mouth \
         at exactly rope_dia means the arc reverted to a slip fit — see \
         `capstan_seat_arc_is_din_15061_oversize`. \
         (pitch_r = {:.4} mm, seat_c = {:.4} mm, groove_r = {:.4} mm, \
         land_r = {:.4} mm)",
        mouth * 1e3,
        MIN_MOUTH_CLEARANCE_FRAC * 100.0,
        min_mouth * 1e3,
        rope_dia * 1e3,
        offset * 1e3,
        pitch_r * 1e3,
        seat_c * 1e3,
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
    // `bore_r`, the land, and the seat between the groove bottom `seat_c −
    // groove_r == pitch_r − rope_dia/2` and the land. Nothing lives between the
    // bore and the groove bottom, so a cut anywhere in that gap cleanly
    // separates "bore" from "land or seat". Take the midpoint of the two
    // surfaces it separates rather than an offset off one of them: the design's
    // own groove-bottom-to-bore wall constraint then guarantees
    // `bore_r < bore_clear < groove_bottom` for EVERY parameter set the file
    // admits, so this cannot go stale inside the design's legal space
    // (`bore_r + groove_r`, the obvious spelling, is already above the groove
    // bottom at a legal `bore_dia = 40mm`).
    let groove_bottom = pitch_r - rope_dia / 2.0;
    let bore_clear = 0.5 * (bore_r + groove_bottom);
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
        (land_max - seat_c).abs() <= tol,
        "the drum's outermost surface inside the wrap band must be the land, and \
         the land must sit ON the seat's arc centre seat_c = {:.4} mm — that is \
         what makes the seat a half-round — but the mesh reaches {:.4} mm (tol \
         {:.4} mm; the design's own land_r cell is {:.4} mm). The reference here \
         is the recomputed seat_c and NOT land_r deliberately: land_r is the very \
         cell that parametrizes the blank's land cylinder, so a mesh-vs-land_r \
         check would move in lockstep with any seat-depth edit and stay green. \
         Against seat_c — derived from pitch_r, groove_r and rope_dia — this is \
         the design claim itself, and a land raised back above the arc centre \
         (the pre-#5580 submerged channel) lands here.",
        seat_c * 1e3,
        land_max * 1e3,
        tol * 1e3,
        land_r * 1e3
    );
    assert!(
        (seat_min - groove_bottom).abs() <= tol,
        "the seat must be cut all the way down to the groove bottom — which is \
         the seated rope's own underside, pitch_r − rope_dia/2 = {:.4} mm — but \
         the drum's innermost non-bore surface inside the wrap band is at \
         {:.4} mm (tol {:.4} mm). This reference does not mention groove_r at \
         all, so it holds for any seat arc radius. A seat that never breaks \
         through leaves this at the land radius {:.4} mm; a seat cut past the \
         rope's underside drives it below.",
        groove_bottom * 1e3,
        seat_min * 1e3,
        tol * 1e3,
        land_r * 1e3
    );
}

// ── DIN 15061: the seat arc is OVERSIZE, not a slip fit ──────────────────────

/// The rope seat's arc must be DIN 15061 oversize (`r = 0.53·d`) rather than a
/// zero-clearance slip fit, and oversizing it must not have moved the rope off
/// the D/d circle.
///
/// This is the module's conformance pin to an EXTERNAL standard, and the only
/// assertion here that can catch a revert to `groove_r = rope_dia/2`: both
/// other gates are parametrized by the file's own `groove_r` and would stay
/// green. See [`DIN_15061_SEAT_RATIO`] for why it references the standard's
/// number rather than the design's `seat_arc_ratio` cell.
///
/// Every claim here is scalar arithmetic over four cells — no mesh, no volume,
/// no geometry — so this gate reads [`dev_capstan_scalars`] and carries NO
/// `OCCT_AVAILABLE` guard. It is the module's only slip-fit gate, and OCCT
/// going absent or stub-degraded is silent in this repo, so it deliberately
/// keeps running where the other three skip.
///
/// Two claims, from the file's own cells:
///   1. **DIN conformance** — `groove_r == 0.53·rope_dia` (3.180 mm here).
///      That one ratio also carries DIN's mechanical reason for oversizing:
///      `groove_r > rope_dia/2` is exactly "the seat is wider than the rope at
///      the rope's widest section" (0.175 mm per side here), so a load-ovalised
///      braid cannot wedge against the seat walls. Asserting that width
///      inequality separately would add no coverage — it follows algebraically
///      from this claim for any positive `rope_dia` — and it is anyway gated
///      independently as `dev_capstan.ri`'s own constraint, which
///      [`capstan_surfaces_only_the_finished_drum`] requires to be satisfied.
///   2. **The design's own `seat_c` derivation is this module's** — the DSL's
///      `let seat_c` equals [`seat_arc_centre`]'s recomputation. This is the
///      one place the two derivations meet, and it is not lockstep: an edit to
///      either side alone lands here. It subsumes the two identities it
///      replaced — that the SEATED rope's centreline `seat_c − (groove_r −
///      rope_dia/2)` is still `pitch_r` (the D/d story), and that the seat
///      bottom `seat_c − groove_r` is still the seated rope's underside
///      `pitch_r − rope_dia/2` (what lets the mesh gate reference a figure that
///      never mentions `groove_r`) — both of which followed algebraically from
///      the Rust helper alone and so could not fail.
///
/// Both claims carry a **measured negative control**, taken on this branch with
/// no OCCT tessellation in the run (0.4–0.9 s per run, vs ~9 s for the kernel
/// path — so these are the kernel-free evaluation's numbers, not the
/// tessellation's): `seat_arc_ratio = 0.5` fails claim (1) with groove_r =
/// 3.000 mm against the standard's 3.180 mm, and `let seat_c = pitch_r +
/// groove_r` (the rope-bottoming term dropped) fails claim (2) with the design
/// reading 27.180 mm against a recomputed 24.180 mm.
#[test]
fn capstan_seat_arc_is_din_15061_oversize() {
    let cells = dev_capstan_scalars();

    let rope_dia = capstan_cell(cells, "rope_dia", DimensionVector::LENGTH);
    let pitch_r = capstan_cell(cells, "pitch_r", DimensionVector::LENGTH);
    let groove_r = capstan_cell(cells, "groove_r", DimensionVector::LENGTH);
    let seat_c = seat_arc_centre(pitch_r, groove_r, rope_dia);

    // ---- (1) DIN conformance ----
    let din_groove_r = DIN_15061_SEAT_RATIO * rope_dia;
    assert!(
        (groove_r - din_groove_r).abs() <= din_groove_r * 1e-9,
        "the rope seat's arc radius must conform to DIN 15061 rope-drum \
         practice, r_groove = {DIN_15061_SEAT_RATIO}·d: expected {:.6} mm for a \
         rope_dia of {:.4} mm, but the design's groove_r is {:.6} mm. A \
         groove_r of exactly rope_dia/2 = {:.4} mm is a zero-clearance SLIP FIT \
         — the seat then has no mouth clearance and a load-ovalised braid \
         pinches at the seat bottom. This assertion references the standard's \
         0.53 and NOT the file's seat_arc_ratio cell, deliberately: the volume \
         and mesh gates are both parametrized by groove_r and would stay green \
         through exactly this revert.",
        din_groove_r * 1e3,
        rope_dia * 1e3,
        groove_r * 1e3,
        rope_dia * 0.5e3
    );

    // ---- (2) The design's own seat_c derivation is this module's ----
    // Everything else here hangs off the RECOMPUTED `seat_arc_centre` — the
    // closed form's spine, the mesh land reference, the volume gate's premise
    // guard — precisely so no mesh check moves in lockstep with a design cell.
    // That leaves the DSL's own `let seat_c` unpinned, which is what this reads:
    // design cell vs. independent Rust recomputation, so an edit to EITHER side
    // alone lands here rather than being caught only indirectly (and only under
    // OCCT) by the land premise guard.
    //
    // This is the whole content of the two claims it replaced. Under tension the
    // rope bottoms out in its seat, so its centre lies `groove_r - rope_dia/2`
    // inboard of the arc centre: oversizing the arc moves the ARC outboard, and
    // the ROPE must not move at all. Given this equality, the seated centreline
    // is `pitch_r` and the seat bottom is `pitch_r - rope_dia/2` identically —
    // asserting those against the recomputed `seat_c` asserted nothing but
    // `(pitch_r + a) - a == pitch_r`.
    let design_seat_c = capstan_cell(cells, "seat_c", DimensionVector::LENGTH);
    assert!(
        (design_seat_c - seat_c).abs() <= seat_c * 1e-9,
        "the design's own seat_c cell must equal this module's recomputation of \
         it: dev_capstan.ri says {:.6} mm, seat_arc_centre(pitch_r = {:.6} mm, \
         groove_r = {:.6} mm, rope_dia = {:.6} mm) says {:.6} mm. Both spell \
         `pitch_r + groove_r − rope_dia/2` — the radius that puts the SEATED \
         rope's centreline on the D/d circle, the rope having bottomed out \
         `groove_r − rope_dia/2` inboard of the arc centre. If they part \
         company, then either the design moved the rope off pitch_r (drum_d, \
         the pitch circumference, active_turns, groove_len and the transmission \
         ratio all silently off, and the seat no longer bottoms on the rope's \
         underside pitch_r − rope_dia/2 that this module's mesh gate \
         references), or this module's helper drifted from the design and every \
         radius the closed form and the mesh checks hang off is measured \
         against the wrong spine.",
        design_seat_c * 1e3,
        pitch_r * 1e3,
        groove_r * 1e3,
        rope_dia * 1e3,
        seat_c * 1e3
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
    let rope_dia = capstan_cell(&result.values, "rope_dia", DimensionVector::LENGTH);
    let pitch_r = capstan_cell(&result.values, "pitch_r", DimensionVector::LENGTH);
    let groove_r = capstan_cell(&result.values, "groove_r", DimensionVector::LENGTH);
    let land_r = capstan_cell(&result.values, "land_r", DimensionVector::LENGTH);
    let lead = capstan_cell(&result.values, "lead", DimensionVector::LENGTH);
    let groove_len = capstan_cell(&result.values, "groove_len", DimensionVector::LENGTH);

    // ---- Pappus: unroll the helix to get its arc length ----
    // The spine is the seat's ARC CENTRE, which is where the swept section is
    // centred — not the rope centreline `pitch_r` the two coincide on only when
    // the seat is equal-radii. See [`seat_arc_centre`].
    let seat_c = seat_arc_centre(pitch_r, groove_r, rope_dia);
    let turns = groove_len / lead;
    let l_helix = helix_arc_len(seat_c, turns, groove_len);
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
         (seat_c = {seat_c:.6} m, pitch_r = {pitch_r:.6} m, groove_r = \
         {groove_r:.6} m, lead = {lead:.6} m, groove_len = {groove_len:.6} m). A \
         result near 2x this is a submerged full-tube channel, which #5580 \
         retired.",
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
    // DSL and this gate agree on what counts as "land_r == seat_c" and there is
    // no band of seat depths that `reify check` passes but this test panics on.
    // Both are pure fp slack: `let land_r = seat_c` is a bit-exact assignment,
    // so the measured difference is 0.0 and the width only has to be positive.
    let half_round_premise_tol = groove_r * 1e-6;
    assert!(
        (land_r - seat_c).abs() < half_round_premise_tol,
        "the half-round closed form below assumes land_r == seat_c (the seat's \
         arc centre lies ON the land surface), but land_r = {:.6} m and seat_c = \
         {:.6} m differ by {:.3e} m (window {:.3e} m, mirroring dev_capstan.ri's \
         own land_r band). A seat at any other depth seats a circular SEGMENT \
         rather than exactly half the section, so it needs a different seated \
         area AND a different centroid — re-derive the closed form rather than \
         widening the band.",
        land_r,
        seat_c,
        (land_r - seat_c).abs(),
        half_round_premise_tol
    );

    // Seated section = half the swept disc; its area centroid is 4r/(3π) inboard
    // of the spine, so it sweeps a shorter helix than the spine does.
    let a_seat = PI * groove_r.powi(2) / 2.0;
    let rho_c = seat_c - 4.0 * groove_r / (3.0 * PI);
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
    let cot_alpha = (2.0 * PI * seat_c * turns) / groove_len;
    let v_ends = 2.0 * (groove_r.powi(3) / 3.0) * cot_alpha;
    let v_pred = v_band + v_ends;

    let half_round_err = (delta - v_pred).abs() / v_pred;
    assert!(
        half_round_err < HALF_ROUND_REL_TOL,
        "seat stock removal off the centroid-Pappus + end-lens prediction: ΔV = \
         {delta:.6e} m³, expected {v_pred:.6e} m³ (rel err {:.2} %, tol {:.0} %). \
         Breakdown: A_seat = {a_seat:.6e} m² swept at the seated centroid radius \
         rho_c = {rho_c:.6} m (vs. spine seat_c = {seat_c:.6} m) gives v_band = \
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
/// check` reporting "All constraints satisfied"). Those are the oversize-arc
/// bound `groove_r > rope_dia / 2` (which is the anti-pinch AND the
/// mouth-clearance statement at once), the two-sided `land_r` band that pins
/// the seat depth to the arc centre `seat_c` (and so subsumes break-through),
/// the groove-bottom-to-bore wall `seat_c - groove_r > bore_r`, the
/// mouth-clearance lead `lead > groove_r * 2.0`, and `flange_r > land_r`. That
/// in-file band is what makes `reify check` — not just this Rust gate — report
/// a seat depth edited off `seat_c`.
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
