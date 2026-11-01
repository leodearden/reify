//! **I-REG-1**, registry-scoped: `reify_builtins::lookup` is the only
//! string→builtin resolution in the workspace, so no `"<registered name>" =>`
//! match arm and no re-entrant `eval_builtin("<registered name>", …)` call may
//! live outside `crates/reify-builtins`.
//!
//! Task #6001 α, `docs/prds/v0_6/builtin-signature-registry.md` §7.2.
//!
//! # Scope: registry-scoped with a ledger, not workspace-wide
//!
//! This gate sweeps the REGISTERED names only, not every builtin name. The
//! residue it finds is REAL string dispatch on registered names, outside the
//! charter of the leaf that registered them — so it is COUNTED in
//! [`SEED_STRING_DISPATCH_LEDGER`], one entry per (file, name) pair with its
//! exact site count and a one-sentence WHY naming the leaf that owns
//! re-homing it, rather than hidden by narrowing the scan or "fixed" by
//! editing a crate the registering task does not own.
//! An UNLEDGERED site fails, naming the file, line and name. The
//! workspace-wide grep gate (every builtin name, not just the registered ones)
//! arrives at task ω.
//!
//! The name list is derived from [`reify_builtins::rows`], never restated —
//! mirroring `reify-compiler`'s `units.rs` disjointness list — so a row added
//! by a later τ migration is swept automatically instead of silently escaping.
//!
//! A row may be registered before its consumers migrate. Their string
//! dispatch on its name is then ledgered under the leaf that migrates them,
//! as is dispatch on a same-name builtin that is not that row at all. A leaf
//! that retires sites must shrink its entry's count, and a pair that grows a
//! site reds.
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
//!
//! One limitation runs the other way, over-reporting rather than missing: the
//! `#[cfg(test)]` mask is IN-FILE only. A test-only module declared out of line
//! in its parent (`#[cfg(test)] mod tests;`) is scanned as if it were
//! production code, so its name slices are reported as sites. Those are
//! ledgered as such (`WHY_OUT_OF_LINE_TEST_MODULE`) until the scanner resolves
//! out-of-line modules (ticket tkt_0RW1M0N2PGJKPYWNSHA35CJ5T5).

mod common;
use common::workspace_root;

/// The scanner. Policy lives here; lexical shape lives there.
#[path = "common/seed_name_scan.rs"]
mod seed_name_scan;
use seed_name_scan::{Site, scan};

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

// ── the ledger ──────────────────────────────────────────────────────────────

/// A known, accepted string-dispatch (file, name) pair on a registered name.
struct LedgerEntry {
    /// Workspace-relative path, `/`-separated.
    file: &'static str,
    /// The registered name dispatched on.
    name: &'static str,
    /// The exact number of occurrences the scan finds for this (file, name).
    sites: usize,
    /// Which leaf OWNS re-homing these sites, and why the registering leaf
    /// cannot.
    why: &'static str,
}

const EXPR_LIB: &str = "crates/reify-expr/src/lib.rs";
const EXPR_ANALYSIS: &str = "crates/reify-expr/src/analysis.rs";
const RELATION_SIGNATURES: &str = "crates/reify-compiler/src/relation_signatures.rs";
const CONSTRAINTS_RELATE_SOLVE: &str = "crates/reify-constraints/src/relate_solve.rs";
const EVAL_RELATE_SOLVE: &str = "crates/reify-eval/src/relate_solve.rs";
const COMPILER_UNITS: &str = "crates/reify-compiler/src/units.rs";
const STDLIB_GEOMETRY: &str = "crates/reify-stdlib/src/geometry.rs";
const EVAL_GEOMETRY_OPS: &str = "crates/reify-eval/src/geometry_ops.rs";
const EVAL_GEOMETRY_OPS_TESTS: &str = "crates/reify-eval/src/geometry_ops/tests.rs";
const EVAL_REGISTRY_DRIFT_TESTS: &str = "crates/reify-eval/src/registry_drift_tests.rs";

const WHY_FIELD_ARG_INTERCEPT: &str =
    "Field-arg intercept — needs BindingKind::ExprIntercept (τ-fea/analysis)";
const WHY_REENTRANT_EVAL_CALL: &str =
    "re-entrant eval_builtin call — needs CompiledExpr to carry BuiltinId (leaf β)";
const WHY_RELATION_VOCABULARY: &str =
    "relation vocabulary dispatch — S9 λ #8741 (RelationSig as a total fn over RelationKind)";
const WHY_RESIDUAL_DISPATCH: &str =
    "relate-solve `residual_dispatch` — S6 γ #8830 binds RelationKind exhaustively";
const WHY_DESCRIBE_DEMAND: &str =
    "relate-solve `describe_demand` — S6 γ #8830 binds RelationKind exhaustively";
const WHY_GEOMETRY_QUERY_DERIVE: &str =
    "GEOMETRY_QUERY_NAMES' arity-2 DERIVE form, a different builtin — τ8 rows in #6944";
const WHY_DATUM_OFFSET: &str =
    "arity-2 construction datum `offset(Plane, Length)`, a different builtin — τ7 rows in #6944";
const WHY_TOPOLOGY_SELECTOR_HELPER: &str =
    "TopologySelectorHelper string map — #6944 (fk-c02915c1952e)";
const WHY_OUT_OF_LINE_TEST_MODULE: &str = "out-of-line `#[cfg(test)]` module, not dispatch — the \
     scanner's cfg(test) mask is in-file only (ticket tkt_0RW1M0N2PGJKPYWNSHA35CJ5T5)";

/// One [`SEED_STRING_DISPATCH_LEDGER`] entry, written as a single line.
const fn ledgered(
    file: &'static str,
    name: &'static str,
    sites: usize,
    why: &'static str,
) -> LedgerEntry {
    LedgerEntry {
        file,
        name,
        sites,
        why,
    }
}

/// Every string-dispatch (file, name) pair on a registered name left standing,
/// one entry each, carrying its exact occurrence count.
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
///
/// Skipped by rustfmt so each `ledgered(..)` entry stays one line per pair;
/// rustfmt's call-width heuristic would otherwise break most of them over five.
#[rustfmt::skip]
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
    ledgered(EXPR_LIB, "von_mises", 1, WHY_FIELD_ARG_INTERCEPT),
    ledgered(EXPR_LIB, "principal_stresses", 1, WHY_FIELD_ARG_INTERCEPT),
    ledgered(EXPR_LIB, "max_shear", 1, WHY_FIELD_ARG_INTERCEPT),
    ledgered(EXPR_LIB, "safety_factor", 1, WHY_FIELD_ARG_INTERCEPT),
    // ── reify-expr's re-entrant call back into eval ─────────────────────────
    //
    // `compute_safety_factor` evaluates the field's inner lambda down to a
    // concrete tensor and then re-enters `eval_builtin` BY NAME. The string
    // cannot go until `CompiledExpr` carries a `BuiltinId` instead of a `&str`,
    // which is explicitly leaf β (PRD §9).
    ledgered(EXPR_ANALYSIS, "safety_factor", 1, WHY_REENTRANT_EVAL_CALL),
    // The other three arrive at `eval_builtin` ONE HOP LATER, through
    // `sample_unary_analysis_at_point(…, builtin_name: &str)`, which passes
    // the name straight through. Same root cause, same fix, same owning leaf:
    // once `CompiledExpr` carries a `BuiltinId`, the forwarder takes an id and
    // all four go together. Splitting them across leaves would misdirect the
    // leaf that has to do the work.
    ledgered(EXPR_ANALYSIS, "von_mises", 1, WHY_REENTRANT_EVAL_CALL),
    ledgered(EXPR_ANALYSIS, "principal_stresses", 1, WHY_REENTRANT_EVAL_CALL),
    ledgered(EXPR_ANALYSIS, "max_shear", 1, WHY_REENTRANT_EVAL_CALL),
    // ── the Relation rows' consumers, not yet migrated ──────────────────────
    //
    // The compiler's relation vocabulary: the name slice, the arity-gated resolver and
    // the per-name operand and ΔDOF tables. S9 λ retires them by reading every relation
    // signature off its row.
    ledgered(RELATION_SIGNATURES, "coincident", 3, WHY_RELATION_VOCABULARY),
    ledgered(RELATION_SIGNATURES, "on", 3, WHY_RELATION_VOCABULARY),
    ledgered(RELATION_SIGNATURES, "parallel", 4, WHY_RELATION_VOCABULARY),
    ledgered(RELATION_SIGNATURES, "antiparallel", 4, WHY_RELATION_VOCABULARY),
    ledgered(RELATION_SIGNATURES, "perpendicular", 4, WHY_RELATION_VOCABULARY),
    ledgered(RELATION_SIGNATURES, "concentric", 4, WHY_RELATION_VOCABULARY),
    ledgered(RELATION_SIGNATURES, "flush", 4, WHY_RELATION_VOCABULARY),
    ledgered(RELATION_SIGNATURES, "offset", 6, WHY_RELATION_VOCABULARY),
    ledgered(RELATION_SIGNATURES, "tangent", 3, WHY_RELATION_VOCABULARY),
    ledgered(RELATION_SIGNATURES, "fasten", 3, WHY_RELATION_VOCABULARY),
    ledgered(RELATION_SIGNATURES, "angle", 4, WHY_RELATION_VOCABULARY),
    ledgered(RELATION_SIGNATURES, "distance", 5, WHY_RELATION_VOCABULARY),
    //
    // The relate solve's per-relation residual dispatch: the runtime binding
    // `RelationKind` exists for (I-REG-2).
    ledgered(CONSTRAINTS_RELATE_SOLVE, "coincident", 1, WHY_RESIDUAL_DISPATCH),
    ledgered(CONSTRAINTS_RELATE_SOLVE, "on", 1, WHY_RESIDUAL_DISPATCH),
    ledgered(CONSTRAINTS_RELATE_SOLVE, "parallel", 1, WHY_RESIDUAL_DISPATCH),
    ledgered(CONSTRAINTS_RELATE_SOLVE, "antiparallel", 1, WHY_RESIDUAL_DISPATCH),
    ledgered(CONSTRAINTS_RELATE_SOLVE, "perpendicular", 1, WHY_RESIDUAL_DISPATCH),
    ledgered(CONSTRAINTS_RELATE_SOLVE, "concentric", 1, WHY_RESIDUAL_DISPATCH),
    ledgered(CONSTRAINTS_RELATE_SOLVE, "flush", 1, WHY_RESIDUAL_DISPATCH),
    ledgered(CONSTRAINTS_RELATE_SOLVE, "offset", 1, WHY_RESIDUAL_DISPATCH),
    ledgered(CONSTRAINTS_RELATE_SOLVE, "tangent", 1, WHY_RESIDUAL_DISPATCH),
    ledgered(CONSTRAINTS_RELATE_SOLVE, "fasten", 1, WHY_RESIDUAL_DISPATCH),
    ledgered(CONSTRAINTS_RELATE_SOLVE, "angle", 1, WHY_RESIDUAL_DISPATCH),
    ledgered(CONSTRAINTS_RELATE_SOLVE, "distance", 1, WHY_RESIDUAL_DISPATCH),
    //
    // The relate solve's per-relation demand description, retired with the
    // residual dispatch above. `fasten` has no arm here.
    ledgered(EVAL_RELATE_SOLVE, "coincident", 1, WHY_DESCRIBE_DEMAND),
    ledgered(EVAL_RELATE_SOLVE, "on", 1, WHY_DESCRIBE_DEMAND),
    ledgered(EVAL_RELATE_SOLVE, "parallel", 1, WHY_DESCRIBE_DEMAND),
    ledgered(EVAL_RELATE_SOLVE, "antiparallel", 1, WHY_DESCRIBE_DEMAND),
    ledgered(EVAL_RELATE_SOLVE, "perpendicular", 1, WHY_DESCRIBE_DEMAND),
    ledgered(EVAL_RELATE_SOLVE, "concentric", 1, WHY_DESCRIBE_DEMAND),
    ledgered(EVAL_RELATE_SOLVE, "flush", 1, WHY_DESCRIBE_DEMAND),
    ledgered(EVAL_RELATE_SOLVE, "offset", 1, WHY_DESCRIBE_DEMAND),
    ledgered(EVAL_RELATE_SOLVE, "tangent", 1, WHY_DESCRIBE_DEMAND),
    ledgered(EVAL_RELATE_SOLVE, "angle", 1, WHY_DESCRIBE_DEMAND),
    ledgered(EVAL_RELATE_SOLVE, "distance", 1, WHY_DESCRIBE_DEMAND),
    //
    // Different builtins that merely share a relation name: the arity-2 geometry
    // queries `angle` / `distance` and the arity-2 construction datum `offset`. They
    // become rows of their own in #6944, keyed apart from the relation rows by argc.
    ledgered(COMPILER_UNITS, "offset", 2, WHY_DATUM_OFFSET),
    ledgered(COMPILER_UNITS, "angle", 2, WHY_GEOMETRY_QUERY_DERIVE),
    ledgered(COMPILER_UNITS, "distance", 2, WHY_GEOMETRY_QUERY_DERIVE),
    ledgered(STDLIB_GEOMETRY, "offset", 2, WHY_DATUM_OFFSET),
    //
    // The topology-selector helper string map, which #6944 replaces with one
    // `BuiltinId`-keyed dispatch.
    ledgered(EVAL_GEOMETRY_OPS, "angle", 1, WHY_TOPOLOGY_SELECTOR_HELPER),
    ledgered(EVAL_GEOMETRY_OPS, "distance", 1, WHY_TOPOLOGY_SELECTOR_HELPER),
    //
    // Test-only modules declared out of line in their parent. Not dispatch at all:
    // see "What this scan still cannot see" in the module doc.
    ledgered(EVAL_GEOMETRY_OPS_TESTS, "angle", 3, WHY_OUT_OF_LINE_TEST_MODULE),
    ledgered(EVAL_GEOMETRY_OPS_TESTS, "distance", 1, WHY_OUT_OF_LINE_TEST_MODULE),
    ledgered(EVAL_REGISTRY_DRIFT_TESTS, "angle", 2, WHY_OUT_OF_LINE_TEST_MODULE),
    ledgered(EVAL_REGISTRY_DRIFT_TESTS, "distance", 2, WHY_OUT_OF_LINE_TEST_MODULE),
];

fn is_ledgered(file: &str, name: &str) -> bool {
    SEED_STRING_DISPATCH_LEDGER
        .iter()
        .any(|e| e.file == file && e.name == name)
}

fn registered_names() -> BTreeSet<String> {
    reify_builtins::rows()
        .iter()
        .map(|r| r.name.to_string())
        .collect()
}

/// The workspace sweep, run ONCE per test binary.
///
/// The gate tests below that read the tree share one sweep, and [`scan`] makes
/// two full passes over every `crates/*/src/**/*.rs` (forwarder discovery,
/// then classification), re-reading and re-lexing each file — so a per-test
/// call would cost two more full-workspace passes per test.
///
/// Memoising is sound because BOTH of `scan`'s inputs are fixed for the whole
/// binary: [`workspace_root`] is a constant path, and [`registered_names`] is derived
/// from the immutable `rows()` table, so there is no second key a later τ could
/// widen without widening the table itself. `OnceLock` also makes the shared
/// sweep safe under the test harness's default parallel threads.
///
/// The scanner's own fixture-driven tests deliberately do NOT go through this,
/// which is what keeps its behaviour pinned against synthetic sources rather
/// than against whatever the tree happens to contain.
fn workspace_sites() -> &'static [Site] {
    static SITES: OnceLock<Vec<Site>> = OnceLock::new();
    SITES.get_or_init(|| scan(&workspace_root(), &registered_names()))
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

/// Each (file, name) pair has exactly ONE ledger entry, and every entry
/// counts at least one site. Two entries for one pair would each be compared
/// against the pair's total occurrence count, so the count check below would
/// red on both or, with offsetting `sites`, on neither.
#[test]
fn every_ledger_entry_is_the_only_entry_for_its_pair_and_counts_a_site() {
    let mut seen = BTreeSet::new();
    for e in SEED_STRING_DISPATCH_LEDGER {
        assert!(
            seen.insert((e.file, e.name)),
            "SEED_STRING_DISPATCH_LEDGER has two entries for ({}, {:?}) \u{2014} \
             merge them into one entry whose `sites` is the pair's total",
            e.file,
            e.name
        );
        assert!(
            e.sites > 0,
            "SEED_STRING_DISPATCH_LEDGER entry ({}, {:?}) counts zero sites \u{2014} \
             a retired pair is deleted, not zeroed",
            e.file,
            e.name
        );
    }
}

/// The ledger is a RATCHET, not a suppression list: each entry's `sites` must
/// EQUAL the number of occurrences the scan finds for its (file, name) pair.
///
/// The primary gate above is keyed on the pair, so it accepts any number of
/// occurrences of a ledgered pair. This check is the only place an added or
/// removed occurrence surfaces, and the two directions are different defects
/// with different fixes:
///
/// - MORE occurrences than `sites` is NEW RESIDUE on an already-ledgered pair;
/// - FEWER is either RETIRED residue, so the ledger must shrink, or a SCANNER
///   regression that stopped seeing a site still in the tree, which would let
///   the gate report a clean workspace over untouched residue. A pair found
///   zero times is the limiting case of either.
#[test]
fn every_ledger_entry_counts_its_pairs_occurrences_exactly() {
    let mut by_pair: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
    for site in workspace_sites() {
        by_pair
            .entry((site.file.as_str(), site.name.as_str()))
            .or_default()
            .push(site.line);
    }
    let mut grew = Vec::new();
    let mut shrank = Vec::new();
    for e in SEED_STRING_DISPATCH_LEDGER {
        let lines = by_pair.get(&(e.file, e.name)).cloned().unwrap_or_default();
        let line = format!(
            "  {} \u{2014} {:?}: expected {}, found {} at lines {lines:?} ({})",
            e.file,
            e.name,
            e.sites,
            lines.len(),
            e.why
        );
        if lines.len() > e.sites {
            grew.push(line);
        } else if lines.len() < e.sites {
            shrank.push(line);
        }
    }
    assert!(
        grew.is_empty(),
        "{} already-ledgered (file, name) pair(s) grew an ADDITIONAL dispatch \
         site \u{2014} new residue. The primary gate accepts these silently \
         because the ledger is keyed on (file, name), so they are named here \
         instead: re-home them onto `BuiltinId`, or raise the entry's `sites` \
         and widen its WHY so the addition is reviewed.\n{}",
        grew.len(),
        grew.join("\n")
    );
    assert!(
        shrank.is_empty(),
        "{} ledgered (file, name) pair(s) have FEWER occurrences than their \
         `sites` count. If the residue was retired, the ledger must shrink: \
         lower `sites`, or delete the entry once nothing is left. If the sites \
         are still in the file, the scanner has stopped seeing them, and that \
         regression is what must be fixed.\n{}",
        shrank.len(),
        shrank.join("\n")
    );
}
