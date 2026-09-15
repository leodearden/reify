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

use reify_core::{
    ConstraintNodeId, Diagnostic, DimensionVector, ModulePath, Severity, SourceSpan, ValueCellId,
};
use reify_eval::{CheckResult, ConstraintCheckEntry};
use reify_ir::{Satisfaction, Value, ValueMap};
use std::collections::BTreeSet;
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
fn compile_design(
    path: &'static str,
    module: &'static str,
    enum_path_unresolved: &'static [&'static str],
) -> reify_compiler::CompiledModule {
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
    // not move a diagnostic between the arms here and in [`check_design`].
    //
    // The two populations label differently, so each stage resolves the identity
    // its own way. This one wants the smallest CONTAINING cell, because an
    // `UnresolvedName` label sits on an expression INSIDE a cell rather than on
    // the cell itself; [`check_design`] wants the cell's own span, exactly as the
    // capstan gate resolves it. Both are computed against this same compilation,
    // so neither hard-codes a byte offset and an edit to the file's own
    // `IdlerPulley` cannot shift them.
    let spanned_cells: Vec<(SourceSpan, String)> = compiled
        .templates
        .iter()
        .flat_map(|t| t.value_cells.iter())
        .map(|c| (c.span, format!("{}.{}", c.id.entity, c.id.member)))
        .collect();
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

    compiled
}

/// Evaluate and constraint-check an already-compiled design with NO kernel,
/// asserting the check stage raised no Error outside `volume_unresolved`.
///
/// Takes the compilation rather than making its own, so a gate that reads
/// compiled constraint EXPRESSIONS and a gate that reads evaluated CELLS are
/// looking at the same module — see [`printer_compiled`].
///
/// `DiagnosticCode::ConstraintViolated` is routed out for exactly the reason
/// [`super::capstan_groove_e2e`]'s `Strictness` records: a violated constraint
/// is a DESIGN failure, not an evaluation one, and left in this filter it panics
/// the shared fixture first — in every test at once — under a message about
/// evaluation that is false for that failure. The satisfaction gates own it and
/// can say WHICH relation broke.
fn check_design(
    compiled: &reify_compiler::CompiledModule,
    path: &'static str,
    volume_unresolved: &'static [&'static str],
) -> CheckResult {
    let exact: HashMap<SourceSpan, String> = compiled
        .templates
        .iter()
        .flat_map(|t| t.value_cells.iter())
        .map(|c| (c.span, format!("{}.{}", c.id.entity, c.id.member)))
        .collect();
    let labelled_cell = |d: &Diagnostic| -> Option<String> {
        exact.get(&d.labels.first()?.span).cloned()
    };

    let mut engine =
        reify_eval::Engine::new(Box::new(reify_constraints::SimpleConstraintChecker), None);
    let result = engine.check(compiled);

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
    M.get_or_init(|| check_design(printer_compiled(), PRINTER_RI, PRINTER_VOLUME_UNRESOLVED))
}

/// The shared COMPILATION of printer.ri — the surface a gate reads constraint
/// expressions off, as distinct from the evaluated cells [`printer_checked`]
/// carries.
///
/// One `OnceLock` so the constraint expressions a gate inspects and the cell
/// values it compares them against come from the SAME module. Compiling twice
/// would make them agree only by assuming the compiler is deterministic.
fn printer_compiled() -> &'static reify_compiler::CompiledModule {
    static M: OnceLock<reify_compiler::CompiledModule> = OnceLock::new();
    M.get_or_init(|| compile_design(PRINTER_RI, "printer", PRINTER_ENUM_PATH_UNRESOLVED))
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
fn entity_cell(
    values: &ValueMap,
    file: &str,
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
             got {other:?} — is the cell declared in {file}?"
        ),
    }
}

/// Read a dimensionless (`: Real`) cell of `entity` out of a value map.
///
/// Separate from [`entity_cell`] because the evaluator does NOT wrap a
/// dimensionless quantity in `Value::Scalar { dimension: DIMENSIONLESS }`: a
/// `: Real` cell comes back as a bare `Value::Real`. Both spellings are accepted
/// — they denote the same mathematical object — but a `Value::Scalar` carrying
/// any real dimension is rejected, since that would mean the ratio had silently
/// acquired units.
fn entity_real(values: &ValueMap, file: &str, entity: &str, cell: &str) -> f64 {
    match values.get(&ValueCellId::new(entity, cell)) {
        Some(Value::Real(v)) => *v,
        Some(Value::Scalar {
            si_value,
            dimension,
        }) if *dimension == DimensionVector::DIMENSIONLESS => *si_value,
        other => panic!(
            "{entity}.{cell} must be a dimensionless real (a ratio), i.e. a \
             `Value::Real` or a DIMENSIONLESS `Value::Scalar`, got {other:?} — is \
             the cell declared in {file}, and is it still `: Real`?"
        ),
    }
}

/// Read one [`IDLER_CELLS`] entry off a given file's value map — `None`
/// dimension marks the `: Real` cell, which needs [`entity_real`].
fn idler_cell_of(values: &ValueMap, file: &str, cell: &str, dim: Option<DimensionVector>) -> f64 {
    match dim {
        Some(d) => entity_cell(values, file, IDLER_ENTITY, cell, d),
        None => entity_real(values, file, IDLER_ENTITY, cell),
    }
}

/// [`entity_cell`] fixed to [`IDLER_ENTITY`] on printer.ri's surface — the
/// majority of this module's reads.
///
/// printer.ri instantiates `IdlerPulley` 31 times, but task 4147 drops
/// parameter overrides through a `sub`, so the BARE TEMPLATE is the form to
/// read and every instance carries these same numbers.
fn idler_cell(cell: &str, expected_dim: DimensionVector) -> f64 {
    entity_cell(
        &printer_checked().values,
        PRINTER_RI,
        IDLER_ENTITY,
        cell,
        expected_dim,
    )
}

/// [`entity_real`] fixed to [`IDLER_ENTITY`] on printer.ri's surface.
fn idler_real(cell: &str) -> f64 {
    entity_real(&printer_checked().values, PRINTER_RI, IDLER_ENTITY, cell)
}

/// The compiled `IdlerPulley` template out of one design file's module.
///
/// The gates that reach past evaluated VALUES into the compiled tree all start
/// here: the constraint read-sets, the `body` read-set, and the cross-file
/// content-hash equality. `module` and `file` travel together, as
/// [`entity_cell`] takes them, because BOTH files declare a structure of this
/// name and the pair is what tells a failure which one it is about.
fn idler_template<'m>(
    module: &'m reify_compiler::CompiledModule,
    file: &str,
) -> &'m reify_compiler::TopologyTemplate {
    module
        .templates
        .iter()
        .find(|t| t.name == IDLER_ENTITY)
        .unwrap_or_else(|| {
            panic!(
                "{file} must declare the `{IDLER_ENTITY}` structure; templates \
                 compiled: {:?}",
                module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        })
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

/// Every `IdlerPulley` cell that printer.ri's `body` expression READS, as
/// measured off the compiled tree.
///
/// This is the inventory [`IDLER_CELLS`] cannot supply. That one asks what the
/// structure COMPUTES and compares numbers; this one asks what the geometry is
/// BUILT FROM, which no number can answer — `seat_c` holds its value whether or
/// not a single `torus` reads it.
///
/// Held as an EXACT set, both directions. A member going missing is the defect
/// this task exists to remove; a member appearing means the body tree grew and
/// this gate wants a conscious update rather than a widened assertion.
///
/// The compiled reads are all intra-structure, so filtering to [`IDLER_ENTITY`]
/// drops nothing today — measured identical filtered and unfiltered. The filter
/// stays because a future `body` that reaches into a sub-component should not
/// silently enlarge this set.
const IDLER_BODY_READS: &[&str] = &[
    "bore_len",
    "brg_bore",
    "brg_r",
    "brg_width",
    "groove_r",
    "seat_c",
    "sheave_r",
    "sheave_w",
];

// ── The compensation: the SEATED ROPE stays on the pitch circle ──────────────

/// Oversizing the seat arc must not have moved the rope off the sheave's pitch
/// circle — the invariant 31 hand-derived placements in printer.ri rest on.
///
/// This is the task's one real engineering claim. An oversize arc cut on the rim
/// sinks the rope into it by `groove_r - tendon_dia/2`; the arc centre therefore
/// has to sit that far OUTBOARD of the rim for the SEATED rope to come back to
/// `sheave_r`. Four assertions. The first three are each one algebraic identity
/// evaluated two ways; the fourth is structural, and pins the geometry tree the
/// other three are blind to:
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
///   4. **the solid is actually BUILT from that arc centre** — the compiled
///      `body` expression's read-set is exactly [`IDLER_BODY_READS`]. (1)-(3)
///      compare VALUES, and a cell holds its value whether or not any geometry
///      reads it; this is the only one of the four that can tell the difference
///      between a design that computes the right arc centre and one that cuts
///      the seat with it.
///
/// (2) and (3) are both independent of `seat_arc_ratio`: substituting (1) into
/// either cancels `groove_r` entirely. That is why the compensation can be
/// asserted as an exact identity rather than as a band — and why a future ratio
/// bump cannot silently break either.
///
/// **Which of these can actually fail, measured rather than assumed.** That
/// same cancellation means two of (1)-(3)'s four scalar comparisons are
/// algebraic CONSEQUENCES of (1) and cannot fail while it holds: the
/// `sheave_od/2` half of (2), because `sheave_r` is itself
/// `let sheave_r = sheave_od / 2`, and (3) entire. They are kept for two reasons that are not coverage — they state the
/// identities the structure doc claims, and (3) is the reference the mesh gate
/// reads the seat bottom against — but this gate does not pretend they are
/// independent checks. The capstan gate reached the same conclusion about its
/// own copies of these two and retired them; see
/// `capstan_seat_arc_is_din_15061_oversize`'s claim (2).
///
/// The three that DO carry coverage were each confirmed to fire, on this
/// branch, against a real tree state:
///   * (1), against step 2's uncompensated tree (`let seat_c = sheave_r`):
///     18.180000 mm required against 18.000000 declared, rel err 9.901e-3 — the
///     0.180 mm sink, nine orders above `SCALAR_REL_TOL`.
///   * (2)'s `r_pitch` half, against a tree with (1) PASSING and `sheave_od`
///     moved to 40 mm: 20.000000 mm against `DriveTendons.r_pitch` = 18.000000,
///     a 2.000000 mm miss. That is the measurement showing this half is genuinely
///     independent of (1) — a self-consistent seat on the wrong circle satisfies
///     (1), (2)'s first half and (3), and is caught here alone.
///   * (4), against printer.ri:238 reverted to `torus(sheave_r, groove_r)`: the
///     read-set loses `seat_c`. That mutation is the exact uncompensated seat
///     this task removed, in the production file all 31 placements come from,
///     and it left ALL FIVE of this module's gates green — measured, which is
///     how the gap was found. (4) is the assertion that reds it; the sibling
///     half, for a mutation applied to BOTH copies at once, is the template
///     content-hash equality in `idler_copies_stay_in_lockstep`.
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
        PRINTER_RI,
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

    // ---- (4) …and the SOLID is built from that arc centre, not merely near it ----
    // (1)-(3) are arithmetic over evaluated cells, and a cell keeps its value
    // whether or not the geometry reads it. Reverting `torus(seat_c, groove_r)`
    // to `torus(sheave_r, groove_r)` — the uncompensated seat this task removed —
    // leaves every one of them green, measured. So pin the body's READ-SET.
    let body = idler_template(printer_compiled(), PRINTER_RI)
        .value_cells
        .iter()
        .find(|c| c.id.member == "body")
        .unwrap_or_else(|| {
            panic!(
                "{IDLER_ENTITY} must declare a `body` cell in {PRINTER_RI} — it is \
                 the one handle the whole solid hangs off, and without it there is \
                 no geometry for this gate to pin."
            )
        });
    let body_expr = body.default_expr.as_ref().unwrap_or_else(|| {
        panic!(
            "{IDLER_ENTITY}.body must carry a compiled default expression in \
             {PRINTER_RI}; a `body` declared with no expression builds no solid."
        )
    });
    let reads: BTreeSet<String> = body_expr
        .collect_value_refs()
        .into_iter()
        .filter(|id| id.entity == IDLER_ENTITY)
        .map(|id| id.member)
        .collect();
    let want: BTreeSet<String> = IDLER_BODY_READS.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        reads, want,
        "{IDLER_ENTITY}.body in {PRINTER_RI} must be BUILT FROM exactly \
         {IDLER_BODY_READS:?}, but it reads {reads:?}.\n\
         `seat_c` and `groove_r` are the load-bearing two. A MISSING `seat_c` \
         means the seat arc is being cut at some other radius — almost certainly \
         `sheave_r`, the uncompensated seat #6135 removed — which sinks the \
         seated rope 0.180 mm below the pitch circle at all 31 hand-derived \
         placements while (1)-(3) above stay green, because `seat_c` keeps its \
         value no matter who reads it. A MISSING `groove_r` means the arc is no \
         longer sized by the DIN ratio. An ADDED member is not a defect but is \
         not automatically fine either: the body tree grew, and this gate wants a \
         deliberate update here rather than a widened assertion."
    );
}

/// Fractional clearance DIN 15061's ratio buys at THIS seat's rim opening,
/// derived from the standard's ratio ALONE: `sqrt(4·ratio − 1) − 1` = 5.83 %.
///
/// Derived from the standard rather than from the file's own `groove_r` on
/// purpose: a floor parametrized by the design moves with it and a slip-fit
/// revert would take the floor down with the mouth, leaving the gate green.
///
/// NOT [`super::capstan_groove_e2e`]'s `MIN_MOUTH_CLEARANCE_FRAC`, which is
/// `2·ratio − 1` = 6 %. That is the Capstan's land-at-arc-centre case, where the
/// land is cut back ONTO the arc centre so the mouth is the section's full width
/// `2·groove_r`. Here the rim is pinned at `sheave_r`, one `groove_r −
/// tendon_dia/2` inboard of the arc centre, so the chord is taken off-centre and
/// comes out slightly narrower. Reusing the Capstan's spelling would
/// over-predict this mouth and red a correct design.
///
/// Not a `const` only because `f64::sqrt` is not const-evaluable.
fn min_mouth_clearance_frac() -> f64 {
    (4.0 * super::capstan_groove_e2e::DIN_15061_SEAT_RATIO - 1.0).sqrt() - 1.0
}

/// Assert an entity-scoped `constraint_results` set is non-empty and every entry
/// is `Satisfied`; returns the entries examined.
///
/// The two-step shape is [`super::capstan_groove_e2e`]'s `assert_constraints_ok`,
/// and the first step is the one that silently rots when copied: a satisfaction
/// filter over an EMPTY set is vacuously green, and an empty
/// `constraint_results` emits no diagnostic, so nothing else here could see it.
/// Non-emptiness is therefore asserted BEFORE the filter, never alongside it.
///
/// Strict — `Indeterminate` fails too, not just `Violated`. That is decidable
/// for this entity because every `IdlerPulley` constraint reads pure-scalar cells
/// and none reaches a `volume()` consumer, so an `Indeterminate` here means an
/// input cell stopped evaluating rather than that a kernel was needed. It is
/// also the failure mode a `Violated`-only filter is blindest to: the constraint
/// is still declared, still reported, and checking nothing — and it reaches the
/// diagnostics only as a warning, so no Error filter in this module sees it
/// either.
///
/// Deliberately scoped rather than file-wide: 38 of printer.ri's 406 constraints
/// are not `Satisfied` today (measured, pre-existing — the `volume()` cells'
/// constraints among them), so a strict file-wide claim would be red on arrival
/// and is not this task's business.
fn assert_idler_constraints_ok(entries: &[ConstraintCheckEntry]) -> Vec<&ConstraintCheckEntry> {
    let scoped: Vec<&ConstraintCheckEntry> =
        entries.iter().filter(|c| c.id.entity == IDLER_ENTITY).collect();
    assert!(
        !scoped.is_empty(),
        "no `{IDLER_ENTITY}` constraint results at all on the kernel-free surface \
         of {PRINTER_RI} — the structure declares five, so an empty set means the \
         check never ran or stopped covering this scope, and the satisfaction \
         filter below would then pass vacuously. Entities checked: {:?}",
        entries.iter().map(|c| &c.id.entity).collect::<Vec<_>>()
    );
    let bad: Vec<_> = scoped
        .iter()
        .filter(|c| c.satisfaction != Satisfaction::Satisfied)
        .collect();
    assert!(
        bad.is_empty(),
        "{PRINTER_RI} must satisfy every `{IDLER_ENTITY}` constraint at its \
         defaults — {} of {} did not. `Violated` means the design broke the \
         relation; `Indeterminate` means an input cell failed to EVALUATE, so the \
         constraint is present but checking nothing. Results: {bad:#?}",
        bad.len(),
        scoped.len()
    );
    scoped
}

// ── The seat must clear the tendon, and the design must say so ───────────────

/// The oversize arc must open a seat the rope actually clears, it must not eat
/// the rim shoulders, and printer.ri must STATE both as its own constraints so
/// `reify check` catches a divergence too.
///
/// **One chord, three readings.** Because the compensation puts the rope's
/// centreline exactly ON the rim, the rope's widest section plane IS the rim
/// plane — so the seat's opening at the rim and the gap beside the rope at its
/// widest point are literally the same chord, measured once. That coincidence is
/// a property of step 4's compensation, not of the arc, and it is a further
/// reason that invariant is worth enforcing beyond the 31 call sites.
///
/// Five claims:
///
/// (a) **the design declares the opening**, and `mouth_w` equals the closed form
///     `tendon_dia · sqrt(4·seat_arc_ratio − 1)`. Derivation: the chord of the
///     seat circle (centre `seat_c`, radius `groove_r`) cut by the rim cylinder
///     at `sheave_r` is `2·sqrt(groove_r² − (seat_c − sheave_r)²)`; step 4's
///     `seat_c` makes the offset `seat_c − sheave_r = groove_r − tendon_dia/2`,
///     and substituting collapses the whole thing to the one-term form above,
///     whose `sqrt` argument is DIMENSIONLESS. This is the same algebra #5683's
///     `constraint groove_r > rope_dia / 2` rests on. Both spellings were
///     measured to agree at 6.349803 mm.
///
/// (b) **the opening strictly exceeds the rope**, 6.349803 against 6.000 mm. The
///     floor comes from [`min_mouth_clearance_frac`] — the STANDARD's ratio
///     alone — never from the file's own `groove_r`, so a slip-fit revert reds
///     here. That revert is a live negative control at ZERO margin rather than a
///     guessed threshold: at `seat_arc_ratio = 0.5` the closed form gives
///     `tendon_dia · sqrt(1) = tendon_dia` exactly, so the mouth lands precisely
///     on the rope and both halves of this claim fail at once.
///
/// (c) **the same chord read as anti-pinch clearance** — 0.174902 mm per side at
///     the rope's widest section, which is DIN's mechanical reason for
///     oversizing: a load-ovalised braid cannot wedge against the seat walls.
///
/// (d) **the rim shoulder survives the widened opening** — `sheave_w > mouth_w`,
///     10.000 against 6.349803 mm, leaving 1.825098 mm per side against the
///     nominal `flange_width` of 2 mm. The 0.174902 mm narrowing is ACCEPTED:
///     `sheave_w` is deliberately NOT re-derived from the mouth, which would
///     widen the part to 10.3498 mm across 31 hand-placed instances plus a
///     hand-matched `ShuttlePlate` — an unreviewable ripple bought for 0.175 mm
///     per side, and it would forfeit the change's defining virtue that nothing
///     outside the seat moves.
///
/// (e) **printer.ri states (b) and (d) itself**, pinned by the CELLS each
///     compiled constraint expression reads rather than by a constraint merely
///     being present — otherwise swapping either for any other
///     `IdlerPulley`-scoped constraint leaves this green while the message goes
///     on describing the clearance rule. The two are PARTITIONED and observed
///     separately: both read `mouth_w`, so the discriminator is the other datum
///     (`tendon_dia` for the clearance, `sheave_w` for the shoulder). Note the
///     capstan gate's `sub_cell_reads` `IndexAccess` technique does NOT apply
///     here — these are intra-structure reads, which compile to plain
///     `ValueRef`s that [`reify_ir::CompiledExpr::collect_value_refs`] reports
///     directly.
///
/// Kernel-free throughout, so none of it skips where OCCT is absent.
#[test]
fn idler_seat_clears_the_tendon() {
    let tendon_dia = idler_cell("tendon_dia", DimensionVector::LENGTH);
    let seat_arc_ratio = idler_real("seat_arc_ratio");
    let groove_r = idler_cell("groove_r", DimensionVector::LENGTH);
    let seat_c = idler_cell("seat_c", DimensionVector::LENGTH);
    let sheave_r = idler_cell("sheave_r", DimensionVector::LENGTH);
    let sheave_w = idler_cell("sheave_w", DimensionVector::LENGTH);
    let flange_width = idler_cell("flange_width", DimensionVector::LENGTH);
    let mouth_w = idler_cell("mouth_w", DimensionVector::LENGTH);

    // ---- (a) The declared opening IS the chord the arc opens at the rim ----
    // Both spellings, so the collapsed form the design uses is checked against
    // the general one it was derived from rather than against itself.
    let offset = seat_c - sheave_r;
    let chord = 2.0 * (groove_r * groove_r - offset * offset).max(0.0).sqrt();
    let closed = tendon_dia * (4.0 * seat_arc_ratio - 1.0).sqrt();
    let err_closed = rel_err(chord, closed);
    assert!(
        err_closed <= SCALAR_REL_TOL,
        "the general chord form 2·sqrt(groove_r² − (seat_c − sheave_r)²) = {:.6} \
         mm and the collapsed closed form tendon_dia · sqrt(4·seat_arc_ratio − 1) \
         = {:.6} mm must agree (rel err {err_closed:.3e}) — they are the same \
         expression once seat_c − sheave_r = groove_r − tendon_dia/2 is \
         substituted. If they disagree, step 4's compensation is not what this \
         gate assumes and (b)–(d) below are all measuring the wrong chord.",
        chord * 1e3,
        closed * 1e3,
    );
    let err_mouth = rel_err(mouth_w, closed);
    assert!(
        err_mouth <= SCALAR_REL_TOL,
        "{IDLER_ENTITY}.mouth_w must be the seat's opening at the rim, \
         tendon_dia · sqrt(4·seat_arc_ratio − 1) = {:.6} mm, but the design reads \
         {:.6} mm (rel err {err_mouth:.3e}, tol {SCALAR_REL_TOL:.0e}).",
        closed * 1e3,
        mouth_w * 1e3,
    );

    // ---- (b) The opening strictly clears the rope, by the STANDARD's margin ----
    let floor = tendon_dia * (1.0 + min_mouth_clearance_frac());
    assert!(
        mouth_w > tendon_dia,
        "the seat's opening must STRICTLY exceed the rope it carries: mouth_w = \
         {:.6} mm against tendon_dia = {:.6} mm. At or below it the rope cannot \
         be laid in and a load-ovalised braid wedges — which is exactly what \
         seat_arc_ratio = 0.5 produces, a mouth landing precisely ON the rope.",
        mouth_w * 1e3,
        tendon_dia * 1e3,
    );
    assert!(
        mouth_w >= floor * (1.0 - SCALAR_REL_TOL),
        "the seat's opening must clear the rope by DIN 15061's margin — at least \
         tendon_dia · (1 + sqrt(4·{ratio} − 1) − 1) = {:.6} mm — but it is {:.6} \
         mm, only {:.4} % clear against the required {:.4} %. This floor derives \
         from the STANDARD's ratio alone, not from the file's groove_r, so a \
         slip-fit revert cannot take the floor down with the mouth.",
        floor * 1e3,
        mouth_w * 1e3,
        100.0 * (mouth_w - tendon_dia) / tendon_dia,
        100.0 * min_mouth_clearance_frac(),
        ratio = super::capstan_groove_e2e::DIN_15061_SEAT_RATIO,
    );

    // ---- (c) The same chord, read as anti-pinch clearance per side ----
    let per_side = (mouth_w - tendon_dia) / 2.0;
    assert!(
        per_side > 0.0,
        "the rope must have clearance on BOTH sides at its widest section: \
         (mouth_w − tendon_dia)/2 = {:.6} mm. This is the SAME chord as (b), \
         because step 4's compensation puts the rope's centreline on the rim and \
         so makes the rope's widest section plane the rim plane — one measurement, \
         two readings. It is DIN's mechanical reason for oversizing at all.",
        per_side * 1e3,
    );

    // ---- (d) The oversize arc has not eaten the rim shoulders ----
    assert!(
        sheave_w > mouth_w,
        "the widened opening must leave rim shoulder on both sides: sheave_w = \
         {:.6} mm must exceed mouth_w = {:.6} mm. Shoulder per side is {:.6} mm \
         against a nominal flange_width of {:.6} mm — the {:.6} mm narrowing is \
         accepted, and sheave_w is deliberately NOT re-derived from the mouth \
         (that would widen the part across 31 hand-placed instances for 0.175 mm \
         per side). But at or below equality the shoulders are gone and the seat \
         has broken out of the rim.",
        sheave_w * 1e3,
        mouth_w * 1e3,
        (sheave_w - mouth_w) * 0.5e3,
        flange_width * 1e3,
        (flange_width - (sheave_w - mouth_w) / 2.0) * 1e3,
    );

    // ---- (e) The design states (b) and (d) as its own constraints ----
    // Pinned by the cells each compiled expression READS. A constraint merely
    // being present is not the claim: swapping either for any other
    // `IdlerPulley`-scoped constraint would leave a presence check green.
    let declared: Vec<(&ConstraintNodeId, BTreeSet<String>)> =
        idler_template(printer_compiled(), PRINTER_RI)
            .constraints
            .iter()
            .map(|c| {
                let reads = c
                    .expr
                    .collect_value_refs()
                    .into_iter()
                    .filter(|id| id.entity == IDLER_ENTITY)
                    .map(|id| id.member)
                    .collect();
                (&c.id, reads)
            })
            .collect();

    // Both read `mouth_w`, so the OTHER datum is what tells them apart — which is
    // what makes each individually observable rather than the pair jointly.
    let clearance: Vec<_> = declared
        .iter()
        .filter(|(_, r)| r.contains("mouth_w") && r.contains("tendon_dia"))
        .collect();
    let shoulder: Vec<_> = declared
        .iter()
        .filter(|(_, r)| r.contains("mouth_w") && r.contains("sheave_w"))
        .collect();
    assert_eq!(
        clearance.len(),
        1,
        "{PRINTER_RI} must state the DIN clearance as its OWN constraint, reading \
         `mouth_w` against `tendon_dia`, so `reify check` catches a divergence \
         without this gate — found {} such. That inequality is algebraically \
         exactly `groove_r > tendon_dia / 2`, so anti-pinch and mouth clearance \
         are one statement. Every {IDLER_ENTITY} constraint and the cells it \
         reads: {declared:#?}",
        clearance.len(),
    );
    assert_eq!(
        shoulder.len(),
        1,
        "{PRINTER_RI} must state the rim-shoulder survival as its OWN constraint, \
         reading `sheave_w` against `mouth_w` — found {} such. Without it the \
         shoulder is an unstated consequence, and a future `seat_arc_ratio` bump \
         that really would eat the shoulders passes `reify check`. Every \
         {IDLER_ENTITY} constraint and the cells it reads: {declared:#?}",
        shoulder.len(),
    );
    assert_ne!(
        clearance[0].0, shoulder[0].0,
        "the clearance and shoulder constraints must be two DISTINCT constraints, \
         not one expression matching both filters: {declared:#?}"
    );

    // …and all of them hold, strictly, on this surface.
    assert_idler_constraints_ok(&printer_checked().constraint_results);
}

// ── The second copy, and the fabricated solid ────────────────────────────────

/// The design file carrying the SECOND copy of `IdlerPulley` — a standalone
/// sketch, because v0.1 has no cross-file import.
const DEV_CAPSTAN_RI: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../prj/printer_v01/dev_capstan.ri"
);

/// dev_capstan.ri's own `volume()` consumers — the same two
/// [`super::capstan_groove_e2e`] enumerates for its own surface.
const DEV_CAPSTAN_VOLUME_UNRESOLVED: &[&str] = &["Capstan.blank_volume", "Capstan.body_volume"];

/// EMPTY, and measured so: unlike printer.ri, dev_capstan.ri names no qualified
/// enum variant, so it compiles with zero Error diagnostics. An allowlist that
/// tolerated them here would be tolerating something that does not happen.
const DEV_CAPSTAN_ENUM_PATH_UNRESOLVED: &[&str] = &[];

/// Every scalar cell the `IdlerPulley` structure computes, as
/// `(name, Some(dimension))` — or `None` for the one `: Real` cell.
///
/// **This inventory is the lockstep gate's single source for "what the structure
/// computes".** A cell added to one copy and not the other fails on the READ
/// rather than passing unnoticed, which is the property that keeps the two files
/// from forking again — so it must be populated from the structure, not from
/// whichever cells happened to seem interesting.
///
/// `body` is deliberately ABSENT: it is a geometry handle, not a scalar, so
/// there is no number to compare. The shape it names is held to the design's
/// numbers by `idler_sheave_mesh_has_the_declared_seat` instead.
const IDLER_CELLS: &[(&str, Option<DimensionVector>)] = &[
    ("brg_od", Some(DimensionVector::LENGTH)),
    ("brg_bore", Some(DimensionVector::LENGTH)),
    ("brg_width", Some(DimensionVector::LENGTH)),
    ("sheave_od", Some(DimensionVector::LENGTH)),
    ("tendon_dia", Some(DimensionVector::LENGTH)),
    ("seat_arc_ratio", None),
    ("flange_width", Some(DimensionVector::LENGTH)),
    ("brg_r", Some(DimensionVector::LENGTH)),
    ("sheave_r", Some(DimensionVector::LENGTH)),
    ("groove_r", Some(DimensionVector::LENGTH)),
    ("seat_c", Some(DimensionVector::LENGTH)),
    ("mouth_w", Some(DimensionVector::LENGTH)),
    ("sheave_w", Some(DimensionVector::LENGTH)),
    ("bore_len", Some(DimensionVector::LENGTH)),
];

/// dev_capstan.ri's shared COMPILATION.
fn dev_capstan_compiled() -> &'static reify_compiler::CompiledModule {
    static M: OnceLock<reify_compiler::CompiledModule> = OnceLock::new();
    M.get_or_init(|| {
        // `dev_capstan`, NOT `printer`: both files declare a structure literally
        // named `IdlerPulley` — which is this gate's whole subject — so they must
        // not both claim the same module path.
        compile_design(
            DEV_CAPSTAN_RI,
            "dev_capstan",
            DEV_CAPSTAN_ENUM_PATH_UNRESOLVED,
        )
    })
}

/// dev_capstan.ri's shared kernel-free surface.
fn dev_capstan_checked() -> &'static CheckResult {
    static M: OnceLock<CheckResult> = OnceLock::new();
    M.get_or_init(|| {
        check_design(
            dev_capstan_compiled(),
            DEV_CAPSTAN_RI,
            DEV_CAPSTAN_VOLUME_UNRESOLVED,
        )
    })
}

/// dev_capstan.ri tessellated with a real OCCT kernel, once.
///
/// dev_capstan.ri and not printer.ri: `Engine::tessellate_realizations` takes no
/// entity or scope argument, so tessellating printer.ri would tessellate all 32
/// of its structures — see this module's header for the measurement.
///
/// Constraint VIOLATIONS are routed out of the Error filter for the reason
/// [`super::capstan_groove_e2e`]'s `Strictness` records; it bites harder here,
/// because this surface evaluates and constraint-checks BEFORE it tessellates,
/// so one broken design relation left in the filter would panic here and take the
/// mesh gate down under a message about geometry that is false for that failure.
fn dev_capstan_tessellated() -> &'static reify_eval::TessellateResult {
    static M: OnceLock<reify_eval::TessellateResult> = OnceLock::new();
    M.get_or_init(|| {
        let mut planner = reify_geometry::SingleKernelHolder::new();
        planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
        let mut engine = reify_eval::Engine::new(
            Box::new(reify_constraints::SimpleConstraintChecker),
            Some(Box::new(planner)),
        );
        let result = engine.tessellate_realizations(dev_capstan_compiled());
        let geom_errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .filter(|d| d.code != Some(reify_core::DiagnosticCode::ConstraintViolated))
            .collect();
        assert!(
            geom_errors.is_empty(),
            "unexpected geometry errors tessellating {DEV_CAPSTAN_RI} (constraint \
             violations are routed to the satisfaction gates and are not this \
             fixture's business): {geom_errors:#?}"
        );
        result
    })
}

/// The `sub` chain dev_capstan.ri's `CapstanDrive` binds the front-upper shuttle
/// idler under — `CapstanDrive.shuttle.idler_fu` (dev_capstan.ri `Fairlead`).
const IDLER_INSTANCE_PREFIX: &str = "CapstanDrive.shuttle.idler_fu#realization[";

/// The tessellated surface backing that idler's `body`.
///
/// The realization index is read off the `body` cell's `Value::GeometryHandle`
/// rather than hard-coded, the trick [`super::capstan_groove_e2e`]'s
/// `capstan_body_path` uses. The panic lists every mesh path, which is also how
/// the instance prefix above was discovered.
fn idler_sheave(result: &reify_eval::TessellateResult) -> &reify_eval::MeshSurface {
    let index = match result.values.get(&ValueCellId::new(IDLER_ENTITY, "body")) {
        Some(Value::GeometryHandle {
            realization_ref, ..
        }) => realization_ref.index,
        other => panic!("{IDLER_ENTITY}.body must be a realized Value::GeometryHandle, got {other:?}"),
    };
    let path = format!("{IDLER_INSTANCE_PREFIX}{index}]");
    result
        .meshes
        .iter()
        .find(|s| s.entity_path == path)
        .unwrap_or_else(|| {
            panic!(
                "no tessellated surface at `{path}` (the idler sheave); all \
                 surfaces: {:?}",
                result.meshes.iter().map(|s| &s.entity_path).collect::<Vec<_>>()
            )
        })
}

/// Least-squares (Kåsa) circle through points known to lie on one — returns the
/// centre and the mean radius about it.
///
/// **Why a fit rather than an extent or a mean.** Tessellation vertices lie ON
/// the analytic surface, so a circle through them is exact — but only a FIT
/// recovers it. Measured on this branch, against `sheave_r` = 18.000 mm:
/// the whole-mesh vertex centroid is off by 280 µm, the bounding-box midpoint by
/// 24 µm (the two radial extremes are not equally facet-deficient), the mean over
/// the planar end faces by 188 µm, and the mean of a max-radius shell diverges
/// outright — the mean of an ARC is not its centre. This fit lands the centre
/// exactly and the radius within 0.0035 µm, which is what makes
/// [`MESH_ABS_TOL`] possible at all: at 24 µm no band satisfying both of that
/// constant's conditions exists.
///
/// Data is centred before solving, so the 3×3 normal equations collapse to a 2×2
/// and stay well conditioned at an 18 mm radius offset 180 mm from the origin.
fn circle_fit(pts: &[(f64, f64)]) -> ((f64, f64), f64) {
    let n = pts.len() as f64;
    let (mu, mv) = (
        pts.iter().map(|p| p.0).sum::<f64>() / n,
        pts.iter().map(|p| p.1).sum::<f64>() / n,
    );
    let (mut a11, mut a12, mut a22, mut b1, mut b2) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for &(u, v) in pts {
        let (du, dv) = (u - mu, v - mv);
        let w = du * du + dv * dv;
        a11 += du * du;
        a12 += du * dv;
        a22 += dv * dv;
        b1 += w * du;
        b2 += w * dv;
    }
    let det = a11 * a22 - a12 * a12;
    assert!(
        det.abs() > 0.0,
        "degenerate circle fit over {} points — they are collinear, so no centre \
         is determined",
        pts.len()
    );
    let centre = (
        mu + 0.5 * (b1 * a22 - b2 * a12) / det,
        mv + 0.5 * (a11 * b2 - a12 * b1) / det,
    );
    let r = pts
        .iter()
        .map(|p| (p.0 - centre.0).hypot(p.1 - centre.1))
        .sum::<f64>()
        / n;
    ((centre.0, centre.1), r)
}

/// ABSOLUTE tolerance, in metres, for a mesh read against a design cell.
///
/// Sized from the measurement, not copied. Measured on this branch against the
/// COMPENSATED geometry — i.e. against the tree this constant actually gates,
/// over 1252 mesh vertices (350 on the rim shell, 528 on the seat):
///
/// | read                          | residual |
/// |-------------------------------|----------|
/// | axial half-extent vs sheave_w/2 | 0.0027 µm |
/// | outer radius vs sheave_r        | 0.0035 µm |
/// | seat arc radius vs groove_r     | 0.0002 µm |
/// | seat arc centre vs seat_c       | 0.0001 µm |
/// | seat arc axial offset           | 0.0000 µm |
/// | seat bottom vs the identity     | 0.0001 µm |
/// | seat opening vs mouth_w         | 0.0138 µm |
/// | meridian fit RMS                | 0.0027 µm |
///
/// Worst case 0.0138 µm, and that floor is f32 vertex QUANTIZATION —
/// `MeshSurface::vertices` is `f32`, whose ~7 significant digits give ~0.001 µm
/// at an 18 mm radius — not kernel error, so it will not drift with a
/// tessellation-density change.
///
/// 5 µm is therefore 362× the worst measured residual and still 36× inside the
/// smallest regression it must catch (the 0.180 mm pitch-circle sink) and 70×
/// inside the 0.350 mm seat-opening signal. Both of the plan's conditions hold
/// with room to spare: ≥3× the worst residual (which would allow 0.041 µm), and
/// ≤0.060 mm. Had the worst residual exceeded 20 µm no band could satisfy both,
/// and that was a real risk: the plan's own seat-bottom read, taken as the
/// minimum radius over vertices on the tessellated torus, measures 21.9 µm. The
/// meridian [`circle_fit`] is what brought it to 0.0001 µm.
///
/// Deliberately NOT [`super::capstan_groove_e2e`]'s `MESH_RADIAL_TOL_FRAC`
/// (`0.10 · groove_r`). That is 318 µm here and would not catch the 0.180 mm
/// regression this gate exists for — it would be looser than the signal.
const MESH_ABS_TOL: f64 = 5e-6;

/// The two copies of `IdlerPulley` must not have forked.
///
/// dev_capstan.ri carries its own copy because it is a standalone sketch and
/// v0.1 has no cross-file import; its header states the contract ("copied
/// verbatim from printer.ri") and until #6135 nothing held it. This is the
/// assertion that makes "must stay verbatim" enforceable instead of advisory —
/// and it is also what licenses `idler_sheave_mesh_has_the_declared_seat` taking
/// its EXPECTATIONS from printer.ri and its MESH from dev_capstan.ri.
///
/// Semantic, not textual: it compares EVALUATED cells, so it is blind to
/// formatting and comment differences (the two blocks' comments are deliberately
/// not identical — dev_capstan.ri's are abridged) and sensitive to any fork that
/// changes what the part IS.
///
/// Three claims, the first two localizing and the third exhaustive:
///   1. every [`IDLER_CELLS`] entry reads equal across the two value maps. A
///      cell present in one copy and missing from the other fails on the READ,
///      naming the file — which is the property that keeps the inventory honest
///      as the structure grows;
///   2. the two constraint sets agree in COUNT and every entry is `Satisfied` in
///      BOTH files. The count is what claim (1) cannot see: a copy can reproduce
///      every cell exactly and still drop `mouth_w > tendon_dia`, leaving the
///      numbers free to be edited back to a slip fit with nothing objecting;
///   3. the two `TopologyTemplate`s have equal `content_hash`. (1) and (2) are
///      both INVENTORIES — named cells, counted constraints — and the `body`
///      tree appears in neither, so a fork inside it passes both. Measured:
///      editing printer.ri's seat back to `torus(sheave_r, groove_r)` moves its
///      template hash while dev_capstan.ri's holds, and every other assertion in
///      this module stays green.
///
/// (1) and (2) are kept in front of (3) rather than subsumed by it because they
/// LOCALIZE: they name the cell or the count that moved. The hash can only say
/// that something did. Keeping all three is the difference between a failure
/// that points at a line and one that points at a file.
#[test]
fn idler_copies_stay_in_lockstep() {
    let printer = &printer_checked().values;
    let sketch = &dev_capstan_checked().values;

    for &(cell, dim) in IDLER_CELLS {
        let a = idler_cell_of(printer, PRINTER_RI, cell, dim);
        let b = idler_cell_of(sketch, DEV_CAPSTAN_RI, cell, dim);
        let err = if a == 0.0 { (a - b).abs() } else { rel_err(b, a) };
        assert!(
            err <= SCALAR_REL_TOL,
            "{IDLER_ENTITY}.{cell} has FORKED between the two copies: printer.ri \
             reads {a:?} and dev_capstan.ri reads {b:?} (rel err {err:.3e}, tol \
             {SCALAR_REL_TOL:.0e}). dev_capstan.ri's copy is held byte-equal to \
             printer.ri's by this gate; printer.ri is the original and owns the \
             contract, so the fix is to bring dev_capstan.ri to it, not the \
             reverse."
        );
    }

    let scoped = |entries: &[ConstraintCheckEntry]| -> Vec<ConstraintNodeId> {
        entries
            .iter()
            .filter(|c| c.id.entity == IDLER_ENTITY)
            .map(|c| c.id.clone())
            .collect()
    };
    let a_cons = assert_idler_constraints_ok(&printer_checked().constraint_results);
    let b_ids = scoped(&dev_capstan_checked().constraint_results);
    assert!(
        !b_ids.is_empty(),
        "no `{IDLER_ENTITY}` constraint results at all from {DEV_CAPSTAN_RI} — an \
         empty set would satisfy the satisfaction filter below vacuously."
    );
    let bad: Vec<_> = dev_capstan_checked()
        .constraint_results
        .iter()
        .filter(|c| c.id.entity == IDLER_ENTITY && c.satisfaction != Satisfaction::Satisfied)
        .collect();
    assert!(
        bad.is_empty(),
        "{DEV_CAPSTAN_RI} must satisfy every `{IDLER_ENTITY}` constraint at its \
         defaults — {} did not: {bad:#?}",
        bad.len()
    );
    assert_eq!(
        a_cons.len(),
        b_ids.len(),
        "the two `{IDLER_ENTITY}` copies declare DIFFERENT numbers of constraints \
         — printer.ri {} and dev_capstan.ri {}. The cell comparison above cannot \
         see this: a copy can reproduce every number exactly and still have \
         dropped a constraint, which leaves those numbers free to be edited back \
         to a zero-clearance slip fit with nothing objecting.",
        a_cons.len(),
        b_ids.len(),
    );

    // ---- (3) The whole compiled structure, as one fingerprint ----
    // Everything above compares an INVENTORY: named cells, counted constraints.
    // The body tree is in neither, so a fork inside it passes both. This sees it.
    let a_hash = idler_template(printer_compiled(), PRINTER_RI).content_hash;
    let b_hash = idler_template(dev_capstan_compiled(), DEV_CAPSTAN_RI).content_hash;
    assert_eq!(
        a_hash, b_hash,
        "the two `{IDLER_ENTITY}` copies have FORKED somewhere the assertions \
         above cannot see: printer.ri hashes to {a_hash:?} and dev_capstan.ri to \
         {b_hash:?}.\n\
         This fingerprint covers the whole compiled structure — the `body` tree \
         included, down to operand order and literal values — and it is \
         span-independent and doc-independent, so neither the two blocks' \
         different line numbers nor their deliberately different comments can \
         move it. What it CANNOT do is say WHERE: it names no cell and no \
         expression. Start at the cell loop and the constraint count above; if \
         both are green, as they will be for a body-tree fork, diff the two \
         `{IDLER_ENTITY}` blocks directly. printer.ri is the original and owns \
         the contract, so the fix is to bring dev_capstan.ri to it.\n\
         It is also blind in one direction by construction: an edit applied to \
         BOTH copies in lockstep keeps the hashes equal. That case is covered by \
         the body read-set pin in `idler_seat_keeps_the_rope_on_the_pitch_circle`."
    );
}

/// The solid the kernel actually fabricates must HAVE the seat the design
/// declares — not merely have cells that describe one.
///
/// The arithmetic gates above are scalar algebra over a handful of cells and
/// would stay green for a torus placed at the wrong radius, a boolean that never
/// breaks through, or a seat that never opens at the rim. This reads the finished
/// sheave back off the mesh.
///
/// **Expectations from printer.ri, mesh from dev_capstan.ri**, licensed by
/// [`idler_copies_stay_in_lockstep`] — the split is deliberate and its reason is
/// in this module's header: printer.ri is the original and owns the contract,
/// dev_capstan.ri is the copy and is the only one affordable to tessellate.
///
/// Four reads, in the idler's OWN frame. The frame was measured, not assumed:
/// the shuttle idlers are posed by `rot_to_x`, so the spin axis is world +X, and
/// the tell is that the de-posed half-extent along x is exactly `sheave_w/2` =
/// 5.000 mm while the other two axes span the rim diameter. The axial centre
/// comes from the two PLANAR end faces, which tessellate exactly; the radial
/// centre from a [`circle_fit`] on the rim.
///   1. the outermost surface is the rim, at `sheave_r`;
///   2. the seat's ARC, fitted in the meridian (axial, radial) plane, has centre
///      radius `seat_c` and arc radius `groove_r`. This is the read with teeth:
///      the seat BOTTOM alone cannot tell the two seats apart — measured at
///      15.000 mm under the pre-#6135 conformal seat AND under the compensated
///      one, which is exactly the identity step 4 was built to preserve — whereas
///      the arc's centre and radius move 18.000→18.180 and 3.000→3.180;
///   3. the bottom that arc implies is still `sheave_r - tendon_dia/2`, the
///      statement that never mentions `groove_r` and so holds for any arc radius;
///   4. the seat's axial opening at the rim is `mouth_w`.
#[test]
fn idler_sheave_mesh_has_the_declared_seat() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let sheave_r = idler_cell("sheave_r", DimensionVector::LENGTH);
    let groove_r = idler_cell("groove_r", DimensionVector::LENGTH);
    let seat_c = idler_cell("seat_c", DimensionVector::LENGTH);
    let tendon_dia = idler_cell("tendon_dia", DimensionVector::LENGTH);
    let mouth_w = idler_cell("mouth_w", DimensionVector::LENGTH);
    let brg_r = idler_cell("brg_r", DimensionVector::LENGTH);
    let sheave_w = idler_cell("sheave_w", DimensionVector::LENGTH);

    let surface = idler_sheave(dev_capstan_tessellated());
    let verts: Vec<(f64, f64, f64)> = surface
        .mesh
        .vertices
        .chunks_exact(3)
        .map(|c| (c[0] as f64, c[1] as f64, c[2] as f64))
        .collect();
    assert!(
        verts.len() > 64,
        "the idler sheave tessellated to only {} vertices — too few to read a \
         seat off; the solid is not where the design says it is",
        verts.len()
    );

    // ---- The idler's own frame, recovered from the mesh ----
    // Axial: world +X, from the planar end faces.
    let (mut xlo, mut xhi) = (f64::MAX, f64::MIN);
    for q in &verts {
        xlo = xlo.min(q.0);
        xhi = xhi.max(q.0);
    }
    let x0 = 0.5 * (xlo + xhi);
    let half_w = 0.5 * (xhi - xlo);
    assert!(
        (half_w - sheave_w / 2.0).abs() <= MESH_ABS_TOL,
        "the idler's axial half-extent must be sheave_w/2 = {:.6} mm — that is \
         the measurement identifying world +X as the spin axis under this \
         instance's `rot_to_x` pose — but the mesh spans {:.6} mm. If this fails, \
         the pose changed and every read below is in the wrong frame.",
        sheave_w * 0.5e3,
        half_w * 1e3,
    );
    // Radial: a circle fit on the rim shell, located with a first pass off the
    // bounding box (good to ~24 µm, which is ample to SELECT the shell).
    let (mut ylo, mut yhi, mut zlo, mut zhi) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for q in &verts {
        ylo = ylo.min(q.1);
        yhi = yhi.max(q.1);
        zlo = zlo.min(q.2);
        zhi = zhi.max(q.2);
    }
    let seed = (0.5 * (ylo + yhi), 0.5 * (zlo + zhi));
    let seed_rmax = verts
        .iter()
        .map(|q| (q.1 - seed.0).hypot(q.2 - seed.1))
        .fold(f64::MIN, f64::max);
    let rim_shell: Vec<(f64, f64)> = verts
        .iter()
        .filter(|q| (q.1 - seed.0).hypot(q.2 - seed.1) > seed_rmax - 50.0 * MESH_ABS_TOL)
        .map(|q| (q.1, q.2))
        .collect();
    let ((y0, z0), _) = circle_fit(&rim_shell);
    let r_of = |q: &(f64, f64, f64)| (q.1 - y0).hypot(q.2 - z0);

    // ---- (1) The outermost surface is the rim ----
    let r_outer = verts.iter().map(r_of).fold(f64::MIN, f64::max);
    assert!(
        (r_outer - sheave_r).abs() <= MESH_ABS_TOL,
        "the sheave's outermost surface must be the rim at sheave_r = {:.6} mm, \
         but the mesh reaches {:.6} mm (tol {:.4} µm, residual {:.4} µm). The rim \
         is what all 31 hand-derived placements position against.",
        sheave_r * 1e3,
        r_outer * 1e3,
        MESH_ABS_TOL * 1e6,
        (r_outer - sheave_r).abs() * 1e6,
    );

    // ---- (2) The seat's ARC, fitted in the meridian plane ----
    // The torus seat's meridian section is a circle of radius groove_r centred at
    // (0, seat_c). Everything strictly inside the rim and outside the bore cut is
    // that surface: the cut sits at the midpoint of brg_r and the seat bottom, so
    // nothing lives between it and either surface it separates.
    let bore_clear = 0.5 * (brg_r + (sheave_r - tendon_dia / 2.0));
    let seat_pts: Vec<(f64, f64)> = verts
        .iter()
        .filter(|q| r_of(q) > bore_clear && r_of(q) < r_outer - 10.0 * MESH_ABS_TOL)
        .map(|q| (q.0 - x0, r_of(q)))
        .collect();
    assert!(
        seat_pts.len() >= 16,
        "only {} mesh vertices lie on the seat surface (strictly inside the rim \
         {:.6} mm and outside the bore cut {:.6} mm) — the seat was never cut, or \
         never broke through the rim, so the sheave renders smooth",
        seat_pts.len(),
        r_outer * 1e3,
        bore_clear * 1e3,
    );
    let ((seat_ax, fit_c), fit_groove) = circle_fit(&seat_pts);
    assert!(
        seat_ax.abs() <= MESH_ABS_TOL,
        "the seat arc must be centred on the sheave's mid-plane, but the fit puts \
         it {:.6} mm off axially (tol {:.4} µm)",
        seat_ax * 1e3,
        MESH_ABS_TOL * 1e6,
    );
    assert!(
        (fit_groove - groove_r).abs() <= MESH_ABS_TOL,
        "the seat arc's RADIUS in the fabricated solid must be groove_r = {:.6} \
         mm — DIN 15061's oversize arc — but the mesh's arc fits {:.6} mm (tol \
         {:.4} µm, residual {:.4} µm). A conformal slip-fit seat fits \
         tendon_dia/2 = {:.6} mm here.",
        groove_r * 1e3,
        fit_groove * 1e3,
        MESH_ABS_TOL * 1e6,
        (fit_groove - groove_r).abs() * 1e6,
        tendon_dia * 0.5e3,
    );
    assert!(
        (fit_c - seat_c).abs() <= MESH_ABS_TOL,
        "the seat arc's CENTRE radius in the fabricated solid must be seat_c = \
         {:.6} mm — outboard of the rim by exactly the pitch-circle compensation \
         — but the mesh's arc is centred at {:.6} mm (tol {:.4} µm, residual \
         {:.4} µm). An UNCOMPENSATED seat centres the arc on the rim at {:.6} mm \
         instead, and that is the 0.180 mm that would move all 31 placements.",
        seat_c * 1e3,
        fit_c * 1e3,
        MESH_ABS_TOL * 1e6,
        (fit_c - seat_c).abs() * 1e6,
        sheave_r * 1e3,
    );

    // ---- (3) The bottom that arc implies is the seated rope's underside ----
    let bottom = fit_c - fit_groove;
    let want_bottom = sheave_r - tendon_dia / 2.0;
    assert!(
        (bottom - want_bottom).abs() <= MESH_ABS_TOL,
        "the fabricated seat's bottom must be sheave_r − tendon_dia/2 = {:.6} mm \
         — a reference that never mentions groove_r, so it holds for any arc \
         radius — but the fitted arc bottoms at {:.6} mm (tol {:.4} µm). Below \
         this the seat eats into the bore clearance (brg_r = {:.6} mm).",
        want_bottom * 1e3,
        bottom * 1e3,
        MESH_ABS_TOL * 1e6,
        brg_r * 1e3,
    );

    // ---- (4) The seat's axial opening at the rim ----
    // Rim vertices exist only outside the seat's mouth, so twice the smallest
    // |axial| among them IS the opening — and the seat's own break-through edge
    // lies exactly on the rim radius, at exactly mouth_w/2.
    let half_mouth = verts
        .iter()
        .filter(|q| (r_of(q) - r_outer).abs() <= MESH_ABS_TOL)
        .map(|q| (q.0 - x0).abs())
        .fold(f64::MAX, f64::min);
    assert!(
        half_mouth.is_finite(),
        "no mesh vertices sit on the rim radius {:.6} mm — the opening cannot be \
         read",
        r_outer * 1e3,
    );
    assert!(
        (2.0 * half_mouth - mouth_w).abs() <= MESH_ABS_TOL,
        "the seat's OPENING at the rim must be mouth_w = {:.6} mm, but the \
         fabricated solid opens {:.6} mm (tol {:.4} µm, residual {:.4} µm). This \
         is the chord the rope is laid in through, and it is also twice the \
         per-side anti-pinch clearance at the rope's widest section — one \
         measurement, both readings, because the compensation puts the rope's \
         centreline on the rim. A conformal slip-fit seat opens exactly \
         tendon_dia = {:.6} mm here and pinches.",
        mouth_w * 1e3,
        2.0 * half_mouth * 1e3,
        MESH_ABS_TOL * 1e6,
        (2.0 * half_mouth - mouth_w).abs() * 1e6,
        tendon_dia * 1e3,
    );
}
