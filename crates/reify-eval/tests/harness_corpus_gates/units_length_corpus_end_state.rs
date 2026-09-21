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
//! required: the swept-corpus floor, the seeded positive, and the op-reach floor.
//!
//! THE SKIP IS NARROW ON PURPOSE, and this is the file's one subtle decision. A corpus
//! member that fails to COMPILE cannot be built, so it must be skipped — but "skip
//! anything that fails to compile" would be self-defeating here: a bare length draws a
//! compile-layer `ArgTypeMismatch` as well as the build-layer rejection this gate looks
//! for, so that rule would skip precisely the files row 20 exists to catch, and the gate
//! would report zero violations over a corpus full of them. Only errors that are NOT
//! `ArgTypeMismatch` skip a file. Measured cause, not a hypothetical one:
//! `examples/auto/bearing_computed_default_unevaluated.ri` compiles with an
//! auto-type-param ambiguity Error, which leaves `TypeParam("T")` unresolved and makes
//! `Engine::build` panic at `crates/reify-eval/src/engine_eval.rs:210`.
//!
//! A BUILD PANIC IS THE SECOND SKIP CAUSE, and it is a pre-existing defect this gate
//! surfaced rather than one it introduced: `./target/debug/reify build
//! examples/integration_corner_cases.ri` panics identically at
//! `crates/reify-eval/src/engine_build.rs:716` (`expected scoped override cell
//! ValueCellId { entity: "RecTree.child", member: "depth" } in values map`), so the
//! production CLI reaches it too in any build with debug assertions on. That assertion is
//! a `debug_assert!` whose release fallback is documented as benign, which is why it has
//! gone unnoticed. Catching it keeps one latent engine bug from blocking a units gate
//! that has nothing to do with it; the skip is REPORTED and bounded by [`SWEPT_FLOOR`],
//! so it can never grow into a silent hole.

use crate::eval_gate_support;
use reify_core::{DiagnosticCode, Severity};
use reify_eval::Engine;
use reify_ir::ExportFormat;
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, compile_source_with_stdlib};

/// The `examples/` subtree whose contribution is asserted separately, so a reorg that
/// moved it out from under the walk cannot silently shrink the corpus to zero of it.
const BEST_PRACTICES_PREFIX: &str = "best_practices/";

/// Files the sweep must actually BUILD, not merely walk.
///
/// Asserted on the swept count rather than the walked one so that a regression which made
/// every member skip — the one way this gate could go silently vacuous — reds here. A
/// FLOOR, not an equality: adding examples must never break the gate, losing most of them
/// must.
const SWEPT_FLOOR: usize = 200;

/// Geometry ops observed reaching the mock kernel across the whole sweep.
///
/// Without this, "built nothing, found nothing" would read exactly like "built everything,
/// found nothing". Measured at 401 over the 257 members built when this landed; the floor
/// is half that, for the same reason as [`SWEPT_FLOOR`] — it must red when realization
/// collapses, not when a single example changes shape.
const OPS_REACH_FLOOR: usize = 200;

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

/// What became of one corpus member.
enum FileOutcome {
    Swept(SweepOutcome),
    /// Could not be built, for a reason that is not a units rejection. Carries the
    /// unrelated compile error so a skip is readable rather than a silent hole.
    Skipped(String),
}

/// THE checker. The corpus sweep and the seeded positive both go through this one
/// function, which is what makes the seed's red a statement about the sweep rather than
/// about a parallel implementation of it.
fn sweep_source(rel: &str, source: &str) -> FileOutcome {
    let compiled = compile_source_with_stdlib(source);

    if let Some(unrelated) = compiled
        .diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error && d.code != Some(DiagnosticCode::ArgTypeMismatch))
    {
        return FileOutcome::Skipped(unrelated.message.clone());
    }

    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| realize(rel, &compiled))) {
        Ok(outcome) => FileOutcome::Swept(outcome),
        Err(payload) => FileOutcome::Skipped(format!("build panicked: {}", panic_text(&payload))),
    }
}

/// The payload of a caught panic, as text, so a skip line says WHY rather than just that.
fn panic_text(payload: &Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".to_string())
}

/// Realize `compiled` against a mock kernel and collect what this gate reads off it.
fn realize(rel: &str, compiled: &reify_compiler::CompiledModule) -> SweepOutcome {
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result = engine.build(compiled, ExportFormat::Step);

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
/// The three floors are not decoration. Tolerating unbuildable members (see the module
/// header) means a sweep that skipped everything, or walked an empty directory, would
/// report zero violations and read as green. Each floor closes one of those: enough
/// members were genuinely BUILT, `examples/best_practices/` was among them, and geometry
/// really reached the kernel.
#[test]
fn no_shipped_example_trips_a_length_gate() {
    let (root, files) = examples_corpus();
    let examples = root.join("examples");

    let mut violations = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut ops_reached = 0usize;
    let mut swept = 0usize;
    let mut swept_best_practices = 0usize;

    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read corpus member {rel}: {e}"));

        match sweep_source(&rel, &source) {
            FileOutcome::Skipped(reason) => skipped.push(format!("{rel}: {reason}")),
            FileOutcome::Swept(outcome) => {
                swept += 1;
                if path
                    .strip_prefix(&examples)
                    .is_ok_and(|r| r.to_string_lossy().starts_with(BEST_PRACTICES_PREFIX))
                {
                    swept_best_practices += 1;
                }
                ops_reached += outcome.ops_reached;
                violations.extend(outcome.violations);
            }
        }
    }

    eprintln!(
        "units-length corpus end state: {} example(s) walked, {swept} built \
         ({swept_best_practices} under examples/{BEST_PRACTICES_PREFIX}), {} skipped, \
         {ops_reached} geometry op(s) reached the mock kernel, {} units rejection(s)",
        files.len(),
        skipped.len(),
        violations.len(),
    );
    for skip in &skipped {
        eprintln!("  SKIP (not built): {skip}");
    }

    assert!(
        swept >= SWEPT_FLOOR,
        "only {swept} of {} walked example(s) were actually BUILT, below the \
         {SWEPT_FLOOR} floor — a sweep that skips the corpus reports zero violations for \
         the wrong reason. Skips:\n  {}",
        files.len(),
        skipped.join("\n  "),
    );
    assert!(
        swept_best_practices >= 1,
        "no file under examples/{BEST_PRACTICES_PREFIX} was built — that directory is \
         part of what §6 row 20 covers, and neither a reorg nor a blanket skip may drop \
         it silently"
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
/// indistinguishable from a corpus that is genuinely clean. It also pins the narrow skip
/// described in the module header: a bare length draws a compile-layer `ArgTypeMismatch`,
/// so any skip rule broader than "errors that are not `ArgTypeMismatch`" would skip this
/// seed, and this test would red.
#[test]
fn the_corpus_checker_fires_on_a_seeded_bare_length() {
    let FileOutcome::Swept(outcome) = sweep_source(
        "<seeded>",
        r#"
        structure def SeededBareBox {
            let body = box(20, 20, 10)
        }
        "#,
    ) else {
        panic!(
            "the seeded bare box must be BUILT, not skipped — its only compile Error is \
             the ArgTypeMismatch the narrow skip deliberately does not act on"
        );
    };

    // THREE, measured: a box is one gesture but each slot draws its own coded
    // diagnostic, so an author fixing a bare box is told about all three in one pass
    // rather than over three edit-build cycles.
    let messages: Vec<&str> = outcome
        .violations
        .iter()
        .map(|v| v.message.as_str())
        .collect();
    assert_eq!(
        messages.len(),
        3,
        "a bare box draws one coded rejection per slot; got: {messages:?}"
    );
    for slot in ["width", "height", "depth"] {
        assert!(
            messages
                .iter()
                .any(|m| m.contains(&format!("box: {slot} argument expects Length"))),
            "the seeded rejections should name `{slot}`; got: {messages:?}"
        );
    }
}
