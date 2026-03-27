fn main() {
    let src_dir = std::path::Path::new("src");
    let parser_path = src_dir.join("parser.c");
    let grammar_path = std::path::Path::new("grammar.js");

    // Re-run if the grammar source or generated parser changes.
    println!("cargo:rerun-if-changed=grammar.js");
    println!("cargo:rerun-if-changed=src/parser.c");

    // Auto-generate parser.c from grammar.js when missing.
    if !parser_path.exists() {
        eprintln!("tree-sitter-reify: parser.c not found, running tree-sitter generate...");
        let status = std::process::Command::new("tree-sitter")
            .arg("generate")
            .status();
        match status {
            Ok(s) if s.success() => {
                eprintln!("tree-sitter-reify: parser.c generated successfully");
            }
            Ok(s) => {
                panic!(
                    "tree-sitter generate failed with exit code {}.\n\
                     Ensure tree-sitter CLI is installed: cargo install tree-sitter-cli",
                    s.code().unwrap_or(-1)
                );
            }
            Err(e) => {
                panic!(
                    "Failed to run tree-sitter generate: {}\n\
                     Install tree-sitter CLI: cargo install tree-sitter-cli\n\
                     Or run: scripts/tree-sitter-generate.sh",
                    e
                );
            }
        }
    } else if grammar_path.exists() {
        // Warn if grammar.js is newer than parser.c (stale generated files).
        if let (Ok(grammar_meta), Ok(parser_meta)) =
            (grammar_path.metadata(), parser_path.metadata())
        {
            if let (Ok(grammar_mod), Ok(parser_mod)) =
                (grammar_meta.modified(), parser_meta.modified())
            {
                if grammar_mod > parser_mod {
                    println!(
                        "cargo:warning=tree-sitter-reify: grammar.js is newer than parser.c. \
                         Run `scripts/tree-sitter-generate.sh` to regenerate."
                    );
                }
            }
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
