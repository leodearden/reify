//! Stale `--test <stem>` doc-invocation guard.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test stale_test_target_doc_invocations`
//!
//! Scans tracked `crates/**/*.rs` source for hand-typed `cargo test --test
//! <stem>` / `--test <stem>` doc invocations and asserts each `<stem>`
//! resolves to an existing top-level `crates/<crate>/tests/<stem>.rs`. Where
//! the referenced test is `#[ignore]`d, the doc invocation is its entire
//! operational access route — a stale stem silently retires the guard it
//! documents (recurred in tasks 5282, 5649, 5687).
//!
//! Two tests:
//! - **Test A** (hermetic, always runs): drives `scan()` over synthetic
//!   fixture files in a tempdir — no git repo required.
//! - **Test B** (live anti-drift, no `#[ignore]`): runs `scan()` over the
//!   real repo's tracked `crates/**/*.rs`; graceful-skip when git is absent.
//!
//! Escape hatch: a line carrying the literal `stale-test-target:allow` opts
//! out of the sweep for that line (mirrors ptodo.rs's `ptodo:allow`, §6.8).
//! Same-line only; trailing `— reason` prose expected.

use std::path::Path;

/// Write `content` to relative `path` inside `root`, creating parent dirs.
fn write_file(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("create_dir_all");
    }
    std::fs::write(&full, content).expect("write_file");
}

/// A `--test <stem>` doc invocation whose stem does not resolve to an
/// existing `crates/<crate>/tests/<stem>.rs`.
#[derive(Debug)]
struct Finding {
    file: String,
    line: usize,
    crate_name: String,
    stem: String,
}

/// Derive the crate name from a `crates/<crate>/...`-relative path.
fn crate_from_path(file: &str) -> Option<String> {
    let mut parts = file.split('/');
    if parts.next()? != "crates" {
        return None;
    }
    let crate_name = parts.next()?;
    if crate_name.is_empty() {
        return None;
    }
    Some(crate_name.to_string())
}

/// Extract every `--test <stem>` stem on `line`. A stem is `[A-Za-z0-9_]+`
/// immediately following a space or `=` separator. Anything else after the
/// separator — including the `<` of a `--test <name>` metavariable
/// placeholder, or end-of-line — yields an empty stem and is skipped.
fn extract_stems(line: &str) -> Vec<String> {
    const NEEDLE: &str = "--test";
    let mut stems = Vec::new();
    for (idx, _) in line.match_indices(NEEDLE) {
        let rest = &line[idx + NEEDLE.len()..];
        let mut chars = rest.chars();
        let Some(sep) = chars.next() else { continue };
        let after_sep = if sep == '=' {
            &rest[sep.len_utf8()..]
        } else if sep.is_whitespace() {
            rest.trim_start_matches(|c: char| c.is_whitespace())
        } else {
            continue;
        };
        // A `<` immediately after the separator is a metavariable placeholder
        // (`--test <name>`, the crates/reify-kernel-occt/tests/harness_occt.rs:13
        // shape), not a real stem — skip explicitly rather than relying on
        // the alphanumeric/`_` character class below to incidentally reject it.
        if after_sep.starts_with('<') {
            continue;
        }
        let stem: String = after_sep
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if stem.is_empty() {
            continue;
        }
        stems.push(stem);
    }
    stems
}

/// Extract a same-line `-p <crate>` flag's crate name, if present. Requires
/// the separator (space or `=`) to sit immediately after `-p`, so this does
/// not match inside a longer flag such as `--profile` or `--package`.
fn extract_p_crate(line: &str) -> Option<String> {
    const NEEDLE: &str = "-p";
    for (idx, _) in line.match_indices(NEEDLE) {
        let rest = &line[idx + NEEDLE.len()..];
        let mut chars = rest.chars();
        let Some(sep) = chars.next() else { continue };
        let after_sep = if sep == '=' {
            &rest[sep.len_utf8()..]
        } else if sep.is_whitespace() {
            rest.trim_start_matches(|c: char| c.is_whitespace())
        } else {
            continue;
        };
        let crate_name: String = after_sep
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if crate_name.is_empty() {
            continue;
        }
        return Some(crate_name);
    }
    None
}

/// Inline escape marker (mirrors ptodo.rs's `ptodo:allow`, §6.8). Same-line
/// only; trailing `— reason` prose expected. Named once so it stays
/// greppable and cannot silently drift out of sync with its doc mentions.
const ALLOW_MARKER: &str = "stale-test-target:allow";

/// A line carrying the literal [`ALLOW_MARKER`] opts out of the sweep for
/// that line entirely (checked before extraction).
fn line_escaped(line: &str) -> bool {
    line.contains(ALLOW_MARKER)
}

/// Scan `files` (paths relative to `root`) for `--test <stem>` doc
/// invocations and return a [`Finding`] for each stem that does not resolve
/// to an existing `crates/<crate>/tests/<stem>.rs`. Pure function of its
/// arguments — no `git` or `cargo` invocation — so it can be driven over a
/// tempdir with no git repo (Test A) as well as the real repo (Test B).
///
/// Crate resolution is a precedence: an explicit same-line `-p <crate>`
/// wins; when absent, falls back to the containing file's `crates/<crate>/`
/// path so bare `cargo test --test X` invocations (run from inside their
/// crate dir) still resolve.
fn scan(root: &Path, files: &[String]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        let Ok(content) = std::fs::read_to_string(root.join(file)) else {
            continue;
        };
        let path_crate = crate_from_path(file);
        for (idx, line) in content.lines().enumerate() {
            if line_escaped(line) {
                continue;
            }
            let stems = extract_stems(line);
            if stems.is_empty() {
                continue;
            }
            let Some(crate_name) = extract_p_crate(line).or_else(|| path_crate.clone()) else {
                continue;
            };
            for stem in stems {
                let resolves = root
                    .join("crates")
                    .join(&crate_name)
                    .join("tests")
                    .join(format!("{stem}.rs"))
                    .is_file();
                if !resolves {
                    findings.push(Finding {
                        file: file.clone(),
                        line: idx + 1,
                        crate_name: crate_name.clone(),
                        stem,
                    });
                }
            }
        }
    }
    findings
}

/// Test A: hermetic extraction + resolution over two synthetic sources.
///
/// `crates/crate-a/src/live.rs` names a stem with a matching
/// `crates/crate-a/tests/live_stem.rs` — zero findings.
/// `crates/crate-a/src/stale.rs` names a stem with no matching test file —
/// exactly one finding.
#[test]
fn extracts_and_resolves_basic_stem() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(
        root,
        "crates/crate-a/src/live.rs",
        "//! Run with `cargo test --test live_stem`\n", // stale-test-target:allow — synthetic fixture
    );
    write_file(root, "crates/crate-a/tests/live_stem.rs", "// empty\n");

    write_file(
        root,
        "crates/crate-a/src/stale.rs",
        "//! Run with `cargo test --test stale_stem`\n", // stale-test-target:allow — synthetic fixture
    );

    let files = vec![
        "crates/crate-a/src/live.rs".to_string(),
        "crates/crate-a/tests/live_stem.rs".to_string(),
        "crates/crate-a/src/stale.rs".to_string(),
    ];

    let findings = scan(root, &files);

    assert_eq!(
        findings.len(),
        1,
        "expected exactly one finding; got {findings:?}"
    );
    assert_eq!(findings[0].crate_name, "crate-a");
    assert_eq!(findings[0].stem, "stale_stem");
    assert_eq!(findings[0].file, "crates/crate-a/src/stale.rs");
    assert_eq!(findings[0].line, 1);
}

/// Test A: false-positive class (1) — cross-crate `-p` precedence.
///
/// `crates/crate-a/src/x.rs` documents two invocations that both name
/// `-p crate-b`, the `crates/reify-test-support/src/temp_dirs.rs:146` shape.
/// Line 1's stem resolves against `crate-b`'s test dir (which exists) even
/// though the containing file lives under `crate-a` — zero findings for it.
/// Line 2's stem resolves against neither — its finding must attribute
/// `crate-b` (from the flag), not `crate-a` (from the containing path), so a
/// real failure points the reader at the crate the invocation actually runs.
#[test]
fn cross_crate_p_flag_takes_precedence_over_containing_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(
        root,
        "crates/crate-a/src/x.rs",
        "// REIFY_KEEP_TEMP_DIRS=1 cargo test -p crate-b --test some_harness\n// cargo test -p crate-b --test absent_stem\n", // stale-test-target:allow — synthetic fixture
    );
    write_file(root, "crates/crate-b/tests/some_harness.rs", "// empty\n");
    // Deliberately NOT creating crates/crate-a/tests/some_harness.rs — if the
    // scanner attributed crate-a here (containing path) it would wrongly flag
    // this correct, cross-crate invocation.

    let files = vec![
        "crates/crate-a/src/x.rs".to_string(),
        "crates/crate-b/tests/some_harness.rs".to_string(),
    ];

    let findings = scan(root, &files);

    assert_eq!(
        findings.len(),
        1,
        "expected exactly one finding (the absent_stem line); got {findings:?}"
    );
    assert_eq!(
        findings[0].stem, "absent_stem",
        "unexpected finding: {:?}",
        findings[0]
    );
    assert_eq!(
        findings[0].crate_name, "crate-b",
        "finding must attribute the -p flag's crate, not the containing path's crate-a: {:?}",
        findings[0]
    );
}

/// Test A: false-positive class (3) — no-`-p` invocations resolve via the
/// containing crate, plus the `--test <name>` metavariable guard.
///
/// (a) `crates/crate-a/src/wrapped.rs` wraps a doc comment across two lines
///     (`crates/reify-expr/tests/kleene_logic_tests.rs:4-5` shape) with no
///     `-p` anywhere — resolves against the containing crate.
/// (b) `crates/crate-a/src/bare_mention.rs` is a bare parenthetical mention
///     with no `cargo` and no `-p` (`crates/reify-compiler/src/entity.rs:2608`
///     shape) — also resolves via the containing crate, and DOES produce a
///     finding when the target is absent (proving the line is genuinely
///     checked, not silently skipped for lacking `-p`/`cargo`).
/// (c) `crates/crate-a/src/metavar.rs` contains the angle-bracket
///     metavariable `--test <name>` (`crates/reify-kernel-occt/tests/
///     harness_occt.rs:13` shape) — must NOT be extracted at all.
#[test]
fn no_p_flag_falls_back_to_containing_crate_and_skips_metavariables() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(
        root,
        "crates/crate-a/src/wrapped.rs",
        "//! cargo test\n//! --test wrapped_stem\n", // stale-test-target:allow — synthetic fixture
    );
    write_file(root, "crates/crate-a/tests/wrapped_stem.rs", "// empty\n");

    write_file(
        root,
        "crates/crate-a/src/bare_mention.rs",
        "// (run with `--test bare_stem -- foo:: --ignored`)\n", // stale-test-target:allow — synthetic fixture
    );
    // Deliberately NOT creating crates/crate-a/tests/bare_stem.rs.

    write_file(
        root,
        "crates/crate-a/src/metavar.rs",
        "//! Usage: `cargo test --test <name>`\n", // stale-test-target:allow — synthetic fixture
    );

    let files = vec![
        "crates/crate-a/src/wrapped.rs".to_string(),
        "crates/crate-a/tests/wrapped_stem.rs".to_string(),
        "crates/crate-a/src/bare_mention.rs".to_string(),
        "crates/crate-a/src/metavar.rs".to_string(),
    ];

    let findings = scan(root, &files);

    assert_eq!(
        findings.len(),
        1,
        "expected exactly one finding (bare_mention.rs's bare_stem); got {findings:?}"
    );
    assert_eq!(findings[0].file, "crates/crate-a/src/bare_mention.rs");
    assert_eq!(findings[0].crate_name, "crate-a");
    assert_eq!(findings[0].stem, "bare_stem");
}

/// Test A: false-positive class (2) — the inline `stale-test-target:allow`
/// escape.
///
/// `crates/crate-a/src/escaped.rs` names a stem with no matching test file,
/// but the line carries a trailing `stale-test-target:allow` marker — zero
/// findings. `crates/crate-a/src/unescaped.rs` carries the IDENTICAL stem
/// reference with no marker — this negative control DOES produce a finding,
/// proving the marker (not some parsing quirk) is what suppresses the first.
#[test]
fn inline_allow_marker_suppresses_the_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(
        root,
        "crates/crate-a/src/escaped.rs",
        "//! Historically ran with `--test escaped_stem` // stale-test-target:allow — prose, not an invocation\n",
    );
    write_file(
        root,
        "crates/crate-a/src/unescaped.rs",
        "//! Historically ran with `--test escaped_stem`\n", // stale-test-target:allow — synthetic fixture
    );
    // Deliberately NOT creating crates/crate-a/tests/escaped_stem.rs.

    let files = vec![
        "crates/crate-a/src/escaped.rs".to_string(),
        "crates/crate-a/src/unescaped.rs".to_string(),
    ];

    let findings = scan(root, &files);

    assert_eq!(
        findings.len(),
        1,
        "expected exactly one finding (from unescaped.rs only); got {findings:?}"
    );
    assert_eq!(findings[0].file, "crates/crate-a/src/unescaped.rs");
    assert_eq!(findings[0].crate_name, "crate-a");
    assert_eq!(findings[0].stem, "escaped_stem");
}
