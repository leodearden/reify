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
//! Five shapes. The first three are observed in the tree; shapes 4 and 5 have
//! no live instance today and are closed pre-emptively, because "the gate
//! reports a clean workspace" must not be readable as stronger than the shapes
//! it can actually see — and shape 5 in particular is LITERALLY the dispatch
//! form this task deleted, so a new crate could reintroduce it verbatim.
//!
//! 1. `MatchArm` — a string literal equal to a seed name in
//!    PATTERN position, i.e. followed by `=>` (possibly through `|`
//!    or-pattern alternatives and an `if` guard). This is the shape the PRD
//!    names.
//! 2. `EvalCall` — a seed name passed as the first argument of an
//!    `eval_builtin(…)` call. Not a match arm, but the same defect: a builtin
//!    identified by a hard-coded string rather than by its `BuiltinId`.
//! 3. `ForwardedEvalCall` — a seed name passed to a helper that
//!    itself calls `eval_builtin(<that very parameter>, …)`. Shape 2 one hop
//!    later, and a real hole: `reify-expr`'s
//!    `sample_unary_analysis_at_point(…, builtin_name: &str)` launders three
//!    of the four analysis names this way, so a lexical "literal adjacent to
//!    `eval_builtin(`" rule certifies a residue it cannot see.
//!
//!    The forwarder set is DERIVED, never declared (see `Forwarder`): a fn
//!    qualifies only if its own body dispatches on the parameter. That
//!    distinction is load-bearing rather than pedantic — `wrap_tensor_field(…,
//!    op: &str, …)` and `validate_tensor_field(…, op: &str)` sit in the same
//!    file and take a seed name purely as a diagnostic label, so an "any
//!    `&str` param" rule would inflate the ledger with entries that name no
//!    dispatch at all.
//!
//! 4. `EqualityTest` — a seed name compared with `==` / `!=`
//!    (`if name == "von_mises" { … }`), on either side of the operator. The
//!    match arm of shape 1 written as an `if`, and lexically invisible to it:
//!    `match_arm_head` walks forward from the literal, hits `{` or an ident
//!    and DECIDES `NotArm`, so nothing downstream looks again. See
//!    `is_equality_operand`.
//!
//! 5. `NameList` — a seed name inside an array/slice of string
//!    literals, i.e. `const PARSE_FN_NAMES: &[&str] = &["parse_length", …]`
//!    paired with a `.contains(&name)` membership test somewhere else. That is
//!    the exact `PARSE_FN_NAMES` / `ANALYSIS_FN_NAMES` mechanism α replaced,
//!    and `reify-compiler`'s `units.rs` registry-vs-legacy disjointness test
//!    covers only the slices that EXIST — a new one in a new crate would be
//!    invisible to it. Flagged at the literals' definition site, which is the
//!    only place the shape is lexically visible at all. See
//!    `is_string_array_element`.
//!
//! 6. `UnresolvedArmHead` — not a shape at all, but the scanner
//!    admitting it could not decide: the arm-head walk reached end of source
//!    without resolving `=>` (arm) or a non-pattern token (not an arm), and
//!    nothing else claimed the literal. Reported rather than dropped, on the
//!    same reasoning as the `#[cfg(not(test))]` case below — the two ways this
//!    file can be wrong fail in OPPOSITE directions, and only over-reporting
//!    is loud. Under-reporting certifies a clean workspace over residue the
//!    scan never looked at. No site in the tree is classified this way today.
//!
//! # What deliberately does NOT count
//!
//! - **Anything under `#[cfg(test)]`.** A test may legitimately name a builtin
//!   as a string — that is how you write a call-site regression pin — and a
//!   NEGATIVE assertion such as `crates/reify-compiler/src/units.rs`'s
//!   `!is_fea_envelope_query("von_mises")` asserts the ABSENCE of a claim, the
//!   exact opposite of dispatch. Flagging either would make the gate punish
//!   test coverage. Test-gated blocks are therefore masked out before the scan
//!   (see `mask_cfg_test_blocks`). "Test-gated" means `#[cfg(test)]`,
//!   `#[cfg(any(test, …))]` and a `feature` whose name starts with `test`
//!   (`test-support`, `test-fixtures`) — but NOT `#[cfg(not(test))]`, which is
//!   a production-only guard and stays in the scan. That distinction is
//!   `attr_gates_test_code`'s, and it is the one `reify-audit`'s
//!   `p2_consumer_stub::is_test_cfg_attr` already makes for the same reason.
//! - **Comments and raw strings.** Prose naming a builtin is not dispatch, and
//!   an `r#"…"#` block in a `src/` file is embedded `.ri` fixture text, not
//!   Rust pattern syntax.
//! - **`crates/reify-builtins` itself**, which is where the one legal
//!   string→builtin table lives.
//! - **Anything outside `crates/*/src/`** — `tests/`, `benches/`, `examples/`
//!   and the GUI's TypeScript are out of the seed gate's remit.
//!
//! # What this scan still cannot see
//!
//! Stated rather than left to be inferred from the absence of findings, so a
//! later leaf does not read the gate as stronger than it is. Every entry below
//! is a shape a determined author could use to dispatch on a seed name without
//! tripping any rule above; none has a live instance today, and closing them
//! is workspace-wide task ω's business, not the seed gate's:
//!
//! - **A macro that takes patterns.** `matches!(name, "von_mises")` puts the
//!   literal in pattern position without a `=>` anywhere, so `match_arm_head`
//!   decides `NotArm`. Same for any macro with match-like arms.
//! - **A name that is never a literal at the dispatch site**: built by
//!   `format!`, read from a const in another crate, or compared through a
//!   binding (`let n = SOME_CONST; if name == n`). Purely lexical rules cannot
//!   follow a value; only shape 3's one-hop `&str` forwarding is chased, and
//!   only one hop.
//! - **String methods other than `==`**: `name.starts_with("von_mises")`,
//!   `name.eq("von_mises")`. Not swept because the method surface is open
//!   ended; the array shape (5) covers the collection form these usually take.

mod common;
use common::workspace_root;

/// The scanner. Policy lives here; lexical shape lives there.
#[path = "common/seed_name_scan.rs"]
mod seed_name_scan;
use seed_name_scan::{Site, scan};

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

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

fn seed_names() -> BTreeSet<String> {
    reify_builtins::rows()
        .iter()
        .map(|r| r.name.to_string())
        .collect()
}

/// The workspace sweep, run ONCE per test binary.
///
/// The three gate tests below all need the same sweep, and [`scan`] makes two
/// full passes over every `crates/*/src/**/*.rs` (forwarder discovery, then
/// classification), re-reading and re-lexing each file — so a per-test call
/// meant six full-workspace sweeps per binary.
///
/// Memoising is sound because BOTH of `scan`'s inputs are fixed for the whole
/// binary: [`workspace_root`] is a constant path, and [`seed_names`] is derived
/// from the immutable `rows()` table, so there is no second key a later τ could
/// widen without widening the table itself. `OnceLock` also makes the shared
/// sweep safe under the test harness's default parallel threads.
///
/// The scanner's own fixture-driven tests deliberately do NOT go through this,
/// which is what keeps its behaviour pinned against synthetic sources rather
/// than against whatever the tree happens to contain.
fn workspace_sites() -> &'static [Site] {
    static SITES: OnceLock<Vec<Site>> = OnceLock::new();
    SITES.get_or_init(|| scan(&workspace_root(), &seed_names()))
}

// ── the gate ────────────────────────────────────────────────────────────────

#[test]
fn no_unledgered_seed_name_string_dispatch_outside_reify_builtins() {
    let sites = workspace_sites();

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
    let sites = workspace_sites();

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
/// residue sat untouched.
///
/// # Two granularities, two assertions
///
/// The ledger models `(file, name)` PAIRS — deliberately de-lined, and
/// [`is_ledgered`] keys on exactly that pair. [`scan`] returns per-OCCURRENCE
/// sites. The two coincide today (one occurrence per pair), but a `sites.len()
/// == LEDGER.len()` comparison would CONFLATE the two ways they can diverge,
/// which are opposite defects with opposite fixes:
///
/// - a pair the scan no longer sees is a SCANNER regression — the gate goes
///   quietly vacuous;
/// - a pair that grew a SECOND occurrence is NEW RESIDUE, which the primary
///   gate accepts silently precisely because `is_ledgered` is pair-keyed.
///
/// So each is asserted on its own, with a message naming its own cause.
#[test]
fn the_scan_still_finds_every_ledgered_site() {
    let sites = workspace_sites();

    // (a) Pair-level agreement — the granularity the ledger actually models.
    let found: BTreeSet<(&str, &str)> = sites
        .iter()
        .map(|s| (s.file.as_str(), s.name.as_str()))
        .collect();
    let ledgered: BTreeSet<(&str, &str)> = SEED_STRING_DISPATCH_LEDGER
        .iter()
        .map(|e| (e.file, e.name))
        .collect();
    assert_eq!(
        found, ledgered,
        "the scan's (file, name) set must equal the ledger's. A pair MISSING \
         from the scan is a scanner regression — the gate would report a clean \
         workspace over untouched residue. An EXTRA pair is an unledgered \
         site, reported with file/line/kind by \
         no_unledgered_seed_name_string_dispatch_outside_reify_builtins.\n\n\
         Sites found:\n{sites:#?}"
    );

    // (b) Occurrence-level: a ledgered pair must not quietly grow a second
    //     dispatch site. Nothing else in this file would notice — the primary
    //     gate is pair-keyed by design, so this is the only place the addition
    //     surfaces.
    let mut by_pair: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
    for site in sites {
        by_pair
            .entry((site.file.as_str(), site.name.as_str()))
            .or_default()
            .push(site.line);
    }
    let duplicated: Vec<String> = by_pair
        .iter()
        .filter(|(_, lines)| lines.len() > 1)
        .map(|((file, name), lines)| format!("  {file} \u{2014} {name:?} at lines {lines:?}"))
        .collect();
    assert!(
        duplicated.is_empty(),
        "{} already-ledgered (file, name) pair(s) grew an ADDITIONAL dispatch \
         site \u{2014} new residue, not a scanner regression. The primary gate \
         accepts these silently because the ledger is keyed on (file, name), \
         so they are named here instead: re-home them onto `BuiltinId`, or \
         widen the ledger entry's WHY so the addition is reviewed.\n{}",
        duplicated.len(),
        duplicated.join("\n")
    );
}
