//! Tolerance-scope extraction from purpose declarations.
//!
//! Activates the dormant tolerance-scope infrastructure described in
//! `docs/prds/v0_2/per-purpose-tolerance.md` ("Resolved design decisions"
//! section, "Tolerance lives at the purpose"): walks each active purpose's
//! subject graph, extracts every `RepresentationWithin(subject, tolerance)`
//! constraint, propagates the tolerance onto every reachable entity, and
//! combines contributions across purposes via `min` (tighter satisfies
//! looser — same partial-order semantics as the cache-side
//! `ToleranceBucket`).
//!
//! # MVP scope clip
//!
//! Today this module only recognises `RepresentationWithin(<bare-param>, <length-literal>)`
//! where `<bare-param>` is the purpose's `StructureRef`-typed parameter. Member-access
//! subjects (`RepresentationWithin(subject.head, tol)`) are deferred to a follow-up —
//! the PRD's "tighter entity-level overrides" semantics is fully achievable in the MVP
//! via *multiple active purposes with overlapping subjects* (purpose A bound to
//! `bracket`, purpose B bound to `bracket.head`, with B tighter).

use crate::graph::ValueCellNode;
use reify_compiler::CompiledPurpose;
use reify_types::{CompiledExprKind, DimensionVector, PersistentMap, Type, Value, ValueCellId};
use std::collections::{BTreeSet, HashMap};

/// One extracted tolerance scope root: the entity-ref the purpose was bound
/// to, and the SI tolerance (metres) carried by the matching
/// `RepresentationWithin` constraint.
#[derive(Debug, Clone, PartialEq)]
pub struct ToleranceBinding {
    pub subject_entity: String,
    pub si_tolerance: f64,
}

/// Walk `purpose.constraints` and extract every
/// `RepresentationWithin(<bare-param-StructureRef>, <length-literal>)`
/// binding, anchored on `bound_entity_ref`.
///
/// Non-matching constraints are silently skipped — this matches the PRD's
/// "activate dormant infrastructure" posture: a constraint that doesn't
/// match the recognised shape simply contributes no tolerance.
pub fn extract_tolerance_bindings(
    purpose: &CompiledPurpose,
    bound_entity_ref: &str,
) -> Vec<ToleranceBinding> {
    let mut bindings = Vec::new();
    for constraint in &purpose.constraints {
        // Match: top-level UserFunctionCall("RepresentationWithin", [arg0, arg1])
        let (function_name, args) = match &constraint.expr.kind {
            CompiledExprKind::UserFunctionCall {
                function_name,
                args,
            } => (function_name, args),
            _ => continue,
        };
        if function_name != "RepresentationWithin" {
            continue;
        }
        if args.len() != 2 {
            continue;
        }

        // arg0 must be a ValueRef whose result_type is StructureRef(_).
        let subject_arg = &args[0];
        if !matches!(subject_arg.kind, CompiledExprKind::ValueRef(_)) {
            continue;
        }
        if !matches!(subject_arg.result_type, Type::StructureRef(_)) {
            continue;
        }

        // arg1 must be a Literal(Value::Scalar { dimension == LENGTH, .. }).
        let tol_arg = &args[1];
        let si_value = match &tol_arg.kind {
            CompiledExprKind::Literal(Value::Scalar {
                si_value,
                dimension,
            }) if *dimension == DimensionVector::LENGTH => *si_value,
            _ => continue,
        };

        bindings.push(ToleranceBinding {
            subject_entity: bound_entity_ref.to_string(),
            si_tolerance: si_value,
        });
    }
    bindings
}

/// Collect every distinct entity-ref reachable from `subject` in the
/// post-elaboration value-cell graph: the subject itself, plus every entity
/// whose ref begins with `subject + "."` (the dot-separated descendant
/// encoding established by `unfold.rs`).
///
/// Iteration order is determinised via `BTreeSet` (ascending lexical) —
/// `PersistentMap` does not guarantee stable iteration across runs, so we
/// sort. This mirrors the convention in `engine_purposes.rs::expand_purpose
/// _reflective_placeholders` fallback scan.
pub fn propagate_subject_to_descendants(
    subject: &str,
    value_cells: &PersistentMap<ValueCellId, ValueCellNode>,
) -> Vec<String> {
    // Compute the dot-boundary prefix once; without it, `"AB"` would match
    // `starts_with("A")` and silently leak in as a descendant of `"A"`.
    let prefix = format!("{}.", subject);
    let mut entities: BTreeSet<String> = BTreeSet::new();
    for (id, _) in value_cells.iter() {
        if id.entity == subject || id.entity.starts_with(&prefix) {
            entities.insert(id.entity.clone());
        }
    }
    entities.into_iter().collect()
}

/// Merge `additions` into `scope`, applying `min` per entity (tighter
/// satisfies looser — same partial-order semantics as the cache-side
/// `ToleranceBucket`). Entries new to `scope` are inserted as-is.
pub fn merge_with_min<I: IntoIterator<Item = (String, f64)>>(
    scope: &mut HashMap<String, f64>,
    additions: I,
) {
    for (entity, tol) in additions {
        scope
            .entry(entity)
            .and_modify(|cur| {
                if tol < *cur {
                    *cur = tol;
                }
            })
            .or_insert(tol);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::ValueCellNode;
    use reify_compiler::ValueCellKind;
    use reify_test_support::builders::CompiledPurposeBuilder;
    use reify_types::{
        BinOp, CompiledExpr, ContentHash, DimensionVector, PersistentMap, Type, Value, ValueCellId,
    };
    use std::collections::HashMap;

    /// Build a one-cell `PersistentMap` entry shaped like the existing
    /// `engine_purposes.rs` unit-test fixtures: a `Param` cell typed
    /// `Type::Real`, with a content_hash derived from the member name.
    fn insert_param_cell(
        cells: &mut PersistentMap<ValueCellId, ValueCellNode>,
        entity: &str,
        member: &str,
    ) {
        let id = ValueCellId::new(entity, member);
        cells.insert(
            id.clone(),
            ValueCellNode {
                id: id.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str(&format!("{}.{}", entity, member)),
            },
        );
    }

    /// Build the canonical `RepresentationWithin(<ValueRef typed
    /// StructureRef>, <Literal Scalar(LENGTH)>)` shape that
    /// `extract_tolerance_bindings` is expected to recognise.
    fn representation_within_constraint(
        subject_kind: &str,
        si_value: f64,
        dimension: DimensionVector,
    ) -> CompiledExpr {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef(subject_kind.to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar { si_value, dimension },
            Type::Scalar { dimension },
        );
        CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        )
    }

    #[test]
    fn extract_tolerance_bindings_returns_single_binding_for_one_representation_within() {
        let constraint_expr = representation_within_constraint(
            "Bracket",
            1e-6,
            DimensionVector::LENGTH,
        );

        let purpose = CompiledPurposeBuilder::new("manufacturing")
            .param("subject", "Structure")
            .constraint("subject", 0, None, constraint_expr)
            .build();

        let bindings = extract_tolerance_bindings(&purpose, "MyDesign");

        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].subject_entity, "MyDesign");
        assert_eq!(bindings[0].si_tolerance, 1e-6);
    }

    #[test]
    fn extract_tolerance_bindings_skips_non_tolerance_constraints() {
        // (a) UserFunctionCall("AllParamsDetermined", [...]) — wrong function name.
        let all_params_determined = CompiledExpr::user_function_call(
            "AllParamsDetermined".to_string(),
            vec![CompiledExpr::value_ref(
                ValueCellId::new("subject", "self"),
                Type::StructureRef("Bracket".to_string()),
            )],
            Type::Bool,
        );

        // (b) BinOp(Gt, ValueRef(...), Literal(Real(0.0))) — wrong outer node kind.
        let binop_constraint = CompiledExpr::binop(
            BinOp::Gt,
            CompiledExpr::value_ref(
                ValueCellId::new("subject", "thickness"),
                Type::Real,
            ),
            CompiledExpr::literal(Value::Real(0.0), Type::Real),
            Type::Bool,
        );

        // (c) A valid RepresentationWithin.
        let rep_within = representation_within_constraint(
            "Bracket",
            5e-6,
            DimensionVector::LENGTH,
        );

        let purpose = CompiledPurposeBuilder::new("manufacturing")
            .param("subject", "Structure")
            .constraint("subject", 0, None, all_params_determined)
            .constraint("subject", 1, None, binop_constraint)
            .constraint("subject", 2, None, rep_within)
            .build();

        let bindings = extract_tolerance_bindings(&purpose, "MyDesign");

        assert_eq!(
            bindings.len(),
            1,
            "only the RepresentationWithin constraint should yield a binding"
        );
        assert_eq!(bindings[0].subject_entity, "MyDesign");
        assert_eq!(bindings[0].si_tolerance, 5e-6);
    }

    #[test]
    fn extract_tolerance_bindings_skips_representation_within_with_non_length_dimension() {
        // RepresentationWithin whose 2nd arg is a *dimensionless* literal —
        // must be silently skipped (returns empty).
        let constraint_expr = representation_within_constraint(
            "Bracket",
            1.0,
            DimensionVector::DIMENSIONLESS,
        );

        let purpose = CompiledPurposeBuilder::new("manufacturing")
            .param("subject", "Structure")
            .constraint("subject", 0, None, constraint_expr)
            .build();

        let bindings = extract_tolerance_bindings(&purpose, "MyDesign");

        assert!(
            bindings.is_empty(),
            "non-LENGTH dimension on tolerance arg must not yield a binding"
        );
    }

    #[test]
    fn propagate_subject_to_descendants_collects_subject_and_dotted_descendants() {
        let mut cells: PersistentMap<ValueCellId, ValueCellNode> = PersistentMap::default();
        // Subject + dotted descendants — every entity needs at least one cell
        // for prefix-scan to surface it.
        insert_param_cell(&mut cells, "A", "x");
        insert_param_cell(&mut cells, "A.x", "len");
        insert_param_cell(&mut cells, "A.y", "len");
        insert_param_cell(&mut cells, "A.x.z", "len");
        // Unrelated subject — must NOT appear in the result.
        insert_param_cell(&mut cells, "B", "len");

        let descendants = propagate_subject_to_descendants("A", &cells);

        assert_eq!(
            descendants,
            vec![
                "A".to_string(),
                "A.x".to_string(),
                "A.x.z".to_string(),
                "A.y".to_string(),
            ],
            "result must equal subject + dotted descendants in ascending order, \
             excluding unrelated entities"
        );
    }

    #[test]
    fn propagate_subject_does_not_match_prefix_lookalikes() {
        // Without the dot-boundary check, `"A".starts_with(prefix-of("A"))`
        // would silently include `"AB"` as if it were a descendant.
        let mut cells: PersistentMap<ValueCellId, ValueCellNode> = PersistentMap::default();
        insert_param_cell(&mut cells, "A", "len");
        insert_param_cell(&mut cells, "AB", "len");

        let descendants = propagate_subject_to_descendants("A", &cells);

        assert_eq!(
            descendants,
            vec!["A".to_string()],
            "prefix-lookalike entity (`AB`) must NOT be matched by propagate(`A`)"
        );
    }

    #[test]
    fn merge_with_min_keeps_tighter_when_existing_is_looser() {
        // scope = {"A": 50e-6}, additions = [("A", 1e-6)] → scope == {"A": 1e-6}
        let mut scope: HashMap<String, f64> = HashMap::new();
        scope.insert("A".to_string(), 50e-6);

        merge_with_min(&mut scope, vec![("A".to_string(), 1e-6)]);

        assert_eq!(scope.len(), 1);
        assert_eq!(scope.get("A"), Some(&1e-6));
    }

    #[test]
    fn merge_with_min_keeps_existing_when_addition_is_looser() {
        // scope = {"A": 1e-6}, additions = [("A", 50e-6)] → scope == {"A": 1e-6}
        let mut scope: HashMap<String, f64> = HashMap::new();
        scope.insert("A".to_string(), 1e-6);

        merge_with_min(&mut scope, vec![("A".to_string(), 50e-6)]);

        assert_eq!(scope.len(), 1);
        assert_eq!(scope.get("A"), Some(&1e-6));
    }

    #[test]
    fn merge_with_min_inserts_new_entries() {
        // scope = {}, additions = [("A", 1e-6), ("B", 5e-6)]
        //   → scope == {"A": 1e-6, "B": 5e-6}
        let mut scope: HashMap<String, f64> = HashMap::new();

        merge_with_min(
            &mut scope,
            vec![("A".to_string(), 1e-6), ("B".to_string(), 5e-6)],
        );

        assert_eq!(scope.len(), 2);
        assert_eq!(scope.get("A"), Some(&1e-6));
        assert_eq!(scope.get("B"), Some(&5e-6));
    }
}
