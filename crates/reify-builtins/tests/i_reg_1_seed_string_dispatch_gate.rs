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
//! Three shapes, all observed in the tree:
//!
//! 1. [`SiteKind::MatchArm`] — a string literal equal to a seed name in
//!    PATTERN position, i.e. followed by `=>` (possibly through `|`
//!    or-pattern alternatives and an `if` guard). This is the shape the PRD
//!    names.
//! 2. [`SiteKind::EvalCall`] — a seed name passed as the first argument of an
//!    `eval_builtin(…)` call. Not a match arm, but the same defect: a builtin
//!    identified by a hard-coded string rather than by its `BuiltinId`.
//! 3. [`SiteKind::ForwardedEvalCall`] — a seed name passed to a helper that
//!    itself calls `eval_builtin(<that very parameter>, …)`. Shape 2 one hop
//!    later, and a real hole: `reify-expr`'s
//!    `sample_unary_analysis_at_point(…, builtin_name: &str)` launders three
//!    of the four analysis names this way, so a lexical "literal adjacent to
//!    `eval_builtin(`" rule certifies a residue it cannot see.
//!
//!    The forwarder set is DERIVED, never declared (see [`Forwarder`]): a fn
//!    qualifies only if its own body dispatches on the parameter. That
//!    distinction is load-bearing rather than pedantic — `wrap_tensor_field(…,
//!    op: &str, …)` and `validate_tensor_field(…, op: &str)` sit in the same
//!    file and take a seed name purely as a diagnostic label, so an "any
//!    `&str` param" rule would inflate the ledger with entries that name no
//!    dispatch at all.
//!
//! # What deliberately does NOT count
//!
//! - **Anything under `#[cfg(test)]`.** A test may legitimately name a builtin
//!   as a string — that is how you write a call-site regression pin — and a
//!   NEGATIVE assertion such as `crates/reify-compiler/src/units.rs`'s
//!   `!is_fea_envelope_query("von_mises")` asserts the ABSENCE of a claim, the
//!   exact opposite of dispatch. Flagging either would make the gate punish
//!   test coverage. Test-gated blocks are therefore masked out before the scan
//!   (see [`mask_cfg_test_blocks`]). "Test-gated" means `#[cfg(test)]`,
//!   `#[cfg(any(test, …))]` and a `feature` whose name starts with `test`
//!   (`test-support`, `test-fixtures`) — but NOT `#[cfg(not(test))]`, which is
//!   a production-only guard and stays in the scan. That distinction is
//!   [`attr_gates_test_code`]'s, and it is the one `reify-audit`'s
//!   `p2_consumer_stub::is_test_cfg_attr` already makes for the same reason.
//! - **Comments and raw strings.** Prose naming a builtin is not dispatch, and
//!   an `r#"…"#` block in a `src/` file is embedded `.ri` fixture text, not
//!   Rust pattern syntax.
//! - **`crates/reify-builtins` itself**, which is where the one legal
//!   string→builtin table lives.
//! - **Anything outside `crates/*/src/`** — `tests/`, `benches/`, `examples/`
//!   and the GUI's TypeScript are out of the seed gate's remit.

mod common;
use common::workspace_root;

use std::collections::{BTreeMap, BTreeSet};
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
    // The other three arrive at `eval_builtin` ONE HOP LATER, through
    // `sample_unary_analysis_at_point(…, builtin_name: &str)`, which passes
    // the name straight through. Same root cause, same fix, same owning leaf:
    // once `CompiledExpr` carries a `BuiltinId`, the forwarder takes an id and
    // all four go together. Splitting them across leaves would misdirect the
    // leaf that has to do the work.
    LedgerEntry {
        file: "crates/reify-expr/src/analysis.rs",
        name: "von_mises",
        why: "re-entrant eval_builtin call — needs CompiledExpr to carry BuiltinId (leaf β)",
    },
    LedgerEntry {
        file: "crates/reify-expr/src/analysis.rs",
        name: "principal_stresses",
        why: "re-entrant eval_builtin call — needs CompiledExpr to carry BuiltinId (leaf β)",
    },
    LedgerEntry {
        file: "crates/reify-expr/src/analysis.rs",
        name: "max_shear",
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
    /// A seed name handed to a helper that itself calls
    /// `eval_builtin(<that parameter>, …)` — the same defect as
    /// [`SiteKind::EvalCall`], one hop later. See [`Forwarder`].
    ForwardedEvalCall,
}

impl SiteKind {
    fn describe(self) -> &'static str {
        match self {
            SiteKind::MatchArm => "match-arm string dispatch",
            SiteKind::EvalCall => "eval_builtin call keyed by name string",
            SiteKind::ForwardedEvalCall => {
                "name string forwarded one hop into an eval_builtin call"
            }
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

/// Does this `#[cfg(…)]` attribute gate **test-only** code — i.e. may the item
/// it guards be masked out of the scan?
///
/// Mirrors the classification `crates/reify-audit/src/p2_consumer_stub.rs`'s
/// `is_test_cfg_attr` already makes for the same reason, rather than the naive
/// "does the token `test` appear anywhere" test this gate used to run. That
/// test was wrong in the SILENT direction — it masked production code:
///
/// - `#[cfg(not(test))]` is a **production-only** guard (live in tree, e.g.
///   `crates/reify-ir/src/sampled.rs`), so blanking it hid real production code
///   from an I-REG-1 gate whose entire job is to see it. Negated predicates are
///   therefore never test-gating here, tracked by paren depth so `not(...)`
///   nested under `any`/`all` is handled too.
/// - `#[cfg(feature = "test-support")]` / `"test-fixtures"` (live in
///   `crates/reify-kernel-manifold`) matched only because `test-support` splits
///   on `-` into `test` + `support`. They ARE test-support code and masking
///   them is right, but it must be a DECISION, not an accident of tokenising —
///   so a `feature` whose name starts with `test` is matched deliberately here,
///   and `#[cfg(feature = "fastest")]` is not.
///
/// A predicate under `not(...)` returns `false` (do not mask), which is the
/// fail-LOUD direction: the item stays in the scan, so a string-dispatch site
/// hidden there is reported rather than silently certified clean.
fn attr_gates_test_code(attr: &str) -> bool {
    let Some(inner) = attr.strip_prefix("#[cfg(") else {
        return false;
    };
    let b = inner.as_bytes();
    // Paren depths at which a `not(` is still open. Non-empty ⇒ the predicate
    // being read is negated, so it gates PRODUCTION code, not test code.
    let mut not_depths: Vec<usize> = Vec::new();
    let mut depth = 0usize;
    // The last bare identifier, so a string literal can be attributed to the
    // `feature` it belongs to (`feature = "test-support"`).
    let mut last_ident = "";
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                while not_depths.last() == Some(&depth) {
                    not_depths.pop();
                }
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'"' => {
                let start = i + 1;
                let mut k = start;
                while k < b.len() && b[k] != b'"' {
                    if b[k] == b'\\' {
                        k += 1;
                    }
                    k += 1;
                }
                let content = &inner[start..k.min(inner.len())];
                if last_ident == "feature" && content.starts_with("test") && not_depths.is_empty() {
                    return true;
                }
                last_ident = "";
                i = k + 1;
            }
            c if c.is_ascii_alphanumeric() || c == b'_' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                let ident = &inner[start..i];
                if ident == "not" {
                    // `not` negates everything inside the `(` that follows it.
                    not_depths.push(depth + 1);
                } else if ident == "test" && not_depths.is_empty() {
                    return true;
                }
                last_ident = ident;
            }
            _ => i += 1,
        }
    }
    false
}

/// Blank every test-gated item (see [`attr_gates_test_code`]), so the scan sees
/// production code only. See the module docs for why test code is out of remit.
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
        if !attr_gates_test_code(&attr) {
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

// ── one-hop `&str` forwarding ───────────────────────────────────────────────

/// A production fn that launders a `&str` parameter into `eval_builtin` — the
/// one hop [`is_eval_call`] is lexically blind to.
///
/// Discovered from the source, never declared: a fn qualifies only if its own
/// body calls `eval_builtin(<that very parameter>, …)`. That is what keeps the
/// rule from degrading into a hand-maintained allowlist, and what keeps a
/// `&str` taken purely as a diagnostic label (`wrap_tensor_field(…, op: &str,
/// …)`) from being mistaken for dispatch.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Forwarder {
    /// The fn's name, as written at its definition.
    name: String,
    /// Zero-based position of the laundered parameter **as callers write it**
    /// — a `self` receiver is not an argument, so it is excluded from the
    /// count.
    param: usize,
    /// That parameter's identifier, carried so the rule can be re-verified
    /// independently of the walk that produced it (see
    /// `every_discovered_forwarder_really_forwards_to_eval_builtin`).
    param_name: String,
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn skip_ws(code: &[u8], mut i: usize) -> usize {
    while i < code.len() && code[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// Read the identifier starting at `i`, returning it and the offset one past
/// its end. `None` when `i` is not on an identifier byte.
fn read_ident(code: &[u8], i: usize) -> Option<(String, usize)> {
    let mut j = i;
    while j < code.len() && is_ident_byte(code[j]) {
        j += 1;
    }
    (j > i).then(|| (String::from_utf8_lossy(&code[i..j]).into_owned(), j))
}

/// Offset of the delimiter matching the opener at `open`, skipping string
/// literals so a brace inside one cannot unbalance the count.
fn match_delim(code: &[u8], lit_end_at: &[Option<usize>], open: usize) -> Option<usize> {
    let (o, c) = match code.get(open)? {
        b'(' => (b'(', b')'),
        b'[' => (b'[', b']'),
        b'{' => (b'{', b'}'),
        _ => return None,
    };
    let mut depth = 0usize;
    let mut i = open;
    while i < code.len() {
        if let Some(end) = lit_end_at[i] {
            i = end;
            continue;
        }
        if code[i] == o {
            depth += 1;
        } else if code[i] == c {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Split `code[open+1..close]` at depth-0 commas, returning one `(start, end)`
/// span per parameter/argument. An empty trailing segment (trailing comma) is
/// dropped.
///
/// `generic_types` distinguishes the two callers: a PARAMETER list may contain
/// `Option<&str>`, so `<` … `>` must nest, while an ARGUMENT list may contain
/// `a < b`, so it must not — there, only a turbofish `::<` opens a nesting
/// level. A stray `>` is ignored rather than driving the depth negative.
fn split_delimited(
    code: &[u8],
    lit_end_at: &[Option<usize>],
    open: usize,
    close: usize,
    generic_types: bool,
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut angle = 0i32;
    let mut start = open + 1;
    let mut i = start;
    while i < close {
        if let Some(end) = lit_end_at[i] {
            i = end.min(close);
            continue;
        }
        match code[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'<' if generic_types || (i > 0 && code[i - 1] == b':') => angle += 1,
            b'>' if angle > 0 && !(i > 0 && code[i - 1] == b'-') => angle -= 1,
            b',' if depth == 0 && angle == 0 => {
                out.push((start, i));
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if code[start.min(close)..close]
        .iter()
        .any(|b| !b.is_ascii_whitespace())
    {
        out.push((start, close));
    }
    out
}

/// Is `ty` a shared string slice — `&str`, `& str`, `&'a str`, `&'static str`?
fn is_str_ref(ty: &str) -> bool {
    let t = ty.trim();
    let Some(rest) = t.strip_prefix('&') else {
        return false;
    };
    let rest = rest.trim_start();
    let rest = if let Some(lt) = rest.strip_prefix('\'') {
        let end = lt
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(lt.len());
        lt[end..].trim_start()
    } else {
        rest
    };
    rest == "str"
}

/// The binding name of a parameter, from the text before its `:` — `mut name`
/// yields `name`. `None` for a wildcard or anything that is not a plain ident
/// (a destructuring pattern cannot be forwarded by name).
fn param_ident(pat: &str) -> Option<String> {
    let last = pat.split_whitespace().next_back()?;
    if last == "_" || !last.bytes().all(is_ident_byte) {
        return None;
    }
    Some(last.to_string())
}

/// Is the first parameter a `self` receiver? Callers do not write it, so it
/// must not be counted when converting a signature position into an argument
/// position.
fn is_self_receiver(seg: &str) -> bool {
    let t = seg.trim().trim_start_matches('&').trim_start();
    let t = t
        .strip_prefix('\'')
        .map(|lt| {
            let end = lt
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(lt.len());
            lt[end..].trim_start()
        })
        .unwrap_or(t);
    let t = t.strip_prefix("mut ").unwrap_or(t).trim_start();
    t == "self" || t.starts_with("self:")
}

/// The `(start, end)` span of a fn body, given the offset just past its
/// parameter list. `None` for a brace-less declaration (a trait method
/// signature), which has no body to forward from.
fn fn_body_span(code: &[u8], lit_end_at: &[Option<usize>], from: usize) -> Option<(usize, usize)> {
    let mut depth = 0i32;
    let mut i = from;
    while i < code.len() {
        if let Some(end) = lit_end_at[i] {
            i = end;
            continue;
        }
        match code[i] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b';' if depth <= 0 => return None,
            b'{' if depth <= 0 => {
                let close = match_delim(code, lit_end_at, i)?;
                return Some((i + 1, close));
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Does `code[body]` call `eval_builtin(<param>, …)` — the identifier itself,
/// not a literal?
fn body_forwards(
    code: &[u8],
    lit_end_at: &[Option<usize>],
    body: (usize, usize),
    param: &str,
) -> bool {
    const CALLEE: &[u8] = b"eval_builtin";
    let (start, end) = body;
    let mut i = start;
    while i < end {
        if let Some(lit) = lit_end_at[i] {
            i = lit;
            continue;
        }
        if code[i] != CALLEE[0] || i + CALLEE.len() > end || &code[i..i + CALLEE.len()] != CALLEE {
            i += 1;
            continue;
        }
        let is_token = i == 0 || !is_ident_byte(code[i - 1]);
        let after = skip_ws(code, i + CALLEE.len());
        if is_token && after < end && code[after] == b'(' {
            let arg = skip_ws(code, after + 1);
            if let Some((id, arg_end)) = read_ident(code, arg) {
                let delim = skip_ws(code, arg_end);
                if id == param && delim < end && (code[delim] == b',' || code[delim] == b')') {
                    return true;
                }
            }
        }
        i += CALLEE.len();
    }
    false
}

/// Every fn in `code` that launders a `&str` parameter into `eval_builtin`.
fn find_forwarders(code: &[u8], lit_end_at: &[Option<usize>]) -> Vec<Forwarder> {
    let n = code.len();
    let mut out: Vec<Forwarder> = Vec::new();
    let mut i = 0usize;
    while i + 2 <= n {
        if code[i] != b'f' || code[i + 1] != b'n' {
            i += 1;
            continue;
        }
        let prev_ok = i == 0 || !is_ident_byte(code[i - 1]);
        let next = i + 2;
        if !prev_ok || (next < n && is_ident_byte(code[next])) {
            i += 1;
            continue;
        }
        // Past `fn`: every `continue` below has already made progress.
        i = next;
        let Some((fname, name_end)) = read_ident(code, skip_ws(code, i)) else {
            continue;
        };
        i = name_end;

        // Optional generics, then the parameter list.
        let mut k = skip_ws(code, name_end);
        if k < n && code[k] == b'<' {
            let mut d = 0i32;
            while k < n {
                match code[k] {
                    b'<' => d += 1,
                    b'>' if !(k > 0 && code[k - 1] == b'-') => {
                        d -= 1;
                        if d == 0 {
                            k += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            k = skip_ws(code, k);
        }
        if k >= n || code[k] != b'(' {
            continue;
        }
        let Some(close) = match_delim(code, lit_end_at, k) else {
            continue;
        };
        let params = split_delimited(code, lit_end_at, k, close, true);
        let Some(body) = fn_body_span(code, lit_end_at, close + 1) else {
            continue;
        };

        let receiver = params
            .first()
            .map(|&(s, e)| is_self_receiver(&String::from_utf8_lossy(&code[s..e])))
            .unwrap_or(false);

        for (idx, &(ps, pe)) in params.iter().enumerate() {
            if receiver && idx == 0 {
                continue;
            }
            let seg = String::from_utf8_lossy(&code[ps..pe]).into_owned();
            let Some((pat, ty)) = seg.split_once(':') else {
                continue;
            };
            if !is_str_ref(ty) {
                continue;
            }
            let Some(pname) = param_ident(pat) else {
                continue;
            };
            if body_forwards(code, lit_end_at, body, &pname) {
                out.push(Forwarder {
                    name: fname.clone(),
                    param: if receiver { idx - 1 } else { idx },
                    param_name: pname,
                });
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// [`find_forwarders`] over raw source text, applying the same comment and
/// `#[cfg(test)]` masking the classification pass uses — which is what keeps
/// test helpers such as `crates/reify-stdlib/src/complex.rs`'s
/// `assert_complex_builtin_undef(builtin: &str, …)` out of the forwarder set.
fn find_forwarders_in(src: &str) -> Vec<Forwarder> {
    let (mut code, lits) = strip_comments_and_collect_literals(src);
    mask_cfg_test_blocks(&mut code, &lits);
    let lit_end_at = literal_start_index(code.len(), &lits);
    find_forwarders(&code, &lit_end_at)
}

/// Byte offsets of the string literals sitting at a forwarder's laundered
/// argument position — i.e. the names that reach `eval_builtin` one hop later.
fn forwarded_literal_starts(
    code: &[u8],
    lit_end_at: &[Option<usize>],
    forwarders: &[Forwarder],
) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    if forwarders.is_empty() {
        return out;
    }
    let mut by_name: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
    for f in forwarders {
        by_name.entry(f.name.as_str()).or_default().insert(f.param);
    }

    let n = code.len();
    let mut i = 0usize;
    while i < n {
        if let Some(end) = lit_end_at[i] {
            i = end;
            continue;
        }
        if !is_ident_byte(code[i]) || (i > 0 && is_ident_byte(code[i - 1])) {
            i += 1;
            continue;
        }
        let Some((id, ident_end)) = read_ident(code, i) else {
            i += 1;
            continue;
        };
        i = ident_end;
        let Some(params) = by_name.get(id.as_str()) else {
            continue;
        };
        let k = skip_ws(code, ident_end);
        if k >= n || code[k] != b'(' {
            continue;
        }
        let Some(close) = match_delim(code, lit_end_at, k) else {
            continue;
        };
        let args = split_delimited(code, lit_end_at, k, close, false);
        for &p in params {
            let Some(&(s, e)) = args.get(p) else {
                continue;
            };
            let s = skip_ws(code, s);
            // A literal here, and not one blanked out by the test mask.
            if s < e && lit_end_at[s].is_some() && code[s] == b'"' {
                out.insert(s);
            }
        }
    }
    out
}

/// Classify ONE source text: the single classification path, shared by the
/// real sweep in [`scan`] and by the synthetic fixtures, so a fixture cannot
/// pin a rule the workspace sweep does not actually apply.
fn classify_text(
    rel: &str,
    src: &str,
    forwarders: &[Forwarder],
    seed_names: &BTreeSet<String>,
) -> Vec<Site> {
    let (mut code, lits) = strip_comments_and_collect_literals(src);
    mask_cfg_test_blocks(&mut code, &lits);
    let lit_end_at = literal_start_index(code.len(), &lits);
    let forwarded = forwarded_literal_starts(&code, &lit_end_at, forwarders);

    let mut sites = Vec::new();
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
        } else if forwarded.contains(&lit.start) {
            SiteKind::ForwardedEvalCall
        } else {
            continue;
        };
        sites.push(Site {
            file: rel.to_string(),
            line: line_of(&code, lit.start),
            name: lit.content.clone(),
            kind,
        });
    }
    sites
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
///
/// Two passes over `crates/*/src/`, because a forwarder may be called from
/// another file in its crate: pass one discovers every fn that launders a
/// `&str` into `eval_builtin`, pass two classifies with that table in hand.
fn scan(root: &Path, seed_names: &BTreeSet<String>) -> Vec<Site> {
    let sources = production_sources(root);

    let mut forwarders: Vec<Forwarder> = Vec::new();
    for path in &sources {
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        forwarders.extend(find_forwarders_in(&src));
    }
    forwarders.sort();
    forwarders.dedup();

    let mut sites = Vec::new();
    for path in &sources {
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        sites.extend(classify_text(&rel, &src, &forwarders, seed_names));
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

/// The cfg classification itself, pinned shape by shape — because the two ways
/// it can be wrong fail in OPPOSITE directions and only one of them is loud.
///
/// Masking too little costs a false positive, which a reviewer sees. Masking
/// too MUCH blanks production code out of the gate, which reports a clean
/// workspace over a residue it never looked at — the silent failure this whole
/// file exists to prevent. `#[cfg(not(test))]` is the case that bit: it is a
/// production-only guard, live in tree (`crates/reify-ir/src/sampled.rs`), and
/// the previous "does the token `test` appear anywhere" rule blanked it.
#[test]
fn cfg_attr_classification_masks_test_code_but_never_production_code() {
    // Masked: genuinely test-only.
    for attr in [
        "#[cfg(test)]",
        "#[cfg(any(test, feature = \"x\"))]",
        "#[cfg(all(test, unix))]",
        // Deliberate, not an accident of splitting "test-support" on `-`:
        // a `test*` feature gates test-support code. Both are live in tree
        // (reify-stdlib's `test-support`, reify-kernel-manifold's
        // `test-fixtures`).
        "#[cfg(feature = \"test-support\")]",
        "#[cfg(feature=\"test-fixtures\")]",
    ] {
        assert!(
            attr_gates_test_code(attr),
            "{attr} gates test-only code and must be masked out of the scan"
        );
    }

    // NOT masked: production code, or nothing to do with tests at all.
    for attr in [
        // The regression this test exists for — a production-only guard.
        "#[cfg(not(test))]",
        "#[cfg(all(not(test), unix))]",
        "#[cfg(any(not(test), feature = \"y\"))]",
        // Production when the test-support feature is OFF.
        "#[cfg(not(feature = \"test-support\"))]",
        // "test" only as a substring of a feature name.
        "#[cfg(feature = \"fastest\")]",
        // Not a `cfg` at all — `cfg_attr` adds attributes, it removes no code.
        "#[cfg_attr(test, derive(Debug))]",
        "#[derive(Debug)]",
        "#[cfg(unix)]",
    ] {
        assert!(
            !attr_gates_test_code(attr),
            "{attr} does NOT gate test-only code — masking it would blank \
             production code out of the I-REG-1 scan, which is the silent \
             failure mode: a clean report over a residue never looked at"
        );
    }
}

/// The `#[cfg(not(test))]` hole, end to end rather than at the predicate:
/// a dispatch site under a production-only guard must survive masking and be
/// FOUND, exactly as if the attribute were not there.
#[test]
fn production_only_cfg_not_test_items_stay_in_the_scan() {
    let src = r###"
#[cfg(not(test))]
fn production_dispatch(name: &str) -> u8 {
    match name {
        "von_mises" => 1,
        _ => 0,
    }
}

#[cfg(test)]
fn only_for_tests(name: &str) -> u8 {
    match name {
        "max_shear" => 1,
        _ => 0,
    }
}
"###;
    let (mut code, lits) = strip_comments_and_collect_literals(src);
    mask_cfg_test_blocks(&mut code, &lits);

    let names: BTreeSet<String> = ["von_mises", "max_shear"]
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
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        found,
        vec![("von_mises".to_string(), SiteKind::MatchArm)],
        "the `#[cfg(not(test))]` arm is PRODUCTION code and must be flagged; \
         only the `#[cfg(test)]` arm may be masked"
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
