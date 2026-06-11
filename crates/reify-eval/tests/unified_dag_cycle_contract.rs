//! Integration gate for the unified build-DAG driver wired into `Engine::build()`
//! (task 4357 δ, step-17/18).
//!
//! The exhaustive cycle-contract coverage (every §6 acceptance bar, determinism,
//! the auto-read closure) lives in the `engine_fixpoint` unit tests (steps 9–16),
//! which drive `run_unified_pass` over synthetic graphs directly. THIS file proves
//! only the user-observable wiring: that `Engine::build()`, when the active
//! [`BuildScheduler`] is `UnifiedDag`, forwards the driver's diagnostics onto the
//! `BuildResult`, and that the `LegacyMultiPass` default is byte-preserving.
//!
//! The scheduler is selected through the deterministic test seam
//! `Engine::set_build_scheduler` (a `#[cfg(any(test, feature =
//! "test-instrumentation"))]` setter that forces the selection DIRECTLY,
//! independent of the `unified-dag` cargo feature and WITHOUT mutating process
//! env — so these tests stay parallel-safe). The integration tests reach the
//! test-instrumentation-gated setter via the self-dev-dep with the
//! `test-instrumentation` feature enabled (see `crates/reify-eval/Cargo.toml`).

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, Severity};
use reify_eval::{BuildScheduler, Engine};
use reify_ir::ExportFormat;
use reify_test_support::{MockGeometryKernel, compile_source};

/// One diagnostic projected to the `(code, message, severity)` triple the
/// byte-preserving / code-presence assertions compare over.
type DiagTriple = (Option<DiagnosticCode>, String, Severity);

/// Compile `source`, build it on a FRESH engine under the given `scheduler`, and
/// return the resulting diagnostics as comparable triples.
///
/// A fresh engine per call guarantees the cold-start `eval()` path runs (which
/// populates `eval_state.trace_map`); a second build on the same engine would
/// hit the `eval_cached` path and is irrelevant here.
fn build_under(source: &str, scheduler: BuildScheduler) -> Vec<DiagTriple> {
    let compiled = compile_source(source);
    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    engine.set_build_scheduler(scheduler);
    let result = engine.build(&compiled, ExportFormat::Step);
    result
        .diagnostics
        .iter()
        .map(|d| (d.code, d.message.clone(), d.severity))
        .collect()
}

/// True if any diagnostic in `diags` carries `code`.
fn carries(diags: &[DiagTriple], code: DiagnosticCode) -> bool {
    diags.iter().any(|(c, _, _)| *c == Some(code))
}

/// PRIMARY (must-pass) — the Stage-1 residue==∅ gate, observed through `build()`.
///
/// An acyclic, legacy-passing module — a Boolean union over two sub-realizations,
/// exercising realization→realization edges in the trace map — built under
/// `UnifiedDag` must:
///   (a) surface NO `EvalCycle` / `EvalUnresolved` diagnostics (the driver's
///       residue is empty and no constraint reaches an auto cell), AND
///   (b) produce a `BuildResult` whose diagnostic vector is byte-identical to the
///       `LegacyMultiPass` build's — the unified pass adds zero diagnostics on an
///       acyclic graph, so the default legacy behaviour is preserved exactly.
#[test]
fn unified_dag_acyclic_module_is_byte_preserving() {
    let source = r#"pub structure A {
    let part = box(10mm, 10mm, 10mm)
}
pub structure B {
    let part = box(5mm, 5mm, 5mm)
}
pub structure C {
    sub a = A()
    sub b = B()
    let result = union(self.a.part, self.b.part)
}"#;

    let legacy = build_under(source, BuildScheduler::LegacyMultiPass);
    let unified = build_under(source, BuildScheduler::UnifiedDag);

    // (a) no unified-pass diagnostics leak in on an acyclic module.
    assert!(
        !carries(&unified, DiagnosticCode::EvalCycle)
            && !carries(&unified, DiagnosticCode::EvalUnresolved),
        "acyclic module must not surface EvalCycle/EvalUnresolved under UnifiedDag; got: {unified:?}"
    );

    // (b) byte-preserving: UnifiedDag diagnostics == LegacyMultiPass diagnostics.
    assert_eq!(
        unified, legacy,
        "UnifiedDag must be byte-preserving vs LegacyMultiPass on an acyclic module"
    );
}

/// SECONDARY — a cyclic input surfaces `EvalCycle` under `UnifiedDag` while the
/// legacy path is unchanged.
///
/// `let a = b + 1.0` / `let b = a + 1.0` is a mutual let-cycle. The compiler
/// accepts it (the cycle is an eval-time property), and `detect_let_cycle` emits
/// its own un-coded "circular let-binding dependency" diagnostic WITHOUT halting
/// — so `eval()` still populates `eval_state.trace_map` with the cyclic edges.
/// Under `UnifiedDag`, `build()` forwards the driver's structured
/// `DiagnosticCode::EvalCycle`; under `LegacyMultiPass` no `EvalCycle` code is
/// ever attached (the legacy path emits only the un-coded string diagnostic).
#[test]
fn unified_dag_cyclic_module_surfaces_eval_cycle() {
    let source = "structure S {\n    let a = b + 1.0\n    let b = a + 1.0\n}";

    let legacy = build_under(source, BuildScheduler::LegacyMultiPass);
    let unified = build_under(source, BuildScheduler::UnifiedDag);

    // UnifiedDag forwards the driver's structured EvalCycle code.
    assert!(
        carries(&unified, DiagnosticCode::EvalCycle),
        "UnifiedDag must surface DiagnosticCode::EvalCycle on a cyclic module; got: {unified:?}"
    );

    // Legacy path unchanged: it never attaches the EvalCycle code (it emits its
    // own un-coded "circular let-binding dependency" diagnostic instead).
    assert!(
        !carries(&legacy, DiagnosticCode::EvalCycle),
        "LegacyMultiPass must NOT carry DiagnosticCode::EvalCycle; got: {legacy:?}"
    );
}
