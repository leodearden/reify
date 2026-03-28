use std::path::Path;
use std::process::Command;

fn main() {
    let src_dir = Path::new("src");
    let parser_path = src_dir.join("parser.c");

    // Re-run when the grammar source changes or the generated parser is missing.
    println!("cargo:rerun-if-changed=grammar.js");
    println!("cargo:rerun-if-changed=src/parser.c");

    // Auto-generate parser.c from grammar.js when it doesn't exist.
    // This happens after a fresh clone/checkout because generated files are gitignored
    // to avoid merge conflicts.
    if !parser_path.exists() {
        eprintln!("tree-sitter-reify: src/parser.c not found — running `tree-sitter generate`");
        let status = Command::new("tree-sitter")
            .arg("generate")
            .status()
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to run `tree-sitter generate`: {e}\n\
                     Install tree-sitter CLI: cargo install tree-sitter-cli\n\
                     Or: npm install -g tree-sitter-cli"
                );
            });
        if !status.success() {
            panic!(
                "`tree-sitter generate` failed with exit code {:?}.\n\
                 Ensure grammar.js is valid and tree-sitter-cli >= 0.26 is installed.",
                status.code()
            );
        }
        assert!(
            parser_path.exists(),
            "tree-sitter generate succeeded but src/parser.c was not created"
        );
    }

    // Compile the generated C parser into a static library.
    let mut c_config = cc::Build::new();
    c_config.include(src_dir);
    c_config
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-trigraphs");
    c_config.file(&parser_path);
    c_config.compile("tree_sitter_reify");
}
