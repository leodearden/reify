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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeQuery {
    pub user_label: Option<String>,
    pub role_and_index: Option<(Role, u32)>,
    pub feature_id: Option<FeatureId>,
}

/// Three-arm resolution outcome.
///
/// PRD line 68 mandates that "no construction history" (imported geometry)
/// is reported separately from "match failed", so callers can route through
/// computed selectors on the former and emit a diagnostic on the latter.
/// Folding both into `Option::None` would force every caller to re-derive
/// the difference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeResolution {
    /// Exactly one candidate matched the query.
    Resolved(GeometryHandleId),
    /// None of the supplied candidates carry an attribute entry — the
    /// imported-geometry signal. Callers route through computed selectors.
    FallbackToComputed,
    /// At least one candidate has an entry, but the query produced zero or
    /// multiple matches. A `TopologyAttributeStale` diagnostic has been
    /// pushed to the supplied `diagnostics` vec.
    Unresolved,
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
    // selectors. Dedup defensively — a misbehaving extractor that returned
    // duplicates would otherwise still trigger the fallback correctly, but
    // a HashSet keeps the contract symmetric with the match counter below.
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(candidates.len());
    let mut any_has_entry = false;
    for &id in candidates {
        if seen.insert(id) && table.lookup(id).is_some() {
            any_has_entry = true;
            break;
        }
    }
    if !any_has_entry {
        return AttributeResolution::FallbackToComputed;
    }

    // (step-16b) All-None query: the resolver has nothing to match against.
    // Treat this as a zero-match query so the caller surfaces the same
    // diagnostic shape as a stale-attribute miss. Defends against
    // accidental "match-everything" semantics that would otherwise arise
    // if a future caller construction-defaulted all three fields to None.
    // Placed AFTER the fallback pre-pass so imported-geometry routing
    // still wins on import-style candidate sets (consistent with the
    // FallbackToComputed-takes-priority decision in the resolver
    // contract).
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
    if let Some(label) = query.user_label.as_deref() {
        let (found, n) = count_unique_matches(table, candidates, |attr| {
            attr.user_label.as_deref() == Some(label) && feature_id_filter(attr)
        });
        match n {
            1 => return AttributeResolution::Resolved(found.unwrap()),
            // Zero matches: fall through to role/idx branch.
            0 => last_count = Some(0),
            // Multi-match: explicitly do NOT fall through. The role/idx
            // branch is skipped so an authored label collision is not
            // silently converted to a role-based match. Emit the
            // count-aware stale diagnostic and surface the ambiguity.
            _ => {
                emit_attribute_stale_diagnostic(selector_span, n, diagnostics);
                return AttributeResolution::Unresolved;
            }
        }
    }
    // role + local_index branch (step-4).
    if let Some((role, idx)) = query.role_and_index {
        let (found, n) = count_unique_matches(table, candidates, |attr| {
            attr.role == role && attr.local_index == idx && feature_id_filter(attr)
        });
        if n == 1 {
            return AttributeResolution::Resolved(found.unwrap());
        }
        last_count = Some(n);
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
/// to the attribute. Returns `(first_matching_handle, total_match_count)`.
///
/// Mirrors `resolve_unique_by_tag`'s zero/one/many counting discipline. The
/// returned count is exactly the number of candidates that matched the
/// predicate; callers branch on `0` / `1` / `>1` to decide whether to
/// resolve, fall through, or emit a diagnostic.
fn count_unique_matches<F>(
    table: &TopologyAttributeTable,
    candidates: &[GeometryHandleId],
    predicate: F,
) -> (Option<GeometryHandleId>, usize)
where
    F: Fn(&TopologyAttribute) -> bool,
{
    // (step-16c) Deduplicate candidate ids before counting. Mirrors
    // `resolve_unique_by_tag` at topology_selectors.rs:703 so a
    // misbehaving extractor that returned the same handle multiple times
    // does not inflate the match count and spuriously trigger an
    // ambiguity diagnostic.
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(candidates.len());
    let mut found: Option<GeometryHandleId> = None;
    let mut n: usize = 0;
    for &id in candidates {
        if !seen.insert(id) {
            continue;
        }
        if let Some(attr) = table.lookup(id)
            && predicate(attr)
        {
            n += 1;
            if n == 1 {
                found = Some(id);
            }
        }
    }
    (found, n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{CapKind, FeatureId, Severity};

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
        assert!(diagnostics.is_empty(), "no diagnostics expected on a unique user_label match");
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
        assert!(diagnostics.is_empty(), "no diagnostics expected on a unique role/idx match");
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

    /// step-11 — multi-match unresolved diagnostic.
    ///
    /// Two candidates carry the same `(role, local_index)`. The role/idx
    /// query matches BOTH, so the resolver returns Unresolved with a
    /// TopologyAttributeStale Warning whose message contains "matched 2
    /// sub-shapes". (The v0.2 invariant that splits go to mod_history
    /// rather than reusing local_index is enforced by the populator; the
    /// resolver still defends against degenerate inputs.)
    #[test]
    fn unresolved_with_diagnostic_when_multi_match() {
        let mut table = TopologyAttributeTable::default();
        table.record(h(60), attr(Role::Side, 0, None));
        table.record(h(61), attr(Role::Side, 0, None));
        let candidates = [h(60), h(61)];
        let query = AttributeQuery {
            user_label: None,
            role_and_index: Some((Role::Side, 0)),
            feature_id: None,
        };
        let mut diagnostics = Vec::new();
        let result =
            resolve_unique_by_attribute(&table, &candidates, &query, span(), &mut diagnostics);
        assert_eq!(result, AttributeResolution::Unresolved);
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        let diag = &diagnostics[0];
        assert_eq!(diag.code, Some(DiagnosticCode::TopologyAttributeStale));
        assert!(
            diag.message.contains("matched 2 sub-shapes"),
            "message should contain 'matched 2 sub-shapes', got: {}",
            diag.message
        );
    }

    /// step-9 — zero-match unresolved diagnostic emission.
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
        let result =
            resolve_unique_by_attribute(&table, &candidates, &query, selector_span, &mut diagnostics);
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

    /// step-13 — `feature_id` constrains BOTH match branches.
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
        let result_slot = resolve_unique_by_attribute(
            &table,
            &candidates,
            &query_slot,
            span(),
            &mut diagnostics,
        );
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
        let result_boss = resolve_unique_by_attribute(
            &table,
            &candidates,
            &query_boss,
            span(),
            &mut diagnostics,
        );
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
}
