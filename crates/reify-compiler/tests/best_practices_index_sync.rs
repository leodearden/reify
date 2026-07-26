//! Catalogue-drift guard for the `examples/best_practices/` exemplar corpus
//! (task #5397).
//!
//! The corpus is *compile*-gated for free: `examples_smoke.rs` discovers
//! `examples/**/*.ri` recursively, so every exemplar is already covered by
//! `all_examples_parse_and_compile_with_stdlib` and
//! `no_example_emits_ctor_field_conformance_diagnostics` with no new infra.
//!
//! What is NOT covered for free is the corpus's hand-maintained catalogue.
//! The `reify-design` skill's session-wrap probe-graduation hook tells agents
//! to grep `examples/best_practices/INDEX.md` *first* to decide whether an
//! idiom is already captured.  That grep is only trustworthy if the index and
//! the directory cannot silently diverge: a graduated probe with no index
//! entry is invisible to exactly the lookup the hook prescribes, which
//! reintroduces the capability≠use drift the corpus exists to fight.
//!
//! So this test pins one bidirectional invariant, on **filenames only**:
//!
//! 1. every `*.ri` file in the corpus directory is named somewhere in
//!    `INDEX.md`; and
//! 2. every bare `*.ri` filename named in `INDEX.md` is a file that actually
//!    exists in the corpus directory.
//!
//! It deliberately asserts nothing about prose, wording, ordering, or entry
//! format — those are documentation, and pinning them would just make the
//! index expensive to edit.  Structural correspondence is the whole contract.
//!
//! Shape (path const, flat `read_dir`, accumulate-then-panic-once with an
//! actionable report) follows the in-repo precedent set by
//! `examples_smoke.rs::skip_set_entries_exist_under_examples_dir`.

use std::path::{Path, PathBuf};

/// Absolute path to the `examples/best_practices/` corpus, resolved at compile
/// time from this crate's manifest directory (two levels up) — same idiom as
/// `examples_smoke.rs`'s `EXAMPLES_DIR`.
const CORPUS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/best_practices");

/// Basename of the hand-maintained catalogue that must stay in step with the
/// corpus directory.
const INDEX_NAME: &str = "INDEX.md";

/// Repo-relative prefix accepted on a path-qualified index reference, so an
/// entry may be written either bare (`negation.ri`) or fully qualified
/// (`examples/best_practices/negation.ri`) without changing its meaning.
const CORPUS_PREFIX: &str = "examples/best_practices/";

/// Appended to every failure report.  The guard fires most often on a
/// half-finished probe graduation, so the message has to state the remedy
/// rather than merely the symptom — a future agent that lands an exemplar and
/// forgets its index entry gets told exactly what to do, with no need to read
/// this file or the skill.
const GRADUATION_HOOK_HINT: &str = "\
The `examples/best_practices/` corpus and its INDEX.md are maintained together \
by the probe-graduation hook in `.claude/skills/reify-design/SKILL.md` \
(\"Session wrap — graduate your probes\"): landing an exemplar .ri file and \
adding its INDEX.md entry are ONE step, never two.\n\
  * missing index entry  -> add a one-line INDEX.md entry naming the file and \
the idiom it demonstrates (do NOT delete the exemplar to silence this).\n\
  * missing file         -> the index names an exemplar that is not there; \
restore the file, or drop/repair the stale entry.\n\
Re-verify with: cargo test -p reify-compiler --test examples_smoke \
--test best_practices_index_sync";

/// Bidirectional filename correspondence between the corpus directory and its
/// `INDEX.md`.
///
/// Every violation is accumulated and reported in one panic: the guard exists
/// for catalogue-wide visibility, so it must not fail fast and hide the second
/// through Nth drifted entry behind the first.
#[test]
fn best_practices_index_matches_corpus_directory() {
    let dir = Path::new(CORPUS_DIR);
    let index_path = dir.join(INDEX_NAME);
    let mut violations: Vec<String> = Vec::new();

    // ── Preconditions ────────────────────────────────────────────────────
    // A missing directory or index makes every downstream check vacuous, so
    // report and stop rather than emit a cascade of derived noise.
    if !dir.is_dir() {
        violations.push(format!(
            "corpus directory '{}' does not exist (or is not a directory)",
            dir.display()
        ));
        report(violations);
        return;
    }
    if !index_path.is_file() {
        violations.push(format!(
            "'{}' is missing — the corpus directory exists but has no catalogue",
            index_path.display()
        ));
        report(violations);
        return;
    }

    let ri_files = corpus_ri_files(dir);
    if ri_files.is_empty() {
        violations.push(format!(
            "corpus directory '{}' contains no *.ri exemplars — an empty \
             best-practices corpus is drift, not a valid state",
            dir.display()
        ));
    }

    let index_text = std::fs::read_to_string(&index_path).unwrap_or_else(|e| {
        panic!(
            "best_practices_index_sync: cannot read '{}': {}",
            index_path.display(),
            e
        )
    });
    let referenced = index_ri_references(&index_text);

    // ── Direction 1: file on disk -> entry in INDEX.md ───────────────────
    for name in &ri_files {
        if !referenced.iter().any(|r| r == name) {
            violations.push(format!(
                "'{}' exists in the corpus but is never named in {}",
                name, INDEX_NAME
            ));
        }
    }

    // ── Direction 2: entry in INDEX.md -> file on disk ───────────────────
    for name in &referenced {
        if !ri_files.iter().any(|f| f == name) {
            violations.push(format!(
                "{} names '{}' but no such file exists in the corpus directory",
                INDEX_NAME, name
            ));
        }
    }

    report(violations);
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Panic once with a combined, actionable report if `violations` is non-empty.
fn report(violations: Vec<String>) {
    if violations.is_empty() {
        return;
    }
    let n = violations.len();
    let lines: Vec<String> = violations.iter().map(|v| format!("  - {}", v)).collect();
    panic!(
        "best_practices corpus/INDEX.md drift: {} violation(s).\n\n{}\n\n{}",
        n,
        lines.join("\n"),
        GRADUATION_HOOK_HINT
    );
}

/// Basenames of the `*.ri` files directly inside `dir`, sorted for
/// deterministic reporting.
///
/// Deliberately a flat (non-recursive) read: the corpus is a single flat
/// drawer of idiom exemplars by design, and a nested subdirectory appearing
/// here is a structural change that should be reviewed rather than silently
/// absorbed by the guard.
fn corpus_ri_files(dir: &Path) -> Vec<String> {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "best_practices_index_sync: cannot read directory '{}': {}",
            dir.display(),
            e
        )
    });
    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.expect("IO error reading best_practices dir entry");
        let path: PathBuf = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ri") {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            names.push(name.to_string());
        }
    }
    names.sort();
    names
}

/// Corpus-local `*.ri` filenames referenced anywhere in `text`, deduplicated
/// and sorted.
///
/// A reference counts when it is either a bare basename (`negation.ri`) or is
/// qualified with the corpus's own repo-relative prefix
/// (`examples/best_practices/negation.ri`).  A `*.ri` token pointing anywhere
/// else in the repo — e.g. the `examples/kernel_queries/intersects_smoke.ri`
/// cross-references the exemplars are expected to carry — is NOT a corpus
/// membership claim and is ignored, so cross-referencing a proven-green file
/// elsewhere in `examples/` never trips this guard.
fn index_ri_references(text: &str) -> Vec<String> {
    let mut refs: Vec<String> = Vec::new();
    let mut current = String::new();

    // Markdown decoration (backticks, brackets, parens, commas, whitespace)
    // all fall outside this set, so it splits `` `negation.ri` `` and
    // `[negation.ri](negation.ri)` into clean tokens without special-casing
    // any particular link style.
    let is_token_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/');

    for ch in text.chars() {
        if is_token_char(ch) {
            current.push(ch);
        } else {
            push_ref(&current, &mut refs);
            current.clear();
        }
    }
    push_ref(&current, &mut refs);

    refs.sort();
    refs.dedup();
    refs
}

/// Normalize one raw token and, if it names a corpus-local `*.ri` file, push
/// its basename onto `refs`.
fn push_ref(raw: &str, refs: &mut Vec<String>) {
    // Trailing sentence punctuation rides along with the token ("see
    // negation.ri.") — strip it before the extension test.
    let token = raw.trim_end_matches('.');
    if !token.ends_with(".ri") {
        return;
    }
    // Reject a bare ".ri" and glob fragments like "*.ri": no stem, no claim.
    let name = match token.strip_prefix(CORPUS_PREFIX) {
        Some(rest) => rest,
        // Any other path-qualified token points outside the corpus.
        None if token.contains('/') => return,
        None => token,
    };
    if name.contains('/') || name.len() <= ".ri".len() {
        return;
    }
    refs.push(name.to_string());
}
