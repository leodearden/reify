// Tests for the startup-load helpers: resolve_initial_file_path canonicalises
// the argv path, and begin_initial_file_load submits its load to the
// evaluation queue.
//
// CWD-mutating tests are serialised via the shared process-global Mutex at
// `crate::tests::test_helpers::cwd_lock`.  These tests live in a separate
// file so the module boundary keeps main-helper concerns out of the general
// command tests.

use crate::tests::test_helpers::cwd_lock;

/// (a) Given a CWD-relative argv path that exists on disk, returns
/// `Some(canonical_absolute_pathbuf)`.
#[test]
fn resolve_initial_file_path_relative_existing_returns_canonical() {
    use crate::commands::resolve_initial_file_path;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("mydesign.ri");
    std::fs::write(&file, "structure Foo {}").unwrap();
    let expected = std::fs::canonicalize(&file)
        .unwrap();

    let _guard = cwd_lock().lock().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let result = resolve_initial_file_path("mydesign.ri");

    std::env::set_current_dir(&original).unwrap();

    assert_eq!(
        result,
        Some(expected),
        "CWD-relative .ri path that exists should return Some(canonical)"
    );
}

/// (b) Given an already-absolute path, returns the same canonical path
/// (idempotent).
#[test]
fn resolve_initial_file_path_absolute_is_idempotent() {
    use crate::commands::resolve_initial_file_path;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("design.ri");
    std::fs::write(&file, "structure Bar {}").unwrap();
    let abs_str = file.to_str().unwrap().to_string();
    let expected = std::fs::canonicalize(&file).unwrap();

    let result = resolve_initial_file_path(&abs_str);
    assert_eq!(
        result,
        Some(expected),
        "Absolute .ri path should return Some(canonical) idempotently"
    );
}

/// (c) Returns `None` when the path is empty.
#[test]
fn resolve_initial_file_path_empty_returns_none() {
    use crate::commands::resolve_initial_file_path;

    assert_eq!(
        resolve_initial_file_path(""),
        None,
        "Empty path should return None"
    );
}

/// (c) Returns `None` when the extension is not `.ri`.
#[test]
fn resolve_initial_file_path_non_ri_extension_returns_none() {
    use crate::commands::resolve_initial_file_path;

    // .step, .stl, no-extension — all should return None
    assert_eq!(
        resolve_initial_file_path("model.step"),
        None,
        ".step extension should return None"
    );
    assert_eq!(
        resolve_initial_file_path("model.stl"),
        None,
        ".stl extension should return None"
    );
    assert_eq!(
        resolve_initial_file_path("/absolute/model.step"),
        None,
        "absolute .step path should return None"
    );
}

/// (d) Returns `Some(path)` even when the file does not exist on disk:
/// `canonicalize_document_key` falls back to the original string, so the
/// caller can attempt `load_file` and receive the actionable IO error.
#[test]
fn resolve_initial_file_path_nonexistent_ri_returns_some_fallback() {
    use crate::commands::resolve_initial_file_path;

    let nonexistent = "/tmp/__reify_test_nonexistent_xyzzy_3892/missing.ri";
    let result = resolve_initial_file_path(nonexistent);
    assert!(
        result.is_some(),
        "Nonexistent .ri path should still return Some (fallback to input)"
    );
    assert_eq!(
        result.unwrap(),
        std::path::PathBuf::from(nonexistent),
        "Fallback should be the original path string"
    );
}

// ── begin_initial_file_load: the argv load goes through the queue (task 7442) ─

fn fresh_engine() -> std::sync::Arc<std::sync::Mutex<crate::engine::EngineSession>> {
    std::sync::Arc::new(std::sync::Mutex::new(crate::engine::EngineSession::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(Box::new(reify_test_support::MockGeometryKernel::new())),
    )))
}

#[test]
fn begin_initial_file_load_submits_nothing_for_an_argv_that_is_not_a_ri_file() {
    use crate::commands::begin_initial_file_load;
    use crate::tests::test_helpers::ManualQueue;

    let rig = ManualQueue::new();

    for argv in ["", "model.step"] {
        assert!(
            begin_initial_file_load(&rig.queue, fresh_engine(), argv).is_none(),
            "argv {argv:?} must not start a load"
        );
    }

    assert_eq!(rig.executor.pending(), 0, "no drainer may be posted");
    assert!(rig.observer.observations().is_empty());
}

/// The frontend's first `get_initial_state` is submitted after `setup()`, so
/// the startup load must already be ahead of it in the queue.
#[test]
fn begin_initial_file_load_is_served_before_a_later_initial_state_request() {
    use crate::commands::{begin_initial_file_load, initial_state_evaluation};
    use crate::tests::test_helpers::{ManualQueue, settled};

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bracket.ri");
    std::fs::write(&file, reify_test_support::bracket_source()).unwrap();
    let engine = fresh_engine();
    let rig = ManualQueue::new();

    let (canonical, load) = begin_initial_file_load(
        &rig.queue,
        std::sync::Arc::clone(&engine),
        file.to_str().unwrap(),
    )
    .expect("a .ri argv must start a load");
    let first_state = rig.queue.submit(initial_state_evaluation(engine));
    rig.executor.run_pending();

    assert_eq!(canonical, std::fs::canonicalize(&file).unwrap());
    assert!(settled(load).is_ok());
    let state = settled(first_state).expect("the initial state should build");
    assert!(
        state
            .files
            .iter()
            .any(|f| f.content == reify_test_support::bracket_source()),
        "the initial state must carry the loaded file, got files {:?}",
        state.files.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
}

#[test]
fn begin_initial_file_load_of_a_missing_file_fails_its_ticket_and_leaves_the_queue_idle() {
    use crate::commands::begin_initial_file_load;
    use crate::eval_queue::EvalActivity;
    use crate::tests::test_helpers::{ManualQueue, settled};

    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.ri");
    let missing = missing.to_str().unwrap();
    let rig = ManualQueue::new();

    let (_, load) = begin_initial_file_load(&rig.queue, fresh_engine(), missing)
        .expect("a missing .ri argv must still start a load, to report why it failed");
    rig.executor.run_pending();

    let error = settled(load).expect_err("loading a missing file must fail");
    assert!(
        error.contains(missing),
        "the error must name the file, got: {error}"
    );
    assert_eq!(rig.observer.activities().last(), Some(&EvalActivity::Idle));
}
