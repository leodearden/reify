//! Throwaway on-disk crate trees for the gate's own tests.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

pub struct Fixture {
    dir: TempDir,
}

impl Fixture {
    pub fn new() -> Self {
        Self {
            dir: TempDir::new().expect("create fixture tempdir"),
        }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn write(&self, rel: &str, contents: &str) {
        let path = self.root().join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent dirs");
        }
        fs::write(&path, contents).expect("write fixture file");
    }
}

/// A root Cargo.toml with an explicit members list and the given
/// `[workspace.dependencies]` body.
pub fn root_manifest(members: &[&str], workspace_deps: &str) -> String {
    let list: Vec<String> = members.iter().map(|m| format!("\"{m}\"")).collect();
    format!(
        "[workspace]\nmembers = [{}]\n\n[workspace.dependencies]\n{workspace_deps}\n",
        list.join(", ")
    )
}

/// A member Cargo.toml: `[package]` named `name`, followed by `rest`.
pub fn package_manifest(name: &str, rest: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n{rest}\n")
}
