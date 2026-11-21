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

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use reify_constraints::SimpleConstraintChecker;
use reify_core::ModulePath;
use reify_eval::CancellationHandle;
use reify_test_support::{
    CompiledModuleBuilder, MockGeometryKernel, TopologyTemplateBuilder, bracket_source,
};

use crate::engine::{
    CompileFailureKind, EngineSession, FeaConvergenceEmitter, FeaDiagnosticsEmitter,
    SolveCancellationSink,
};
use crate::tests::test_helpers::{
    UNPARSEABLE_SRC, rigid_mass_props_fixture_path, rigid_mass_props_session,
};
use crate::types::{FeaConvergenceInfo, FeaDiagnosticInfo, FileData};

/// What one successful reload of a non-FEA design records, in order.
const ONE_RELOAD: [&str; 4] = [
    "solve_started",
    "solve_finished",
    "fea_diagnostics",
    "fea_convergence",
];

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
fn every_reload_entry_point_runs_the_pipeline_exactly_once() {
    let bracket = super::examples_path("bracket.ri");
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
    session.open_document(&bracket).expect("open_document");
    assert_eq!(log.take(), ONE_RELOAD, "open_document");

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

    let (mut single_file, single_file_log) = logged_session();
    single_file
        .load_from_source(bracket_source(), "bracket")
        .expect("prime");
    single_file_log.take();
    let single_file_err = single_file
        .update_source("bracket.ri", UNPARSEABLE_SRC)
        .expect_err("update_source of unparseable text, single-file branch");

    let bracket = super::examples_path("bracket.ri");
    let (mut with_imports, with_imports_log) = logged_session();
    with_imports.load_file(&bracket).expect("prime");
    with_imports_log.take();
    let with_imports_err = with_imports
        .update_source(bracket.to_str().unwrap(), UNPARSEABLE_SRC)
        .expect_err("update_source of unparseable text, multi-file branch");

    for (entry_point, err, session, log) in [
        (
            "load_from_source",
            from_source_err,
            &from_source,
            &from_source_log,
        ),
        ("load_file", from_file_err, &from_file, &from_file_log),
        (
            "update_source, single-file branch",
            single_file_err,
            &single_file,
            &single_file_log,
        ),
        (
            "update_source, multi-file branch",
            with_imports_err,
            &with_imports,
            &with_imports_log,
        ),
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

/// A rigid-body session that has also read its mechanism descriptors,
/// rejected a live edit and recorded a reload error, so each reset-list entry
/// the tests observe has something to clear.
fn primed_rigid_session() -> EngineSession {
    let fixture = rigid_mass_props_fixture_path();
    let mut session = rigid_mass_props_session();
    session.load_file(&fixture).expect("load rigid fixture");
    session.get_mechanism_descriptors();
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

#[test]
fn opening_a_rejected_file_drops_the_loaded_document_and_adopts_it_uncompiled() {
    let mut session = primed_rigid_session();
    assert!(
        !session.geometry_derived_cache_for_test().is_empty(),
        "precondition: the rigid fixture must leave mass-prop values retained"
    );
    assert!(
        session.consumed_idents_cache_for_test().is_some(),
        "precondition: the mechanism-descriptor read must leave consumed idents cached"
    );
    assert!(
        session.parsed_cache_for_test().is_some()
            && session.line_offsets_cache_for_test().is_some(),
        "precondition: the loaded document's parse-derived caches must be installed"
    );
    let log = TelemetryLog::default();
    log.install_on(&mut session);
    log.take();
    let (_dir, broken) = source_file("broken.ri", UNPARSEABLE_SRC);

    let state = session
        .open_document(&broken)
        .expect("a readable file opens even when the engine rejects its source");

    assert!(
        state.meshes.is_empty(),
        "the previous document's meshes must not be served"
    );
    assert!(
        state.values.is_empty(),
        "the previous document's values must not be served"
    );
    assert_eq!(
        state.files,
        vec![FileData {
            path: "broken.ri".to_owned(),
            content: UNPARSEABLE_SRC.to_owned(),
        }]
    );
    assert!(
        !state.compile_diagnostics.is_empty(),
        "the rejection's diagnostics must reach the diagnostics panel"
    );
    assert!(
        state
            .compile_diagnostics
            .iter()
            .all(|d| d.severity == "Error" && d.file_path == "broken.ri"),
        "every diagnostic is the opened file's error, got {:?}",
        state.compile_diagnostics
    );

    let failure = session
        .compile_failure_for_test()
        .expect("the rejection is recorded");
    assert_eq!(failure.kind, CompileFailureKind::ColdStart);
    assert_eq!(failure.source, UNPARSEABLE_SRC);
    assert!(
        session.geometry_derived_cache_for_test().is_empty(),
        "the previous document's geometry-derived values must be dropped"
    );
    assert!(
        !session.is_stale(),
        "the previous document's reload error must be dropped"
    );
    assert!(
        session.consumed_idents_cache_for_test().is_none(),
        "the previous document's consumed idents must be dropped"
    );
    assert!(
        session.parsed_cache_for_test().is_none()
            && session.line_offsets_cache_for_test().is_none(),
        "the previous document's parse-derived caches must be dropped"
    );
    assert_eq!(
        log.take(),
        Vec::<&str>::new(),
        "a rejected compile must neither solve nor emit"
    );
}

#[test]
fn a_repaired_buffer_compiles_as_the_opened_file() {
    let mut session = mock_session();
    session
        .load_file(&super::examples_path("bracket.ri"))
        .expect("load bracket");

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("helper.ri"),
        "pub structure Helper { param x: Length = 10mm }\n",
    )
    .unwrap();
    let main = dir.path().join("main.ri");
    std::fs::write(&main, format!("import helper\n{UNPARSEABLE_SRC}\n")).unwrap();
    let main_str = main.to_str().unwrap();

    let opened = session
        .open_document(&main)
        .expect("open the rejected main.ri");
    assert!(
        !opened.compile_diagnostics.is_empty()
            && opened
                .compile_diagnostics
                .iter()
                .all(|d| d.severity == "Error"),
        "the open carries the rejection's errors, got {:?}",
        opened.compile_diagnostics
    );

    session
        .update_source(main_str, "import helper\nstructure {\n")
        .expect_err("a still-broken edit is rejected");
    let still_broken = session
        .build_gui_state()
        .expect("build after a rejected edit");
    assert!(
        still_broken.meshes.is_empty(),
        "a second rejected edit must not serve the bracket's meshes"
    );
    assert!(
        !still_broken.compile_diagnostics.is_empty()
            && still_broken
                .compile_diagnostics
                .iter()
                .all(|d| d.file_path == "main.ri"),
        "the rejected edit is a cold start of main.ri, got {:?}",
        still_broken.compile_diagnostics
    );

    let repaired = session
        .update_source(
            main_str,
            "import helper\nstructure Top { sub h = Helper() }\n",
        )
        .expect("the repaired buffer compiles");
    let x = repaired
        .values
        .iter()
        .find(|v| v.name == "x" && v.entity_path == "Helper")
        .expect("the import resolves from main.ri's directory");
    assert_eq!((x.value.as_str(), x.unit.as_str()), ("10", "mm"));
    assert!(
        repaired
            .values
            .iter()
            .all(|v| !v.entity_path.starts_with("Bracket")),
        "the repaired buffer compiles as main.ri, not as the bracket"
    );
}

#[test]
fn opening_an_unreadable_file_leaves_the_loaded_document() {
    let mut session = mock_session();
    session
        .load_file(&super::examples_path("bracket.ri"))
        .expect("load bracket");
    let bracket_values = session.build_gui_state().expect("bracket state").values;
    assert!(
        !bracket_values.is_empty(),
        "precondition: the bracket has values"
    );
    let dir = tempfile::tempdir().unwrap();

    let err = session
        .open_document(&dir.path().join("missing.ri"))
        .expect_err("an unreadable file does not open");

    assert!(err.starts_with("Error reading"), "got {err:?}");
    assert!(session.compile_failure_for_test().is_none());
    assert_eq!(
        session.build_gui_state().expect("bracket state").values,
        bracket_values,
        "the loaded document is untouched"
    );
}
