// The build-script logic shared with `tests/build_logic_tests.rs`.
//
// A build script cannot be `use`d by a test target, and the old workaround —
// hand-copying this logic into the test file — is how `#6992` shipped: the
// replica was green while this file was wrong. One source, two include sites.
include!("build_support.rs");

/// Bound on one `tree-sitter generate` run, in seconds. It takes about 4 s on
/// an idle host; this host has run at roughly 15x CPU oversubscription, and the
/// bound still has to catch a genuine hang.
const GENERATE_TIMEOUT_SECS: u64 = 300;

/// Generate the parser from grammar.js into `generated`, or panic.
fn generate(cli: Option<&std::path::Path>, generated: &std::path::Path) {
    let Some(cli) = cli else {
        panic!(
            "tree-sitter-reify: the tree-sitter CLI is not on PATH, and it generates the \
             parser into {}.\ninstall: cargo install tree-sitter-cli",
            generated.display()
        );
    };
    std::fs::create_dir_all(generated)
        .unwrap_or_else(|e| panic!("failed to create {}: {e}", generated.display()));
    let generated_arg = generated
        .to_str()
        .expect("OUT_DIR comes from a UTF-8 environment variable");
    eprintln!(
        "tree-sitter-reify: running tree-sitter generate into {}",
        generated.display()
    );
    if let Err(msg) = run_with_timeout(
        cli,
        &[
            "generate",
            "grammar.js",
            "--abi",
            GENERATED_PARSER_ABI,
            "-o",
            generated_arg,
        ],
        GENERATE_TIMEOUT_SECS,
    ) {
        panic!("tree-sitter generate failed: {msg}");
    }
    verify_outputs(generated).unwrap_or_else(|msg| panic!("{msg}"));
}

fn main() {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR must be set by cargo");
    let out_dir = std::path::Path::new(&out_dir);
    let generated = out_dir.join(GENERATED_DIR);
    let key_stamp = out_dir.join(KEY_STAMP_NAME);
    let cli = find_on_path("tree-sitter", std::env::var_os("PATH").as_deref());

    // The tracked inputs, this script's include!d logic, and the CLI: nothing
    // this script writes. cargo stamps a run at its start, so a watched output
    // would rerun the script on the next build. cc emits no watch of its own.
    for input in ATTESTED_INPUTS {
        println!("cargo:rerun-if-changed={input}");
    }
    println!("cargo:rerun-if-changed=build_support.rs");
    if let Some(cli) = &cli {
        println!("cargo:rerun-if-changed={}", cli.display());
    }

    let grammar = std::path::Path::new("grammar.js");
    let grammar_before = sha256_of_path(grammar);
    let key = match (&grammar_before, &cli) {
        (Ok(Some(sha)), Some(cli)) => cli_version(cli).map(|version| generation_key(sha, &version)),
        _ => None,
    };

    if needs_generate(key.as_deref(), &key_stamp, &generated) {
        remove_if_present(&key_stamp);
        generate(cli.as_deref(), &generated);
        // The key describes grammar.js as hashed before generating; if it
        // changed meanwhile, the outputs may describe either version.
        let grammar_unchanged = matches!(
            (&grammar_before, sha256_of_path(grammar)),
            (Ok(Some(before)), Ok(Some(after))) if *before == after
        );
        match &key {
            Some(key) if grammar_unchanged => write_generation_stamps(&generated, &key_stamp, key),
            _ => eprintln!(
                "tree-sitter-reify: grammar.js changed during generation, or no hasher or \
                 CLI version is available; leaving {} unproven (the next run regenerates)",
                generated.display()
            ),
        }
    }

    // Compiled from OUT_DIR alone, so scanner.c's quote include resolves to the
    // generated header beside it and never to tree-sitter-reify/src. Removed,
    // then written as bytes rather than fs::copy'd, so a read-only checkout's
    // mode never reaches the copy and blocks the next run from replacing it.
    let scanner_copy = generated.join("scanner.c");
    remove_if_present(&scanner_copy);
    std::fs::read("src/scanner.c")
        .and_then(|bytes| std::fs::write(&scanner_copy, bytes))
        .unwrap_or_else(|e| {
            panic!(
                "failed to copy src/scanner.c to {}: {e}",
                scanner_copy.display()
            )
        });
    cc::Build::new()
        .include(&generated)
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-trigraphs")
        .file(generated.join("parser.c"))
        .file(&scanner_copy)
        .compile("tree_sitter_reify");

    // compile() panics on failure, so the archive exists: attest its sources.
    write_inputs_stamp(out_dir, &key_stamp, &scanner_copy);
}
