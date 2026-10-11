#![cfg(feature = "gui")]
//! The `reify_save_file` / `reify_export` seam cores refuse a target outside
//! the open design's directory atomically: nothing written, baseline unmoved.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tempfile::TempDir;

use crate::debug_server::{
    reify_export_on_engine_and_refresh_baseline, reify_save_file_on_engine_and_refresh_baseline,
};
use crate::engine::EngineSession;
use crate::types::GuiState;

const DESIGN_SOURCE: &str = r#"structure def Part {
    param width: Length = 80mm
    param depth: Length = 40mm

    let body = box(width, width, depth)
}"#;

type Baseline = std::sync::Mutex<Option<GuiState>>;

fn gui_state(engine: &Arc<Mutex<EngineSession>>) -> GuiState {
    crate::engine_lock::with_engine_lock(engine, |s| s.build_gui_state())
        .and_then(std::convert::identity)
        .expect("build_gui_state must succeed")
}

/// An engine launched on `<dir>/part.ri`, its baseline S0, and a second,
/// unrelated directory.
struct LaunchedDesign {
    dir: TempDir,
    other: TempDir,
    canonical: PathBuf,
    engine: Arc<Mutex<EngineSession>>,
    s0: GuiState,
    last_state: Baseline,
}

fn launched_design() -> LaunchedDesign {
    let dir = tempfile::tempdir().expect("design tempdir");
    let other = tempfile::tempdir().expect("second tempdir");
    let part = dir.path().join("part.ri");
    std::fs::write(&part, DESIGN_SOURCE).expect("write part.ri");
    let canonical = std::fs::canonicalize(&part).expect("canonical part.ri");
    let engine = crate::tests::make_test_engine();
    crate::engine_lock::with_engine_lock(&engine, |s| s.load_file(&canonical))
        .and_then(std::convert::identity)
        .expect("load_file launch must succeed");
    let s0 = gui_state(&engine);
    let last_state = std::sync::Mutex::new(Some(s0.clone()));
    LaunchedDesign {
        dir,
        other,
        canonical,
        engine,
        s0,
        last_state,
    }
}

impl LaunchedDesign {
    fn design_dir(&self) -> String {
        std::fs::canonicalize(self.dir.path())
            .expect("canonical design dir")
            .to_string_lossy()
            .into_owned()
    }

    async fn save(&self, target: &str) -> Result<GuiState, String> {
        reify_save_file_on_engine_and_refresh_baseline(
            &self.engine,
            &self.last_state,
            Some(target.to_string()),
        )
        .await
    }

    async fn export_step(&self, target: &str) -> Result<GuiState, String> {
        reify_export_on_engine_and_refresh_baseline(&self.engine, &self.last_state, "step", target)
            .await
    }

    /// A symlink outside the design dir that aliases it.
    fn alias_of_design_dir(&self) -> PathBuf {
        let alias = self.other.path().join("alias");
        std::os::unix::fs::symlink(self.dir.path(), &alias).expect("symlink alias -> design dir");
        alias
    }

    /// A refusal wrote nothing to the design and left the baseline at S0.
    fn assert_untouched(&self) {
        assert_eq!(
            std::fs::read_to_string(&self.canonical).expect("part.ri readable"),
            DESIGN_SOURCE,
            "a refused write must leave the design file byte-identical"
        );
        assert_eq!(
            *self.last_state.lock().unwrap(),
            Some(self.s0.clone()),
            "a refused write must leave the baseline where it was"
        );
    }
}

fn path_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[tokio::test]
async fn save_as_into_another_directory_is_refused_atomically() {
    let d = launched_design();
    let target = d.other.path().join("copy.ri");

    let err = d
        .save(&path_string(&target))
        .await
        .expect_err("a save-as outside the design dir must be refused");

    assert!(
        err.contains(&d.design_dir()),
        "the refusal must name the design dir: {err}"
    );
    assert!(!target.exists(), "the refused target must not be created");
    d.assert_untouched();
}

#[tokio::test]
async fn save_as_through_dot_dot_is_refused() {
    let d = launched_design();
    let target = d.dir.path().join("../escape.ri");

    d.save(&path_string(&target))
        .await
        .expect_err("a .. escape must be refused");

    assert!(!target.exists(), "the escape target must not be created");
    d.assert_untouched();
}

#[tokio::test]
async fn a_relative_save_as_is_refused_not_rebased() {
    let d = launched_design();
    let relative = "relative-save-target-7816.ri";

    d.save(relative)
        .await
        .expect_err("a relative save-as must be refused");

    assert!(
        !Path::new(relative).exists(),
        "a relative save-as must not land in the process CWD"
    );
    assert!(!d.dir.path().join(relative).exists());
    d.assert_untouched();
}

#[tokio::test]
async fn export_into_another_directory_is_refused_atomically() {
    let d = launched_design();
    let target = d.other.path().join("part.step");

    d.export_step(&path_string(&target))
        .await
        .expect_err("an export outside the design dir must be refused");

    assert!(!target.exists(), "the refused export must not be written");
    d.assert_untouched();
}

#[tokio::test]
async fn a_relative_export_is_refused() {
    let d = launched_design();
    let relative = "relative-export-target-7816.step";

    d.export_step(relative)
        .await
        .expect_err("a relative export must be refused");

    assert!(!Path::new(relative).exists());
    d.assert_untouched();
}

#[tokio::test]
async fn save_as_into_a_subdirectory_of_the_design_dir_is_permitted() {
    let d = launched_design();
    std::fs::create_dir(d.dir.path().join("out")).expect("mkdir out");
    let target = d.dir.path().join("out").join("copy.ri");

    d.save(&path_string(&target))
        .await
        .expect("a save-as inside the design dir must succeed");

    assert_eq!(
        std::fs::read_to_string(&target).expect("copy.ri readable"),
        DESIGN_SOURCE,
        "the permitted target must receive the session's source"
    );
}

#[tokio::test]
async fn save_as_through_an_alias_of_the_design_dir_lands_in_the_design_dir() {
    let d = launched_design();
    let alias = d.alias_of_design_dir();

    d.save(&path_string(&alias.join("copy.ri")))
        .await
        .expect("a save-as naming the design dir through an alias must succeed");

    assert_eq!(
        std::fs::read_to_string(d.dir.path().join("copy.ri")).expect("copy.ri readable"),
        DESIGN_SOURCE,
        "the save-as must land in the real design dir"
    );
}

#[tokio::test]
async fn export_through_an_alias_of_the_design_dir_lands_in_the_design_dir() {
    let d = launched_design();
    let alias = d.alias_of_design_dir();

    d.export_step(&path_string(&alias.join("part.step")))
        .await
        .expect("an export naming the design dir through an alias must succeed");

    let written = std::fs::metadata(d.dir.path().join("part.step"))
        .expect("the export must land in the real design dir");
    assert!(written.len() > 0, "the exported file must not be empty");
}

#[tokio::test]
async fn a_session_with_no_design_file_refuses_explicit_targets() {
    let engine = crate::tests::make_test_engine();
    let s0 = gui_state(&engine);
    let last_state: Baseline = std::sync::Mutex::new(Some(s0.clone()));
    let dir = tempfile::tempdir().expect("tempdir");
    let saved = dir.path().join("saved.ri");
    let exported = dir.path().join("saved.step");

    reify_save_file_on_engine_and_refresh_baseline(&engine, &last_state, Some(path_string(&saved)))
        .await
        .expect_err("with no design dir there is nowhere a save-as may write");
    reify_export_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        "step",
        &path_string(&exported),
    )
    .await
    .expect_err("with no design dir there is nowhere an export may write");

    assert!(!saved.exists(), "the refused save-as must not be created");
    assert!(!exported.exists(), "the refused export must not be written");
    assert_eq!(
        *last_state.lock().unwrap(),
        Some(s0),
        "refusals must leave the baseline where it was"
    );
}
