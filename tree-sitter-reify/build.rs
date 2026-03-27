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

fn needs_generate(parser_path: &std::path::Path, grammar_path: &std::path::Path) -> bool {
    if !parser_path.exists() {
        return true;
    }
    // Regenerate if grammar.js is newer than parser.c.
    if grammar_path.exists()
        && let (Ok(gm), Ok(pm)) = (grammar_path.metadata(), parser_path.metadata())
        && let (Ok(gt), Ok(pt)) = (gm.modified(), pm.modified())
    {
        return gt > pt;
    }
    false
}

fn main() {
    let src_dir = std::path::Path::new("src");
    let parser_path = src_dir.join("parser.c");
    let grammar_path = std::path::Path::new("grammar.js");

    // Re-run if the grammar source or generated parser changes.
    println!("cargo:rerun-if-changed=grammar.js");
    println!("cargo:rerun-if-changed=src/parser.c");

    // Auto-generate parser.c from grammar.js when missing or stale.
    if needs_generate(&parser_path, grammar_path) {
        run_tree_sitter_generate();
        // Verify parser.c was actually created.
        if !parser_path.exists() {
            panic!(
                "tree-sitter generate succeeded but src/parser.c was not created. \
                 Check tree-sitter CLI version."
            );
        }
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
