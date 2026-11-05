//! The tracked-`.ri` parse guard: the corpus check of
//! `docs/prds/v0_6/syntax-lowering-contract.md` D12 / §9 α (task #9343).
//!
//! Every tracked `.ri` that is not declared in [`MUST_ERROR_RI_FIXTURES`]
//! must have a clean Rust tree-sitter CST and an empty
//! `reify_syntax::parse(..).errors`, outside the trees in
//! [`UNSWEPT_RI_TREES`], whose undeclared members are skipped unread. A
//! declared file is checked wherever it lives and must keep showing its
//! declared fault. When this guard reds, fix the file in the same diff, or
//! declare it in [`MUST_ERROR_RI_FIXTURES`] with a reason.

use std::any::Any;
use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::panic::{self, AssertUnwindSafe};

use reify_core::ModulePath;
use tree_sitter::Node;

use crate::common::make_ts_parser;
use crate::common::tracked_corpus::{tracked_ri_paths, workspace_root};

/// How a declared must-error fixture is expected to fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MustErrorKind {
    /// The Rust tree-sitter CST carries an `ERROR` or `MISSING` node.
    CstFault,
    /// The CST is clean, and the lowering reports a parse error.
    ErrorsOnly,
}

/// One tracked `.ri` that is declared to fail, and why.
struct MustErrorFixture {
    path: &'static str,
    kind: MustErrorKind,
    reason: &'static str,
}

const CATALOG_MEMBERSHIP_SURFACE: &str = "target surface of \
    docs/prds/v0_6/catalog-membership-constraint.md: `constraint x in <catalog>` is outside the \
    grammar on main; when that PRD's leaf makes one parse, this guard reds and that leaf \
    removes the entry";

/// Tracked `.ri` files that are declared to fail, sorted by path.
const MUST_ERROR_RI_FIXTURES: &[MustErrorFixture] = &[
    MustErrorFixture {
        path: "crates/reify-cli/tests/fixtures/bracket_parse_error.ri",
        kind: MustErrorKind::CstFault,
        reason: "reify-cli's parse-error reporting fixture (`@@@` in a structure body)",
    },
    MustErrorFixture {
        path: "crates/reify-spec-conformance/fixtures/_placement-probe/placement_probe.ri",
        kind: MustErrorKind::CstFault,
        reason: "deliberately unparseable placement sentinel (that tree's fixtures/README.md)",
    },
    MustErrorFixture {
        path: "docs/prds/v0_6/fixtures/indexed_sub_instantiation_surface.ri",
        kind: MustErrorKind::ErrorsOnly,
        reason: "clean CST; lower_sub rejects the indexed `sub` instantiation (the #5482 \
                 interim rejection)",
    },
    MustErrorFixture {
        path: "gui/test/fixtures/broken_syntax.ri",
        kind: MustErrorKind::CstFault,
        reason: "GUI error-recovery fixture, intentionally invalid (listed rather than \
                 excluding gui/: syntax-lowering-contract §13 OQ-3)",
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/arrow_type.ri",
        kind: MustErrorKind::CstFault,
        reason: "FAIL grammar probe: the grammar has no arrow-type production",
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/cmc_bolt_feasible.ri",
        kind: MustErrorKind::CstFault,
        reason: CATALOG_MEMBERSHIP_SURFACE,
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/cmc_bolt_gap.ri",
        kind: MustErrorKind::CstFault,
        reason: CATALOG_MEMBERSHIP_SURFACE,
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/cmc_bolt_two_fit.ri",
        kind: MustErrorKind::CstFault,
        reason: CATALOG_MEMBERSHIP_SURFACE,
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/cmc_int_range.ri",
        kind: MustErrorKind::CstFault,
        reason: CATALOG_MEMBERSHIP_SURFACE,
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/cmc_local_catalogs.ri",
        kind: MustErrorKind::CstFault,
        reason: CATALOG_MEMBERSHIP_SURFACE,
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/cmc_membership_check.ri",
        kind: MustErrorKind::CstFault,
        reason: CATALOG_MEMBERSHIP_SURFACE,
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/cmc_sugar_equivalence.ri",
        kind: MustErrorKind::CstFault,
        reason: CATALOG_MEMBERSHIP_SURFACE,
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/cmc_unextractable.ri",
        kind: MustErrorKind::CstFault,
        reason: CATALOG_MEMBERSHIP_SURFACE,
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/decl_passes_purpose_body_sub_swallowed.ri",
        kind: MustErrorKind::CstFault,
        reason: "compiler-declaration-passes B4 evidence: a `sub` in a purpose body is an \
                 ERROR the lowering drops silently",
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/slc_annotation_arg_fault.ri",
        kind: MustErrorKind::CstFault,
        reason: "syntax-lowering-contract §2.2 evidence: a faulty annotation argument \
                 (`@foo(1 +)`)",
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/slc_field_analytical_fault.ri",
        kind: MustErrorKind::CstFault,
        reason: "syntax-lowering-contract §2.2 evidence: a faulty expression in a field's \
                 `analytical` source",
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/slc_field_imported_fault.ri",
        kind: MustErrorKind::CstFault,
        reason: "syntax-lowering-contract §2.2 evidence: a missing value in a field's \
                 `imported` source block",
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/slc_purpose_body_error.ri",
        kind: MustErrorKind::CstFault,
        reason: "syntax-lowering-contract §2.2 evidence: a faulty member in a purpose body \
                 (`param y:`)",
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/slc_purpose_param_missing.ri",
        kind: MustErrorKind::CstFault,
        reason: "syntax-lowering-contract §2.2 evidence: a purpose parameter with no type \
                 (`x: `)",
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/slc_root_error.ri",
        kind: MustErrorKind::CstFault,
        reason: "syntax-lowering-contract §2.2 evidence: a token salad whose root node is \
                 ERROR",
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/slc_struct_header_fault.ri",
        kind: MustErrorKind::CstFault,
        reason: "syntax-lowering-contract §2.2 evidence: a faulty type-parameter bound in a \
                 structure header (`S<T: >`)",
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/slc_trait_body_port.ri",
        kind: MustErrorKind::CstFault,
        reason: "syntax-lowering-contract §2.2 evidence: a `port` member in a trait body",
    },
    MustErrorFixture {
        path: "tests/prd-gate/fixtures/step_import_surface.ri",
        kind: MustErrorKind::CstFault,
        reason: "target surface of docs/prds/v0_6/step-assembly-import.md",
    },
];

/// A tracked tree whose UNDECLARED members the guard skips unread.
struct UnsweptTree {
    prefix: &'static str,
    reason: &'static str,
}

/// The trees the guard does not sweep, ratified on the D12 row of
/// `docs/prds/v0_6/syntax-lowering-contract.md`.
const UNSWEPT_RI_TREES: &[UnsweptTree] = &[
    UnsweptTree {
        prefix: "tests/prd-gate/fixtures/", // pg-drift-dir:allow — an exclusion, not a walk: undeclared members are skipped unread, so adding a fixture stays inert
        reason: "chartered probe inputs that need not parse (tests/prd-gate/README.md); \
                 declared members are still checked",
    },
    UnsweptTree {
        prefix: "crates/reify-spec-conformance/fixtures/",
        reason: "chartered must-reject residents of the spec-conformance corpus (that tree's \
                 README.md); declared members are still checked",
    },
];

/// What the guard demands of one tracked path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expectation {
    Clean,
    MustError(MustErrorKind),
    Unswept,
}

/// A declared entry wins over an unswept tree, so every declared file is
/// checked wherever it lives.
fn expectation(path: &str) -> Expectation {
    if let Some(fixture) = MUST_ERROR_RI_FIXTURES.iter().find(|f| f.path == path) {
        return Expectation::MustError(fixture.kind);
    }
    if UNSWEPT_RI_TREES
        .iter()
        .any(|tree| path.starts_with(tree.prefix))
    {
        return Expectation::Unswept;
    }
    Expectation::Clean
}

/// Where the first CST fault sits: 1-based line and column, and the node
/// (`ERROR`, or `MISSING <kind>`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct CstFaultSite {
    line: usize,
    column: usize,
    node: String,
}

impl fmt::Display for CstFaultSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}:{}", self.node, self.line, self.column)
    }
}

/// What the Rust runtime makes of one source: its first CST fault, and its
/// lowering errors rendered as `line:col: message`.
struct Observed {
    cst_fault: Option<CstFaultSite>,
    errors: Vec<String>,
}

fn observe(source: &str) -> Observed {
    let tree = make_ts_parser()
        .parse(source, None)
        .expect("tree-sitter returns a tree for any source");
    let parsed = reify_syntax::parse(source, ModulePath::single("tracked_corpus"));
    Observed {
        cst_fault: first_cst_fault(tree.root_node()),
        errors: parsed.render_errors(source),
    }
}

/// The first `ERROR` or `MISSING` node in pre-order. `has_error()` only
/// prunes the descent; the fault itself is the node that IS one.
fn first_cst_fault(node: Node<'_>) -> Option<CstFaultSite> {
    if node.is_error() || node.is_missing() {
        let start = node.start_position();
        let label = if node.is_missing() {
            format!("MISSING {}", node.kind())
        } else {
            "ERROR".to_owned()
        };
        return Some(CstFaultSite {
            line: start.row + 1,
            column: start.column + 1,
            node: label,
        });
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.has_error())
        .find_map(first_cst_fault)
}

const REMEDY: &str =
    "fix the file in this diff, or declare it in MUST_ERROR_RI_FIXTURES with a reason";

/// Why one tracked path fails the guard.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Violation {
    UndeclaredCstFault(CstFaultSite),
    UndeclaredErrors { count: usize, first: String },
    DeclaredCstFaultGone,
    DeclaredErrorsOnlyHasCstFault(CstFaultSite),
    DeclaredErrorsGone,
    Unreadable(String),
    ParsePanicked(String),
    StaleEntry,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndeclaredCstFault(site) => write!(
                f,
                "undeclared, but the Rust CST has a fault ({site}); {REMEDY}"
            ),
            Self::UndeclaredErrors { count, first } => write!(
                f,
                "undeclared, but the lowering reports {count} parse error(s), first \
                 `{first}`; {REMEDY}"
            ),
            Self::DeclaredCstFaultGone => f.write_str(
                "declared CstFault, but the Rust CST is clean; remove its \
                 MUST_ERROR_RI_FIXTURES entry (or reclassify it ErrorsOnly if the lowering \
                 still reports)",
            ),
            Self::DeclaredErrorsOnlyHasCstFault(site) => write!(
                f,
                "declared ErrorsOnly, but the Rust CST has a fault ({site}); reclassify its \
                 MUST_ERROR_RI_FIXTURES entry as CstFault"
            ),
            Self::DeclaredErrorsGone => f.write_str(
                "declared ErrorsOnly, but the lowering reports no parse error; remove its \
                 MUST_ERROR_RI_FIXTURES entry",
            ),
            Self::Unreadable(error) => write!(f, "tracked but unreadable: {error}"),
            Self::ParsePanicked(message) => write!(
                f,
                "parsing it panicked: {message}; the parser must reject a source, never \
                 panic on it"
            ),
            Self::StaleEntry => f.write_str(
                "declared in MUST_ERROR_RI_FIXTURES but not a tracked .ri; fix or remove the \
                 entry",
            ),
        }
    }
}

/// The guard's verdict on one observed file: `None` when it meets its
/// expectation.
fn violation(expected: Expectation, observed: &Observed) -> Option<Violation> {
    match (expected, &observed.cst_fault, observed.errors.first()) {
        (Expectation::Clean, Some(site), _) => Some(Violation::UndeclaredCstFault(site.clone())),
        (Expectation::Clean, None, Some(first)) => Some(Violation::UndeclaredErrors {
            count: observed.errors.len(),
            first: first.clone(),
        }),
        (Expectation::Clean, None, None) => None,
        (Expectation::MustError(MustErrorKind::CstFault), None, _) => {
            Some(Violation::DeclaredCstFaultGone)
        }
        (Expectation::MustError(MustErrorKind::CstFault), Some(_), _) => None,
        (Expectation::MustError(MustErrorKind::ErrorsOnly), Some(site), _) => {
            Some(Violation::DeclaredErrorsOnlyHasCstFault(site.clone()))
        }
        (Expectation::MustError(MustErrorKind::ErrorsOnly), None, None) => {
            Some(Violation::DeclaredErrorsGone)
        }
        (Expectation::MustError(MustErrorKind::ErrorsOnly), None, Some(_)) => None,
        (Expectation::Unswept, _, _) => None,
    }
}

/// A violation, attributed to the tracked path it was found on.
#[derive(Debug, PartialEq, Eq)]
struct Finding {
    path: String,
    violation: Violation,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.violation)
    }
}

/// The outcome of a sweep: its findings, and how many files it checked.
struct Sweep {
    findings: Vec<Finding>,
    swept: usize,
}

/// The guard over a listing of tracked paths. `read` loads a path's source
/// and `inspect` observes it; an unswept path is skipped before it is read,
/// and a declared entry missing from the listing is a finding of its own.
fn sweep(
    files: &[String],
    read: impl Fn(&str) -> io::Result<String>,
    inspect: impl Fn(&str) -> Observed,
) -> Sweep {
    let mut findings = Vec::new();
    let mut visited_declared: BTreeSet<&str> = BTreeSet::new();
    let mut swept = 0usize;
    for rel in files {
        let expected = expectation(rel);
        if expected == Expectation::Unswept {
            continue;
        }
        if matches!(expected, Expectation::MustError(_)) {
            visited_declared.insert(rel);
        }
        swept += 1;
        if let Some(violation) = check(rel, expected, &read, &inspect) {
            findings.push(Finding {
                path: rel.clone(),
                violation,
            });
        }
    }
    findings.extend(
        MUST_ERROR_RI_FIXTURES
            .iter()
            .filter(|fixture| !visited_declared.contains(fixture.path))
            .map(|fixture| Finding {
                path: fixture.path.to_owned(),
                violation: Violation::StaleEntry,
            }),
    );
    Sweep { findings, swept }
}

/// One swept file's verdict. A panic while observing it is caught and
/// reported against the file, so the sweep still names it and keeps going.
fn check(
    rel: &str,
    expected: Expectation,
    read: &impl Fn(&str) -> io::Result<String>,
    inspect: &impl Fn(&str) -> Observed,
) -> Option<Violation> {
    let source = match read(rel) {
        Ok(source) => source,
        Err(error) => return Some(Violation::Unreadable(error.to_string())),
    };
    match panic::catch_unwind(AssertUnwindSafe(|| inspect(&source))) {
        Ok(observed) => violation(expected, &observed),
        Err(payload) => Some(Violation::ParsePanicked(panic_message(payload.as_ref()))),
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_owned();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "a non-string panic payload".to_owned()
}

// ── verdict unit tests, over inline sources ─────────────────────────────────

/// A CST fault: the parameter's type is missing.
const FAULTY: &str = "purpose P(x: ) { }\n";

/// A clean CST whose lowering reports: the second row continues the `let`.
const ERRORS_ONLY: &str = "structure S {\n  let d = 5mm\n  - 3mm\n}\n";

const CLEAN: &str = "structure S {\n    param a : Length = 1mm\n}\n";

const PROBE_PATH: &str = "examples/zz_guard_probe.ri";

/// A declared entry that lives inside an unswept tree.
const DECLARED_IN_UNSWEPT_TREE: &str =
    "crates/reify-spec-conformance/fixtures/_placement-probe/placement_probe.ri";

/// An undeclared member of an unswept tree.
const UNDECLARED_IN_UNSWEPT_TREE: &str =
    "crates/reify-spec-conformance/fixtures/zz_section/zz_synthetic.ri";

fn faulty_site() -> CstFaultSite {
    observe(FAULTY)
        .cst_fault
        .expect("premise: FAULTY has a CST fault")
}

#[test]
fn observe_reports_a_cst_fault_with_its_location() {
    assert_eq!(
        observe(FAULTY).cst_fault,
        Some(CstFaultSite {
            line: 1,
            column: 13,
            node: "MISSING identifier".to_owned(),
        }),
        "{FAULTY:?} must observe its CST fault where the type is missing"
    );
}

#[test]
fn observe_reports_lowering_errors_on_a_clean_cst() {
    let observed = observe(ERRORS_ONLY);
    assert_eq!(
        observed.cst_fault, None,
        "{ERRORS_ONLY:?} must have a clean CST"
    );
    assert!(
        !observed.errors.is_empty(),
        "{ERRORS_ONLY:?} must report a lowering error"
    );
}

#[test]
fn observe_is_silent_on_a_clean_source() {
    let observed = observe(CLEAN);
    assert_eq!(observed.cst_fault, None);
    assert!(observed.errors.is_empty(), "errors: {:?}", observed.errors);
}

#[test]
fn undeclared_cst_fault_is_a_violation() {
    assert_eq!(
        violation(Expectation::Clean, &observe(FAULTY)),
        Some(Violation::UndeclaredCstFault(faulty_site()))
    );
}

#[test]
fn undeclared_parse_error_on_a_clean_cst_is_a_violation() {
    let observed = observe(ERRORS_ONLY);
    let first = observed
        .errors
        .first()
        .expect("premise: ERRORS_ONLY reports a lowering error")
        .clone();
    assert_eq!(
        violation(Expectation::Clean, &observed),
        Some(Violation::UndeclaredErrors {
            count: observed.errors.len(),
            first,
        })
    );
}

#[test]
fn undeclared_clean_source_is_not_a_violation() {
    assert_eq!(violation(Expectation::Clean, &observe(CLEAN)), None);
}

#[test]
fn declared_cst_fault_holds_only_while_the_cst_is_faulty() {
    let declared = Expectation::MustError(MustErrorKind::CstFault);
    assert_eq!(violation(declared, &observe(FAULTY)), None);
    assert_eq!(
        violation(declared, &observe(CLEAN)),
        Some(Violation::DeclaredCstFaultGone)
    );
    assert_eq!(
        violation(declared, &observe(ERRORS_ONLY)),
        Some(Violation::DeclaredCstFaultGone),
        "a clean CST breaks a CstFault declaration even while the lowering reports"
    );
}

#[test]
fn declared_errors_only_needs_a_clean_cst_and_errors() {
    let declared = Expectation::MustError(MustErrorKind::ErrorsOnly);
    assert_eq!(violation(declared, &observe(ERRORS_ONLY)), None);
    assert_eq!(
        violation(declared, &observe(CLEAN)),
        Some(Violation::DeclaredErrorsGone)
    );
    assert_eq!(
        violation(declared, &observe(FAULTY)),
        Some(Violation::DeclaredErrorsOnlyHasCstFault(faulty_site()))
    );
}

// ── sweep unit tests, over inline listings ──────────────────────────────────

fn listing(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

fn findings_on<'a>(result: &'a Sweep, path: &str) -> Vec<&'a Violation> {
    result
        .findings
        .iter()
        .filter(|finding| finding.path == path)
        .map(|finding| &finding.violation)
        .collect()
}

#[test]
fn sweep_names_the_file_of_an_undeclared_cst_fault() {
    let result = sweep(&listing(&[PROBE_PATH]), |_| Ok(FAULTY.to_owned()), observe);
    let finding = result
        .findings
        .iter()
        .find(|finding| finding.path == PROBE_PATH)
        .expect("an undeclared CST fault must be a finding");
    assert_eq!(
        finding.violation,
        Violation::UndeclaredCstFault(faulty_site())
    );
    let message = finding.to_string();
    assert!(message.contains(PROBE_PATH), "{message} must name the file");
}

#[test]
fn sweep_reports_every_declared_entry_missing_from_the_listing() {
    let result = sweep(
        &[],
        |rel| panic!("an empty listing reads nothing, yet {rel} was read"),
        observe,
    );
    let stale: Vec<Finding> = MUST_ERROR_RI_FIXTURES
        .iter()
        .map(|fixture| Finding {
            path: fixture.path.to_owned(),
            violation: Violation::StaleEntry,
        })
        .collect();
    assert_eq!(result.findings, stale);
    assert_eq!(result.swept, 0);
}

#[test]
fn sweep_checks_a_listed_declared_entry_inside_an_unswept_tree() {
    let result = sweep(
        &listing(&[DECLARED_IN_UNSWEPT_TREE]),
        |_| Ok(CLEAN.to_owned()),
        observe,
    );
    assert_eq!(
        findings_on(&result, DECLARED_IN_UNSWEPT_TREE),
        [&Violation::DeclaredCstFaultGone],
        "a listed declared entry is read and checked, and is not stale"
    );
    assert_eq!(result.swept, 1);
}

#[test]
fn sweep_skips_an_undeclared_member_of_an_unswept_tree_unread() {
    let result = sweep(
        &listing(&[UNDECLARED_IN_UNSWEPT_TREE]),
        |rel| panic!("{rel} is an undeclared member of an unswept tree and must not be read"),
        observe,
    );
    assert_eq!(
        findings_on(&result, UNDECLARED_IN_UNSWEPT_TREE),
        Vec::<&Violation>::new()
    );
    assert_eq!(result.swept, 0);
}

#[test]
fn sweep_reports_an_unreadable_file() {
    let result = sweep(
        &listing(&[PROBE_PATH]),
        |_| Err(io::Error::new(io::ErrorKind::NotFound, "probe: gone")),
        observe,
    );
    assert_eq!(
        findings_on(&result, PROBE_PATH),
        [&Violation::Unreadable("probe: gone".to_owned())]
    );
}

#[test]
fn sweep_reports_a_parser_panic_against_the_file_and_keeps_going() {
    let panicking = "examples/zz_guard_panic_probe.ri";
    let result = sweep(
        &listing(&[panicking, PROBE_PATH]),
        |rel| Ok(rel.to_owned()),
        |source| {
            if source == panicking {
                panic!("probe panic");
            }
            observe(FAULTY)
        },
    );
    assert_eq!(
        findings_on(&result, panicking),
        [&Violation::ParsePanicked("probe panic".to_owned())]
    );
    assert_eq!(
        findings_on(&result, PROBE_PATH),
        [&Violation::UndeclaredCstFault(faulty_site())],
        "the sweep keeps checking after a panic"
    );
}

// ── the tracked corpus ──────────────────────────────────────────────────────

#[test]
fn expectation_declared_entry_wins_over_an_unswept_tree() {
    assert_eq!(
        expectation(DECLARED_IN_UNSWEPT_TREE),
        Expectation::MustError(MustErrorKind::CstFault),
        "a declared entry is checked wherever it lives"
    );
    assert_eq!(
        expectation(UNDECLARED_IN_UNSWEPT_TREE),
        Expectation::Unswept,
        "an undeclared member of an unswept tree is skipped"
    );
    assert_eq!(
        expectation("examples/zz_synthetic.ri"),
        Expectation::Clean,
        "a file outside every table must parse cleanly"
    );
}

/// THE GUARD: every swept tracked `.ri` meets its expectation, and every
/// declared entry names a tracked `.ri` the sweep actually checked.
#[test]
fn every_swept_tracked_ri_parses_cleanly_unless_declared_must_error() {
    let root = workspace_root();
    let files = tracked_ri_paths(&root);
    assert!(
        files.len() > 100,
        "sweep found only {} tracked .ri files under {} — the listing is broken, \
         not the repo",
        files.len(),
        root.display()
    );

    let result = sweep(
        &files,
        |rel| std::fs::read_to_string(root.join(rel)),
        observe,
    );
    let report: Vec<String> = result.findings.iter().map(Finding::to_string).collect();
    assert!(
        report.is_empty(),
        "{} tracked-.ri parse guard violation(s) (swept {} of {} tracked files):\n{}",
        report.len(),
        result.swept,
        files.len(),
        report.join("\n")
    );
}

#[test]
fn must_error_and_unswept_tables_are_well_formed() {
    let paths: Vec<&str> = MUST_ERROR_RI_FIXTURES.iter().map(|f| f.path).collect();
    let mut seen = BTreeSet::new();
    let duplicates: Vec<&str> = paths.iter().copied().filter(|p| !seen.insert(*p)).collect();
    assert!(
        duplicates.is_empty(),
        "MUST_ERROR_RI_FIXTURES lists these paths more than once: {duplicates:?}"
    );
    let unsorted: Vec<[&str; 2]> = paths
        .windows(2)
        .filter(|pair| pair[0] > pair[1])
        .map(|pair| [pair[0], pair[1]])
        .collect();
    assert!(
        unsorted.is_empty(),
        "MUST_ERROR_RI_FIXTURES must be sorted by path; out of order: {unsorted:?}"
    );

    let reasonless: Vec<&str> = MUST_ERROR_RI_FIXTURES
        .iter()
        .filter(|f| f.reason.trim().is_empty())
        .map(|f| f.path)
        .chain(
            UNSWEPT_RI_TREES
                .iter()
                .filter(|t| t.reason.trim().is_empty())
                .map(|t| t.prefix),
        )
        .collect();
    assert!(
        reasonless.is_empty(),
        "these MUST_ERROR_RI_FIXTURES / UNSWEPT_RI_TREES rows carry no reason: {reasonless:?}"
    );

    let unterminated: Vec<&str> = UNSWEPT_RI_TREES
        .iter()
        .map(|t| t.prefix)
        .filter(|prefix| !prefix.ends_with('/'))
        .collect();
    assert!(
        unterminated.is_empty(),
        "UNSWEPT_RI_TREES prefixes must end with '/' so they cannot match a sibling \
         tree: {unterminated:?}"
    );

    let tracked = tracked_ri_paths(&workspace_root());
    let dead: Vec<&str> = UNSWEPT_RI_TREES
        .iter()
        .map(|t| t.prefix)
        .filter(|prefix| !tracked.iter().any(|rel| rel.starts_with(prefix)))
        .collect();
    assert!(
        dead.is_empty(),
        "these UNSWEPT_RI_TREES rows cover no tracked .ri; delete them: {dead:?}"
    );
}
