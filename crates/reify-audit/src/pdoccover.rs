//! PDOCCOVER — bidirectional registry↔chunk name-drift detector.
//!
//! ## Purpose
//!
//! The MCP language-reference chunks (`crates/reify-mcp/src/tools/chunks/*.md`)
//! are the surface a design-authoring agent reads before writing `.ri` source.
//! They drift from the compiler in BOTH directions, and both directions cost
//! design-author time:
//!
//! - **Omission** — a builtin the compiler recognises is documented nowhere,
//!   so the agent never learns it exists.
//! - **Fabrication** — a chunk documents a call that the compiler does *not*
//!   recognise, so the agent writes source that cannot compile.
//!
//! PDOCCOVER detects both from ONE pass. The two directions share the chunk
//! read: a single read of the chunk corpus feeds both the documented-name
//! index (omission) and the call-shaped-mention list (fabrication).
//!
//! Reference: `docs/prds/v0_6/doc-chunk-truth-enforcement.md` §(b) / leaf γ.
//!
//! ## Direction 1 — omission
//!
//! Census: the `*_NAMES` string-slice registries declared in
//! `crates/reify-compiler/src/units.rs` (the PRD-pinned corpus). Each name is
//! compliant iff exactly one of:
//!
//! 1. **documented** — word-boundary match in ≥1 `chunks/*.md`;
//! 2. **allowed** — its registry entry line carries
//!    `// pdoccover:allow — <reason>` (reason mandatory);
//! 3. **baselined** — listed in `crates/reify-audit/pdoccover-baseline.txt`.
//!
//! Anything else is an offender.
//!
//! ## Direction 2 — fabrication
//!
//! Reverse census: every **call-shaped** name mentioned in a chunk (an
//! identifier immediately followed by `(`) must exist somewhere in the
//! compiler/stdlib sources. Names that do not are fabrications.
//!
//! ## Finding categories
//!
//! All five ride at [`Severity::High`] under the single [`Pattern::PDocCover`]
//! variant, carried as a stable summary prefix (PTODO's `kind`-as-prefix
//! convention, `lib.rs` §PTodo):
//!
//! | Prefix | Meaning |
//! |---|---|
//! | `undocumented-name:` | registry name with no chunk mention, no allow, no baseline |
//! | `fabricated-name:` | chunk documents a call-shaped name that exists nowhere in source |
//! | `stale-baseline-entry:` | baselined name that IS documented — ratchet honesty |
//! | `stale-allow-entry:` | allow-marked name that IS documented — ratchet honesty |
//! | `allow-missing-reason:` | `pdoccover:allow` with no reason body — confers NO exemption |
//!
//! ## Escape hatch
//!
//! `// pdoccover:allow — <reason>` on the registry entry line (units.rs side)
//! or on the mentioning line (chunk side). The **prefixed** token is the only
//! form consumed; the bare `doccover:allow` of earlier PRD drafts was renamed
//! 2026-07-25 for uniformity with `ptodo:allow` / `ds-sentinel:allow` and is
//! deliberately NOT honoured. The reason body is mandatory: a reasonless
//! marker emits `allow-missing-reason:` and confers no exemption, so the
//! escape hatch can never become un-reviewable (PRD design decision 7).
//!
//! ## Scope
//!
//! Working-tree reads only — `ls_files()` + `std::fs`, never jcodemunch, and
//! deliberately no `reify-compiler` dependency (the audit crate stays a pure
//! text scanner, PRD §(b)). No regex crate: the audit crate has none and must
//! not gain one.

use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity};
use std::collections::BTreeSet;

// -----------------------------------------------------------------------
// Paths
// -----------------------------------------------------------------------

/// The omission-census registry source (PRD-pinned).
pub const UNITS_PATH: &str = "crates/reify-compiler/src/units.rs";

/// The documentation chunk corpus directory prefix.
pub const CHUNKS_PREFIX: &str = "crates/reify-mcp/src/tools/chunks/";

/// Ratchet baseline. **Seeded by #5480, not by this task** — absent or empty
/// is a supported state and yields an empty allow-set with no error.
pub const BASELINE_PATH: &str = "crates/reify-audit/pdoccover-baseline.txt";

// -----------------------------------------------------------------------
// Registry model
// -----------------------------------------------------------------------

/// One name inside a `*_NAMES` registry, with its 1-based source line and the
/// reason body of a `// pdoccover:allow — <reason>` marker on that line
/// (`None` when the line carries no marker *or* a reasonless one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntry {
    pub name: String,
    pub line: usize,
    pub allow: Option<String>,
    /// `true` when the line carries a `pdoccover:allow` token whose reason
    /// body is blank/absent — the `allow-missing-reason` trigger.
    pub allow_missing_reason: bool,
}

/// One `pub const <IDENT>_NAMES: &[&str] = &[…];` registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry {
    pub const_name: String,
    pub entries: Vec<RegistryEntry>,
}

// -----------------------------------------------------------------------
// Pure scanners (stubs — filled in by the TDD steps that follow)
// -----------------------------------------------------------------------

/// Extract every `*_NAMES` string-slice registry from `units_src`.
///
/// Pure `&str -> Vec<Registry>`, no IO, so the grammar is unit-testable
/// without disk access (the `pdssentinel.rs` `scan_content` split).
// G-allow: consumed by check()/baseline_candidates() in-module and by the
// brittle-parse floor guard in tests/pdoccover.rs (separate crate → must be pub).
pub fn extract_registries(_units_src: &str) -> Vec<Registry> {
    Vec::new()
}

/// Reason body of a `pdoccover:allow` marker on `line`, or `None` when the
/// line carries no marker or the body is blank after trimming.
// G-allow: consumed in-module and by unit tests; pub for symmetry with
// `ptodo::g_allow_marker_body`, whose contract it mirrors.
pub fn allow_marker_reason(_line: &str) -> Option<&str> {
    None
}

/// Subset of `names` that is word-boundary-mentioned in ≥1 chunk source.
///
/// `chunk_sources` are pre-read `(path, content)` pairs so the matcher stays
/// pure and unit-testable without disk access.
// G-allow: consumed by check()/baseline_candidates() in-module and by unit tests.
pub fn documented_names(_names: &[String], _chunk_sources: &[(String, String)]) -> BTreeSet<String> {
    BTreeSet::new()
}

/// Existence oracle for the fabrication direction: every name that the
/// compiler/stdlib sources evidence as real.
// G-allow: consumed by check() in-module and by unit tests.
pub fn known_name_index(_sources: &[(String, String)]) -> BTreeSet<String> {
    BTreeSet::new()
}

/// Call-shaped API mentions in one chunk: `(name, 1-based line)`.
// G-allow: consumed by check() in-module and by the chunk-path brittle-parse
// floor guard in tests/pdoccover.rs (separate crate → must be pub).
pub fn chunk_call_mentions(_content: &str) -> Vec<(String, usize)> {
    Vec::new()
}

// -----------------------------------------------------------------------
// check() — entry point
// -----------------------------------------------------------------------

/// Run both drift directions over the working tree.
///
/// Findings are deterministically ordered. Unreadable files are skipped
/// fail-safe (no finding, no panic).
pub fn check(_ctx: &AuditContext<'_>) -> Vec<Finding> {
    Vec::new()
}

/// The single shared derivation #5480's baseline regenerator consumes: the
/// sorted, deduped set of names that [`check`] reports as `undocumented-name:`.
///
/// Exported so generation and the ratchet can never disagree (PRD §6.6's
/// `ptodo-baseline-gen` lesson). This task ships NO `--emit-baseline` flag,
/// NO `pdoccover-baseline-gen` binary and NO baseline file — all three are
/// #5480's deliverables.
// G-allow: #5480's entry point (PRD open question 1); no in-repo caller yet by design.
pub fn baseline_candidates(_ctx: &AuditContext<'_>) -> Vec<String> {
    Vec::new()
}

// Silence unused-import warnings while the stubs are empty; every one of these
// is consumed once the emission steps land.
const _: fn() = || {
    let _ = |p: Pattern, s: Severity, e: EvidenceRef| (p, s, e);
};
