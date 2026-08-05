//! Citation-hygiene guard — no `.rs`/`.ri` comment in `crates/reify-compiler/stdlib/*.ri`
//! may cite another source file by hard line number.
//!
//! Motivation: a cite of the form `foo.rs:472` is correct only until the next
//! edit to `foo.rs`, after which it silently points at unrelated code.  This
//! defect class has been swept one-file-at-a-time three times (#5837, #5854,
//! #5856); measured at #5856's start, 8 of 10 sampled cites were already stale,
//! one by 71 lines.  The fix convention — established on main by #5837/#5854 —
//! is to cite by **symbol name or stable anchor**, keeping the crate-relative
//! file path and dropping only the `:<line>` suffix.  See ports_thermal.ri,
//! ports_fluid.ri, and ports_mechanical.ri for live exemplars.
//!
//! `PENDING_SWEEP` is a **shrinking ratchet**, not a permanent allowlist: it
//! names the files #5856 has not swept yet, and is emptied and deleted by that
//! task's final step.  Nothing may ever be added back to it.

use std::path::{Path, PathBuf};

/// Absolute path to this crate's `stdlib/` directory, resolved at compile time
/// from the manifest directory.
const STDLIB_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/stdlib");

/// Files not yet swept by #5856.  Each entry is `(basename, reason)`; the
/// reason is mandatory — the `(&str, &str)` tuple shape (copied from
/// `examples_smoke.rs::SKIP_SET`) forces every entry to carry a one-line
/// human-readable justification, making exemptions auditable at review time.
///
/// This list only ever shrinks.  Each #5856 batch deletes its entries (RED),
/// then sweeps those files (GREEN), so every intermediate commit stays green
/// and each batch is independently bisectable and revertable.
const PENDING_SWEEP: &[(&str, &str)] = &[
    ("dynamics.ri", "pending #5856 batch B sweep"),
    ("fields.ri", "pending #5856 batch B sweep"),
    ("solver_elastic.ri", "pending #5856 batch C sweep"),
    ("fea_multi_case.ri", "pending #5856 batch C sweep"),
    ("solver_buckling.ri", "pending #5856 batch C sweep"),
    ("solver_buckling_fns.ri", "pending #5856 batch C sweep"),
    ("trajectory.ri", "pending #5856 batch D sweep"),
    ("trajectory_fns.ri", "pending #5856 batch D sweep"),
    ("kinematic.ri", "pending #5856 batch D sweep"),
    ("tensegrity.ri", "pending #5856 batch D sweep"),
    ("flexures.ri", "pending #5856 batch D sweep"),
    ("modal_analysis.ri", "pending #5856 batch E sweep"),
    ("modal_analysis_fns.ri", "pending #5856 batch F sweep"),
    ("modal_mechanism_fns.ri", "pending #5856 batch F sweep"),
];

/// Source-file extensions whose `:<line>` cites are forbidden.
const CITED_EXTENSIONS: &[&str] = &[".rs:", ".ri:"];

/// Return every 1-indexed line of `src` that carries a hard source-line
/// citation, paired with the trimmed line text.
///
/// A citation is the literal `.rs:` or `.ri:` immediately followed by an ASCII
/// digit.  Hand-rolled rather than regex-based: reify-compiler has no `regex`
/// dev-dependency and the check is a dozen lines of `str` scanning.  The
/// pattern was validated against the full 97-occurrence corpus and matches
/// every observed form — plain (`:472`), range (`:106-114`), slash
/// (`:535/576`), comma (`:261,319`), and open-ended (`:132+`) — with zero
/// false positives (no `.md:`/`.toml:`/`.txt:`/`.sh:` line cites exist in
/// stdlib, and no version or timestamp string collides).
///
/// A line with several citations is reported once; the whole line is the
/// evidence a reader needs.
fn find_hard_cites(src: &str) -> Vec<(usize, String)> {
    let mut hits = Vec::new();
    for (idx, line) in src.lines().enumerate() {
        if line_has_hard_cite(line) {
            hits.push((idx + 1, line.trim().to_string()));
        }
    }
    hits
}

/// True when `line` contains `.rs:` or `.ri:` immediately followed by a digit.
fn line_has_hard_cite(line: &str) -> bool {
    for marker in CITED_EXTENSIONS {
        let mut rest = line;
        while let Some(pos) = rest.find(marker) {
            let after = &rest[pos + marker.len()..];
            if after.starts_with(|c: char| c.is_ascii_digit()) {
                return true;
            }
            rest = &rest[pos + marker.len()..];
        }
    }
    false
}

/// Return all `*.ri` files directly under `STDLIB_DIR`, sorted by path for
/// deterministic output.  The stdlib is a flat directory — no recursion.
fn discover_stdlib_ri_files() -> Vec<PathBuf> {
    let dir = Path::new(STDLIB_DIR);
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "stdlib_citation_hygiene: cannot read directory '{}': {}",
            dir.display(),
            e
        )
    });
    let mut paths: Vec<PathBuf> = entries
        .map(|e| e.expect("IO error reading stdlib dir entry").path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ri"))
        .collect();
    paths.sort();
    paths
}

/// Basename of `path` as a `&str`, for matching against `PENDING_SWEEP`.
fn basename(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn stdlib_ri_comments_cite_symbols_not_source_lines() {
    let mut violations: Vec<String> = Vec::new();

    for path in discover_stdlib_ri_files() {
        let name = basename(&path);
        if PENDING_SWEEP.iter().any(|(f, _)| *f == name) {
            continue;
        }
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read '{}': {}", path.display(), e));
        for (line_no, text) in find_hard_cites(&src) {
            violations.push(format!("  {name}:{line_no}  {text}"));
        }
    }

    assert!(
        violations.is_empty(),
        "{} stdlib comment line(s) cite a source file by hard line number:\n{}\n\n\
         Line numbers go stale on the next edit to the cited file.  Cite by symbol \
         name or stable anchor instead, keeping the crate-relative path and dropping \
         only the `:<line>` suffix — see ports_thermal.ri, ports_fluid.ri, and \
         ports_mechanical.ri for the established convention (#5837, #5854, #5856).",
        violations.len(),
        violations.join("\n"),
    );
}

#[test]
fn pending_sweep_entries_all_name_a_real_stdlib_file() {
    let present: Vec<String> = discover_stdlib_ri_files()
        .iter()
        .map(|p| basename(p))
        .collect();

    let stale: Vec<&str> = PENDING_SWEEP
        .iter()
        .map(|(f, _)| *f)
        .filter(|f| !present.iter().any(|p| p == f))
        .collect();

    assert!(
        stale.is_empty(),
        "PENDING_SWEEP names {} file(s) absent from {}: {:?}\n\
         A rename or deletion must not silently drop a file out of the guard's \
         coverage — remove the stale entry, or re-point it at the new basename.",
        stale.len(),
        STDLIB_DIR,
        stale,
    );
}
