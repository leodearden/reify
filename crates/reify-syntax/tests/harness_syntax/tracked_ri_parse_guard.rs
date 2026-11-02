//! The tracked-`.ri` parse guard: the corpus check of
//! `docs/prds/v0_6/syntax-lowering-contract.md` D12 / §9 α (task #9343).
//!
//! Every tracked `.ri` that is not declared in [`MUST_ERROR_RI_FIXTURES`]
//! must have a clean Rust tree-sitter CST and an empty
//! `reify_syntax::parse(..).errors`. A declared file must keep showing its
//! declared fault. When this guard reds, fix the file in the same diff, or
//! declare it in [`MUST_ERROR_RI_FIXTURES`] with a reason.

use std::fmt;

use reify_core::ModulePath;
use tree_sitter::Node;

use crate::common::make_ts_parser;

/// How a declared must-error fixture is expected to fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MustErrorKind {
    /// The Rust tree-sitter CST carries an `ERROR` or `MISSING` node.
    CstFault,
    /// The CST is clean, and the lowering reports a parse error.
    ErrorsOnly,
}

/// One tracked `.ri` that is declared to fail, and why.
#[allow(dead_code)]
struct MustErrorFixture {
    path: &'static str,
    kind: MustErrorKind,
    reason: &'static str,
}

/// Tracked `.ri` files that are declared to fail, sorted by path.
#[allow(dead_code)]
const MUST_ERROR_RI_FIXTURES: &[MustErrorFixture] = &[];

/// A tracked tree whose UNDECLARED members the guard skips unread.
#[allow(dead_code)]
struct UnsweptTree {
    prefix: &'static str,
    reason: &'static str,
}

/// The trees the guard does not sweep.
#[allow(dead_code)]
const UNSWEPT_RI_TREES: &[UnsweptTree] = &[];

/// What the guard demands of one tracked path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum Expectation {
    Clean,
    MustError(MustErrorKind),
    Unswept,
}

#[allow(dead_code)]
fn expectation(_path: &str) -> Expectation {
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
