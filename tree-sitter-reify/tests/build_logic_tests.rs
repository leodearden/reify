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

/// The expected output files that tree-sitter generate produces.
const EXPECTED_OUTPUTS: &[&str] = &["parser.c", "grammar.json", "node-types.json"];

/// Duplicates needs_generate logic from build.rs for testability.
/// Returns true if regeneration is needed based on content hash staleness.
fn needs_generate(grammar_path: &Path, stamp_path: &Path, output_paths: &[&Path]) -> bool {
    // Must regenerate if any output file is missing.
    for path in output_paths {
        if !path.exists() {
            return true;
        }
    }
    // Must regenerate if stamp file is missing.
    let stamp_content = match std::fs::read_to_string(stamp_path) {
        Ok(s) => s,
        Err(_) => return true,
    };
    // Must regenerate if grammar hash differs from stamp.
    let current_hash = content_hash(grammar_path);
    stamp_content.trim() != current_hash
}

#[test]
fn test_needs_generate_true_when_no_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let grammar = dir.path().join("grammar.js");
    std::fs::write(&grammar, b"module.exports = grammar({});").unwrap();
    let stamp = dir.path().join("stamp.hash");
    // stamp does not exist
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    for name in EXPECTED_OUTPUTS {
        std::fs::write(src_dir.join(name), b"placeholder").unwrap();
    }
    let output_paths: Vec<_> = EXPECTED_OUTPUTS.iter().map(|n| src_dir.join(n)).collect();
    let output_refs: Vec<&Path> = output_paths.iter().map(|p| p.as_path()).collect();

    assert!(
        needs_generate(&grammar, &stamp, &output_refs),
        "must regenerate when stamp file is missing"
    );
}

#[test]
fn test_needs_generate_false_when_stamp_matches() {
    let dir = tempfile::tempdir().unwrap();
    let grammar = dir.path().join("grammar.js");
    std::fs::write(&grammar, b"module.exports = grammar({});").unwrap();
    let stamp = dir.path().join("stamp.hash");
    // Write matching hash to stamp file
    let hash = content_hash(&grammar);
    std::fs::write(&stamp, &hash).unwrap();
    // Create all 3 output files
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    for name in EXPECTED_OUTPUTS {
        std::fs::write(src_dir.join(name), b"placeholder").unwrap();
    }
    let output_paths: Vec<_> = EXPECTED_OUTPUTS.iter().map(|n| src_dir.join(n)).collect();
    let output_refs: Vec<&Path> = output_paths.iter().map(|p| p.as_path()).collect();

    assert!(
        !needs_generate(&grammar, &stamp, &output_refs),
        "must NOT regenerate when stamp matches and all outputs exist"
    );
}

#[test]
fn test_needs_generate_true_when_stamp_stale() {
    let dir = tempfile::tempdir().unwrap();
    let grammar = dir.path().join("grammar.js");
    std::fs::write(&grammar, b"module.exports = grammar({name: 'new'});").unwrap();
    let stamp = dir.path().join("stamp.hash");
    // Write a stale (old) hash to stamp file
    std::fs::write(&stamp, "0000000000000000").unwrap();
    // Create all 3 output files
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    for name in EXPECTED_OUTPUTS {
        std::fs::write(src_dir.join(name), b"placeholder").unwrap();
    }
    let output_paths: Vec<_> = EXPECTED_OUTPUTS.iter().map(|n| src_dir.join(n)).collect();
    let output_refs: Vec<&Path> = output_paths.iter().map(|p| p.as_path()).collect();

    assert!(
        needs_generate(&grammar, &stamp, &output_refs),
        "must regenerate when stamp hash differs from current grammar hash"
    );
}

#[test]
fn test_needs_generate_true_when_output_missing() {
    let dir = tempfile::tempdir().unwrap();
    let grammar = dir.path().join("grammar.js");
    std::fs::write(&grammar, b"module.exports = grammar({});").unwrap();
    let stamp = dir.path().join("stamp.hash");
    // Write matching hash
    let hash = content_hash(&grammar);
    std::fs::write(&stamp, &hash).unwrap();
    // Create only 2 of the 3 output files (grammar.json missing)
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("parser.c"), b"placeholder").unwrap();
    // grammar.json intentionally missing
    std::fs::write(src_dir.join("node-types.json"), b"placeholder").unwrap();

    let output_paths: Vec<_> = EXPECTED_OUTPUTS.iter().map(|n| src_dir.join(n)).collect();
    let output_refs: Vec<&Path> = output_paths.iter().map(|p| p.as_path()).collect();

    assert!(
        needs_generate(&grammar, &stamp, &output_refs),
        "must regenerate when any output file is missing"
    );
}

/// Duplicates verify_outputs logic from build.rs for testability.
/// Returns Err with a message naming the missing file(s).
fn verify_outputs(src_dir: &Path) -> Result<(), String> {
    let mut missing = Vec::new();
    for name in EXPECTED_OUTPUTS {
        if !src_dir.join(name).exists() {
            missing.push(*name);
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "tree-sitter generate succeeded but these output files are missing: {}",
            missing.join(", ")
        ))
    }
}

#[test]
fn test_all_three_outputs_verified() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    // With all 3 files present, verification succeeds.
    for name in EXPECTED_OUTPUTS {
        std::fs::write(src_dir.join(name), b"placeholder").unwrap();
    }
    assert!(verify_outputs(&src_dir).is_ok(), "all files present should verify ok");

    // Remove each file in turn and verify it's detected as missing.
    for name in EXPECTED_OUTPUTS {
        let path = src_dir.join(name);
        std::fs::remove_file(&path).unwrap();
        let err = verify_outputs(&src_dir).unwrap_err();
        assert!(
            err.contains(name),
            "error message should name the missing file '{}', got: {}",
            name,
            err
        );
        // Restore for next iteration
        std::fs::write(&path, b"placeholder").unwrap();
    }
}

#[test]
fn test_no_redundant_rerun_if_changed() {
    // Source-level regression guard: build.rs must NOT contain rerun-if-changed=src/parser.c
    let build_rs = std::fs::read_to_string("build.rs")
        .expect("should be able to read build.rs from tree-sitter-reify crate root");
    assert!(
        !build_rs.contains("rerun-if-changed=src/parser.c"),
        "build.rs must NOT contain 'rerun-if-changed=src/parser.c' — \
         src/parser.c is a generated output managed by build.rs itself. \
         Watching it causes double execution."
    );
}

/// Duplicates run_with_timeout logic from build.rs for testability.
/// Returns Ok(()) on success, Err(message) on failure or timeout.
fn run_with_timeout(cmd: &str, args: &[&str], timeout_secs: u64) -> Result<(), String> {
    use std::time::{Duration, Instant};

    let mut child = std::process::Command::new(cmd)
        .args(args)
        .spawn()
        .map_err(|e| format!("Failed to spawn '{}': {}", cmd, e))?;

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                } else {
                    return Err(format!(
                        "'{}' failed with exit code {}",
                        cmd,
                        status.code().unwrap_or(-1)
                    ));
                }
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "'{}' timed out after {}s",
                        cmd, timeout_secs
                    ));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(format!("Error waiting for '{}': {}", cmd, e));
            }
        }
    }
}

#[test]
fn test_subprocess_timeout_kills_hung_process() {
    use std::time::Instant;

    let start = Instant::now();
    // Use 'sleep 30' to simulate a hung process, with 1s timeout.
    let result = run_with_timeout("sleep", &["30"], 1);
    let elapsed = start.elapsed();

    assert!(result.is_err(), "hung process should return error on timeout");
    let err = result.unwrap_err();
    assert!(
        err.to_lowercase().contains("timeout") || err.to_lowercase().contains("timed out"),
        "error should mention timeout, got: {}",
        err
    );
    // Should complete within ~2s (1s timeout + overhead), not 30s.
    assert!(
        elapsed.as_secs() < 5,
        "should have killed hung process quickly, but took {:?}",
        elapsed
    );
}
