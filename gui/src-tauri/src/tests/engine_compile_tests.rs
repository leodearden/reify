//! The session's two compile front ends. A file-anchored buffer whose entry
//! follows an import compiles as a program through `compile_program` (PRD
//! docs/prds/v0_6/resolution-unification.md ε); any other buffer compiles
//! single-file, and a buffer with no file anchor warns about its imports (D-10).

use std::path::Path;

use reify_constraints::SimpleConstraintChecker;
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
/// the import, and reject the load.
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
        err.contains("in imported module 'helper'"),
        "the rejection must attribute the import unit's error to helper, got: {err}"
    );
    assert!(
        error_diag_count(&session) >= 1,
        "the rejection must carry the re-homed Error diagnostics, got {:?}",
        session.compile_failure_for_test()
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
