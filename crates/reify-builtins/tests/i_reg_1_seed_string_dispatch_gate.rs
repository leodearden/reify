//! **I-REG-1**, seed-scoped: `reify_builtins::lookup` is the only
//! string→builtin resolution in the workspace, so no `"<seed name>" =>` match
//! arm and no re-entrant `eval_builtin("<seed name>", …)` call may live
//! outside `crates/reify-builtins`.
//!
//! Task #6001 α, `docs/prds/v0_6/builtin-signature-registry.md` §7.2.
//!
//! # Scope: seed-scoped with a ledger, not workspace-wide
//!
//! α migrated two families, so this gate sweeps the SEED names only. The
//! residue it finds is REAL string dispatch on registered names, and it is
//! all outside α's charter — so it is COUNTED in
//! [`SEED_STRING_DISPATCH_LEDGER`], one entry per site with a one-sentence WHY
//! naming the leaf that owns re-homing it, rather than hidden by narrowing the
//! scan or "fixed" by editing a crate this task does not own. An UNLEDGERED
//! site fails, naming the file, line and seed name. The workspace-wide grep
//! gate (every builtin name, not just the seeds) arrives at task ω.
//!
//! The name list is derived from [`reify_builtins::rows`], never restated —
//! mirroring `reify-compiler`'s `units.rs` disjointness list — so a row added
//! by a later τ migration is swept automatically instead of silently escaping.
//!
//! # What counts as a violation
//!
//! Two shapes, both observed in the tree:
//!
//! 1. [`SiteKind::MatchArm`] — a string literal equal to a seed name in
//!    PATTERN position, i.e. followed by `=>` (possibly through `|`
//!    or-pattern alternatives and an `if` guard). This is the shape the PRD
//!    names.
//! 2. [`SiteKind::EvalCall`] — a seed name passed as the first argument of an
//!    `eval_builtin(…)` call. Not a match arm, but the same defect: a builtin
//!    identified by a hard-coded string rather than by its `BuiltinId`.
//!
//! # What deliberately does NOT count
//!
//! - **Anything under `#[cfg(test)]`.** A test may legitimately name a builtin
//!   as a string — that is how you write a call-site regression pin — and a
//!   NEGATIVE assertion such as `crates/reify-compiler/src/units.rs`'s
//!   `!is_fea_envelope_query("von_mises")` asserts the ABSENCE of a claim, the
//!   exact opposite of dispatch. Flagging either would make the gate punish
//!   test coverage. Test-gated blocks are therefore masked out before the scan
//!   (see [`mask_cfg_test_blocks`]).
//! - **Comments and raw strings.** Prose naming a builtin is not dispatch, and
//!   an `r#"…"#` block in a `src/` file is embedded `.ri` fixture text, not
//!   Rust pattern syntax.
//! - **`crates/reify-builtins` itself**, which is where the one legal
//!   string→builtin table lives.
//! - **Anything outside `crates/*/src/`** — `tests/`, `benches/`, `examples/`
//!   and the GUI's TypeScript are out of the seed gate's remit.

mod common;
use common::workspace_root;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

// ── the ledger ──────────────────────────────────────────────────────────────

/// A known, accepted string-dispatch site on a registered seed name.
struct LedgerEntry {
    /// Workspace-relative path, `/`-separated.
    file: &'static str,
    /// The seed name dispatched on.
    name: &'static str,
    /// Which leaf OWNS re-homing this site, and why α cannot.
    why: &'static str,
}

/// Every seed-name string-dispatch site α leaves standing, one entry each.
///
/// Follows the in-tree `EXEMPTION_LEDGER` pattern named by the ratified
/// amendment (`crates/reify-compiler/tests/mul_div_static_runtime_parity.rs`)
/// and `registry_drift_tests.rs`'s `QUERY_CALL_LEDGER`: a per-entry WHY
/// comment, a membership predicate, and an unledgered-divergence failure. The
/// point is that the residue is COUNTED, not that it is acceptable — every
/// entry names the leaf that retires it.
///
/// Deliberately keyed on (file, name) and NOT on a line number: a line pin
/// would break on any unrelated edit above the site, turning this gate into
/// churn. The cost is that moving a site WITHIN its file does not re-trigger
/// review; the ledger is small enough to read in full whenever it changes.
const SEED_STRING_DISPATCH_LEDGER: &[LedgerEntry] = &[
    // ── reify-expr's Field-valued interception arms ─────────────────────────
    //
    // These four arms intercept a call whose FIRST ARGUMENT IS A `Value::Field`
    // and route it to reify-expr's field-aware kernels, falling through to
    // `eval_builtin` for concrete tensors. That is `BindingKind::ExprIntercept`
    // territory, but α declares all seven seed rows `EvalBuiltin` — a row has
    // exactly one binding kind, so re-homing the Field path needs a SECOND
    // binding kind and its generated sub-enum, which is τ-fea/analysis work,
    // not α's. Deleting the arms to make this gate pass is NOT an option: it
    // would change Field-valued analysis behaviour outright.
    LedgerEntry {
        file: "crates/reify-expr/src/lib.rs",
        name: "von_mises",
        why: "Field-arg intercept — needs BindingKind::ExprIntercept (τ-fea/analysis)",
    },
    LedgerEntry {
        file: "crates/reify-expr/src/lib.rs",
        name: "principal_stresses",
        why: "Field-arg intercept — needs BindingKind::ExprIntercept (τ-fea/analysis)",
    },
    LedgerEntry {
        file: "crates/reify-expr/src/lib.rs",
        name: "max_shear",
        why: "Field-arg intercept — needs BindingKind::ExprIntercept (τ-fea/analysis)",
    },
    LedgerEntry {
        file: "crates/reify-expr/src/lib.rs",
        name: "safety_factor",
        why: "Field-arg intercept — needs BindingKind::ExprIntercept (τ-fea/analysis)",
    },
    // ── reify-expr's re-entrant call back into eval ─────────────────────────
    //
    // `compute_safety_factor` evaluates the field's inner lambda down to a
    // concrete tensor and then re-enters `eval_builtin` BY NAME. The string
    // cannot go until `CompiledExpr` carries a `BuiltinId` instead of a `&str`,
    // which is explicitly leaf β (PRD §9).
    LedgerEntry {
        file: "crates/reify-expr/src/analysis.rs",
        name: "safety_factor",
        why: "re-entrant eval_builtin call — needs CompiledExpr to carry BuiltinId (leaf β)",
    },
];

fn is_ledgered(file: &str, name: &str) -> bool {
    SEED_STRING_DISPATCH_LEDGER
        .iter()
        .any(|e| e.file == file && e.name == name)
}

// ── the scan ────────────────────────────────────────────────────────────────

/// Why a site was flagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SiteKind {
    /// `"name" =>` (possibly through `|` alternatives and an `if` guard).
    MatchArm,
    /// `eval_builtin("name", …)`.
    EvalCall,
}

impl SiteKind {
    fn describe(self) -> &'static str {
        match self {
            SiteKind::MatchArm => "match-arm string dispatch",
            SiteKind::EvalCall => "eval_builtin call keyed by name string",
        }
    }
}

/// One flagged occurrence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Site {
    /// Workspace-relative, `/`-separated.
    file: String,
    line: usize,
    name: String,
    kind: SiteKind,
}

/// A simple (non-raw) string literal found at code level.
struct StrLit {
    /// Byte offset of the opening quote.
    start: usize,
    /// Byte offset one past the closing quote.
    end: usize,
    content: String,
}

/// Blank `src[a..b]` to spaces, preserving newlines so byte offsets AND line
/// numbers both survive.
fn blank(bytes: &mut [u8], a: usize, b: usize) {
    let end = b.min(bytes.len());
    for byte in &mut bytes[a..end] {
        if *byte != b'\n' {
            *byte = b' ';
        }
    }
}

/// Blank every comment, and collect every simple string literal at code level.
///
/// Raw strings (`r"…"`, `r#"…"#`) are blanked wholesale rather than collected:
/// in a `src/` file they carry embedded `.ri` fixture text, never Rust pattern
/// syntax, and leaving them intact would let a fixture that happens to contain
/// `"von_mises" =>` trip the gate.
///
/// Not a full Rust lexer — it handles line/block comments (block comments
/// nest, as in Rust), simple and raw string literals, and char literals /
/// lifetimes. That is everything the shapes above can hide behind.
fn strip_comments_and_collect_literals(src: &str) -> (Vec<u8>, Vec<StrLit>) {
    let mut bytes = src.as_bytes().to_vec();
    let mut lits = Vec::new();
    let n = bytes.len();
    let raw = src.as_bytes();
    let mut i = 0usize;

    while i < n {
        match raw[i] {
            b'/' if i + 1 < n && raw[i + 1] == b'/' => {
                let start = i;
                while i < n && raw[i] != b'\n' {
                    i += 1;
                }
                blank(&mut bytes, start, i);
            }
            b'/' if i + 1 < n && raw[i + 1] == b'*' => {
                let start = i;
                let mut depth = 1usize;
                i += 2;
                while i < n && depth > 0 {
                    if i + 1 < n && raw[i] == b'/' && raw[i + 1] == b'*' {
                        depth += 1;
                        i += 2;
                    } else if i + 1 < n && raw[i] == b'*' && raw[i + 1] == b'/' {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                blank(&mut bytes, start, i);
            }
            b'r' if i + 1 < n && (raw[i + 1] == b'"' || raw[i + 1] == b'#') => {
                // Only a raw-string opener if the `r` starts a token.
                let prev_is_ident =
                    i > 0 && (raw[i - 1].is_ascii_alphanumeric() || raw[i - 1] == b'_');
                let mut j = i + 1;
                let mut hashes = 0usize;
                while j < n && raw[j] == b'#' {
                    hashes += 1;
                    j += 1;
                }
                if prev_is_ident || j >= n || raw[j] != b'"' {
                    i += 1;
                    continue;
                }
                let start = i;
                j += 1; // past the opening quote
                // Scan for `"` followed by `hashes` `#`s.
                loop {
                    if j >= n {
                        break;
                    }
                    if raw[j] == b'"' {
                        let mut k = j + 1;
                        let mut seen = 0usize;
                        while k < n && seen < hashes && raw[k] == b'#' {
                            seen += 1;
                            k += 1;
                        }
                        if seen == hashes {
                            j = k;
                            break;
                        }
                    }
                    j += 1;
                }
                blank(&mut bytes, start, j);
                i = j;
            }
            b'"' => {
                let start = i;
                i += 1;
                let content_start = i;
                while i < n {
                    if raw[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if raw[i] == b'"' {
                        break;
                    }
                    i += 1;
                }
                let content_end = i.min(n);
                i = (i + 1).min(n);
                lits.push(StrLit {
                    start,
                    end: i,
                    content: String::from_utf8_lossy(&raw[content_start..content_end]).into_owned(),
                });
            }
            b'\'' => {
                // A char literal (`'a'`, `'\n'`) or a lifetime (`'static`).
                // Only the former can hide a `"`; either way, stepping past a
                // char literal is enough and a lifetime is left as code.
                if i + 2 < n && raw[i + 1] == b'\\' {
                    let mut j = i + 2;
                    while j < n && raw[j] != b'\'' {
                        j += 1;
                    }
                    i = (j + 1).min(n);
                } else if i + 2 < n && raw[i + 2] == b'\'' {
                    i += 3;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }

    (bytes, lits)
}

/// Blank every `#[cfg(…test…)]`-gated item, so the scan sees production code
/// only. See the module docs for why test code is out of remit.
///
/// Handles both shapes an attribute can gate: a braced item (`mod tests { … }`,
/// `fn … { … }`) is blanked through its matching `}`, and a brace-less item
/// (`use …;`) through its `;`. Operates on the comment-stripped buffer, so a
/// brace inside a comment cannot unbalance the count; braces inside string
/// literals are skipped using the collected literal spans.
fn mask_cfg_test_blocks(code: &mut [u8], lits: &[StrLit]) {
    // Byte-level membership mask, built once: the naive
    // "is `pos` inside any literal?" scan is O(bytes x literals), which on a
    // 9k-line file like `reify-expr/src/lib.rs` costs tens of seconds.
    let in_lit = literal_mask(code.len(), lits);
    let in_literal = |pos: usize| -> bool { in_lit[pos] };

    let mut i = 0usize;
    while i < code.len() {
        if code[i] != b'#' {
            i += 1;
            continue;
        }
        // Read `#[ … ]`, balanced over the inner parens/brackets.
        if i + 1 >= code.len() || code[i + 1] != b'[' {
            i += 1;
            continue;
        }
        let attr_start = i;
        let mut j = i + 1;
        let mut depth = 0usize;
        while j < code.len() {
            match code[j] {
                b'[' | b'(' => depth += 1,
                b']' | b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        if j >= code.len() {
            break;
        }
        let attr = String::from_utf8_lossy(&code[attr_start..=j]).into_owned();
        let gates_test = attr.starts_with("#[cfg(")
            && attr
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .any(|tok| tok == "test");
        if !gates_test {
            i = j + 1;
            continue;
        }

        // Find the item this attribute gates: to the matching `}` of its first
        // `{`, or to the `;` of a brace-less item, whichever comes first.
        let mut k = j + 1;
        let mut end = None;
        while k < code.len() {
            if in_literal(k) {
                k += 1;
                continue;
            }
            match code[k] {
                b';' => {
                    end = Some(k + 1);
                    break;
                }
                b'{' => {
                    let mut d = 0usize;
                    let mut m = k;
                    while m < code.len() {
                        if !in_literal(m) {
                            if code[m] == b'{' {
                                d += 1;
                            } else if code[m] == b'}' {
                                d -= 1;
                                if d == 0 {
                                    break;
                                }
                            }
                        }
                        m += 1;
                    }
                    end = Some((m + 1).min(code.len()));
                    break;
                }
                _ => k += 1,
            }
        }
        let end = end.unwrap_or(code.len());
        blank(code, attr_start, end);
        i = end;
    }
}

/// `lit_end_at[i]` is `Some(end)` when a string literal STARTS at byte `i`,
/// letting a forward walk step over a literal in O(1) instead of rescanning
/// the literal list at every byte.
fn literal_start_index(len: usize, lits: &[StrLit]) -> Vec<Option<usize>> {
    let mut idx = vec![None; len + 1];
    for l in lits {
        if l.start < idx.len() {
            idx[l.start] = Some(l.end);
        }
    }
    idx
}

/// Byte-level "is this offset inside a string literal?" mask.
fn literal_mask(len: usize, lits: &[StrLit]) -> Vec<bool> {
    let mut mask = vec![false; len + 1];
    for l in lits {
        for slot in mask.iter_mut().take(l.end.min(len)).skip(l.start) {
            *slot = true;
        }
    }
    mask
}

/// Is the string literal at `lits[idx]` in match-PATTERN position — i.e. does
/// the arm it opens reach `=>`?
///
/// Two states, because the two halves of an arm head have different grammar:
///
/// - **Pattern.** Only whitespace, `|` or-pattern separators and further
///   string literals may appear. `=>` here means the literal was a pattern;
///   the `if` keyword hands off to the guard; anything else — `,`, `.`, `)`,
///   an identifier — means it was an ordinary expression, not a pattern.
/// - **Guard.** An arbitrary boolean expression, so its own `.`, `,`, parens
///   and braces are all legal and must NOT abort the walk (the real shape is
///   `if args.len() == 1 && matches!(&args[0], Value::Field { .. }) =>`). Only
///   `=>` at depth 0 accepts; `;` at depth 0 or a depth going negative
///   rejects.
fn is_match_arm(code: &[u8], lit_end_at: &[Option<usize>], idx_end: usize) -> bool {
    let mut i = idx_end;
    let limit = (i + 800).min(code.len());

    // ── pattern position ────────────────────────────────────────────────────
    while i < limit {
        if let Some(end) = lit_end_at[i] {
            i = end;
            continue;
        }
        let c = code[i];
        if c.is_ascii_whitespace() || c == b'|' {
            i += 1;
            continue;
        }
        if c == b'=' && i + 1 < limit && code[i + 1] == b'>' {
            return true;
        }
        if c == b'i' && i + 1 < limit && code[i + 1] == b'f' {
            let after_ok =
                i + 2 >= limit || !(code[i + 2].is_ascii_alphanumeric() || code[i + 2] == b'_');
            if after_ok {
                i += 2;
                break;
            }
        }
        return false;
    }

    // ── guard position ──────────────────────────────────────────────────────
    let mut depth = 0i32;
    while i < limit {
        if let Some(end) = lit_end_at[i] {
            i = end;
            continue;
        }
        let c = code[i];
        if c == b'=' && i + 1 < limit && code[i + 1] == b'>' && depth == 0 {
            return true;
        }
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            b';' if depth == 0 => return false,
            _ => {}
        }
        i += 1;
    }
    false
}

/// Is the string literal at `lit` the first argument of an `eval_builtin(…)`
/// call?
fn is_eval_call(code: &[u8], lit_start: usize) -> bool {
    let mut i = lit_start;
    while i > 0 && code[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    if i == 0 || code[i - 1] != b'(' {
        return false;
    }
    let paren = i - 1;
    let mut start = paren;
    while start > 0 {
        let c = code[start - 1];
        if c.is_ascii_alphanumeric() || c == b'_' {
            start -= 1;
        } else {
            break;
        }
    }
    &code[start..paren] == b"eval_builtin"
}

fn line_of(code: &[u8], offset: usize) -> usize {
    code[..offset].iter().filter(|&&b| b == b'\n').count() + 1
}

/// Every `crates/*/src/**/*.rs` outside `crates/reify-builtins`.
fn production_sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let crates_dir = root.join("crates");
    let mut crate_dirs: Vec<PathBuf> = std::fs::read_dir(&crates_dir)
        .unwrap_or_else(|e| panic!("cannot read {crates_dir:?}: {e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name() != Some("reify-builtins".as_ref()))
        .collect();
    crate_dirs.sort();
    for dir in crate_dirs {
        collect_rs(&dir.join("src"), &mut out);
    }
    out.sort();
    out
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension() == Some("rs".as_ref()) {
            out.push(path);
        }
    }
}

/// Scan the workspace for seed-name string dispatch.
fn scan(root: &Path, seed_names: &BTreeSet<String>) -> Vec<Site> {
    let mut sites = Vec::new();
    for path in production_sources(root) {
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        let (mut code, lits) = strip_comments_and_collect_literals(&src);
        mask_cfg_test_blocks(&mut code, &lits);
        let lit_end_at = literal_start_index(code.len(), &lits);

        for lit in lits.iter() {
            if !seed_names.contains(&lit.content) {
                continue;
            }
            // A literal inside a masked (test-gated) region is gone from `code`.
            if code[lit.start] != b'"' {
                continue;
            }
            let kind = if is_match_arm(&code, &lit_end_at, lit.end) {
                SiteKind::MatchArm
            } else if is_eval_call(&code, lit.start) {
                SiteKind::EvalCall
            } else {
                continue;
            };
            sites.push(Site {
                file: rel.clone(),
                line: line_of(&code, lit.start),
                name: lit.content.clone(),
                kind,
            });
        }
    }
    sites.sort();
    sites
}

fn seed_names() -> BTreeSet<String> {
    reify_builtins::rows()
        .iter()
        .map(|r| r.name.to_string())
        .collect()
}

// ── the gate ────────────────────────────────────────────────────────────────

#[test]
fn no_unledgered_seed_name_string_dispatch_outside_reify_builtins() {
    let root = workspace_root();
    let sites = scan(&root, &seed_names());

    let unledgered: Vec<&Site> = sites
        .iter()
        .filter(|s| !is_ledgered(&s.file, &s.name))
        .collect();

    assert!(
        unledgered.is_empty(),
        "I-REG-1 violated: {} unledgered string dispatch site(s) on a \
         registered builtin name outside crates/reify-builtins.\n\n{}\n\n\
         `reify_builtins::lookup` is the only sanctioned string\u{2192}builtin \
         resolution. Bind the name to its `BuiltinId` in the owning kind's \
         exhaustive dispatcher, or \u{2014} if re-homing it belongs to a later \
         leaf \u{2014} add an entry to SEED_STRING_DISPATCH_LEDGER naming that \
         leaf.",
        unledgered.len(),
        unledgered
            .iter()
            .map(|s| format!(
                "  {}:{} \u{2014} {:?} ({})",
                s.file,
                s.line,
                s.name,
                s.kind.describe()
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The ledger is a RATCHET, not a suppression list: every entry must name a
/// site that still exists. A stale entry means the site was retired and the
/// ledger should SHRINK — which is the whole point of counting the residue
/// rather than hiding it.
#[test]
fn every_ledger_entry_names_a_site_that_still_exists() {
    let root = workspace_root();
    let sites = scan(&root, &seed_names());

    let stale: Vec<String> = SEED_STRING_DISPATCH_LEDGER
        .iter()
        .filter(|e| !sites.iter().any(|s| s.file == e.file && s.name == e.name))
        .map(|e| format!("  {} \u{2014} {:?} ({})", e.file, e.name, e.why))
        .collect();

    assert!(
        stale.is_empty(),
        "SEED_STRING_DISPATCH_LEDGER has {} stale entry/entries \u{2014} the \
         site is gone, so the entry must go too:\n{}",
        stale.len(),
        stale.join("\n")
    );
}

/// The scan itself must keep its teeth: if it silently stopped SEEING the
/// sites the ledger accepts, it would report a clean workspace while the
/// residue sat untouched. Pinning the count means a scanner regression fails
/// here rather than passing vacuously.
#[test]
fn the_scan_still_finds_every_ledgered_site() {
    let root = workspace_root();
    let sites = scan(&root, &seed_names());
    assert_eq!(
        sites.len(),
        SEED_STRING_DISPATCH_LEDGER.len(),
        "expected the scan to find exactly the {} ledgered site(s), found {}: \
         {sites:#?}",
        SEED_STRING_DISPATCH_LEDGER.len(),
        sites.len()
    );
}

/// Pins the `#[cfg(test)]` exclusion documented in the module docs, against
/// synthetic input rather than against whatever the tree happens to contain —
/// so the exclusion cannot rot when the tree changes.
#[test]
fn cfg_test_blocks_and_comments_are_excluded_from_the_scan() {
    let src = r###"
fn production(name: &str) -> u8 {
    match name {
        "von_mises" => 1,
        _ => 0,
    }
}

// A comment naming "max_shear" => is prose, not dispatch.

#[cfg(test)]
mod tests {
    #[test]
    fn negative_assertion() {
        // The units.rs:4010 shape: asserting a name is NOT claimed.
        assert!(!is_fea_envelope_query("von_mises"));
        assert_eq!(eval_builtin("safety_factor", &[]), 0);
        match "principal_stresses" {
            "principal_stresses" => {}
            _ => {}
        }
    }
}
"###;
    let (mut code, lits) = strip_comments_and_collect_literals(src);
    mask_cfg_test_blocks(&mut code, &lits);

    let names: BTreeSet<String> = [
        "von_mises",
        "max_shear",
        "safety_factor",
        "principal_stresses",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();

    let lit_end_at = literal_start_index(code.len(), &lits);
    let found: Vec<(String, SiteKind)> = lits
        .iter()
        .filter(|l| names.contains(&l.content) && code[l.start] == b'"')
        .filter_map(|l| {
            if is_match_arm(&code, &lit_end_at, l.end) {
                Some((l.content.clone(), SiteKind::MatchArm))
            } else if is_eval_call(&code, l.start) {
                Some((l.content.clone(), SiteKind::EvalCall))
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        found,
        vec![("von_mises".to_string(), SiteKind::MatchArm)],
        "only the production match arm should be flagged — the comment and \
         everything under #[cfg(test)] must be excluded"
    );
}

/// Pins the two violation shapes the module docs name, so a scanner that
/// silently stopped recognising guarded arms or `eval_builtin` calls fails
/// loudly instead of reporting a clean workspace.
#[test]
fn both_violation_shapes_are_recognised() {
    let src = r###"
fn dispatch(name: &str, args: &[Value]) -> Value {
    match name {
        "von_mises"
            if args.len() == 1 && matches!(&args[0], Value::Field { .. }) =>
        {
            field_von_mises(&args[0])
        }
        "max_shear" | "principal_stresses" => reduce(name, args),
        _ => reify_stdlib::eval_builtin("safety_factor", args),
    }
}
"###;
    let (mut code, lits) = strip_comments_and_collect_literals(src);
    mask_cfg_test_blocks(&mut code, &lits);

    let names: BTreeSet<String> = [
        "von_mises",
        "max_shear",
        "principal_stresses",
        "safety_factor",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();

    let lit_end_at = literal_start_index(code.len(), &lits);
    let mut found: Vec<(String, SiteKind)> = lits
        .iter()
        .filter(|l| names.contains(&l.content) && code[l.start] == b'"')
        .filter_map(|l| {
            if is_match_arm(&code, &lit_end_at, l.end) {
                Some((l.content.clone(), SiteKind::MatchArm))
            } else if is_eval_call(&code, l.start) {
                Some((l.content.clone(), SiteKind::EvalCall))
            } else {
                None
            }
        })
        .collect();
    found.sort();

    let mut expected = vec![
        ("von_mises".to_string(), SiteKind::MatchArm),
        ("max_shear".to_string(), SiteKind::MatchArm),
        ("principal_stresses".to_string(), SiteKind::MatchArm),
        ("safety_factor".to_string(), SiteKind::EvalCall),
    ];
    expected.sort();

    assert_eq!(
        found, expected,
        "a guarded arm, an or-pattern alternative, and an eval_builtin call \
         must all be recognised"
    );
}

// ── the one-hop forwarder rule (step-22/23) ─────────────────────────────────

/// A production source in the shape `crates/reify-expr/src/analysis.rs` has:
/// one helper that launders a `&str` into `eval_builtin`, and two that take a
/// seed name purely as a diagnostic label. The gate must tell them apart —
/// flagging all four would inflate the ledger with entries that name no
/// dispatch at all, and flagging none certifies a residue it cannot see.
///
/// Synthetic, deliberately: like
/// [`cfg_test_blocks_and_comments_are_excluded_from_the_scan`], this pins the
/// RULE against hand-written text, so it cannot rot when the tree changes.
const FORWARDER_FIXTURE: &str = r###"
fn wrap_tensor_field(field_val: &Value, op: &str, kind: FieldSourceKind) -> Value {
    eprintln!("[reify-expr] {}: not a tensor field", op);
    Value::Undef
}

fn validate_tensor_field(field_val: &Value, op: &str) -> Option<Triple> {
    eprintln!("[reify-expr] {}: expected a Matrix3x3 field", op);
    None
}

fn sample_unary_analysis_at_point(
    inner_lambda: &Value,
    point: &Value,
    ctx: &EvalContext,
    builtin_name: &str,
) -> Value {
    let tensor = apply_lambda_with_point_unpacking(inner_lambda, point, ctx);
    if tensor.is_undef() {
        return Value::Undef;
    }
    reify_stdlib::eval_builtin(builtin_name, &[tensor])
}

pub(crate) fn compute_von_mises(field_val: &Value) -> Value {
    wrap_tensor_field(field_val, "von_mises", FieldSourceKind::VonMises)
}

pub(crate) fn compute_principal_stresses(field_val: &Value) -> Value {
    let triple = validate_tensor_field(field_val, "principal_stresses")?;
    wrap(triple)
}

pub(crate) fn sample_von_mises_at_point(inner: &Value, point: &Value, ctx: &EvalContext) -> Value {
    sample_unary_analysis_at_point(inner, point, ctx, "von_mises")
}

pub(crate) fn sample_max_shear_at_point(inner: &Value, point: &Value, ctx: &EvalContext) -> Value {
    sample_unary_analysis_at_point(inner, point, ctx, "max_shear")
}
"###;

fn fixture_names() -> BTreeSet<String> {
    [
        "von_mises",
        "max_shear",
        "principal_stresses",
        "safety_factor",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

/// (a) POSITIVE — a fn with a `&str` parameter whose body calls
/// `eval_builtin(<that same parameter>, …)` is a FORWARDER, and a seed-name
/// literal passed at that parameter's position is a violation.
///
/// (b) NEGATIVE, load-bearing — `wrap_tensor_field`/`validate_tensor_field`
/// take a `&str` they never pass to `eval_builtin`, so they are NOT
/// forwarders and their literal call sites are NOT flagged. A naive "any
/// `&str` param" rule would flag all four.
#[test]
fn only_a_str_param_that_reaches_eval_builtin_makes_a_forwarder() {
    let found = find_forwarders_in(FORWARDER_FIXTURE);
    assert_eq!(
        found,
        vec![Forwarder {
            name: "sample_unary_analysis_at_point".to_string(),
            param: 3,
            param_name: "builtin_name".to_string(),
        }],
        "exactly one fn in the fixture launders a `&str` into eval_builtin; \
         `wrap_tensor_field`/`validate_tensor_field` take a `&str` label they \
         never dispatch on and must not be treated as forwarders"
    );
}

/// The classification consequence of (a) + (b): the two forwarded call sites
/// are flagged, the two label call sites are not.
#[test]
fn forwarded_seed_names_are_flagged_and_label_arguments_are_not() {
    let forwarders = find_forwarders_in(FORWARDER_FIXTURE);
    let sites = classify_text(
        "fixture.rs",
        FORWARDER_FIXTURE,
        &forwarders,
        &fixture_names(),
    );

    let got: Vec<(String, SiteKind)> = sites
        .iter()
        .map(|s| (s.name.clone(), s.kind))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    assert_eq!(
        got,
        vec![
            ("max_shear".to_string(), SiteKind::ForwardedEvalCall),
            ("von_mises".to_string(), SiteKind::ForwardedEvalCall),
        ],
        "only the literals at the forwarder's laundered parameter position \
         are dispatch; `wrap_tensor_field(field_val, \"von_mises\", …)` and \
         `validate_tensor_field(field_val, \"principal_stresses\")` pass a \
         diagnostic label and must stay unflagged.\nsites: {sites:#?}"
    );
}

/// (c) The forwarder's OWN `eval_builtin(builtin_name, …)` line is not itself
/// a site: its first argument is an identifier, not a literal. Pinned
/// explicitly so a future scanner cannot start double-counting the hop it
/// already counts at the call site.
#[test]
fn the_forwarders_own_eval_builtin_line_is_not_a_site() {
    let forwarders = find_forwarders_in(FORWARDER_FIXTURE);
    let sites = classify_text(
        "fixture.rs",
        FORWARDER_FIXTURE,
        &forwarders,
        &fixture_names(),
    );

    let hop_line = FORWARDER_FIXTURE
        .lines()
        .position(|l| l.contains("eval_builtin(builtin_name"))
        .map(|i| i + 1)
        .expect("fixture must contain the laundered eval_builtin call");

    assert!(
        sites.iter().all(|s| s.line != hop_line),
        "the forwarder's own eval_builtin call takes an identifier, so it \
         must produce no site; the string is counted once, at the call site \
         that supplies it.\nsites: {sites:#?}"
    );
}

/// (d) The forwarder set must be DERIVED, never a hand-maintained allowlist.
/// Sweeps the real tree and re-verifies every fn the scan treats as a
/// forwarder with an INDEPENDENT check — a whitespace-insensitive substring
/// search for `eval_builtin(<param>` — so a scanner that started inventing
/// forwarders fails here rather than silently inflating the ledger.
#[test]
fn every_discovered_forwarder_really_forwards_to_eval_builtin() {
    let root = workspace_root();
    let mut unverified: Vec<String> = Vec::new();

    for path in production_sources(&root) {
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        let forwarders = find_forwarders_in(&src);
        if forwarders.is_empty() {
            continue;
        }
        let squashed: String = src.chars().filter(|c| !c.is_whitespace()).collect();
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        for f in forwarders {
            let comma = format!("eval_builtin({},", f.param_name);
            let only = format!("eval_builtin({})", f.param_name);
            if !squashed.contains(&comma) && !squashed.contains(&only) {
                unverified.push(format!(
                    "  {}: fn {} (param #{} `{}`) is treated as a forwarder, \
                     but the file contains no `eval_builtin({}…)` call",
                    rel, f.name, f.param, f.param_name, f.param_name
                ));
            }
        }
    }

    assert!(
        unverified.is_empty(),
        "the forwarder set must be derived from the source, not declared:\n{}",
        unverified.join("\n")
    );
}
