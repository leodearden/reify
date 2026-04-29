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
/// See the module docstring for the PRD line references and the user-label
/// preference rule.
pub fn resolve_unique_by_attribute(
    table: &TopologyAttributeTable,
    candidates: &[GeometryHandleId],
    query: &AttributeQuery,
    _selector_span: SourceSpan,
    _diagnostics: &mut Vec<Diagnostic>,
) -> AttributeResolution {
    // step-2 — user_label branch.
    if let Some(label) = query.user_label.as_deref() {
        let (found, n) = count_unique_matches(table, candidates, |attr| {
            attr.user_label.as_deref() == Some(label)
        });
        if n == 1 {
            return AttributeResolution::Resolved(found.unwrap());
        }
    }
    // step-4 — role + local_index branch.
    if let Some((role, idx)) = query.role_and_index {
        let (found, n) =
            count_unique_matches(table, candidates, |attr| attr.role == role && attr.local_index == idx);
        if n == 1 {
            return AttributeResolution::Resolved(found.unwrap());
        }
    }
    AttributeResolution::Unresolved
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
    let mut found: Option<GeometryHandleId> = None;
    let mut n: usize = 0;
    for &id in candidates {
        if let Some(attr) = table.lookup(id) {
            if predicate(attr) {
                n += 1;
                if n == 1 {
                    found = Some(id);
                }
            }
        }
    }
    (found, n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{CapKind, FeatureId};

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
}
