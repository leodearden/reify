// The build-script logic shared with `tests/build_logic_tests.rs` for the
// tree-sitter parser pipeline, which generates into `$OUT_DIR/src` and compiles
// from there: staleness predicates, stamp writers, and the generate step's
// helpers.
//
// `include!`-ed by BOTH `build.rs` and `tests/build_logic_tests.rs`. It is a
// bare item list, not a module: a build script cannot be `use`d by a test
// target, and the historical workaround — hand-copying build.rs's logic into
// the test file — is exactly how `#6992` shipped. A replica can be green while
// the real build script is wrong. Sharing the source removes that gap.
//
// HOUSE RULES for this file:
//   * No `use` items. Both include sites already import from `std`, and a
//     duplicate import there is a hard error (E0252). Everything below is
//     fully qualified.
//   * Every item carries `#[allow(dead_code)]`: build.rs and the test target
//     each exercise a different subset, and the unused half must not warn.
//   * Every predicate fails CLOSED. "Regenerate" costs one `tree-sitter
//     generate` (seconds, bounded by build.rs's timeout); "up to date"
//     against a parser that does not match the grammar is the silent false
//     GREEN this whole file exists to prevent.

/// The grammar-derived outputs of `tree-sitter generate`, content-attested by
/// the outputs manifest.
#[allow(dead_code)]
const EXPECTED_OUTPUTS: &[&str] = &["parser.c", "grammar.json", "node-types.json"];

/// The runtime headers `tree-sitter generate` writes beside its outputs
/// (`src_dir`-relative). Untracked since `#9741`: their bytes vary by CLI
/// version. Existence-checked only, since they do not derive from grammar.js.
#[allow(dead_code)]
const GENERATED_HEADERS: &[&str] = &[
    "tree_sitter/alloc.h",
    "tree_sitter/array.h",
    "tree_sitter/parser.h",
];

/// True when every `GENERATED_HEADERS` entry exists in `src_dir`.
#[allow(dead_code)]
fn generated_headers_present(src_dir: &std::path::Path) -> bool {
    GENERATED_HEADERS
        .iter()
        .all(|rel| src_dir.join(rel).exists())
}

/// Report which `EXPECTED_OUTPUTS`, then which `GENERATED_HEADERS`, are missing
/// from `src_dir` after a generate.
/// The build script panics on `Err`, so the message is what cargo prints.
#[allow(dead_code)]
fn verify_outputs(src_dir: &std::path::Path) -> Result<(), String> {
    let missing: Vec<&str> = EXPECTED_OUTPUTS
        .iter()
        .chain(GENERATED_HEADERS)
        .copied()
        .filter(|name| !src_dir.join(name).exists())
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "tree-sitter generate succeeded but these output files are missing: {}. \
         Check tree-sitter CLI version.",
        missing.join(", ")
    ))
}

/// Run a command with a timeout. Returns Ok(()) on success, Err on failure/timeout.
///
/// Stdio disposition rationale: see test_run_with_timeout_discards_child_stdout
/// and test_run_with_timeout_inherits_child_stderr in tests/build_logic_tests.rs.
#[allow(dead_code)]
fn run_with_timeout(
    cmd: impl AsRef<std::ffi::OsStr>,
    args: &[&str],
    timeout_secs: u64,
) -> Result<(), String> {
    let cmd = cmd.as_ref();
    let mut child = std::process::Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Failed to spawn '{}': {}", cmd.to_string_lossy(), e))?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                } else {
                    return Err(format!(
                        "'{}' failed with exit code {}",
                        cmd.to_string_lossy(),
                        status.code().unwrap_or(-1)
                    ));
                }
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait(); // Reap the process.
                    return Err(format!(
                        "'{}' timed out after {}s",
                        cmd.to_string_lossy(),
                        timeout_secs
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait(); // Reap the process to prevent orphans.
                return Err(format!(
                    "Error waiting for '{}': {}",
                    cmd.to_string_lossy(),
                    e
                ));
            }
        }
    }
}

/// Filename of the generated-output content manifest, written beside the
/// outputs it describes. It has two writers in one format: build.rs writes it
/// into `$OUT_DIR/src`, and `scripts/tree-sitter-generate.sh` into
/// `tree-sitter-reify/src` for the CLI consumers.
/// `tests/infra/ts_outputs_manifest_lib.sh` checks both.
#[allow(dead_code)]
const OUTPUTS_STAMP_NAME: &str = ".generated_outputs.stamp";

/// One hashing attempt with one binary.
///
/// Three outcomes, deliberately distinguished — the caller's retry and its
/// `UNAVAILABLE` decision both hinge on telling them apart:
///   `Ok(Some(hash))` hashed;
///   `Ok(None)`       the binary is not on PATH — a permanent fact about this
///                    host, so trying again is pointless;
///   `Err(())`        the binary exists but THIS attempt failed (fork pressure,
///                    EMFILE, a signal) — transient, so worth retrying.
#[allow(dead_code)]
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
#[allow(dead_code)]
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

/// `sha256_of` for a `Path`, preserving its exact three-outcome contract.
///
/// A non-UTF-8 path maps to `Err(())` — "this file would not hash" — rather
/// than to `Ok(None)`, which is reserved for the host-wide no-hasher fact. A
/// lossy conversion is deliberately NOT used: it would hash a DIFFERENT path
/// than the caller asked about, and silently attest the wrong bytes.
#[allow(dead_code)]
fn sha256_of_path(path: &std::path::Path) -> Result<Option<String>, ()> {
    match path.to_str() {
        Some(s) => sha256_of(s),
        None => Err(()),
    }
}

/// Render a generated-output manifest: `<hash>  <relpath>\n` lines, sorted by
/// relpath.
///
/// Byte-identical in FORMAT to what `write_inputs_stamp` emits into
/// `$OUT_DIR/tree_sitter_inputs.stamp` and to `ts_fingerprint` in
/// `scripts/tree-sitter-freshness.sh`, so both existing parsers read it
/// unmodified and neither side can invent a second grammar.
///
/// The ANCHOR differs, and deliberately: relpaths here are relative to the
/// `src/` directory the stamp lives in (`parser.c`, not `src/parser.c`), so the
/// manifest names exactly `EXPECTED_OUTPUTS` and can be verified without
/// knowing where the package root is.
#[allow(dead_code)]
fn outputs_manifest_render(entries: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.1.cmp(&b.1));
    let mut out = String::new();
    for (hash, rel) in sorted {
        out.push_str(hash);
        out.push_str("  ");
        out.push_str(rel);
        out.push('\n');
    }
    out
}

/// Parse a manifest back into `(hash, relpath)` pairs, or `None` if ANY line is
/// malformed.
///
/// All-or-nothing on purpose. A truncated write (killed generate, ENOSPC) is
/// the failure this guards, and a parser that skipped the bad line would hand
/// back a SHORTER manifest that then sails through a per-entry hash loop — the
/// degraded-manifest failure `ts_hash_file` documents on the shell side.
#[allow(dead_code)]
fn outputs_manifest_parse(text: &str) -> Option<Vec<(String, String)>> {
    let mut entries = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            return None;
        }
        let (hash, rel) = line.split_once("  ")?;
        if hash.is_empty() || rel.is_empty() {
            return None;
        }
        // A hash column is hex and nothing else; a relpath column is a single
        // path component-ish string. Whitespace in either means the two-space
        // separator matched somewhere it should not have.
        if hash.chars().any(|c| c.is_whitespace()) || rel.chars().any(|c| c.is_whitespace()) {
            return None;
        }
        entries.push((hash.to_string(), rel.to_string()));
    }
    if entries.is_empty() {
        return None;
    }
    Some(entries)
}

/// Hash every `EXPECTED_OUTPUTS` file in `src_dir` and render the manifest.
///
/// Propagates `sha256_of`'s three outcomes unchanged: `Ok(None)` means this
/// host cannot hash at all, `Err(())` means one named output would not hash.
/// Callers write NO stamp in either case rather than a partial one.
#[allow(dead_code)]
fn outputs_manifest_for(src_dir: &std::path::Path) -> Result<Option<String>, ()> {
    let mut entries: Vec<(String, String)> = Vec::new();
    for name in EXPECTED_OUTPUTS {
        match sha256_of_path(&src_dir.join(name)) {
            Ok(Some(hash)) => entries.push((hash, (*name).to_string())),
            Ok(None) => return Ok(None),
            Err(()) => return Err(()),
        }
    }
    Ok(Some(outputs_manifest_render(&entries)))
}

/// Does the manifest at `manifest_path` describe the bytes now in `src_dir`?
///
/// TRUE only when the manifest parses, names EXACTLY `EXPECTED_OUTPUTS`, and
/// every recorded hash equals the file's hash recomputed from disk. Every other
/// outcome — unreadable file, unparseable line, set mismatch, a file that will
/// not hash, a host with no hasher — is FALSE.
///
/// FAIL CLOSED, and note which direction that is: FALSE means "regenerate",
/// which costs seconds. TRUE is the only answer that can link a parser the
/// grammar never produced.
#[allow(dead_code)]
fn outputs_manifest_matches(manifest_path: &std::path::Path, src_dir: &std::path::Path) -> bool {
    let text = match std::fs::read_to_string(manifest_path) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let entries = match outputs_manifest_parse(&text) {
        Some(e) => e,
        None => return false,
    };
    // Set equality against EXPECTED_OUTPUTS, both directions. A manifest that
    // omits one output attests nothing about it; one that names an extra file
    // describes a different output set than this build script produces.
    if entries.len() != EXPECTED_OUTPUTS.len() {
        return false;
    }
    for (_, rel) in &entries {
        if !EXPECTED_OUTPUTS.contains(&rel.as_str()) {
            return false;
        }
    }
    for name in EXPECTED_OUTPUTS {
        if !entries.iter().any(|(_, rel)| rel == name) {
            return false;
        }
    }
    for (recorded, rel) in &entries {
        match sha256_of_path(&src_dir.join(rel)) {
            Ok(Some(actual)) if &actual == recorded => {}
            _ => return false,
        }
    }
    true
}

/// Write `contents` to `path` atomically: a sibling temp file, then a rename.
///
/// The temp name carries this process's pid so two concurrent writers cannot
/// collide on it. A failed rename removes the temp file.
///
/// Atomicity matters because these stamps are read by a DIFFERENT process than
/// the one writing them (`build.rs` writes, `scripts/tree-sitter-freshness.sh`
/// and the next build script read): a partially-written manifest is exactly the
/// truncated shape `outputs_manifest_parse` refuses, and a reader that caught it
/// mid-write would force a needless regeneration at best.
#[allow(dead_code)]
fn write_atomic(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    let file_name = match path.file_name() {
        Some(n) => n.to_string_lossy().to_string(),
        None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "stamp path has no file name",
            ));
        }
    };
    let tmp = path.with_file_name(format!(".tmp-{}-{}", std::process::id(), file_name));
    std::fs::write(&tmp, contents)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// The directory, relative to `$OUT_DIR`, that build.rs generates into and
/// compiles from.
#[allow(dead_code)]
const GENERATED_DIR: &str = "src";

/// `$OUT_DIR`'s record of the `generation_key` that `GENERATED_DIR` was
/// generated under.
#[allow(dead_code)]
const KEY_STAMP_NAME: &str = "generation_key.stamp";

/// `$OUT_DIR`'s attestation of the bytes `libtree_sitter_reify.a` was compiled
/// from, read by `scripts/tree-sitter-freshness.sh`.
#[allow(dead_code)]
const INPUTS_STAMP_NAME: &str = "tree_sitter_inputs.stamp";

/// The tracked files whose bytes determine the archive, package-root-relative
/// and sorted. build.rs watches exactly these (plus build_support.rs and the
/// resolved CLI), and
/// `scripts/tree-sitter-freshness.sh --list-inputs` prints exactly these.
#[allow(dead_code)]
const ATTESTED_INPUTS: [&str; 2] = ["grammar.js", "src/scanner.c"];

/// The newest parser ABI the workspace's `tree-sitter = "0.24"` runtime loads
/// (issue #1). Held by tests/infra/test_tree_sitter_abi_pin.py; equal to the
/// `--abi` in scripts/tree-sitter-generate.sh.
#[allow(dead_code)]
const GENERATED_PARSER_ABI: &str = "14";

/// Whether `parser` declares `#define LANGUAGE_VERSION <GENERATED_PARSER_ABI>`.
///
/// The stamps attest grammar.js and the generated bytes, never the flags the
/// generator ran with. So a parser.c generated before the pin (issue #1) sits
/// behind a fully consistent stamp pair, and only this check sees that the
/// runtime cannot load it. An absent or unreadable parser.c reads as unpinned,
/// which regenerates.
#[allow(dead_code)]
fn parser_has_pinned_abi(parser: &std::path::Path) -> bool {
    let Ok(file) = std::fs::File::open(parser) else {
        return false;
    };
    std::io::BufRead::lines(std::io::BufReader::new(file))
        .map_while(Result::ok)
        .find_map(|line| {
            line.strip_prefix("#define LANGUAGE_VERSION ")
                .map(|abi| abi.trim() == GENERATED_PARSER_ABI)
        })
        .unwrap_or(false)
}

/// The key a generation is valid under: grammar.js's sha256 and the CLI's
/// `--version` line, rendered `<sha>\n<cli version>\n`.
#[allow(dead_code)]
fn generation_key(grammar_sha: &str, cli_version: &str) -> String {
    format!("{grammar_sha}\n{cli_version}\n")
}

/// The grammar sha256 the key stamp at `key_stamp` names, or `None` when the
/// stamp is absent or its first line is not 64 lowercase hex.
#[allow(dead_code)]
fn attested_grammar_sha(key_stamp: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(key_stamp).ok()?;
    let sha = text.lines().next()?;
    let is_sha = sha.len() == 64 && sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    is_sha.then(|| sha.to_string())
}

/// Whether `generated_dir` must be regenerated before it is compiled.
///
/// FALSE only when the key is known, every output and header is present,
/// parser.c is at the pinned ABI, the key stamp holds this key, and the outputs
/// manifest matches the bytes on disk. That is the one definition of "this
/// directory holds what this build script would generate".
#[allow(dead_code)]
fn needs_generate(
    key: Option<&str>,
    key_stamp: &std::path::Path,
    generated_dir: &std::path::Path,
) -> bool {
    let Some(key) = key else {
        return true;
    };
    if EXPECTED_OUTPUTS
        .iter()
        .any(|name| !generated_dir.join(name).exists())
        || !generated_headers_present(generated_dir)
        || !parser_has_pinned_abi(&generated_dir.join("parser.c"))
    {
        return true;
    }
    match std::fs::read_to_string(key_stamp) {
        Ok(recorded) if recorded.trim() == key.trim() => {}
        _ => return true,
    }
    !outputs_manifest_matches(&generated_dir.join(OUTPUTS_STAMP_NAME), generated_dir)
}

/// Attest a freshly generated `generated_dir`: the outputs manifest beside the
/// outputs, then `key` into `key_stamp`.
///
/// The key stamp is written LAST. A crash between the two leaves a manifest
/// with no key stamp, which `needs_generate` reads as stale; the reverse order
/// would leave a key vouching for outputs nothing has hashed.
///
/// Never fatal: a stamp that cannot be written costs one regeneration on the
/// next run. If the manifest cannot be rendered, neither stamp is written.
#[allow(dead_code)]
fn write_generation_stamps(
    generated_dir: &std::path::Path,
    key_stamp: &std::path::Path,
    key: &str,
) {
    let Ok(Some(manifest)) = outputs_manifest_for(generated_dir) else {
        eprintln!(
            "tree-sitter-reify: could not hash the generated outputs in {}; writing no \
             generation stamps (the next run regenerates)",
            generated_dir.display()
        );
        return;
    };
    let manifest_path = generated_dir.join(OUTPUTS_STAMP_NAME);
    if let Err(e) = write_atomic(&manifest_path, &manifest) {
        eprintln!(
            "warning: failed to write {}: {}",
            manifest_path.display(),
            e
        );
        return;
    }
    if let Err(e) = write_atomic(key_stamp, key) {
        eprintln!("warning: failed to write {}: {}", key_stamp.display(), e);
    }
}

/// Remove `path`, treating an already-absent file as success.
#[allow(dead_code)]
fn remove_if_present(path: &std::path::Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => eprintln!("warning: failed to remove {}: {}", path.display(), e),
    }
}

/// Attest what was just compiled, in `out_dir/INPUTS_STAMP_NAME` beside the
/// archive: `<hash>  <relpath>` lines for `ATTESTED_INPUTS`.
///
/// grammar.js's hash comes from the key stamp, so the stamp names the grammar
/// that produced the compiled parser.c, not whatever grammar.js holds now.
/// scanner.c's comes from `scanner_copy`, the bytes actually compiled.
///
/// Content identity is the point: `cargo:rerun-if-changed` compares mtimes, and
/// warm-lane seeding rewinds source mtimes while CoW-cloning build outputs
/// (task `#5630`), so only these hashes tell "built from these bytes" from
/// "merely newer".
///
/// TWO failure shapes, with OPPOSITE treatments (`#5629`):
///
///   NO HASHER ON THIS HOST (`Ok(None)`) writes the literal `UNAVAILABLE`. An
///   absent stamp reads as stale, so on a host that can never attest anything
///   `scripts/tree-sitter-freshness.sh ensure` would force a rebuild on every
///   run; `UNAVAILABLE` maps to a clean per-dir skip instead.
///
///   UNPROVEN SOURCES (the scanner copy would not hash, or no key stamp names a
///   grammar) write NO stamp and remove any already there, announced via
///   `cargo:warning=`. The sentinel must not be reachable this way: a dormant
///   fingerprint dir never has its stamp rewritten, so it would disable
///   attestation for that dir permanently, and CoW seeding would spread it. An
///   absent stamp is the honest state, stale, which `ensure` self-heals.
///
/// A write failure warns but never fails the build.
#[allow(dead_code)]
fn write_inputs_stamp(
    out_dir: &std::path::Path,
    key_stamp: &std::path::Path,
    scanner_copy: &std::path::Path,
) {
    let stamp_path = out_dir.join(INPUTS_STAMP_NAME);
    let [grammar_rel, scanner_rel] = ATTESTED_INPUTS;
    let unproven = |why: String| {
        println!(
            "cargo:warning=tree-sitter-reify: {why}; writing no {} (the archive stays UNPROVEN \
             and scripts/tree-sitter-freshness.sh ensure will force a rebuild next run)",
            stamp_path.display()
        );
        remove_if_present(&stamp_path);
    };
    let content = match sha256_of_path(scanner_copy) {
        Ok(None) => "UNAVAILABLE\n".to_string(),
        Err(()) => {
            return unproven(format!(
                "could not hash {} (sha256sum/shasum is on PATH but failed on it)",
                scanner_copy.display()
            ));
        }
        Ok(Some(scanner_sha)) => match attested_grammar_sha(key_stamp) {
            Some(grammar_sha) => outputs_manifest_render(&[
                (grammar_sha, grammar_rel.to_string()),
                (scanner_sha, scanner_rel.to_string()),
            ]),
            None => {
                return unproven(format!(
                    "no key stamp at {} names the grammar the parser was generated from",
                    key_stamp.display()
                ));
            }
        },
    };
    if let Err(e) = write_atomic(&stamp_path, &content) {
        eprintln!("warning: failed to write {}: {}", stamp_path.display(), e);
    }
}

/// The first `name` on `path_var` that is a file with an exec bit set (on
/// Unix; any file elsewhere). Empty PATH elements are skipped.
#[allow(dead_code)]
fn find_on_path(name: &str, path_var: Option<&std::ffi::OsStr>) -> Option<std::path::PathBuf> {
    std::env::split_paths(path_var?)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable_file(candidate))
}

/// Whether `path` is a file with an exec bit set (on Unix; any file elsewhere).
#[allow(dead_code)]
fn is_executable_file(path: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        meta.is_file() && std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// The trimmed stdout of `<cli> --version`, or `None` if it does not run,
/// fails, or prints nothing.
#[allow(dead_code)]
fn cli_version(cli: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new(cli)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!version.is_empty()).then_some(version)
}
