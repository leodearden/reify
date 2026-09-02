//! Neutral, fixture-agnostic scaffolding shared by pin tests in this harness:
//! read a `.ri` fixture, compile it against the stdlib, optionally build it
//! with a real OCCT kernel (skipping cleanly when OCCT is absent), and assert
//! individual value cells of the result.
//!
//! NOT a former standalone `tests/<file>.rs` and deliberately holds no
//! `#[test]` fn of its own — it is a support module, so nothing here shows up
//! as a `fixture_scaffolding::<test>` path in a nextest filterset.
//!
//! # Why this module exists (task #5982 amendment)
//!
//! The #5982 split originally left `best_practices_clearance_oracle` reaching
//! into its sibling via `use super::kernel_queries_intersects_smoke::{...}` —
//! the only `use super::<sibling module>` in any consolidated harness in this
//! repo. That made the KGQ-γ intersects pin the *owner* of generic
//! read/compile/build scaffolding for an unrelated fixture, so narrowing or
//! deleting that pin (e.g. when KGQ-γ coverage is reworked) would silently
//! break an unrelated `best_practices` pin. Hoisting the two helpers here
//! makes both consumers peers of a neutral unit instead: neither test module
//! owns the other's scaffolding, and a third consumer can join without
//! deepening the coupling.
//!
//! # Third consumer: inline sources (task #6269)
//!
//! `kernel_queries_intersects_smoke`'s containment pin is that third consumer,
//! and it joined without deepening the coupling in the way the paragraph above
//! anticipates. Its geometry is the SUBJECT of an assertion rather than a
//! corpus exemplar, so its source is an inline `&str` in the test module, not a
//! `.ri` under `examples/`. Rather than let it carry its own copy of the OCCT
//! gate, this module now factors the two concerns apart — one compile core, one
//! OCCT-gate/build core — and routes BOTH the path-based and the inline-source
//! entry point through them. Adding a source-shaped entry point therefore added
//! no second copy of the skip/build block.
//!
//! Every `pub(crate)` entry point below is a thin composition of those two
//! private cores, so each fn's own doc comment and signature remain the single
//! source of truth for what it does — this header deliberately keeps no
//! register of them to fall out of lockstep.
//!
//! # Cell assertions
//!
//! The same reasoning covers `assert_bool_cell` / `assert_length_cell`: a
//! pin's *reading* of a built `BuildResult` cell is as copy-pasteable as its
//! building, and a message-format tweak in one copy silently desynchronises
//! the rest. They live here so that any pin adopting them reports a failing
//! cell identically.
//!
//! Adoption is INCREMENTAL — read the paragraph above as the intent, not as a
//! description of today's harness. As of task #6269 the sole consumer is
//! `kernel_queries_intersects_smoke`; nine sibling modules still hand-roll an
//! inline `ValueCellId::new` + `assert_eq!` / `match ... Value::Scalar` at each
//! call site, the closest being `best_practices_clearance_oracle`. So the drift
//! these helpers close is still open across the rest of the harness: they are
//! AVAILABLE to every pin, and migrating one is a mechanical, self-contained
//! change for whichever task next holds that pin's lock.

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{compile_source_with_stdlib, errors_only};

/// Parses+compiles `source` against the stdlib, asserting it compiles cleanly.
/// The compile core shared by every entry point below, so the assert-clean
/// contract has exactly one implementation regardless of where the source came
/// from.
///
/// `what` is the caller-facing label used in the failure message.
///
/// Deliberately builds on the NON-asserting `compile_source_with_stdlib` rather
/// than its `parse_and_compile_with_stdlib` sibling. The sibling already runs
/// this same emptiness check internally — the
/// `assert!(errors.is_empty(), "compile errors: {:?}", errors)` in
/// `reify-test-support/src/helpers.rs` — and panics with an unlabelled
/// `compile errors: [...]`, which would make the labelled assert below
/// unreachable dead code and leave a compile regression reported without
/// naming WHICH fixture or pin broke. Going through the non-asserting entry
/// point puts `what` back in the failure message.
#[track_caller]
fn compile_source_fixture(source: &str, what: &str) -> reify_compiler::CompiledModule {
    // Validate fixture compilation unconditionally — a grammar/compile regression
    // should fail on every runner.
    let compiled = compile_source_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "{what} should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
    compiled
}

/// Either builds `compiled` with a real OCCT kernel and returns the
/// `BuildResult`, or — when OCCT is not available — emits the standard skip
/// line (naming `what`) and returns `None`, letting the caller early-return.
///
/// The OCCT-gate/build core shared by both building entry points below. This
/// is the piece most likely to diverge silently under copy-paste — a runner
/// where one helper skips and its sibling builds — so it exists exactly once
/// and every consumer inherits identical gate/skip semantics by construction.
fn build_compiled_with_occt(
    compiled: &reify_compiler::CompiledModule,
    what: &str,
) -> Option<reify_eval::BuildResult> {
    // Skip the OCCT-dependent kernel build if OCCT is not built.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions for {what}: OCCT not available");
        return None;
    }

    // Build with real OCCT kernel (SingleKernelHolder + OcctKernelHandle::spawn).
    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    Some(engine.build(compiled, ExportFormat::Step))
}

/// Reads `path` and parses+compiles it against the stdlib, asserting it
/// compiles cleanly. Runs unconditionally — no OCCT dependency — so a
/// missing fixture or a grammar/compile regression fails on every runner.
///
/// Split out of `compile_and_build_with_occt` so the read/compile/
/// assert-clean scaffolding has exactly one implementation shared by every
/// pin test that uses it — including the kernel-less check-surface tests in
/// `best_practices_clearance_oracle`, which need a compiled fixture but never
/// touch OCCT — instead of the risk of two copies (one here, one inlined in a
/// kernel-less test) silently drifting under copy-paste.
///
/// `what` is the repo-relative fixture label used in failure messages, so an
/// offender is reported portably rather than as an absolute build-machine path.
#[track_caller]
pub(crate) fn read_and_compile_fixture(path: &str, what: &str) -> reify_compiler::CompiledModule {
    // Read the fixture unconditionally so a missing file is caught even on
    // OCCT-less runners — fixture presence is a CI contract independent of OCCT.
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{what} should exist at {path}: {e}"));

    compile_source_fixture(&source, what)
}

/// Compiles `path` via `read_and_compile_fixture`, then builds it with a real
/// OCCT kernel via `build_compiled_with_occt` — returning `None` (after the
/// standard skip line) when OCCT is absent, so the caller can early-return.
///
/// Shared so the real-OCCT pin tests in this harness cannot drift against each
/// other on the skip/build scaffolding — the piece most likely to diverge
/// silently under copy-paste.
#[track_caller]
pub(crate) fn compile_and_build_with_occt(
    path: &str,
    what: &str,
) -> Option<reify_eval::BuildResult> {
    let compiled = read_and_compile_fixture(path, what);
    build_compiled_with_occt(&compiled, what)
}

/// Inline-source sibling of `compile_and_build_with_occt`: compiles `source`
/// (asserting it is error-free) and builds it with a real OCCT kernel, or emits
/// the standard skip line and returns `None` when OCCT is absent.
///
/// For a pin whose geometry is the ASSERTION's subject rather than a corpus
/// exemplar — there, the source belongs next to the assertions it exists to
/// justify, not under `examples/` where it would become user-facing and be
/// enrolled in every corpus sweep for no reader benefit. First consumer:
/// `kernel_queries_intersects_smoke::intersects_and_distance_detect_full_containment`
/// (task #6269).
///
/// Shares the OCCT gate, the skip line and the build sequence with the
/// path-based sibling by construction (both delegate to
/// `build_compiled_with_occt`), so an inline-source pin can never drift into
/// building on a runner where a path-based one skips.
///
/// `what` is the caller-facing label used in the skip and failure messages;
/// unlike the path-based helpers there is no file to name, so pass something
/// that identifies the pin (e.g. `"containment pin (inline source)"`).
#[track_caller]
pub(crate) fn build_source_with_occt(source: &str, what: &str) -> Option<reify_eval::BuildResult> {
    let compiled = compile_source_fixture(source, what);
    build_compiled_with_occt(&compiled, what)
}

/// Asserts that `struct_name.cell_name` in `result` is `Value::Bool(expected)`.
///
/// Hoisted here (task #6269 amendment) because every real-OCCT pin in this
/// harness reads its Bool cells the same way, and each copy of the read was
/// re-deriving the same `ValueCellId::new` + `get` + `assert_eq!` shape — the
/// same silent-drift risk in the *reading* of a `BuildResult` that this
/// module's build helpers already close for the *producing* of one.
#[track_caller]
pub(crate) fn assert_bool_cell(
    result: &reify_eval::BuildResult,
    struct_name: &str,
    cell_name: &str,
    expected: bool,
) {
    let cell = ValueCellId::new(struct_name, cell_name);
    let actual = result.values.get(&cell);
    assert_eq!(
        actual,
        Some(&Value::Bool(expected)),
        "{struct_name}.{cell_name} should be Value::Bool({expected}), got: {actual:?}"
    );
}

/// Asserts that `struct_name.cell_name` in `result` is a LENGTH `Value::Scalar`
/// whose `si_value` matches `expected`.
///
/// `tolerance` selects the comparison, and the choice is load-bearing rather
/// than stylistic:
///
/// - `tolerance <= 0.0` asserts EXACT `f64` equality. Correct when the kernel
///   returns a classified literal rather than a computed extremum — e.g. OCCT's
///   `SolidTreatment`/`InnerSolution()` containment path, which yields a literal
///   `0.0`. A tolerance there would be strictly weaker and would blur
///   "contained" (0.0) against "clear by a small margin" (a positive gap).
/// - `tolerance > 0.0` asserts `|actual - expected| < tolerance`. Correct for a
///   genuinely computed extremum; the caller is expected to DERIVE the epsilon
///   from the geometry's representation error rather than tune it against an
///   observed value.
///
/// Both arms name `struct_name.cell_name` and print the delta, so a failure in
/// any pin in this harness reads identically.
#[track_caller]
pub(crate) fn assert_length_cell(
    result: &reify_eval::BuildResult,
    struct_name: &str,
    cell_name: &str,
    expected: f64,
    tolerance: f64,
) {
    let cell = ValueCellId::new(struct_name, cell_name);
    let actual = result.values.get(&cell);
    match actual {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) if *dimension == reify_core::DimensionVector::LENGTH => {
            let delta = (si_value - expected).abs();
            if tolerance > 0.0 {
                assert!(
                    delta < tolerance,
                    "{struct_name}.{cell_name} si_value should be {expected} \
                     (within {tolerance:.1e}), got {si_value:.15} (delta {delta:.3e})"
                );
            } else {
                assert_eq!(
                    *si_value, expected,
                    "{struct_name}.{cell_name} si_value should be EXACTLY {expected} \
                     — asserted exact, not within a tolerance — got {si_value:.17e} \
                     (delta {delta:.3e})"
                );
            }
        }
        other => panic!(
            "{struct_name}.{cell_name} should be Value::Scalar{{LENGTH, {expected}}}, \
             got: {other:?}"
        ),
    }
}
