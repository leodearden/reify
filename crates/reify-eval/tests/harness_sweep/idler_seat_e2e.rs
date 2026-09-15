//! End-to-end acceptance gate for the printer_v01 `IdlerPulley`'s rope seat
//! (task #6135), which extends the DIN 15061 oversize seat arc from the
//! Capstan — where #5683 landed it — to the idler sheave, in lockstep across
//! the design's two copies of the structure.
//!
//! The sibling module [`super::capstan_groove_e2e`] gates the Capstan end of
//! the same standard; this one reuses its externally-pinned ratio
//! ([`super::capstan_groove_e2e::DIN_15061_SEAT_RATIO`]) and its
//! construction-independent arc-centre formula
//! ([`super::capstan_groove_e2e::seat_arc_centre`]) rather than restating
//! either. What does NOT carry over is that module's
//! `MIN_MOUTH_CLEARANCE_FRAC`: it is the Capstan's land-at-arc-centre case and
//! over-predicts the mouth here, where the rim is pinned at the rope
//! centreline instead. The floor is re-derived from the ratio below.
//!
//! **Why the seat moves at all.** Both copies cut the seat with a `torus`
//! whose tube radius was the tendon's own radius — the zero-clearance slip fit
//! DIN 15061 exists to open up. Oversizing the tube to `0.53·d` alone would
//! sink the seated rope 0.180 mm below the rim, and `sheave_od/2 == r_pitch`
//! is not decoration in this design: `printer.ri` threads it into
//! `CapstanUnit.r_pitch`, into `CarriageIdlers.ab_split` and into
//! `DriveTendons` ("every tendon centreline is tangent to its rope's pitch
//! circle"), behind 31 hand-derived placements. So the arc's CENTRE moves
//! outboard to compensate, and two exact, ratio-independent identities fall
//! out — the seat bottom and the seated rope's centreline both stay where they
//! were. Those identities are what the gates here pin.
//!
//! **Why this module is split across two design files.** The contract lives in
//! `prj/printer_v01/printer.ri`, which is the original, and is read
//! KERNEL-FREE (`compile_with_stdlib_checked` + `Engine::check`): cells and
//! `constraint_results` only. `Engine::tessellate_realizations` takes no
//! entity or scope argument, so tessellating printer.ri means tessellating all
//! 32 of its structures — 3635 lines, 31 torus-boolean idlers, never once
//! tessellated by any test in this repo and plausibly minutes. A full stdlib
//! COMPILE of it, by contrast, already runs in CI today
//! (`crates/reify-compiler/tests/harness_constructor_typing/orientation_constructor_typing_tests.rs`),
//! so the kernel-free surface is proven affordable. The OCCT mesh readback
//! therefore comes from `prj/printer_v01/dev_capstan.ri` (345 lines, ~5 s),
//! which carries four `IdlerPulley` instances in its `Fairlead` shuttle — and
//! `idler_copies_stay_in_lockstep` is precisely the assertion that licenses
//! taking the EXPECTATIONS from one file and the MESH from the other.
//!
//! # The measured kernel-free surface of printer.ri (task #6135, pre-2)
//!
//! Every figure and every allowlist entry below was MEASURED on this branch
//! before the first assertion was written, so the loader's error handling is
//! sized to the file as it actually is. printer.ri parses with 0 errors, and
//! `IdlerPulley`'s eleven cells and `DriveTendons.r_pitch` all resolve off the
//! bare template — printer.ri instantiates `IdlerPulley` 31 times but task
//! 4147 drops parameter overrides, so the bare-template form is the one to
//! read. `DriveTendons.r_pitch` and `IdlerPulley.sheave_r` both measure
//! 0.018000000000000002 m: BIT-IDENTICAL, because `36mm / 2` and `18mm` are
//! the same IEEE-754 double. `IdlerPulley`'s three constraints are all
//! `Satisfied`, out of 406 file-wide across 29 entities.
//!
//! Pre-change `IdlerPulley` figures, for the delta claims the gates below
//! make: rim 18.000 mm, seat bottom 15.000 mm, seat opening at the rim
//! 6.000 mm, sheave width 10.000 mm.
//!
//! **The file emits Error-severity diagnostics in TWO populations, and they
//! are allowlisted asymmetrically.** That asymmetry is the measurement's real
//! finding; a single blanket filter would have hidden both, and treating them
//! alike would make this gate demand that a bug stay unfixed.
//!
//! 1. **Six `EvalUnresolved` at the CHECK stage** — `AFrame.vol_body` and the
//!    five `ToolDock.pen_*` cells, every one of them a `volume()` consumer.
//!    This is the exact analogue of
//!    [`super::capstan_groove_e2e`]'s `VOLUME_UNRESOLVED_CELLS`: `volume()` is
//!    a geometry-consumer builtin resolvable only on the build()/tessellate()
//!    path, so these are a PERMANENT property of the kernel-free surface.
//!    Allowlisted as an exact identity set, both directions — a missing entry
//!    means the cell was dropped or renamed.
//!
//! 2. **Eleven `UnresolvedName` at the COMPILE stage** — qualified
//!    enum-variant paths (`Finish.Satin`, `ElementOrder.P2`, `ShellForce.Off`)
//!    across eight cells: `CFRP_Rolled_Tube.appearance`,
//!    `HomogenisedPanel.appearance`, `GantryFea.{r_static, opts_cant,
//!    opts_ss}` and `AFrame.{opts_field, r_pil, mc}`. These are a PRE-EXISTING
//!    compiler/stdlib gap on main, not something this task introduced: the
//!    enum-name scope is built from the module's own `enum_defs`
//!    (`crates/reify-compiler/src/entity.rs`), and printer.ri is a single file
//!    that never imports the modules defining these three enums. Nothing in
//!    the repo observes them today — the one existing test that compiles
//!    printer.ri counts *infer warnings* only
//!    (`orientation_constructor_typing_tests::real_printer_ri_emits_zero_infer_warnings`).
//!    So they are allowlisted as a CEILING rather than an expectation: these
//!    cells MAY raise it and the loader tolerates them, but none is required
//!    to. Fixing the gap therefore makes this gate greener, never redder —
//!    which is the whole reason population 2 is not held to population 1's
//!    exact-identity rule. Filed as an observation; none of the eight cells is
//!    in `IdlerPulley` or `DriveTendons`, so none touches this gate's subject.
//!
//! Both populations are recognised by CELL IDENTITY, never by message text —
//! the prose belongs to another crate and a rewording of it must not reroute a
//! diagnostic. Population 1 resolves the label span through the compiled
//! module's value cells exactly as the capstan gate does. Population 2 needs
//! one refinement: its label sits on an expression INSIDE a cell rather than on
//! the cell itself, so the identity is the smallest value cell whose span
//! CONTAINS the label's. Both are computed against the same compilation the
//! diagnostics came from, so neither hard-codes a byte offset and edits to this
//! file's own `IdlerPulley` cannot shift them.

use reify_core::{Diagnostic, DimensionVector, ModulePath, Severity, SourceSpan, ValueCellId};
use reify_eval::CheckResult;
use reify_ir::{Value, ValueMap};
use std::collections::HashMap;
use std::sync::OnceLock;

/// The design file that OWNS the idler seat contract, reached from this crate's
/// manifest dir. Mirrors [`super::capstan_groove_e2e`]'s `DEV_CAPSTAN`.
const PRINTER_RI: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../prj/printer_v01/printer.ri"
);

/// The design entity whose cells and constraints this module gates.
const IDLER_ENTITY: &str = "IdlerPulley";

/// The structure whose `r_pitch` the idler's seated rope has to agree with.
///
/// `printer.ri`'s cross-structure single source: `DriveTendons` declares
/// `let r_pitch = 18mm` (:914) under the statement that "every tendon
/// centreline is tangent to its rope's pitch circle" (:909), and the same 18 mm
/// is threaded into `CapstanUnit` as "the single source (not hand-matched)"
/// (:477) and into `CarriageIdlers.ab_split` (:830). Nothing in the repo
/// checked the idler end of it before #6135.
const DRIVE_ENTITY: &str = "DriveTendons";


/// Relative tolerance for a cell against a closed form recomputed from the
/// file's own cells.
///
/// Every comparison in this module is ONE algebraic identity evaluated two
/// ways over the same literals, so the two sides differ only by float
/// association order — but `==` would sit at zero margin, where a single
/// reassociation inside the evaluator (or one extra `mm`→m conversion on
/// either side) reds a design that never moved. 1e-9 is ~7 orders above that
/// noise and ~7 orders below the smallest signal any of these gates has to
/// catch (step 3's is a 1.0 % relative miss), so it separates the two cleanly
/// rather than splitting the difference.
const SCALAR_REL_TOL: f64 = 1e-9;

/// Cells of printer.ri whose `volume()` call cannot resolve without a kernel —
/// measured, and the direct analogue of [`super::capstan_groove_e2e`]'s
/// `VOLUME_UNRESOLVED_CELLS`.
///
/// Identities rather than a count, for the reason that module records: a bare
/// `len() == 6` pin is satisfied by any six, so an edit that drops one and adds
/// an unrelated `volume()` cell elsewhere would keep this green while the
/// failure message went on naming these. Held in BOTH directions — see
/// [`load_checked`] — because `volume()` is a geometry consumer and its
/// unresolvability here is a permanent property of the kernel-free surface, not
/// a bug that might get fixed.
const PRINTER_VOLUME_UNRESOLVED: &[&str] = &[
    "AFrame.vol_body",
    "ToolDock.pen_web_today",
    "ToolDock.pen_x_lintel",
    "ToolDock.pen_x_parked",
    "ToolDock.pen_y_lintel",
    "ToolDock.pen_y_parked",
];

/// Cells of printer.ri carrying a qualified enum-variant path the compiler
/// cannot resolve (`Finish.Satin`, `ElementOrder.P2`, `ShellForce.Off`) — a
/// pre-existing gap on main, measured at 11 `UnresolvedName` Errors across
/// these eight cells, and observed by nothing else in the repo.
///
/// A CEILING, not an expectation: these cells MAY raise it and [`load_checked`]
/// tolerates them, but none is REQUIRED to, so fixing the underlying gap makes
/// this module greener rather than redder. That is the one place this module
/// deliberately departs from the capstan gate's exact-identity discipline, and
/// the reason is in the header: population 1 is a permanent property of the
/// surface, this one is a bug. None of the eight is an `IdlerPulley` or
/// `DriveTendons` cell, so none touches this gate's subject.
const PRINTER_ENUM_PATH_UNRESOLVED: &[&str] = &[
    "AFrame.mc",
    "AFrame.opts_field",
    "AFrame.r_pil",
    "CFRP_Rolled_Tube.appearance",
    "GantryFea.opts_cant",
    "GantryFea.opts_ss",
    "GantryFea.r_static",
    "HomogenisedPanel.appearance",
];

/// Evaluate and constraint-check a design file with NO kernel, asserting the
/// pipeline raised no Error outside the two measured allowlists.
///
/// Path- and allowlist-parameterised rather than written against printer.ri,
/// because the lockstep gate needs the identical surface over dev_capstan.ri
/// and the two files' allowlists differ. Each caller memoises its own result;
/// see [`printer_checked`].
///
/// `DiagnosticCode::ConstraintViolated` is routed out for exactly the reason
/// [`super::capstan_groove_e2e`]'s `Strictness` records: a violated constraint
/// is a DESIGN failure, not an evaluation one, and left in this filter it
/// panics the shared fixture first — in every test at once — under a message
/// about evaluation that is false for that failure. The satisfaction gates own
/// it and can say WHICH relation broke.
fn load_checked(
    path: &'static str,
    module: &'static str,
    volume_unresolved: &'static [&'static str],
    enum_path_unresolved: &'static [&'static str],
) -> CheckResult {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));

    let parsed = reify_syntax::parse(&source, ModulePath::single(module));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {path}: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_stdlib_checked(
        &parsed,
        &reify_constraints::SimpleConstraintChecker,
    );

    // WHICH cell a diagnostic is about, structurally. Nothing below reads a
    // message: the prose belongs to another crate, and a rewording of it must
    // not move a diagnostic between these arms.
    //
    // Two resolutions are needed because the two populations label differently.
    // A cell's own span, for the `EvalUnresolved` arm, exactly as the capstan
    // gate resolves it. And the smallest CONTAINING cell, for the compile-stage
    // `UnresolvedName` arm, whose label sits on an expression inside a cell
    // rather than on the cell itself. Both are computed against this same
    // compilation, so neither hard-codes a byte offset and an edit to the
    // file's own `IdlerPulley` cannot shift them.
    let spanned_cells: Vec<(SourceSpan, String)> = compiled
        .templates
        .iter()
        .flat_map(|t| t.value_cells.iter())
        .map(|c| (c.span, format!("{}.{}", c.id.entity, c.id.member)))
        .collect();
    let exact: HashMap<SourceSpan, &String> =
        spanned_cells.iter().map(|(sp, n)| (*sp, n)).collect();
    let labelled_cell = |d: &Diagnostic| -> Option<String> {
        exact.get(&d.labels.first()?.span).map(|n| (*n).clone())
    };
    let containing_cell = |d: &Diagnostic| -> Option<String> {
        let sp = d.labels.first()?.span;
        spanned_cells
            .iter()
            .filter(|(cs, _)| cs.start <= sp.start && sp.end <= cs.end)
            .min_by_key(|(cs, _)| cs.end - cs.start)
            .map(|(_, n)| n.clone())
    };

    // ---- Compile stage: the enum-path CEILING ----
    let compile_unexpected: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .filter(|d| {
            !(d.code == Some(reify_core::DiagnosticCode::UnresolvedName)
                && containing_cell(d)
                    .is_some_and(|c| enum_path_unresolved.contains(&c.as_str())))
        })
        .collect();
    assert!(
        compile_unexpected.is_empty(),
        "unexpected COMPILE errors in {path}. Only an `UnresolvedName` inside a \
         cell named in this file's enum-path allowlist is tolerated (a measured, \
         pre-existing gap: the enum-name scope is built from the module's own \
         `enum_defs`, and this file imports nothing). Anything else here is a \
         real compile regression: {compile_unexpected:#?}"
    );

    // ---- Check stage ----
    let mut engine =
        reify_eval::Engine::new(Box::new(reify_constraints::SimpleConstraintChecker), None);
    let result = engine.check(&compiled);

    let (volume_errors, rest): (Vec<_>, Vec<_>) = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .partition(|d| {
            d.code == Some(reify_core::DiagnosticCode::EvalUnresolved)
                && labelled_cell(d).is_some_and(|c| volume_unresolved.contains(&c.as_str()))
        });
    let unexpected: Vec<_> = rest
        .into_iter()
        .filter(|d| d.code != Some(reify_core::DiagnosticCode::ConstraintViolated))
        .collect();
    assert!(
        unexpected.is_empty(),
        "unexpected evaluation errors on the kernel-free surface of {path}: only \
         the `volume()` geometry-consumer cells in this file's allowlist may fail \
         to resolve here (constraint violations go to the satisfaction gates). An \
         `EvalUnresolved` on any OTHER cell lands here by design: either a \
         `volume()` cell was added and the allowlist needs moving with it, or a \
         cell of the design stopped evaluating — and with no kernel this is the \
         only place the latter is caught: {unexpected:#?}"
    );

    // And every listed cell really did raise one — identities, not a count. The
    // partition admits only cells already in the list, so this half catches a
    // MISSING one; an EXTRA is caught by `unexpected` above.
    let mut got: Vec<String> = volume_errors
        .iter()
        .map(|d| labelled_cell(d).expect("partitioned on the label resolving to a value cell"))
        .collect();
    got.sort();
    let mut want: Vec<String> = volume_unresolved.iter().map(|s| (*s).to_string()).collect();
    want.sort();
    assert_eq!(
        got, want,
        "{path} must raise exactly one `EvalUnresolved` per allowlisted \
         `volume()` cell on the kernel-free surface. A MISSING entry means that \
         cell was dropped or renamed, and the allowlist is now claiming coverage \
         it does not have. Raw diagnostics: {volume_errors:#?}"
    );

    result
}

/// The shared kernel-free surface of printer.ri — parsed, compiled and checked
/// ONCE for every gate in this module.
///
/// One `OnceLock` rather than a load per test for the reason the capstan gate
/// gives: the gates compare cells ACROSS entities (`IdlerPulley` against
/// `DriveTendons`), and two separate compilations would make them agree only by
/// assuming the compiler is deterministic — an assumption none of them states.
/// The cost side is the ordinary saving: printer.ri is 3635 lines and is read,
/// parsed and stdlib-compiled once.
fn printer_checked() -> &'static CheckResult {
    static M: OnceLock<CheckResult> = OnceLock::new();
    M.get_or_init(|| {
        load_checked(
            PRINTER_RI,
            "printer",
            PRINTER_VOLUME_UNRESOLVED,
            PRINTER_ENUM_PATH_UNRESOLVED,
        )
    })
}

/// Read a `Value::Scalar` cell of `entity` out of a value map, asserting its
/// dimension, and return its SI value (metres for a `Length`).
///
/// Values in the map are SI METRES; every failure message in this module
/// formats them in mm, the unit the design file is written in.
///
/// The panic names the cell AND the file on purpose: a contract this module
/// states but the design does not declare must read as "this file does not
/// declare this cell", which is exactly what a RED step here means.
fn entity_cell(values: &ValueMap, entity: &str, cell: &str, expected_dim: DimensionVector) -> f64 {
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
             got {other:?} — is the cell declared in {PRINTER_RI}?"
        ),
    }
}

/// [`entity_cell`] fixed to [`IDLER_ENTITY`] on printer.ri's surface — the
/// majority of this module's reads.
///
/// printer.ri instantiates `IdlerPulley` 31 times, but task 4147 drops
/// parameter overrides through a `sub`, so the BARE TEMPLATE is the form to
/// read and every instance carries these same numbers.
fn idler_cell(cell: &str, expected_dim: DimensionVector) -> f64 {
    entity_cell(&printer_checked().values, IDLER_ENTITY, cell, expected_dim)
}

/// Read a dimensionless (`: Real`) cell of [`IDLER_ENTITY`] — the seat arc
/// ratio is the only one.
///
/// Separate from [`idler_cell`] because the evaluator does NOT wrap a
/// dimensionless quantity in `Value::Scalar { dimension: DIMENSIONLESS }`: a
/// `: Real` cell comes back as a bare `Value::Real`. Both spellings are
/// accepted — they denote the same mathematical object — but a `Value::Scalar`
/// carrying any real dimension is rejected, since that would mean the ratio had
/// silently acquired units.
fn idler_real(cell: &str) -> f64 {
    let id = ValueCellId::new(IDLER_ENTITY, cell);
    match printer_checked().values.get(&id) {
        Some(Value::Real(v)) => *v,
        Some(Value::Scalar {
            si_value,
            dimension,
        }) if *dimension == DimensionVector::DIMENSIONLESS => *si_value,
        other => panic!(
            "{IDLER_ENTITY}.{cell} must be a dimensionless real (a ratio), i.e. a \
             `Value::Real` or a DIMENSIONLESS `Value::Scalar`, got {other:?} — is \
             the cell declared in {PRINTER_RI}, and is it still `: Real`?"
        ),
    }
}

/// Relative error of `actual` against a non-zero `expected`.
fn rel_err(actual: f64, expected: f64) -> f64 {
    (actual - expected).abs() / expected.abs()
}

// ── DIN 15061: the seat arc is OVERSIZE, not a slip fit ──────────────────────

/// The idler's rope seat must be cut on a DIN 15061 OVERSIZE arc
/// (`r = 0.53·d`) rather than on the tendon's own radius, which is the
/// zero-clearance slip fit both copies carried before #6135.
///
/// Two claims, and neither mentions the compensation — that is step 3's
/// subject, and this gate stays true whether or not the arc centre ever moves:
///   1. **DIN conformance** — the design's `seat_arc_ratio` IS the standard's
///      ratio;
///   2. **the arc is derived from it** — `groove_r == tendon_dia ·
///      seat_arc_ratio`, so the ratio is load-bearing rather than a decorative
///      cell the geometry ignores.
///
/// Claim (1) references [`super::capstan_groove_e2e::DIN_15061_SEAT_RATIO`] and
/// NOT this file's own `seat_arc_ratio`, because an external standard's number
/// is the reference: reading the design's cell back and comparing it to itself
/// would assert only that the file equals itself. That constant's own doc
/// carries the full measured account of what each check does and does not catch,
/// and of why the ratio is pinned rather than one side of it — this gate links
/// there rather than restating it.
///
/// Scalar arithmetic over three cells, so it runs on the kernel-free surface and
/// carries NO `OCCT_AVAILABLE` guard: a stub-degraded OCCT is silent in this
/// repo (CLAUDE.md "Native deps"; #6343), and this is the conformance pin that
/// must not skip alongside the mesh gate.
#[test]
fn idler_seat_arc_is_din_15061_oversize() {
    let seat_arc_ratio = idler_real("seat_arc_ratio");
    let tendon_dia = idler_cell("tendon_dia", DimensionVector::LENGTH);
    let groove_r = idler_cell("groove_r", DimensionVector::LENGTH);

    // ---- (1) The ratio IS the standard's ----
    // Exact equality: both sides are the same decimal literal `0.53`, hence the
    // same double. Nothing is computed on either side, so there is no
    // association order for a tolerance to absorb.
    assert_eq!(
        seat_arc_ratio,
        super::capstan_groove_e2e::DIN_15061_SEAT_RATIO,
        "{IDLER_ENTITY}.seat_arc_ratio must be DIN 15061's oversize seat-arc \
         ratio {}, but {PRINTER_RI} declares {seat_arc_ratio}. 0.5 is the \
         zero-clearance slip fit this change exists to remove — under it the \
         seat arc is the tendon's own radius and a load-ovalised braid wedges \
         against the seat walls.",
        super::capstan_groove_e2e::DIN_15061_SEAT_RATIO,
    );

    // ---- (2) The arc really is derived from it ----
    let want = tendon_dia * seat_arc_ratio;
    let err = rel_err(groove_r, want);
    assert!(
        err <= SCALAR_REL_TOL,
        "{IDLER_ENTITY}.groove_r must be the seat ARC radius tendon_dia · \
         seat_arc_ratio = {:.6} mm, but the design reads {:.6} mm (rel err \
         {err:.3e}, tol {SCALAR_REL_TOL:.0e}). Without this the ratio is a cell \
         the geometry ignores: the seat could stay on the tendon radius {:.6} mm \
         while `seat_arc_ratio` sat in the file claiming otherwise.",
        want * 1e3,
        groove_r * 1e3,
        tendon_dia * 0.5e3,
    );
}

// ── The compensation: the SEATED ROPE stays on the pitch circle ──────────────

/// Oversizing the seat arc must not have moved the rope off the sheave's pitch
/// circle — the invariant 31 hand-derived placements in printer.ri rest on.
///
/// This is the task's one real engineering claim. An oversize arc cut on the rim
/// sinks the rope into it by `groove_r - tendon_dia/2`; the arc centre therefore
/// has to sit that far OUTBOARD of the rim for the SEATED rope to come back to
/// `sheave_r`. Three assertions, each one algebraic identity evaluated two ways:
///
///   1. **the design's own arc-centre derivation is this module's** —
///      `seat_c` equals [`super::capstan_groove_e2e::seat_arc_centre`]
///      recomputed from three INDEPENDENT cells. The recomputation is the point:
///      reading a fourth design cell back and comparing design-to-design would
///      move in lockstep with any edit and assert nothing. Shared with the
///      Capstan gate because the offset is construction-independent — that
///      function's doc carries the derivation.
///   2. **the seated rope's centreline is still the pitch circle** —
///      `seat_c - groove_r + tendon_dia/2` equals `sheave_od/2` AND equals
///      [`DRIVE_ENTITY`]'s `r_pitch`. The load-bearing one. The second equality
///      is the CROSS-STRUCTURE single source: printer.ri hand-derives 31 idler
///      positions from 18 mm, and nothing checked it before #6135. Note it is
///      the ROPE's centreline that lands there, not the arc centre — the arc
///      centre is deliberately outboard at 18.180 mm.
///   3. **the seat bottom has not moved at all** — `seat_c - groove_r` equals
///      `sheave_r - tendon_dia/2`, i.e. 15.000 mm, bit-identical to the
///      pre-#6135 seat. This is what keeps the bore clearance (15 > brg_r 11)
///      and the flange height untouched, so the change is provably invisible
///      outside the seat.
///
/// (2) and (3) are both independent of `seat_arc_ratio`: substituting (1) into
/// either cancels `groove_r` entirely. That is why the compensation can be
/// asserted as an exact identity rather than as a band — and why a future ratio
/// bump cannot silently break either.
///
/// **Which of these can actually fail, measured rather than assumed.** That
/// same cancellation means two of the four comparisons are algebraic
/// CONSEQUENCES of (1) and cannot fail while it holds: the `sheave_od/2` half of
/// (2), because `sheave_r` is itself `let sheave_r = sheave_od / 2`, and (3)
/// entire. They are kept for two reasons that are not coverage — they state the
/// identities the structure doc claims, and (3) is the reference the mesh gate
/// reads the seat bottom against — but this gate does not pretend they are
/// independent checks. The capstan gate reached the same conclusion about its
/// own copies of these two and retired them; see
/// `capstan_seat_arc_is_din_15061_oversize`'s claim (2).
///
/// The two that DO carry coverage were each confirmed to fire, on this branch,
/// against a real tree state:
///   * (1), against step 2's uncompensated tree (`let seat_c = sheave_r`):
///     18.180000 mm required against 18.000000 declared, rel err 9.901e-3 — the
///     0.180 mm sink, nine orders above `SCALAR_REL_TOL`.
///   * (2)'s `r_pitch` half, against a tree with (1) PASSING and `sheave_od`
///     moved to 40 mm: 20.000000 mm against `DriveTendons.r_pitch` = 18.000000,
///     a 2.000000 mm miss. That is the measurement showing this half is genuinely
///     independent of (1) — a self-consistent seat on the wrong circle satisfies
///     (1), (2)'s first half and (3), and is caught here alone.
///
/// Kernel-free, so it cannot skip on a machine without OCCT.
#[test]
fn idler_seat_keeps_the_rope_on_the_pitch_circle() {
    let sheave_r = idler_cell("sheave_r", DimensionVector::LENGTH);
    let sheave_od = idler_cell("sheave_od", DimensionVector::LENGTH);
    let groove_r = idler_cell("groove_r", DimensionVector::LENGTH);
    let tendon_dia = idler_cell("tendon_dia", DimensionVector::LENGTH);
    let seat_c = idler_cell("seat_c", DimensionVector::LENGTH);

    // ---- (1) The design's arc centre IS the standard offset ----
    let want_c = super::capstan_groove_e2e::seat_arc_centre(sheave_r, groove_r, tendon_dia);
    let err = rel_err(seat_c, want_c);
    assert!(
        err <= SCALAR_REL_TOL,
        "{IDLER_ENTITY}.seat_c must be the seat arc's CENTRE radius, pushed \
         outboard of the rim by exactly how far the oversize arc would otherwise \
         sink the rope: sheave_r + groove_r - tendon_dia/2 = {:.6} mm. The design \
         reads {:.6} mm (rel err {err:.3e}, tol {SCALAR_REL_TOL:.0e}). Recomputed \
         from sheave_r = {:.6}, groove_r = {:.6} and tendon_dia = {:.6} mm rather \
         than read back off a fourth cell, so an edit to either side alone lands \
         here.",
        want_c * 1e3,
        seat_c * 1e3,
        sheave_r * 1e3,
        groove_r * 1e3,
        tendon_dia * 1e3,
    );

    // ---- (2) The SEATED rope's centreline is still the pitch circle ----
    // Bottomed out in its seat, the rope's underside rests at `seat_c - groove_r`
    // and its centreline sits one rope radius above that.
    let seated_centreline = seat_c - groove_r + tendon_dia / 2.0;
    let err_od = rel_err(seated_centreline, sheave_od / 2.0);
    assert!(
        err_od <= SCALAR_REL_TOL,
        "the SEATED rope's centreline must lie on the rim, sheave_od/2 = {:.6} \
         mm, but it sits at {:.6} mm (rel err {err_od:.3e}, tol \
         {SCALAR_REL_TOL:.0e}) — off by {:.6} mm. printer.ri declares sheave_od \
         as \"rim (outer) diameter == rope pitch circle\", so this equality is \
         what makes that comment true. It is the ROPE's centreline that belongs \
         here, NOT the seat arc's centre (seat_c = {:.6} mm, deliberately \
         outboard).",
        sheave_od * 0.5e3,
        seated_centreline * 1e3,
        (seated_centreline - sheave_od / 2.0) * 1e3,
        seat_c * 1e3,
    );

    let r_pitch = entity_cell(
        &printer_checked().values,
        DRIVE_ENTITY,
        "r_pitch",
        DimensionVector::LENGTH,
    );
    let err_pitch = rel_err(seated_centreline, r_pitch);
    assert!(
        err_pitch <= SCALAR_REL_TOL,
        "the SEATED rope's centreline must equal {DRIVE_ENTITY}.r_pitch = {:.6} \
         mm — printer.ri's cross-structure single source — but it sits at {:.6} \
         mm (rel err {err_pitch:.3e}, tol {SCALAR_REL_TOL:.0e}), off by {:.6} mm. \
         THIS IS THE INVARIANT 31 HAND-DERIVED PLACEMENTS REST ON, and a miss \
         here is not cosmetic: printer.ri states that \"every tendon centreline \
         is tangent to its rope's pitch circle\" (:909), threads the same 18 mm \
         into CapstanUnit as \"the single source (not hand-matched)\" (:477) and \
         into CarriageIdlers.ab_split (:830). Every one of those positions is \
         hand-derived from this number, so moving the rope without moving them \
         silently falsifies all of them — and before #6135 nothing in the repo \
         checked it.",
        r_pitch * 1e3,
        seated_centreline * 1e3,
        (seated_centreline - r_pitch) * 1e3,
    );

    // ---- (3) The seat bottom is bit-identical to the pre-#6135 seat ----
    let seat_bottom = seat_c - groove_r;
    let want_bottom = sheave_r - tendon_dia / 2.0;
    let err_bottom = rel_err(seat_bottom, want_bottom);
    assert!(
        err_bottom <= SCALAR_REL_TOL,
        "the seat BOTTOM must be sheave_r - tendon_dia/2 = {:.6} mm — where the \
         pre-#6135 conformal seat put it — but it sits at {:.6} mm (rel err \
         {err_bottom:.3e}, tol {SCALAR_REL_TOL:.0e}), off by {:.6} mm. This \
         reference does not mention groove_r at all, so it holds for any seat arc \
         radius. It is what keeps the oversize arc invisible outside the seat: \
         the bore clearance below it (brg_r = {:.6} mm) and the flange height \
         above it are both untouched only while this holds.",
        want_bottom * 1e3,
        seat_bottom * 1e3,
        (seat_bottom - want_bottom) * 1e3,
        idler_cell("brg_r", DimensionVector::LENGTH) * 1e3,
    );
}
