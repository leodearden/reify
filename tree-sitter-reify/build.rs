// The staleness primitives shared with `tests/build_logic_tests.rs`.
//
// A build script cannot be `use`d by a test target, and the old workaround —
// hand-copying this logic into the test file — is how `#6992` shipped: the
// replica was green while this file was wrong. One source, two include sites.
include!("build_support.rs");

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

/// The exact set of files whose bytes end up inside `libtree_sitter_reify.a`:
/// the two translation units handed to `cc::Build`, plus the headers they include.
///
/// Paths are package-root-relative and sorted by byte order, matching
/// `scripts/tree-sitter-freshness.sh --list-inputs` exactly. The two sides must
/// agree byte-for-byte or every freshness check is meaningless, so this is the
/// SINGLE enumeration used by both the watch-directive loop and the stamp writer
/// below — they cannot drift.
///
/// Headers come from a sorted `read_dir` rather than a hardcoded
/// alloc.h/array.h/parser.h list: a hardcoded list is exactly how this defect
/// class recurs (someone adds a header, nothing watches it). See `#5629`.
fn compilation_inputs() -> Vec<String> {
    let mut headers: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir("src/tree_sitter") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".h") {
                headers.push(format!("src/tree_sitter/{}", name));
            }
        }
    }
    headers.sort();

    let mut inputs = vec!["src/parser.c".to_string(), "src/scanner.c".to_string()];
    inputs.extend(headers);
    inputs
}

/// Attest what was just compiled.
///
/// Writes a per-file SHA-256 manifest — `<hash>  <relpath>` lines, sorted by
/// relpath — to `$OUT_DIR/tree_sitter_inputs.stamp`, i.e. right beside the
/// `libtree_sitter_reify.a` this build script just produced. Called only after
/// `cc::Build::compile` returns, and `compile` panics on failure, so a stamp
/// sitting next to an archive ATTESTS that archive was built from these bytes.
///
/// Content identity is the point: `cargo:rerun-if-changed` is an mtime
/// comparison, and warm-lane seeding bulk-stamps sources to 2020-01-01 while the
/// CoW-cloned build outputs carry seed-time (task `#5630`), so "newer than" says
/// nothing useful there. Only these hashes distinguish "built from these bytes"
/// from "merely newer".
///
/// TWO failure shapes, and they get OPPOSITE treatments (`#5629` amendment pass):
///
///   NO HASHER ON THIS HOST (`sha256_of` -> `Ok(None)`) writes the literal
///   `UNAVAILABLE` rather than omitting the stamp. Nothing on this host can ever
///   attest anything, so an ABSENT stamp — which reads as "unproven", i.e. stale —
///   would make `scripts/tree-sitter-freshness.sh ensure` force a rebuild on every
///   single run, forever. `UNAVAILABLE` instead maps to a clean per-dir skip.
///
///   THIS FILE WOULD NOT HASH (`sha256_of` -> `Err(())`) writes NO stamp, and
///   removes any stamp already there. The sentinel must NOT be reachable this way:
///   it permanently disables attestation for this fingerprint dir (cargo never
///   rebuilds a dormant dir, so the stamp is never rewritten) and propagates
///   through CoW lane seeding — a silent, permanent hole from one unreadable file
///   or one fork-pressure spike. An absent stamp is the honest state: UNPROVEN,
///   hence stale, which `ensure` self-heals on the next run. A stale stamp left in
///   place beside a NEWER archive would be worse still — an active mis-attestation.
///   The condition is announced via `cargo:warning=` naming the file, because a
///   silently unattestable archive is exactly what this whole guard exists to
///   prevent. This is the same call the shell half makes: `ts_fingerprint` refuses
///   to emit a partial manifest and hard-fails naming the path.
///
/// A write failure warns but never fails the build.
fn write_inputs_stamp(out_dir: &str) {
    let stamp_path = std::path::Path::new(out_dir).join("tree_sitter_inputs.stamp");

    let mut manifest = String::new();
    let mut no_hasher = false;
    for rel in compilation_inputs() {
        match sha256_of(&rel) {
            Ok(Some(hash)) => manifest.push_str(&format!("{}  {}\n", hash, rel)),
            // Host-wide: no hasher exists, so no later input can fare better.
            Ok(None) => {
                no_hasher = true;
                break;
            }
            // File-scoped: leave the archive UNPROVEN rather than minting the
            // permanent sentinel, and clear any prior stamp so nothing here
            // attests bytes this build did not compile.
            Err(()) => {
                println!(
                    "cargo:warning=tree-sitter-reify: could not hash {} (sha256sum/shasum is \
                     on PATH but failed on it); writing no {} — the archive stays UNPROVEN and \
                     scripts/tree-sitter-freshness.sh ensure will force a rebuild next run",
                    rel,
                    stamp_path.display()
                );
                match std::fs::remove_file(&stamp_path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => eprintln!(
                        "warning: failed to remove stale {}: {}",
                        stamp_path.display(),
                        e
                    ),
                }
                return;
            }
        }
    }

    let content = if no_hasher {
        "UNAVAILABLE\n".to_string()
    } else {
        manifest
    };

    if let Err(e) = std::fs::write(&stamp_path, content) {
        eprintln!("warning: failed to write {}: {}", stamp_path.display(), e);
    }
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

    // Declare every input cargo must watch. Two halves, for two different reasons
    // (`#5629`, esc-5392-1):
    //
    //   src/parser.c IS watched, since `#6992`. This build script WRITES it, and
    //   the old exclusion cited the resulting "double execution" — but that cost
    //   is BOUNDED and CONVERGENT, not a loop: cargo re-runs this script once
    //   because parser.c is newer than its recorded reference, that run finds
    //   both shell stamps current and writes nothing, and the run after it is
    //   clean. One extra `cc::Build::compile` after a grammar change is a build
    //   you were going to pay for anyway. (Pinned by
    //   `test_gating_predicates_converge_after_one_regeneration`.)
    //
    //   What it buys is the reverse direction, which the exclusion left wide
    //   open: cargo narrows a build script's watch set to EXACTLY the emitted
    //   rerun-if-changed list, so an UNWATCHED parser.c could be deleted (the
    //   `git clean -xfd -e target` every lane acquire runs) or CoW-replaced with
    //   a copy from a different base, with grammar.js untouched — and cargo had
    //   no reason to re-run this script at all. The previously-built
    //   libtree_sitter_reify.a stayed linked and the change was never under test.
    //
    //   src/scanner.c and src/tree_sitter/*.h ARE watched. This build script never
    //   writes them, so the double-execution objection does not apply — and before
    //   this change they were watched by NOTHING. `cargo:rerun-if-changed=grammar.js`
    //   was the only directive emitted, and the `cc` crate emits none of its own
    //   (task #5784 verified this against the vendored cc-1.2.62: zero
    //   `rerun-if-changed` occurrences in its sources), and cargo narrows a build
    //   script's watch set to EXACTLY the emitted `rerun-if-changed` list.
    //   The consequence was a false GREEN: an edit confined to src/scanner.c gave
    //   cargo no reason to re-run this script, so cc::Build::compile was never
    //   re-invoked, the previously-built libtree_sitter_reify.a stayed linked, and
    //   the external-scanner change was simply never under test.
    //
    // Task #5784 fixed the scanner.c half of that with a single hardcoded
    // `cargo:rerun-if-changed=src/scanner.c` line. This loop SUBSUMES it: it
    // derives the watch set from `compilation_inputs()` — the same single
    // enumeration the stamp writer uses — so it covers src/scanner.c AND every
    // tracked src/tree_sitter/*.h, and a header added later is watched
    // automatically rather than silently unwatched (which is exactly how this
    // defect class recurs).
    println!("cargo:rerun-if-changed=grammar.js");
    // The shared staleness logic is `include!`d, not a separate crate, so
    // cargo does not learn about it from the module graph — it must be
    // declared here or an edit to the predicates never re-runs them.
    println!("cargo:rerun-if-changed=build_support.rs");
    for rel in compilation_inputs() {
        println!("cargo:rerun-if-changed={}", rel);
    }

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

    if needs_generate(&grammar_hash, &stamp_path, &output_refs, src_dir) {
        // Fast-path: if the shell script already validated the outputs, skip
        // `tree-sitter generate` (which can take >60 s on a loaded build host).
        // This is safe: cargo's `rerun-if-changed=grammar.js` guarantees the
        // build script only re-runs when grammar.js actually changes, so if we
        // land here with a fresh OUT_DIR stamp but a valid shell stamp, the
        // outputs are already current.
        if !shell_stamp_is_current(grammar_path, &output_refs, src_dir) {
            // Hash grammar.js BEFORE generating, exactly as `grammar_hash`
            // above and as `scripts/tree-sitter-generate.sh` do (it captures
            // `GRAMMAR_HASH` before taking the lock and writes it after). The
            // stamp must describe the grammar the generator actually consumed;
            // a hash taken AFTER a >60 s `tree-sitter generate` describes
            // whatever landed in the meantime.
            let grammar_sha_before = sha256_of_path(grammar_path);
            run_tree_sitter_generate();
            // Verify all 3 output files were created.
            verify_outputs(src_dir);
            // Re-attest what was just generated (`#6992`, Hole B). Before this,
            // build.rs could regenerate parser.c and leave
            // `src/.grammar_hash.stamp` describing the PREVIOUS grammar — so a
            // later merge or checkout restoring that grammar made the stamp
            // match again, and it then actively vouched for a parser the current
            // grammar never produced. Whatever regenerates must re-attest.
            //
            // Re-hash and require the two to AGREE (`#6992` amendment pass).
            // `tree-sitter generate` can run for over a minute, and an
            // interactive edit / cargo-watch / a merge landing in that window
            // makes grammar.js(B) the thing we would stamp while parser.c and
            // the outputs manifest describe A. That pair is SELF-CONSISTENT and
            // therefore permanently green: the next build sees the OUT_DIR
            // content hash differ, but `shell_stamp_is_current` then finds
            // sha256(grammar.js) == the grammar stamp AND the manifest matching
            // parser.c, skips generation, and links parser.c(A) against
            // grammar.js(B) forever — the very false GREEN this task removes,
            // through a narrower window. On disagreement write NEITHER stamp:
            // the outputs stay unproven and the next build regenerates.
            match (grammar_sha_before, sha256_of_path(grammar_path)) {
                (Ok(Some(before)), Ok(Some(after))) if before == after => {
                    write_shell_stamps(src_dir, &before)
                }
                (Ok(Some(_)), Ok(Some(_))) => eprintln!(
                    "tree-sitter-reify: {} changed while `tree-sitter generate` \
                     was running; leaving the shell stamps unwritten (the \
                     outputs stay unproven and the next build will regenerate)",
                    grammar_path.display()
                ),
                // No hasher, or a grammar.js that would not hash: write NO
                // stamp rather than a wrong one. The outputs stay UNPROVEN, so
                // the next build regenerates — the safe direction, and the same
                // call `write_inputs_stamp` makes.
                _ => eprintln!(
                    "tree-sitter-reify: could not hash {}; leaving the shell \
                     stamps unwritten (the outputs stay unproven and the next \
                     build will regenerate)",
                    grammar_path.display()
                ),
            }
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

    // compile() panics on failure, so reaching here means libtree_sitter_reify.a
    // was written. Record WHAT it was built from, beside the archive itself.
    write_inputs_stamp(&out_dir);
}
