//! γ's per-site ruling on a surveyed site: the three per-site tables, the
//! resolver that unions them, and the corpus-wide unwaived/stale assertion over
//! that resolver.

use crate::corpus::NAMED_SITE_HOST;
use crate::owner::Owner;
use crate::survey_site::{
    ARG_PREFIX, CTOR_ARITY_PREFIX, SurveySite, synth_inline_site, synth_site,
};
use crate::sweep::{SurveyRun, host_line_of, survey_corpus, survey_inline_corpus};
use crate::workspace_git::WORKSPACE_ROOT;

// ─── γ (task #5305): the files γ migrated to ctor-conformance clean ──────────

/// Repo-relative `.ri` files that task #5305 (γ) migrated to ctor-conformance
/// clean, pinned so a regression is caught on the merge gate instead of only by
/// the `#[ignore]`d corpus generator.
///
/// Deliberately a two-file pin rather than a corpus walk. The corpus-wide
/// assertion lives in `generate_ctor_conformance_corpus_survey` and stays
/// `#[ignore]`d because it compiles every tracked `.ri`, ~2.5× the `examples/`
/// walk (`docs/prds/merge-gate-compile-cost.md`). This pin costs one stdlib
/// prelude compile plus these files, so the sites γ actually changed become
/// gate-resident without reversing that landed cost decision.
///
/// Matching is on `DiagnosticCode` IDENTITY via `is_ctor_conformance_code`,
/// never on message prose. The prose-keyed guard covering the same r3b fixture
/// in `crates/reify-eval-fea-tests/tests/r3b_modal_selector_displacement.rs` is
/// deliberately left alone — it is documented code-AGNOSTIC on purpose and keys
/// on a different pair of params (`alpha` / `beta`).
///
/// Both entries reach the walker by DIFFERENT routes, which is why the pin is
/// worth two files rather than one: the r3b site is a ctor ARG, the
/// `mv-2-priv-param.ri` site is a D8 param DEFAULT
/// (`check_param_default_conformance`). One shared assertion covers both because
/// `survey_corpus` filters on the diagnostic CODE, not on the emitting path.
const CTOR_CONFORMANCE_PINNED_CLEAN: &[&str] = &[
    "tests/prd-gate/fixtures/r3b_displacement_at_selector_grammar.ri",
    "tree-sitter-reify/test/fixtures/mv-2-priv-param.ri",
];

/// Every [`CTOR_CONFORMANCE_PINNED_CLEAN`] file compiles with ZERO
/// ctor-conformance diagnostics.
///
/// Accumulates across files and panics once, so a run names every regressed site
/// rather than stopping at the first — the corpus-wide-visibility principle the
/// sibling gates in this binary already follow.
///
/// Coverage is asserted BEFORE the site count, against every way a pinned file
/// can contribute zero sites WITHOUT being clean:
///
/// * it failed to read or parse — `not_surveyed`. Without that check the pin is
///   satisfied by the file disappearing, or by its grammar breaking.
/// * it was dropped from the corpus handed in — the `surveyed` count is short.
/// * it compiled with Error-severity diagnostics — `partial`. [`survey_corpus`]
///   increments `surveyed` BEFORE it tests for errors, so this third path
///   satisfies both checks above on its own: a pinned file that regresses into a
///   compile error may stop contributing sites entirely, because the conformance
///   walk need never reach the ctor. The generator reports 73 of 684 surveyed
///   members in `partial` today — neither pinned file among them — so the shape
///   is live in the corpus and latent here, not hypothetical.
#[test]
fn pinned_clean_files_emit_no_ctor_conformance_diagnostic() {
    let corpus: Vec<String> = CTOR_CONFORMANCE_PINNED_CLEAN
        .iter()
        .map(|p| (*p).to_owned())
        .collect();
    let run = survey_corpus(std::path::Path::new(WORKSPACE_ROOT), &corpus);

    assert!(
        run.not_surveyed.is_empty(),
        "every pinned file must reach the compile phase, else this pin passes \
         vacuously; unreachable: {:?}",
        run.not_surveyed,
    );
    assert_eq!(
        run.surveyed,
        corpus.len(),
        "all {} pinned file(s) must be surveyed, only {} were",
        corpus.len(),
        run.surveyed,
    );
    assert!(
        run.partial.is_empty(),
        "every pinned file must compile with NO Error-severity diagnostic, else the \
         conformance walk may never reach its ctor and this pin passes vacuously; \
         partial: {:?}\n\n\
         This is a DIFFERENT defect from the site regression reported below and wants a \
         different fix: the file no longer COMPILES. Fix the compile error first — the \
         zero-site result above says nothing about conformance until it does.",
        run.partial,
    );

    let offenders: Vec<String> = run
        .sites
        .iter()
        .map(|s| {
            format!(
                "  {}:{} :: param '{}'  [{} / {}]  {}",
                s.file,
                s.line,
                s.field.as_deref().unwrap_or("—"),
                s.code,
                s.severity,
                s.message,
            )
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "{} ctor-conformance diagnostic(s) in file(s) task #5305 migrated to clean:\n{}\n\n\
         A pinned file carries no waiver and no owner — that is the point of the pin. \
         Either the migration was reverted, or a new un-migrated site was added; fix \
         the site (a ctor argument, or a param default).",
        offenders.len(),
        offenders.join("\n"),
    );
}

// ─── γ (task #5305): the sites γ deferred, with their owners ─────────────────

/// Per-SITE, owner-attributed deferrals for the ctor-conformance sites that
/// survive γ (task #5305) OUTSIDE `examples/`.
///
/// Each entry is `(repo_relative_path, param_name, owning_task, why)`.
///
/// # These are DELIBERATE before-images. Do not "fix" them.
///
/// Every site below is a committed RED before-image for another PRD, and the
/// conformance violation IS the fixture's content. Each listed fixture's header
/// records its post-δ state. The `dcr_*` files record, SHA-stamped, that they
/// evaluated clean at decompose, then a POST-δ block names THIS chokepoint as
/// the source of their current exit 1. `curvature_rad_literal.ri` differs: it
/// was already red at decompose on its own `ParamDefaultTypeMismatch`, and its
/// POST-δ block records THIS chokepoint as a second Error on the same mismatch.
/// How each owning leaf reads its signal instead is recorded once, in
/// `docs/notes/ctor-conformance-flip-leaf-signals.md`.
/// `dcr_solver_load_dropped_dimensioned.ri` exists for no other purpose than to
/// show that the units-CORRECT `force: 1000N` contributes exactly ZERO force to
/// `solve_elastic_static` while the bare control contributes 1000 N. Dimension
/// the call site and the measurement it encodes is gone.
///
/// So these are NOT un-migrated call sites that nobody got around to. That
/// distinction is the entire reason this table carries a `why` column.
///
/// # This is a γ RULING, recorded where it can be checked
///
/// PRD §4 D9 assigns the per-case judgment — call-site bug, or wrong declared
/// field type — to γ. γ ruled: two sites were the call site's fault and are
/// fixed in this branch (see [`CTOR_CONFORMANCE_PINNED_CLEAN`]); these eleven
/// are owned elsewhere and are deferred, per the shape
/// `CTOR_CONFORMANCE_GATE_REMEDY` remedy 3 already mandates on main — *"Add a
/// per-SITE entry naming the file, the param and the LIVE task that owns
/// retiring it. Per-site, never per-file, and never without an owner."*
///
/// # Retirement
///
/// Each entry is deleted by its OWNING task's own diff, exactly as #5847 retires
/// the two `CTOR_CONFORMANCE_MIGRATION_DEBT` entries. An entry left behind after
/// its site is retired is caught by the corpus-wide check inside
/// `generate_ctor_conformance_corpus_survey`, which reports a stale entry and
/// an unexplained site as two different defects.
///
/// The `why` column's Greek leaf labels are
/// `docs/prds/v0_6/dimension-checked-readers.md`'s, kept alongside the `#NNNN`
/// cite so the attribution stays legible if those cluster tasks are re-split:
/// γ1/ε/η are #6922, γ2/β/ζ are #6941.
///
/// # Sibling of `CTOR_CONFORMANCE_MIGRATION_DEBT`, not a merge of it
///
/// That list is `examples/`-keyed BY CONSTRUCTION: its own doc forbids the
/// repo-relative spelling, and the gate consuming it walks `examples_dir()` only.
/// It cannot name a path under `tests/prd-gate/fixtures/` at all. The two tables
/// are joined at exactly one place — the disposition resolver — and
/// [`ctor_conformance_corpus_residual_is_disjoint_from_migration_debt`] keeps
/// them from ever describing the same site.
///
/// # This is NOT `SKIP_SET`
///
/// Nothing here is dropped from any walk. Each entry excuses ONE
/// `(file, param)` pair; every other diagnostic in these files stays unwaived,
/// and a ctor-conformance diagnostic at a different param in the same file is
/// an unexplained site.
pub(crate) const CTOR_CONFORMANCE_CORPUS_RESIDUAL: &[(&str, &str, &str, &str)] = &[
    (
        "tests/prd-gate/fixtures/curvature_rad_literal.ri",
        "kc",
        "#6179",
        "angle-completion leaf α, boundary row B1: CURVATURE is m^-1 pre-α, so the \
         rad·m^-1 initializer mismatches; the fixture's own header calls that \
         check-time flip α's signal",
    ),
    (
        "tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri",
        "force",
        "#6941",
        "leaf γ2: PointLoad.force is declared `Real` in fea_multi_case.ri, so the \
         units-CORRECT `force: 5000N` warns; γ2 retypes the FIELD, and the call site \
         is already right",
    ),
    (
        "tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri",
        "traction",
        "#6941",
        "leaf γ2: TractionLoad.traction is declared `Real` in fea_multi_case.ri; same \
         retype, and TractionLoad reaches no solver at all today (INV-SF-3)",
    ),
    (
        "tests/prd-gate/fixtures/dcr_material_dimension_silent.ri",
        "youngs_modulus",
        "#6941",
        "leaf β, boundary row B4: `youngs_modulus: 200mm` is read as 0.2 Pa by \
         material_field_si, measured 1e12x wrong at exit 0 with zero Error diagnostics",
    ),
    (
        "tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri",
        "ex",
        "#6922",
        "leaf η: `FDMCouponOverride(ex: 2mm)` stores 0.002 m and the dimension-blind \
         opt_f64 reads it as 0.002 Pa",
    ),
    (
        "tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri",
        "line_width",
        "#6922",
        "leaf η: `AsPrintedOptions(line_width: 0.4)` is read as 0.4 METRES by \
         field_scalar — a 1000x error on a 0.4mm extrusion",
    ),
    (
        "tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri",
        "mass",
        "#6922",
        "leaf ε: `MassProperties(mass: 2m)` is read as 2.0 kg by the blind cell_f64 \
         copy, while the dimension-checking cell_mass_f64 sits unused ~300 lines away",
    ),
    (
        "tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri",
        "target_frequency",
        "#6941",
        "leaf ζ: `ZVShaper(target_frequency: 50rad/s)` is stored verbatim as rad·s^-1 \
         and the Hz->rad/s marshalling then multiplies by 2π — a 6.28x error",
    ),
    (
        "tests/prd-gate/fixtures/dcr_shaper_frequency_dimension_silent.ri",
        "target_frequency",
        "#6941",
        "leaf ζ signal fixture: the same 6.28x error, but CONSUMED via input_shape so \
         read_scalar_si actually runs — the ctor alone never reaches the reader",
    ),
    (
        "tests/prd-gate/fixtures/dcr_solver_load_dropped_dimensioned.ri",
        "force",
        "#6922",
        "leaf γ1 headline inversion: the units-CORRECT `force: 1000N` contributes \
         EXACTLY ZERO force to solve_elastic_static (max_von_mises 0, iterations 0) \
         where the bare control contributes 1000 N",
    ),
    (
        "tests/prd-gate/fixtures/dcr_yield_stress_dimension_silent.ri",
        "yield_stress",
        "#6941",
        "leaf β, boundary row B5: `yield_stress: 310mm` is stored as Some(0.31 m) and \
         material_field_si reads it as 0.31 Pa, at exit 0 with zero diagnostics",
    ),
];

/// The repo-relative prefix of the `examples/` corpus.
///
/// [`CTOR_CONFORMANCE_MIGRATION_DEBT`](reify_test_support::ctor_conformance_debt::CTOR_CONFORMANCE_MIGRATION_DEBT)
/// is keyed relative to that directory; every key in THIS module is
/// repo-relative. This const is the whole of the difference.
pub(crate) const EXAMPLES_PREFIX: &str = "examples/";

/// Whether a [`CTOR_CONFORMANCE_MIGRATION_DEBT`](reify_test_support::ctor_conformance_debt::CTOR_CONFORMANCE_MIGRATION_DEBT)
/// entry describes the REPO-RELATIVE site `(file, param)`.
///
/// The single place the two tables' key forms are bridged. The debt list is
/// `examples/`-keyed by construction — its own doc forbids the repo-relative
/// spelling, and the gate that consumes it walks `examples_dir()` only — so
/// neither table can change shape and the join has to happen here. A file
/// outside `examples/` can never match a debt entry, which is exactly why
/// [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] has to exist as a sibling table.
///
/// The `(file, param)` matching RULE is not restated here; it is
/// `reify_test_support::ctor_conformance_debt::debt_entry_matches`, called through.
fn debt_entry_describes(entry: &(&str, &str, &str), file: &str, param: Option<&str>) -> bool {
    file.strip_prefix(EXAMPLES_PREFIX).is_some_and(|key| {
        reify_test_support::ctor_conformance_debt::debt_entry_matches(entry, key, param)
    })
}

/// The reason every
/// [`CTOR_CONFORMANCE_MIGRATION_DEBT`](reify_test_support::ctor_conformance_debt::CTOR_CONFORMANCE_MIGRATION_DEBT)
/// site is deferred.
///
/// That table carries no `why` column — it predates this one, and every entry in
/// it shares one reason — so the reason belongs to the TABLE, not to a row.
/// Stated here once rather than copied into each rendered row, and deliberately
/// not added as a fourth column there: #5847 and #5306 are both chartered
/// against that list by name.
const MIGRATION_DEBT_WHY: &str = "un-migrated examples/ call site that cannot be dimensioned \
     in isolation; waived per-site in CTOR_CONFORMANCE_MIGRATION_DEBT and retired by its \
     owning task's own diff";

/// The `Debug` rendering of the severity `CTOR_FIELD_CONFORMANCE_SEVERITY` emits
/// a ctor-conformance site at, which is how [`SurveySite::severity`] carries it.
///
/// `"Error"` since δ (#5306) flipped that knob; `"Warning"` under α, which is
/// what γ's signal was first scoped to. Named for its ROLE rather than for
/// either value, so the next flip moves the value here and nothing else.
///
/// Read in exactly ONE place — [`disposition_of`] — so the scope is stated once
/// and every consumer inherits it through the resolver instead of re-filtering on
/// severity itself.
pub(crate) const CTOR_CONFORMANCE_SITE_SEVERITY: &str = "Error";

/// Whether `site`'s wording names the CTOR ARGUMENT it is about — the half of
/// [`disposition_of`]'s scope statement that severity used to carry alone.
///
/// Most emitters the knob governs name the offending argument through the
/// ctor's own argument list, either with [`ARG_PREFIX`] (α's four wordings and
/// ε's `unknown named argument '…'`) or, for ε's arity code, with
/// [`CTOR_ARITY_PREFIX`] — which names the def and a count instead, there being
/// no single argument to name. The non-ctor emitters of the same codes name a
/// composition, an overload set or a `required by param`, and so match neither.
///
/// # KNOWN BLIND SPOT: two knob-governed ctor families are scoped OUT
///
/// "Most", not "every". The leaf-trait arms of `emit_leaf_conformance_for_arg_type`
/// (`type 'X' does not conform to trait 'T' required by param 'p'`) and its
/// wrapper-shape arm (`type 'X' does not match wrapper shape required by param
/// 'p' …`) are built with `diag_at(ctx.severity, …)`, so the knob DOES govern
/// them on the ctor path — yet they match neither prefix. The fn-call entry
/// (`check_fn_arg_conformance`) emits byte-identical wording, so no reading of
/// the prose can separate the two paths. Consequence: a genuine ctor-field
/// trait-conformance or wrapper-shape site resolves
/// [`Disposition::NotApplicable`] and [`assert_no_unwaived_ctor_conformance_sites`]
/// does not count it. Under α severity kept these in scope; δ's flip lost that.
/// `a_ctor_path_required_by_param_site_is_scoped_out_known_blind_spot` pins the
/// current classification so the gap is visible rather than silent. The cure is
/// a structured ctor-vs-fn-call discriminator on the diagnostic itself, not more
/// prose matching — tracked as a follow-up to #5306.
///
/// Both prefixes are already this survey's, read here rather than re-spelled:
/// `epsilon_and_required_by_param_prose_extractors_hold_against_the_live_emitters`
/// pins them against the live emitters, so a wording drift reds there — at the
/// extractor that owns the coupling — instead of quietly narrowing this scope.
///
/// Read in exactly ONE place, [`disposition_of`], for the same reason the
/// severity const is.
fn names_a_ctor_argument(site: &SurveySite) -> bool {
    site.message.contains(ARG_PREFIX) || site.message.starts_with(CTOR_ARITY_PREFIX)
}

/// The committed CLI probe-set that asserts every
/// [`CTOR_CONFORMANCE_REJECTION_FIXTURES`] file actually rejects.
///
/// Named here because this table's soundness depends on it: without a probe, an
/// entry is an unbacked declaration. Run by
/// `tests/infra/test_prd_gate_struct_ctor_conformance.sh`.
const REJECTION_PROBE_SET_REL: &str = "tests/prd-gate/struct-ctor-conformance-probe-set.json";

/// Sites whose ctor-conformance violation IS the deliverable: δ's (#5306)
/// committed PRD §7 boundary-row rejection fixtures.
///
/// Each entry is `(repo-relative file, param or `None`, why)`. The param is the
/// one [`SurveySite::field`] recovers, taken off the generator's MEASURED panic
/// text rather than read off the fixture source — the two can differ, and only
/// the first is what the resolver keys on.
///
/// # No owner column, deliberately
///
/// The other two tables carry a `#NNNN` owner because their entries are RETIRED
/// by that task's diff, and [`is_canonical_task_cite`] keeps the cite
/// liveness-checkable. Neither applies here: these fixtures are never retired —
/// they are the PRD's G2 headline signal made repeatable, and deleting one reds
/// `tests/infra/test_prd_gate_struct_ctor_conformance.sh`. Naming #5306 would
/// orphan the cite the moment δ goes done on merge, and would tell a reader that
/// someone owes work here. What retires this table is the survey's own
/// retirement, already documented in the harness root — not a task.
///
/// # This is NOT `SKIP_SET`, and the guard that makes that true is not a comment
///
/// Each entry excuses ONE `(file, param)` pair, so a rejection fixture that grows
/// a SECOND, unintended violation is still reported
/// (`intended_rejection_claims_the_listed_param_and_not_its_neighbours`). And
/// every entry's file must be the fixture of a probe in
/// [`REJECTION_PROBE_SET_REL`] that asserts `reify check` rejects it
/// (`every_rejection_fixture_is_asserted_by_a_committed_cli_probe`), so a file
/// can be declared an intended rejection only while a committed gate
/// independently asserts that it DOES reject.
const CTOR_CONFORMANCE_REJECTION_FIXTURES: &[(&str, Option<&str>, &str)] = &[
    (
        "tests/prd-gate/fixtures/struct_ctor_conformance_int_at_string_field.ri",
        Some("label"),
        "PRD §7 row 2 (general concrete leaf, Int at a String field); asserted to exit 1 by \
         the `check` probe for this file in tests/prd-gate/struct-ctor-conformance-probe-set.json",
    ),
    (
        "tests/prd-gate/fixtures/struct_ctor_conformance_over_arity.ri",
        None,
        "PRD §7 row 12 (E_CTOR_ARITY); the diagnostic names a def and a count, never an \
         argument, so no param is recoverable — asserted to exit 1 by the `check` probe for \
         this file in tests/prd-gate/struct-ctor-conformance-probe-set.json",
    ),
    (
        "tests/prd-gate/fixtures/struct_ctor_conformance_pose_at_selector_field.ri",
        Some("face"),
        "PRD §7 row 1 (a coordinate pose at a selector-typed field, over the real stdlib \
         PressureLoad so α's Option-unwrap arm is exercised); asserted to exit 1 by the \
         `check` probe for this file in tests/prd-gate/struct-ctor-conformance-probe-set.json",
    ),
    (
        "tests/prd-gate/fixtures/struct_ctor_conformance_string_at_selector_field.ri",
        Some("face"),
        "PRD §7 row 3 (disallow-string at a selector-typed field, deliberately WITHOUT the \
         pose hint per the 4581 over-tag guard); asserted to exit 1 by the `check` probe for \
         this file in tests/prd-gate/struct-ctor-conformance-probe-set.json",
    ),
    (
        "tests/prd-gate/fixtures/struct_ctor_conformance_unknown_field.ri",
        Some("labl"),
        "PRD §7 row 11 (E_CTOR_UNKNOWN_FIELD); asserted to exit 1 by the `check` probe for \
         this file in tests/prd-gate/struct-ctor-conformance-probe-set.json",
    ),
];

/// γ's per-site ruling on a surveyed site, resolved from the site's measured
/// severity and the waiver tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Disposition {
    /// A live task owns retiring the site. `why` says what breaks if someone
    /// migrates it here instead.
    Deferred {
        owning_task: &'static str,
        why: &'static str,
    },
    /// The site carries a ctor-conformance CODE, but is not one of the sites the
    /// conformance knob governs: it is outside γ's signal, and nobody owns
    /// retiring it.
    ///
    /// Its own variant rather than folded into [`Disposition::Unattributed`],
    /// because the two call for OPPOSITE actions — an unattributed row is work,
    /// this is work that does not exist. Folding them told the artifact's reader
    /// to go fix three deliberate rejection fixtures whose violation IS their
    /// content.
    NotApplicable,
    /// An in-scope site whose violation IS the deliverable: a committed PRD §7
    /// boundary-row fixture that `reify check` is asserted to REJECT.
    ///
    /// Distinct from [`Disposition::NotApplicable`] on scope — these sites ARE
    /// knob-governed and DO name a ctor argument — and from
    /// [`Disposition::Deferred`] on ownership: no task retires them, so there is
    /// no owner to carry. Folding either way told the artifact's reader to go
    /// break δ's own CLI gate.
    IntendedRejection,
    /// A knob-governed site that no table names: it is actionable, and nobody has
    /// claimed it.
    Unattributed,
    /// A row from the INLINE half of the corpus: a Reify snippet embedded in a
    /// Rust test fixture.
    ///
    /// A CENSUS state, not a triage state. δ (#5306) fixed the inline sites its
    /// severity flip exposed (f247bade44) and kept the rest as deliberate Error
    /// pins, so a surviving inline site is one its host test asserts, tolerates,
    /// or never compiles. It is neither pending work nor waived, and no task owns
    /// it: its host test owns the verdict. The rows are listed so the class stays
    /// countable and cannot recur unnoticed on the next severity change.
    ///
    /// Resolved FIRST, ahead of the scope and param early returns and ahead of
    /// all three tables, so the entire inline half answers to ONE rule. Any later
    /// placement lets an inline row be read as `Unattributed` (which panics
    /// [`assert_no_unwaived_ctor_conformance_sites`] over sites no `.ri` table
    /// can name) or as `Deferred` (which would let an inline row satisfy a `.ri`
    /// waiver entry and keep a landed task's waiver looking live forever).
    InlineCensus,
}

impl Disposition {
    /// The artifact cell for this disposition.
    ///
    /// A deferred cell names the owner and the reason so it reads standalone —
    /// the artifact is consumed one row at a time, and a bare cite would send
    /// the reader hunting for a table to find out why. The `n/a` cell instead
    /// stays short and the artifact header explains that class ONCE: it applies
    /// to whole rows identically, so repeating a paragraph per row would be the
    /// copy that rots.
    pub(crate) fn label(self) -> String {
        match self {
            Disposition::Deferred { owning_task, why } => {
                format!("deferred — owned by {owning_task}: {why}")
            }
            Disposition::NotApplicable => {
                "n/a — names no ctor argument, outside the ctor-conformance signal: \
                 nothing to retire"
                    .to_owned()
            }
            Disposition::IntendedRejection => {
                "intended rejection — this fixture's violation is the signal: nothing to retire"
                    .to_owned()
            }
            Disposition::Unattributed => "unattributed — actionable".to_owned(),
            Disposition::InlineCensus => {
                "census — inline Rust fixture: enumerated, not ruled; its host test owns \
                 the verdict"
                    .to_owned()
            }
        }
    }
}

/// Resolve `site`'s disposition from [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`],
/// [`CTOR_CONFORMANCE_MIGRATION_DEBT`](reify_test_support::ctor_conformance_debt::CTOR_CONFORMANCE_MIGRATION_DEBT)
/// and [`CTOR_CONFORMANCE_REJECTION_FIXTURES`].
///
/// The ONLY place the three tables are unioned. The tables are the single source
/// of truth and the artifact is a projection of them, so nothing else re-derives
/// this mapping — including the corpus-wide check in
/// `generate_ctor_conformance_corpus_survey`, which calls straight through.
///
/// # Four dispositions, because a reader has four DIFFERENT things to do
///
/// The arms are not four shades of the same judgement; each one calls for a
/// different action, and that is the whole reason the artifact is worth reading
/// one row at a time:
///
/// * [`Disposition::Unattributed`] — FIX IT. In scope, unclaimed, actionable.
/// * [`Disposition::Deferred`] — LEAVE IT, and read the named task to learn what
///   breaks if you migrate it here instead.
/// * [`Disposition::NotApplicable`] — IGNORE IT. The row carries a
///   ctor-conformance code but the knob's walk did not emit it; the work does
///   not exist.
/// * [`Disposition::IntendedRejection`] — DO NOT TOUCH IT. In scope and
///   genuinely violating, on purpose: the violation is a committed PRD §7
///   signal, and removing it reds the CLI gate that asserts the rejection.
///
/// Collapsing any pair sends the reader the opposite way from the right one.
///
/// Consultation order is immaterial:
/// [`ctor_conformance_corpus_residual_is_disjoint_from_migration_debt`] proves
/// no site can be described by both.
///
/// An in-scope site whose param could not be recovered is
/// [`Disposition::Unattributed`]. Both tables key on `(file, param)`, so there is
/// nothing to match on, and the conservative default is the one that does not
/// invent an owner.
///
/// # SCOPE is decided here, before any table is consulted
///
/// γ's signal is "zero unwaived ctor-conformance sites", and a site is in that
/// signal when the ctor-argument walk the knob governs is what emitted it. Two
/// conditions say so, and a site failing either is
/// [`Disposition::NotApplicable`]: it carries
/// [`CTOR_CONFORMANCE_SITE_SEVERITY`], and [`names_a_ctor_argument`].
///
/// Under α the severity alone was the whole scope statement — the knob was the
/// only Warning-severity source of these seven codes. δ (#5306) flipped it to
/// `Error`, which is where the OTHER emitters of the same codes already were, so
/// the second condition took over the job of separating them. It does that job
/// only partially: ctor-path `required by param` sites (leaf-trait and
/// wrapper-shape) are scoped OUT alongside the fn-call ones they cannot be told
/// apart from — see [`names_a_ctor_argument`]'s "KNOWN BLIND SPOT".
///
/// # The corpus's three non-knob sites, measured
///
/// `bt1_wrong_kind_union.ri`, `bt6_kind_typed_param.ri` and
/// `raw_lambda_material_field_rejected.ri` are deliberate REJECTION fixtures for
/// other PRDs, reached from non-ctor paths (selector composition, overload
/// resolution, trait conformance). They work exactly as intended; giving them an
/// owner would invent work, and leaving them `Unattributed` told the artifact's
/// reader to go delete three other PRDs' signals. Their wording — recorded in
/// the committed artifact's rows for those three files, which γ measured while
/// the knob was still `Warning` — names no argument:
///
/// * `selector composition kind mismatch: cannot compose FaceSelector and …`
/// * `no matching overload for needs_face(EdgeSelector), candidates: …`
/// * `type 'Field<…>' does not conform to trait 'ConstitutiveLaw' required by
///   param 'material'`
///
/// # Why not the `Owner` column, which looks like it already draws this line
///
/// [`Owner::UnresolvedDef`] means "the anchor named something that is not a
/// declared `structure def`", which is exactly the non-ctor call shape, and it
/// does catch the first two. It is not sufficient, and the difference is
/// MEASURED rather than argued: `raw_lambda`'s site recovers no def at all
/// ([`Owner::Unknown`]) — and so does a genuine, waived ctor site,
/// `curvature_rad_literal.ri :: param 'kc'`, whose label anchors at an
/// initializer rather than at a `Def(` call. Scoping on the owner column would
/// therefore stale a live [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] entry, so the
/// ctor/non-ctor split has to be read off the DIAGNOSTIC, not off the anchor.
///
/// Stating the scope HERE rather than as a filter at each consumer is what keeps
/// the artifact's `disposition` column and
/// [`assert_no_unwaived_ctor_conformance_sites`] unable to disagree about which
/// sites the signal even covers.
///
/// The next flip of `CTOR_FIELD_CONFORMANCE_SEVERITY` moves
/// [`CTOR_CONFORMANCE_SITE_SEVERITY`] with it. Until it does, every waiver entry
/// reads as STALE and the assertion goes RED naming them — loudly re-scoped,
/// never vacuously green.
pub(crate) fn disposition_of(site: &SurveySite) -> Disposition {
    // FIRST, ahead of the severity and scope returns and of all three tables:
    // the inline half is a census in its entirety, so it answers to one rule.
    // See `Disposition::InlineCensus` for what each later placement would break.
    if site.snippet_line.is_some() {
        return Disposition::InlineCensus;
    }

    if site.severity != CTOR_CONFORMANCE_SITE_SEVERITY || !names_a_ctor_argument(site) {
        return Disposition::NotApplicable;
    }

    // BEFORE the param early return, and that ordering is forced rather than
    // stylistic: ε's arity wording names a def and a count, so `site.field` is
    // `None` at `struct_ctor_conformance_over_arity.ri` and the early return
    // below is exactly what stranded it as an unexplained site. Matching on
    // `(file, param)` — with `param` as `Option` — is what lets one table reach
    // both shapes.
    if CTOR_CONFORMANCE_REJECTION_FIXTURES
        .iter()
        .any(|(file, param, _)| *file == site.file && *param == site.field.as_deref())
    {
        return Disposition::IntendedRejection;
    }

    let Some(param) = site.field.as_deref() else {
        return Disposition::Unattributed;
    };

    if let Some(&(_, _, owning_task, why)) = CTOR_CONFORMANCE_CORPUS_RESIDUAL
        .iter()
        .find(|entry| entry.0 == site.file && entry.1 == param)
    {
        return Disposition::Deferred { owning_task, why };
    }

    if let Some(&(_, _, owning_task)) = reify_test_support::ctor_conformance_debt::CTOR_CONFORMANCE_MIGRATION_DEBT
        .iter()
        .find(|entry| debt_entry_describes(entry, &site.file, Some(param)))
    {
        return Disposition::Deferred {
            owning_task,
            why: MIGRATION_DEBT_WHY,
        };
    }

    Disposition::Unattributed
}

/// Panic unless every in-scope ctor-conformance site in `run` is accounted for by
/// exactly one waiver-table entry, and every entry accounts for at least one
/// site.
///
/// This is γ's actual signal — "zero UNWAIVED ctor-conformance sites" — made
/// repeatable instead of asserted once in a commit message.
///
/// Both directions are reported together, because they are DIFFERENT defects:
///
/// * a site named by NEITHER table is an UNEXPLAINED site. Someone added an
///   un-migrated call site, or reverted a migration; γ's invariant is broken.
/// * an entry matching NO site is STALE. Its owning task landed and the entry
///   must be deleted in that same diff — exactly the rot
///   `ctor_conformance_migration_debt_entries_are_all_live` catches for the debt
///   list, extended to the whole tracked corpus.
///
/// # Scoped, but it does not say so itself
///
/// The corpus also carries ctor-conformance-CODED sites the conformance knob did
/// not emit — `bt1_wrong_kind_union.ri`, `bt6_kind_typed_param.ri`,
/// `raw_lambda_material_field_rejected.ri`. Those are deliberate REJECTION
/// fixtures reached from non-ctor paths (selector composition, overload
/// resolution, trait conformance): they are working exactly as intended, they
/// are not ctor-conformance sites, and enumerating them as residual would claim
/// an owner for something nobody needs to retire.
///
/// That scope is [`disposition_of`]'s, not this function's: nothing here reads
/// [`CTOR_CONFORMANCE_SITE_SEVERITY`] or calls [`names_a_ctor_argument`]. Both
/// directions below are decided entirely by the resolver — `Unattributed` is the
/// unexplained set, `Deferred` is the waived set — so the artifact's
/// `disposition` column and this assertion cannot disagree about whether a site
/// is in scope OR about whether it is waived. A scope filter here as well would
/// be a second, silently divergent copy of the scope statement.
pub(crate) fn assert_no_unwaived_ctor_conformance_sites(run: &SurveyRun) {
    let unexplained: Vec<String> = run
        .sites
        .iter()
        .filter(|s| disposition_of(s) == Disposition::Unattributed)
        .map(|s| {
            format!(
                "  {}:{} :: param '{}'  {}",
                s.file,
                s.line,
                s.field.as_deref().unwrap_or("—"),
                s.message,
            )
        })
        .collect();

    // The waived set, by the same resolver that renders the artifact column.
    // Scope rides along rather than being re-stated: only an in-scope site can
    // resolve to `Deferred`, so an entry whose only site left the scope reads as
    // stale — the loud, correct outcome.
    let waived: Vec<&SurveySite> = run
        .sites
        .iter()
        .filter(|s| matches!(disposition_of(s), Disposition::Deferred { .. }))
        .collect();

    let stale_residual = CTOR_CONFORMANCE_CORPUS_RESIDUAL.iter().filter_map(|entry| {
        let matched = waived
            .iter()
            .any(|s| s.file == entry.0 && s.field.as_deref() == Some(entry.1));
        (!matched).then(|| {
            format!(
                "  {} :: param '{}'  (owner {}, CTOR_CONFORMANCE_CORPUS_RESIDUAL)",
                entry.0, entry.1, entry.2,
            )
        })
    });
    let stale_debt = reify_test_support::ctor_conformance_debt::CTOR_CONFORMANCE_MIGRATION_DEBT
        .iter()
        .filter_map(|entry| {
            let matched = waived
                .iter()
                .any(|s| debt_entry_describes(entry, &s.file, s.field.as_deref()));
            (!matched).then(|| {
                format!(
                    "  {}{} :: param '{}'  (owner {}, CTOR_CONFORMANCE_MIGRATION_DEBT)",
                    EXAMPLES_PREFIX, entry.0, entry.1, entry.2,
                )
            })
        });
    // Rejection entries go stale exactly as waiver entries do, and for a reason
    // that is MORE likely here than there: a renamed or deleted fixture leaves a
    // dead entry behind, and a dead entry in THIS table is a claim that a probe
    // is watching a file that no longer exists.
    let claimed: Vec<&SurveySite> = run
        .sites
        .iter()
        .filter(|s| disposition_of(s) == Disposition::IntendedRejection)
        .collect();
    let stale_rejection = CTOR_CONFORMANCE_REJECTION_FIXTURES
        .iter()
        .filter_map(|(file, param, _)| {
            let matched = claimed
                .iter()
                .any(|s| s.file == *file && s.field.as_deref() == *param);
            (!matched).then(|| {
                format!(
                    "  {} :: param '{}'  (CTOR_CONFORMANCE_REJECTION_FIXTURES)",
                    file,
                    param.unwrap_or("—"),
                )
            })
        });

    let stale: Vec<String> = stale_residual
        .chain(stale_debt)
        .chain(stale_rejection)
        .collect();

    assert!(
        unexplained.is_empty() && stale.is_empty(),
        "the tracked corpus and the waiver tables disagree: {} unexplained \
         site(s), {} stale entry/entries.\n\n\
         UNEXPLAINED — an in-scope ctor-conformance site named by NEITHER \
         CTOR_CONFORMANCE_CORPUS_RESIDUAL nor CTOR_CONFORMANCE_MIGRATION_DEBT:\n{}\n\n\
         Fix the site. Add a waiver ONLY if a LIVE task genuinely owns retiring it, \
         and then name that task and say what breaks if it is migrated here instead. \
         A THIRD, narrower option applies only to a committed rejection FIXTURE whose \
         violation is itself a PRD §7 signal: add it to \
         CTOR_CONFORMANCE_REJECTION_FIXTURES, which requires a probe in \
         tests/prd-gate/struct-ctor-conformance-probe-set.json already asserting that \
         the file rejects.\n\n\
         STALE — a waiver entry matching no live site:\n{}\n\n\
         The expected case is that the owning task landed: DELETE the entry, in the \
         same diff that retired the site. TWO other causes make EVERY entry go stale \
         at once, and neither is fixed by deleting them: param extraction stopped \
         matching the emitter's `argument '<name>'` wording (fix the extraction), or \
         `CTOR_FIELD_CONFORMANCE_SEVERITY` moved again and \
         CTOR_CONFORMANCE_SITE_SEVERITY did not move with it (re-point that const, \
         which with `names_a_ctor_argument` is the ONE place that scope is stated).\n\n\
         Scope comes from `disposition_of`, and TWO distinct classes sit outside the \
         actionable set for different reasons. Non-knob sites carry a \
         ctor-conformance code but were emitted by another PRD's path, so they name \
         no ctor argument and resolve to `n/a`. In-scope INTENDED REJECTIONS are \
         knob-governed and genuinely violating, on purpose: they are committed PRD §7 \
         boundary-row fixtures whose rejection a CLI probe asserts. Neither is waived \
         and neither is counted here.",
        unexplained.len(),
        stale.len(),
        if unexplained.is_empty() {
            "  (none)".to_owned()
        } else {
            unexplained.join("\n")
        },
        if stale.is_empty() {
            "  (none)".to_owned()
        } else {
            stale.join("\n")
        },
    );
}

// ─── the inline half is a CENSUS, not a gate ─────────────────────────────────

/// Every inline row resolves to [`Disposition::InlineCensus`] — whatever its
/// severity, and whatever its param extraction recovered.
///
/// This is the single most dangerous interaction in task #7543.
/// [`assert_no_unwaived_ctor_conformance_sites`] panics on any site resolving
/// to [`Disposition::Unattributed`], and the inline half surfaces sites whose
/// verdict their host tests own — δ (#5306) kept them as deliberate Error pins —
/// and that no `.ri` table can name. Routing them through the resolver — rather than adding a second
/// severity-or-origin filter at the assertion — is what keeps the artifact's
/// `disposition` column and that assertion unable to disagree, exactly as the
/// assertion's own doc requires.
///
/// The `field: None` case is the one that pins the arm's PLACEMENT: the existing
/// `let Some(param) = … else { return Unattributed }` early return would
/// otherwise claim an inline row whose param extraction missed. The
/// Warning-severity case pins it further up still, ahead of the scope early
/// return, so the whole inline half resolves by ONE rule rather than by two that
/// could drift.
#[test]
fn every_inline_row_resolves_to_the_census_disposition() {
    for (field, severity, what) in [
        (
            Some("z"),
            CTOR_CONFORMANCE_SITE_SEVERITY,
            "an in-scope row with its param recovered",
        ),
        (
            None,
            CTOR_CONFORMANCE_SITE_SEVERITY,
            "an in-scope row whose param extraction missed",
        ),
        (Some("z"), "Warning", "an out-of-scope-severity inline row"),
    ] {
        let site = synth_inline_site(NAMED_SITE_HOST, field, severity);
        assert_eq!(
            disposition_of(&site),
            Disposition::InlineCensus,
            "{what} must resolve to the census disposition; anything else either \
             panics the generator or claims an owner that #7543 does not have"
        );
    }
}

/// The same site WITHOUT its snippet coordinate is still `Unattributed`.
///
/// Without this the census arm could be passing vacuously — resolving every site
/// it is handed, inline or not, and silently disarming γ's whole signal.
#[test]
fn the_census_disposition_is_keyed_on_the_snippet_coordinate_alone() {
    let mut site = synth_inline_site(
        "examples/definitely_not_waived_anywhere.ri",
        Some("z"),
        CTOR_CONFORMANCE_SITE_SEVERITY,
    );
    site.snippet_line = None;
    assert_eq!(
        disposition_of(&site),
        Disposition::Unattributed,
        "a tracked `.ri` row that no table names must still be actionable — the \
         census arm must key on `snippet_line`, nothing else"
    );
}

/// An inline row cannot MASK a genuinely stale waiver.
///
/// [`assert_no_unwaived_ctor_conformance_sites`] reads its waived set as
/// exactly the [`Disposition::Deferred`] rows. An inline row carrying a real
/// waiver entry's `(file, param)` must therefore NOT resolve to `Deferred` —
/// which is what fixes the census arm's position ahead of the table lookups
/// as well as ahead of the early returns. Placed after them, an inline row would
/// keep a landed task's entry looking live forever.
#[test]
fn an_inline_row_never_satisfies_a_waiver_entry() {
    let (residual_file, residual_param, ..) = CTOR_CONFORMANCE_CORPUS_RESIDUAL[0];
    let (debt_key, debt_param, _) =
        reify_test_support::ctor_conformance_debt::CTOR_CONFORMANCE_MIGRATION_DEBT[0];

    for (file, param) in [
        (residual_file.to_owned(), residual_param),
        (format!("{EXAMPLES_PREFIX}{debt_key}"), debt_param),
    ] {
        let site = synth_inline_site(&file, Some(param), CTOR_CONFORMANCE_SITE_SEVERITY);
        assert_eq!(
            disposition_of(&site),
            Disposition::InlineCensus,
            "an inline row at {file} :: param '{param}' must not be read as waived; \
             the waiver tables key on `.ri` files, and letting an inline row satisfy \
             one would keep a landed task's entry looking live forever"
        );
    }
}

/// One synthetic `.ri` site per waiver-table AND rejection-table entry, so the
/// STALE direction of [`assert_no_unwaived_ctor_conformance_sites`] is satisfied
/// and the UNEXPLAINED direction is the only thing a test below can trip.
///
/// A param-less rejection entry gets ε's arity wording, the only in-scope
/// wording that recovers no param.
#[cfg(test)]
fn waiver_satisfying_ri_sites() -> Vec<SurveySite> {
    let residual = CTOR_CONFORMANCE_CORPUS_RESIDUAL
        .iter()
        .map(|(file, param, ..)| synth_site(file, 1, "Waived", param, Owner::Unknown));
    let debt = reify_test_support::ctor_conformance_debt::CTOR_CONFORMANCE_MIGRATION_DEBT
        .iter()
        .map(|(key, param, _)| {
            synth_site(
                &format!("{EXAMPLES_PREFIX}{key}"),
                1,
                "Waived",
                param,
                Owner::Unknown,
            )
        });
    let rejection = CTOR_CONFORMANCE_REJECTION_FIXTURES
        .iter()
        .map(|(file, param, _)| {
            let mut site = synth_site(file, 1, "Widget", param.unwrap_or("—"), Owner::Unknown);
            if param.is_none() {
                site.field = None;
                site.code = "CtorArity".to_owned();
                site.message =
                    format!("{CTOR_ARITY_PREFIX}Widget() expects at most 1 argument, got 2");
            }
            site
        });
    residual.chain(debt).chain(rejection).collect()
}

/// Adding the inline half to a run that already satisfies every waiver leaves
/// [`assert_no_unwaived_ctor_conformance_sites`] passing.
///
/// The baseline half of this test is load-bearing: it proves the run WOULD pass
/// without the inline rows, so a failure after adding them is attributable to
/// them and to nothing else.
#[test]
fn the_unwaived_assertion_survives_the_inline_half() {
    let baseline = SurveyRun {
        total: 1,
        surveyed: 1,
        sites: waiver_satisfying_ri_sites(),
        ..SurveyRun::default()
    };
    assert_no_unwaived_ctor_conformance_sites(&baseline);

    let mut with_inline = baseline;
    with_inline.sites.extend([
        synth_inline_site(
            NAMED_SITE_HOST,
            Some("material"),
            CTOR_CONFORMANCE_SITE_SEVERITY,
        ),
        synth_inline_site(
            NAMED_SITE_HOST,
            Some("youngs_modulus"),
            CTOR_CONFORMANCE_SITE_SEVERITY,
        ),
        synth_inline_site(NAMED_SITE_HOST, None, CTOR_CONFORMANCE_SITE_SEVERITY),
    ]);
    assert_no_unwaived_ctor_conformance_sites(&with_inline);
}

/// [`Disposition::InlineCensus`] renders its own non-empty cell.
///
/// An empty or duplicated cell would make the artifact's disposition column
/// silently ambiguous about which half a row came from — the column exists to
/// tell a reader whether a row is work.
#[test]
fn the_census_disposition_renders_a_distinct_cell() {
    let census = Disposition::InlineCensus.label();
    assert!(
        !census.trim().is_empty(),
        "the census disposition must render a real cell, not a blank"
    );
    for other in [
        Disposition::Unattributed,
        Disposition::NotApplicable,
        Disposition::IntendedRejection,
        Disposition::Deferred {
            owning_task: "#5306",
            why: "any",
        },
    ] {
        assert_ne!(
            census,
            other.label(),
            "the census cell must be distinguishable from {other:?}"
        );
    }
}

/// A verbatim copy of the two PRE-δ `purpose_compile_tests.rs` fixtures that
/// task #7543's VERIFY names (`git show a8d7f5fb24:crates/reify-compiler/tests/
/// harness_compilation_surface/purpose_compile_tests.rs`, lines 1441-1576),
/// trimmed to each fixture's `let source` binding with nesting and indentation
/// kept.
///
/// Synthetic because δ fixed the live sites in f247bade44, retyping them to
/// `Real`, so post-δ main no longer carries them. The artifact committed at
/// 2f7cafa18a, generated at the pre-δ base, lists them as
/// `purpose_compile_tests.rs:1453/1454/1567`.
#[cfg(test)]
const PRE_DELTA_PURPOSE_FIXTURES_HOST: &str = r##"mod guarded {
    use super::*;

    #[test]
    fn guarded_where_arm_lowers_to_implies() {
        let source = r#"
structure Frame {
    param material : Length = 1.0
    param youngs_modulus : Length = 200.0
}

purpose p(subject : Structure) {
    where subject.material > 0.0 {
        constraint subject.youngs_modulus > 0.0
    }
}
"#;
    }

    #[test]
    fn guarded_else_arm_lowers_to_not_implies() {
        let source = r#"
structure Frame {
    param z : Length = 5.0
}

purpose p(subject : Structure) {
    where 0.0 > 1.0 {
    } else {
        constraint subject.z > 0.0
    }
}
"#;
    }
}
"##;

/// The inline sweep still sees the purpose_compile_tests sites that task
/// #7543's VERIFY names, at their host lines, and resolves each to the census.
///
/// A characterization witness that is independent of the live corpus. See
/// [`PRE_DELTA_PURPOSE_FIXTURES_HOST`] for why it is synthetic.
#[test]
fn survey_inline_corpus_still_sees_the_sites_task_7543_verify_names() {
    let host = PRE_DELTA_PURPOSE_FIXTURES_HOST;
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("purpose_compile_tests.rs"), host)
        .expect("write synthetic host");
    let run = survey_inline_corpus(dir.path(), &["purpose_compile_tests.rs".to_owned()]);

    let mut found: Vec<(&str, u32)> = run
        .sites
        .iter()
        .map(|s| (s.field.as_deref().unwrap_or("—"), s.line))
        .collect();
    found.sort();
    let mut expected = vec![
        ("material", host_line_of(host, "param material : Length")),
        (
            "youngs_modulus",
            host_line_of(host, "param youngs_modulus : Length"),
        ),
        ("z", host_line_of(host, "param z : Length")),
    ];
    expected.sort();
    assert_eq!(found, expected, "sites: {:#?}", run.sites);

    for site in &run.sites {
        assert_eq!(
            (site.expected.as_deref(), site.found.as_deref()),
            (Some("Scalar[m]"), Some("Real")),
            "a bare number at a `Length` param: {}",
            site.message
        );
        assert_eq!(disposition_of(site), Disposition::InlineCensus);
    }
}

/// True when `cite` is the repo's canonical `#NNNN` task-cite form.
///
/// Greek-letter aliases (`task ε`), PRD-relative indices (`task-5`) and prose
/// forms (`task 6941`) all resolve to `malformed-cite` under the repo's
/// TODO-citation convention, and a malformed cite is liveness-checkable by
/// nothing — which is the whole value of naming an owner.
fn is_canonical_task_cite(cite: &str) -> bool {
    cite.strip_prefix('#')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// Every [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] entry names a file that exists and
/// is a `.ri`.
///
/// Mirrors `ctor_conformance_migration_debt_entries_exist_under_examples_dir`:
/// cheap, and it separates a MIS-TYPED path from an ALREADY-RETIRED site. Both
/// would otherwise surface only as the corpus-wide staleness failure inside the
/// `#[ignore]`d generator, which reads as "already retired" and invites deleting
/// an entry that is still load-bearing.
#[test]
fn ctor_conformance_corpus_residual_entries_name_existing_ri_files() {
    for (path, param, owner, _why) in CTOR_CONFORMANCE_CORPUS_RESIDUAL {
        assert!(
            path.ends_with(".ri"),
            "CTOR_CONFORMANCE_CORPUS_RESIDUAL entry '{path}' (param '{param}', owner \
             {owner}) is not a `.ri` path"
        );
        let full = std::path::Path::new(WORKSPACE_ROOT).join(path);
        assert!(
            full.exists(),
            "CTOR_CONFORMANCE_CORPUS_RESIDUAL entry '{path}' (param '{param}', owner \
             {owner}) does not exist under {WORKSPACE_ROOT}"
        );
    }
}

/// Every [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] entry names its owner in the
/// canonical `#NNNN` cite form.
///
/// A deferral without a liveness-checkable owner is a permanent hole dressed up
/// as a temporary one — the failure mode `CTOR_CONFORMANCE_GATE_REMEDY`'s remedy
/// 3 forbids by name ("never without an owner").
#[test]
fn ctor_conformance_corpus_residual_entries_cite_a_canonical_task() {
    let malformed: Vec<String> = CTOR_CONFORMANCE_CORPUS_RESIDUAL
        .iter()
        .filter(|(_, _, owner, _)| !is_canonical_task_cite(owner))
        .map(|(path, param, owner, _)| format!("  {path} :: param '{param}'  (owner {owner})"))
        .collect();

    assert!(
        malformed.is_empty(),
        "CTOR_CONFORMANCE_CORPUS_RESIDUAL has {} entry/entries whose owner is not a \
         canonical `#NNNN` task cite:\n{}\n\n\
         A Greek-letter leaf label, a PRD-relative index or a `task NNNN` prose form is \
         a malformed cite: nothing can liveness-check it. Put the leaf label in the \
         `why` column and the task id in the owner column.",
        malformed.len(),
        malformed.join("\n"),
    );
}

/// [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] is sorted and duplicate-free on
/// `(path, param)`.
///
/// Sorted so a reader can find a site and a diff shows one line per change;
/// duplicate-free because the disposition resolver takes the FIRST match, so a
/// second entry for the same site would be silently unreachable — including one
/// naming a different owner.
#[test]
fn ctor_conformance_corpus_residual_is_sorted_and_duplicate_free() {
    let keys: Vec<(&str, &str)> = CTOR_CONFORMANCE_CORPUS_RESIDUAL
        .iter()
        .map(|(path, param, _, _)| (*path, *param))
        .collect();

    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(
        keys, sorted,
        "CTOR_CONFORMANCE_CORPUS_RESIDUAL must be sorted on (path, param)"
    );

    let mut deduped = sorted.clone();
    deduped.dedup();
    assert_eq!(
        sorted, deduped,
        "CTOR_CONFORMANCE_CORPUS_RESIDUAL must carry at most one entry per \
         (path, param); a second entry for the same site is unreachable"
    );
}

/// [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] and
/// [`CTOR_CONFORMANCE_MIGRATION_DEBT`](reify_test_support::ctor_conformance_debt::CTOR_CONFORMANCE_MIGRATION_DEBT)
/// describe DISJOINT sites.
///
/// The two tables are siblings, not a merge: one site described in both would
/// drift, and the resolver would have to pick a winner between two owners.
/// Compared after normalising the two key forms through
/// [`debt_entry_describes`], never by eyeballing the spellings.
#[test]
fn ctor_conformance_corpus_residual_is_disjoint_from_migration_debt() {
    let overlap: Vec<String> = CTOR_CONFORMANCE_CORPUS_RESIDUAL
        .iter()
        .filter(|(path, param, _, _)| {
            reify_test_support::ctor_conformance_debt::CTOR_CONFORMANCE_MIGRATION_DEBT
                .iter()
                .any(|entry| debt_entry_describes(entry, path, Some(param)))
        })
        .map(|(path, param, owner, _)| format!("  {path} :: param '{param}'  (owner {owner})"))
        .collect();

    assert!(
        overlap.is_empty(),
        "these site(s) are described by BOTH CTOR_CONFORMANCE_CORPUS_RESIDUAL and \
         CTOR_CONFORMANCE_MIGRATION_DEBT:\n{}\n\n\
         Pick one. CTOR_CONFORMANCE_MIGRATION_DEBT owns sites under examples/, because \
         the gate that consumes it walks examples_dir() only and its keys are relative to \
         that directory. CTOR_CONFORMANCE_CORPUS_RESIDUAL owns everything else.",
        overlap.join("\n"),
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// CTOR_CONFORMANCE_REJECTION_FIXTURES — the third table, and why it is a third
// ═════════════════════════════════════════════════════════════════════════════
//
// δ (#5306) committed five `.ri` under `tests/prd-gate/fixtures/` whose WHOLE
// CONTENT is a ctor-conformance violation: they are the PRD's §7 boundary-row
// rejection signals, and `tests/infra/test_prd_gate_struct_ctor_conformance.sh`
// asserts that `reify check` exits 1 on each. They are tracked, so the corpus
// sweep finds them; they are knob-governed and name a ctor argument, so
// `disposition_of` puts them IN SCOPE; and no waiver table names them, so before
// this table every one resolved `Unattributed` — the generator reported five
// UNEXPLAINED sites, telling the artifact's reader to go fix the very fixtures
// whose violation IS the deliverable.
//
// That is the same failure `Disposition::NotApplicable` was minted for, one axis
// over, and it takes a fourth arm rather than a fifth reading of an existing
// one: these sites ARE in scope (unlike `NotApplicable`) and nobody will ever
// retire them (unlike `Deferred`). The four dispositions map one-to-one onto
// four DIFFERENT reader actions, which is the property that makes the artifact
// worth reading at all.

/// The first [`CTOR_CONFORMANCE_REJECTION_FIXTURES`] entry that carries a param,
/// or `None` when the table holds none.
///
/// Borrowed rather than copied, for the reason
/// `render_survey_resolves_each_site_disposition_from_the_tables` states: a key
/// spelled a second time here would stale the moment a fixture is renamed, and
/// draining the table is a legitimate end state that must not red the gate.
fn first_rejection_entry_with_param() -> Option<&'static (&'static str, Option<&'static str>, &'static str)>
{
    CTOR_CONFORMANCE_REJECTION_FIXTURES
        .iter()
        .find(|(_, param, _)| param.is_some())
}

/// A listed `(file, param)` resolves [`Disposition::IntendedRejection`], and an
/// UNLISTED param in the SAME file still resolves [`Disposition::Unattributed`].
///
/// Both halves are load-bearing and the second is the non-vacuity guard: keyed on
/// the file alone this table would be a `SKIP_SET` by another name, silencing
/// every future ctor-conformance diagnostic a rejection fixture grows. A fixture
/// that acquires a SECOND, unintended violation must still be reported.
#[test]
fn intended_rejection_claims_the_listed_param_and_not_its_neighbours() {
    let Some(&(file, param, _)) = first_rejection_entry_with_param() else {
        println!("skipped: CTOR_CONFORMANCE_REJECTION_FIXTURES holds no param-keyed entry");
        return;
    };
    let param = param.expect("first_rejection_entry_with_param only yields Some");

    let listed = synth_site(file, 1, "Widget", param, Owner::Unknown);
    assert_eq!(
        disposition_of(&listed),
        Disposition::IntendedRejection,
        "a listed (file, param) must resolve IntendedRejection: {file} :: param '{param}'"
    );

    // A param no entry can name, asserted rather than assumed so this half cannot
    // go vacuous if the sentinel is ever added to the table.
    const UNLISTED: &str = "a_param_no_rejection_fixture_declares";
    assert!(
        !CTOR_CONFORMANCE_REJECTION_FIXTURES
            .iter()
            .any(|(f, p, _)| *f == file && *p == Some(UNLISTED)),
        "the sentinel param must stay absent from the table, or this guard proves nothing"
    );
    let neighbour = synth_site(file, 2, "Widget", UNLISTED, Owner::Unknown);
    assert_eq!(
        disposition_of(&neighbour),
        Disposition::Unattributed,
        "an UNLISTED param in a listed file must stay actionable — keyed on the file \
         alone this table would be a SKIP_SET, and a rejection fixture that grows a \
         SECOND, unintended violation would be silently swallowed"
    );
}

/// Pins [`names_a_ctor_argument`]'s KNOWN BLIND SPOT: a knob-governed
/// ctor-path leaf-trait or wrapper-shape site — the wording
/// `boundary13_option_trait_param_nonconforming_errors_trait_conformance`
/// measures on `Holder(mat: NotAMaterial())` — resolves
/// [`Disposition::NotApplicable`], because its prose is indistinguishable from
/// the fn-call path's.
///
/// This asserts the CURRENT classification, not the desired one. When a
/// structured ctor-vs-fn-call discriminator lands, this test is expected to go
/// red and be rewritten to assert the site is in scope.
#[test]
fn a_ctor_path_required_by_param_site_is_scoped_out_known_blind_spot() {
    for message in [
        "type 'NotAMaterial' does not conform to trait 'MaterialSpec' required by param 'mat'",
        "type 'Int' does not match wrapper shape required by param 'mat' (expected 'Option')",
    ] {
        let mut site = synth_site("examples/holder.ri", 1, "Holder", "mat", Owner::Unknown);
        site.code = "TypeNotConformingToTrait".to_owned();
        site.message = message.to_owned();
        assert_eq!(site.severity, CTOR_CONFORMANCE_SITE_SEVERITY);

        assert!(
            !names_a_ctor_argument(&site),
            "if this now holds, the blind spot is closed: update the doc and flip this test"
        );
        assert_eq!(
            disposition_of(&site),
            Disposition::NotApplicable,
            "ctor-path `required by param` sites are currently scoped out: {message}"
        );
    }
}

/// A listed fixture whose diagnostic recovers NO param still resolves
/// [`Disposition::IntendedRejection`].
///
/// This pins the LOOKUP ORDER inside [`disposition_of`], not merely a table row.
/// ε's `E_CTOR_ARITY` wording names a def and a count because there is no single
/// argument to name, so `site.field` is `None` and the `let … else` param
/// early-return fires. Placed after it, the rejection lookup could never reach
/// `struct_ctor_conformance_over_arity.ri` — which is exactly how that fixture
/// was stranded as an UNEXPLAINED site with `param '—'`.
#[test]
fn intended_rejection_reaches_a_site_whose_wording_names_no_argument() {
    let Some(&(file, _, _)) = CTOR_CONFORMANCE_REJECTION_FIXTURES
        .iter()
        .find(|(_, param, _)| param.is_none())
    else {
        println!("skipped: CTOR_CONFORMANCE_REJECTION_FIXTURES holds no param-less entry");
        return;
    };

    let mut site = synth_site(file, 1, "Widget", "ignored", Owner::Unknown);
    site.field = None;
    site.code = "CtorArity".to_owned();
    site.message = format!("{CTOR_ARITY_PREFIX}Widget() expects at most 1 argument, got 2");
    assert!(
        names_a_ctor_argument(&site),
        "the arity wording must stay IN SCOPE, or this test would pass off NotApplicable"
    );

    assert_eq!(
        disposition_of(&site),
        Disposition::IntendedRejection,
        "a param-less listed site must resolve IntendedRejection, which requires the \
         rejection lookup to precede the `site.field` early return: {file}"
    );
}

/// [`Disposition::IntendedRejection`]'s artifact cell reads distinctly from every
/// other disposition, and names no owner.
///
/// The cell is the whole interface to a reader consuming the artifact one row at
/// a time. Reading like `NotApplicable` would tell them the site is out of scope
/// when it is in scope; naming a task would send them hunting for work that does
/// not exist, because nothing retires these fixtures short of the survey module's
/// own deletion.
#[test]
fn intended_rejection_cell_is_distinct_and_names_no_owner() {
    let cell = Disposition::IntendedRejection.label();
    let deferred = Disposition::Deferred {
        owning_task: "#5847",
        why: "an owning task's reason",
    }
    .label();

    assert_ne!(cell, Disposition::NotApplicable.label());
    assert_ne!(cell, Disposition::Unattributed.label());
    assert_ne!(cell, deferred);
    assert!(
        !cell.contains('#'),
        "the IntendedRejection cell must name no task — there is nothing to retire, so a \
         cite would send the reader hunting for work that does not exist. Got: {cell:?}"
    );
}

/// Every [`CTOR_CONFORMANCE_REJECTION_FIXTURES`] entry names a file that exists
/// and is a `.ri`.
///
/// Mirrors `ctor_conformance_corpus_residual_entries_name_existing_ri_files`, and
/// for the same reason: it separates a MIS-TYPED path from a DELETED fixture,
/// which otherwise surface identically inside the `#[ignore]`d generator.
#[test]
fn ctor_conformance_rejection_fixtures_name_existing_ri_files() {
    for (path, param, _why) in CTOR_CONFORMANCE_REJECTION_FIXTURES {
        assert!(
            path.ends_with(".ri"),
            "CTOR_CONFORMANCE_REJECTION_FIXTURES entry '{path}' (param {param:?}) is not a \
             `.ri` path"
        );
        let full = std::path::Path::new(WORKSPACE_ROOT).join(path);
        assert!(
            full.exists(),
            "CTOR_CONFORMANCE_REJECTION_FIXTURES entry '{path}' (param {param:?}) does not \
             exist under {WORKSPACE_ROOT}"
        );
    }
}

/// [`CTOR_CONFORMANCE_REJECTION_FIXTURES`] describes sites DISJOINT from both
/// waiver tables.
///
/// Three sibling tables, not a merge. A site in two of them would make the
/// resolver pick a winner between "nobody will ever retire this" and "a live task
/// owns retiring this" — opposite claims about the same row.
#[test]
fn ctor_conformance_rejection_fixtures_are_disjoint_from_both_waiver_tables() {
    let overlap: Vec<String> = CTOR_CONFORMANCE_REJECTION_FIXTURES
        .iter()
        .filter(|(path, param, _)| {
            CTOR_CONFORMANCE_CORPUS_RESIDUAL
                .iter()
                .any(|(rp, rparam, _, _)| *rp == *path && Some(*rparam) == *param)
                || reify_test_support::ctor_conformance_debt::CTOR_CONFORMANCE_MIGRATION_DEBT
                    .iter()
                    .any(|entry| debt_entry_describes(entry, path, *param))
        })
        .map(|(path, param, _)| format!("  {path} :: param {param:?}"))
        .collect();

    assert!(
        overlap.is_empty(),
        "these site(s) are described by CTOR_CONFORMANCE_REJECTION_FIXTURES AND by a waiver \
         table:\n{}\n\n\
         Pick one. A rejection fixture is never retired and names no owner; a waiver entry \
         is retired by the task that owns it. A site cannot be both.",
        overlap.join("\n"),
    );
}

/// THE ANTI-SKIP_SET INVARIANT: every
/// [`CTOR_CONFORMANCE_REJECTION_FIXTURES`] file is the fixture of a probe in the
/// committed CLI probe-set that asserts `reify check` REJECTS it.
///
/// This is what keeps the table from becoming a place to hide an inconvenient
/// site. A file may be declared an intended rejection ONLY if a committed probe
/// independently asserts that it DOES reject — so every entry is backed by a gate
/// that reds if the rejection stops happening, and an entry can never silence a
/// site nothing else is watching.
///
/// The probe-set is PARSED, through
/// `reify_test_support::prd_gate_probe_set::fixtures_asserted_to_reject`, rather
/// than searched as text: a path mentioned only in a `capability` string, or the
/// fixture of a probe expecting exit 0 or the match `absent`, watches nothing
/// and must not satisfy this guard. That each probe also PASSES is
/// `tests/infra/test_prd_gate_struct_ctor_conformance.sh`'s half of the chain.
#[test]
fn every_rejection_fixture_is_asserted_by_a_committed_cli_probe() {
    let probe_set = std::path::Path::new(WORKSPACE_ROOT).join(REJECTION_PROBE_SET_REL);
    let text = std::fs::read_to_string(&probe_set).unwrap_or_else(|e| {
        panic!(
            "CTOR_CONFORMANCE_REJECTION_FIXTURES is only sound while its probe-set exists: \
             cannot read {}: {e}",
            probe_set.display()
        )
    });
    let asserted_rejections =
        reify_test_support::prd_gate_probe_set::fixtures_asserted_to_reject(&text)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", probe_set.display()));

    let unwatched: Vec<&str> = CTOR_CONFORMANCE_REJECTION_FIXTURES
        .iter()
        .map(|(path, _, _)| *path)
        .filter(|path| !asserted_rejections.contains(*path))
        .collect();

    assert!(
        unwatched.is_empty(),
        "these CTOR_CONFORMANCE_REJECTION_FIXTURES entries have NO probe in \
         {REJECTION_PROBE_SET_REL} asserting that `reify check` rejects them (a `check` \
         probe on that fixture expecting `present` with `exit_code: 1`):\n  {}\n\n\
         An entry declares `reify check` rejects this file ON PURPOSE. Without a probe \
         asserting the rejection, the declaration is unbacked and the table degrades into \
         a place to hide a site nobody is watching. Add the probe, or remove the entry and \
         fix the site.",
        unwatched.join("\n  "),
    );
}

/// Every [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] entry says WHY it is deferred.
///
/// The owner cite says who retires the site; the `why` says what would break if
/// someone "fixed" it instead. These fixtures are measured before-images whose
/// violation IS their content, so a reader who meets one without that sentence
/// has every reason to migrate it and delete another PRD's signal.
#[test]
fn ctor_conformance_corpus_residual_entries_say_why() {
    for (path, param, owner, why) in CTOR_CONFORMANCE_CORPUS_RESIDUAL {
        assert!(
            !why.trim().is_empty(),
            "CTOR_CONFORMANCE_CORPUS_RESIDUAL entry '{path}' :: param '{param}' (owner \
             {owner}) carries no reason; an unexplained deferral reads as an oversight"
        );
    }
}

/// Expiry guard, GATE-RESIDENT: every [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] entry
/// must still name a live site that [`disposition_of`] defers.
///
/// The `examples/`-keyed sibling has had this at gate cadence since α
/// (`ctor_conformance_migration_debt_entries_are_all_live`). Without the same
/// guard here, this table's only staleness check lives inside the `#[ignore]`d
/// `generate_ctor_conformance_corpus_survey`: when #6941 lands and retires its
/// six sites, the entries would rot until someone remembered to run an ignored
/// test — and a waiver that outlives its site is a permanent hole in the gate at a
/// `(file, param)` pair nobody is looking at any more. The four sibling hygiene
/// tests cannot see it: path existence, cite form, sort order and disjointness are
/// all satisfied by an entry whose site is gone.
///
/// # Cost
///
/// The entries name SEVEN distinct files, so this compiles seven `.ri` members
/// plus the cached stdlib prelude — the same order as
/// [`pinned_clean_files_emit_no_ctor_conformance_diagnostic`], and nowhere near
/// the 689-member corpus walk that keeps the generator `#[ignore]`d per
/// `docs/prds/merge-gate-compile-cost.md`. Deduplication assumes the table's
/// sortedness (its own test) only as an OPTIMISATION: an unsorted table compiles a
/// file twice, which costs time and cannot produce a false pass.
///
/// # `partial` is tolerated here, unlike the clean pin
///
/// These files are RED before-images, and `curvature_rad_literal.ri` carries
/// Error-severity diagnostics TODAY (its own header says so) — a partially
/// compiled member still contributes its ctor sites. No vacuity follows: this
/// assertion needs the sites to be PRESENT, so a file that stops contributing them
/// turns its entries stale and reds this test BY NAME. `not_surveyed` is still
/// asserted, not because it could hide a pass, but because "this fixture no longer
/// parses" and "this site was retired" want opposite fixes.
#[test]
fn ctor_conformance_corpus_residual_entries_are_all_live() {
    let mut corpus: Vec<String> = CTOR_CONFORMANCE_CORPUS_RESIDUAL
        .iter()
        .map(|(path, _, _, _)| (*path).to_owned())
        .collect();
    corpus.dedup();
    let run = survey_corpus(std::path::Path::new(WORKSPACE_ROOT), &corpus);

    assert!(
        run.not_surveyed.is_empty(),
        "every file named by CTOR_CONFORMANCE_CORPUS_RESIDUAL must reach the compile \
         phase, else its entries cannot be checked at all; unreachable: {:?}\n\n\
         These are committed before-images for other PRDs: a read or parse failure here \
         means the fixture was moved, renamed or broken, NOT that its site was retired.",
        run.not_surveyed,
    );

    let stale: Vec<String> = CTOR_CONFORMANCE_CORPUS_RESIDUAL
        .iter()
        .filter(|entry| {
            !run.sites.iter().any(|s| {
                s.file == entry.0
                    && s.field.as_deref() == Some(entry.1)
                    && matches!(disposition_of(s), Disposition::Deferred { .. })
            })
        })
        .map(|(path, param, owner, _)| format!("  {path} :: param '{param}'  (owner {owner})"))
        .collect();

    assert!(
        stale.is_empty(),
        "CTOR_CONFORMANCE_CORPUS_RESIDUAL has {} stale entry/entries — each defers no \
         live site:\n{}\n\n\
         The expected case is that the owning task landed and retired the site: DELETE \
         the entry, in that same diff. Two other causes stale MANY entries at once and \
         are fixed by neither deleting them nor touching the fixtures: param extraction \
         stopped matching the emitter's `argument '<name>'` wording (fix the \
         extraction), or `CTOR_FIELD_CONFORMANCE_SEVERITY` moved again and \
         CTOR_CONFORMANCE_SITE_SEVERITY did not move with it (re-point that const, the \
         one place that half of the scope is stated).",
        stale.len(),
        stale.join("\n"),
    );
}
