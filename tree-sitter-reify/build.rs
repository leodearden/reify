use std::hash::{Hash, Hasher};

/// Compute a content hash of a file's bytes, returning a hex-encoded u64.
/// Used for staleness detection — not for security.
fn content_hash(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|e| {
        panic!("Failed to read {} for hashing: {}", path.display(), e)
    });
    let mut hasher = std::hash::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn run_tree_sitter_generate() {
    eprintln!("tree-sitter-reify: running tree-sitter generate...");
    let status = std::process::Command::new("tree-sitter")
        .arg("generate")
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            panic!(
                "tree-sitter generate failed with exit code {}.\n\
                 Ensure tree-sitter CLI is installed.\n\
                 Or run: scripts/tree-sitter-generate.sh",
                s.code().unwrap_or(-1)
            );
        }
        Err(e) => {
            panic!(
                "Failed to run tree-sitter generate: {}\n\
                 Ensure tree-sitter CLI is installed.\n\
                 Or run: scripts/tree-sitter-generate.sh",
                e
            );
        }
    }
}

/// The expected output files that tree-sitter generate produces.
const EXPECTED_OUTPUTS: &[&str] = &["parser.c", "grammar.json", "node-types.json"];

/// Check if regeneration is needed based on content hash staleness.
/// Returns true if any output file is missing, stamp file is missing,
/// or stamp hash doesn't match grammar.js content hash.
fn needs_generate(
    grammar_path: &std::path::Path,
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
    let current_hash = content_hash(grammar_path);
    stamp_content.trim() != current_hash
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
    let output_paths: Vec<std::path::PathBuf> = EXPECTED_OUTPUTS
        .iter()
        .map(|n| src_dir.join(n))
        .collect();
    let output_refs: Vec<&std::path::Path> = output_paths.iter().map(|p| p.as_path()).collect();
    // Stamp file stored in OUT_DIR (cargo build directory).
    let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());
    let stamp_path = std::path::Path::new(&out_dir).join("grammar_hash.stamp");

    if needs_generate(grammar_path, &stamp_path, &output_refs) {
        run_tree_sitter_generate();
        // Verify all 3 output files were created.
        verify_outputs(src_dir);
        // Write updated stamp.
        let hash = content_hash(grammar_path);
        std::fs::write(&stamp_path, &hash).unwrap_or_else(|e| {
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
    c_config.compile("tree_sitter_reify");
}
