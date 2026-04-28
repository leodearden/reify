//! Integration tests for `Manifest::load_from_path`.

use std::fs;

use reify_config::{KernelId, Manifest, ManifestError};
use tempfile::TempDir;

#[test]
fn load_from_path_reads_valid_manifest() {
    let dir = TempDir::new().expect("create temp dir");
    let path = dir.path().join("reify.toml");
    fs::write(&path, "[kernels]\nfidget = \"0.3.4\"\n").expect("write reify.toml");

    let manifest = Manifest::load_from_path(&path).expect("manifest should load");
    let entries: Vec<(&KernelId, &reify_config::KernelPin)> = manifest.kernel_pins().collect();
    assert_eq!(entries.len(), 1);
    let (id, pin) = entries[0];
    assert_eq!(*id, KernelId::Fidget);
    assert_eq!(pin.version, "0.3.4");
}

#[test]
fn load_from_path_missing_file_returns_io_error() {
    let dir = TempDir::new().expect("create temp dir");
    let path = dir.path().join("does-not-exist.toml");

    let err = Manifest::load_from_path(&path).expect_err("missing file should fail");
    match err {
        ManifestError::Io(_) => {}
        other => panic!("expected ManifestError::Io(_), got {:?}", other),
    }
}

#[test]
fn load_from_path_propagates_parse_errors() {
    let dir = TempDir::new().expect("create temp dir");
    let path = dir.path().join("reify.toml");
    fs::write(&path, "[kernels]\nfoobar = \"1.0\"\n").expect("write reify.toml");

    let err = Manifest::load_from_path(&path).expect_err("unknown kernel should fail");
    match err {
        ManifestError::UnknownKernel(ref name) => assert_eq!(name, "foobar"),
        other => panic!("expected ManifestError::UnknownKernel, got {:?}", other),
    }
}
