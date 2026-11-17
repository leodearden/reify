//! The session's two compile front ends. A file-anchored buffer whose entry
//! follows an import compiles as a program through `compile_program` (PRD
//! docs/prds/v0_6/resolution-unification.md ε); any other buffer compiles
//! single-file, and a buffer with no file anchor warns about its imports (D-10).

use std::path::Path;

use reify_constraints::SimpleConstraintChecker;
use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_test_support::MockGeometryKernel;

use crate::engine::EngineSession;

/// A structure whose one param names an unknown type and an unknown function:
/// it parses, but compiling it raises Errors.
const BROKEN_BODY: &str = "{\n    param q : Frobnicate = zorp()\n}\n";

fn session() -> EngineSession {
    EngineSession::new(
        reify_eval::SolverPosture::SolverFree,
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    )
}

fn write(dir: &Path, name: &str, source: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, source).expect("write fixture");
    path
}

fn error_diag_count(session: &EngineSession) -> usize {
    session.compile_failure_for_test().map_or(0, |failure| {
        failure
            .diags
            .iter()
            .filter(|diag| diag.severity == "Error")
            .count()
    })
}

/// I2: an import unit's own Errors are re-homed onto the entry, attributed to
/// the import's file, and reject the load. Their spans index helper.ri, not
/// the entry, so the wire marks no location in the entry for them.
#[test]
fn load_file_surfaces_an_import_units_errors_on_the_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "helper.ri",
        &format!("pub structure Helper {BROKEN_BODY}"),
    );
    let main = write(
        dir.path(),
        "main.ri",
        "import helper\nstructure Top { sub h = Helper() }\n",
    );
    let mut session = session();

    let err = session
        .load_file(&main)
        .expect_err("a broken import unit must reject the load");

    assert!(
        err.contains("in imported module 'helper'") && err.contains("helper.ri"),
        "the rejection must attribute the import unit's error to helper.ri, got: {err}"
    );
    assert!(
        error_diag_count(&session) >= 1,
        "the rejection must carry the re-homed Error diagnostics, got {:?}",
        session.compile_failure_for_test()
    );
    let rehomed: Vec<_> = session
        .compile_failure_for_test()
        .expect("the rejection must be recorded")
        .diags
        .iter()
        .filter(|diag| diag.message.starts_with("in imported module 'helper'"))
        .collect();
    assert!(!rehomed.is_empty());
    assert!(
        rehomed.iter().all(|diag| !diag.has_location),
        "a re-homed diagnostic has no location in the entry, got {rehomed:?}"
    );
}

/// Boundary #6: on an anchored session the dirty-buffer path resolves imports
/// from the entry's directory through the program compile, as load_file does.
#[test]
fn dirty_buffer_on_an_anchored_session_compiles_as_a_program() {
    let dir = tempfile::tempdir().expect("tempdir");
    let main = write(
        dir.path(),
        "main.ri",
        "structure Top { param w: Length = 5mm }\n",
    );
    write(
        dir.path(),
        "bad.ri",
        &format!("pub structure Bad {BROKEN_BODY}"),
    );
    let mut session = session();
    session
        .load_file(&main)
        .expect("the healthy entry must load");

    let err = session
        .update_source(
            main.to_str().expect("utf-8 tempdir path"),
            "import bad\nstructure Top { sub b = Bad() }\n",
        )
        .expect_err("the dirty buffer's broken import must reject the edit");

    assert!(
        err.contains("in imported module 'bad'"),
        "the dirty buffer's import must compile as a program unit, got: {err}"
    );
}

// ─── D-10: a buffer with no file anchor warns about its imports ─────────────

/// Two imports a file-backed compile would follow (`helper`, `util`), one it
/// would not because the stdlib is ambient (`std.units`), and one it would not
/// because its cfg is unsatisfied (`ghost`).
const BUF: &str = "import helper\nimport util\nimport std.units\n#cfg(target = \"wasm\")\nimport ghost\nstructure Solo { param w: Length = 7mm }\n";

fn d10_warnings(session: &EngineSession) -> Vec<&Diagnostic> {
    session
        .compiled_for_test()
        .expect("the buffer must have compiled")
        .diagnostics
        .iter()
        .filter(|diag| diag.code == Some(DiagnosticCode::ImportRequiresFileBackedBuffer))
        .collect()
}

/// Assert `session` carries exactly one D-10 Warning, naming exactly the
/// followed imports and anchored at the first of them, and return its message.
fn assert_one_d10_warning(session: &EngineSession) -> String {
    let warnings = d10_warnings(session);
    assert_eq!(
        warnings.len(),
        1,
        "exactly one D-10 warning per buffer, got {warnings:?}"
    );
    let warning = warnings[0];
    assert_eq!(warning.severity, Severity::Warning);
    for named in ["helper", "util", "file-backed"] {
        assert!(
            warning.message.contains(named),
            "the D-10 warning must contain {named:?}, got: {}",
            warning.message
        );
    }
    for unnamed in ["units", "ghost"] {
        assert!(
            !warning.message.contains(unnamed),
            "the D-10 warning must not name the unfollowed import {unnamed:?}, got: {}",
            warning.message
        );
    }
    assert_eq!(
        warning.labels.first().map(|label| label.span.start),
        Some(0),
        "the D-10 warning must point at the first followed import"
    );
    warning.message.clone()
}

#[test]
fn buffer_only_load_with_imports_warns_and_still_compiles() {
    let mut session = session();

    let state = session
        .load_from_source(BUF, "solo")
        .expect("a buffer-only compile with imports must still proceed");

    let w = state
        .values
        .iter()
        .find(|value| value.cell_id == "Solo.w")
        .expect("Solo.w must be evaluated");
    assert_eq!((w.value.as_str(), w.unit.as_str()), ("7", "mm"));
    let message = assert_one_d10_warning(&session);
    let wire = session.build_gui_state().expect("build_gui_state");
    assert!(
        wire.compile_diagnostics
            .iter()
            .any(|diag| diag.severity == "Warning" && diag.message == message),
        "the D-10 warning must reach the wire, got {:?}",
        wire.compile_diagnostics
    );
}

#[test]
fn fresh_session_update_source_with_imports_warns() {
    let mut session = session();

    session
        .update_source("/nonexistent/dir/solo.ri", BUF)
        .expect("an unanchored update_source with imports must still proceed");

    assert_one_d10_warning(&session);
}

#[test]
fn buffer_only_rejection_still_explains_the_unresolved_import() {
    let mut session = session();

    let err = session
        .load_from_source("import helper\nstructure Top { sub h = Helper() }\n", "top")
        .expect_err("Helper is unknown without its import");

    assert!(err.starts_with("Compile errors"), "got: {err}");
    assert!(
        !err.contains("file-backed"),
        "the Err message is built from Errors only, got: {err}"
    );
    let diags = &session
        .compile_failure_for_test()
        .expect("the rejection must be recorded")
        .diags;
    let first_warning = diags
        .iter()
        .position(|diag| diag.severity == "Warning")
        .expect("the rejection must carry the D-10 warning");
    assert!(
        first_warning > 0,
        "the compile Errors come first, got {diags:?}"
    );
    assert!(
        diags[..first_warning]
            .iter()
            .all(|diag| diag.severity == "Error"),
        "got {diags:?}"
    );
    let warning = &diags[first_warning];
    assert!(
        warning.message.contains("helper") && warning.message.contains("file-backed"),
        "the warning must name the import and the cause, got: {}",
        warning.message
    );
}

/// `load_from_source` never compiles against a file, even on a session a
/// prior `load_file` anchored, so it warns about its imports; the anchor it
/// preserves still resolves the next `update_source`'s imports.
#[test]
fn load_from_source_on_an_anchored_session_warns_and_keeps_the_anchor() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "helper.ri",
        "module helper\npub structure def Helper {\n    param x : Length = 10mm\n}\n",
    );
    let main = write(
        dir.path(),
        "main.ri",
        "module main\nstructure Top { param w: Length = 5mm }\n",
    );
    let mut session = session();
    session
        .load_file(&main)
        .expect("the healthy entry must load");

    session
        .load_from_source(BUF, "solo")
        .expect("a buffer-only compile with imports must still proceed");
    assert_one_d10_warning(&session);

    session
        .update_source(
            main.to_str().expect("utf-8 tempdir path"),
            "module main\nimport helper\nstructure Top { sub h = Helper() }\n",
        )
        .expect("the preserved anchor must resolve helper beside main.ri");
    assert!(d10_warnings(&session).is_empty());
}

/// A rejected buffer with no file anchor and no followed import carries its
/// Errors and nothing else.
#[test]
fn buffer_only_rejection_without_followed_imports_carries_only_errors() {
    let mut session = session();

    session
        .load_from_source(
            &format!("import std.units\nstructure Solo {BROKEN_BODY}"),
            "solo",
        )
        .expect_err("the broken body must reject the buffer");

    let diags = &session
        .compile_failure_for_test()
        .expect("the rejection must be recorded")
        .diags;
    assert!(!diags.is_empty());
    assert!(
        diags.iter().all(|diag| diag.severity == "Error"),
        "no explanation is attached when no import needs one, got {diags:?}"
    );
}

#[test]
fn buffer_without_followed_imports_gets_no_d10_warning() {
    let mut session = session();

    session
        .load_from_source(
            "import std.units\nstructure Solo { param w: Length = 7mm }\n",
            "solo",
        )
        .expect("a std-only buffer compiles");

    assert!(d10_warnings(&session).is_empty());
}

#[test]
fn anchored_session_never_gets_the_d10_warning() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "helper.ri",
        "module helper\npub structure def Helper {\n    param x : Length = 10mm\n}\n",
    );
    let main = write(
        dir.path(),
        "main.ri",
        "module main\nimport helper\nstructure Top { sub h = Helper() }\n",
    );
    let mut session = session();

    session
        .load_file(&main)
        .expect("the anchored entry and its healthy import load");

    assert!(d10_warnings(&session).is_empty());
}
