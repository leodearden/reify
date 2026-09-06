// Shared staleness primitives for the tree-sitter parser pipeline.
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
//     generate` (seconds, already bounded by a 60 s timeout); "up to date"
//     against a parser that does not match the grammar is the silent false
//     GREEN this whole file exists to prevent.

/// The expected output files that tree-sitter generate produces.
#[allow(dead_code)]
const EXPECTED_OUTPUTS: &[&str] = &["parser.c", "grammar.json", "node-types.json"];

/// Filename of the generated-output content manifest, written beside the
/// outputs it describes (i.e. inside `tree-sitter-reify/src/`).
///
/// A SIBLING of `.grammar_hash.stamp`, deliberately not a replacement: three
/// live consumers assert that file is exactly 64 hex characters equal to
/// `sha256(grammar.js)` (`scripts/test_tree_sitter_generate.sh`,
/// `tests/infra/test_verify_semaphore_e2e.sh` twice), so folding the output
/// hashes into it would break all three. This stamp answers the question that
/// one cannot: do the outputs on disk match the outputs that were generated?
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
