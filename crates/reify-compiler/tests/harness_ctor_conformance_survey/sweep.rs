//! The sweep that turns a list of corpus members — tracked `.ri` files, or Rust
//! hosts with embedded Reify snippets — into a [`SurveyRun`] of sites and
//! coverage.

use reify_test_support::is_ctor_conformance_code;
// The reusable inline-fixture walker (task #7543): `is_inline_fixture_host`
// decides what the second corpus half contains, and the collector/admission
// filter decide what an embedded snippet IS. It lives in reify-test-support
// rather than here because a Rust mini-lexer is a second-consumer shape, not a
// survey concern.
use reify_test_support::rust_fixture_scan;

use crate::corpus::{HostShape, NAMED_SITE_HOST, host_shape};
use crate::owner::{
    Owner, collect_structure_defs_into, d9_owner, fea_owned_defs, stdlib_structure_defs,
};
use crate::survey_site::{ARG_PREFIX, DefOrigin, SurveySite, survey_site_from_diagnostic};
use crate::workspace_git::WORKSPACE_ROOT;

// ─── step 9/10: end-to-end sweep over a synthetic mini-corpus ────────────────

/// The result of one pass over a corpus.
///
/// `surveyed + not_surveyed.len() == total` is an invariant: every member is
/// accounted for. The house "no silent caps" rule applies directly here — a
/// bounded sweep must state what it dropped, or the artifact reads as full
/// coverage and under-sizes γ.
#[derive(Debug, Default)]
pub(crate) struct SurveyRun {
    /// Every member handed in — the coverage denominator.
    pub(crate) total: usize,
    /// Members that reached the compile phase and contributed their sites.
    pub(crate) surveyed: usize,
    /// `(path, reason)` for members that contributed NO sites at all.
    /// Reasons: `read-error`, `parse-error`.
    pub(crate) not_surveyed: Vec<(String, String)>,
    /// `(path, reason)` for members that WERE surveyed but whose compile also
    /// produced Error-severity diagnostics OTHER than the ctor-conformance sites
    /// this sweep collects — their ctor sites are collected, but coverage of that
    /// file may be partial. Reason: `compile-error`.
    ///
    /// A separate bucket from `not_surveyed` on purpose: `compile_with_stdlib`
    /// is the SINGLE-FILE path (`reify check` instead uses
    /// `module_dag::compile_entry_with_stdlib_cfg_checked`, which follows
    /// `#cfg`-gated user imports), so every multi-module corpus member — the
    /// `examples/module_visibility/consumer.ri` class — lands here. Calling
    /// those "not surveyed" would understate coverage; calling them fully
    /// surveyed would overstate it. Naming them is the honest third option.
    ///
    /// The sweep's own signal is excluded because it hides nothing: δ (#5306)
    /// changed `CTOR_FIELD_CONFORMANCE_SEVERITY` and nothing else at the emit
    /// site, so a ctor-conformance Error suppresses exactly as little of the
    /// compile as the Warning it replaced — and the site it names is in
    /// [`SurveyRun::sites`], collected rather than lost. Counting it here would
    /// mark every member that carries a site as partially surveyed, which is the
    /// one claim this bucket exists to make truthfully.
    pub(crate) partial: Vec<(String, String)>,
    /// Every ctor-conformance site found, sorted `(file, line, field)`.
    pub(crate) sites: Vec<SurveySite>,
}

/// Sweep `rel_paths` (resolved against `root`) and collect every
/// ctor-conformance site.
///
/// Mirrors `examples_smoke.rs`'s `ctor_conformance_one` — read →
/// `parse_with_stdlib(&source, ModulePath::single(stem))` →
/// `compile_with_stdlib` → filter `compiled.diagnostics` by
/// `is_ctor_conformance_code`, the shared
/// `reify_test_support::ctor_conformance` predicate both read — so the survey
/// and the landed α corpus gate cannot disagree about what a ctor-conformance
/// site IS.
///
/// Two deliberate differences from that gate:
/// 1. The root widens from `examples/` to whatever corpus is handed in.
/// 2. Read and parse failures are RECORDED rather than panicked-on or silently
///    `return`ed. The non-examples corpus contains many intentionally
///    unparseable fixtures, and omitting them would inflate apparent coverage.
///
/// D9 owner assignment is a SECOND pass, after the loop: a site in the first
/// swept file may construct a def declared in the last one, so the known-def set
/// has to be complete before any row is classified. Classifying inline would
/// make a row's owner depend on the order members were handed in — the exact
/// non-determinism the artifact's byte-reproducibility rules out.
pub(crate) fn survey_corpus(root: &std::path::Path, rel_paths: &[String]) -> SurveyRun {
    // Seeded with the stdlib so a site constructing a stdlib def resolves even
    // when the declaring stdlib file is not part of the corpus handed in; every
    // swept member then contributes its own declarations.
    let mut structure_defs = stdlib_structure_defs().clone();
    let mut run = SurveyRun {
        total: rel_paths.len(),
        ..SurveyRun::default()
    };

    for rel in rel_paths {
        let path = root.join(rel);
        let Ok(source) = std::fs::read_to_string(&path) else {
            run.not_surveyed
                .push((rel.clone(), "read-error".to_owned()));
            continue;
        };
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let sweep = sweep_member(rel, &source, &stem, &mut structure_defs);
        run.record(rel.clone(), sweep);
    }

    finish_run(&mut run, &structure_defs);
    run
}

/// What sweeping ONE corpus member yields, whichever half it came from.
enum MemberSweep {
    /// The member did not parse, so it contributes no sites.
    ParseError,
    /// The member compiled. `partial` is set when an Error-severity diagnostic
    /// OTHER than the sweep's own ctor-conformance signal fired — see
    /// [`SurveyRun::partial`] for why that signal is excluded.
    Compiled {
        sites: Vec<SurveySite>,
        partial: bool,
    },
}

/// The per-member pipeline BOTH corpus halves run, stated once so the halves
/// cannot drift: harvest `structure def`s → `parse_with_stdlib` →
/// `compile_with_stdlib` → the partial rule → [`survey_site_from_diagnostic`]
/// over every [`is_ctor_conformance_code`] diagnostic.
///
/// Declarations are harvested from the raw text BEFORE the parse gate: a member
/// that fails to parse can still legitimately declare a def that another member
/// constructs, and dropping it would demote that other member's rows to
/// `UnresolvedDef` for no reason.
///
/// A site's `line` is relative to `source`; a caller whose member is embedded
/// in a larger file maps it.
fn sweep_member(
    file: &str,
    source: &str,
    stem: &str,
    structure_defs: &mut std::collections::BTreeSet<String>,
) -> MemberSweep {
    use reify_compiler::{compile_with_stdlib, parse_with_stdlib};
    use reify_core::{ModulePath, Severity};

    collect_structure_defs_into(source, structure_defs);

    let parsed = parse_with_stdlib(source, ModulePath::single(stem));
    if !parsed.errors.is_empty() {
        return MemberSweep::ParseError;
    }

    let compiled = compile_with_stdlib(&parsed);
    let partial = compiled
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && !is_ctor_conformance_code(d.code));
    let sites = compiled
        .diagnostics
        .iter()
        .filter(|d| is_ctor_conformance_code(d.code))
        .filter_map(|d| survey_site_from_diagnostic(file, source, d))
        .collect();
    MemberSweep::Compiled { sites, partial }
}

impl SurveyRun {
    /// Account for one swept member under `member`, its coverage key.
    fn record(&mut self, member: String, sweep: MemberSweep) {
        match sweep {
            MemberSweep::ParseError => {
                self.not_surveyed.push((member, "parse-error".to_owned()));
            }
            MemberSweep::Compiled { sites, partial } => {
                self.surveyed += 1;
                if partial {
                    self.partial.push((member, "compile-error".to_owned()));
                }
                self.sites.extend(sites);
            }
        }
    }
}

/// The tail BOTH corpus halves run once every member is swept.
///
/// D9 owner assignment is a second pass because a site in the first swept
/// member may construct a def declared in the last one. The total order makes
/// the artifact byte-reproducible regardless of the order members were handed
/// in; `code` and `message` break the remaining ties so two sites at the same
/// `(file, line, field)` still sort deterministically.
fn finish_run(run: &mut SurveyRun, structure_defs: &std::collections::BTreeSet<String>) {
    let fea = fea_owned_defs();
    for site in &mut run.sites {
        site.owner = d9_owner(site.def.as_deref(), fea, structure_defs);
    }
    run.sites.sort_by(|a, b| {
        (&a.file, a.line, &a.field, &a.code, &a.message)
            .cmp(&(&b.file, b.line, &b.field, &b.code, &b.message))
    });
    run.not_surveyed.sort();
    run.partial.sort();
}

/// The known-SITE member: PRD §7 boundary-test row 2, reused verbatim from
/// `struct_ctor_field_conformance_tests.rs`'s `SOURCE_ROW2_VALUE_CELL_STRING`.
///
/// Using α's own landed fixture means the end-to-end test asserts against a
/// site shape the compiler is ALREADY proven to emit — the premise is verified
/// live on `main`, not guessed.
#[cfg(test)]
const SYNTH_CONFORMANCE_SITE: &str = "module test.row2\n\
     structure def Widget { param label : String }\n\
     structure def Root {\n\
     \x20   let x = Widget(label: 42)\n\
     }\n";

/// The known-CLEAN member: the same source with a conforming argument.
#[cfg(test)]
const SYNTH_CLEAN: &str = "module test.clean\n\
     structure def Widget { param label : String }\n\
     structure def Root {\n\
     \x20   let x = Widget(label: \"ok\")\n\
     }\n";

/// The known-UNPARSEABLE member. The tracked corpus really does contain
/// deliberately-unparseable negative fixtures and tree-sitter parser-corpus
/// inputs, so the sweep must record them rather than panic.
#[cfg(test)]
const SYNTH_BROKEN: &str = "module test.broken\n((( this is not reify at all ]]] §§§\n";

/// The known-COMPILE-ERROR member: parses cleanly, then emits an Error-severity
/// diagnostic (`unresolved name: no_such_binding`).
///
/// This is the `partial` bucket's only coverage. Roughly a tenth of the tracked
/// corpus lands there — every multi-module file the single-file
/// `compile_with_stdlib` path cannot resolve; the artifact header carries the
/// exact figure, which drifts as the corpus grows — yet without this member the
/// synthetic sweep never populates `run.partial` at all, and a regression that
/// stopped filling it (or that moved compile-error files into `not_surveyed`,
/// breaking the coverage arithmetic) would go undetected while every other test
/// stayed green.
#[cfg(test)]
const SYNTH_COMPILE_ERROR: &str = "module test.compile_error\n\
     structure def Root {\n\
     \x20   let x = no_such_binding + 1\n\
     }\n";

// ─── prose-contract members: the extractors, against the LIVE emitters ───────
//
// `SYNTH_CONFORMANCE_SITE` above pins `ArgTypeMismatch`'s `argument '<f>'` /
// `expected '<X>', got '<Y>'` shapes end-to-end. Every OTHER admitted wording
// was pinned only against `synth(...)` fixtures whose message strings are
// hand-written in this survey — a presumed copy of what the emitters produce, not
// a measurement of it. An emitter rewording would leave all of those green
// while `def_of_diagnostic` / `field_of_message` silently stopped recovering on
// real input, and the committed artifact carries zero ε rows today, so nothing
// else would surface it either.
//
// This is the same failure the survey already fixed once for the type table:
// `selector_type_renderings_match_what_reify_core_actually_displays` pins
// `is_selector_type` by CONSTRUCTING `reify_core::Type` values after an earlier
// draft matched a `Selector(Face)` string the compiler never emits.
//
// These three members close the gap for the remaining shapes at the same
// near-zero cost — three small files through the real parse→compile pipeline.
// They are deliberately a SEPARATE corpus rather than extra `synth_corpus()`
// members: `survey_corpus_finds_the_known_conformance_site_with_every_column_resolved`
// asserts "exactly one ctor-conformance site across the mini-corpus", and the
// coverage-arithmetic tests assert `run.total == 4`. Diluting those to carry
// prose coverage would weaken guards that exist for a different reason.

/// ε `CtorUnknownField`, live: the `in call to '<Def>'` def prose and the
/// `unknown named argument '<f>'` field prose in one message.
#[cfg(test)]
const PROSE_CTOR_UNKNOWN_FIELD: &str = "module test.prose_unknown_field\n\
     structure def Widget { param label : String }\n\
     structure def Root {\n\
     \x20   let x = Widget(nosuchfield: \"v\")\n\
     }\n";

/// ε `CtorArity`, live: the `E_CTOR_ARITY: <Def>() expects …` def prose. Its
/// wording names no param, so the FIELD column must stay `None` — the one
/// admitted code for which that is the correct answer rather than a miss.
#[cfg(test)]
const PROSE_CTOR_ARITY: &str = "module test.prose_arity\n\
     structure def Widget { param label : String }\n\
     structure def Root {\n\
     \x20   let x = Widget(\"a\", \"b\")\n\
     }\n";

/// `TypeNotConformingToTrait`'s `required by param '<f>'` shape, live.
///
/// The `sub =` binding and the empty `trait Fastener {}` are both load-bearing:
/// this is the shape `m9_error_cases.rs`'s `type_does_not_conform_to_trait`
/// proves the compiler emits, and a `let` binding of a trait-param ctor does
/// NOT reach the check (measured — an earlier draft of this fixture produced no
/// diagnostic at all and would have been a silently vacuous test).
#[cfg(test)]
const PROSE_REQUIRED_BY_PARAM: &str = "module test.prose_required_by_param\n\
     trait Fastener {}\n\
     structure def Bolt { param dia : Real = 3.0 }\n\
     structure def Holder { param part : Fastener }\n\
     structure def Root { sub h = Holder(part: Bolt()) }\n";

/// Write the three prose-contract members into a temp dir and sweep them.
#[cfg(test)]
fn prose_contract_run() -> SurveyRun {
    let dir = tempfile::tempdir().expect("tempdir");
    let members = [
        ("arity.ri", PROSE_CTOR_ARITY),
        ("required_by_param.ri", PROSE_REQUIRED_BY_PARAM),
        ("unknown_field.ri", PROSE_CTOR_UNKNOWN_FIELD),
    ];
    for (name, source) in members {
        std::fs::write(dir.path().join(name), source).expect("write prose-contract member");
    }
    let paths: Vec<String> = members.iter().map(|(n, _)| (*n).to_owned()).collect();
    survey_corpus(dir.path(), &paths)
}

#[test]
fn epsilon_and_required_by_param_prose_extractors_hold_against_the_live_emitters() {
    let run = prose_contract_run();

    // Every member must actually COMPILE and yield its site. A fixture that
    // stopped reaching its emitter would otherwise make each assertion below
    // vacuous rather than red.
    assert_eq!(
        run.not_surveyed,
        Vec::new(),
        "every prose-contract member must parse; a fixture that stopped compiling \
         would make this whole test vacuous"
    );
    assert_eq!(
        run.sites.len(),
        3,
        "one site per prose-contract member — a missing one means that emitter no \
         longer produces the shape this test claims to pin: {:#?}",
        run.sites
    );

    let site = |file: &str| {
        run.sites
            .iter()
            .find(|s| s.file == file)
            .unwrap_or_else(|| panic!("no site for {file}; got {:#?}", run.sites))
    };

    // ε `CtorUnknownField`: BOTH prose extractors, measured.
    let unknown = site("unknown_field.ri");
    assert_eq!(unknown.code, "CtorUnknownField");
    assert_eq!(
        unknown.def.as_deref(),
        Some("Widget"),
        "the `in call to '<Def>'` prose must still yield the def"
    );
    assert_eq!(
        unknown.def_origin,
        DefOrigin::DiagnosticProse,
        "the def must come from the PROSE path, not the call-site anchor — if the \
         wording drifts, this is where it shows up"
    );
    assert_eq!(
        unknown.field.as_deref(),
        Some("nosuchfield"),
        "the `unknown named argument '<f>'` prose must still yield the field"
    );

    // ε `CtorArity`: def from prose, and field CORRECTLY absent.
    let arity = site("arity.ri");
    assert_eq!(arity.code, "CtorArity");
    assert_eq!(
        arity.def.as_deref(),
        Some("Widget"),
        "the `E_CTOR_ARITY: <Def>()` prose must still yield the def"
    );
    assert_eq!(arity.def_origin, DefOrigin::DiagnosticProse);
    assert_eq!(
        arity.field, None,
        "this wording names no param; a field here would be a fabrication, not a \
         recovery"
    );

    // `required by param '<f>'` — reached ONLY through `field_of_message`'s
    // `or_else` fallback, because the message carries no `argument '`. This is
    // the live-emitter half of the fallback-liveness contract that
    // `an_empty_quoted_token_is_a_miss_so_the_fallback_prefix_is_still_consulted`
    // pins structurally.
    let required = site("required_by_param.ri");
    assert_eq!(required.code, "TypeNotConformingToTrait");
    assert!(
        !required.message.contains(ARG_PREFIX),
        "premise: this shape must NOT carry `argument '`, or it would be recovered \
         by the first prefix and the fallback would go untested; got {:?}",
        required.message
    );
    assert_eq!(
        required.field.as_deref(),
        Some("part"),
        "the `required by param '<f>'` FALLBACK must still yield the field"
    );
    // Measured, and stated rather than editorialised: the label anchors at the
    // ARGUMENT's ctor (`Bolt()`), so the call-site anchor recovers the argument
    // type, not the receiving def (`Holder`). That is the survey's documented
    // "def is whatever identifier sits at the anchor" caveat, observed live.
    assert_eq!(required.def.as_deref(), Some("Bolt"));
    assert_eq!(required.def_origin, DefOrigin::CallSiteAnchor);
}

/// Write the three synthetic members into a temp dir and return `(dir, paths)`.
#[cfg(test)]
fn synth_corpus() -> (tempfile::TempDir, Vec<String>) {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, source) in [
        ("conformance_site.ri", SYNTH_CONFORMANCE_SITE),
        ("clean.ri", SYNTH_CLEAN),
        ("broken.ri", SYNTH_BROKEN),
        ("compile_error.ri", SYNTH_COMPILE_ERROR),
    ] {
        std::fs::write(dir.path().join(name), source).expect("write synthetic member");
    }
    let paths = vec![
        "broken.ri".to_owned(),
        "clean.ri".to_owned(),
        "compile_error.ri".to_owned(),
        "conformance_site.ri".to_owned(),
    ];
    (dir, paths)
}

#[test]
fn survey_corpus_finds_the_known_conformance_site_with_every_column_resolved() {
    let (dir, paths) = synth_corpus();
    let run = survey_corpus(dir.path(), &paths);

    assert_eq!(
        run.sites.len(),
        1,
        "exactly one ctor-conformance site across the mini-corpus, got: {:#?}",
        run.sites
    );
    let site = &run.sites[0];
    assert_eq!(site.file, "conformance_site.ri");
    assert_eq!(site.code, "ArgTypeMismatch");
    assert_eq!(
        site.severity, "Error",
        "δ flipped the knob to Error — the sweep must report what it MEASURED, never \
         assume the severity the signal was first scoped to"
    );
    assert_eq!(site.field.as_deref(), Some("label"));
    assert_eq!(site.def.as_deref(), Some("Widget"));
    assert_eq!(site.expected.as_deref(), Some("String"));
    assert_eq!(site.found.as_deref(), Some("Int"));
    assert_eq!(
        site.line, 4,
        "the ctor is on line 4 of the fixture; got line {} for {:?}",
        site.line, site.message
    );
    assert_eq!(
        site.owner,
        Owner::NonFea,
        "`Widget` is not an FEA stdlib def — the sweep must CLASSIFY, not leave Unknown"
    );
}

#[test]
fn survey_corpus_records_unsurveyable_members_instead_of_dropping_them() {
    let (dir, paths) = synth_corpus();
    let run = survey_corpus(dir.path(), &paths);

    assert_eq!(run.total, 4, "the denominator is every file handed in");
    let broken: Vec<&(String, String)> = run
        .not_surveyed
        .iter()
        .filter(|(f, _)| f == "broken.ri")
        .collect();
    assert_eq!(
        broken.len(),
        1,
        "the unparseable member must be RECORDED, not silently dropped: {:#?}",
        run.not_surveyed
    );
    assert_eq!(
        broken[0].1, "parse-error",
        "its reason must name the phase that failed"
    );

    // "No silent caps": the coverage denominator has to be visible, or γ is
    // sized against a survey that reads as full coverage but is not.
    assert_eq!(
        run.surveyed + run.not_surveyed.len(),
        run.total,
        "surveyed + not_surveyed must account for every corpus member"
    );
    assert_eq!(
        run.surveyed, 3,
        "the three parseable members are surveyed — including the one that then \
         failed to COMPILE, which is partial coverage, not zero coverage"
    );
}

#[test]
fn survey_corpus_records_a_compile_error_member_as_partial_not_missing() {
    let (dir, paths) = synth_corpus();
    let run = survey_corpus(dir.path(), &paths);

    assert_eq!(
        run.partial,
        vec![("compile_error.ri".to_owned(), "compile-error".to_owned())],
        "a member that PARSES and then emits an Error-severity diagnostic belongs in \
         `partial` with its reason named. Roughly a tenth of the tracked corpus lands \
         here — every multi-module file the single-file `compile_with_stdlib` path \
         cannot resolve; the artifact header carries the exact count — so an empty \
         `partial` on the real corpus would be a silent coverage overstatement: {:#?}",
        run.partial
    );

    // The three-way split has to stay consistent, or the artifact's coverage
    // arithmetic (`surveyed + not_surveyed == total`) stops adding up.
    assert!(
        run.not_surveyed
            .iter()
            .all(|(f, _)| f != "compile_error.ri"),
        "a partially-surveyed member must NOT also be listed as not-surveyed — that \
         would double-count it and break the coverage denominator: {:#?}",
        run.not_surveyed
    );
    assert_eq!(
        run.surveyed + run.not_surveyed.len(),
        run.total,
        "`partial` is a QUALIFIER on surveyed members, never a fourth disjoint bucket"
    );
    assert!(
        run.partial.iter().all(|(f, _)| f != "broken.ri"),
        "a member that never reached the compile phase cannot be `partial`: {:#?}",
        run.partial
    );
}

#[test]
fn survey_corpus_does_not_leak_prelude_diagnostics_into_every_file() {
    // If stdlib-prelude diagnostics were re-attributed to each swept file, the
    // artifact would inflate ~660x and be worthless. Two distinct files, each
    // yielding ONLY its own sites, is the cheap pin on that.
    let (dir, paths) = synth_corpus();
    let run = survey_corpus(dir.path(), &paths);

    let clean_sites = run.sites.iter().filter(|s| s.file == "clean.ri").count();
    assert_eq!(
        clean_sites, 0,
        "the conforming member must contribute ZERO sites; prelude leakage would \
         give it the same site count as every other file"
    );

    // Sweeping the clean member ALONE must likewise be empty.
    let solo = survey_corpus(dir.path(), &["clean.ri".to_owned()]);
    assert!(
        solo.sites.is_empty(),
        "a clean file swept alone must yield no sites, got: {:#?}",
        solo.sites
    );
    assert_eq!(solo.surveyed, 1);
    assert_eq!(solo.total, 1);
}

#[test]
fn survey_corpus_records_a_read_error_rather_than_panicking() {
    let (dir, _) = synth_corpus();
    let run = survey_corpus(dir.path(), &["no_such_file.ri".to_owned()]);
    assert_eq!(run.total, 1);
    assert_eq!(run.surveyed, 0);
    assert_eq!(
        run.not_surveyed,
        vec![("no_such_file.ri".to_owned(), "read-error".to_owned())],
        "an unreadable member is recorded with its reason, never a panic that \
         would abort a 660-file sweep"
    );
}

#[test]
fn survey_corpus_orders_sites_deterministically() {
    // Byte-reproducibility of the artifact starts here: the same corpus handed
    // in a different order must produce the same site list.
    let (dir, paths) = synth_corpus();
    let forward = survey_corpus(dir.path(), &paths);
    let mut reversed = paths.clone();
    reversed.reverse();
    let backward = survey_corpus(dir.path(), &reversed);
    assert_eq!(
        forward.sites, backward.sites,
        "site ordering must not depend on the order files are handed in"
    );
}

// ─── the second half: Reify snippets embedded in Rust test source ────────────

/// Sweep the Reify snippets embedded in `host_rel_paths` (resolved against
/// `root`) and collect every ctor-conformance site.
///
/// Calls the SAME [`sweep_member`] per member and the same [`finish_run`] tail
/// that [`survey_corpus`] calls, so the two corpus halves cannot disagree about
/// what a ctor-conformance site IS or about which member is partial. Exactly
/// two things differ: what a MEMBER is, and how a diagnostic's line is mapped.
///
/// # A member is a snippet, not a host file
///
/// The coverage denominator counts SNIPPETS, plus one entry for a host that
/// could not be read at all. Counting hosts instead would report a host
/// carrying ten snippets, one of which failed to parse, as fully surveyed.
/// A raw string that is not Reify at all (JSON, a Rust-source fixture) is not a
/// member and is not counted: there is nothing to survey and nothing to
/// disclose. A Reify-SHAPED `format!` template IS a member — it is Reify a
/// reader would expect the census to cover — and is recorded under its own
/// `format-template` reason rather than left to land as a noise `parse-error`.
///
/// # Position
///
/// A row's `file`/`line` is the HOST `.rs` position a human opens; the
/// snippet-relative coordinate is kept alongside in
/// [`SurveySite::snippet_line`]. [`survey_site_from_diagnostic`] is handed the
/// SNIPPET text, so the line it computes is snippet-relative and is mapped up
/// with `host_line + snippet_line - 1` —
/// [`rust_fixture_scan::InlineSnippet::host_line`] is the host line of the
/// snippet's line 1, which makes that mapping uniform.
///
/// # Not `reify_test_support::compile_source_with_stdlib`
///
/// That helper PANICS on parse errors. Recording them is the whole point of the
/// coverage accounting here — inline fixtures include deliberately-unparseable
/// negative cases, and a panic would take the sweep down with them.
///
/// A snippet carrying no `module` declaration compiles under
/// `ModulePath::single(<host stem>)` and emits `W_MODULE_DECL_MISSING`, a
/// non-ctor code the shared [`is_ctor_conformance_code`] filter already drops.
pub(crate) fn survey_inline_corpus(root: &std::path::Path, host_rel_paths: &[String]) -> SurveyRun {
    let mut structure_defs = stdlib_structure_defs().clone();
    let mut run = SurveyRun::default();

    for rel in host_rel_paths {
        let path = root.join(rel);
        let Ok(host_source) = std::fs::read_to_string(&path) else {
            run.total += 1;
            run.not_surveyed
                .push((rel.clone(), "read-error".to_owned()));
            continue;
        };
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        let scan = rust_fixture_scan::inline_ri_snippets(&host_source);
        run.total += scan.snippets.len() + scan.format_templates.len();
        for template in &scan.format_templates {
            run.not_surveyed.push((
                format!("{rel}:{}", template.host_line),
                INLINE_TEMPLATE_REASON.to_owned(),
            ));
        }

        for snippet in &scan.snippets {
            let mut sweep = sweep_member(rel, &snippet.text, &stem, &mut structure_defs);
            if let MemberSweep::Compiled { sites, .. } = &mut sweep {
                for site in sites {
                    let snippet_line = site.line;
                    site.snippet_line = Some(snippet_line);
                    site.line = snippet.host_line + snippet_line - 1;
                }
            }
            run.record(format!("{rel}:{}", snippet.host_line), sweep);
        }
    }

    finish_run(&mut run, &structure_defs);
    run
}

/// The coverage reason a `format!` template is recorded under.
///
/// One spelling, because `push_inline_limitation` COUNTS the rows carrying it
/// to state how many snippets the walker reached: a drifted spelling there would
/// silently turn a real figure into zero.
pub(crate) const INLINE_TEMPLATE_REASON: &str = "format-template";

/// One synthetic Rust host carrying, in order: an admitted Reify snippet that
/// WARNS, a non-Reify blob, a Reify-shaped snippet that cannot parse, a
/// `format!` template, and a Reify-shaped snippet hidden inside a doc comment.
///
/// The outer literal needs a DOUBLED hash count because the warning snippet
/// already uses `r##"` (the shape `crates/reify-eval/src/engine_build/tests.rs`
/// uses live).
#[cfg(test)]
const INLINE_SYNTH_HOST: &str = r####"// A synthetic host for the inline sweep.
fn warns() {
    let source = r##"
structure def W {
    param z : Length = 5.0
}
"##;
    let _ = source;
}

fn not_reify() {
    let json = r#"{"capabilities":{}}"#;
    let _ = json;
}

fn unparseable() {
    let broken = r#"
module test.inline_broken
((( this is not reify at all ]]] §§§
"#;
    let _ = broken;
}

fn templated() {
    let t = format!(r#"
structure def T {{
    param q : Length = {WIDTH}
}}
"#);
    let _ = t;
}

/// A doc comment carrying r#"structure def Ghost { param g : Length = 9.0 }"#
/// must contribute nothing at all.
fn documented() {}
"####;

/// The 1-based line of the ONE line of `host` containing `needle`.
///
/// Every expected host line below is COMPUTED with this rather than
/// hand-counted, so a fixture edit fails on its anchor instead of silently
/// invalidating an expectation.
#[cfg(test)]
pub(crate) fn host_line_of(host: &str, needle: &str) -> u32 {
    let hits: Vec<u32> = host
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(needle))
        .map(|(i, _)| i as u32 + 1)
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "fixture anchor {needle:?} must appear on exactly one line, found {hits:?}"
    );
    hits[0]
}

/// Write [`INLINE_SYNTH_HOST`] into a temp dir and return
/// `(dir, [host, missing host])` — the same tempfile idiom as [`synth_corpus`].
#[cfg(test)]
fn inline_synth_corpus() -> (tempfile::TempDir, Vec<String>) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("host.rs"), INLINE_SYNTH_HOST).expect("write synthetic host");
    (dir, vec!["absent_host.rs".to_owned(), "host.rs".to_owned()])
}

#[test]
fn survey_inline_corpus_finds_the_snippet_site_at_its_host_position() {
    let (dir, hosts) = inline_synth_corpus();
    let run = survey_inline_corpus(dir.path(), &hosts);

    assert_eq!(
        run.sites.len(),
        1,
        "exactly one ctor-conformance site across the synthetic host, got: {:#?}",
        run.sites
    );
    let site = &run.sites[0];

    assert_eq!(
        site.file, "host.rs",
        "a row's `file` is the HOST .rs path a human opens, not a synthesised snippet name"
    );
    assert_eq!(
        site.line,
        host_line_of(INLINE_SYNTH_HOST, "param z : Length"),
        "a row's `line` is the HOST line of the offending declaration"
    );
    let snippet_line = site
        .snippet_line
        .expect("an inline row must carry its snippet-relative coordinate");
    let snippet_start = host_line_of(INLINE_SYNTH_HOST, "structure def W {");
    assert_eq!(
        snippet_start + snippet_line - 1,
        site.line,
        "host_line_of_snippet_start + snippet_line - 1 must reconstruct the host line"
    );
    assert_eq!(site.field.as_deref(), Some("z"));
    assert_eq!(
        site.owner,
        d9_owner(
            site.def.as_deref(),
            fea_owned_defs(),
            stdlib_structure_defs()
        ),
        "the inline half must classify by the same `d9_owner` the .ri half uses"
    );
}

#[test]
fn survey_inline_corpus_records_every_unsurveyable_snippet_with_its_reason() {
    let (dir, hosts) = inline_synth_corpus();
    let run = survey_inline_corpus(dir.path(), &hosts);

    let reason_for = |key: &str| -> Option<&str> {
        run.not_surveyed
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, r)| r.as_str())
    };

    let broken_key = format!(
        "host.rs:{}",
        host_line_of(INLINE_SYNTH_HOST, "module test.inline_broken")
    );
    assert_eq!(
        reason_for(&broken_key),
        Some("parse-error"),
        "an unparseable snippet is RECORDED at `<host>:<line>`, never dropped; \
         not_surveyed = {:#?}",
        run.not_surveyed
    );

    let template_key = format!(
        "host.rs:{}",
        host_line_of(INLINE_SYNTH_HOST, "structure def T {{")
    );
    assert_eq!(
        reason_for(&template_key),
        Some("format-template"),
        "a `format!` template is disclosed under its own reason rather than \
         landing as a noise parse-error; not_surveyed = {:#?}",
        run.not_surveyed
    );

    assert_eq!(
        reason_for("absent_host.rs"),
        Some("read-error"),
        "an unreadable host is recorded rather than panicking the sweep"
    );

    assert_eq!(
        run.total,
        run.surveyed + run.not_surveyed.len(),
        "the coverage denominator must still account for every member exactly once"
    );
}

#[test]
fn survey_inline_corpus_orders_sites_deterministically() {
    let (dir, hosts) = inline_synth_corpus();
    let forward = survey_inline_corpus(dir.path(), &hosts);
    let mut reversed = hosts.clone();
    reversed.reverse();
    let backward = survey_inline_corpus(dir.path(), &reversed);
    assert_eq!(
        forward.sites, backward.sites,
        "site ordering must not depend on the order hosts are handed in"
    );
    assert_eq!(forward.not_surveyed, backward.not_surveyed);
}

/// The SAME member text, swept once as a tracked `.ri` file and once as an
/// inline snippet, classifies identically in both halves: the same coverage
/// bucket and the same site columns.
///
/// This pins the property rather than one predicate. δ's step 7 (f16a387d06)
/// changed the `.ri` sweep's partial rule so that the sweep's own
/// ctor-conformance Error no longer marks a member partial, and the inline sweep,
/// a copy of that loop, silently kept the old rule. Any future drift in either
/// half reds here.
#[test]
fn both_corpus_halves_classify_a_member_identically() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut host = String::new();
    for (name, text) in [
        ("site", SYNTH_CONFORMANCE_SITE),
        ("err", SYNTH_COMPILE_ERROR),
    ] {
        std::fs::write(dir.path().join(format!("{name}.ri")), text).expect("write .ri member");
        host.push_str(&format!(
            "fn {name}_member() {{\n    let source = r#\"\n{text}\"#;\n    let _ = source;\n}}\n\n"
        ));
    }
    std::fs::write(dir.path().join("host.rs"), &host).expect("write synthetic host");
    assert_eq!(
        rust_fixture_scan::inline_ri_snippets(&host).snippets.len(),
        2,
        "both members must be admitted as inline snippets, or the inline half of \
         this comparison is vacuous:\n{host}"
    );

    let ri = survey_corpus(dir.path(), &["err.ri".to_owned(), "site.ri".to_owned()]);
    let inline = survey_inline_corpus(dir.path(), &["host.rs".to_owned()]);

    assert_eq!(
        ri.partial,
        vec![("err.ri".to_owned(), "compile-error".to_owned())],
        "the `.ri` half marks only the compile-error member partial; the site \
         member's only Error is the sweep's own signal"
    );
    let err_key = format!(
        "host.rs:{}",
        host_line_of(&host, "module test.compile_error")
    );
    assert_eq!(
        inline.partial,
        vec![(err_key, "compile-error".to_owned())],
        "the inline half must mark the SAME member partial and not the site \
         member, exactly as the `.ri` half does"
    );

    let [ri_site] = ri.sites.as_slice() else {
        panic!(
            "the `.ri` half must find exactly one site, got {:#?}",
            ri.sites
        );
    };
    let [inline_site] = inline.sites.as_slice() else {
        panic!(
            "the inline half must find exactly one site, got {:#?}",
            inline.sites
        );
    };
    let columns = |s: &SurveySite| {
        (
            s.code.clone(),
            s.severity.clone(),
            s.field.clone(),
            s.expected.clone(),
            s.found.clone(),
            s.def.clone(),
            s.def_origin,
            s.owner,
            s.message.clone(),
        )
    };
    assert_eq!(
        columns(inline_site),
        columns(ri_site),
        "one member text must yield one site shape, whichever half swept it"
    );
    assert_eq!(
        inline_site.snippet_line,
        Some(ri_site.line),
        "the inline site's snippet-relative line is the `.ri` site's line"
    );
}

// ─── the inline half's gate-resident coverage pin ────────────────────────────

/// `(repo_relative_host, minimum_admitted_snippets)` for the inline hosts whose
/// extraction is pinned on the merge gate.
///
/// # The failure mode the corpus-parity gate CANNOT see
///
/// [`corpus_parity`](crate::corpus::corpus_parity) watches the ENUMERATIONS: it reds when a walker returns an
/// empty or under-floor set of paths. It knows nothing about what comes out of
/// those paths. A regression in [`rust_fixture_scan::raw_string_literals`] or in
/// [`rust_fixture_scan::looks_like_reify_source`] leaves the host enumeration
/// fully intact — every path present, every floor cleared, parity green — while
/// admitting zero snippets from every one of them. The artifact would then
/// regenerate with an empty inline section and read as an honest census of
/// nothing. This pin is the only thing standing in front of that, which is why
/// its floor counts ADMITTED SNIPPETS rather than paths.
///
/// # One live member of every [`HostShape`], on purpose
///
/// [`NAMED_SITE_HOST`] is a `tests` DIRECTORY host and carries the sites this
/// task's VERIFY criterion names; `engine_build/tests.rs` is
/// `src/**/tests.rs`, the shape
/// `reify_test_support::ignore_hygiene::walk_test_rs_files` structurally cannot
/// reach; `compute_representation_bounds_tests.rs` is `src/**/*_tests.rs`, 10
/// of whose 12 members a `tests.rs`-exact clause missed; and `analysis.rs` is a
/// production `src/*.rs` whose `#[cfg(test)]` module carries Reify fixtures.
/// A narrowing that dropped any one shape would leave the others still passing,
/// which is exactly how the first three were lost without a red gate.
///
/// # The floors are BROKEN-EXTRACTION floors
///
/// Measured live at 38, 21, 7 and 17 admitted snippets respectively. The floors
/// sit far below that for the same reason [`CorpusHalf::floor`](crate::corpus::CorpusHalf::floor) does: a
/// legitimate fixture edit that deletes a few snippets must not red the merge
/// gate. These numbers detect a BREAK, not drift.
///
/// # Why this pin names no site
///
/// It asserts only that snippets are still EXTRACTED, reach the compile phase,
/// and report resolving host lines. It names no param, no def and no site count.
/// δ (#5306) fixed conformance sites inside these very hosts in f247bade44, and
/// any assertion about which sites are found would have redded this gate then,
/// as it would on the next fixture or severity change — which is why a future
/// reader must not "helpfully" tighten this into a residual pin. The mechanism is pinned synthetically by
/// [`survey_inline_corpus_finds_the_snippet_site_at_its_host_position`]; the
/// live census belongs in the artifact, which is regenerated on demand, not on
/// the gate.
const INLINE_FIXTURE_PINNED_HOSTS: &[(&str, usize)] = &[
    (NAMED_SITE_HOST, 20),
    ("crates/reify-eval/src/engine_build/tests.rs", 10),
    (
        "crates/reify-eval/src/tolerance_combine/compute_representation_bounds_tests.rs",
        3,
    ),
    ("crates/reify-lsp/src/analysis.rs", 8),
];

/// [`INLINE_FIXTURE_PINNED_HOSTS`] names a live member of every [`HostShape`],
/// and every entry is a real file the shared host predicate admits.
///
/// Asserted SEPARATELY from the extraction pin below so an emptied or
/// narrowed pin list fails HERE, loudly, instead of making every per-host
/// assertion iterate zero times and pass vacuously.
#[test]
fn inline_fixture_pinned_hosts_name_all_four_enumeration_shapes() {
    let hosts: Vec<&str> = INLINE_FIXTURE_PINNED_HOSTS
        .iter()
        .map(|(h, _)| *h)
        .collect();

    assert!(
        hosts.contains(&NAMED_SITE_HOST),
        "the pin must name {NAMED_SITE_HOST} — the host of the sites this task's \
         VERIFY criterion names; pinned: {hosts:?}"
    );
    for shape in <HostShape as strum::IntoEnumIterator>::iter() {
        assert!(
            hosts.iter().any(|h| host_shape(h) == shape),
            "the pin must name a live {shape:?} host. This is the assertion the \
             corpus-parity gate structurally cannot make: a predicate re-narrowed \
             to one shape still clears every floor, so only a per-shape live member \
             sees it; pinned: {hosts:?}"
        );
    }

    for (host, floor) in INLINE_FIXTURE_PINNED_HOSTS {
        assert!(
            rust_fixture_scan::is_inline_fixture_host(std::path::Path::new(host)),
            "pinned host {host} must satisfy the same `is_inline_fixture_host` \
             predicate the corpus enumeration filters through, else the pin covers a \
             file the census never sweeps"
        );
        assert!(
            std::path::Path::new(WORKSPACE_ROOT).join(host).is_file(),
            "pinned host {host} does not exist under {WORKSPACE_ROOT}"
        );
        assert!(
            *floor > 0,
            "pinned host {host} carries a zero floor, which no extraction can fail"
        );
    }
}

/// Every [`INLINE_FIXTURE_PINNED_HOSTS`] entry still yields its floor of
/// admitted snippets, reaches the compile phase, and reports only host lines
/// that resolve.
///
/// Priced like `pinned_clean_files_emit_no_ctor_conformance_diagnostic`: a
/// handful of real files, not a corpus walk, so it costs one stdlib prelude
/// compile and stays gate-resident without reversing the landed
/// `docs/prds/merge-gate-compile-cost.md` decision.
///
/// Deliberately does NOT pin the param names at the sites VERIFY names. δ
/// (#5306) fixed those sites in f247bade44, which is exactly how a residual pin
/// would have redded this gate;
/// `survey_inline_corpus_still_sees_the_sites_task_7543_verify_names` pins
/// them against a verbatim pre-δ copy instead. The MECHANISM is pinned
/// synthetically by
/// [`survey_inline_corpus_finds_the_snippet_site_at_its_host_position`]; the
/// live census is the artifact, not a gate.
#[test]
fn inline_fixture_pinned_hosts_still_yield_their_snippets() {
    let mut failures: Vec<String> = Vec::new();

    for (host, floor) in INLINE_FIXTURE_PINNED_HOSTS {
        let host_path = std::path::Path::new(WORKSPACE_ROOT).join(host);
        let host_source = std::fs::read_to_string(&host_path)
            .unwrap_or_else(|e| panic!("pinned host {host} is unreadable: {e}"));
        let host_lines = host_source.lines().count() as u32;

        let admitted = rust_fixture_scan::inline_ri_snippets(&host_source)
            .snippets
            .len();
        if admitted < *floor {
            failures.push(format!(
                "  {host}: {admitted} admitted snippet(s), floor is {floor} — the host is \
                 still enumerated, so the corpus-parity gate is green and blind to this; \
                 an admission-filter or raw-string-lexer regression looks exactly like it"
            ));
        }

        let run = survey_inline_corpus(std::path::Path::new(WORKSPACE_ROOT), &[(*host).to_owned()]);

        for (member, reason) in &run.not_surveyed {
            if reason == "read-error" {
                failures.push(format!(
                    "  {member}: read-error — every extracted snippet must reach the \
                     compile phase, else this pin passes vacuously"
                ));
            }
        }
        assert_eq!(
            run.total,
            run.surveyed + run.not_surveyed.len(),
            "the coverage denominator must account for every member of {host} exactly once"
        );

        // Every reported host line is a `file:line` a human will open, so a line
        // past the end of the host is a dangling pointer — the same
        // postcondition `line_of_span` enforces for `.ri` rows.
        let mut reported: Vec<(String, u32)> = run
            .sites
            .iter()
            .map(|s| (format!("site {}", s.file), s.line))
            .collect();
        reported.extend(
            run.not_surveyed
                .iter()
                .chain(run.partial.iter())
                .filter_map(|(member, reason)| {
                    let (_, line) = member.rsplit_once(':')?;
                    Some((format!("{reason} {member}"), line.parse().ok()?))
                }),
        );
        for (what, line) in reported {
            if line < 1 || line > host_lines {
                failures.push(format!(
                    "  {what}: host line {line} does not resolve — {host} has \
                     {host_lines} line(s)"
                ));
            }
        }

        for site in &run.sites {
            if site.snippet_line.is_none() {
                failures.push(format!(
                    "  site {}:{}: an inline row must carry its snippet-relative \
                     coordinate",
                    site.file, site.line
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} inline-extraction regression(s):\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

