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

/// One hashing attempt with one binary.
///
/// Three outcomes, deliberately distinguished — the caller's retry and its
/// `UNAVAILABLE` decision both hinge on telling them apart:
///   `Ok(Some(hash))` hashed;
///   `Ok(None)`       the binary is not on PATH — a permanent fact about this
///                    host, so trying again is pointless;
///   `Err(())`        the binary exists but THIS attempt failed (fork pressure,
///                    EMFILE, a signal) — transient, so worth retrying.
fn try_hasher(bin: &str, args: &[&str], path: &str) -> Result<Option<String>, ()> {
    let output = match std::process::Command::new(bin)
        .args(args)
        .arg(path)
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(o) => o,
        // ENOENT means "no such binary": a permanent property of this host.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    if !output.status.success() {
        return Err(());
    }
    // sha256sum / `shasum -a 256` output format: "<hash>  <filename>\n"
    let stdout = String::from_utf8(output.stdout).map_err(|_| ())?;
    match stdout.split_whitespace().next() {
        Some(h) if !h.is_empty() => Ok(Some(h.to_string())),
        _ => Err(()),
    }
}

/// SHA-256 of a file, via `sha256sum` or `shasum -a 256`.
///
/// THREE outcomes, and the caller depends on telling them apart (`#5629`
/// amendment pass) — collapsing the last two into one `None` is what let a
/// per-file failure mint the permanent `UNAVAILABLE` sentinel:
///   `Ok(Some(hash))` hashed;
///   `Ok(None)`       NO hasher on this host — neither binary is on PATH. A
///                    permanent, host-wide fact, and the ONLY thing
///                    `UNAVAILABLE` is allowed to mean;
///   `Err(())`        a hasher IS on PATH but would not hash THIS file after
///                    the retries (an unreadable mode, or sustained fork/EMFILE
///                    pressure). Scoped to one file, and NOT a statement about
///                    the host — so the caller writes no stamp rather than the
///                    sentinel. This mirrors the shell half exactly:
///                    `ts_hash_file`/`ts_fingerprint` hard-fail naming the file
///                    instead of emitting a degraded manifest.
///
/// TWO hashers, and a bounded retry, for two distinct reasons (`#5629` review):
///
/// 1. The shell side of this contract —
///    `scripts/tree-sitter-freshness.sh` -> `compute_sha256` ->
///    `portable_sha256` in `scripts/lib.sh` — supports BOTH binaries. With
///    `sha256sum` only here, a shasum-only host (macOS is the canonical case)
///    makes the two sides disagree: every stamp says `UNAVAILABLE` while the
///    script computes a real fingerprint, so every archive is permanently
///    unattestable and the guard is silently a no-op for that whole checkout.
///
/// 2. `UNAVAILABLE` must mean "no hasher on this host" and nothing else.
///    Without the retry, one momentary subprocess failure during one build
///    mints the sentinel for a fingerprint dir — and a dir cargo will not
///    rebuild never gets it rewritten, so that one spike disables attestation
///    for that dir indefinitely, then propagates into every lane CoW-seeded
///    from that base.
///
/// The loop exits immediately (no sleeps) when neither binary is on PATH at all.
fn sha256_of(path: &str) -> Result<Option<String>, ()> {
    const HASHERS: [(&str, &[&str]); 2] = [("sha256sum", &[]), ("shasum", &["-a", "256"])];
    const ATTEMPTS: u32 = 3;

    for attempt in 0..ATTEMPTS {
        let mut retryable = false;
        for (bin, args) in HASHERS {
            match try_hasher(bin, args, path) {
                Ok(Some(hash)) => return Ok(Some(hash)),
                Ok(None) => {} // not on PATH — fall through to the next binary
                Err(()) => retryable = true, // present but failed — a retry may win
            }
        }
        // Nothing failed transiently, so nothing can change on a retry: the
        // host simply has no hasher. Return now rather than sleeping twice.
        if !retryable {
            return Ok(None);
        }
        if attempt + 1 < ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(
                100 * u64::from(attempt + 1),
            ));
        }
    }
    // A hasher exists and kept failing on THIS file. Deliberately NOT Ok(None):
    // that would claim a host-wide property from one file's evidence.
    Err(())
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
    //   src/parser.c is deliberately NOT watched. This build script WRITES it, so
    //   watching it would make every run dirty its own watch set — double execution.
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
    for rel in compilation_inputs() {
        if rel == "src/parser.c" {
            continue;
        }
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

    // compile() panics on failure, so reaching here means libtree_sitter_reify.a
    // was written. Record WHAT it was built from, beside the archive itself.
    write_inputs_stamp(&out_dir);
}
