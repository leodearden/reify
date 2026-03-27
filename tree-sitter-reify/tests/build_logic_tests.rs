//! Tests for build.rs pure logic functions.
//!
//! Since build.rs is compiled as a standalone build script by cargo,
//! its functions cannot be imported by test targets. This file
//! re-implements the pure logic (content hashing, staleness detection,
//! output verification) to validate correctness.

use std::hash::{Hash, Hasher};
use std::path::Path;

/// Duplicates the content_hash logic from build.rs for testability.
/// Returns hex-encoded u64 hash of file contents.
fn content_hash(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|e| {
        panic!("Failed to read {} for hashing: {}", path.display(), e)
    });
    let mut hasher = std::hash::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[test]
fn test_content_hash_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.js");
    std::fs::write(&file, b"module.exports = grammar({});").unwrap();

    let hash1 = content_hash(&file);
    let hash2 = content_hash(&file);
    assert_eq!(hash1, hash2, "hashing identical content must produce same hash");
}

#[test]
fn test_content_hash_changes_on_modification() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("grammar.js");
    std::fs::write(&file, b"module.exports = grammar({name: 'v1'});").unwrap();
    let hash1 = content_hash(&file);

    std::fs::write(&file, b"module.exports = grammar({name: 'v2'});").unwrap();
    let hash2 = content_hash(&file);

    assert_ne!(hash1, hash2, "different content must produce different hashes");
}
