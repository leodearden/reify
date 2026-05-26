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
        run_tree_sitter_generate();
        // Verify all 3 output files were created.
        verify_outputs(src_dir);
        // Write stamp using the *same* hash that was checked — guarantees the
        // stamp reflects the grammar version that produced these outputs.
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
