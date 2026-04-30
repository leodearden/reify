//! v0.2 persistent-naming-v2 attribute-based selector resolver.
//!
//! Mirrors `topology_selectors.rs::resolve_unique_by_tag` (the v0.1 path)
//! but consumes `TopologyAttributeTable` instead of `FeatureTagTable`.
//!
//! ## PRD references
//!
//! See `docs/prds/v0_2/persistent-naming-v2.md`:
//!
//! - **Line 62 — user-label preference rule.** When both `user_label` and
//!   `(role, local_index)` apply, the user-supplied label wins. Implemented
//!   here as: a UNIQUE `user_label` match returns immediately; a zero
//!   `user_label` match falls through to the role+local_index branch; a
//!   multi-match on `user_label` does NOT fall through (it surfaces an
//!   ambiguity diagnostic so an authored name collision is not silently
//!   converted to a role/idx match).
//!
//! - **Line 68 — imported-geometry fallback.** Native (constructed) geometry
//!   carries `TopologyAttribute` entries seeded by the per-op populators
//!   (`seed_primitive_attributes`, `populate_extrude_attributes`, etc.).
//!   Imported geometry (STEP/STL/...) skips that path, so its result handles
//!   are absent from the table. The resolver detects this by checking that
//!   NONE of the supplied candidates carry an entry; that absence is the
//!   imported-geometry signal and the resolver returns
//!   [`AttributeResolution::FallbackToComputed`] without emitting any
//!   diagnostic. Upstream callers route through computed selectors
//!   (`faces_by_normal`, `edges_by_length`, ...) on this signal.
//!
//! ## Purity
//!
//! The resolver is pure Rust: it does NOT take a `&mut dyn GeometryKernel`.
//! Callers pre-extract candidates via `kernel.extract_faces(...)` /
//! `kernel.extract_edges(...)` and pass a slice. This mirrors
//! `resolve_unique_by_tag`'s discipline and keeps the resolver testable
//! without an OCCT build.

use std::collections::HashSet;

use reify_types::{
    Diagnostic, DiagnosticCode, DiagnosticLabel, FeatureId, GeometryHandleId, Role, SourceSpan,
    TopologyAttribute, TopologyAttributeTable,
};

/// Query used to pick a unique sub-shape out of a candidate slice.
///
/// All three fields are independently optional:
/// - `user_label` — match against `TopologyAttribute::user_label`. When `Some`,
///   takes precedence over `role_and_index` per PRD line 62 (with the
///   fallthrough rules documented on [`resolve_unique_by_attribute`]).
/// - `role_and_index` — match against `(TopologyAttribute::role,
///   TopologyAttribute::local_index)`.
/// - `feature_id` — when `Some`, additionally constrains BOTH branches: a
///   candidate is considered only if its `TopologyAttribute::feature_id`
///   equals the query value.
///
/// ## Contract
///
/// At least one of `user_label` or `role_and_index` MUST be `Some`. A query
/// with `feature_id=Some` but both positional fields `None` is contractually
/// invalid.
///
/// `feature_id` is a **filter**, not a constraint: it narrows whichever
/// positional branch fires, but by itself it does not supply a query. The
/// resolver intentionally routes feature_id-only queries through the all-None
/// positional branch (see [`resolve_unique_by_attribute`]), returning
/// [`AttributeResolution::Unresolved`] with a `TopologyAttributeStale`
/// "matched 0 sub-shapes" diagnostic. Callers that hit this in the wild have
/// a construction bug; the diagnostic shape is intentional and documented here
/// so it is not mistaken for a topology-change miss.
///
/// The regression test
/// `tests::feature_id_only_query_is_treated_as_all_none_positional_miss` pins
/// this contract as a behavior guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeQuery {
    pub user_label: Option<String>,
    pub role_and_index: Option<(Role, u32)>,
    pub feature_id: Option<FeatureId>,
}

/// Four-arm resolution outcome.
///
/// PRD line 68 mandates that "no construction history" (imported geometry)
/// is reported separately from "match failed", so callers can route through
/// computed selectors on the former and emit a diagnostic on the latter.
/// Folding both into `Option::None` would force every caller to re-derive
/// the difference.
///
/// PRD line 64 (modification-history postfix) further mandates that a
/// multi-match where ALL matched candidates share the parent-key
/// (`feature_id, role, local_index, user_label`) is reported as the SET of
/// split children rather than folded into a generic Unresolved miss — so
/// callers can surface the ambiguity for user disambiguation rather than
/// silently rebinding to one arbitrary child. Hence the
/// `AmbiguousAfterSplit` variant is distinct from `Unresolved`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeResolution {
    /// Exactly one candidate matched the query.
    Resolved(GeometryHandleId),
    /// None of the supplied candidates carry an attribute entry — the
    /// imported-geometry signal. Callers route through computed selectors.
    FallbackToComputed,
    /// At least one candidate has an entry, but the query produced zero or
    /// multiple matches with mixed parent-keys (genuine ambiguity, not a
    /// split). A `TopologyAttributeStale` diagnostic has been pushed to
    /// the supplied `diagnostics` vec.
    Unresolved,
    /// All matched candidates share the same parent-key
    /// (`feature_id, role, local_index, user_label`) and only differ in
    /// `mod_history` — the v0.2 split-children signature. Children list
    /// is in records-encounter order (which matches per-parent
    /// `split_index` ordering written by the propagator). Per PRD line 64,
    /// callers surface this for user disambiguation rather than silently
    /// rebinding. A `TopologyAttributeAmbiguousAfterSplit` diagnostic has
    /// been pushed to the supplied `diagnostics` vec.
    AmbiguousAfterSplit { children: Vec<GeometryHandleId> },
}

/// Resolve a `query` against a slice of candidate handles, consulting the
/// attribute `table` and emitting at most one diagnostic into `diagnostics`.
///
/// # User-label preference rule (PRD line 62)
///
/// When BOTH `query.user_label` and `query.role_and_index` are `Some`:
///
/// - **Unique user_label match** → return [`AttributeResolution::Resolved`]
///   immediately; the role/idx branch is not consulted.
/// - **Zero user_label matches** → fall through to the role/idx branch.
///   This makes a query that names a label that never existed gracefully
///   try the role-based interpretation instead of failing — mirrors how
///   `@face("top")` might mean "the role-named top cap" when no
///   user-labeled "top" exists.
/// - **Multi-match user_label** (≥ 2 candidates carry the same label) →
///   do NOT fall through. A multi-match is itself an ambiguity signal —
///   converting it to a role/idx match would silently paper over a name
///   collision the user authored intentionally. Falls into the
///   [`AttributeResolution::Unresolved`] arm; step-12's diagnostic emission
///   refines this with a `TopologyAttributeStale` warning.
///
/// # Imported-geometry fallback (PRD line 68)
///
/// Step-8 will add a pre-pass that returns
/// [`AttributeResolution::FallbackToComputed`] when NONE of `candidates`
/// carry a `TopologyAttributeTable` entry. That outcome is the
/// imported-geometry signal; upstream callers route through computed
/// selectors on it.
pub fn resolve_unique_by_attribute(
    table: &TopologyAttributeTable,
    candidates: &[GeometryHandleId],
    query: &AttributeQuery,
    selector_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> AttributeResolution {
    // (step-16a) Empty candidate slice: nothing to look up — by definition
    // no candidate carries an entry, so route through computed selectors.
    // This is the same outcome as the fallback pre-pass below, but the
    // explicit early-return skips the lookup loop entirely.
    if candidates.is_empty() {
        return AttributeResolution::FallbackToComputed;
    }

    // Imported-geometry fallback pre-pass (step-8). Per PRD line 68: if
    // NONE of the supplied candidates carry an attribute entry, the result
    // handles came from an op that didn't seed/propagate (the imported-
    // geometry case once import ops exist). Route through computed
    // selectors. A plain linear scan short-circuits on the first hit;
    // deduplication is unnecessary here because `table.lookup` is
    // idempotent — re-querying the table for the same id returns the same
    // answer, so duplicate candidate ids cannot change the outcome. The
    // dedup discipline is enforced where it matters, inside
    // `count_unique_matches` (which counts matches, not just any-match).
    if !candidates.iter().any(|&id| table.lookup(id).is_some()) {
        return AttributeResolution::FallbackToComputed;
    }

    // (step-16b) All-None *positional* query: the resolver has no positional
    // constraint to match against. This check inspects ONLY `user_label` and
    // `role_and_index`; `feature_id` is NOT consulted here. A query with
    // `feature_id=Some` but both positional fields `None` is contract-illegal
    // per `AttributeQuery`'s docs and intentionally falls into this branch —
    // `feature_id` is a filter, not a constraint, and by itself it does not
    // supply a query. Treating the case as zero-match ensures the caller
    // surfaces the same diagnostic shape as a stale-attribute miss. Defends
    // against accidental "match-everything" semantics that would otherwise
    // arise if a future caller construction-defaulted all positional fields to
    // None. Placed AFTER the fallback pre-pass so imported-geometry routing
    // still wins on import-style candidate sets (consistent with the
    // FallbackToComputed-takes-priority decision in the resolver contract).
    if query.user_label.is_none() && query.role_and_index.is_none() {
        emit_attribute_stale_diagnostic(selector_span, 0, diagnostics);
        return AttributeResolution::Unresolved;
    }

    // Track the count from the last branch consulted so the final
    // diagnostic message reports an accurate "matched N sub-shapes". The
    // user_label multi-match path short-circuits below (it does NOT fall
    // through to the role/idx branch); the zero-match user_label path
    // falls through and may be overridden by the role/idx count.
    let mut last_count: Option<usize> = None;

    // `query.feature_id`, when Some, additionally constrains BOTH match
    // branches: a candidate is considered only if its
    // `TopologyAttribute::feature_id` equals the query value. The
    // imported-geometry fallback pre-pass above is intentionally
    // unaffected — it counts candidates with ANY entry, so a feature_id
    // filter never spuriously flips a native-geometry resolution attempt
    // into the FallbackToComputed arm.
    let feature_id_filter = |attr: &TopologyAttribute| -> bool {
        match query.feature_id.as_ref() {
            None => true,
            Some(fid) => attr.feature_id == *fid,
        }
    };

    // user_label branch (step-2). Per PRD line 62, this branch fires first
    // when query.user_label is Some.
    //
    // A single `collect_matches` call replaces the legacy two-pass scan
    // (count then collect): the unique-match short-circuit and the
    // multi-match clustering path both consume the same Vec, dropping the
    // redundant predicate-closure construction per multi-match.
    if let Some(label) = query.user_label.as_deref() {
        let matches = collect_matches(table, candidates, |attr| {
            attr.user_label.as_deref() == Some(label) && feature_id_filter(attr)
        });
        match matches.len() {
            1 => return AttributeResolution::Resolved(matches[0].0),
            // Zero matches: fall through to role/idx branch.
            0 => last_count = Some(0),
            // Multi-match: explicitly do NOT fall through. The role/idx
            // branch is skipped so an authored label collision is not
            // silently converted to a role-based match.
            //
            // Try the parent-key clustering check first (task #2653): a
            // labelled face that gets split inherits the same user_label
            // on every child, so a user_label multi-match where all
            // matched candidates share `(feature_id, role, local_index,
            // user_label)` is a split, surfaced via AmbiguousAfterSplit.
            // Otherwise (mixed parent-keys = genuine label collision)
            // fall through to Unresolved with the count-aware diagnostic.
            n => {
                if let Some(resolution) =
                    try_cluster_after_split(&matches, selector_span, diagnostics)
                {
                    return resolution;
                }
                emit_attribute_stale_diagnostic(selector_span, n, diagnostics);
                return AttributeResolution::Unresolved;
            }
        }
    }
    // role + local_index branch (step-4). Same single-scan discipline as
    // the user_label branch above.
    if let Some((role, idx)) = query.role_and_index {
        let matches = collect_matches(table, candidates, |attr| {
            attr.role == role && attr.local_index == idx && feature_id_filter(attr)
        });
        match matches.len() {
            1 => return AttributeResolution::Resolved(matches[0].0),
            // Multi-match: try the parent-key clustering check (task
            // #2653). If all matched candidates share `(feature_id, role,
            // local_index, user_label)` and differ only in `mod_history`,
            // this is a split — surface the SET of children via
            // AmbiguousAfterSplit instead of Unresolved. Otherwise fall
            // through to the existing Unresolved arm with the generic
            // count-aware diagnostic.
            n if n > 1 => {
                if let Some(resolution) =
                    try_cluster_after_split(&matches, selector_span, diagnostics)
                {
                    return resolution;
                }
                last_count = Some(n);
            }
            // Zero matches.
            n => last_count = Some(n),
        }
    }
    // Stale-attribute diagnostic emission (step-10/step-12). At least one
    // candidate carries an entry (the fallback pre-pass already eliminated
    // the imported-geometry case) but no branch produced a unique match.
    // `last_count` reflects the count of the last branch consulted (zero
    // when neither branch fired — e.g. an all-None query, defended in
    // step-16).
    let n = last_count.unwrap_or(0);
    emit_attribute_stale_diagnostic(selector_span, n, diagnostics);
    AttributeResolution::Unresolved
}

/// Push a `TopologyAttributeStale` Warning describing a `matched n
/// sub-shapes` outcome onto `diagnostics`.
///
/// Shared by the zero-match (step-10) and multi-match (step-12) emission
/// sites so the message form stays consistent. The primary label is
/// attached at `selector_span` with the canonical "selector call" text,
/// matching `resolve_unique_by_tag`'s diagnostic shape. A secondary
/// "feature originally produced here" label is intentionally omitted —
/// `TopologyAttribute` does not currently carry a `source_span`, so there
/// is no canonical originating-feature span to point at.
fn emit_attribute_stale_diagnostic(
    selector_span: SourceSpan,
    n: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(
        Diagnostic::warning(format!(
            "topology-attribute selector matched {n} sub-shapes \
             (expected exactly 1; topology may have changed)"
        ))
        .with_code(DiagnosticCode::TopologyAttributeStale)
        .with_label(DiagnosticLabel::new(selector_span, "selector call")),
    );
}

/// Walk `candidates`, looking up each id in `table` and applying `predicate`
/// to the attribute. Returns the de-duplicated Vec of all matching
/// `(handle, attribute)` pairs in candidate-encounter order.
///
/// Mirrors `resolve_unique_by_tag`'s zero/one/many counting discipline.
/// Callers branch on `matches.len()` (`0` / `1` / `>1`) to decide whether
/// to resolve, fall through, or emit a diagnostic. The returned slice
/// also feeds [`try_cluster_after_split`] without re-querying the table:
/// each match already carries a borrow of the matching attribute, so
/// the cluster predicate runs directly over the borrowed attrs rather
/// than re-doing `table.lookup` per matched id.
///
/// Deduplicates candidate ids via a HashSet (mirroring
/// `resolve_unique_by_tag` at topology_selectors.rs:703) so a misbehaving
/// extractor that returned the same handle multiple times does not
/// inflate the match count and spuriously trigger an ambiguity
/// diagnostic.
fn collect_matches<'t, F>(
    table: &'t TopologyAttributeTable,
    candidates: &[GeometryHandleId],
    predicate: F,
) -> Vec<(GeometryHandleId, &'t TopologyAttribute)>
where
    F: Fn(&TopologyAttribute) -> bool,
{
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(candidates.len());
    let mut out: Vec<(GeometryHandleId, &TopologyAttribute)> = Vec::new();
    for &id in candidates {
        if !seen.insert(id) {
            continue;
        }
        if let Some(attr) = table.lookup(id)
            && predicate(attr)
        {
            out.push((id, attr));
        }
    }
    out
}

/// If every attribute behind `matches` agrees on its parent-key fields
/// (`feature_id, role, local_index, user_label`), emit a `split children`
/// diagnostic and return `Some(AmbiguousAfterSplit { children })`.
/// Otherwise return `None` so the caller falls through to the generic
/// Unresolved arm with the existing "matched N sub-shapes" diagnostic.
///
/// Operates directly on the `(handle, attr)` pairs returned by
/// [`collect_matches`] — both the parent-key windows check and the
/// children-list construction use the borrowed attrs, so no
/// `table.lookup` is re-issued per matched id.
///
/// Failure modes that yield `None` (caller proceeds to Unresolved):
///   - `matches.len() < 2` — no cluster to detect; a single-element
///     match is handled upstream by the unique-match short-circuit and
///     a zero-element match would be caught by the zero-match emission.
///   - Any pair of matched attributes disagrees on any parent-key field
///     (`feature_id`, `role`, `local_index`, or `user_label`). This is
///     the genuine-ambiguity path: e.g. two distinct features colliding
///     on a label (`Boss` and `Slot` both labelled `"top"`), or two
///     roles colliding on `local_index` after a populator reassignment.
///     Distinct from a post-split cluster where every matched candidate
///     shares one parent and only `mod_history` differs.
///   - Any consecutive pair of matched attributes shares the same `mod_history`
///     (detected via `windows(2).all(distinct)`). The propagator records
///     children in ascending `split_index` order, so a consecutive equal pair
///     means a duplicate `split_index` — a populator bug. This catches both
///     fully-uniform clusters (`[A, A, A]`) and partially-duplicate ones
///     (`[A, A, B]`) where some elements are still indistinguishable to the
///     future task-10 `split_by(...)` selector. Routed to `Unresolved` via the
///     canonical "matched N sub-shapes" diagnostic so the populator bug surfaces
///     to the user, instead of producing a children list with ambiguous elements.
///
/// Children list inside the returned variant is the `matches` ids in
/// candidate-encounter order — which matches per-parent record-stream
/// position (i.e. ascending `split_index`), per the propagator's
/// iteration order documented in `count_children_per_parent`.
fn try_cluster_after_split(
    matches: &[(GeometryHandleId, &TopologyAttribute)],
    selector_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<AttributeResolution> {
    if matches.len() < 2 {
        return None;
    }
    if !matches.windows(2).all(|w| w[0].1.same_parent_as(w[1].1)) {
        return None;
    }
    // Require ALL consecutive pairs to have distinct mod_history. The propagator
    // records children in ascending split_index order, so a consecutive equal
    // pair means a duplicate split_index — a populator bug, not a genuine
    // post-split cluster. Using all(distinct) catches both fully-uniform
    // clusters (e.g. [A, A, A]) and partially-duplicate ones (e.g. [A, A, B])
    // where some elements remain indistinguishable to the future task-10
    // split_by(...) selector; fall through to Unresolved in those cases.
    if !matches.windows(2).all(|w| w[0].1.mod_history != w[1].1.mod_history) {
        return None;
    }
    emit_split_children_diagnostic(selector_span, matches.len(), diagnostics);
    Some(AttributeResolution::AmbiguousAfterSplit {
        children: matches.iter().map(|(id, _)| *id).collect(),
    })
}

/// Push a `TopologyAttributeAmbiguousAfterSplit` Warning describing a
/// `matched n split children of the same parent` outcome onto `diagnostics`.
///
/// Sibling of [`emit_attribute_stale_diagnostic`], which emits
/// `TopologyAttributeStale` for the genuine-ambiguity / populator-bug path.
/// This function uses the typed `TopologyAttributeAmbiguousAfterSplit` code
/// so downstream consumers (LSP/MCP) can distinguish the two outcomes
/// without substring-parsing the message. The human-readable message wording
/// is preserved for user-visible output:
///   - "matched N sub-shapes" → genuine ambiguity (Unresolved,
///     `TopologyAttributeStale`).
///   - "matched N split children of the same parent" → split cluster
///     (`AmbiguousAfterSplit`, `TopologyAttributeAmbiguousAfterSplit`; user
///     can disambiguate via split_by(...) once vocabulary v2 lands per task 10).
fn emit_split_children_diagnostic(
    selector_span: SourceSpan,
    n: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(
        Diagnostic::warning(format!(
            "topology-attribute selector matched {n} split children of the same parent \
             (disambiguate via split_by(...) selector once vocabulary v2 lands)"
        ))
        .with_code(DiagnosticCode::TopologyAttributeAmbiguousAfterSplit)
        .with_label(DiagnosticLabel::new(selector_span, "selector call")),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{CapKind, FeatureId, ModEntry, Severity};

    fn span() -> SourceSpan {
        SourceSpan::empty(0)
    }

    fn feat() -> FeatureId {
        FeatureId::new("Feature#realization[0]")
    }

    /// Build a `TopologyAttribute` with the provided fields, defaulting the
    /// rest. Keeps test setup terse.
    fn attr(role: Role, local_index: u32, user_label: Option<&str>) -> TopologyAttribute {
        TopologyAttribute {
            feature_id: feat(),
            role,
            local_index,
            user_label: user_label.map(|s| s.to_string()),
            mod_history: Vec::new(),
        }
    }

    /// Variant of [`attr`] that takes an explicit `feature_id`. Used by
    /// step-13's feature_id-constraint test.
    fn attr_for(
        feature_id: FeatureId,
        role: Role,
        local_index: u32,
        user_label: Option<&str>,
    ) -> TopologyAttribute {
        TopologyAttribute {
            feature_id,
            role,
            local_index,
            user_label: user_label.map(|s| s.to_string()),
            mod_history: Vec::new(),
        }
    }

    fn h(n: u64) -> GeometryHandleId {
        GeometryHandleId(n)
    }

    /// step-1 — user_label uniquely identifies the sub-shape; the resolver
    /// returns `Resolved(handle 10)` and emits no diagnostics.
    #[test]
    fn resolve_unique_by_attribute_user_label_match_returns_resolved() {
        let mut table = TopologyAttributeTable::default();
        table.record(h(10), attr(Role::Side, 0, Some("top")));
        table.record(h(11), attr(Role::Side, 1, Some("bottom")));
        let candidates = [h(10), h(11)];
        let query = AttributeQuery {
            user_label: Some("top".to_string()),
            role_and_index: None,
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result =
            resolve_unique_by_attribute(&table, &candidates, &query, span(), &mut diagnostics);
        assert_eq!(result, AttributeResolution::Resolved(h(10)));
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected on a unique user_label match"
        );
    }

    /// step-3 — role+local_index uniquely identifies the sub-shape (no
    /// user_label set on either candidate); returns Resolved with no
    /// diagnostics.
    #[test]
    fn resolve_unique_by_attribute_role_and_index_match_returns_resolved() {
        let mut table = TopologyAttributeTable::default();
        table.record(h(20), attr(Role::Cap(CapKind::Top), 0, None));
        table.record(h(21), attr(Role::Side, 0, None));
        let candidates = [h(20), h(21)];
        let query = AttributeQuery {
            user_label: None,
            role_and_index: Some((Role::Cap(CapKind::Top), 0)),
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result =
            resolve_unique_by_attribute(&table, &candidates, &query, span(), &mut diagnostics);
        assert_eq!(result, AttributeResolution::Resolved(h(20)));
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected on a unique role/idx match"
        );
    }

    /// step-5 — user_label preference rule (PRD line 62).
    ///
    /// Companion sub-cases:
    /// - When user_label uniquely matches handle 30 (whose role/idx do NOT
    ///   match the query's role/idx), user_label wins → Resolved(30).
    /// - When user_label is set but matches NO candidate, fall through to
    ///   the role/idx branch → Resolved(31).
    #[test]
    fn user_label_preferred_over_role_and_index_when_both_apply() {
        let mut table = TopologyAttributeTable::default();
        // handle 30 — has the user_label, but mismatched role/idx.
        table.record(h(30), attr(Role::Side, 7, Some("top")));
        // handle 31 — no user_label, but matches the queried role/idx.
        table.record(h(31), attr(Role::Cap(CapKind::Top), 0, None));
        let candidates = [h(30), h(31)];

        // (a) user_label match exists → user_label wins, role/idx ignored.
        let query_a = AttributeQuery {
            user_label: Some("top".to_string()),
            role_and_index: Some((Role::Cap(CapKind::Top), 0)),
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result_a =
            resolve_unique_by_attribute(&table, &candidates, &query_a, span(), &mut diagnostics);
        assert_eq!(
            result_a,
            AttributeResolution::Resolved(h(30)),
            "user_label match wins over role/idx match per PRD line 62"
        );
        assert!(diagnostics.is_empty());

        // (b) user_label matches nothing → fall through to role/idx branch.
        let query_b = AttributeQuery {
            user_label: Some("nonexistent".to_string()),
            role_and_index: Some((Role::Cap(CapKind::Top), 0)),
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result_b =
            resolve_unique_by_attribute(&table, &candidates, &query_b, span(), &mut diagnostics);
        assert_eq!(
            result_b,
            AttributeResolution::Resolved(h(31)),
            "missing user_label falls through to role/idx branch"
        );
        assert!(diagnostics.is_empty());
    }

    /// Multi-match with MIXED parent-keys → Unresolved + count-aware
    /// diagnostic (genuine ambiguity, not a split).
    ///
    /// Multi-match where the matched candidates disagree on one of the four
    /// parent-key fields (`feature_id`, `role`, `local_index`, `user_label`)
    /// must route to `AttributeResolution::Unresolved` with the canonical
    /// "matched N sub-shapes" diagnostic — NOT the "split children" sub-form,
    /// which is reserved for parent-key-clustered matches per task #2653.
    ///
    /// Parameterized over all four parent-key disagreement axes. Each axis
    /// uses the query branch that allows both candidates to match (see design
    /// decisions for branch-coverage rationale). Pins PRD line 64 and the
    /// disambiguation between genuine ambiguity (`Unresolved`) and split
    /// detection (`AmbiguousAfterSplit`).
    #[test]
    fn unresolved_with_diagnostic_when_multi_match() {
        struct Case {
            axis: &'static str,
            attr_a: TopologyAttribute,
            attr_b: TopologyAttribute,
            query: AttributeQuery,
        }

        let cases = [
            // (1) feature_id axis: both match role/idx query; parent-keys
            //     disagree on feature_id → cluster check fails → Unresolved.
            Case {
                axis: "feature_id",
                attr_a: attr_for(FeatureId::new("Boss"), Role::Side, 0, None),
                attr_b: attr_for(FeatureId::new("Slot"), Role::Side, 0, None),
                query: AttributeQuery {
                    user_label: None,
                    role_and_index: Some((Role::Side, 0)),
                    feature_id: None,
                },
            },
            // (2) role axis: both match user_label query ("seam"); parent-keys
            //     disagree on role → cluster check fails → Unresolved.
            //     (role/idx branch would filter out role-disagreeing candidates,
            //     so user_label branch is required here.)
            Case {
                axis: "role",
                attr_a: attr(Role::Side, 0, Some("seam")),
                attr_b: attr(Role::Cap(CapKind::Top), 0, Some("seam")),
                query: AttributeQuery {
                    user_label: Some("seam".to_string()),
                    role_and_index: None,
                    feature_id: None,
                },
            },
            // (3) local_index axis: both match user_label query ("seam");
            //     parent-keys disagree on local_index → cluster check fails →
            //     Unresolved. (Same reasoning as role axis.)
            Case {
                axis: "local_index",
                attr_a: attr(Role::Side, 0, Some("seam")),
                attr_b: attr(Role::Side, 1, Some("seam")),
                query: AttributeQuery {
                    user_label: Some("seam".to_string()),
                    role_and_index: None,
                    feature_id: None,
                },
            },
            // (4) user_label axis: both match role/idx query (Side, 0);
            //     parent-keys disagree on user_label → cluster check fails →
            //     Unresolved. (user_label branch would filter out
            //     user_label-disagreeing candidates, so role/idx branch is
            //     required here.)
            Case {
                axis: "user_label",
                attr_a: attr(Role::Side, 0, Some("alpha")),
                attr_b: attr(Role::Side, 0, Some("beta")),
                query: AttributeQuery {
                    user_label: None,
                    role_and_index: Some((Role::Side, 0)),
                    feature_id: None,
                },
            },
        ];

        for case in cases {
            let mut table = TopologyAttributeTable::default();
            table.record(h(60), case.attr_a);
            table.record(h(61), case.attr_b);
            let candidates = [h(60), h(61)];
            let mut diagnostics = Vec::new();
            let result = resolve_unique_by_attribute(
                &table,
                &candidates,
                &case.query,
                span(),
                &mut diagnostics,
            );
            assert_eq!(
                result,
                AttributeResolution::Unresolved,
                "[{}] mixed parent-keys must resolve to Unresolved",
                case.axis
            );
            assert_eq!(
                diagnostics.len(),
                1,
                "[{}] expected exactly one diagnostic",
                case.axis
            );
            let diag = &diagnostics[0];
            assert_eq!(
                diag.code,
                Some(DiagnosticCode::TopologyAttributeStale),
                "[{}] expected TopologyAttributeStale diagnostic code",
                case.axis
            );
            assert!(
                diag.message.contains("matched 2 sub-shapes"),
                "[{}] message should contain 'matched 2 sub-shapes', got: {}",
                case.axis,
                diag.message
            );
            assert!(
                !diag.message.contains("split children"),
                "[{}] mixed-parent multi-match must NOT use the split-children sub-form, got: {}",
                case.axis,
                diag.message
            );
        }
    }

    /// Role/idx multi-match where ALL matched candidates share the same
    /// parent-key routes to AmbiguousAfterSplit.
    ///
    /// Two candidates h(60), h(61) carry identical
    /// `(feature_id, role, local_index, user_label)` but distinct
    /// `mod_history` (the split-children signature: same parent, different
    /// `ModEntry`s). The role/idx query matches both. Per PRD line 64, the
    /// resolver surfaces the SET of children (rather than silently
    /// rebinding) via `AmbiguousAfterSplit`, with a
    /// `TopologyAttributeAmbiguousAfterSplit` diagnostic.
    #[test]
    fn resolve_returns_ambiguous_after_split_when_role_idx_match_clusters_on_parent_key() {
        let mut table = TopologyAttributeTable::default();
        // Both entries share the parent-key (feat(), Side, 0, None) and
        // differ only in mod_history — the post-split signature.
        let mut a = attr(Role::Side, 0, None);
        a.mod_history = vec![ModEntry {
            splitting_feature_id: FeatureId::new("Fuse#realization[0]"),
            split_index: 0,
        }];
        let mut b = attr(Role::Side, 0, None);
        b.mod_history = vec![ModEntry {
            splitting_feature_id: FeatureId::new("Fuse#realization[0]"),
            split_index: 1,
        }];
        table.record(h(60), a);
        table.record(h(61), b);
        let candidates = [h(60), h(61)];
        let query = AttributeQuery {
            user_label: None,
            role_and_index: Some((Role::Side, 0)),
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result =
            resolve_unique_by_attribute(&table, &candidates, &query, span(), &mut diagnostics);
        assert_eq!(
            result,
            AttributeResolution::AmbiguousAfterSplit {
                children: vec![h(60), h(61)],
            },
            "matched cluster shares parent-key → AmbiguousAfterSplit with both children in encounter order"
        );
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        let diag = &diagnostics[0];
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::TopologyAttributeAmbiguousAfterSplit)
        );
        assert!(
            diag.message.contains("matched 2 split children"),
            "diagnostic message should interpolate the children count via the wording \
             template ('matched N split children'), got: {}",
            diag.message,
        );
    }

    /// User_label multi-match where ALL matched candidates share the
    /// parent-key routes to AmbiguousAfterSplit.
    ///
    /// Pins that the user_label branch participates in clustering detection
    /// symmetrically with the role/idx branch — i.e. a labelled face that
    /// gets split surfaces as AmbiguousAfterSplit (with the children list
    /// for caller disambiguation), NOT a silent first-match Resolved or a
    /// generic Unresolved miss.
    #[test]
    fn resolve_returns_ambiguous_after_split_when_user_label_match_clusters_on_parent_key() {
        let mut table = TopologyAttributeTable::default();
        // Both entries share the parent-key (feat(), Side, 0, Some("seam"))
        // and differ only in mod_history — the post-split signature.
        let mut a = attr(Role::Side, 0, Some("seam"));
        a.mod_history = vec![ModEntry {
            splitting_feature_id: FeatureId::new("Fuse#realization[0]"),
            split_index: 0,
        }];
        let mut b = attr(Role::Side, 0, Some("seam"));
        b.mod_history = vec![ModEntry {
            splitting_feature_id: FeatureId::new("Fuse#realization[0]"),
            split_index: 1,
        }];
        table.record(h(70), a);
        table.record(h(71), b);
        let candidates = [h(70), h(71)];
        let query = AttributeQuery {
            user_label: Some("seam".to_string()),
            role_and_index: None,
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result =
            resolve_unique_by_attribute(&table, &candidates, &query, span(), &mut diagnostics);
        assert_eq!(
            result,
            AttributeResolution::AmbiguousAfterSplit {
                children: vec![h(70), h(71)],
            },
            "labelled face that gets split → AmbiguousAfterSplit with both children"
        );
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        let diag = &diagnostics[0];
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::TopologyAttributeAmbiguousAfterSplit)
        );
        assert!(
            diag.message.contains("matched 2 split children"),
            "diagnostic message should interpolate the children count via the wording \
             template ('matched N split children'), got: {}",
            diag.message,
        );
    }

    /// Zero-match unresolved diagnostic emission.
    ///
    /// At least one candidate has an attribute entry (so we are NOT in the
    /// imported-geometry fallback case), but the query asks for a role/idx
    /// that no candidate matches. The resolver returns Unresolved and emits
    /// exactly one TopologyAttributeStale Warning with a primary label at
    /// `selector_span` reading "selector call" and a message containing
    /// "matched 0 sub-shapes".
    #[test]
    fn unresolved_with_diagnostic_when_zero_match_but_entries_exist() {
        let mut table = TopologyAttributeTable::default();
        table.record(h(50), attr(Role::Side, 0, None));
        let candidates = [h(50)];
        let selector_span = SourceSpan::new(10, 20);
        let query = AttributeQuery {
            user_label: None,
            role_and_index: Some((Role::Cap(CapKind::Top), 0)),
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result = resolve_unique_by_attribute(
            &table,
            &candidates,
            &query,
            selector_span,
            &mut diagnostics,
        );
        assert_eq!(result, AttributeResolution::Unresolved);
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        let diag = &diagnostics[0];
        assert_eq!(diag.severity, Severity::Warning);
        assert_eq!(diag.code, Some(DiagnosticCode::TopologyAttributeStale));
        assert!(
            diag.message.contains("matched 0 sub-shapes"),
            "message should contain 'matched 0 sub-shapes', got: {}",
            diag.message
        );
        // Primary label at selector_span with the canonical "selector call"
        // text (mirrors resolve_unique_by_tag).
        assert!(!diag.labels.is_empty(), "expected at least one label");
        let primary = &diag.labels[0];
        assert_eq!(primary.span, selector_span);
        assert_eq!(primary.message, "selector call");
    }

    /// step-7 — imported-geometry fallback (PRD line 68).
    ///
    /// Sub-case (a): the table is fully empty. Any non-empty query returns
    /// `FallbackToComputed` (no candidate carries an entry).
    ///
    /// Sub-case (b): the table HAS entries but for handles NOT in
    /// `candidates`. Still `FallbackToComputed` because none of the SUPPLIED
    /// candidates carry an entry; entries for unrelated handles are
    /// irrelevant.
    #[test]
    fn fallback_to_computed_when_no_candidate_has_attribute_entry() {
        // (a) Empty table.
        let table = TopologyAttributeTable::default();
        let candidates = [h(40), h(41)];
        let query = AttributeQuery {
            user_label: Some("anything".to_string()),
            role_and_index: Some((Role::Side, 0)),
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result =
            resolve_unique_by_attribute(&table, &candidates, &query, span(), &mut diagnostics);
        assert_eq!(result, AttributeResolution::FallbackToComputed);
        assert!(
            diagnostics.is_empty(),
            "fallback emits no diagnostic — it is an expected path for imported geometry"
        );

        // (b) Table populated for OTHER handles.
        let mut table_b = TopologyAttributeTable::default();
        table_b.record(h(99), attr(Role::Side, 0, None));
        // Candidates 40/41 still have no entries.
        let mut diagnostics = Vec::new();
        let result_b =
            resolve_unique_by_attribute(&table_b, &candidates, &query, span(), &mut diagnostics);
        assert_eq!(result_b, AttributeResolution::FallbackToComputed);
        assert!(diagnostics.is_empty());
    }

    /// `feature_id` constrains BOTH match branches.
    ///
    /// Two candidates share `(role, local_index) = (Role::Side, 0)` but
    /// originate from different features ("Boss" vs "Slot"). With no
    /// feature_id constraint the role/idx branch would multi-match and
    /// emit Unresolved; constraining by feature_id picks the unique
    /// candidate from the named feature.
    ///
    /// Sub-cases:
    /// - feature_id=Some("Slot") → Resolved(handle 71)
    /// - feature_id=Some("Boss") → Resolved(handle 70)
    /// - feature_id=Some("Other") → Unresolved with zero-match diagnostic
    ///   (entries exist on both candidates but neither matches "Other"
    ///   — distinguishes from FallbackToComputed which fires only when
    ///   NO candidate carries an entry).
    #[test]
    fn feature_id_constraint_filters_candidates() {
        let boss = FeatureId::new("Boss");
        let slot = FeatureId::new("Slot");
        let other = FeatureId::new("Other");
        let mut table = TopologyAttributeTable::default();
        table.record(h(70), attr_for(boss.clone(), Role::Side, 0, None));
        table.record(h(71), attr_for(slot.clone(), Role::Side, 0, None));
        let candidates = [h(70), h(71)];

        // (a) feature_id=Slot → handle 71.
        let query_slot = AttributeQuery {
            user_label: None,
            role_and_index: Some((Role::Side, 0)),
            feature_id: Some(slot.clone()),
        };
        let mut diagnostics = Vec::new();
        let result_slot =
            resolve_unique_by_attribute(&table, &candidates, &query_slot, span(), &mut diagnostics);
        assert_eq!(
            result_slot,
            AttributeResolution::Resolved(h(71)),
            "feature_id=Slot should pick handle 71"
        );
        assert!(diagnostics.is_empty());

        // (b) feature_id=Boss → handle 70.
        let query_boss = AttributeQuery {
            user_label: None,
            role_and_index: Some((Role::Side, 0)),
            feature_id: Some(boss.clone()),
        };
        let mut diagnostics = Vec::new();
        let result_boss =
            resolve_unique_by_attribute(&table, &candidates, &query_boss, span(), &mut diagnostics);
        assert_eq!(
            result_boss,
            AttributeResolution::Resolved(h(70)),
            "feature_id=Boss should pick handle 70"
        );
        assert!(diagnostics.is_empty());

        // (c) feature_id=Other → Unresolved with zero-match diagnostic.
        // Entries exist on candidates (so we are NOT in the
        // imported-geometry FallbackToComputed arm), but no candidate
        // matches feature_id="Other".
        let query_other = AttributeQuery {
            user_label: None,
            role_and_index: Some((Role::Side, 0)),
            feature_id: Some(other),
        };
        let mut diagnostics = Vec::new();
        let result_other = resolve_unique_by_attribute(
            &table,
            &candidates,
            &query_other,
            span(),
            &mut diagnostics,
        );
        assert_eq!(result_other, AttributeResolution::Unresolved);
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        let diag = &diagnostics[0];
        assert_eq!(diag.code, Some(DiagnosticCode::TopologyAttributeStale));
        assert!(
            diag.message.contains("matched 0 sub-shapes"),
            "message should contain 'matched 0 sub-shapes', got: {}",
            diag.message
        );
    }

    /// step-15 — defensive edge cases.
    ///
    /// (a) Empty candidate slice + populated table → FallbackToComputed.
    ///     No supplied candidate carries an entry by definition; the
    ///     resolver routes through computed selectors.
    ///
    /// (b) All-None query on populated candidates → Unresolved with the
    ///     zero-match diagnostic. A query with no constraints matches
    ///     nothing by definition; this defends against accidental
    ///     "match-everything" semantics.
    ///
    /// (c) Duplicate candidate ids → Resolved. The resolver must
    ///     deduplicate before counting matches so a misbehaving extractor
    ///     that returned the same handle three times still yields
    ///     `Resolved(handle)`, mirroring `resolve_unique_by_tag`'s
    ///     defense-in-depth `HashSet::insert` discipline.
    #[test]
    fn edge_cases() {
        // (a) Empty candidate slice + populated table.
        let mut table_a = TopologyAttributeTable::default();
        table_a.record(h(80), attr(Role::Side, 0, None));
        let candidates_a: [GeometryHandleId; 0] = [];
        let query_a = AttributeQuery {
            user_label: None,
            role_and_index: Some((Role::Side, 0)),
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result_a = resolve_unique_by_attribute(
            &table_a,
            &candidates_a,
            &query_a,
            span(),
            &mut diagnostics,
        );
        assert_eq!(
            result_a,
            AttributeResolution::FallbackToComputed,
            "empty candidates → FallbackToComputed"
        );
        assert!(diagnostics.is_empty());

        // (b) All-None query on populated candidates.
        let mut table_b = TopologyAttributeTable::default();
        table_b.record(h(81), attr(Role::Side, 0, Some("anything")));
        let candidates_b = [h(81)];
        let query_b = AttributeQuery {
            user_label: None,
            role_and_index: None,
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result_b = resolve_unique_by_attribute(
            &table_b,
            &candidates_b,
            &query_b,
            span(),
            &mut diagnostics,
        );
        assert_eq!(
            result_b,
            AttributeResolution::Unresolved,
            "all-None query → Unresolved (no constraint matches nothing)"
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code,
            Some(DiagnosticCode::TopologyAttributeStale)
        );
        assert!(
            diagnostics[0].message.contains("matched 0 sub-shapes"),
            "message should contain 'matched 0 sub-shapes', got: {}",
            diagnostics[0].message
        );

        // (c) Duplicate candidate ids → Resolved (dedup before counting).
        let mut table_c = TopologyAttributeTable::default();
        table_c.record(h(80), attr(Role::Side, 0, None));
        // h(80) repeated three times.
        let candidates_c = [h(80), h(80), h(80)];
        let query_c = AttributeQuery {
            user_label: None,
            role_and_index: Some((Role::Side, 0)),
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result_c = resolve_unique_by_attribute(
            &table_c,
            &candidates_c,
            &query_c,
            span(),
            &mut diagnostics,
        );
        assert_eq!(
            result_c,
            AttributeResolution::Resolved(h(80)),
            "duplicate candidates → resolver dedups, single match"
        );
        assert!(diagnostics.is_empty());
    }

    /// Regression pin for the feature_id-only contract (plan #2704).
    ///
    /// A query with `feature_id=Some` but both positional fields
    /// (`user_label` and `role_and_index`) `None` is contractually invalid
    /// per [`AttributeQuery`]'s docs. The resolver intentionally routes such
    /// queries through the all-None positional branch, returning `Unresolved`
    /// with a `TopologyAttributeStale` "matched 0 sub-shapes" diagnostic —
    /// the same outcome as an all-three-fields-None query.
    ///
    /// This is a **behavior pin**, not a doc-string assertion: it locks the
    /// documented contract so a future maintainer cannot silently change the
    /// all-None positional check to also consult `feature_id` and divert
    /// feature_id-only queries to the FallbackToComputed arm (which would
    /// misclassify a caller bug as an imported-geometry signal).
    #[test]
    fn feature_id_only_query_is_treated_as_all_none_positional_miss() {
        // Populate the table with two entries so we are NOT in the
        // imported-geometry FallbackToComputed arm.
        // h(90) is attributed to the default feature (feat()).
        // h(91) is attributed to a different feature — confirming that
        // feature_id is NOT consulted by the all-None positional pre-pass
        // even when some candidates would match the filter.
        let other_feature = FeatureId::new("OtherFeature");
        let mut table = TopologyAttributeTable::default();
        table.record(h(90), attr(Role::Side, 0, None));
        table.record(h(91), attr_for(other_feature, Role::Side, 1, None));
        let candidates = [h(90), h(91)];

        // feature_id=Some, but both positional fields are None.
        // This is a contract-illegal query per AttributeQuery's docs.
        let query = AttributeQuery {
            user_label: None,
            role_and_index: None,
            feature_id: Some(feat()),
        };
        let mut diagnostics = Vec::new();
        let result =
            resolve_unique_by_attribute(&table, &candidates, &query, span(), &mut diagnostics);

        // The all-None positional check fires regardless of feature_id, so
        // the resolver returns Unresolved with the standard zero-match
        // diagnostic. feature_id alone does not supply a query.
        assert_eq!(
            result,
            AttributeResolution::Unresolved,
            "feature_id-only query must be routed through the all-None positional branch"
        );
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        let diag = &diagnostics[0];
        assert_eq!(diag.code, Some(DiagnosticCode::TopologyAttributeStale));
        assert!(
            diag.message.contains("matched 0 sub-shapes"),
            "message should contain 'matched 0 sub-shapes', got: {}",
            diag.message
        );
    }

    /// Pins the populator-bug case where `try_cluster_after_split` would be
    /// too permissive: matching on the parent-key alone is insufficient to
    /// justify routing to `AmbiguousAfterSplit`. If all matched attributes
    /// ALSO share the same `mod_history` (no `split_index` distinguishes
    /// them), the cluster is genuinely indistinguishable and can only arise
    /// from a populator bug. The resolver must fall through to `Unresolved`
    /// with the canonical "matched N sub-shapes" diagnostic rather than
    /// producing an unselectable `AmbiguousAfterSplit { children }` whose
    /// elements the future task-10 `split_by(...)` selector could never
    /// disambiguate.
    ///
    /// Contrast with
    /// `resolve_returns_ambiguous_after_split_when_role_idx_match_clusters_on_parent_key`
    /// (positive case): DISTINCT `split_index` values → `AmbiguousAfterSplit`.
    /// This test (negative case): IDENTICAL `mod_history` → `Unresolved`.
    #[test]
    fn resolve_keeps_unresolved_when_matches_share_parent_key_and_mod_history() {
        let mut table = TopologyAttributeTable::default();
        // Both entries share the parent-key (feat(), Side, 0, None) AND the
        // same mod_history — the populator-bug signature (no split_index
        // distinguishes them).
        let mut a = attr(Role::Side, 0, None);
        a.mod_history = vec![ModEntry {
            splitting_feature_id: FeatureId::new("Fuse#realization[0]"),
            split_index: 0,
        }];
        let mut b = attr(Role::Side, 0, None);
        b.mod_history = vec![ModEntry {
            splitting_feature_id: FeatureId::new("Fuse#realization[0]"),
            split_index: 0, // same as a — populator bug: identical split_index
        }];
        table.record(h(60), a);
        table.record(h(61), b);
        let candidates = [h(60), h(61)];
        let query = AttributeQuery {
            user_label: None,
            role_and_index: Some((Role::Side, 0)),
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result =
            resolve_unique_by_attribute(&table, &candidates, &query, span(), &mut diagnostics);
        assert_eq!(
            result,
            AttributeResolution::Unresolved,
            "identical mod_history → Unresolved, not AmbiguousAfterSplit (populator-bug case)"
        );
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        let diag = &diagnostics[0];
        assert_eq!(diag.code, Some(DiagnosticCode::TopologyAttributeStale));
        assert!(
            diag.message.contains("matched 2 sub-shapes"),
            "message should contain 'matched 2 sub-shapes' (canonical sub-form), got: {}",
            diag.message
        );
        assert!(
            !diag.message.contains("split children"),
            "populator-bug case must NOT use the split-children sub-form, got: {}",
            diag.message
        );
    }

    /// Populator-bug case where both attrs have the *default* empty `mod_history`
    /// (`Vec::new()`). Before this fix two such attrs would wrongly route to
    /// `AmbiguousAfterSplit`; this is the canonical regression since `attr()`
    /// already defaults to `mod_history: Vec::new()` with no explicit setup.
    ///
    /// Complements `resolve_keeps_unresolved_when_matches_share_parent_key_and_mod_history`
    /// (which uses a non-empty identical `mod_history`). Empty mod_history is the
    /// most common populator-bug form and was the original motivation for the fix.
    #[test]
    fn resolve_keeps_unresolved_when_matches_share_parent_key_and_empty_mod_history() {
        let mut table = TopologyAttributeTable::default();
        // Both attrs use the default empty mod_history — no split has been
        // recorded at all, yet the parent-key matches. Classic populator bug.
        table.record(h(70), attr(Role::Side, 0, None));
        table.record(h(71), attr(Role::Side, 0, None));
        let candidates = [h(70), h(71)];
        let query = AttributeQuery {
            user_label: None,
            role_and_index: Some((Role::Side, 0)),
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result =
            resolve_unique_by_attribute(&table, &candidates, &query, span(), &mut diagnostics);
        assert_eq!(
            result,
            AttributeResolution::Unresolved,
            "empty mod_history → Unresolved, not AmbiguousAfterSplit (populator-bug case)"
        );
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        let diag = &diagnostics[0];
        assert_eq!(diag.code, Some(DiagnosticCode::TopologyAttributeStale));
        assert!(
            diag.message.contains("matched 2 sub-shapes"),
            "message should contain 'matched 2 sub-shapes' (canonical sub-form), got: {}",
            diag.message
        );
        assert!(
            !diag.message.contains("split children"),
            "empty-mod_history case must NOT use the split-children sub-form, got: {}",
            diag.message
        );
    }

    /// Three-handle populator-bug case with empty `mod_history`. Confirms that
    /// the diagnostic count scales correctly ("matched 3 sub-shapes") and that
    /// the all(distinct) predicate continues to return None for any cluster size
    /// ≥ 2 where no split_index distinguishes the elements.
    #[test]
    fn resolve_keeps_unresolved_for_three_matches_with_empty_mod_history() {
        let mut table = TopologyAttributeTable::default();
        table.record(h(80), attr(Role::Side, 0, None));
        table.record(h(81), attr(Role::Side, 0, None));
        table.record(h(82), attr(Role::Side, 0, None));
        let candidates = [h(80), h(81), h(82)];
        let query = AttributeQuery {
            user_label: None,
            role_and_index: Some((Role::Side, 0)),
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result =
            resolve_unique_by_attribute(&table, &candidates, &query, span(), &mut diagnostics);
        assert_eq!(
            result,
            AttributeResolution::Unresolved,
            "three handles with empty mod_history → Unresolved (populator-bug case)"
        );
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        let diag = &diagnostics[0];
        assert_eq!(diag.code, Some(DiagnosticCode::TopologyAttributeStale));
        assert!(
            diag.message.contains("matched 3 sub-shapes"),
            "message should contain 'matched 3 sub-shapes' (count scales with handle count), got: {}",
            diag.message
        );
        assert!(
            !diag.message.contains("split children"),
            "empty-mod_history case must NOT use the split-children sub-form, got: {}",
            diag.message
        );
    }

    /// Pins the `windows(2).all(distinct)` predicate in `try_cluster_after_split`
    /// for the **partially-duplicate** `[A, A, B]` mod_history cluster explicitly
    /// called out in the function docstring.
    ///
    /// `windows(2)` yields pairs `(A, A)` and `(A, B)`. The first pair has
    /// equal `mod_history` → `all(distinct)` returns `false` → the cluster is
    /// rejected → the resolver falls through to `Unresolved` with the canonical
    /// "matched N sub-shapes" diagnostic.
    ///
    /// A regression that weakened the predicate to a first/last-only comparison
    /// (`cluster[0].mod_history != cluster.last().mod_history`) would evaluate
    /// A vs B, find them distinct, and incorrectly return `AmbiguousAfterSplit`
    /// with the "split children" message — silently breaking this case while
    /// still passing the existing uniformly-identical-mod_history tests.
    ///
    /// Contrast with:
    /// - `resolve_returns_ambiguous_after_split_when_role_idx_match_clusters_on_parent_key`
    ///   (positive case): ALL `mod_history` values distinct → `AmbiguousAfterSplit`.
    /// - `resolve_keeps_unresolved_when_matches_share_parent_key_and_mod_history`
    ///   (negative case): ALL `mod_history` values identical → `Unresolved`.
    ///
    /// This test: PARTIALLY duplicate `[A, A, B]` → must also yield `Unresolved`.
    #[test]
    fn resolve_keeps_unresolved_when_partial_duplicate_mod_history() {
        let mut table = TopologyAttributeTable::default();
        // A = split_index 0, B = split_index 1; pattern is [A, A, B].
        // The (A, A) window has equal mod_history, so all(distinct) → false.
        let mod_entry_a = vec![ModEntry {
            splitting_feature_id: FeatureId::new("Fuse#realization[0]"),
            split_index: 0,
        }];
        let mod_entry_b = vec![ModEntry {
            splitting_feature_id: FeatureId::new("Fuse#realization[0]"),
            split_index: 1,
        }];
        let mut h60 = attr(Role::Side, 0, None);
        h60.mod_history = mod_entry_a.clone(); // A
        let mut h61 = attr(Role::Side, 0, None);
        h61.mod_history = mod_entry_a; // A (duplicate)
        let mut h62 = attr(Role::Side, 0, None);
        h62.mod_history = mod_entry_b; // B
        table.record(h(60), h60);
        table.record(h(61), h61);
        table.record(h(62), h62);
        let candidates = [h(60), h(61), h(62)];
        let query = AttributeQuery {
            user_label: None,
            role_and_index: Some((Role::Side, 0)),
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result =
            resolve_unique_by_attribute(&table, &candidates, &query, span(), &mut diagnostics);
        assert_eq!(
            result,
            AttributeResolution::Unresolved,
            "partial-duplicate [A,A,B] mod_history → Unresolved, not AmbiguousAfterSplit"
        );
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        let diag = &diagnostics[0];
        assert_eq!(diag.code, Some(DiagnosticCode::TopologyAttributeStale));
        assert!(
            diag.message.contains("matched 3 sub-shapes"),
            "message should contain 'matched 3 sub-shapes' (canonical sub-form), got: {}",
            diag.message
        );
        assert!(
            !diag.message.contains("split children"),
            "partial-duplicate case must NOT use the split-children sub-form, got: {}",
            diag.message
        );
    }

    /// Cross-branch override: `user_label` zero-match sets
    /// `last_count = Some(0)` at line 230; `role_and_index` multi-match
    /// overwrites it with `last_count = Some(n)` at line 274.  The final
    /// `emit_attribute_stale_diagnostic` at line 287 must therefore report
    /// the role/idx count (2), NOT the user_label count (0).
    ///
    /// This is the only execution path where the `last_count = Some(0)`
    /// plumbing changes observable behavior:
    ///   • unique match → short-circuits before the stale-attribute emission
    ///   • user_label multi-match without split-cluster → emits its own
    ///     diagnostic and returns before the role/idx branch fires
    ///   • role/idx-only queries → user_label branch never sets
    ///     `last_count` at all
    ///
    /// Contrast tests:
    ///   • `unresolved_with_diagnostic_when_multi_match` — role/idx-only
    ///     multi-match (no user_label branch), same fixture shape
    ///   • `user_label_preferred_over_role_and_index_when_both_apply`
    ///     sub-case b — user_label-zero falls through to a UNIQUE role/idx
    ///     match (resolved, no diagnostic)
    #[test]
    fn user_label_zero_match_role_idx_multi_match_uses_role_idx_count_in_diagnostic() {
        let boss = FeatureId::new("Boss");
        let slot = FeatureId::new("Slot");
        let mut table = TopologyAttributeTable::default();
        // Distinct feature_ids on the two matched candidates → mixed
        // parent-keys → cluster check fails → Unresolved.
        table.record(h(60), attr_for(boss, Role::Side, 0, None));
        table.record(h(61), attr_for(slot, Role::Side, 0, None));
        let candidates = [h(60), h(61)];
        // user_label "missing" matches neither candidate: sets last_count = Some(0).
        // role_and_index (Role::Side, 0) matches both: sets last_count = Some(2).
        let query = AttributeQuery {
            user_label: Some("missing".to_string()),
            role_and_index: Some((Role::Side, 0)),
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result =
            resolve_unique_by_attribute(&table, &candidates, &query, span(), &mut diagnostics);

        // Neither branch produced a unique match; mixed parent-keys mean
        // try_cluster_after_split returns None → Unresolved.
        assert_eq!(result, AttributeResolution::Unresolved);
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        let diag = &diagnostics[0];
        assert_eq!(diag.code, Some(DiagnosticCode::TopologyAttributeStale));
        // Core override pin: role/idx count (2) must override user_label count (0).
        assert!(
            diag.message.contains("matched 2 sub-shapes"),
            "message should contain 'matched 2 sub-shapes' (role/idx count), got: {}",
            diag.message
        );
        // Regression guard: must NOT report the user_label zero-count.
        assert!(
            !diag.message.contains("matched 0 sub-shapes"),
            "message must NOT report the user_label zero-count, got: {}",
            diag.message
        );
        // Routing guard: mixed parent-keys must NEVER use the split-children sub-form.
        assert!(
            !diag.message.contains("split children"),
            "mixed-parent multi-match must NOT use the split-children sub-form, got: {}",
            diag.message
        );
    }
}
