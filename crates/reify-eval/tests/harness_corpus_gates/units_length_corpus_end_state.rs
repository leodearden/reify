//! §6 row 20 — the CORPUS END STATE: with every LENGTH gate live, the shipped
//! `examples/**/*.ri` corpus needs zero migrations.
//!
//! The PRD's §2 probe predicted this before the gates were built. Every other §6 row
//! asserts that a bare number IS rejected; this one asserts the complement over real
//! code — that nothing the project actually ships trips the gates. A gate that rejected
//! correct designs would pass every other row in the suite and still be wrong.
//!
//! The claim is keyed on `DiagnosticCode::DimensionedArgRejected`, never on message text:
//! a reword of the wording template must not silently turn this gate vacuous. The D9
//! wording itself is pinned elsewhere, at
//! `crates/reify-cli/tests/harness_cli/units_length_boundary_gate.rs`.
//!
//! WHY UNRELATED BUILD FAILURES ARE TOLERATED: a `MockGeometryKernel` cannot realize
//! everything the corpus describes, and this gate asserts only the ABSENCE of a units
//! rejection — not that every example builds. That tolerance is exactly what would let a
//! broken sweep read as green, so it is paid for by three companions in this file, all
//! required: the corpus-size floor, the seeded positive, and the op-reach floor.

use crate::eval_gate_support;
use reify_core::DiagnosticCode;
use reify_eval::Engine;
use reify_ir::ExportFormat;
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, compile_source_with_stdlib};

/// The `examples/` subtree whose contribution is asserted separately, so a reorg that
/// moved it out from under the walk cannot silently shrink the corpus to zero of it.
const BEST_PRACTICES_PREFIX: &str = "best_practices/";

/// The corpus is 264 files today. The floor is deliberately well below that and is a
/// FLOOR, not an equality: adding examples must never break this gate, but deleting most
/// of them — or a walk that silently stopped finding them — must.
const CORPUS_FLOOR: usize = 200;

/// Geometry ops observed reaching the mock kernel across the whole sweep.
///
/// Without this, "built nothing, found nothing" would read exactly like "built everything,
/// found nothing". Measured at 1000+ on the corpus this landed against; set well below
/// the measurement for the same reason as [`CORPUS_FLOOR`].
const OPS_REACH_FLOOR: usize = 400;

/// One units rejection observed while sweeping, carrying enough to act on: which file,
/// and what the user would have been told.
#[derive(Debug)]
struct UnitsViolation {
    rel: String,
    message: String,
}

/// What one source produced when built against a mock kernel: every units rejection, and
/// how many geometry ops actually reached the kernel.
struct SweepOutcome {
    violations: Vec<UnitsViolation>,
    ops_reached: usize,
}

/// THE checker. The corpus sweep and the seeded positive both go through this one
/// function, which is what makes the seed's red a statement about the sweep rather than
/// about a parallel implementation of it.
fn sweep_source(rel: &str, source: &str) -> SweepOutcome {
    let compiled = compile_source_with_stdlib(source);

    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result = engine.build(&compiled, ExportFormat::Step);

    let violations = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DimensionedArgRejected))
        .map(|d| UnitsViolation {
            rel: rel.to_string(),
            message: d.message.clone(),
        })
        .collect();
    let ops_reached = ops_ref.lock().unwrap().len();

    SweepOutcome {
        violations,
        ops_reached,
    }
}

/// The `examples/` tree, and the workspace root its members are reported relative to.
fn examples_corpus() -> (std::path::PathBuf, Vec<std::path::PathBuf>) {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../..");
    let root = root.canonicalize().unwrap_or(root);

    let mut files = Vec::new();
    eval_gate_support::collect_ri_files(&root.join("examples"), &mut files);
    files.sort();
    (root, files)
}

/// §6 row 20 — no shipped example needs a length migration, and the sweep that says so
/// really swept something.
///
/// The three floors are not decoration. Tolerating per-file build failures (see the
/// module header) means a sweep that compiled nothing, built nothing, or walked an empty
/// directory would report zero violations and read as green. Each floor closes one of
/// those: the corpus was found, its `best_practices/` half was found, and geometry
/// genuinely reached the kernel.
#[test]
fn no_shipped_example_trips_a_length_gate() {
    let (root, files) = examples_corpus();

    let best_practices = files
        .iter()
        .filter(|p| {
            p.strip_prefix(root.join("examples"))
                .is_ok_and(|rel| rel.to_string_lossy().starts_with(BEST_PRACTICES_PREFIX))
        })
        .count();

    let mut violations = Vec::new();
    let mut ops_reached = 0usize;
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read corpus member {rel}: {e}"));
        let outcome = sweep_source(&rel, &source);
        ops_reached += outcome.ops_reached;
        violations.extend(outcome.violations);
    }

    eprintln!(
        "units-length corpus end state: {} example(s) swept ({best_practices} under \
         examples/{BEST_PRACTICES_PREFIX}), {ops_reached} geometry op(s) reached the mock \
         kernel, {} units rejection(s)",
        files.len(),
        violations.len(),
    );

    assert!(
        files.len() >= CORPUS_FLOOR,
        "the examples corpus walk found only {} file(s), below the {CORPUS_FLOOR} floor — \
         a sweep over a corpus that is not there reports zero violations for the wrong \
         reason",
        files.len(),
    );
    assert!(
        best_practices >= 1,
        "no file under examples/{BEST_PRACTICES_PREFIX} was walked — that directory is \
         part of what §6 row 20 covers, and a reorg must not drop it silently"
    );
    assert!(
        ops_reached >= OPS_REACH_FLOOR,
        "only {ops_reached} geometry op(s) reached the mock kernel, below the \
         {OPS_REACH_FLOOR} floor — a sweep that builds nothing finds nothing"
    );

    assert!(
        violations.is_empty(),
        "§6 row 20: the shipped corpus needed ZERO length migrations, so every one of \
         these is a regression in a gate, an example, or both:\n{}",
        violations
            .iter()
            .map(|v| format!("  {}: {}", v.rel, v.message))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

/// The seeded positive: the SAME checker, run over a source that IS bare, must fire.
///
/// This is what makes the sweep's silence meaningful. Without it, a checker filtering on
/// a code no producer emits any more would sweep the corpus, find nothing, and be
/// indistinguishable from a corpus that is genuinely clean.
#[test]
fn the_corpus_checker_fires_on_a_seeded_bare_length() {
    let outcome = sweep_source(
        "<seeded>",
        r#"
        structure def SeededBareBox {
            let body = box(20, 20, 10)
        }
        "#,
    );

    assert_eq!(
        outcome.violations.len(),
        1,
        "a bare box is one gesture and draws ONE coded rejection naming every slot; \
         got: {:?}",
        outcome.violations,
    );
    let message = &outcome.violations[0].message;
    for needle in ["box", "width argument expects", "Length"] {
        assert!(
            message.contains(needle),
            "the seeded rejection should name `{needle}`; got: {message}"
        );
    }
}
