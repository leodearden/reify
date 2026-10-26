//! The jcodemunch seam: the [`JCodemunchOps`] trait the symbol-level detectors query source introspection through, the symbol shapes it returns, its inert [`NoopJCodemunchOps`], and the test-support `MockJCodemunchOps`.

use serde::{Deserialize, Serialize};
#[cfg(any(test, feature = "test-support"))]
use std::collections::HashMap;

/// The opt-outs a declaration carries, as READ by the suppression-enrichment
/// pass (`jcodemunch_client`'s `extract_suppression`). Reaches a detector only
/// inside [`ChangedSymbol::suppression`], whose `Option` carries the separate
/// question of whether the declaration was located at all.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclSuppression {
    /// `true` when the declaration carries `#[allow(dead_code)]` — an
    /// intentional-orphan opt-out (suppresses the finding). Per
    /// `f-infra-design.md` §5 P1.
    pub has_allow_dead_code: bool,
    /// `true` when the declaration is `#[cfg(test)]`-gated (test-only symbol;
    /// suppresses the finding). Per `f-infra-design.md` §5 P1.
    pub has_cfg_test: bool,
    /// The reason text of a `// G-allow:` marker on the declaration, if any.
    /// A `Some` with non-blank content suppresses the finding; `Some("")` /
    /// whitespace does NOT (mirrors `scripts/audit-orphan-producers.sh`'s
    /// `G_ALLOW_RE` pattern, `//\s*G-allow:\s*(.+)`, where `(.+)` requires
    /// content).
    pub g_allow_marker: Option<String>,
}

impl DeclSuppression {
    /// `true` when the declaration carries ANY of the opt-outs above — the
    /// single home of the rule P1 and P5 H2 both apply, so the two cannot
    /// drift. Per `f-infra-design.md` §5 P1/P5.
    // G-allow: single home of the opt-out rule; callers are intra-crate (is_symbol_suppressed) — orphan-audit script counts only inter-crate call sites
    pub fn opts_out(&self) -> bool {
        self.has_allow_dead_code
            || self.has_cfg_test
            || self
                .g_allow_marker
                .as_deref()
                .is_some_and(|r| !r.trim().is_empty())
    }
}

/// A public symbol introduced (or changed) by a `done` task, as reported by
/// `mcp__jcodemunch__get_changed_symbols`. Carries pre-extracted suppression
/// metadata so the detector stays pure-logic (it never reads source files —
/// symmetric with how [`GitOps::diff_added_lines`](crate::GitOps::diff_added_lines) pre-extracts strings).
/// Per `f-infra-design.md` §3 and §5 P1.
///
/// Suppression is THREE-state, and [`decl_located`](Self::decl_located) is the
/// state a consumer branches on FIRST: `None` means the declaration was never
/// located, so no opt-out judgement was possible; a default
/// [`DeclSuppression`] means it was read and carries none; one with a flag set
/// means it was read and opted out. All three causes of `None` — the wire
/// reported no `line`, the line is past the declaring file's current end
/// (a stale index), or the declaring file could not be read — are carried
/// here per symbol, not merely summarised to the operator on stderr by the
/// enrichment pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedSymbol {
    /// The symbol's name, used as the key for [`JCodemunchOps::find_references`].
    pub name: String,
    /// Workspace-relative path of the file declaring the symbol.
    pub file: String,
    /// 1-based line of the declaration (forensic evidence locator) WHEN
    /// the wire reports one. `0` is the sentinel for "not reported",
    /// mirroring [`SymbolReference::line`]: a `get_changed_symbols` payload
    /// that omits the `line` column still yields the symbol, located at
    /// `0`, rather than dropping it. The sentinel is for forensic display
    /// only — it covers just one of the three ways a declaration goes
    /// unlocatable, so a consumer asks
    /// [`decl_located`](Self::decl_located), never `line == 0`.
    pub line: usize,
    /// The opt-outs this symbol's declaration carries, or `None` when the
    /// declaration was never located — see the three-state note on the struct.
    pub suppression: Option<DeclSuppression>,
}

impl ChangedSymbol {
    /// `true` when the suppression-enrichment pass actually read this symbol's
    /// declaration, so [`suppression`](Self::suppression) is a judgement about
    /// the source rather than an absence of one.
    ///
    /// `false` reports a degradation of the jcodemunch substrate, never a
    /// statement about the code: the wire reported no `line` (the grammar
    /// drift release 1.108.54 already shipped for `find_references`), the
    /// reported line is past the declaring file's current end (a stale index),
    /// or the declaring file could not be read. A consumer that skips this
    /// question and reads `None` as "the author declined every opt-out" turns
    /// any of the three into a false positive over every intentionally
    /// suppressed symbol.
    // G-allow: accessor on the public ChangedSymbol API; every caller is intra-crate or a test — orphan-audit script counts only inter-crate call sites
    pub fn decl_located(&self) -> bool {
        self.suppression.is_some()
    }

    /// `true` when this symbol's declaration was located AND carries one of
    /// [`DeclSuppression`]'s opt-outs — the `Option`-lifting of
    /// [`DeclSuppression::opts_out`] onto a symbol, so no detector rewrites
    /// it inline.
    ///
    /// An unlocatable declaration answers `false`: it made no opt-out claim
    /// either way. That is NOT permission to report the symbol — ask
    /// [`decl_located`](Self::decl_located) first; this answers only "did
    /// the author opt out".
    // G-allow: accessor on the public ChangedSymbol API; every caller is intra-crate or a test — orphan-audit script counts only inter-crate call sites
    pub fn opts_out(&self) -> bool {
        self.suppression
            .as_ref()
            .is_some_and(DeclSuppression::opts_out)
    }
}

/// A non-declaration reference (caller site) of a symbol, as reported by
/// `mcp__jcodemunch__find_references`. P1 filters these to non-test paths to
/// decide whether a workspace consumer exists. Per `f-infra-design.md` §5 P1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolReference {
    /// Workspace-relative path of the referencing file.
    pub file: String,
    /// 1-based line of the reference WHEN the wire reports one. `0` is the
    /// sentinel for "not reported": jcodemunch's `find_references` records
    /// carry only `file`/`specifier`/`match_type`, no line number.
    pub line: usize,
}

/// A symbol with no callers above a given confidence, as reported by
/// `mcp__jcodemunch__get_dead_code_v2`. Mirrors the jcodemunch tool's response
/// shape for use by the L-PDEAD detector leaf.
///
/// Note: `confidence` is `f64` (IEEE 754), so `Eq` and `Hash` are intentionally
/// NOT derived — floating-point equality semantics are unsuitable for collection
/// key use. This deviates from [`ChangedSymbol`]'s derive set by design.
/// Per `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeadSymbol {
    /// Unique identifier for the symbol as assigned by jcodemunch.
    pub id: String,
    /// The symbol's declared name.
    pub name: String,
    /// The kind of symbol (e.g. `"function"`, `"struct"`, `"const"`).
    pub kind: String,
    /// Workspace-relative path of the file declaring the symbol.
    pub file: String,
    /// 1-based line of the declaration WHEN the wire reports one. `0` is the
    /// sentinel for "not reported", mirroring [`ChangedSymbol::line`] and
    /// [`SymbolReference::line`]: a `get_dead_code_v2` payload that omits the
    /// `line` column still yields the symbol, located at `0`, rather than
    /// dropping it from the PDEAD sweep.
    pub line: usize,
    /// Jcodemunch's confidence score that the symbol is truly unreachable
    /// (0.0 = uncertain; 1.0 = certain). Filtered by `min_confidence` in
    /// [`JCodemunchOps::get_dead_code`].
    pub confidence: f64,
    /// Diagnostic signals contributing to the confidence score
    /// (e.g. `"no_callers"`, `"private_module"`).
    pub signals: Vec<String>,
}

/// A symbol not reached by any test, as reported by
/// `mcp__jcodemunch__get_untested_symbols`. Mirrors the jcodemunch tool's
/// response shape for use by the L-PUNTESTED detector leaf.
///
/// Note: `confidence` is `f64` — `Eq`/`Hash` not derived. See [`DeadSymbol`].
/// Per `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UntestedSymbol {
    /// Unique identifier for the symbol as assigned by jcodemunch.
    pub symbol_id: String,
    /// The symbol's declared name.
    pub name: String,
    /// Workspace-relative path of the file declaring the symbol.
    pub file: String,
    /// `false` when no test path reaches the symbol.
    pub reached: bool,
    /// Confidence score (0.0–1.0) that the symbol is genuinely untested.
    /// Filtered by `min_confidence` in [`JCodemunchOps::get_untested_symbols`].
    pub confidence: f64,
}

/// A layer-violation: an import that is forbidden by the project's layering
/// rules, as reported by `mcp__jcodemunch__get_layer_violations`. Mirrors the
/// jcodemunch tool's response shape for use by the L-PLAYER detector leaf.
///
/// Per `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerViolation {
    /// Workspace-relative path of the file containing the violating import.
    pub from_file: String,
    /// Workspace-relative path of the file (or crate root) being imported.
    pub to_file: String,
    /// Human-readable name of the layer rule that was violated
    /// (e.g. `"gui-must-not-depend-on-kernel"`).
    pub rule: String,
}

/// A [`JCodemunchOps`] query that failed outright, as distinct from one that
/// answered with nothing. Carried by every [`JCodemunchOps`] method's `Err`
/// so a caller can report what it could not examine instead of reading the
/// failure as empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JCodemunchCallFailed {
    /// The jcodemunch tool whose call failed, e.g. `"get_changed_symbols"`.
    pub tool: &'static str,
    /// The client's description of the failure (transport, timeout, or an
    /// error envelope).
    pub detail: String,
}

impl std::fmt::Display for JCodemunchCallFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "jcodemunch {} call failed: {}", self.tool, self.detail)
    }
}

impl std::error::Error for JCodemunchCallFailed {}

/// Source-introspection operations the P1 detector needs. Production: a
/// jcodemunch-MCP-backed impl supplied by the T-4 CLI. Tests:
/// [`MockJCodemunchOps`] (gated behind `feature = "test-support"`) holds
/// canned answers keyed on `(since_sha, until_sha)` for changed-symbol
/// queries and `(file, name)` for reference queries, enabling per-commit and
/// declaration-site disambiguation. Per `f-infra-design.md` §3 and §5 P1.
///
/// Object-safe by design — `AuditContext` holds `&'a dyn JCodemunchOps` so
/// the production and mock impls coexist behind the same vtable (mirrors
/// [`GitOps`](crate::GitOps)).
///
/// A query's `Ok` is the serve's answer, possibly empty. Its `Err`
/// ([`JCodemunchCallFailed`]) means the call itself failed — unreachable
/// mid-sweep, timed out, an error envelope — and must never be read as an
/// empty answer: whatever the query was about went unexamined, and the
/// caller reports it as such.
pub trait JCodemunchOps {
    /// Equivalent of `mcp__jcodemunch__get_changed_symbols(since_sha, until_sha)`
    /// (jcodemunch v1.108.27+): the public symbols introduced/changed in the
    /// commit range `since_sha..until_sha`. Typically `since_sha = "{commit}^1"`
    /// and `until_sha = "{commit}"` for a single merged commit (mirrors the
    /// `^1..commit` convention from `RealGitOps::diff_added_lines_in_commit`).
    ///
    /// An empty vec means the range introduced nothing (or its commits are
    /// not in the index).
    fn get_changed_symbols(
        &self,
        since_sha: &str,
        until_sha: &str,
    ) -> Result<Vec<ChangedSymbol>, JCodemunchCallFailed>;

    /// Equivalent of `mcp__jcodemunch__find_references(identifier = symbol.name)`:
    /// the workspace references to `symbol`, where each
    /// [`SymbolReference::file`] is the REFERENCING site.
    ///
    /// Impls MUST NOT narrow the result by the referencing file. jcodemunch's
    /// lookup is import-based, so the declaring file is never in it, and a
    /// cross-file caller is exactly the evidence P1 and P5 H2 read as "any
    /// non-test reference" (#6504).
    ///
    /// Impls MAY drop references to same-named symbols declared elsewhere
    /// when their substrate can tell them apart (`MockJCodemunchOps` keys on
    /// `(file, name)`). The jcodemunch-backed impl cannot: its lookup is
    /// name-based with no file parameter, so it over-approximates, counting
    /// a same-named symbol's importer as a caller. That is a false-negative
    /// bias, accepted for a low-false-positive detector.
    ///
    /// Returns an empty vec when the symbol has no callers (an orphan
    /// candidate). Per `f-infra-design.md` §5 P1.
    fn find_references(
        &self,
        symbol: &ChangedSymbol,
    ) -> Result<Vec<SymbolReference>, JCodemunchCallFailed>;

    /// Equivalent of `mcp__jcodemunch__get_dead_code_v2(min_confidence)`:
    /// public symbols with no callers whose confidence score meets or exceeds
    /// `min_confidence` (0.0–1.0). Returns an empty vec when none found.
    /// Per `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §4-b.
    fn get_dead_code(&self, min_confidence: f64) -> Result<Vec<DeadSymbol>, JCodemunchCallFailed>;

    /// Equivalent of `mcp__jcodemunch__get_untested_symbols(min_confidence)`:
    /// symbols not reached by any test whose confidence score meets or exceeds
    /// `min_confidence`. Returns an empty vec when none found. Per PRD §4-b.
    fn get_untested_symbols(
        &self,
        min_confidence: f64,
    ) -> Result<Vec<UntestedSymbol>, JCodemunchCallFailed>;

    /// Equivalent of `mcp__jcodemunch__get_layer_violations()`: all detected
    /// imports that violate the project's layering rules. Returns an empty vec
    /// when none found. Per PRD §4-b.
    fn get_layer_violations(&self) -> Result<Vec<LayerViolation>, JCodemunchCallFailed>;
}

/// Inert [`JCodemunchOps`] — every query answers "nothing".
///
/// Its empty answers are not statements about the code: no query was made.
/// Unlike [`MockJCodemunchOps`] this is NOT test-support: it is the
/// production binding whenever a run has no live seam, and it is ungated for
/// exactly that reason. Its production call sites:
///
/// 1. `--no-jcodemunch` — the explicit offline escape hatch: P1 runs and
///    produces zero findings without opening a socket.
/// 2. Detector runs that never touch the seam (`needs_jcodemunch() == false`).
/// 3. The fallback for a needed seam that is unreachable, refused the
///    handshake, or sits over an unusable index
///    ([`jcodemunch_seam::Seam::Unavailable`](crate::jcodemunch_seam::Seam::Unavailable)). The bin records the affected
///    detectors as unexamined in that case, so their silence is not read as
///    a clean result.
/// 4. The `*-baseline-gen` structural censuses, which still have to populate
///    [`AuditContext`](crate::AuditContext)'s field.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopJCodemunchOps;

impl JCodemunchOps for NoopJCodemunchOps {
    fn get_changed_symbols(
        &self,
        _since_sha: &str,
        _until_sha: &str,
    ) -> Result<Vec<ChangedSymbol>, JCodemunchCallFailed> {
        Ok(vec![])
    }
    fn find_references(
        &self,
        _symbol: &ChangedSymbol,
    ) -> Result<Vec<SymbolReference>, JCodemunchCallFailed> {
        Ok(vec![])
    }
    fn get_dead_code(&self, _min_confidence: f64) -> Result<Vec<DeadSymbol>, JCodemunchCallFailed> {
        Ok(vec![])
    }
    fn get_untested_symbols(
        &self,
        _min_confidence: f64,
    ) -> Result<Vec<UntestedSymbol>, JCodemunchCallFailed> {
        Ok(vec![])
    }
    fn get_layer_violations(&self) -> Result<Vec<LayerViolation>, JCodemunchCallFailed> {
        Ok(vec![])
    }
}

/// HashMap-backed [`JCodemunchOps`] for tests. Gated behind
/// `feature = "test-support"` so it never pollutes the production public API
/// (mirrors [`MockGitOps`](crate::MockGitOps)). The crate self-pulls this feature in its own
/// `[dev-dependencies]` so integration tests in `tests/p1.rs` see it.
///
/// Changed-symbol queries are keyed on `(since_sha, until_sha)` (the
/// commit-range surface per jcodemunch v1.108.27+); reference queries are
/// keyed on `(file, name)` for declaration-site disambiguation, which the
/// [`JCodemunchOps::find_references`] contract permits but the
/// jcodemunch-backed impl cannot provide.
/// Dead-code / untested-symbol data is stored as a flat Vec and filtered by
/// `min_confidence` at query time (mirrors the real tool's semantics).
///
/// Every canned answer is a `Result`: a `set_*` setter stores the serve's
/// answer, a `fail_*` setter makes that call fail, and a query nothing was
/// set for answers `Ok(vec![])`.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug)]
pub struct MockJCodemunchOps {
    get_changed_symbols:
        HashMap<(String, String), Result<Vec<ChangedSymbol>, JCodemunchCallFailed>>,
    find_references: HashMap<(String, String), Result<Vec<SymbolReference>, JCodemunchCallFailed>>,
    dead_code: Result<Vec<DeadSymbol>, JCodemunchCallFailed>,
    untested: Result<Vec<UntestedSymbol>, JCodemunchCallFailed>,
    layer_violations: Result<Vec<LayerViolation>, JCodemunchCallFailed>,
    // Records the min_confidence last passed to get_dead_code() so tests can
    // assert the detector passes the intended threshold to the seam.
    last_dead_code_min_confidence: std::cell::Cell<Option<f64>>,
}

#[cfg(any(test, feature = "test-support"))]
impl Default for MockJCodemunchOps {
    fn default() -> Self {
        Self {
            get_changed_symbols: HashMap::new(),
            find_references: HashMap::new(),
            dead_code: Ok(vec![]),
            untested: Ok(vec![]),
            layer_violations: Ok(vec![]),
            last_dead_code_min_confidence: std::cell::Cell::new(None),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl MockJCodemunchOps {
    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn new() -> Self {
        Self::default()
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_changed_symbols(
        &mut self,
        since_sha: &str,
        until_sha: &str,
        symbols: Vec<ChangedSymbol>,
    ) {
        self.get_changed_symbols
            .insert((since_sha.to_string(), until_sha.to_string()), Ok(symbols));
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn fail_changed_symbols(&mut self, since_sha: &str, until_sha: &str, detail: &str) {
        self.get_changed_symbols.insert(
            (since_sha.to_string(), until_sha.to_string()),
            Err(Self::canned_failure("get_changed_symbols", detail)),
        );
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_find_references(&mut self, file: &str, name: &str, refs: Vec<SymbolReference>) {
        self.find_references
            .insert((file.to_string(), name.to_string()), Ok(refs));
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn fail_find_references(&mut self, file: &str, name: &str, detail: &str) {
        self.find_references.insert(
            (file.to_string(), name.to_string()),
            Err(Self::canned_failure("find_references", detail)),
        );
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_dead_code(&mut self, symbols: Vec<DeadSymbol>) {
        self.dead_code = Ok(symbols);
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn fail_dead_code(&mut self, detail: &str) {
        self.dead_code = Err(Self::canned_failure("get_dead_code_v2", detail));
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn last_dead_code_min_confidence(&self) -> Option<f64> {
        self.last_dead_code_min_confidence.get()
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_untested_symbols(&mut self, symbols: Vec<UntestedSymbol>) {
        self.untested = Ok(symbols);
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn fail_untested_symbols(&mut self, detail: &str) {
        self.untested = Err(Self::canned_failure("get_untested_symbols", detail));
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_layer_violations(&mut self, violations: Vec<LayerViolation>) {
        self.layer_violations = Ok(violations);
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn fail_layer_violations(&mut self, detail: &str) {
        self.layer_violations = Err(Self::canned_failure("get_layer_violations", detail));
    }

    /// The canned failure a `fail_*` setter stores: `tool` is the wire tool
    /// name `RealJCodemunchOps` reports for the same op.
    fn canned_failure(tool: &'static str, detail: &str) -> JCodemunchCallFailed {
        JCodemunchCallFailed {
            tool,
            detail: detail.to_string(),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl JCodemunchOps for MockJCodemunchOps {
    fn get_changed_symbols(
        &self,
        since_sha: &str,
        until_sha: &str,
    ) -> Result<Vec<ChangedSymbol>, JCodemunchCallFailed> {
        self.get_changed_symbols
            .get(&(since_sha.to_string(), until_sha.to_string()))
            .cloned()
            .unwrap_or_else(|| Ok(vec![]))
    }

    fn find_references(
        &self,
        symbol: &ChangedSymbol,
    ) -> Result<Vec<SymbolReference>, JCodemunchCallFailed> {
        self.find_references
            .get(&(symbol.file.clone(), symbol.name.clone()))
            .cloned()
            .unwrap_or_else(|| Ok(vec![]))
    }

    fn get_dead_code(&self, min_confidence: f64) -> Result<Vec<DeadSymbol>, JCodemunchCallFailed> {
        self.last_dead_code_min_confidence.set(Some(min_confidence));
        let symbols = self.dead_code.as_ref().map_err(Clone::clone)?;
        Ok(symbols
            .iter()
            .filter(|s| s.confidence >= min_confidence)
            .cloned()
            .collect())
    }

    fn get_untested_symbols(
        &self,
        min_confidence: f64,
    ) -> Result<Vec<UntestedSymbol>, JCodemunchCallFailed> {
        let symbols = self.untested.as_ref().map_err(Clone::clone)?;
        Ok(symbols
            .iter()
            .filter(|s| s.confidence >= min_confidence)
            .cloned()
            .collect())
    }

    fn get_layer_violations(&self) -> Result<Vec<LayerViolation>, JCodemunchCallFailed> {
        self.layer_violations.clone()
    }
}

/// Combined OPT-OUT predicate for P5 H2 (`check_live_path_stranded`): the
/// author's own opt-out, plus the `crates/reify-stdlib/` scope-exclude (every
/// `.ri` structure def is technically orphan until something calls it).
///
/// Answers "did the author opt out", NOT "should this be reported". A symbol
/// whose declaration was never located answers `false`, because no opt-out
/// was ever observed; whether it is reportable is the SEPARATE question
/// [`ChangedSymbol::decl_located`] asks, and folding the two into one
/// predicate is what made an unlocatable declaration indistinguishable from a
/// clean one.
///
/// P1 (`p1_producer_orphan`) calls [`ChangedSymbol::opts_out`] on its own, so
/// the stdlib clause below is the whole of the difference between the two
/// detectors. Per `f-infra-design.md` §5 P1/P5.
// G-allow: shared suppression predicate; callers are intra-crate (p5_phantom_done::check_live_path_stranded) — orphan-audit script counts only inter-crate call sites
pub(crate) fn is_symbol_suppressed(symbol: &ChangedSymbol) -> bool {
    symbol.file.starts_with("crates/reify-stdlib/") || symbol.opts_out()
}

/// `Some(n)` when `symbols` carries `n >= 1` entries and NOT ONE of their
/// declarations could be located, so a detector's per-symbol pass examined
/// nothing it was handed; `None` when at least one was examinable.
///
/// Such a sweep reports zero findings for the same reason an empty one does —
/// it looked at nothing — but it is invisible to an `is_empty()` check, so
/// the two states need separate detection. The cause is a degraded jcodemunch
/// substrate (see [`ChangedSymbol::decl_located`]); before unlocatable symbols
/// were skipped, that degradation announced itself as a false-positive storm,
/// and without this it would announce itself as nothing at all.
///
/// An EMPTY sweep is deliberately not folded in: "received nothing" and
/// "examined nothing of what was received" have different remedies, so each
/// detector words them as separate clauses.
// G-allow: shared vacuity rule; callers are intra-crate (p1_producer_orphan, p5_phantom_done) — orphan-audit script counts only inter-crate call sites
pub(crate) fn wholly_unlocatable_count(symbols: &[ChangedSymbol]) -> Option<usize> {
    (!symbols.is_empty() && symbols.iter().all(|s| !s.decl_located())).then_some(symbols.len())
}
