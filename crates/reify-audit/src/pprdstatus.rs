//! PPRDSTATUS: PRD status-prose drift. A PRD's `Status:` header and its
//! status-annotated task cites are claims about the task graph, and nothing
//! re-checks them once the decomposition lands. This module reads the loaded
//! task corpus and flags the prose that has drifted from it.

#[cfg(test)]
mod tests {
    use super::*;

    /// `docs/prds/kernel-seam-contracts.md` before its SHIPPED re-stamp
    /// (`edd9703fae^`): the live `Status: contract` header.
    const KERNEL_SEAM_CONTRACTS_PRE_FIX: &str =
        include_str!("../tests/fixtures/pprdstatus/kernel-seam-contracts.pre-edd9703fae.md");

    /// `docs/prds/kernel-seam-contracts.md` after its SHIPPED re-stamp
    /// (`edd9703fae`).
    const KERNEL_SEAM_CONTRACTS_POST_FIX: &str =
        include_str!("../tests/fixtures/pprdstatus/kernel-seam-contracts.post-edd9703fae.md");

    /// `docs/prds/v0_6/data-carrying-enums.md`, lines 1-3.
    const DATA_CARRYING_ENUMS: &str = r#"# PRD: Data-Carrying Enums (Algebraic Data Types)

**Status:** **SHIPPED (v0.6)** — all decomposition leaves (3936/3938/3940/3942/3944/3946/3949/3951) **landed**; runnable end-to-end example `examples/m6_data_carrying_enum.ri`. Mirrors spec §18 roadmap row 6, "Realized (v0.6)". Originally filed as `deferred` in spec-gap batch `spec-gap-2026-05-27`, cluster `data-carrying-enums`. Decomposition style **B + H** (design-first contract + boundary tests) per `preferences_implementation_chain_portfolio`. Authored 2026-05-27; shipped-status recorded 2026-08-06.
"#;

    /// `docs/prds/v0_6/process-dfm-geometry-metrology.md`, lines 1-3.
    const PROCESS_DFM_GEOMETRY_METROLOGY: &str = r#"# PRD (SUPERSEDED → SPLIT): `std.process` geometry-metrology DFM engine

**Status:** SUPERSEDED 2026-06-08 · split into two PRDs after a feasibility sweep · **Milestone:** v0_6
"#;

    /// `docs/prds/auto-type-param-resolution.md`, lines 1-3.
    const AUTO_TYPE_PARAM_RESOLUTION: &str = r#"# PRD: `auto` Type-Parameter Resolution (`Bearing<auto: Seal>`)

Status: Superseded by docs/prds/v0_3/auto-type-param-resolution-completion.md (v0.3 completion contract). Applied after residuals α/β/γ/δ landed.
"#;

    /// `docs/prds/kinematic-constraints.md`, lines 1-6.
    const KINEMATIC_CONSTRAINTS: &str = r#"# Kinematic Constraints — Forward, Open-Chain, Library-Level

## §0 — Superseded

Status: deferred — superseded by `docs/prds/v0_3/kinematic-constraints-completion.md`
(authored 2026-05-17; decomposition landed 2026-07-06).
"#;

    /// `docs/prds/merge-gate-guard-diagnosability.md`, lines 1-5.
    const MERGE_GATE_GUARD_DIAGNOSABILITY: &str = r#"# PRD: kLOC-cap guard diagnosability + at-source trigger

**Date:** 2026-07-22 · **Status:** approved for decomposition · version-agnostic
(root `docs/prds/`). **Approach: B** (two point-hardenings of an existing,
landed guard — no new mechanism, no new seam).
"#;

    /// `docs/prds/naming-convergence/P1-structured-featureid-feature-value.md`, lines 1-4.
    const P1_STRUCTURED_FEATUREID_FEATURE_VALUE: &str = r#"# P1 — Structured `FeatureId` + first-class `Feature` value + fallible codec

> **Status:** active (Wave 1, independent foundation). Naming & Selection Convergence program,
> P1 of P0–P4. Date: 2026-06-24. Approach **B + H** (contract + two-way boundary tests).
"#;

    /// `docs/prds/v0_4/fea-result-model.capability-manifest.md`, lines 1-7.
    const FEA_RESULT_MODEL_CAPABILITY_MANIFEST: &str = r#"# Capability Manifest — fea-result-model.md

Mechanizes G3 + G6 per leaf (overlay → *Capability Manifest — reify evidence forms*). Each task's user-observable/RED signal is decomposed into asserted capabilities, each bound to evidence ∈ `{grep:file:line-wired | producer:task-upstream | grammar-fixture:parses | floor:bound>X | field-population}`. A binding resolving to `{declared-only | test-only | producer-absent | producer-downstream | fixture-ERROR | bound≤floor}` **blocks** queueing until resolved.

Sentinel for this PRD: `Value::Undef` (and the `{ ElasticResult() }` stub body, `scalar_channels: HashMap::new()`, `displaced_positions: None`). Evidence current as of 2026-05-30; G3 fixtures `/tmp/prd-gate-fixtures/fea-result-model-{1,2}.ri` parse with 0 ERROR nodes.

**Status legend:** ✅ PASS · ⏳ FAIL-today-resolved-by-this-batch (in-batch producer is upstream; DAG-correct) · ⛔ BLOCK (must resolve before queue).
"#;

    /// `docs/prds/merge-gate-health.capability-manifest.md`, line 12.
    const MERGE_GATE_HEALTH_CAPABILITY_MANIFEST_TABLE_HEADER: &str = r#"| Leaf | Capability asserted | Evidence binding | Status |
"#;

    /// `docs/prds/kernel-seam-contracts.capability-manifest.md`, lines 1-10.
    const KERNEL_SEAM_CONTRACTS_CAPABILITY_MANIFEST: &str = r#"# Capability Manifest — kernel-seam-contracts

> **AS-AUTHORED GATE ARTIFACT (2026-07-06) — do not refresh.** This manifest records the
> **pre-decomposition** evidence check that cleared this PRD's leaves to queue; its `PASS` verdicts
> are statements about *binding quality at author time*, not about landed state. All 16 leaves have
> since landed (α #5102 … ξ #5116, plus #4876) — see the parent PRD's SHIPPED header. Consequently
> its forward-looking phrasings ("post-landing grep", "red on current main", "the leaf must
> establish") and its hard `file:line` / `@NNNN` anchors are 2026-07-06 provenance and have drifted.
> Rewriting them would destroy the record of what was actually gated. Parent:
> `docs/prds/kernel-seam-contracts.md`.
"#;

    /// `docs/prds/v0_6/tolerance-stackup-analysis.md`, lines 1-12.
    const TOLERANCE_STACKUP_ANALYSIS: &str = r#"# Tolerance Stack-Up Analysis

> A designer who has dimensioned a stacked/assembled set of parts wants one question
> answered before release: **does the accumulated ±tolerance keep a critical gap or fit
> within spec?** Reify already lets you *declare* per-feature dimensional tolerances
> (`stdlib/tolerancing.ri`: `DimensionalTolerance`, GD&T traits, `Fit`). What is missing is
> the *analysis* that propagates those tolerances along a dimension chain and reports the
> resulting gap distribution — worst-case, statistical (RSS), and Monte-Carlo. This PRD adds
> that analysis as a set of stdlib builtins surfaced through `reify eval`, mirroring the
> existing stress-analysis builtin pattern (`stdlib/analysis.ri` + `reify-stdlib/src/analysis.rs`).

Status: contract (B+H). Authored 2026-05-27 in a `/prd` spec-gap-filling batch.
"#;

    /// `docs/prds/v0_3/auto-type-param-constraint-seeding-gaps.md`, lines 1-3.
    const AUTO_TYPE_PARAM_CONSTRAINT_SEEDING_GAPS: &str = r#"# `auto:` Constraint-Seeding Gaps — Computed Defaults (C) and Nested Member Access (D)

Status: completion-residual contract for
"#;

    fn live(token: &str, line: usize) -> StatusHeader {
        StatusHeader::Live { token: token.to_string(), line }
    }

    #[test]
    fn plain_live_label_reports_its_token_and_line() {
        assert_eq!(read_status_header(KERNEL_SEAM_CONTRACTS_PRE_FIX), live("contract", 3));
    }

    #[test]
    fn bold_label_with_trailing_period_is_terminal() {
        assert_eq!(
            read_status_header(KERNEL_SEAM_CONTRACTS_POST_FIX),
            StatusHeader::Terminal(TerminalToken::Shipped)
        );
    }

    #[test]
    fn bold_token_after_bold_label_is_terminal() {
        assert_eq!(
            read_status_header(DATA_CARRYING_ENUMS),
            StatusHeader::Terminal(TerminalToken::Shipped)
        );
    }

    /// The successor is not named on the Status line; the token alone decides.
    #[test]
    fn superseded_token_decides_without_a_named_successor() {
        assert_eq!(
            read_status_header(PROCESS_DFM_GEOMETRY_METROLOGY),
            StatusHeader::Terminal(TerminalToken::Superseded)
        );
    }

    #[test]
    fn title_case_terminal_token_is_terminal() {
        assert_eq!(
            read_status_header(AUTO_TYPE_PARAM_RESOLUTION),
            StatusHeader::Terminal(TerminalToken::Superseded)
        );
    }

    /// A substring match on "superseded" would misclassify this header.
    #[test]
    fn only_the_first_token_decides_terminality() {
        assert_eq!(read_status_header(KINEMATIC_CONSTRAINTS), live("deferred", 5));
    }

    #[test]
    fn withdrawn_is_terminal_in_either_case() {
        for text in [
            "# Retired PRD\n\n**Status:** WITHDRAWN — every leaf was cancelled, no successor.\n",
            "# Retired PRD\n\nStatus: withdrawn after the 2026-09 review.\n",
        ] {
            assert_eq!(
                read_status_header(text),
                StatusHeader::Terminal(TerminalToken::Withdrawn),
                "{text}"
            );
        }
    }

    #[test]
    fn mid_line_label_is_found() {
        assert_eq!(read_status_header(MERGE_GATE_GUARD_DIAGNOSABILITY), live("approved", 3));
    }

    #[test]
    fn blockquote_label_is_found() {
        assert_eq!(read_status_header(P1_STRUCTURED_FEATUREID_FEATURE_VALUE), live("active", 3));
    }

    #[test]
    fn status_legend_is_not_a_label() {
        assert_eq!(read_status_header(FEA_RESULT_MODEL_CAPABILITY_MANIFEST), StatusHeader::Absent);
    }

    #[test]
    fn table_header_status_column_is_not_a_label() {
        assert_eq!(
            read_status_header(MERGE_GATE_HEALTH_CAPABILITY_MANIFEST_TABLE_HEADER),
            StatusHeader::Absent
        );
    }

    #[test]
    fn terminal_word_in_prose_without_a_label_is_absent() {
        assert_eq!(
            read_status_header(KERNEL_SEAM_CONTRACTS_CAPABILITY_MANIFEST),
            StatusHeader::Absent
        );
    }

    #[test]
    fn header_window_ends_after_the_deepest_measured_label() {
        assert_eq!(read_status_header(TOLERANCE_STACKUP_ANALYSIS), live("contract", 12));
        let past_window = format!("{}Status: SHIPPED\n", "filler\n".repeat(STATUS_HEADER_WINDOW));
        assert_eq!(read_status_header(&past_window), StatusHeader::Absent);
    }

    #[test]
    fn dropped_synonyms_are_not_terminal() {
        assert_eq!(
            read_status_header("# Old PRD\n\nStatus: landed 2026-07-01, all leaves done.\n"),
            live("landed", 3)
        );
        assert_eq!(
            read_status_header("# Old PRD\n\nStatus: retired in favour of the v0.3 PRD.\n"),
            live("retired", 3)
        );
    }

    #[test]
    fn first_label_in_the_window_wins() {
        let text = "# Two labels\n\nStatus: active\n\n\nStatus: SHIPPED\n";
        assert_eq!(read_status_header(text), live("active", 3));
    }

    #[test]
    fn hyphenated_token_is_kept_whole() {
        assert_eq!(
            read_status_header(AUTO_TYPE_PARAM_CONSTRAINT_SEEDING_GAPS),
            live("completion-residual", 3)
        );
    }
}
