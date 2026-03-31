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

/// Creates base/src/, writes placeholder files for all EXPECTED_OUTPUTS,
/// and returns the src_dir path. Deduplicates setup across stamp/output tests.
fn make_populated_src_dir(base: &Path) -> std::path::PathBuf {
    let src_dir = base.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    for name in EXPECTED_OUTPUTS {
        std::fs::write(src_dir.join(name), b"placeholder").unwrap();
    }
    src_dir
}

/// Duplicates stamp-write logic from build.rs for testability.
/// Writes the grammar hash to the stamp file, warning on failure instead of panicking.
fn stamp_write(stamp_path: &Path, grammar_hash: &str) {
    std::fs::write(stamp_path, grammar_hash).unwrap_or_else(|e| {
        eprintln!(
            "cargo:warning=Failed to write stamp file {}: {}",
            stamp_path.display(),
            e
        );
    });
}

/// Duplicates needs_generate logic from build.rs for testability.
/// Returns true if regeneration is needed based on content hash staleness.
/// The caller passes a pre-computed grammar hash to avoid TOCTOU races.
fn needs_generate(grammar_hash: &str, stamp_path: &Path, output_paths: &[&Path]) -> bool {
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
    stamp_content.trim() != grammar_hash
}

#[test]
fn test_needs_generate_true_when_no_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let grammar = dir.path().join("grammar.js");
    std::fs::write(&grammar, b"module.exports = grammar({});").unwrap();
    let stamp = dir.path().join("stamp.hash");
    // stamp does not exist
    let src_dir = make_populated_src_dir(dir.path());
    let output_paths: Vec<_> = EXPECTED_OUTPUTS.iter().map(|n| src_dir.join(n)).collect();
    let output_refs: Vec<&Path> = output_paths.iter().map(|p| p.as_path()).collect();

    let hash = content_hash(&grammar);
    assert!(
        needs_generate(&hash, &stamp, &output_refs),
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
    let src_dir = make_populated_src_dir(dir.path());
    let output_paths: Vec<_> = EXPECTED_OUTPUTS.iter().map(|n| src_dir.join(n)).collect();
    let output_refs: Vec<&Path> = output_paths.iter().map(|p| p.as_path()).collect();

    assert!(
        !needs_generate(&hash, &stamp, &output_refs),
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
    let src_dir = make_populated_src_dir(dir.path());
    let output_paths: Vec<_> = EXPECTED_OUTPUTS.iter().map(|n| src_dir.join(n)).collect();
    let output_refs: Vec<&Path> = output_paths.iter().map(|p| p.as_path()).collect();

    let hash = content_hash(&grammar);
    assert!(
        needs_generate(&hash, &stamp, &output_refs),
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
    // Create all 3 output files, then remove grammar.json
    let src_dir = make_populated_src_dir(dir.path());
    std::fs::remove_file(src_dir.join("grammar.json")).unwrap();

    let output_paths: Vec<_> = EXPECTED_OUTPUTS.iter().map(|n| src_dir.join(n)).collect();
    let output_refs: Vec<&Path> = output_paths.iter().map(|p| p.as_path()).collect();

    assert!(
        needs_generate(&hash, &stamp, &output_refs),
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
    let src_dir = make_populated_src_dir(dir.path());
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
    let build_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/build.rs"))
        .expect("should be able to read build.rs");
    assert!(
        !build_rs.contains("rerun-if-changed=src/parser.c"),
        "build.rs must NOT contain 'rerun-if-changed=src/parser.c' — \
         src/parser.c is a generated output managed by build.rs itself. \
         Watching it causes double execution."
    );
}

#[test]
fn test_stamp_shared_across_simulated_profiles() {
    // Integration test: validates the core invariant that two separate callers
    // (simulating different cargo profiles) writing/reading the same stamp path
    // see each other's results — no redundant generation across profiles.
    let dir = tempfile::tempdir().unwrap();

    // Set up a shared src/ directory (simulating the real crate layout).
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    for name in EXPECTED_OUTPUTS {
        std::fs::write(src_dir.join(name), b"placeholder").unwrap();
    }

    // The shared stamp path — profile-independent, in src/.
    let stamp_path = src_dir.join(".grammar_hash.stamp");

    // Create a grammar file and compute its hash.
    let grammar = dir.path().join("grammar.js");
    std::fs::write(&grammar, b"module.exports = grammar({name: 'shared'});").unwrap();
    let hash = content_hash(&grammar);

    let output_paths: Vec<_> = EXPECTED_OUTPUTS.iter().map(|n| src_dir.join(n)).collect();
    let output_refs: Vec<&Path> = output_paths.iter().map(|p| p.as_path()).collect();

    // "Profile A" sees no stamp → needs_generate is true.
    assert!(
        needs_generate(&hash, &stamp_path, &output_refs),
        "profile A should need generation (no stamp yet)"
    );

    // "Profile A" writes the stamp after generation.
    std::fs::write(&stamp_path, &hash).unwrap();

    // "Profile B" reads the same stamp → needs_generate is false.
    assert!(
        !needs_generate(&hash, &stamp_path, &output_refs),
        "profile B must NOT need generation — stamp was written by profile A \
         to the shared src/ location"
    );
}

#[test]
fn test_stamp_path_is_profile_independent() {
    // Source-level regression guard: build.rs must NOT use OUT_DIR for the stamp path
    // (OUT_DIR is per-profile, causing redundant tree-sitter generate calls when switching
    // between `cargo test`, `cargo build`, and `cargo clippy`).
    // Instead it must use `.grammar_hash.stamp` in src/ — matching the shell script convention.
    let build_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/build.rs"))
        .expect("should be able to read build.rs");

    // Must NOT use OUT_DIR for the stamp path.
    assert!(
        !build_rs.contains(r#"env::var("OUT_DIR")"#),
        "build.rs must NOT read OUT_DIR — the stamp file must be stored in src/ \
         so it is shared across all cargo profiles (debug, release, clippy, test)."
    );

    // Must reference the correct stamp filename.
    assert!(
        build_rs.contains(".grammar_hash.stamp"),
        "build.rs must reference '.grammar_hash.stamp' as the stamp filename, \
         consistent with scripts/tree-sitter-generate.sh."
    );
}

/// Find the Err(e) arm in source code using brace-depth tracking.
/// Returns the slice from `Err(e) =>` through the arm's closing `}`.
/// Tracks brace depth, skipping braces inside double-quoted string literals
/// so that format strings like `format!("hint: '}}'")` don't fool the counter.
fn find_err_arm_braced(source: &str) -> Option<&str> {
    let err_start = source.find("Err(e) =>")?;
    let after_arrow = &source[err_start..];

    // Find the opening brace of the arm body.
    let brace_offset = after_arrow.find('{')?;
    let body_start = err_start + brace_offset;

    let bytes = source.as_bytes();
    let mut depth: usize = 0;
    let mut i = body_start;

    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                // Skip string literal contents — braces inside strings don't count.
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2; // skip escaped character (e.g. \")
                        continue;
                    }
                    if bytes[i] == b'"' {
                        break; // closing quote
                    }
                    i += 1;
                }
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[err_start..=i]);
                }
            }
            _ => {}
        }
        i += 1;
    }

    None // unbalanced braces
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
                let _ = child.kill();
                let _ = child.wait(); // Reap the process to prevent orphans.
                return Err(format!("Error waiting for '{}': {}", cmd, e));
            }
        }
    }
}

#[test]
fn test_try_wait_error_path_kills_child() {
    // Source-level regression guard: the Err(e) arm of try_wait() in run_with_timeout
    // must contain child.kill() and child.wait() to prevent orphan processes on I/O errors.
    let build_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/build.rs"))
        .expect("should be able to read build.rs");

    // Extract the Err(e) arm using brace-depth tracking with string-literal awareness.
    // This precisely captures the arm body without the fragility of a fixed-size window.
    let err_arm = find_err_arm_braced(&build_rs)
        .expect("build.rs should contain an Err(e) arm in try_wait match");

    assert!(
        err_arm.contains("child.kill()"),
        "Err(e) arm of try_wait() must contain child.kill() to prevent orphan processes. \
         Window: {}",
        err_arm
    );
    assert!(
        err_arm.contains("child.wait()"),
        "Err(e) arm of try_wait() must contain child.wait() to reap the child process. \
         Window: {}",
        err_arm
    );
}

#[test]
fn test_err_arm_extraction_not_fooled_by_format_braces() {
    // Synthetic source where child.kill()/child.wait() appear AFTER a format string with '}'.
    // This demonstrates the fragility of the naive .find('}') approach and validates
    // that find_err_arm_braced handles it correctly.
    let source = r#"
        match child.try_wait() {
            Ok(Some(status)) => { return Ok(()); }
            Ok(None) => { /* polling */ }
            Err(e) => {
                return Err(format!("Error: '{}'", e));
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    "#;

    // The naive .find('}') approach finds the '}' inside the format string,
    // not the arm's closing brace.
    let err_start = source.find("Err(e) =>").unwrap();
    let err_section = &source[err_start..];
    let naive_end = err_section.find('}').unwrap();
    let naive_slice = &err_section[..=naive_end];

    // The naive approach misses child.kill() and child.wait() because they
    // appear after the format string's '}'.
    assert!(
        !naive_slice.contains("child.kill()"),
        "naive .find('}}') should NOT capture child.kill() — it stops at format string brace"
    );

    // The brace-depth tracker with string-literal awareness captures the full arm.
    let braced = find_err_arm_braced(source).expect("should find Err(e) arm");
    assert!(
        braced.contains("child.kill()"),
        "find_err_arm_braced should capture child.kill(). Got: {}",
        braced
    );
    assert!(
        braced.contains("child.wait()"),
        "find_err_arm_braced should capture child.wait(). Got: {}",
        braced
    );
}

#[test]
fn test_find_err_arm_braced_simple() {
    // Simple match block with Err(e) arm — no format strings or nested braces.
    let source = r#"
        match child.try_wait() {
            Ok(Some(status)) => { return Ok(()); }
            Ok(None) => { /* polling */ }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("error: {}", e));
            }
        }
    "#;

    let arm = find_err_arm_braced(source)
        .expect("should find Err(e) arm in simple match block");

    assert!(
        arm.contains("child.kill()"),
        "extracted arm should contain child.kill(). Got: {}",
        arm
    );
    assert!(
        arm.contains("child.wait()"),
        "extracted arm should contain child.wait(). Got: {}",
        arm
    );
}

#[test]
fn test_find_err_arm_braced_skips_format_braces() {
    // Err(e) arm contains format!() with '}}' (Rust escaped brace) BEFORE child.kill()/child.wait().
    // The '}}' in source text is two literal '}' characters — the naive brace counter
    // decrements depth to 0 at the first '}', stopping before child.kill()/child.wait().
    // A string-literal-aware tracker must skip braces inside "..." to extract the full arm.
    let source = r#"
        match child.try_wait() {
            Ok(Some(status)) => { return Ok(()); }
            Ok(None) => { /* polling */ }
            Err(e) => {
                let msg = format!("err={}, hint: '}}' escapes", e);
                let _ = child.kill();
                let _ = child.wait();
                return Err(msg);
            }
        }
    "#;

    let arm = find_err_arm_braced(source)
        .expect("should find Err(e) arm with format strings");

    assert!(
        arm.contains("child.kill()"),
        "brace-depth tracker must not stop at format string braces. Got: {}",
        arm
    );
    assert!(
        arm.contains("child.wait()"),
        "brace-depth tracker must capture full arm past format strings. Got: {}",
        arm
    );
}

#[test]
fn test_out_dir_not_used_for_stamp() {
    // Source-level regression guard: build.rs must NOT use OUT_DIR for the stamp file.
    // OUT_DIR is per-cargo-profile, so using it causes redundant tree-sitter generate
    // calls when switching between `cargo test`, `cargo build`, and `cargo clippy`.
    // The stamp must live in src/ where it's shared across all profiles.
    let build_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/build.rs"))
        .expect("should be able to read build.rs");

    assert!(
        !build_rs.contains("env::var(\"OUT_DIR\")"),
        "build.rs must NOT read OUT_DIR — the stamp file must be stored in src/ \
         so it is shared across all cargo profiles (debug, release, clippy, test)."
    );
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

#[test]
#[cfg(unix)] // set_readonly(true) on a directory only prevents file creation on Unix (POSIX);
             // on Windows the readonly attribute does NOT block creating files within the directory.
fn test_stamp_write_failure_no_panic() {
    // Verify that stamp_write does not panic when the destination is read-only.
    // This mirrors build.rs behavior where write failure emits a warning instead of panicking.
    let dir = tempfile::tempdir().unwrap();
    let readonly_dir = dir.path().join("readonly");
    std::fs::create_dir_all(&readonly_dir).unwrap();

    // Make the directory read-only so file creation fails
    let mut perms = std::fs::metadata(&readonly_dir).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&readonly_dir, perms).unwrap();

    // Guard ensures cleanup even if assertions below panic
    let _guard = ReadonlyGuard::new(readonly_dir.clone());

    let stamp_path = readonly_dir.join("grammar_hash.stamp");

    // Should not panic — just warn
    stamp_write(&stamp_path, "somehash");

    // Verify the stamp was NOT written (write should have failed)
    assert!(
        !stamp_path.exists(),
        "stamp should not exist in a read-only directory"
    );
}

/// RAII guard that unconditionally restores write permissions on drop.
/// Prevents temp-directory leaks when assertions panic between
/// set_readonly(true) and the manual permission restore.
struct ReadonlyGuard {
    path: std::path::PathBuf,
}

impl ReadonlyGuard {
    fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for ReadonlyGuard {
    fn drop(&mut self) {
        if let Ok(meta) = std::fs::metadata(&self.path) {
            let mut perms = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(&self.path, perms);
        }
    }
}

#[test]
#[cfg(unix)] // set_readonly(true) on a directory only prevents file creation on Unix (POSIX);
             // on Windows the readonly attribute does NOT block creating files within the directory.
fn test_readonly_guard_restores_on_drop() {
    // Verify that ReadonlyGuard's Drop impl restores write permissions.
    let dir = tempfile::tempdir().unwrap();
    let subdir = dir.path().join("guarded");
    std::fs::create_dir_all(&subdir).unwrap();

    // Make the directory read-only
    let mut perms = std::fs::metadata(&subdir).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&subdir, perms).unwrap();

    // Guard takes ownership of the path and restores permissions on drop
    {
        let _guard = ReadonlyGuard::new(subdir.clone());
        // While guard is alive, directory is still read-only
        assert!(
            std::fs::File::create(subdir.join("probe_while_guarded.txt")).is_err(),
            "directory should still be read-only while guard is alive"
        );
    }
    // After guard is dropped, directory should be writable again
    std::fs::File::create(subdir.join("probe_after_drop.txt"))
        .expect("directory should be writable after ReadonlyGuard is dropped");
}
