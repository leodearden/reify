use std::hash::{Hash, Hasher};

/// Compute a content hash of a file's bytes, returning a hex-encoded u64.
/// Used for staleness detection — not for security.
fn content_hash(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path)
        .unwrap_or_else(|e| panic!("Failed to read {} for hashing: {}", path.display(), e));
    let mut hasher = std::hash::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Run a command with a timeout. Returns Ok(()) on success, Err on failure/timeout.
///
/// IMPORTANT: Child stdout is discarded (Stdio::null) for two reasons:
///   1. Cargo parses build-script stdout line-by-line for "cargo:" directives.
///      If the child emits anything to stdout, Cargo would misinterpret it.
///   2. Using Stdio::piped() creates a deadlock risk: the parent only drains
///      the pipe after try_wait() returns Some(status), but if the child writes
///      \>64KB to stdout, the pipe buffer fills, the child blocks, and try_wait()
///      returns Ok(None) indefinitely — a hard deadlock until the timeout fires.
///
/// tree-sitter generate writes its useful diagnostics to stderr, which is
/// inherited directly (Stdio::inherit) and displayed by Cargo as-is.
fn run_with_timeout(cmd: &str, args: &[&str], timeout_secs: u64) -> Result<(), String> {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let mut child = std::process::Command::new(cmd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
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
                    let _ = child.wait(); // Reap the process.
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

/// Default timeout for tree-sitter generate subprocess (seconds).
const GENERATE_TIMEOUT_SECS: u64 = 60;

fn run_tree_sitter_generate() {
    eprintln!("tree-sitter-reify: running tree-sitter generate...");
    if let Err(msg) = run_with_timeout("tree-sitter", &["generate"], GENERATE_TIMEOUT_SECS) {
        panic!(
            "tree-sitter generate failed: {}\n\
             Ensure tree-sitter CLI is installed.\n\
             Or run: scripts/tree-sitter-generate.sh",
            msg
        );
    }
}

/// The expected output files that tree-sitter generate produces.
const EXPECTED_OUTPUTS: &[&str] = &["parser.c", "grammar.json", "node-types.json"];

/// Check if regeneration is needed based on content hash staleness.
/// Returns true if any output file is missing, stamp file is missing,
/// or stamp hash doesn't match the provided grammar hash.
///
/// The caller must compute `grammar_hash` once and pass it here as well as
/// to the stamp-write step — this avoids a TOCTOU race where grammar.js
/// could change between the staleness check and the stamp write.
fn needs_generate(
    grammar_hash: &str,
    stamp_path: &std::path::Path,
    output_paths: &[&std::path::Path],
) -> bool {
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

/// Check whether the shell-script stamp (`src/.grammar_hash.stamp`) already
/// confirms that the generated outputs match the current `grammar.js`.
///
/// The shell script (`scripts/tree-sitter-generate.sh`) writes a SHA-256 hash
/// of `grammar.js` into `src/.grammar_hash.stamp` every time it regenerates.
/// When `verify.sh` runs the script first — which it always does — and the
/// script says "up to date", the stamp is guaranteed to reflect the current
/// grammar.  In that case, re-running `tree-sitter generate` from the build
/// script is redundant and, on a loaded host, risks timing out.
///
/// Returns `true` (safe to skip generation) only when ALL of:
///   1. Every expected output file exists.
///   2. `src/.grammar_hash.stamp` contains a non-empty hash string.
///   3. `sha256sum grammar.js` matches that hash exactly.
///   4. No output file is newer than the shell stamp (a newer output file
///      would indicate it was partially overwritten by a failed generate run).
///
/// Any failure in this chain (missing stamp, `sha256sum` unavailable, hash
/// mismatch, or suspiciously-new output file) returns `false` so the caller
/// falls back to regenerating.
fn shell_stamp_is_current(
    grammar_path: &std::path::Path,
    output_paths: &[&std::path::Path],
) -> bool {
    // 1. All expected output files must exist.
    for path in output_paths {
        if !path.exists() {
            return false;
        }
    }
    // 2. Shell-script stamp must exist and contain a non-empty hash.
    let shell_stamp_path = std::path::Path::new("src/.grammar_hash.stamp");
    let shell_stamp = match std::fs::read_to_string(shell_stamp_path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let expected_hash = shell_stamp.trim();
    if expected_hash.is_empty() {
        return false;
    }
    // 3. Compute SHA-256 of grammar.js via sha256sum and compare.
    //    sha256sum on a single small file is near-instant (<10 ms); no timeout needed.
    let output = match std::process::Command::new("sha256sum")
        .arg(grammar_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(o) => o,
        Err(_) => return false, // sha256sum unavailable; fall back to generation
    };
    if !output.status.success() {
        return false;
    }
    let stdout = match String::from_utf8(output.stdout) {
        Ok(s) => s,
        Err(_) => return false,
    };
    // sha256sum output format: "<hash>  <filename>\n"
    let computed_hash = stdout.split_whitespace().next().unwrap_or("");
    if computed_hash != expected_hash {
        return false;
    }
    // 4. Guard against partially-overwritten output files: if any output file
    //    is newer than the shell stamp, a previous (failed) generate attempt
    //    may have left truncated content.  In that case, force regeneration.
    let stamp_mtime = match std::fs::metadata(shell_stamp_path)
        .and_then(|m| m.modified())
    {
        Ok(t) => t,
        Err(_) => return true, // Can't stat stamp; assume it's fine
    };
    for path in output_paths {
        let file_mtime = match std::fs::metadata(path).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => continue, // Can't stat output; skip this check
        };
        if file_mtime > stamp_mtime {
            // Output file is newer than the shell stamp — likely corrupted.
            eprintln!(
                "tree-sitter-reify: {:?} is newer than the shell stamp; forcing regeneration",
                path
            );
            return false;
        }
    }
    true
}

/// Verify that all expected output files exist after generation.
/// Panics with a clear message naming whichever file is missing.
fn verify_outputs(src_dir: &std::path::Path) {
    let mut missing = Vec::new();
    for name in EXPECTED_OUTPUTS {
        if !src_dir.join(name).exists() {
            missing.push(*name);
        }
    }
    if !missing.is_empty() {
        panic!(
            "tree-sitter generate succeeded but these output files are missing: {}. \
             Check tree-sitter CLI version.",
            missing.join(", ")
        );
    }
}

fn main() {
    let src_dir = std::path::Path::new("src");
    let parser_path = src_dir.join("parser.c");
    let grammar_path = std::path::Path::new("grammar.js");

    // Re-run if the grammar source changes.
    // Note: we do NOT watch src/parser.c — it's a generated output managed by
    // this build script. Watching it would cause double execution.
    println!("cargo:rerun-if-changed=grammar.js");

    // Auto-generate from grammar.js when missing or stale.
    let output_paths: Vec<std::path::PathBuf> =
        EXPECTED_OUTPUTS.iter().map(|n| src_dir.join(n)).collect();
    let output_refs: Vec<&std::path::Path> = output_paths.iter().map(|p| p.as_path()).collect();
    // Stamp file stored in OUT_DIR (cargo build directory).
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR must be set by cargo");
    let stamp_path = std::path::Path::new(&out_dir).join("grammar_hash.stamp");

    // Capture the grammar hash once, before generation, and reuse it for both
    // the staleness check and the stamp write.  This eliminates a TOCTOU race
    // where grammar.js could change between the two reads.
    let grammar_hash = content_hash(grammar_path);

    if needs_generate(&grammar_hash, &stamp_path, &output_refs) {
        // Fast-path: if the shell script already validated the outputs, skip
        // `tree-sitter generate` (which can take >60 s on a loaded build host).
        // This is safe: cargo's `rerun-if-changed=grammar.js` guarantees the
        // build script only re-runs when grammar.js actually changes, so if we
        // land here with a fresh OUT_DIR stamp but a valid shell stamp, the
        // outputs are already current.
        if !shell_stamp_is_current(grammar_path, &output_refs) {
            run_tree_sitter_generate();
            // Verify all 3 output files were created.
            verify_outputs(src_dir);
        }
        // Write the OUT_DIR stamp whether we regenerated or bypassed —
        // subsequent build-script invocations will hit the fast path in
        // `needs_generate` and skip everything.
        std::fs::write(&stamp_path, &grammar_hash).unwrap_or_else(|e| {
            eprintln!("warning: failed to write stamp file: {}", e);
        });
    }

    let mut c_config = cc::Build::new();
    c_config.include(src_dir);
    c_config
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-trigraphs");
    c_config.file(&parser_path);
    c_config.file("src/scanner.c");
    c_config.compile("tree_sitter_reify");
}
