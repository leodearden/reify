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
