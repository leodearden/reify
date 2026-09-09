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
//! `band ≤ stroke ≤ band + lead` over the file's evaluated cells — reading the
//! instance-scoped spelling the DSL constraints resolve against, and separately
//! asserting it agrees with the bare template the rest of this module reads (see
//! [`sub_entity`]); the second pins that `CapstanDrive` states that relation in
//! the DSL itself — matched by the datums the compiled constraint actually reads
//! (see [`sub_cell_reads`]), not merely by *a* constraint being present, and
//! partitioned so that EACH half of the window (`≥ band`, `< band + lead`) is
//! observed separately, since both halves read the same two datums and either
//! alone would otherwise satisfy the gate — so a plain `reify check` catches a
//! divergence too. Neither reads geometry, so both go
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
//! also the only regression guard on `dev_capstan.ri` as a whole. That is why
//! its two FILE-WIDE claims — evaluation Error-freedom (`check_dev_capstan`,
//! against an enumerated `volume()` exception list) and "constraints ran and
//! none is violated" (`capstan_design_file_checks_clean_without_a_kernel`) —
//! are stated on the kernel-free surface: behind the OCCT gate they would cover
//! the file only on machines that happen to have a kernel.

use reify_core::{
    ConstraintNodeId, Diagnostic, DiagnosticCode, DimensionVector, ModulePath, Severity,
    SourceSpan, ValueCellId,
};
use reify_eval::{CheckResult, ConstraintCheckEntry, TessellateResult};
use reify_ir::{CompiledExpr, CompiledExprKind, Satisfaction, Value, ValueMap};
use std::collections::{HashMap, HashSet};
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
/// **The module's canonical statement of the two key forms and the override
/// drop** — other sites link here rather than restate it.
///
/// The value map carries BOTH key forms for a contained structure's scalar
/// cells: the bare template (`Capstan.band`, `Fairlead.stroke`) and the
/// instance-scoped composition below. They agree today only because parameter
/// overrides through `sub shuttle = Fairlead(…)` are DROPPED — only the `at`
/// pose comes through (task 4147; the design file's own header records it, and
/// it is why `Fairlead.stroke` is a hand-set param rather than derived from the
/// capstan). The file's `CapstanDrive` constraints resolve against the
/// INSTANCE, so the gates read the instance form and separately assert the two
/// agree — claim (0) of
/// `capstan_active_band_is_covered_by_the_fairlead_stroke`.
fn sub_entity(sub: &str) -> String {
    format!("{CAPSTAN_DRIVE_ENTITY}.{sub}")
}

/// Every `<sub>.<cell>` datum a compiled expression reads, as
/// `(entity path, cell name)` pairs in traversal order.
///
/// A cross-sub field reference does NOT compile to one `ValueRef`: the
/// compiler emits `IndexAccess { object: ValueRef(CapstanDrive.shuttle),
/// index: Literal(String("stroke")) }`, so
/// [`CompiledExpr::collect_value_refs`] on its own reports which SUBS a
/// constraint touches and not which of their cells. Re-pairing the two is what
/// lets `capstan_drive_constrains_the_shuttle_to_cover_the_band` pin the
/// relation's SHAPE rather than merely its presence — without it, swapping the
/// coverage constraint for any other `CapstanDrive`-scoped one leaves that gate
/// green while its message goes on describing `shuttle.stroke >= capstan.band`.
/// Necessary but NOT sufficient on its own, though: the two halves of that
/// window read overlapping datums, so recovering the cells is what lets the gate
/// PARTITION them (on `capstan.lead`) and observe each half separately — see
/// claim (1) there for the measurement.
///
/// Built on the canonical [`CompiledExpr::walk`] traversal rather than a local
/// match, so a future expression variant cannot quietly hide a read from it.
fn sub_cell_reads(expr: &CompiledExpr) -> Vec<(String, String)> {
    let mut reads = Vec::new();
    expr.walk(&mut |node| {
        if let CompiledExprKind::IndexAccess { object, index } = &node.kind
            && let CompiledExprKind::ValueRef(id) = &object.kind
            && let CompiledExprKind::Literal(Value::String(cell)) = &index.kind
        {
            reads.push((id.entity.clone(), cell.clone()));
        }
    });
    reads
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

/// The design file's compiled module, computed once per test binary — the
/// single compilation EVERY other fixture and test in this module reads.
///
/// Memoized for correctness first and cost second. Two gates read the compiled
/// module and the check surface *together*:
/// `capstan_drive_constrains_the_shuttle_to_cover_the_band` counts
/// `drive_template.constraints` against `result.constraint_results`, and
/// [`check_dev_capstan`] maps diagnostic label spans back through
/// `templates[].value_cells`. Both comparisons are only meaningful if the two
/// sides came from the SAME compilation; reaching [`compile_dev_capstan`]
/// twice would make them agree only by assuming the compiler is deterministic
/// — an assumption neither gate states, and one that a future span-numbering
/// or template-ordering change could quietly break. One `OnceLock` makes it a
/// fact instead of an assumption.
///
/// The cost side is the ordinary saving: the file is read, parsed and
/// stdlib-compiled once rather than once per caller, and the parse/compile
/// Error-freedom assertions inside [`compile_dev_capstan`] run once.
fn dev_capstan_compiled() -> &'static reify_compiler::CompiledModule {
    static M: OnceLock<reify_compiler::CompiledModule> = OnceLock::new();
    M.get_or_init(compile_dev_capstan)
}

/// Load, parse and compile `prj/printer_v01/dev_capstan.ri`, asserting both
/// stages are Error-diagnostic-free. Use [`dev_capstan_compiled`] rather than
/// calling this directly — every caller must share one compilation.
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

/// The geometry-consumer builtin whose unresolvability on the kernel-free
/// surface [`check_dev_capstan`] tolerates.
///
/// MESSAGE TEXT ONLY — nothing matches on it. The partition there is made on
/// `DiagnosticCode::EvalUnresolved` plus the label span's resolved cell
/// identity, both structured, so a rewording of the emission's prose (which
/// this module does not own) cannot reroute a diagnostic.
const VOLUME_BUILTIN_MENTION: &str = "`volume`";

/// Which cells the design file must produce those for, as `<entity>.<cell>`
/// identities — one per `volume()`-consuming cell of
/// `prj/printer_v01/dev_capstan.ri`.
///
/// Identities rather than a count. A bare `len() == 2` pin is satisfied by any
/// TWO `volume()` cells, so an edit that drops `blank_volume` and adds an
/// unrelated `volume()` cell elsewhere keeps the fixture green while its
/// failure message goes on naming the two cells above — the fixture would be
/// asserting a number and claiming an identity. Comparing the recovered set
/// pins both at once: the length agreement is implied by the equality.
///
/// Pinned rather than left open-ended so the exception stays an *enumerated*
/// one. If a later edit legitimately adds, renames or moves a `volume()` cell,
/// this list moves with it — a set that drifted on its own would put the
/// exception back to being a blanket ignore.
const VOLUME_UNRESOLVED_CELLS: [&str; 2] = ["Capstan.blank_volume", "Capstan.body_volume"];

/// Evaluate and constraint-check the design file with no kernel. Use
/// [`dev_capstan_checked`] rather than calling this directly.
///
/// Asserted Error-diagnostic-free like [`tessellate_dev_capstan`], but against
/// a *known-exception* list rather than the empty set: the file's
/// `blank_volume` / `body_volume` cells call `volume()`, a geometry-consumer
/// builtin only resolvable on the build()/tessellate() path, so this surface
/// reports one `EvalUnresolved` error per cell in [`VOLUME_UNRESOLVED_CELLS`]
/// by construction. The exception is recognised by CELL IDENTITY (the
/// diagnostic's label span, resolved through the compiled module's value
/// cells), never by the emission's message text. Those are the OCCT fixture's
/// business. `DiagnosticCode::ConstraintViolated` is routed out too — a
/// violated constraint is a DESIGN failure, not an evaluation one, and the
/// satisfaction gates own it (mechanism: [`Strictness`]). Every other Error is a
/// real evaluation regression and fails here.
///
/// Enumerating them rather than dropping all diagnostics is what keeps the
/// OCCT-less path — the one this fixture exists to serve — covered at all. The
/// two design-level gates read only the handful of cells they consume, so with
/// a blanket ignore a cell they never touch (`flange_r`, a `ShuttlePlate` cell,
/// a later `Fairlead` cell) could stop evaluating and nothing would observe it:
/// the Error-freedom assertion would live solely in the OCCT-gated
/// [`tessellate_dev_capstan`], and this module is the only regression guard on
/// `dev_capstan.ri` as a whole. `capstan_design_file_checks_clean_without_a_kernel`
/// closes the same gap on the CONSTRAINT half of that surface.
fn check_dev_capstan() -> CheckResult {
    let compiled = dev_capstan_compiled();
    let mut engine = reify_eval::Engine::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        None,
    );
    let result = engine.check(compiled);

    {
        // WHICH cell a diagnostic is about, structurally. The emission carries the
        // offending cell's `span` as its label
        // (`crates/reify-eval/src/engine_eval.rs`), so the identity is recovered by
        // mapping that span back through the compiled module's value cells — the
        // same compilation `result` came from, which is what
        // [`dev_capstan_compiled`] guarantees. Nothing here reads the message: the
        // prose is another crate's, and a rewording of it must not move a
        // diagnostic between the two arms below.
        let cell_by_span: HashMap<SourceSpan, &ValueCellId> = compiled
            .templates
            .iter()
            .flat_map(|t| t.value_cells.iter())
            .map(|cell| (cell.span, &cell.id))
            .collect();
        let labelled_cell = |d: &Diagnostic| -> Option<String> {
            let label = d.labels.first()?;
            let id = cell_by_span.get(&label.span)?;
            Some(format!("{}.{}", id.entity, id.member))
        };

        let (volume_errors, rest): (Vec<_>, Vec<_>) = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .partition(|d| {
                d.code == Some(DiagnosticCode::EvalUnresolved)
                    && labelled_cell(d)
                        .is_some_and(|cell| VOLUME_UNRESOLVED_CELLS.contains(&cell.as_str()))
            });
        // The OTHER Error a healthy design file can raise here is a constraint
        // VIOLATION, co-emitted alongside the typed result. It is routed out and
        // deliberately not asserted about — the satisfaction gates own it and can
        // say WHICH relation broke; see [`Strictness`] for why. This fixture's
        // claim is evaluation Error-freedom, nothing more.
        let unexpected: Vec<_> = rest
            .into_iter()
            .filter(|d| d.code != Some(DiagnosticCode::ConstraintViolated))
            .collect();
        assert!(
            unexpected.is_empty(),
            "unexpected evaluation errors on the kernel-free surface of \
             {DEV_CAPSTAN}: only the cells in `VOLUME_UNRESOLVED_CELLS` — the \
             {VOLUME_BUILTIN_MENTION}() geometry consumers — may fail to resolve \
             here (constraint violations go to the satisfaction gates). An \
             `EvalUnresolved` on any OTHER cell lands here by design: either a \
             {VOLUME_BUILTIN_MENTION}() cell was added and the exception list needs \
             moving with it, or a cell of the design stopped evaluating — and this \
             is the only place the latter is caught when OCCT is absent: \
             {unexpected:#?}"
        );
        // And every listed cell really did raise one — identities, not a count.
        // The partition above admits only cells already in the list, so this half
        // catches a MISSING one; an EXTRA is caught by `unexpected` just above.
        // A bare `len() == 2` pin would be satisfied by any two of them and could
        // see neither.
        let mut got: Vec<String> = volume_errors
            .iter()
            .map(|d| labelled_cell(d).expect("partitioned on the label resolving to a value cell"))
            .collect();
        got.sort();
        let mut want: Vec<String> = VOLUME_UNRESOLVED_CELLS
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        want.sort();
        assert_eq!(
            got, want,
            "{DEV_CAPSTAN} must raise exactly one `EvalUnresolved` on the \
             kernel-free surface per cell in `VOLUME_UNRESOLVED_CELLS`. A MISSING \
             entry means that cell was dropped or renamed — the OCCT fixture's \
             stock-removal gate would then be gating less than it reads. (An EXTRA \
             {VOLUME_BUILTIN_MENTION}() cell does not reach here: it is not in the \
             list, so it is not in this partition, and the `unexpected` assertion \
             above reports it. A SWAP trips both halves.) Raw diagnostics: \
             {volume_errors:#?}"
        );
    }

    result
}

/// Load, parse, compile and tessellate `prj/printer_v01/dev_capstan.ri` with a
/// real OCCT kernel, asserting the pipeline is Error-diagnostic-free at every
/// stage. Use [`dev_capstan`] rather than calling this directly.
///
/// "Error-diagnostic-free" here means free of PIPELINE errors:
/// `DiagnosticCode::ConstraintViolated` is routed out and left to the
/// satisfaction gates, the same partition [`check_dev_capstan`] makes on the
/// kernel-free surface — mechanism: [`Strictness`].
fn tessellate_dev_capstan() -> TessellateResult {
    let compiled = dev_capstan_compiled();

    // ---- Tessellate with a real OCCT kernel via SingleKernelHolder ----
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(Box::new(planner)),
    );

    let result = engine.tessellate_realizations(compiled);
    // Constraint VIOLATIONS are routed out, exactly as [`check_dev_capstan`]
    // routes them out of its own Error filter and for the reason [`Strictness`]
    // records. It bites harder here: this surface evaluates and constraint-checks
    // BEFORE it tessellates, so one broken DESIGN relation left in this filter
    // panics here first and takes all three OCCT gates down at once — including
    // the two that read only geometry and have nothing to do with the relation
    // that broke. The satisfaction gates own them; this fixture's claim stays what
    // its message says: the KERNEL path raised no Error.
    let geom_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .filter(|d| d.code != Some(DiagnosticCode::ConstraintViolated))
        .collect();
    assert!(
        geom_errors.is_empty(),
        "unexpected geometry errors tessellating {DEV_CAPSTAN} (constraint \
         violations are routed to the satisfaction gates and are not this \
         fixture's business): {geom_errors:#?}"
    );
    result
}

/// How strictly [`assert_constraints_ok`] reads a set of constraint results.
///
/// **This is the module's one statement of why a constraint failure is not a
/// fixture's business**, and every other site links here rather than re-deriving
/// it — nothing executable checks a restatement, so copies go stale. This enum
/// is the natural home because it is the axis that exists *because* of it.
///
/// How `SimpleConstraintChecker` reports a failure alongside the typed
/// `Satisfaction` result is that crate's business, not this module's: see
/// `crates/reify-constraints/src/lib.rs`. The one consequence this module acts
/// on is that a failure reaches the diagnostics as `ConstraintViolated`
/// (`Violated`) or as a mere WARNING (`Indeterminate`) — so both fixtures,
/// [`check_dev_capstan`] and [`tessellate_dev_capstan`], route the former OUT
/// of their Error filters and neither can see the latter at all.
///
/// Routed out because a violated constraint is a DESIGN failure, not a pipeline
/// one. Left in, it panics the shared fixture first — in every test at once,
/// including the pure-geometry ones that have nothing to do with the relation
/// that broke — under a message about evaluation or geometry that is false for
/// that failure, and it shadows the diagnosis [`assert_constraints_ok`] exists
/// to give (WHICH relation, at what strictness).
///
/// So NEITHER failure is a fixture's business: the satisfaction gates own both,
/// and they read `constraint_results` directly rather than the diagnostics — which
/// is what keeps their claims true however the checker chooses to report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Strictness {
    /// Every result must be `Satisfied` — `Indeterminate` fails too.
    ///
    /// `Indeterminate` is what a constraint whose inputs failed to EVALUATE
    /// reports: an undef leaf, or a cross-`sub` field reference that did not
    /// resolve. It is therefore the failure mode a `Violated`-only filter is
    /// blindest to — the constraint is still declared, still reported, and
    /// checking nothing — and per the enum doc above it reaches the diagnostics
    /// only as a WARNING, so no Error filter here sees it either. This strictness
    /// is the only claim in the module that catches it.
    AllSatisfied,
    /// Only `Violated` fails — the weaker statement `reify check` itself makes.
    NoneViolated,
}

/// Assert a `constraint_results` set is non-empty and holds at the strictness
/// asked for, optionally scoped to one entity; returns the entries examined.
///
/// One helper rather than a copy per site because all three of this module's
/// constraint claims — `capstan_surfaces_only_the_finished_drum` (scoped
/// `Capstan`, plus its file-wide mirror), the `CapstanDrive` scope of
/// `capstan_drive_constrains_the_shuttle_to_cover_the_band`, and the file-wide
/// `capstan_design_file_checks_clean_without_a_kernel` — are the same two-step
/// statement, and the first step is the one that silently rots when copied:
///
///   * **Non-emptiness first.** A satisfaction filter over an empty set is
///     vacuously green, and an empty `constraint_results` emits NO diagnostic,
///     so nothing else in the module can see it. That guard has to hold at every
///     site or the site that lost it stops asserting anything at all.
///   * **Then the satisfaction filter**, at [`Strictness`] — the axis that
///     genuinely differs between the sites, so it is a parameter rather than
///     three hand-written filters that could drift apart.
///
/// `surface` names which evaluation surface the entries came from and `note`
/// carries the site's own mechanical reading of a failure; both are only ever
/// message text.
fn assert_constraints_ok<'a>(
    entries: &'a [ConstraintCheckEntry],
    scope: Option<&str>,
    strictness: Strictness,
    surface: &str,
    note: &str,
) -> Vec<&'a ConstraintCheckEntry> {
    let scoped: Vec<&ConstraintCheckEntry> = match scope {
        Some(entity) => entries.iter().filter(|c| c.id.entity == entity).collect(),
        None => entries.iter().collect(),
    };
    let what = match scope {
        Some(entity) => format!("`{entity}` constraint results"),
        None => "constraint results".to_string(),
    };

    assert!(
        !scoped.is_empty(),
        "no {what} at all on {surface} of {DEV_CAPSTAN} — every structure in the \
         file declares constraints, so an empty set means the check never ran, or \
         stopped covering this scope, and the satisfaction filter would then pass \
         vacuously. {note} Entities checked: {:?}",
        entries.iter().map(|c| &c.id.entity).collect::<Vec<_>>()
    );

    let bad: Vec<_> = scoped
        .iter()
        .filter(|c| match strictness {
            Strictness::AllSatisfied => c.satisfaction != Satisfaction::Satisfied,
            Strictness::NoneViolated => c.satisfaction == Satisfaction::Violated,
        })
        .collect();
    assert!(
        bad.is_empty(),
        "{DEV_CAPSTAN} must satisfy {what} at its defaults on {surface} — {} of {} \
         did not, at {strictness:?} strictness. `Violated` means the design broke the \
         relation; `Indeterminate` means an input cell failed to EVALUATE, so the \
         constraint is present but checking nothing. {note} Results: {bad:#?}",
        bad.len(),
        scoped.len()
    );

    scoped
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
/// Both structures are `sub`s of the file's `CapstanDrive` assembly, so their
/// scalar cells are in the map under two key forms ([`sub_entity`]). Which one a
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

/// Relative slack on "these two reads are the same evaluated cell" — the
/// template-vs-instance comparison in claim (0) below.
///
/// Pure fp slack, not an empirical fit: the two sides are one cell read under
/// its two key spellings, bit-identical today. Deliberately ~4 orders below the
/// smallest physically meaningful drift (a 1 µm move on a 60 mm band is 1.7e-5
/// relative), so it can only ever absorb representation noise.
const CELL_SPELLING_REL_TOL: f64 = 1e-12;

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
/// Three claims, all read back from those cells:
///   0. the two spellings of each cell agree — the bare template
///      (`Capstan.band`, `Fairlead.stroke`) and the instance-scoped
///      [`sub_entity`] composition the DSL constraints resolve against
///      (`CapstanDrive.capstan.band`, `CapstanDrive.shuttle.stroke`). They can
///      diverge in principle and cannot today only by evaluator behaviour (see
///      [`sub_entity`] for why), so it is asserted rather than assumed: it is
///      what makes the template-form reads elsewhere in this module and the DSL
///      constraint provably statements about the same two numbers, and should
///      that behaviour change this claim fails first and points at the fork;
///   1. `band < groove_len` strictly — the two axial figures have not collapsed
///      into one (they cannot while `dead_total > 0`). This is the
///      definition-drift guard: `band` silently redefined as the total grooved
///      extent lands exactly here;
///   2. the coverage relation itself: `band <= stroke <= band + lead`, read off
///      the INSTANCE cells — the same form the DSL constraints resolve against,
///      so the Rust gate and the design gate check the same numbers.
///
/// Deliberately NOT asserted: `band == lead · active_turns` and `groove_len −
/// band == lead · dead_total`. Both restate a `let` of the design file one line
/// away, so on the happy path they exercise nothing but the evaluator's
/// multiply, and the drift they would guard against — `band` redefined as the
/// grooved extent — is caught strictly and independently by (1), which also
/// survives a `dead_total == 0` edit that the decomposition identity would
/// still pass. The active-band / dead-wrap decomposition itself is described in
/// `docs/projects/printer_v01.md` § "Drive: Vectran tendons + capstans", cited
/// by path and section rather than quoted (a copy here would go stale the
/// moment that paragraph is reworded — it already did once on this branch).
///
/// The upper bound in (2) is derived, not tuned to admit the observed 63 mm: the
/// stroke is the band rounded UP to a whole turn, `ceil(active_turns) · lead`,
/// and `ceil(x) · lead < x · lead + lead` holds identically for every `x`. A
/// bare lower bound would let the stroke grow without limit and still pass, so
/// it would not pin the design intent at all. Keeping both bounds derived is
/// what lets a future `lead` or `d_ratio` edit move this gate with the design
/// instead of going stale — the same principle as the module's "no geometry
/// number is hard-coded here". The design file states the same two-sided window
/// as a pair of `CapstanDrive` constraints, so `reify check` pins it too.
#[test]
fn capstan_active_band_is_covered_by_the_fairlead_stroke() {
    // No geometry is read here, so this gate runs on the kernel-free
    // evaluate + constraint-check surface: it must bite everywhere, not only
    // where OCCT happens to be installed (module doc §3).
    let result = dev_capstan_checked();

    let band = capstan_cell(&result.values, "band", DimensionVector::LENGTH);
    let lead = capstan_cell(&result.values, "lead", DimensionVector::LENGTH);
    let active_turns = entity_real(&result.values, CAPSTAN_ENTITY, "active_turns");
    let dead_total = entity_real(&result.values, CAPSTAN_ENTITY, "dead_total");
    let groove_len = capstan_cell(&result.values, "groove_len", DimensionVector::LENGTH);
    let stroke = entity_cell(&result.values, FAIRLEAD_ENTITY, "stroke", DimensionVector::LENGTH);

    // The instance-scoped spellings — what the DSL constraints resolve against.
    let capstan_inst = sub_entity(CAPSTAN_SUB);
    let shuttle_inst = sub_entity(SHUTTLE_SUB);
    let band_inst = entity_cell(&result.values, &capstan_inst, "band", DimensionVector::LENGTH);
    let stroke_inst = entity_cell(&result.values, &shuttle_inst, "stroke", DimensionVector::LENGTH);

    // ---- (0) The template and instance spellings are the same number ----
    // Not a formality: an override through `sub capstan = Capstan(...)` would
    // rebind the instance and leave the template default standing, forking the
    // two, and everything below plus the DSL constraints would then be talking
    // past each other. Why it holds today, and why that is behaviour rather
    // than design: [`sub_entity`]. Only fp slack is in play here — the same
    // evaluated cell read twice, bit-identical today.
    assert!(
        (band_inst - band).abs() <= CELL_SPELLING_REL_TOL * band.abs(),
        "`{CAPSTAN_ENTITY}.band` and `{capstan_inst}.band` must be the same evaluated \
         cell: template {:.9} mm, instance {:.9} mm. A divergence means a `sub` \
         constructor override took effect (task 4147 fixed?), forking the template \
         form the claims below read from the instance form the file's constraints \
         resolves against — see this test's doc comment.",
        band * 1e3,
        band_inst * 1e3
    );
    assert!(
        (stroke_inst - stroke).abs() <= CELL_SPELLING_REL_TOL * stroke.abs(),
        "`{FAIRLEAD_ENTITY}.stroke` and `{shuttle_inst}.stroke` must be the same \
         evaluated cell: template {:.9} mm, instance {:.9} mm. A divergence means a \
         `sub` constructor override took effect (task 4147 fixed?), and the coverage \
         window below — which reads the instance form — would be gating a different \
         stroke from the one this module describes.",
        stroke * 1e3,
        stroke_inst * 1e3
    );

    // ---- (1) The two axial figures have not collapsed into one ----
    // Also the definition-drift guard: `band` redefined as the total grooved
    // extent lands here, strictly, without restating the file's own `let`.
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

    // ---- (2) The shuttle covers the band ----
    // Lower bound: a stroke short of the band leaves the fleet angle to open up
    // at one end of travel — the fairlead stops tracking and starts side-loading.
    // Upper bound: the stroke is the band rounded UP to a whole turn,
    // ceil(active_turns) · lead, and ceil(x) · lead < x · lead + lead for every
    // x. So the two bounds together say "one whole turn of margin, no more" —
    // neither is the literal 63 mm, which is deliberately absent from this
    // assertion so a lead/d_ratio edit moves the gate with the design.
    // Read off the INSTANCE cells: the file's `CapstanDrive` constraints spell
    // the same window over those, so asserting the same spelling here means the
    // two gates cannot end up pinning different numbers. Claim (0) already
    // proved they equal the template forms, which is what keeps `lead`
    // (template) a legitimate term in the upper bound.
    assert!(
        stroke_inst >= band_inst && stroke_inst <= band_inst + lead,
        "the fairlead shuttle's stroke must cover the capstan's band migration and \
         overshoot it by less than one whole turn: {shuttle_inst}.stroke = {:.6} mm \
         against a band of {:.6} mm (lower bound) and band + lead = {:.6} mm (upper \
         bound). Below the band the shuttle runs out of travel before the wrap band \
         does — the fleet angle opens and the fairlead side-loads instead of guiding. \
         Above it the stroke is no longer the band rounded up to a whole turn, \
         ceil({active_turns:.6}) · lead = {:.6} mm — it is an unexplained number.",
        stroke_inst * 1e3,
        band_inst * 1e3,
        (band_inst + lead) * 1e3,
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
    // Two statements, both through [`assert_constraints_ok`] so the non-emptiness
    // guard cannot go missing from either: a satisfaction filter over an empty
    // `constraint_results` (nothing checked at all) is vacuously green. Scoped to
    // `Capstan` the claim is the strict one — all of its constraint inputs are
    // defined on the happy path, so `Indeterminate`, precisely what an undef input
    // produces when a geometry or scalar cell fails to evaluate, has to fail here
    // too. File-wide it is the weaker statement `reify check` itself makes.
    assert_constraints_ok(
        &result.constraint_results,
        Some(CAPSTAN_ENTITY),
        Strictness::AllSatisfied,
        "the OCCT build surface",
        "Anything other than `Satisfied` for this entity — Violated OR \
         Indeterminate — is a regression in the rope-seat work this module gates.",
    );
    assert_constraints_ok(
        &result.constraint_results,
        None,
        Strictness::NoneViolated,
        "the OCCT build surface",
        "This is the file-wide claim; the strict entity-scoped one is asserted \
         just above.",
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
/// Three claims:
///   1. `CapstanDrive` declares BOTH halves of the coverage window, told apart
///      by the datums each compiled constraint reads — the relation's SHAPE,
///      over the compiled template, not merely "some constraint exists".
///      Matching datums at all is what lifts the gate above presence: presence
///      alone is satisfied by any `CapstanDrive`-scoped constraint (a pose or
///      clearance check, say). But datum-matching alone is necessary and NOT
///      sufficient, because the file spells the window as a PAIR whose halves
///      read overlapping datums — `shuttle.stroke < capstan.band +
///      capstan.lead` reads `shuttle.stroke` and `capstan.band` exactly as the
///      lower bound does, so "some constraint reads both" is satisfied by
///      either half alone. Measured in this worktree against that weaker form:
///      deleting `constraint shuttle.stroke >= capstan.band` left the module
///      green (6 passed), and so did deleting `constraint shuttle.stroke <
///      capstan.band + capstan.lead` — the blind spot was SYMMETRIC, and in
///      both directions `reify check` had quietly stopped enforcing that side.
///      So the constraints reading both datums are PARTITIONED on whether they
///      ALSO read `capstan.lead`, and each half is asserted non-empty:
///      `band_only` is the lower bound ("the stroke covers the band"),
///      `with_lead` the upper ("…with less than one whole turn of margin").
///      The halves are disjoint by construction, so this also entails that the
///      two are distinct constraints, and it reds on either deletion. The
///      relation is cross-structure — a cell of `capstan` against a cell of
///      `shuttle` — so the assembly that owns both `sub`s is the only scope it
///      CAN be stated in. Deriving `Fairlead.stroke` from the capstan instead
///      would need a parameter override through `sub shuttle = Fairlead(…)`,
///      which does not come through (see [`sub_entity`]). So the stroke stays a
///      hand-set param and the assembly asserts it stays honest;
///   2. the checker actually EVALUATED what the template declares — every
///      declared constraint's `ConstraintNodeId` appears among the reported
///      results. A declared-but-unevaluated relation would leave (3) quantifying
///      over less than the file states — and, if none reached the surface at all,
///      over an empty set, i.e. vacuously green. Containment, not a count: the
///      check surface also reports the active branch of any `when`-guarded group
///      (`Engine::collect_active_constraints`), so an over-count is expected
///      rather than a failure;
///   3. every one of those results is `Satisfied`.
///
/// (1)'s partition is a COVERAGE-HOLE closure, not one half of a RED/GREEN pair
/// — both constraints it separates already exist in the design file, and the
/// weaker form of this gate was green while they did. What was absent was this
/// test's ability to notice either one LEAVING, so the strengthening was
/// verified by deleting each half in turn and watching this test red (and the
/// pre-strengthening baseline recorded under (1) by watching it stay green),
/// rather than by a failing-then-passing test of new design behaviour.
///
/// STILL deliberately NOT pinned, after (1)'s partition: the direction of each
/// comparison (`capstan.band <= shuttle.stroke` is the same relation spelled the
/// other way round), the source phrasing, and the arrangement of the operands —
/// `shuttle.stroke - capstan.band < capstan.lead` reads all three datums and
/// lands in `with_lead` unchanged. A gate that rejected any of those would be
/// pinning source phrasing rather than design intent. What (1) pins is that both
/// halves of the window are still stated and still about the same datums.
///
/// Nor is the partition pinned as a COUNT ("exactly one half reads `lead`"),
/// only as "at least one in each half" — the same reasoning (2) gives for
/// preferring containment to a count. A future THIRD lead-reading bound
/// tightening the margin is a legitimate edit that a count would red on while
/// claiming the exact INVERSE of what happened, sending the reader after a
/// dropped constraint that is still there. It costs nothing: replacing the lower
/// bound with a second lead-reading constraint empties `band_only` and reds
/// anyway.
///
/// ACCEPTED LIMITATION, named in the failure message rather than worked around:
/// re-spelling the pair as ONE conjunction constraint reds this gate. That is
/// intended, not collateral — one entry reports one `Satisfaction`, so the check
/// surface could no longer say WHICH half broke and (3)'s per-half diagnosis
/// would be lost.
///
/// (3) is asserted positively rather than as "nothing is `Violated`" — the same
/// reason `capstan_surfaces_only_the_finished_drum` gives for `Capstan`, and it
/// matters more here. A cross-sub field reference that fails to resolve
/// evaluates to `Indeterminate`, not `Violated`, so a `!= Violated` filter would
/// stay green on exactly the failure mode this constraint is most exposed to,
/// and the file-wide `violated.is_empty()` check in that test is likewise
/// vacuous on it. That combination — an empty result set and an indeterminate
/// one both reading as green — is the gap this test closes; (2) is what keeps
/// the "empty result set" half of it closed.
#[test]
fn capstan_drive_constrains_the_shuttle_to_cover_the_band() {
    // Kernel-free surface deliberately: this gate's whole claim is that the
    // relation bites outside a full OCCT run, so it must not itself be skipped
    // when OCCT is absent (module doc §3).
    let result = dev_capstan_checked();

    // ---- (1) The assembly states THIS relation, not merely some constraint ----
    // Read off the compiled template rather than the check results: a
    // `ConstraintCheckEntry` carries only an id and a satisfaction, so the
    // evaluated side cannot tell `shuttle.stroke >= capstan.band` apart from any
    // other `CapstanDrive`-scoped constraint. The template still holds the
    // expression, and `sub_cell_reads` recovers the `<sub>.<cell>` datums out of
    // it.
    // The SAME compilation `dev_capstan_checked()` above was produced from
    // ([`dev_capstan_compiled`]), so claim (2)'s count of declared constraints and
    // its count of reported results are two views of one module rather than two
    // compilations assumed to agree.
    let compiled = dev_capstan_compiled();
    let drive_template = compiled
        .templates
        .iter()
        .find(|t| t.name == CAPSTAN_DRIVE_ENTITY)
        .unwrap_or_else(|| {
            panic!(
                "{DEV_CAPSTAN} must declare the `{CAPSTAN_DRIVE_ENTITY}` assembly \
                 that owns both `sub`s; templates compiled: {:?}",
                compiled
                    .templates
                    .iter()
                    .map(|t| &t.name)
                    .collect::<Vec<_>>()
            )
        });

    let stroke_read = (sub_entity(SHUTTLE_SUB), "stroke".to_string());
    let band_read = (sub_entity(CAPSTAN_SUB), "band".to_string());
    // `lead` is a `capstan`-sub read of the same `IndexAccess` shape
    // [`sub_cell_reads`] already recovers, so it needs no helper change — it is
    // simply the datum that tells the window's two halves apart.
    let lead_read = (sub_entity(CAPSTAN_SUB), "lead".to_string());

    // Every `CapstanDrive` constraint with its datums, kept for the failure
    // messages: this dump is what makes a shape regression diagnosable at all,
    // rather than reporting only that *something* is missing.
    let declared: Vec<(&ConstraintNodeId, Vec<(String, String)>)> = drive_template
        .constraints
        .iter()
        .map(|c| (&c.id, sub_cell_reads(&c.expr)))
        .collect();

    // Both halves of the window read `stroke` and `band`; only the upper one
    // also reads `lead`. Partitioning on that datum is what makes each half
    // INDIVIDUALLY observable — see claim (1) for the measurement showing that
    // "some constraint reads both" is green with either half deleted. The two
    // sets are disjoint by construction, so asserting both non-empty also
    // entails that the pair is two distinct constraints.
    let (with_lead, band_only): (Vec<_>, Vec<_>) = declared
        .iter()
        .filter(|(_, reads)| reads.contains(&stroke_read) && reads.contains(&band_read))
        .partition(|(_, reads)| reads.contains(&lead_read));

    assert!(
        !band_only.is_empty(),
        "`{CAPSTAN_DRIVE_ENTITY}` must carry the LOWER half of the coverage window \
         — a constraint relating `{}.{}` to `{}.{}` and NOT reading `{}.{}`, i.e. \
         `shuttle.stroke >= capstan.band` (either order is fine; the datums are \
         not). Without it the fairlead runs out of travel before the wrap band \
         does: the rope departs outside the shuttle's reach, the fleet angle opens \
         and the fairlead side-loads the rope instead of guiding it. Note this is \
         NOT satisfied by the upper half — `shuttle.stroke < capstan.band + \
         capstan.lead` reads `stroke` and `band` too, which is why the halves are \
         told apart by `{}.{}`. ACCEPTED LIMITATION: re-spelling the pair as ONE \
         conjunction constraint reds this gate deliberately — a single entry \
         reports a single `Satisfaction`, so the check surface could no longer say \
         WHICH half broke and claim (3)'s per-half diagnosis would be lost. If that \
         was the deliberate edit, this gate and its message have to move with it. \
         Datums each `{CAPSTAN_DRIVE_ENTITY}` constraint reads: {declared:?}",
        stroke_read.0,
        stroke_read.1,
        band_read.0,
        band_read.1,
        lead_read.0,
        lead_read.1,
        lead_read.0,
        lead_read.1,
    );
    assert!(
        !with_lead.is_empty(),
        "`{CAPSTAN_DRIVE_ENTITY}` must carry the UPPER half of the coverage window \
         — a constraint reading `{}.{}`, `{}.{}` AND `{}.{}`, i.e. \
         `shuttle.stroke < capstan.band + capstan.lead`. Without it the stroke is \
         unbounded above: ANY stroke exceeding the band passes, and the design \
         intent that `Fairlead.stroke` is the band rounded up to a whole turn \
         (`ceil(active_turns) * lead`) stops being asserted anywhere in the DSL — \
         a shuttle sized for twice the travel it needs would `reify check` clean. \
         The lower half alone does not cover this: it reads `stroke` and `band` \
         but not `{}.{}`. Datums each `{CAPSTAN_DRIVE_ENTITY}` constraint reads: \
         {declared:?}",
        stroke_read.0,
        stroke_read.1,
        band_read.0,
        band_read.1,
        lead_read.0,
        lead_read.1,
        lead_read.0,
        lead_read.1,
    );

    // ---- (3) …and it holds at the file's defaults ----
    // Ordered ahead of (2) only because [`assert_constraints_ok`] leads with the
    // non-emptiness guard and hands back the scoped `CapstanDrive` entries that (2)
    // then counts — so both claims quantify over ONE scoped read rather than two
    // filters that could drift apart. The claims stay numbered by the argument in
    // the doc comment above, not by execution order.
    let drive_constraints = assert_constraints_ok(
        &result.constraint_results,
        Some(CAPSTAN_DRIVE_ENTITY),
        Strictness::AllSatisfied,
        "the kernel-free check surface",
        "Here `Violated` means the shuttle's stroke no longer covers the capstan's \
         band migration — the fairlead runs out of travel before the wrap band does \
         — and `Indeterminate` would mean the cross-sub field reference stopped \
         resolving, which is why this scope is read at `AllSatisfied`.",
    );

    // ---- (2) …the checker evaluated every constraint the template declares ----
    // Stated as CONTAINMENT by `ConstraintNodeId`, not as a count: the reported set
    // can legitimately be a SUPERSET of `template.constraints` (mechanism:
    // `Engine::collect_active_constraints`, crates/reify-eval/src/engine_constraints.rs).
    // A count pin would red on a `when`-guarded relation — a plausible next edit,
    // since the project doc describes two capstans — claiming the exact INVERSE of
    // what happened, and sending the reader after a dropped constraint that does
    // not exist. Containment is also the claim that matters: it is what makes (3)'s
    // all-`Satisfied` sweep cover every relation the template declares rather than
    // some subset. NOTE the direction — a constraint MOVED into a guarded group
    // whose guard evaluates `Undef` leaves both sides at once and is not covered here.
    let reported: HashSet<&ConstraintNodeId> = drive_constraints.iter().map(|c| &c.id).collect();
    let unchecked: Vec<&ConstraintNodeId> = drive_template
        .constraints
        .iter()
        .map(|c| &c.id)
        .filter(|id| !reported.contains(id))
        .collect();
    assert!(
        unchecked.is_empty(),
        "every constraint `{CAPSTAN_DRIVE_ENTITY}` declares must reach the check \
         surface: {} of the {} declared did not — {unchecked:?}. A declared relation \
         that is never evaluated enforces nothing, and claim (3) would then quantify \
         over less than the file states. Containment rather than equality: the checker \
         also reports the active branch of any `when`-guarded group, so MORE results \
         than declared constraints is expected and not a failure. Reported: {:?}",
        unchecked.len(),
        drive_template.constraints.len(),
        drive_constraints.iter().map(|c| &c.id).collect::<Vec<_>>()
    );
}

// ── …and the whole file still checks clean with no kernel at all ─────────────

/// The file-wide statement plain `reify check` makes — constraints were checked,
/// and none of them is `Violated` — asserted on the KERNEL-FREE surface.
///
/// Half of this is a DECOUPLING and half is NEW coverage, and the two are worth
/// keeping apart:
///
///   * `Violated`-emptiness. This half used to be implied by
///     [`check_dev_capstan`]'s Error-freedom assertion, through the
///     `Diagnostic::error` the checker co-emits alongside every `Violated`
///     result — but that code is now routed OUT of both fixtures' Error filters,
///     precisely so the satisfaction gates own the failure and can name the
///     relation (see [`Strictness`]). On the kernel-free surface this test is
///     therefore the file-wide owner, not a duplicate of one: a violated
///     `Fairlead` / `IdlerPulley` / `ShuttlePlate` constraint is caught HERE and
///     nowhere else. Measured, not assumed — dropping `IdlerPulley.sheave_od`
///     below its `brg_od` bound fails this test and the OCCT-gated
///     `capstan_surfaces_only_the_finished_drum`, and does NOT panic
///     `check_dev_capstan`. It also reads `constraint_results` directly rather
///     than any diagnostic, so the claim does not rest on the checker's severity
///     choice at all — which this module neither owns nor pins anywhere.
///   * Non-emptiness is the half that is genuinely uncovered otherwise. An empty
///     `constraint_results` emits NO diagnostic, so the fixture cannot see it,
///     and every other kernel-free gate here quantifies over a subset: claim (2)
///     of `capstan_drive_constrains_the_shuttle_to_cover_the_band` covers only
///     `CapstanDrive` results, and
///     `capstan_active_band_is_covered_by_the_fairlead_stroke` reads value cells
///     rather than constraints. `capstan_surfaces_only_the_finished_drum` does
///     make the file-wide non-emptiness statement — but it is OCCT-gated and
///     returns early wherever the kernel is absent, and this module is the only
///     regression guard on `dev_capstan.ri` as a whole. Without this test, a
///     check surface that stopped producing results for every entity but
///     `CapstanDrive` reads green on a kernel-free machine.
///
/// Two claims, at two scopes, because strictness and scope are independent
/// choices here:
///
///   * FILE-WIDE it is deliberately the WEAK claim (`Violated`-emptiness),
///     mirroring the scoping that OCCT-gated copy uses. A constraint reading one
///     of the `volume()` cells would only be decidable with a kernel and would
///     read `Indeterminate` here, so an all-`Satisfied` pin at file scope would
///     fail on a legitimate edit rather than on a regression.
///   * SCOPED to `Capstan` it is the strict anything-but-`Satisfied` claim,
///     because every `Capstan` constraint reads scalar cells only (`land_r`,
///     `pitch_r`, `groove_r`, `bore_r`, `lead`, `flange_r`, `d_ratio`) and none
///     of them touches a `volume()` cell — so `Indeterminate` there is always a
///     regression, never a missing kernel. That claim was previously made for
///     `Capstan` ONLY inside the OCCT-gated
///     `capstan_surfaces_only_the_finished_drum`, which returns early with no
///     kernel; since `Indeterminate` reaches the diagnostics only as a warning
///     (see [`Strictness::AllSatisfied`]), a `Capstan` constraint that was
///     present, reported and checking nothing was caught by nothing at all on a
///     machine without OCCT. The `CapstanDrive` scope gets the same strict
///     treatment, for the same reason, in
///     `capstan_drive_constrains_the_shuttle_to_cover_the_band`.
///
/// The non-emptiness guard is not ceremony: a `Violated` filter over an empty
/// `constraint_results` is vacuously green, the same trap
/// `capstan_surfaces_only_the_finished_drum` guards against for the same reason.
#[test]
fn capstan_design_file_checks_clean_without_a_kernel() {
    let result = dev_capstan_checked();

    assert_constraints_ok(
        &result.constraint_results,
        None,
        Strictness::NoneViolated,
        "the kernel-free check surface",
        "Read off `constraint_results` directly, so this holds however the checker \
         chooses to report a violation as a diagnostic. Both fixtures route \
         `ConstraintViolated` out of their Error filters, so on the kernel-free \
         surface this gate is where a file-wide violation lands.",
    );

    // ---- …and `Capstan` strictly, on this surface too ----
    // The file-wide claim above is deliberately weak, but `Capstan` is one of the
    // entities whose constraint inputs are ALL defined kernel-free — every one of
    // them reads scalar cells only (`land_r`, `pitch_r`, `groove_r`, `bore_r`,
    // `lead`, `flange_r`, `d_ratio`), none reaches a `volume()` cell — so the
    // strict claim is decidable here and safe to make.
    //
    // Without it, `Indeterminate` on a `Capstan` constraint reads green on every
    // kernel-free machine — the blind spot [`Strictness`] documents. Neither of
    // the two places it could otherwise be caught does: the file-wide claim just
    // above is `NoneViolated`, and the module's other strict `Capstan` claim lives
    // in `capstan_surfaces_only_the_finished_drum`, which returns early with no
    // kernel.
    assert_constraints_ok(
        &result.constraint_results,
        Some(CAPSTAN_ENTITY),
        Strictness::AllSatisfied,
        "the kernel-free check surface",
        "`Capstan`'s constraints read scalar cells only, so all of them are \
         decidable without a kernel: an `Indeterminate` here means an input cell \
         stopped evaluating, NOT that a kernel was needed.",
    );
}
