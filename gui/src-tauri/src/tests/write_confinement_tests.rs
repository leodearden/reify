//! Tests for `crate::write_confinement::confine_to_design_dir`: AI write
//! targets must resolve inside the open design's directory, and stay writable
//! only while that design is still open.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::write_confinement::{WriteTargetRefusal, confine_to_design_dir};

/// A design directory holding `part.ri`, and a second, unrelated directory.
struct Fixture {
    dir: TempDir,
    other: TempDir,
    design_file: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("design tempdir");
    let other = tempfile::tempdir().expect("second tempdir");
    let part = dir.path().join("part.ri");
    fs::write(&part, "structure def Part {}\n").expect("write part.ri");
    let design_file = fs::canonicalize(&part).expect("canonical part.ri");
    Fixture {
        dir,
        other,
        design_file,
    }
}

impl Fixture {
    fn design_dir(&self) -> PathBuf {
        fs::canonicalize(self.dir.path()).expect("canonical design dir")
    }

    fn in_dir(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn in_other(&self, rel: &str) -> PathBuf {
        self.other.path().join(rel)
    }

    fn confine(&self, requested: &Path) -> Result<PathBuf, WriteTargetRefusal> {
        let target = confine_to_design_dir(Some(&self.design_file), path_str(requested))?;
        target
            .path_while_open(Some(&self.design_file))
            .map(PathBuf::from)
    }

    fn assert_accepted(&self, requested: &Path) -> PathBuf {
        let resolved = self
            .confine(requested)
            .unwrap_or_else(|r| panic!("{} should be accepted: {r}", requested.display()));
        assert!(
            resolved.starts_with(self.design_dir()),
            "{} resolved outside the design dir: {}",
            requested.display(),
            resolved.display()
        );
        resolved
    }
}

fn path_str(p: &Path) -> &str {
    p.to_str().expect("tempdir paths are UTF-8")
}

// --- Accepted targets ---

#[test]
fn a_new_file_in_the_design_dir_is_accepted() {
    let f = fixture();
    let resolved = f.assert_accepted(&f.in_dir("copy.ri"));
    assert_eq!(resolved, f.design_dir().join("copy.ri"));
}

#[test]
fn an_existing_file_in_the_design_dir_is_accepted() {
    let f = fixture();
    fs::write(f.in_dir("existing.ri"), "").expect("write existing.ri");
    f.assert_accepted(&f.in_dir("existing.ri"));
}

#[test]
fn a_new_file_in_an_existing_subdir_is_accepted() {
    let f = fixture();
    fs::create_dir(f.in_dir("out")).expect("mkdir out");
    let resolved = f.assert_accepted(&f.in_dir("out/part.step"));
    assert_eq!(resolved, f.design_dir().join("out").join("part.step"));
}

#[test]
fn the_design_file_itself_is_accepted() {
    let f = fixture();
    let resolved = f.assert_accepted(&f.design_file);
    assert_eq!(resolved, f.design_file);
}

#[test]
fn a_design_dir_reached_through_a_symlink_is_accepted_on_both_sides() {
    let f = fixture();
    let alias = f.in_other("alias");
    symlink(f.dir.path(), &alias).expect("symlink alias -> design dir");

    let via_alias = f.assert_accepted(&alias.join("copy.ri"));
    assert_eq!(via_alias, f.design_dir().join("copy.ri"));

    let design_via_alias = alias.join("part.ri");
    let target = confine_to_design_dir(Some(&design_via_alias), path_str(&f.in_dir("copy.ri")))
        .expect("a design file named through a symlink still roots its real directory");
    let resolved = target
        .path_while_open(Some(&design_via_alias))
        .expect("the design it was checked against is still open");
    assert_eq!(Path::new(resolved), f.design_dir().join("copy.ri"));
}

// --- Refused targets ---

#[test]
fn no_open_design_refuses_every_target() {
    let f = fixture();
    let refusal = confine_to_design_dir(None, path_str(&f.in_dir("copy.ri")));
    assert!(
        matches!(refusal, Err(WriteTargetRefusal::NoDesignOpen { .. })),
        "{refusal:?}"
    );
}

#[test]
fn a_relative_target_is_refused_not_rebased() {
    let f = fixture();
    let refusal = confine_to_design_dir(Some(&f.design_file), "copy.ri");
    assert!(
        matches!(refusal, Err(WriteTargetRefusal::NotAbsolute { .. })),
        "{refusal:?}"
    );
}

#[test]
fn a_target_in_another_directory_is_refused() {
    let f = fixture();
    let refusal = f.confine(&f.in_other("x.ri"));
    assert!(
        matches!(refusal, Err(WriteTargetRefusal::OutsideDesignDir { .. })),
        "{refusal:?}"
    );
}

#[test]
fn a_dot_dot_escape_is_refused() {
    let f = fixture();
    let refusal = f.confine(&f.in_dir("../escape.ri"));
    assert!(
        matches!(refusal, Err(WriteTargetRefusal::OutsideDesignDir { .. })),
        "{refusal:?}"
    );
}

#[test]
fn a_symlinked_subdir_pointing_elsewhere_is_refused() {
    let f = fixture();
    symlink(f.other.path(), f.in_dir("link")).expect("symlink link -> other");
    let refusal = f.confine(&f.in_dir("link/x.ri"));
    assert!(
        matches!(refusal, Err(WriteTargetRefusal::OutsideDesignDir { .. })),
        "{refusal:?}"
    );
}

#[test]
fn an_existing_symlink_to_a_file_elsewhere_is_refused() {
    let f = fixture();
    fs::write(f.in_other("target.ri"), "").expect("write target.ri");
    symlink(f.in_other("target.ri"), f.in_dir("f.ri")).expect("symlink f.ri -> other");
    let refusal = f.confine(&f.in_dir("f.ri"));
    assert!(
        matches!(refusal, Err(WriteTargetRefusal::OutsideDesignDir { .. })),
        "{refusal:?}"
    );
}

#[test]
fn a_dangling_symlink_is_refused() {
    let f = fixture();
    let missing = f.in_other("not-there.ri");
    symlink(&missing, f.in_dir("dangling.ri")).expect("symlink dangling.ri -> missing");
    let refusal = f.confine(&f.in_dir("dangling.ri"));
    assert!(
        matches!(refusal, Err(WriteTargetRefusal::Unresolvable { .. })),
        "{refusal:?}"
    );
    assert!(!missing.exists(), "the link target must not be created");
}

#[test]
fn a_missing_parent_directory_is_refused() {
    let f = fixture();
    let refusal = f.confine(&f.in_dir("missing/x.ri"));
    assert!(
        matches!(refusal, Err(WriteTargetRefusal::Unresolvable { .. })),
        "{refusal:?}"
    );
}

#[test]
fn a_new_path_with_no_file_name_is_refused() {
    let f = fixture();
    let refusal = f.confine(&f.in_dir("missing/.."));
    assert!(
        matches!(
            &refusal,
            Err(WriteTargetRefusal::Unresolvable { reason, .. }) if reason.contains("names no file")
        ),
        "{refusal:?}"
    );
}

#[test]
fn an_outside_refusal_names_the_target_and_the_design_dir() {
    let f = fixture();
    let requested = f.in_other("x.ri");
    let message = f
        .confine(&requested)
        .expect_err("outside target is refused")
        .to_string();
    assert!(
        message.contains(path_str(&requested)),
        "message should name the requested path: {message}"
    );
    assert!(
        message.contains(path_str(&f.design_dir())),
        "message should name the design directory: {message}"
    );
}

// --- A confined target outlives a design switch only as a refusal ---

#[test]
fn a_confined_target_is_refused_once_another_design_is_open() {
    let f = fixture();
    let target = confine_to_design_dir(Some(&f.design_file), path_str(&f.in_dir("copy.ri")))
        .expect("copy.ri is inside the design dir");
    let elsewhere = f.in_other("elsewhere.ri");

    let refusal = target.path_while_open(Some(&elsewhere));

    assert!(
        matches!(refusal, Err(WriteTargetRefusal::DesignChanged { .. })),
        "{refusal:?}"
    );
    let message = refusal.expect_err("refused").to_string();
    assert!(
        message.contains(path_str(&f.design_file)) && message.contains(path_str(&elsewhere)),
        "message should name the design checked against and the one open now: {message}"
    );
}

#[test]
fn a_confined_target_is_refused_once_no_design_is_open() {
    let f = fixture();
    let target = confine_to_design_dir(Some(&f.design_file), path_str(&f.in_dir("copy.ri")))
        .expect("copy.ri is inside the design dir");

    let refusal = target.path_while_open(None);

    assert!(
        matches!(refusal, Err(WriteTargetRefusal::DesignChanged { .. })),
        "{refusal:?}"
    );
}
