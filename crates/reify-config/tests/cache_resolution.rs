//! Integration tests for the cache config file loader.
//!
//! Mirrors `tests/load_from_path.rs` for the manifest loader: tempdir-based
//! coverage of the happy path, missing-file (`Io`), and malformed-TOML
//! (`Parse`) branches.

use std::fs;
use std::path::PathBuf;

use reify_config::cache::{load_cache_config_from_path, CacheConfig, CacheError};
use tempfile::TempDir;

#[test]
fn load_cache_config_from_path_reads_valid_document() {
    let dir = TempDir::new().expect("create temp dir");
    let path = dir.path().join("config.toml");
    fs::write(&path, "[cache]\ndir = \"/foo\"\nmax_bytes = 99\n")
        .expect("write cache config");

    let cfg = load_cache_config_from_path(&path).expect("config should load");
    assert_eq!(
        cfg,
        CacheConfig {
            dir: Some(PathBuf::from("/foo")),
            max_bytes: Some(99),
        }
    );
}

#[test]
fn load_cache_config_from_path_missing_file_returns_io_error() {
    let dir = TempDir::new().expect("create temp dir");
    let path = dir.path().join("does-not-exist.toml");

    let err = load_cache_config_from_path(&path).expect_err("missing file should fail");
    match err {
        CacheError::Io(_) => {}
        other => panic!("expected CacheError::Io(_), got {:?}", other),
    }
}

#[test]
fn load_cache_config_from_path_propagates_parse_errors_with_diagnostic() {
    let dir = TempDir::new().expect("create temp dir");
    let path = dir.path().join("config.toml");
    // Unclosed [cache — never reaches the lift; toml::from_str surfaces a
    // syntax error with line/col context preserved.
    fs::write(&path, "[cache\ndir = \"/foo\"\n").expect("write malformed config");

    let err = load_cache_config_from_path(&path)
        .expect_err("malformed TOML should be rejected");
    match err {
        CacheError::Parse(msg) => {
            assert!(
                !msg.is_empty(),
                "Parse error message must carry diagnostic text"
            );
        }
        other => panic!("expected CacheError::Parse(_), got {:?}", other),
    }
}
