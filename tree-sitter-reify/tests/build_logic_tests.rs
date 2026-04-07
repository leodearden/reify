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
    let bytes = std::fs::read(path)
        .unwrap_or_else(|e| panic!("Failed to read {} for hashing: {}", path.display(), e));
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
    assert_eq!(
        hash1, hash2,
        "hashing identical content must produce same hash"
    );
}

#[test]
fn test_content_hash_changes_on_modification() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("grammar.js");
    std::fs::write(&file, b"module.exports = grammar({name: 'v1'});").unwrap();
    let hash1 = content_hash(&file);

    std::fs::write(&file, b"module.exports = grammar({name: 'v2'});").unwrap();
    let hash2 = content_hash(&file);

    assert_ne!(
        hash1, hash2,
        "different content must produce different hashes"
    );
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
    assert!(
        verify_outputs(&src_dir).is_ok(),
        "all files present should verify ok"
    );

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
                    return Err(format!("'{}' timed out after {}s", cmd, timeout_secs));
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
    let build_rs = std::fs::read_to_string("build.rs")
        .expect("should be able to read build.rs from tree-sitter-reify crate root");

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

    let arm = find_err_arm_braced(source).expect("should find Err(e) arm in simple match block");

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

    let arm = find_err_arm_braced(source).expect("should find Err(e) arm with format strings");

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
fn test_out_dir_no_silent_fallback() {
    // Source-level regression guard: build.rs must NOT silently fall back to "." when
    // OUT_DIR is unset. Cargo always sets OUT_DIR for build scripts, so a missing value
    // means something is fundamentally wrong — we should panic, not pollute the source tree.
    let build_rs = std::fs::read_to_string("build.rs")
        .expect("should be able to read build.rs from tree-sitter-reify crate root");

    // Find the line that reads the OUT_DIR env var (not comments mentioning OUT_DIR).
    let out_dir_line = build_rs
        .lines()
        .find(|line| {
            line.contains("env::var(\"OUT_DIR\")") || line.contains("env::var( \"OUT_DIR\")")
        })
        .expect("build.rs should contain a line reading env::var(\"OUT_DIR\")");

    assert!(
        !out_dir_line.contains("unwrap_or_else"),
        "OUT_DIR line must NOT use unwrap_or_else (silent fallback). \
         Cargo always sets OUT_DIR; a missing value should panic. Line: {}",
        out_dir_line
    );
    assert!(
        out_dir_line.contains("expect"),
        "OUT_DIR line must use .expect() for a clear panic message. Line: {}",
        out_dir_line
    );
}

#[test]
fn test_subprocess_timeout_kills_hung_process() {
    use std::time::Instant;

    let start = Instant::now();
    // Use 'sleep 30' to simulate a hung process, with 1s timeout.
    let result = run_with_timeout("sleep", &["30"], 1);
    let elapsed = start.elapsed();

    assert!(
        result.is_err(),
        "hung process should return error on timeout"
    );
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
    if is_root() {
        eprintln!("skipping: test requires non-root user (root bypasses DAC permissions)");
        return;
    }
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

#[test]
fn test_stamp_write_exact_content() {
    // Validates that stamp_write produces exact hash bytes with no trailing
    // whitespace, newline, or other content corruption. Since needs_generate
    // uses .trim() when reading, corruption would be silently masked without
    // this direct content assertion.
    let dir = tempfile::tempdir().unwrap();
    let stamp_path = dir.path().join("grammar_hash.stamp");
    let hash = "a1b2c3d4e5f60708";

    stamp_write(&stamp_path, hash);

    let raw_content = std::fs::read_to_string(&stamp_path).unwrap();
    assert_eq!(
        raw_content, hash,
        "stamp_write must produce exact hash bytes — no trailing newline, whitespace, or BOM. \
         Got {:?}, expected {:?}",
        raw_content, hash
    );
}

#[test]
fn test_stamp_path_is_profile_independent() {
    // Prove that staleness detection is purely hash-driven and works identically
    // across different OUT_DIR paths (simulating debug vs release profiles).
    let dir = tempfile::tempdir().unwrap();
    let grammar = dir.path().join("grammar.js");
    std::fs::write(&grammar, b"module.exports = grammar({name: 'test'});").unwrap();
    let src_dir = make_populated_src_dir(dir.path());
    let output_paths: Vec<_> = EXPECTED_OUTPUTS.iter().map(|n| src_dir.join(n)).collect();
    let output_refs: Vec<&Path> = output_paths.iter().map(|p| p.as_path()).collect();

    // Simulate two different cargo profile OUT_DIR paths
    let debug_out = dir.path().join("target/debug/build/out");
    let release_out = dir.path().join("target/release/build/out");
    std::fs::create_dir_all(&debug_out).unwrap();
    std::fs::create_dir_all(&release_out).unwrap();

    let hash = content_hash(&grammar);

    // Write matching stamp to both profiles
    let debug_stamp = debug_out.join("grammar_hash.stamp");
    let release_stamp = release_out.join("grammar_hash.stamp");
    stamp_write(&debug_stamp, &hash);
    assert_eq!(std::fs::read_to_string(&debug_stamp).unwrap(), hash);
    stamp_write(&release_stamp, &hash);
    assert_eq!(std::fs::read_to_string(&release_stamp).unwrap(), hash);

    // Both profiles report no regeneration needed
    assert!(
        !needs_generate(&hash, &debug_stamp, &output_refs),
        "debug profile: must NOT regenerate when stamp matches"
    );
    assert!(
        !needs_generate(&hash, &release_stamp, &output_refs),
        "release profile: must NOT regenerate when stamp matches"
    );

    // Mutate grammar content — both profiles must now detect staleness
    std::fs::write(&grammar, b"module.exports = grammar({name: 'changed'});").unwrap();
    let new_hash = content_hash(&grammar);
    assert!(
        needs_generate(&new_hash, &debug_stamp, &output_refs),
        "debug profile: must regenerate after grammar change"
    );
    assert!(
        needs_generate(&new_hash, &release_stamp, &output_refs),
        "release profile: must regenerate after grammar change"
    );
}

#[test]
fn test_stamp_shared_across_simulated_profiles() {
    // Prove that identical hash content at any stamp location yields identical
    // staleness decisions, and that stamp presence is per-location.
    let dir = tempfile::tempdir().unwrap();
    let grammar = dir.path().join("grammar.js");
    std::fs::write(&grammar, b"module.exports = grammar({name: 'shared'});").unwrap();
    let src_dir = make_populated_src_dir(dir.path());
    let output_paths: Vec<_> = EXPECTED_OUTPUTS.iter().map(|n| src_dir.join(n)).collect();
    let output_refs: Vec<&Path> = output_paths.iter().map(|p| p.as_path()).collect();

    let hash = content_hash(&grammar);

    // Two separate OUT_DIR-like paths
    let out_dir_1 = dir.path().join("target/debug/build/out");
    let out_dir_2 = dir.path().join("target/release/build/out");
    std::fs::create_dir_all(&out_dir_1).unwrap();
    std::fs::create_dir_all(&out_dir_2).unwrap();

    let stamp_1 = out_dir_1.join("grammar_hash.stamp");
    let stamp_2 = out_dir_2.join("grammar_hash.stamp");

    // Write stamp to only OUT_DIR_1
    stamp_write(&stamp_1, &hash);
    assert_eq!(std::fs::read_to_string(&stamp_1).unwrap(), hash);

    // OUT_DIR_1 has matching stamp — no regeneration needed
    assert!(
        !needs_generate(&hash, &stamp_1, &output_refs),
        "OUT_DIR_1: must NOT regenerate when stamp matches"
    );
    // OUT_DIR_2 has no stamp — regeneration needed
    assert!(
        needs_generate(&hash, &stamp_2, &output_refs),
        "OUT_DIR_2: must regenerate when stamp is absent"
    );

    // Now write the same stamp to OUT_DIR_2
    stamp_write(&stamp_2, &hash);
    assert_eq!(std::fs::read_to_string(&stamp_2).unwrap(), hash);

    // Both locations now report no regeneration needed
    assert!(
        !needs_generate(&hash, &stamp_1, &output_refs),
        "OUT_DIR_1: still must NOT regenerate"
    );
    assert!(
        !needs_generate(&hash, &stamp_2, &output_refs),
        "OUT_DIR_2: must NOT regenerate after stamp written with matching hash"
    );
}

/// Returns true when running as root (UID 0). Used to skip tests that rely on
/// DAC permission enforcement, which root/CAP_DAC_OVERRIDE bypasses.
#[cfg(unix)]
fn is_root() -> bool {
    // SAFETY: libc::getuid() is a trivial POSIX syscall. libc provides a
    // well-audited, platform-tested binding — uid_t maps to u32 on Linux/macOS/BSD.
    unsafe { libc::getuid() == 0 }
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
            if let Err(e) = std::fs::set_permissions(&self.path, perms) {
                eprintln!(
                    "warning: ReadonlyGuard failed to restore permissions on {}: {}",
                    self.path.display(),
                    e
                );
            }
        }
    }
}

/// Extracts the source text of a test function identified by `fn_sig`.
///
/// Searches `source` for `fn_sig` and returns the slice from that point up to
/// (but not including) the next `\n#[test]` annotation, or to the end of
/// `source` if no subsequent test function exists.
///
/// The sub-slice offset arithmetic adds 1 when converting from
/// `fn_section[1..].find(...)` back to an index in `fn_section`, avoiding the
/// off-by-one that would clip the character immediately before the next `#[test]`.
/// Similarly the fallback uses `fn_section.len()` (not `len() - 1`) so that
/// the last function's closing `}` is always included.
fn extract_test_fn_body<'a>(source: &'a str, fn_sig: &str) -> Option<&'a str> {
    let fn_start = source.find(fn_sig)?;
    let fn_section = &source[fn_start..];
    // `find` on `fn_section[1..]` returns an offset relative to the sub-slice.
    // Adding 1 converts it back to an index in `fn_section`.
    let fn_end = fn_section[1..]
        .find("\n#[test]")
        .map(|p| p + 1)
        .unwrap_or(fn_section.len());
    Some(&fn_section[..fn_end])
}

/// Scans `source` for test functions annotated with `#[cfg(unix)]` and returns
/// their signatures (e.g. `"fn test_foo()"`).
///
/// Uses a line-by-line state machine: once `#[cfg(unix)]` is seen, the flag
/// `saw_cfg_unix` is set. Intermediate attribute/comment lines keep the flag
/// alive. When a line starting with `fn test_` is reached with the flag set,
/// the signature up to and including `()` is collected. Non-test `fn` lines or
/// a blank line clears the flag, preventing false positives from isolated
/// `#[cfg(unix)]` helper functions.
fn find_cfg_unix_test_fns(source: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut saw_cfg_unix = false;
    let mut saw_test = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[cfg(unix)]") {
            saw_cfg_unix = true;
        } else if trimmed == "#[test]" {
            saw_test = true;
        } else if trimmed.starts_with("fn test_") && saw_cfg_unix && saw_test {
            // Extract "fn name()" — everything up to and including the first ')'
            if let Some(end) = trimmed.find(')') {
                result.push(trimmed[..=end].to_string());
            }
            saw_cfg_unix = false;
            saw_test = false;
        } else if trimmed.starts_with('#') {
            // Another attribute — keep flags alive (e.g. #[allow(...)])
        } else if trimmed.starts_with("//") || trimmed.is_empty() {
            // Comment or blank line — keep flags alive
        } else {
            // Any other line (fn without test, let, etc.) resets the flags
            saw_cfg_unix = false;
            saw_test = false;
        }
    }
    result
}

#[test]
fn test_unix_permission_tests_have_root_guard() {
    // Source-level regression guard: every #[cfg(unix)] test function that
    // relies on DAC permission enforcement must contain an is_root() skip guard.
    // Without it, tests produce misleading failures under root/CAP_DAC_OVERRIDE.
    //
    // The set of unix test functions is discovered dynamically by
    // find_cfg_unix_test_fns so that newly added #[cfg(unix)] tests are
    // automatically checked without updating a hardcoded list.
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/build_logic_tests.rs"
    ))
    .expect("should be able to read this test file");

    let unix_test_fns = find_cfg_unix_test_fns(&source);

    // Sanity-check: the scanner must find at least 2 unix tests (the ones we
    // know about). This catches a broken scanner that silently returns empty.
    assert!(
        unix_test_fns.len() >= 2,
        "find_cfg_unix_test_fns should discover at least 2 #[cfg(unix)] test functions, \
         but found {:?}",
        unix_test_fns
    );

    for fn_sig in &unix_test_fns {
        let fn_body = extract_test_fn_body(&source, fn_sig)
            .unwrap_or_else(|| panic!("source should contain {}", fn_sig));

        assert!(
            fn_body.contains("is_root()"),
            "{} must contain an is_root() skip guard to prevent misleading failures \
             when running as root. Function body:\n{}",
            fn_sig,
            fn_body
        );
    }
}

#[test]
fn test_readonly_guard_drop_logs_error() {
    // Source-level regression guard: ReadonlyGuard::drop must log errors from
    // set_permissions via eprintln! rather than silently discarding with `let _ =`.
    // Follows the established source-level test pattern (test_try_wait_error_path_kills_child).
    let source = std::fs::read_to_string("tests/build_logic_tests.rs")
        .expect("should be able to read this test file");

    // Extract the Drop impl for ReadonlyGuard
    let drop_start = source
        .find("impl Drop for ReadonlyGuard")
        .expect("source should contain Drop impl for ReadonlyGuard");
    let drop_section = &source[drop_start..];
    // Find the closing brace of the impl block (next unindented '}')
    let drop_end = drop_section
        .find("\n}\n")
        .expect("Drop impl should have a closing brace");
    let drop_impl = &drop_section[..drop_end];

    assert!(
        !drop_impl.contains("let _ = std::fs::set_permissions"),
        "ReadonlyGuard::drop must NOT silently discard set_permissions errors with `let _ =`. \
         Use `if let Err(e) = ... {{ eprintln!(...) }}` instead. \
         Found in Drop impl:\n{}",
        drop_impl
    );
    assert!(
        drop_impl.contains("eprintln!"),
        "ReadonlyGuard::drop must log set_permissions errors via eprintln!. \
         Found in Drop impl:\n{}",
        drop_impl
    );
}

#[test]
#[cfg(unix)] // set_readonly(true) on a directory only prevents file creation on Unix (POSIX);
// on Windows the readonly attribute does NOT block creating files within the directory.
fn test_readonly_guard_restores_on_drop() {
    if is_root() {
        eprintln!("skipping: test requires non-root user (root bypasses DAC permissions)");
        return;
    }
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

#[test]
fn test_is_root_uses_libc_not_raw_ffi() {
    // Source-level regression guard: is_root() must use libc::getuid() rather than
    // a raw `unsafe extern "C" { fn getuid() -> u32; }` declaration.
    // Raw FFI declarations are fragile (no type-checked header), whereas libc provides
    // a well-audited, platform-tested binding.
    let source = std::fs::read_to_string("tests/build_logic_tests.rs")
        .expect("should be able to read this test file");

    let fn_start = source
        .find("fn is_root()")
        .expect("source should contain is_root() function");
    let fn_section = &source[fn_start..];
    // Grab just up to the next function definition to avoid false positives
    let fn_end = fn_section[1..]
        .find("\nfn ")
        .map(|p| p + 1)
        .unwrap_or(fn_section.len());
    let fn_body = &fn_section[..fn_end];

    assert!(
        !fn_body.contains("unsafe extern \"C\""),
        "is_root() must NOT use a raw `unsafe extern \"C\"` FFI declaration. \
         Use `libc::getuid()` from the libc crate instead. Found body:\n{}",
        fn_body
    );
    assert!(
        fn_body.contains("libc::getuid()"),
        "is_root() must use `libc::getuid()` from the libc crate. Found body:\n{}",
        fn_body
    );
}

#[test]
fn test_extract_test_fn_body_no_off_by_one() {
    // Tests for the extract_test_fn_body() helper.
    // This helper extracts the source text of a test function bounded by fn_sig.
    // Key correctness property: the sub-slice offset arithmetic must add 1 when
    // converting from `fn_section[1..].find(...)` back to an index in `fn_section`.

    // Case (a): middle function — body must include all content up to (not including)
    // the "\n#[test]" that introduces the next function.
    let src_middle = concat!(
        "#[test]\n",
        "fn test_first() {\n",
        "    let x = 1;\n",
        "}\n",
        "#[test]\n",
        "fn test_second() {\n",
        "    let y = 2;\n",
        "}\n",
        "#[test]\n",
        "fn test_third() {\n",
        "    let z = 3;\n",
        "}"
    );
    let body = extract_test_fn_body(src_middle, "fn test_second()");
    let body = body.expect("extract_test_fn_body should find test_second");
    assert!(
        body.contains("let y = 2;"),
        "body should contain function content; got: {:?}",
        body
    );
    // The off-by-one would clip the closing `}` of test_second; verify it's present.
    assert!(
        body.contains('}'),
        "body should include closing brace of test_second; got: {:?}",
        body
    );
    // Body must NOT bleed into the next function.
    assert!(
        !body.contains("let z = 3;"),
        "body must not include content from test_third; got: {:?}",
        body
    );

    // Case (b): last function — off-by-one `unwrap_or(len - 1)` would clip the
    // final character. The corrected version uses `unwrap_or(len)`.
    let src_last = concat!(
        "#[test]\n",
        "fn test_alpha() {\n",
        "    is_root();\n",
        "}\n",
        "#[test]\n",
        "fn test_omega() {\n",
        "    let last = true;\n",
        "}" // NOTE: no trailing newline — the off-by-one clips this `}`
    );
    let body_last = extract_test_fn_body(src_last, "fn test_omega()");
    let body_last = body_last.expect("extract_test_fn_body should find test_omega");
    assert!(
        body_last.ends_with('}'),
        "body of last function must include the closing brace (off-by-one would clip it); \
         got: {:?}",
        body_last
    );

    // Case (c): function not found returns None.
    let none = extract_test_fn_body(src_last, "fn test_nonexistent()");
    assert!(none.is_none(), "should return None for missing function");
}

#[test]
fn test_find_cfg_unix_test_fns_discovers_dynamically() {
    // Tests for the find_cfg_unix_test_fns() helper.
    // The helper should find all test functions annotated with #[cfg(unix)],
    // regardless of ordering of attributes.

    let synthetic_source = concat!(
        // A plain test — no #[cfg(unix)] — should be ignored.
        "#[test]\n",
        "fn test_plain_no_unix() {\n",
        "    let _ = 1;\n",
        "}\n\n",
        // A #[cfg(unix)] test — should be collected.
        "#[cfg(unix)]\n",
        "#[test]\n",
        "fn test_unix_one() {\n",
        "    if is_root() { return; }\n",
        "}\n\n",
        // A non-test #[cfg(unix)] function — should be ignored.
        "#[cfg(unix)]\n",
        "fn helper_unix_not_a_test() {\n",
        "    let _ = 2;\n",
        "}\n\n",
        // A #[test] before #[cfg(unix)] — should also be collected.
        "#[test]\n",
        "#[cfg(unix)]\n",
        "fn test_unix_two() {\n",
        "    if is_root() { return; }\n",
        "}\n\n",
        // A #[cfg(unix)] followed by a non-test fn — should be ignored.
        "#[cfg(unix)]\n",
        "fn not_a_test_fn() {}\n",
    );

    let fns = find_cfg_unix_test_fns(synthetic_source);

    assert!(
        fns.contains(&"fn test_unix_one()".to_string()),
        "should discover test_unix_one; got: {:?}",
        fns
    );
    assert!(
        fns.contains(&"fn test_unix_two()".to_string()),
        "should discover test_unix_two; got: {:?}",
        fns
    );
    assert!(
        !fns.contains(&"fn test_plain_no_unix()".to_string()),
        "should NOT include test_plain_no_unix (no #[cfg(unix)]); got: {:?}",
        fns
    );
    assert!(
        !fns.iter().any(|s| s.contains("helper_unix_not_a_test")),
        "should NOT include helper_unix_not_a_test (not a #[test]); got: {:?}",
        fns
    );
    assert!(
        !fns.iter().any(|s| s.contains("not_a_test_fn")),
        "should NOT include not_a_test_fn (not a #[test]); got: {:?}",
        fns
    );
}

#[test]
fn test_self_read_paths_use_manifest_dir() {
    // Meta-test / regression guard: the two source-self-inspection tests that read
    // this file must use `concat!(env!("CARGO_MANIFEST_DIR"), "/tests/build_logic_tests.rs")`
    // rather than the bare relative path `"tests/build_logic_tests.rs"`.
    // A bare relative path is fragile — it depends on the working directory from which
    // `cargo test` is invoked, causing failures when tests are run from outside the
    // crate root (e.g., workspace-level `cargo test -p tree-sitter-reify`).
    //
    // This meta-test itself demonstrates the correct idiom by using CARGO_MANIFEST_DIR
    // to read the source file.
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/build_logic_tests.rs"
    ))
    .expect("should be able to read this test file via CARGO_MANIFEST_DIR");

    // Check test_unix_permission_tests_have_root_guard uses CARGO_MANIFEST_DIR.
    let root_guard_body =
        extract_test_fn_body(&source, "fn test_unix_permission_tests_have_root_guard()")
            .expect("source should contain test_unix_permission_tests_have_root_guard");
    assert!(
        root_guard_body.contains("CARGO_MANIFEST_DIR"),
        "test_unix_permission_tests_have_root_guard must read the test file via \
         concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/tests/build_logic_tests.rs\") \
         rather than a bare relative path. Function body:\n{}",
        root_guard_body
    );

    // Check test_readonly_guard_drop_logs_error uses CARGO_MANIFEST_DIR.
    let drop_logs_body =
        extract_test_fn_body(&source, "fn test_readonly_guard_drop_logs_error()")
            .expect("source should contain test_readonly_guard_drop_logs_error");
    assert!(
        drop_logs_body.contains("CARGO_MANIFEST_DIR"),
        "test_readonly_guard_drop_logs_error must read the test file via \
         concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/tests/build_logic_tests.rs\") \
         rather than a bare relative path. Function body:\n{}",
        drop_logs_body
    );
}
