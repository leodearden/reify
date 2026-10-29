//! The session's reload pipeline: every way a module enters an
//! `EngineSession` runs one compile → check → commit → telemetry → build
//! sequence (PRD docs/prds/v0_6/gui-backend-engine-modules-events-tool-contract.md
//! §4.2, §5 B2').
//!
//! The pipeline is observed through a single ordered [`TelemetryLog`] over the
//! three channels that fire unconditionally once per reload: the solve slot
//! around `check` (`solve_started` / `solve_finished`) and the two FEA
//! emitters every commit fires (including empty payloads). One successful
//! reload of a non-FEA design is therefore exactly [`ONE_RELOAD`]: a dropped
//! pipeline records fewer entries, a doubled one more, and the order pins that
//! state is committed (check finished) before telemetry fires. A rejected
//! compile records nothing.
//!
//! Every commit, injected (`load_from_compiled`) or compiled, resets the
//! session's caches through the one reset list (§5 B2).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use reify_constraints::SimpleConstraintChecker;
use reify_core::ModulePath;
use reify_eval::CancellationHandle;
use reify_test_support::{
    CompiledModuleBuilder, MockGeometryKernel, TopologyTemplateBuilder, bracket_source,
};

use crate::engine::{
    EngineSession, FeaConvergenceEmitter, FeaDiagnosticsEmitter, SolveCancellationSink,
};
use crate::tests::test_helpers::{rigid_mass_props_fixture_path, rigid_mass_props_session};
use crate::types::{FeaConvergenceInfo, FeaDiagnosticInfo};

/// What one successful reload of a non-FEA design records, in order.
const ONE_RELOAD: [&str; 4] = [
    "solve_started",
    "solve_finished",
    "fea_diagnostics",
    "fea_convergence",
];

/// A source that fails to parse.
const UNPARSEABLE_SRC: &str = "this is not valid {{{}}}";

/// A source that parses but fails to compile: the default names nothing.
const UNCOMPILABLE_SRC: &str = "structure Bolt { param mass: Length = no_such_name }";

/// One ordered log shared by the solve-slot sink and both FEA emitters.
#[derive(Clone, Default)]
struct TelemetryLog(Arc<Mutex<Vec<&'static str>>>);

impl TelemetryLog {
    fn push(&self, entry: &'static str) {
        self.0.lock().unwrap().push(entry);
    }

    fn take(&self) -> Vec<&'static str> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }

    fn install_on(&self, session: &mut EngineSession) {
        session.set_solve_cancel_sink(Arc::new(self.clone()));
        session.set_fea_diagnostics_emitter(Arc::new(self.clone()));
        session.set_fea_convergence_emitter(Arc::new(self.clone()));
    }
}

impl SolveCancellationSink for TelemetryLog {
    fn solve_started(&self, _handle: CancellationHandle) {
        self.push("solve_started");
    }

    fn solve_finished(&self) {
        self.push("solve_finished");
    }
}

impl FeaDiagnosticsEmitter for TelemetryLog {
    fn changed(&self, _payload: Vec<FeaDiagnosticInfo>) {
        self.push("fea_diagnostics");
    }
}

impl FeaConvergenceEmitter for TelemetryLog {
    fn changed(&self, _payload: Option<FeaConvergenceInfo>) {
        self.push("fea_convergence");
    }
}

fn mock_session() -> EngineSession {
    EngineSession::new(
        reify_eval::SolverPosture::SolverFree,
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    )
}

/// A fresh session with a [`TelemetryLog`] installed.
fn logged_session() -> (EngineSession, TelemetryLog) {
    let mut session = mock_session();
    let log = TelemetryLog::default();
    log.install_on(&mut session);
    (session, log)
}

fn bracket_path() -> PathBuf {
    Path::new(&super::gui_crate_manifest_dir())
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/bracket.ri")
}

/// `source` written to `<fresh tempdir>/<file_name>`; the dir lives as long as
/// the returned guard.
fn source_file(file_name: &str, source: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(file_name);
    std::fs::write(&path, source).unwrap();
    (dir, path)
}

fn simple_compiled_module() -> reify_compiler::CompiledModule {
    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(TopologyTemplateBuilder::new("Simple").build())
        .build()
}

#[test]
fn every_reload_entry_point_goes_through_reload() {
    let bracket = bracket_path();
    let bracket_str = bracket.to_str().unwrap();

    let (mut session, log) = logged_session();
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source");
    assert_eq!(log.take(), ONE_RELOAD, "load_from_source");

    session
        .update_source("bracket.ri", bracket_source())
        .expect("update_source on a load_from_source session (single-file branch)");
    assert_eq!(log.take(), ONE_RELOAD, "update_source, single-file branch");

    let (mut session, log) = logged_session();
    session.load_file(&bracket).expect("load_file");
    assert_eq!(log.take(), ONE_RELOAD, "load_file");

    session
        .update_source(bracket_str, bracket_source())
        .expect("update_source on a load_file session (multi-file branch)");
    assert_eq!(log.take(), ONE_RELOAD, "update_source, multi-file branch");

    let (mut session, log) = logged_session();
    session
        .load_from_compiled(simple_compiled_module(), "test")
        .expect("load_from_compiled");
    assert_eq!(log.take(), ONE_RELOAD, "load_from_compiled");
}

#[test]
fn a_rejected_compile_runs_no_part_of_the_pipeline() {
    let (_dir, broken_path) = source_file("broken.ri", UNPARSEABLE_SRC);

    let (mut from_source, from_source_log) = logged_session();
    let from_source_err = from_source
        .load_from_source(UNPARSEABLE_SRC, "broken")
        .expect_err("load_from_source of unparseable text");

    let (mut from_file, from_file_log) = logged_session();
    let from_file_err = from_file
        .load_file(&broken_path)
        .expect_err("load_file of unparseable text");

    let (mut updated, updated_log) = logged_session();
    updated
        .load_from_source(bracket_source(), "bracket")
        .expect("prime");
    updated_log.take();
    let updated_err = updated
        .update_source("bracket.ri", UNPARSEABLE_SRC)
        .expect_err("update_source of unparseable text");

    for (entry_point, err, session, log) in [
        (
            "load_from_source",
            from_source_err,
            &from_source,
            &from_source_log,
        ),
        ("load_file", from_file_err, &from_file, &from_file_log),
        ("update_source", updated_err, &updated, &updated_log),
    ] {
        assert!(
            err.starts_with("Parse errors"),
            "{entry_point}: Err must report the parse failure, got {err:?}"
        );
        assert_eq!(
            log.take(),
            Vec::<&str>::new(),
            "{entry_point}: a rejected compile must neither solve nor emit"
        );
        assert!(
            session.compile_failure_for_test().is_some(),
            "{entry_point}: a rejected compile must be recorded for the diagnostics panel"
        );
    }
}

#[test]
fn both_compile_helpers_reject_compile_errors_alike() {
    let (_dir, simple_path) = source_file("simple.ri", UNCOMPILABLE_SRC);

    let mut single_file = mock_session();
    let single_file_err = single_file
        .load_from_source(UNCOMPILABLE_SRC, "simple")
        .expect_err("load_from_source of uncompilable text");

    let mut with_imports = mock_session();
    let with_imports_err = with_imports
        .load_file(&simple_path)
        .expect_err("load_file of uncompilable text");

    assert!(
        single_file_err.starts_with("Compile errors"),
        "the source must parse and fail compile, got {single_file_err:?}"
    );
    assert_eq!(single_file_err, with_imports_err);

    let single_file_diags = &single_file.compile_failure_for_test().unwrap().diags;
    let with_imports_diags = &with_imports.compile_failure_for_test().unwrap().diags;
    assert!(
        !single_file_diags.is_empty(),
        "a rejected compile carries its diagnostics"
    );
    assert_eq!(single_file_diags, with_imports_diags);
    assert!(
        single_file_diags.iter().all(|d| d.severity == "Error"),
        "only error-severity diagnostics reject a compile, got {single_file_diags:?}"
    );
}

/// A rigid-body session that has also rejected a live edit and recorded a
/// reload error, so every reset-list entry has something to clear.
fn primed_rigid_session() -> EngineSession {
    let fixture = rigid_mass_props_fixture_path();
    let mut session = rigid_mass_props_session();
    session.load_file(&fixture).expect("load rigid fixture");
    session
        .update_source(fixture.to_str().unwrap(), UNPARSEABLE_SRC)
        .expect_err("update_source of unparseable text");
    session.record_reload_error("stale".into());
    session
}

/// The resets a commit performs, as observed through the session's accessors.
/// Parse-derived caches are left out: `load_from_compiled` commits no source.
fn observed_resets(session: &EngineSession) -> (Vec<reify_core::ValueCellId>, bool, bool, bool) {
    let mut retained: Vec<_> = session
        .geometry_derived_cache_for_test()
        .keys()
        .cloned()
        .collect();
    retained.sort();
    (
        retained,
        session.compile_failure_for_test().is_none(),
        session.is_stale(),
        session.consumed_idents_cache_for_test().is_none(),
    )
}

#[test]
fn load_from_compiled_resets_like_commit_state() {
    const SRC: &str = "structure Bolt { param mass: Length = 1mm }";

    let mut injected = primed_rigid_session();
    let mut compiled_from_source = primed_rigid_session();
    assert!(
        !injected.geometry_derived_cache_for_test().is_empty(),
        "precondition: the rigid fixture must leave mass-prop values retained"
    );

    let parsed = reify_compiler::parse_with_stdlib(SRC, ModulePath::single("simple"));
    let compiled = reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker);
    assert!(
        compiled.diagnostics.is_empty(),
        "load_from_compiled requires a diagnostic-free module, got {:?}",
        compiled.diagnostics
    );

    injected
        .load_from_compiled(compiled, "simple")
        .expect("load_from_compiled");
    compiled_from_source
        .load_from_source(SRC, "simple")
        .expect("load_from_source");

    let leaked: Vec<_> = injected.geometry_derived_cache_for_test().keys().collect();
    assert!(
        leaked.is_empty(),
        "load_from_compiled kept the previous module's geometry-derived values: {leaked:?}"
    );
    assert_eq!(
        observed_resets(&injected),
        observed_resets(&compiled_from_source),
        "load_from_compiled and load_from_source must reset the same state"
    );
}

/// `load_from_compiled` breaks `module_name` before it builds the state it
/// returns, so that build — like every later one on the session — runs over
/// the unresolved source: an injected module carrying a diagnostic trips
/// `get_diagnostics`' unresolved-source assert during the load itself.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "resolve_source returned None with non-empty diagnostics")]
fn load_from_compiled_builds_its_state_over_the_unresolved_source() {
    let compiled = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(TopologyTemplateBuilder::new("Simple").build())
        .diagnostic(reify_core::Diagnostic::warning("injected"))
        .build();
    let _ = mock_session().load_from_compiled(compiled, "test");
}
