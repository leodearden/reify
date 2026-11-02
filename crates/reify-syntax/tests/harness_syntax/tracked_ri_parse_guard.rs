//! The tracked-`.ri` parse guard: the corpus check of
//! `docs/prds/v0_6/syntax-lowering-contract.md` D12 / §9 α (task #9343).
//!
//! Every tracked `.ri` that is not declared in [`MUST_ERROR_RI_FIXTURES`]
//! must have a clean Rust tree-sitter CST and an empty
//! `reify_syntax::parse(..).errors`. A declared file must keep showing its
//! declared fault. When this guard reds, fix the file in the same diff, or
//! declare it in [`MUST_ERROR_RI_FIXTURES`] with a reason.

use std::collections::BTreeSet;
use std::fmt;

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

/// The guard's verdict on one file: `None` when it meets its expectation,
/// otherwise a message naming the file and the remedy.
fn violation(path: &str, expected: Expectation, observed: &Observed) -> Option<String> {
    match (expected, &observed.cst_fault, observed.errors.first()) {
        (Expectation::Clean, Some(site), _) => Some(format!(
            "{path}: undeclared, but the Rust CST has a fault ({site}); {REMEDY}"
        )),
        (Expectation::Clean, None, Some(first)) => Some(format!(
            "{path}: undeclared, but the lowering reports {} parse error(s), first `{first}`; \
             {REMEDY}",
            observed.errors.len()
        )),
        (Expectation::Clean, None, None) => None,
        (Expectation::MustError(MustErrorKind::CstFault), None, _) => Some(format!(
            "{path}: declared CstFault, but the Rust CST is clean; remove its \
             MUST_ERROR_RI_FIXTURES entry (or reclassify it ErrorsOnly if the lowering \
             still reports)"
        )),
        (Expectation::MustError(MustErrorKind::CstFault), Some(_), _) => None,
        (Expectation::MustError(MustErrorKind::ErrorsOnly), Some(site), _) => Some(format!(
            "{path}: declared ErrorsOnly, but the Rust CST has a fault ({site}); reclassify \
             its MUST_ERROR_RI_FIXTURES entry as CstFault"
        )),
        (Expectation::MustError(MustErrorKind::ErrorsOnly), None, None) => Some(format!(
            "{path}: declared ErrorsOnly, but the lowering reports no parse error; remove \
             its MUST_ERROR_RI_FIXTURES entry"
        )),
        (Expectation::MustError(MustErrorKind::ErrorsOnly), None, Some(_)) => None,
        (Expectation::Unswept, _, _) => None,
    }
}

// ── verdict unit tests, over inline sources ─────────────────────────────────

/// A CST fault: the parameter's type is missing.
const FAULTY: &str = "purpose P(x: ) { }\n";

/// A clean CST whose lowering reports: the second row continues the `let`.
const ERRORS_ONLY: &str = "structure S {\n  let d = 5mm\n  - 3mm\n}\n";

const CLEAN: &str = "structure S {\n    param a : Length = 1mm\n}\n";

const PROBE_PATH: &str = "examples/zz_guard_probe.ri";

#[test]
fn observe_reports_a_cst_fault_with_its_location() {
    let observed = observe(FAULTY);
    assert!(
        observed.cst_fault.is_some(),
        "{FAULTY:?} must observe a CST fault"
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
fn undeclared_cst_fault_is_a_violation_naming_the_file() {
    let observed = observe(FAULTY);
    let site = observed
        .cst_fault
        .as_ref()
        .expect("premise: FAULTY has a CST fault")
        .to_string();
    let message = violation(PROBE_PATH, Expectation::Clean, &observed)
        .expect("an undeclared CST fault must be a violation");
    assert!(message.contains(PROBE_PATH), "{message}");
    assert!(message.contains(&site), "{message} must name {site}");
}

#[test]
fn undeclared_parse_error_on_a_clean_cst_is_a_violation_naming_the_file() {
    let observed = observe(ERRORS_ONLY);
    let first = observed
        .errors
        .first()
        .expect("premise: ERRORS_ONLY reports a lowering error")
        .clone();
    let message = violation(PROBE_PATH, Expectation::Clean, &observed)
        .expect("an undeclared parse error must be a violation");
    assert!(message.contains(PROBE_PATH), "{message}");
    assert!(message.contains(&first), "{message} must carry {first}");
}

#[test]
fn undeclared_clean_source_is_not_a_violation() {
    assert_eq!(
        violation(PROBE_PATH, Expectation::Clean, &observe(CLEAN)),
        None
    );
}

#[test]
fn declared_cst_fault_holds_only_while_the_cst_is_faulty() {
    let declared = Expectation::MustError(MustErrorKind::CstFault);
    assert_eq!(violation(PROBE_PATH, declared, &observe(FAULTY)), None);

    let message = violation(PROBE_PATH, declared, &observe(CLEAN))
        .expect("a declared CstFault that parses cleanly must be a violation");
    assert!(message.contains(PROBE_PATH), "{message}");
    assert!(message.contains("MUST_ERROR_RI_FIXTURES"), "{message}");
}

#[test]
fn declared_errors_only_needs_a_clean_cst_and_errors() {
    let declared = Expectation::MustError(MustErrorKind::ErrorsOnly);
    assert_eq!(violation(PROBE_PATH, declared, &observe(ERRORS_ONLY)), None);

    let clean = violation(PROBE_PATH, declared, &observe(CLEAN))
        .expect("a declared ErrorsOnly that reports nothing must be a violation");
    assert!(clean.contains(PROBE_PATH), "{clean}");
    assert!(clean.contains("MUST_ERROR_RI_FIXTURES"), "{clean}");

    let faulty = violation(PROBE_PATH, declared, &observe(FAULTY))
        .expect("a declared ErrorsOnly with a CST fault must be a violation");
    assert!(faulty.contains(PROBE_PATH), "{faulty}");
    assert!(
        faulty.contains("CstFault"),
        "{faulty} must say to reclassify"
    );
}

// ── the tracked corpus ──────────────────────────────────────────────────────

#[test]
fn expectation_declared_entry_wins_over_an_unswept_tree() {
    assert_eq!(
        expectation("crates/reify-spec-conformance/fixtures/_placement-probe/placement_probe.ri"),
        Expectation::MustError(MustErrorKind::CstFault),
        "a declared entry is checked wherever it lives"
    );
    assert_eq!(
        expectation("crates/reify-spec-conformance/fixtures/zz_section/zz_synthetic.ri"),
        Expectation::Unswept,
        "an undeclared member of an unswept tree is skipped"
    );
    assert_eq!(
        expectation("examples/zz_synthetic.ri"),
        Expectation::Clean,
        "a file outside every table must parse cleanly"
    );
}

/// THE GUARD: every tracked `.ri` meets its expectation, and every declared
/// entry names a tracked `.ri` the sweep actually checked.
#[test]
fn every_tracked_ri_parses_cleanly_unless_declared_must_error() {
    let root = workspace_root();
    let files = tracked_ri_paths(&root);
    assert!(
        files.len() > 100,
        "sweep found only {} tracked .ri files under {} — the listing is broken, \
         not the repo",
        files.len(),
        root.display()
    );

    let mut violations: Vec<String> = Vec::new();
    let mut visited_declared: BTreeSet<&str> = BTreeSet::new();
    let mut swept = 0usize;
    for rel in &files {
        let expected = expectation(rel);
        if expected == Expectation::Unswept {
            continue;
        }
        if matches!(expected, Expectation::MustError(_)) {
            visited_declared.insert(rel);
        }
        swept += 1;
        match std::fs::read_to_string(root.join(rel)) {
            Ok(source) => violations.extend(violation(rel, expected, &observe(&source))),
            Err(e) => violations.push(format!("{rel}: tracked but unreadable: {e}")),
        }
    }
    violations.extend(
        MUST_ERROR_RI_FIXTURES
            .iter()
            .filter(|fixture| !visited_declared.contains(fixture.path))
            .map(|fixture| {
                format!(
                    "{}: declared in MUST_ERROR_RI_FIXTURES but not a tracked .ri; fix or \
                     remove the entry",
                    fixture.path
                )
            }),
    );

    assert!(
        violations.is_empty(),
        "{} tracked-.ri parse guard violation(s) (swept {swept} of {} tracked files):\n{}",
        violations.len(),
        files.len(),
        violations.join("\n")
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
