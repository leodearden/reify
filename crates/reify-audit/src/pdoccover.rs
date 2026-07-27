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

// -----------------------------------------------------------------------
// Unit tests — pure scan grammar
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Names of the registries `extract_registries` found, in discovery order.
    fn const_names(regs: &[Registry]) -> Vec<&str> {
        regs.iter().map(|r| r.const_name.as_str()).collect()
    }

    /// `(name, line)` pairs of every entry in `reg`.
    fn entry_pairs(reg: &Registry) -> Vec<(&str, usize)> {
        reg.entries
            .iter()
            .map(|e| (e.name.as_str(), e.line))
            .collect()
    }

    /// Just the names of every entry in `reg`.
    fn entry_names(reg: &Registry) -> Vec<&str> {
        reg.entries.iter().map(|e| e.name.as_str()).collect()
    }

    // ── step-1 (a): single-line registry form ────────────────────────────

    /// The one-liner shape `pub const X_NAMES: &[&str] = &["nominal"];` (the
    /// real `TOLERANCING_MARKER_NAMES`) yields one registry with one entry,
    /// whose line is the declaration line.
    #[test]
    fn extract_registries_single_line_form() {
        let src = r#"
/// Tolerancing markers.
pub const TOLERANCING_MARKER_NAMES: &[&str] = &["nominal"];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["TOLERANCING_MARKER_NAMES"],
            "single-line registry must be discovered; got: {regs:?}"
        );
        assert_eq!(
            entry_pairs(&regs[0]),
            vec![("nominal", 3)],
            "single-line registry must yield its one entry at the declaration \
             line; got: {:?}",
            regs[0].entries
        );
    }

    // ── step-1 (b): multi-line block form ────────────────────────────────

    /// The block shape (the real `GEOMETRY_FUNCTION_NAMES`) — one name per
    /// line, trailing comma — yields every name with correct 1-based lines.
    #[test]
    fn extract_registries_multi_line_block_form() {
        let src = r#"pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    "box",
    "cylinder",
    "sphere",
];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["GEOMETRY_FUNCTION_NAMES"],
            "block-form registry must be discovered; got: {regs:?}"
        );
        assert_eq!(
            entry_pairs(&regs[0]),
            vec![("box", 2), ("cylinder", 3), ("sphere", 4)],
            "block-form entries must carry correct 1-based line numbers; got: {:?}",
            regs[0].entries
        );
    }

    // ── step-1 (c): line-broken header form ──────────────────────────────

    /// The real `GEOMETRY_KINEMATIC_QUERY_NAMES` shape puts `=` at the end of
    /// the declaration line and `&[` on the NEXT line. It must be parsed, not
    /// skipped — skipping it would silently drop a whole registry from the
    /// census, exactly the brittle-parse failure the step-3 guard defends.
    #[test]
    fn extract_registries_line_broken_header_form() {
        let src = r#"pub const GEOMETRY_KINEMATIC_QUERY_NAMES: &[&str] =
    &["interferes", "interferes_with", "min_clearance"];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["GEOMETRY_KINEMATIC_QUERY_NAMES"],
            "line-broken header form must be discovered, not skipped; got: {regs:?}"
        );
        assert_eq!(
            entry_names(&regs[0]),
            vec!["interferes", "interferes_with", "min_clearance"],
            "line-broken header form must yield all three names; got: {:?}",
            regs[0].entries
        );
    }

    // ── step-1 (d): visibility prefixes ──────────────────────────────────

    /// `pub(crate) const` and bare `const` are accepted alongside `pub const`.
    #[test]
    fn extract_registries_accepts_visibility_prefixes() {
        let src = r#"pub(crate) const ALPHA_NAMES: &[&str] = &["a"];
const BETA_NAMES: &[&str] = &["b"];
pub const GAMMA_NAMES: &[&str] = &["c"];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["ALPHA_NAMES", "BETA_NAMES", "GAMMA_NAMES"],
            "pub(crate)/bare/pub const prefixes must all be accepted; got: {regs:?}"
        );
    }

    // ── step-1 (e): non-`_NAMES` consts ignored ──────────────────────────

    /// A const whose identifier does not end in `_NAMES` is not a builtin-name
    /// registry and must be ignored — including tuple-slice consts like the
    /// real `SI_PREFIXES`, whose quoted tokens are not builtin names.
    #[test]
    fn extract_registries_ignores_non_names_consts() {
        let src = r#"pub const SI_PREFIXES: &[(&str, f64)] = &[("kilo", 1e3), ("milli", 1e-3)];
pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &["box"];
pub const SOME_OTHER: &[&str] = &["not_a_registry"];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["GEOMETRY_FUNCTION_NAMES"],
            "only `*_NAMES` string-slice consts are registries; got: {regs:?}"
        );
    }

    // ── step-1 (f): comment lines inside the block ───────────────────────

    /// `//` and `///` comment lines inside a registry block contribute no
    /// entry — including a comment that itself contains a quoted token, which
    /// must never be mistaken for a registry name.
    #[test]
    fn extract_registries_skips_comment_lines_in_block() {
        let src = r#"pub const GEOMETRY_QUERY_HELPER_NAMES: &[&str] = &[
    // Task 2320: the original trio (see "watertight" discussion).
    "is_watertight",
    /// doc-comment mentioning "is_phantom"
    "is_manifold",
];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            entry_names(&regs[0]),
            vec!["is_watertight", "is_manifold"],
            "comment lines (and quoted tokens inside them) must contribute no \
             entries; got: {:?}",
            regs[0].entries
        );
    }

    // ── step-1 (g): pdoccover:allow marker on an entry line ──────────────

    /// An entry line carrying `// pdoccover:allow — <reason>` records the
    /// trimmed reason body in `allow`, and the marker text itself is never
    /// mistaken for an entry.
    #[test]
    fn extract_registries_records_allow_reason() {
        let src = r#"pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    "box",
    "lower_shim", // pdoccover:allow — geometry-only internal
];
"#;
        let regs = extract_registries(src);
        let entries = &regs[0].entries;
        assert_eq!(
            entries.len(),
            2,
            "allow-marked line yields exactly one entry (the name), not the \
             marker text; got: {entries:?}"
        );
        assert_eq!(entries[0].name, "box");
        assert_eq!(
            entries[0].allow, None,
            "unmarked entry must carry allow=None; got: {:?}",
            entries[0]
        );
        assert_eq!(entries[1].name, "lower_shim");
        assert_eq!(
            entries[1].allow.as_deref(),
            Some("geometry-only internal"),
            "allow-marked entry must carry the trimmed reason body; got: {:?}",
            entries[1]
        );
        assert!(
            !entries[1].allow_missing_reason,
            "a marker WITH a reason must not set allow_missing_reason; got: {:?}",
            entries[1]
        );
    }

    // ── step-1 (h): two names on one line ────────────────────────────────

    /// Two names on one line are both extracted, sharing that line number
    /// (the real `DYNAMICS_CONSTRUCTOR_NAMES` shape).
    #[test]
    fn extract_registries_two_names_one_line() {
        let src =
            r#"pub const DYNAMICS_CONSTRUCTOR_NAMES: &[&str] = &["mass_properties", "point_mass"];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            entry_pairs(&regs[0]),
            vec![("mass_properties", 1), ("point_mass", 1)],
            "both names on one line must be extracted with that shared line \
             number; got: {:?}",
            regs[0].entries
        );
    }

    // ── step-2: allow_marker_reason grammar ──────────────────────────────

    /// The reason separator accepts an em dash, an ASCII hyphen or a colon —
    /// and a bare space also works. The body is returned trimmed.
    ///
    /// Three separators rather than only the PRD's literal em dash: a gate
    /// that fails on an invisible typographic difference would be a trap. The
    /// marker token plus a non-blank reason carry the normative weight.
    #[test]
    fn allow_marker_reason_accepts_three_separators() {
        for line in [
            r#"    "x", // pdoccover:allow — internal lowering shim"#,
            r#"    "x", // pdoccover:allow - internal lowering shim"#,
            r#"    "x", // pdoccover:allow: internal lowering shim"#,
            r#"    "x", // pdoccover:allow   internal lowering shim  "#,
        ] {
            assert_eq!(
                allow_marker_reason(line),
                Some("internal lowering shim"),
                "marker reason must be extracted and trimmed for line: {line:?}"
            );
        }
    }

    /// A reasonless marker (bare, or a separator with a blank body) yields
    /// `None` — which is what makes `allow-missing-reason` fall out naturally,
    /// mirroring `ptodo::g_allow_marker_body`'s blank-body contract.
    #[test]
    fn allow_marker_reason_none_for_blank_body() {
        for line in [
            r#"    "x", // pdoccover:allow"#,
            r#"    "x", // pdoccover:allow —"#,
            r#"    "x", // pdoccover:allow:   "#,
        ] {
            assert_eq!(
                allow_marker_reason(line),
                None,
                "a reasonless marker must yield None for line: {line:?}"
            );
        }
    }

    /// The legacy UNPREFIXED `doccover:allow` (earlier PRD drafts, renamed
    /// 2026-07-25 for uniformity with `ptodo:allow`) is NOT honoured — only
    /// the prefixed token is consumed.
    #[test]
    fn allow_marker_reason_rejects_legacy_unprefixed_token() {
        assert_eq!(
            allow_marker_reason(r#"    "x", // doccover:allow — legacy form"#),
            None,
            "the legacy unprefixed `doccover:allow` must confer nothing"
        );
    }

    /// A line with no marker at all yields `None`.
    #[test]
    fn allow_marker_reason_none_without_marker() {
        assert_eq!(allow_marker_reason(r#"    "box","#), None);
    }

    /// `allow_missing_reason` is set on a registry entry whose line carries a
    /// reasonless marker (and the marker confers no reason).
    #[test]
    fn extract_registries_flags_reasonless_allow_marker() {
        let src = r#"pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    "lower_shim", // pdoccover:allow
];
"#;
        let regs = extract_registries(src);
        let e = &regs[0].entries[0];
        assert_eq!(e.name, "lower_shim");
        assert_eq!(
            e.allow, None,
            "a reasonless marker confers no reason; got: {e:?}"
        );
        assert!(
            e.allow_missing_reason,
            "a reasonless marker must set allow_missing_reason; got: {e:?}"
        );
    }
}
