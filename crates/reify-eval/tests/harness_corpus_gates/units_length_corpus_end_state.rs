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
//! broken sweep read as green, so it is paid for by four companions in this file, all
//! required: the exactly-pinned skip set, the swept-corpus floor, the seeded positive,
//! and the op-reach floor.
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
//! that has nothing to do with it; the skip is REPORTED, and pinned by name and cause in
//! [`KNOWN_SKIPS`], so it can never grow into a silent hole.

use crate::eval_gate_support;
use reify_core::{DiagnosticCode, Severity};
use reify_eval::Engine;
use reify_ir::ExportFormat;
use reify_test_support::{MockGeometryKernel, compile_source_with_stdlib};

/// The `examples/` subtree whose contribution is asserted separately, so a reorg that
/// moved it out from under the walk cannot silently shrink the corpus to zero of it.
const BEST_PRACTICES_PREFIX: &str = "best_practices/";

/// Files the sweep must actually BUILD, not merely walk.
///
/// Asserted on the swept count rather than the walked one so that a regression which made
/// every member skip — the one way this gate could go silently vacuous — reds here. A
/// FLOOR, not an equality: adding examples must never break the gate, losing most of them
/// must.
///
/// It is the cheap BACKSTOP, not the guard: [`KNOWN_SKIPS`] pins the skip set exactly, so
/// a single new skip reds long before the count approaches this floor.
const SWEPT_FLOOR: usize = 200;

/// Geometry ops observed reaching the mock kernel across the whole sweep.
///
/// Without this, "built nothing, found nothing" would read exactly like "built everything,
/// found nothing". Measured at 401 over the 257 members built, and unchanged at 401 across
/// all four engine wirings [`realize`] tabulates. The floor is half that, for the same
/// reason as [`SWEPT_FLOOR`] — it must red when realization collapses, not when a single
/// example changes shape.
const OPS_REACH_FLOOR: usize = 200;

/// One corpus member the sweep is KNOWN not to build, and why.
///
/// `SWEPT_FLOOR` alone bounds the skips only in bulk: with 264 members walked and a floor
/// of 200, some fifty more could quietly degrade from BUILT to SKIPPED — a new compile
/// error, or a fresh `Engine::build` panic swallowed by `catch_unwind` — and row 20's
/// claim would shrink with them while this gate stayed green. So the skip SET is pinned
/// exactly, in the shape
/// `eval_invariant_corpus_sweep::residual_exemptions_and_failure_policy_stay_per_invariant`
/// already uses for its residual exemptions: a new skip reds naming the file and its
/// cause, and a member that starts building again reds so its dead entry is deleted.
///
/// The CAUSE is pinned; the detail text is not. Both causes are pre-existing engine
/// defects unrelated to units (see the module header), so a reworded engine diagnostic
/// must not red a units gate.
struct KnownSkip {
    /// Workspace-relative path, exactly as the sweep reports it.
    rel: &'static str,
    cause: SkipCause,
    /// One line on what is actually wrong, for the reader who hits this entry.
    why: &'static str,
    when: SkipsWhen,
}

/// Which builds the skip is expected in. A skip caused by a `debug_assert!` does not
/// happen in a release build (the gate runs both), so pinning it for every build makes the
/// release pass red it as a dead entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SkipsWhen {
    Always,
    DebugAssertionsOn,
}

impl KnownSkip {
    fn expected_in_this_build(&self) -> bool {
        match self.when {
            SkipsWhen::Always => true,
            SkipsWhen::DebugAssertionsOn => cfg!(debug_assertions),
        }
    }
}

/// The subset of [`KNOWN_SKIPS`] this build is expected to observe.
fn known_skips_in_this_build() -> Vec<&'static KnownSkip> {
    KNOWN_SKIPS
        .iter()
        .filter(|k| k.expected_in_this_build())
        .collect()
}

/// Every member this sweep is known not to build. Measured on this tree, not predicted.
///
/// Seven of 264 with debug assertions on, six in a release build. Every one is a pre-existing condition of the example itself — four are
/// negative fixtures that are SUPPOSED to fail to compile — and not one is a units
/// rejection, which is what makes tolerating them compatible with row 20's claim.
const KNOWN_SKIPS: &[KnownSkip] = &[
    KnownSkip {
        rel: "examples/auto/bearing_computed_default_unevaluated.ri",
        cause: SkipCause::UnrelatedCompileError,
        why: "auto type parameter has two feasible candidates for bound 'Seal', so \
              `TypeParam` stays unresolved",
        when: SkipsWhen::Always,
    },
    KnownSkip {
        rel: "examples/auto/bearing_constraint_select.ri",
        cause: SkipCause::UnrelatedCompileError,
        why: "same auto-type-parameter ambiguity on bound 'Seal'",

        when: SkipsWhen::Always,
    },
    KnownSkip {
        rel: "examples/auto/bearing_unsat.ri",
        cause: SkipCause::UnrelatedCompileError,
        why: "same auto-type-parameter ambiguity on bound 'Seal'",

        when: SkipsWhen::Always,
    },
    KnownSkip {
        rel: "examples/conditional_compilation/main.ri",
        cause: SkipCause::UnrelatedCompileError,
        why: "type `Platform` is supplied by the conditional-compilation selection this \
              single-module compile does not perform",
        when: SkipsWhen::Always,
    },
    KnownSkip {
        rel: "examples/module_visibility/consumer.ri",
        cause: SkipCause::UnrelatedCompileError,
        why: "sub-component references structure `Motor` from a sibling module that a \
              single-module compile does not load",
        when: SkipsWhen::Always,
    },
    KnownSkip {
        rel: "examples/multi_aspect_objective_mixed.ri",
        cause: SkipCause::UnrelatedCompileError,
        why: "a NEGATIVE fixture: its objective deliberately mixes Money with Mass \
              (E_OBJECTIVE_MIXED_DIMENSION)",
        when: SkipsWhen::Always,
    },
    KnownSkip {
        rel: "examples/integration_corner_cases.ri",
        cause: SkipCause::BuildPanic,
        why: "the pre-existing `RecTree.child`/`depth` scoped-override debug_assert named \
              in this module's header; `reify build` panics on it too",
        when: SkipsWhen::DebugAssertionsOn,
    },
];

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

/// Why a corpus member could not be built. Structured rather than sniffed back out of
/// the detail text, so [`KNOWN_SKIPS`] pins a cause without pinning any wording.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SkipCause {
    /// Compilation reported an Error that is not the `ArgTypeMismatch` a bare length
    /// draws, so there is nothing to build.
    UnrelatedCompileError,
    /// `Engine::build` panicked. Pre-existing engine defects, not units ones — see the
    /// module header.
    BuildPanic,
}

/// One member the sweep could not build: why, and the detail a reader needs to act on it.
struct Skip {
    cause: SkipCause,
    /// The compile message or panic payload. REPORTED, never asserted on — pinning it
    /// would turn a reworded engine diagnostic into a red units gate.
    detail: String,
}

/// What became of one corpus member.
enum FileOutcome {
    Swept(SweepOutcome),
    /// Could not be built, for a reason that is not a units rejection.
    Skipped(Skip),
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
        return FileOutcome::Skipped(Skip {
            cause: SkipCause::UnrelatedCompileError,
            detail: unrelated.message.clone(),
        });
    }

    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| realize(rel, &compiled))) {
        Ok(outcome) => FileOutcome::Swept(outcome),
        Err(payload) => FileOutcome::Skipped(Skip {
            cause: SkipCause::BuildPanic,
            detail: panic_text(&payload),
        }),
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
///
/// WHY THIS IS NOT `eval_gate_support::gate_engine(true)`, the constructor whose own doc
/// names it "the ONE engine-construction site every corpus-wide invariant gate routes
/// through". Two reasons, one structural and one measured, and neither is an oversight.
///
/// Structural: this gate needs the kernel's `operations_ref()` handle for [`OPS_REACH_FLOOR`],
/// and `gate_engine` hands back the `Engine` alone, having already boxed away the kernel
/// it built. Calling it is not possible without widening its signature and migrating its
/// three existing callers — the right end state, and filed as follow-up work, because it
/// reaches outside this gate's own file.
///
/// Measured: the checker half of that wiring IS adopted here — `SimpleConstraintChecker`,
/// what every other corpus-wide gate evaluates through, in place of a mock that answers
/// nothing. `register_production_compute_fns` is NOT, and the reason is a four-way
/// back-to-back measurement of this sweep over all 264 members:
///
/// | wiring | wall clock | geometry ops reaching the kernel |
/// |---|---|---|
/// | mock checker, no compute fns | 36.8s | 401 |
/// | `SimpleConstraintChecker`, no compute fns (THIS) | 40.9s | 401 |
/// | mock checker + `register_production_compute_fns` | 347.2s | 401 |
/// | `gate_engine(true)`'s exact wiring | 416.4s | 401 |
///
/// Registering the production trampolines costs 9.4x wall clock and moves the op reach by
/// ZERO. That equality is the load-bearing half of the evidence, not the timing: an op
/// dropped before the LENGTH gate never reaches the kernel, so a degraded compute dispatch
/// that hid one would show a LOWER count. It does not — no geometry op in this corpus sits
/// behind an `@optimized` trampoline, and row 20 sees the same surface either way. If that
/// ever changes the registration must come back, and the sweep must be sharded on the
/// sibling's `CORPUS_SHARD_COUNT` idiom first so the merge gate is not handed a 400s
/// straggler.
fn realize(rel: &str, compiled: &reify_compiler::CompiledModule) -> SweepOutcome {
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
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
    let mut skipped: Vec<(String, Skip)> = Vec::new();
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
            FileOutcome::Skipped(skip) => skipped.push((rel, skip)),
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
    for (rel, skip) in &skipped {
        eprintln!(
            "  SKIP (not built): {rel}: {:?}: {}",
            skip.cause, skip.detail
        );
    }

    assert_skip_set_is_the_pinned_one(&skipped);

    assert!(
        swept >= SWEPT_FLOOR,
        "only {swept} of {} walked example(s) were actually BUILT, below the \
         {SWEPT_FLOOR} floor — a sweep that skips the corpus reports zero violations for \
         the wrong reason. Skips:\n  {}",
        files.len(),
        describe_skips(&skipped).join("\n  "),
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

/// Skips as readable lines, for a failure message.
fn describe_skips(skipped: &[(String, Skip)]) -> Vec<String> {
    skipped
        .iter()
        .map(|(rel, skip)| format!("{rel}: {:?}: {}", skip.cause, skip.detail))
        .collect()
}

/// The observed skip set must be exactly [`KNOWN_SKIPS`] — same files, same causes.
///
/// Both directions are failures. A file that newly stops building shrinks what row 20
/// actually covers; a pinned file that starts building again leaves a dead entry that
/// would mask the next real regression.
fn assert_skip_set_is_the_pinned_one(skipped: &[(String, Skip)]) {
    let observed: Vec<(&str, SkipCause)> = skipped
        .iter()
        .map(|(rel, skip)| (rel.as_str(), skip.cause))
        .collect();

    let expected = known_skips_in_this_build();

    let unexpected: Vec<&String> = skipped
        .iter()
        .filter(|(rel, skip)| {
            !expected
                .iter()
                .any(|k| k.rel == rel && k.cause == skip.cause)
        })
        .map(|(rel, _)| rel)
        .collect();
    let stale: Vec<String> = expected
        .iter()
        .filter(|k| {
            !observed
                .iter()
                .any(|(rel, cause)| *rel == k.rel && *cause == k.cause)
        })
        .map(|k| format!("{} ({:?}: {})", k.rel, k.cause, k.why))
        .collect();

    assert!(
        unexpected.is_empty() && stale.is_empty(),
        "the sweep's skip set must be exactly the {} KNOWN_SKIPS expected in this build.\n\
         NEW skip(s) — each shrinks what row 20 covers, so fix the cause or pin it with \
         a reason: {unexpected:?}\n\
         PINNED but now building — delete the dead entry, or the next real regression \
         hides behind it: {stale:?}\n\
         observed:\n  {}",
        expected.len(),
        describe_skips(skipped).join("\n  "),
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
