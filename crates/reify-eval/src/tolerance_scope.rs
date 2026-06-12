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
use reify_core::{DimensionVector, Type, ValueCellId};
use reify_ir::{CompiledExprKind, PersistentMap, Value};
use std::collections::{BTreeSet, HashMap};

/// One extracted tolerance scope root: the entity-ref the purpose was bound
/// to, and the SI tolerance (metres) carried by the matching
/// `RepresentationWithin` constraint.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToleranceBinding {
    pub subject_entity: String,
    pub si_tolerance: f64,
}

/// Walk `purpose.constraints` and extract every
/// `RepresentationWithin(<bare-param-StructureRef>, <length-literal>)`
/// binding, routing each matched constraint's subject param to its own bound
/// entity from `bindings`.
///
/// Non-matching constraints are silently skipped — this matches the PRD's
/// "activate dormant infrastructure" posture: a constraint that doesn't
/// match the recognised shape simply contributes no tolerance.
///
/// # Validation gates
///
/// 1. **Outer shape:** top-level `UserFunctionCall("RepresentationWithin", [arg0, arg1])`.
/// 2. **Subject (arg0):** `ValueRef` whose `result_type` is `StructureRef(_)` AND whose
///    `ValueCellId.entity` matches one of `purpose.params[*].name` (the "bare-purpose-param"
///    contract). A `ValueRef` to a non-param entity is rejected even if it happens to be
///    typed `StructureRef(_)`, so an unrelated structure reference doesn't silently
///    bind a tolerance to an entity it has no semantic connection to.
/// 3. **Tolerance literal (arg1):** `Literal(Value::Scalar { dimension == LENGTH, si_value })`
///    where `si_value.is_finite() && si_value >= 0.0`. Non-finite values (NaN, ±Inf) and
///    negative finite values have no semantics for a tolerance — and worse, both would
///    propagate into the scope and corrupt downstream consumers. NaN sticks because
///    `merge_with_min` could never displace it (NaN comparisons evaluate false); a
///    negative literal would win `merge_with_min` against any positive contributor and
///    then panic `combine_demanded_tolerance`'s debug-assert `is_finite() && >= 0.0`
///    in debug builds (or silently win an `o.min(p)` race in release). The
///    `>= 0.0` half of the gate restores the symmetry `tolerance_combine.rs`'s
///    "Recognition-shape twin" docstring claims with `extract_output_tolerance_bound`.
///
/// # Per-param binding
///
/// For each matched constraint, the subject's param name (`subject_cell_id.entity`, already
/// membership-checked against `purpose.params`) is looked up in `bindings` to resolve the
/// per-param bound entity. This routes each `RepresentationWithin` constraint's
/// `ToleranceBinding.subject_entity` to the entity that was specifically bound to that param
/// at activation time, rather than collapsing all constraints onto a single ref.
///
/// If a param passes the membership gate but has no entry in `bindings`, its constraint is
/// skipped. This cannot occur for validly-activated purposes (C2 ensures every declared param
/// has a binding), but is a safe no-op for defensive handling.
pub(crate) fn extract_tolerance_bindings(
    purpose: &CompiledPurpose,
    bindings: &[(String, String)],
) -> Vec<ToleranceBinding> {
    let mut result = Vec::new();
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

        // arg0 must be a ValueRef whose result_type is StructureRef(_) AND whose
        // entity matches one of the purpose's param names. The entity check is
        // what enforces the "bare-purpose-param subject" contract documented at
        // the module level — without it, a `RepresentationWithin(<unrelated
        // StructureRef ValueRef>, tol)` would silently bind a tolerance to an
        // unrelated entity. Today's compiler emits no such shape, but matching
        // the documented contract prevents surprises when a real producer comes online.
        let subject_arg = &args[0];
        let subject_cell_id = match &subject_arg.kind {
            CompiledExprKind::ValueRef(id) => id,
            _ => continue,
        };
        if !matches!(subject_arg.result_type, Type::StructureRef(_)) {
            continue;
        }
        if !purpose
            .params
            .iter()
            .any(|p| p.name == subject_cell_id.entity)
        {
            continue;
        }

        // Resolve the param's bound entity from the bindings slice. Each param routes
        // to its own entity so multi-param purposes produce per-param ToleranceBindings
        // rather than collapsing all constraints onto a single ref.
        let subject_entity = match bindings.iter().find(|(p, _)| p == &subject_cell_id.entity) {
            Some((_, entity)) => entity.clone(),
            // Param not in bindings — skip. Defensively safe; valid activations guarantee
            // every declared param has a binding (C2).
            None => continue,
        };

        // arg1 must be a Literal(Value::Scalar { dimension == LENGTH, si_value })
        // where si_value.is_finite() && si_value >= 0.0 — see gate-3 in the
        // function docstring above for the full rationale.
        let tol_arg = &args[1];
        let si_value = match &tol_arg.kind {
            CompiledExprKind::Literal(Value::Scalar {
                si_value,
                dimension,
            }) if *dimension == DimensionVector::LENGTH => *si_value,
            _ => continue,
        };
        if !crate::tolerance_gate::is_valid_tolerance_si(si_value) {
            continue;
        }

        result.push(ToleranceBinding {
            subject_entity,
            si_tolerance: si_value,
        });
    }
    result
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
pub(crate) fn propagate_subject_to_descendants(
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
pub(crate) fn merge_with_min<I: IntoIterator<Item = (String, f64)>>(
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
    use reify_core::{ContentHash, DimensionVector, Type, ValueCellId};
    use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, PersistentMap, ResolvedFunction, Value};
    use reify_test_support::builders::CompiledPurposeBuilder;
    use std::collections::HashMap;

    /// Build a one-cell `PersistentMap` entry shaped like the existing
    /// `engine_purposes.rs` unit-test fixtures: a `Param` cell typed
    /// `Type::dimensionless_scalar()`, with a content_hash derived from the member name.
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
                cell_type: Type::dimensionless_scalar(),
                default_expr: None,
                content_hash: ContentHash::of_str(&format!("{}.{}", entity, member)),
            },
        );
    }

    /// Build the canonical `RepresentationWithin(<ValueRef typed
    /// StructureRef>, <Literal Scalar(LENGTH)>)` shape that
    /// `extract_tolerance_bindings` is expected to recognise.
    ///
    /// `subject_param_name` is used as the `ValueCellId.entity` of the
    /// subject `ValueRef`, so the produced fixture binds to the purpose's
    /// actual parameter (the matcher rejects `ValueRef`s whose entity does
    /// not appear in `purpose.params`). Tests that want to exercise the
    /// rejection branch can pass a non-param name here directly.
    fn representation_within_constraint(
        subject_param_name: &str,
        subject_kind: &str,
        si_value: f64,
        dimension: DimensionVector,
    ) -> CompiledExpr {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new(subject_param_name, "self"),
            Type::StructureRef(subject_kind.to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value,
                dimension,
            },
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
        let constraint_expr =
            representation_within_constraint("subject", "Bracket", 1e-6, DimensionVector::LENGTH);

        let purpose = CompiledPurposeBuilder::new("manufacturing")
            .param("subject", "Structure")
            .constraint("subject", 0, None, constraint_expr)
            .build();

        let bindings = extract_tolerance_bindings(
            &purpose,
            &[("subject".to_string(), "MyDesign".to_string())],
        );

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
            CompiledExpr::value_ref(ValueCellId::new("subject", "thickness"), Type::dimensionless_scalar()),
            CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar()),
            Type::Bool,
        );

        // (c) A valid RepresentationWithin.
        let rep_within =
            representation_within_constraint("subject", "Bracket", 5e-6, DimensionVector::LENGTH);

        let purpose = CompiledPurposeBuilder::new("manufacturing")
            .param("subject", "Structure")
            .constraint("subject", 0, None, all_params_determined)
            .constraint("subject", 1, None, binop_constraint)
            .constraint("subject", 2, None, rep_within)
            .build();

        let bindings = extract_tolerance_bindings(
            &purpose,
            &[("subject".to_string(), "MyDesign".to_string())],
        );

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
            "subject",
            "Bracket",
            1.0,
            DimensionVector::DIMENSIONLESS,
        );

        let purpose = CompiledPurposeBuilder::new("manufacturing")
            .param("subject", "Structure")
            .constraint("subject", 0, None, constraint_expr)
            .build();

        let bindings = extract_tolerance_bindings(
            &purpose,
            &[("subject".to_string(), "MyDesign".to_string())],
        );

        assert!(
            bindings.is_empty(),
            "non-LENGTH dimension on tolerance arg must not yield a binding"
        );
    }

    #[test]
    fn extract_tolerance_bindings_rejects_value_ref_to_non_purpose_param() {
        // Reviewer issue #1 (amend): the matcher's contract is
        // `RepresentationWithin(<bare-purpose-param>, <length-literal>)` —
        // a ValueRef whose entity is not one of the purpose's params must
        // be rejected, even if its result_type is StructureRef(_) (which
        // would otherwise pass the type-tag gate). Without this gate, a
        // hypothetical `RepresentationWithin(<unrelated StructureRef>, tol)`
        // would silently bind a tolerance to `bound_entity_ref` even though
        // the constraint's subject is not the bound parameter.
        let constraint_expr = representation_within_constraint(
            "not_a_param", // entity name that does NOT appear in purpose.params
            "Bracket",
            1e-6,
            DimensionVector::LENGTH,
        );

        let purpose = CompiledPurposeBuilder::new("manufacturing")
            .param("subject", "Structure")
            .constraint("subject", 0, None, constraint_expr)
            .build();

        let bindings = extract_tolerance_bindings(
            &purpose,
            &[("subject".to_string(), "MyDesign".to_string())],
        );

        assert!(
            bindings.is_empty(),
            "ValueRef whose entity is not a purpose param must be rejected \
             even if it is typed StructureRef(_)"
        );
    }

    #[test]
    fn extract_tolerance_bindings_rejects_non_finite_tolerance_literals() {
        // Reviewer issue #2 (amend): NaN / ±Inf tolerances have no semantics
        // and would propagate into the scope. Worse, NaN comparisons always
        // evaluate false, so `merge_with_min` could never displace a stale
        // NaN with a real finite value. Reject at extraction time.
        for bad_value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let constraint_expr = representation_within_constraint(
                "subject",
                "Bracket",
                bad_value,
                DimensionVector::LENGTH,
            );

            let purpose = CompiledPurposeBuilder::new("manufacturing")
                .param("subject", "Structure")
                .constraint("subject", 0, None, constraint_expr)
                .build();

            let bindings = extract_tolerance_bindings(
                &purpose,
                &[("subject".to_string(), "MyDesign".to_string())],
            );

            assert!(
                bindings.is_empty(),
                "non-finite tolerance literal {:?} must be rejected",
                bad_value
            );
        }
    }

    /// Negative-finite tolerance literals (e.g. -1e-3, -1e-6, -1.0) must be
    /// rejected by `extract_tolerance_bindings` and produce no `ToleranceBinding`.
    ///
    /// Why: (a) A tolerance is a magnitude — a negative value has no semantics.
    /// (b) Allowing a negative literal to survive extraction is a runtime hazard:
    /// it wins `merge_with_min` against any positive contributor (because
    /// `negative < positive` is unconditionally true), propagates to
    /// `combine_demanded_tolerance`, and panics that function's debug-assert
    /// (`is_finite() && >= 0.0`) in debug builds — or silently wins an
    /// `o.min(p)` race in release, corrupting the demanded tolerance for the
    /// entire output. (c) The `>= 0.0` half of the gate restores the symmetry
    /// the `tolerance_combine.rs` "Recognition-shape twin" docstring claims
    /// with `extract_output_tolerance_bound`: both extractors must apply the
    /// identical `is_finite() && >= 0.0` guard so the combiner's invariant is
    /// upheld upstream rather than panicked at the boundary.
    #[test]
    fn extract_tolerance_bindings_rejects_negative_finite_tolerance_literals() {
        for bad_value in [-1e-6_f64, -1e-3, -1.0] {
            let constraint_expr = representation_within_constraint(
                "subject",
                "Bracket",
                bad_value,
                DimensionVector::LENGTH,
            );

            let purpose = CompiledPurposeBuilder::new("manufacturing")
                .param("subject", "Structure")
                .constraint("subject", 0, None, constraint_expr)
                .build();

            let bindings = extract_tolerance_bindings(
                &purpose,
                &[("subject".to_string(), "MyDesign".to_string())],
            );

            assert!(
                bindings.is_empty(),
                "negative finite tolerance literal {:?} must be rejected",
                bad_value
            );
        }
    }

    /// `si_value == 0.0` is the exact lower boundary accepted by the
    /// `si_value >= 0.0` gate. A zero-tolerance `RepresentationWithin` is
    /// semantically valid (it means "exact representation" — a degenerate but
    /// permissible tolerance). This test pins that 0.0 produces exactly one
    /// binding with `si_tolerance == 0.0`, which also locks the boundary
    /// contract in lockstep with `extract_output_tolerance_bound` (which
    /// accepts 0.0 under the same `>= 0.0` gate).
    #[test]
    fn extract_tolerance_bindings_accepts_zero_tolerance_literal() {
        let constraint_expr =
            representation_within_constraint("subject", "Bracket", 0.0, DimensionVector::LENGTH);

        let purpose = CompiledPurposeBuilder::new("manufacturing")
            .param("subject", "Structure")
            .constraint("subject", 0, None, constraint_expr)
            .build();

        let bindings = extract_tolerance_bindings(
            &purpose,
            &[("subject".to_string(), "MyDesign".to_string())],
        );

        assert_eq!(
            bindings.len(),
            1,
            "zero tolerance literal must be accepted (lower boundary of >= 0.0 gate)"
        );
        assert_eq!(
            bindings[0].si_tolerance, 0.0,
            "binding si_tolerance must preserve the zero value exactly"
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

    /// A 2-param purpose with two `RepresentationWithin` constraints — one per
    /// param — must yield two `ToleranceBinding`s, each routed to its OWN bound
    /// entity rather than collapsed onto a single ref.
    ///
    /// This is the unit-level pin for the per-param identity threading introduced
    /// in step-2 of task 4070: `extract_tolerance_bindings` now resolves each
    /// matched constraint's subject param against the `bindings` slice so
    /// multi-param purposes produce per-param `ToleranceBinding`s.
    /// Regression lock: `extract_tolerance_bindings` (scope side) and
    /// `recognize_representation_within` (combine side) must agree on every
    /// `CompiledExpr` shape in the shared fixture set.
    ///
    /// Each fixture's subject `ValueRef` entity is declared as a purpose param
    /// ("subject") with a matching binding, so the scope-only membership and
    /// binding gates always pass — the shared shape gate is the sole decider.
    ///
    /// Before the routing fix the resolved-`FunctionCall` fixture drives RED:
    /// the combine side recognises it (`Some`) but the still-hand-rolled scope
    /// side does not (empty `Vec`).
    #[test]
    fn extract_tolerance_bindings_agrees_with_recognize_representation_within_on_fixture_set() {
        let subject_vref = || {
            CompiledExpr::value_ref(
                ValueCellId::new("subject", "self"),
                Type::StructureRef("Bracket".to_string()),
            )
        };
        let len_tol = |si: f64| {
            CompiledExpr::literal(
                Value::Scalar { si_value: si, dimension: DimensionVector::LENGTH },
                Type::Scalar { dimension: DimensionVector::LENGTH },
            )
        };

        // Resolved FunctionCall variant (RED driver before routing fix) —
        // mirrors reify-ir's `make_function_call` helper (expr.rs:1827).
        let f_resolved_fc = CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: "RepresentationWithin".to_string(),
                    qualified_name: "std::RepresentationWithin".to_string(),
                },
                args: vec![subject_vref(), len_tol(1e-6)],
            },
            result_type: Type::Bool,
            content_hash: ContentHash::of("RepresentationWithin".as_bytes()),
        };
        let f_wrong_name_fc = CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: "ToleranceWithin".to_string(),
                    qualified_name: "std::ToleranceWithin".to_string(),
                },
                args: vec![subject_vref(), len_tol(1e-6)],
            },
            result_type: Type::Bool,
            content_hash: ContentHash::of("ToleranceWithin".as_bytes()),
        };

        // Each case: (label, expr, expected_si — None means no-match expected).
        let cases: Vec<(&str, CompiledExpr, Option<f64>)> = vec![
            // Gate-2 variant coverage.
            (
                "canonical UFC",
                representation_within_constraint("subject", "Bracket", 1e-6, DimensionVector::LENGTH),
                Some(1e-6),
            ),
            (
                "resolved FunctionCall",
                f_resolved_fc,
                Some(1e-6),
            ),
            (
                "wrong name UFC",
                CompiledExpr::user_function_call(
                    "ToleranceWithin".to_string(),
                    vec![subject_vref(), len_tol(1e-6)],
                    Type::Bool,
                ),
                None,
            ),
            ("wrong name resolved FC", f_wrong_name_fc, None),
            // Arity gate.
            (
                "arity-1 UFC",
                CompiledExpr::user_function_call(
                    "RepresentationWithin".to_string(),
                    vec![subject_vref()],
                    Type::Bool,
                ),
                None,
            ),
            // Gate-3: arg0 shape.
            (
                "non-ValueRef arg0",
                CompiledExpr::user_function_call(
                    "RepresentationWithin".to_string(),
                    vec![
                        CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar()),
                        len_tol(1e-6),
                    ],
                    Type::Bool,
                ),
                None,
            ),
            (
                "non-StructureRef arg0",
                CompiledExpr::user_function_call(
                    "RepresentationWithin".to_string(),
                    vec![
                        CompiledExpr::value_ref(
                            ValueCellId::new("subject", "self"),
                            Type::dimensionless_scalar(),
                        ),
                        len_tol(1e-6),
                    ],
                    Type::Bool,
                ),
                None,
            ),
            // Gate-4a: dimension.
            (
                "DIMENSIONLESS tol",
                representation_within_constraint("subject", "Bracket", 1.0, DimensionVector::DIMENSIONLESS),
                None,
            ),
            // Gate-4b/c: is_valid_tolerance_si.
            (
                "NaN tol",
                representation_within_constraint("subject", "Bracket", f64::NAN, DimensionVector::LENGTH),
                None,
            ),
            (
                "+Inf tol",
                representation_within_constraint("subject", "Bracket", f64::INFINITY, DimensionVector::LENGTH),
                None,
            ),
            (
                "negative tol",
                representation_within_constraint("subject", "Bracket", -1e-6, DimensionVector::LENGTH),
                None,
            ),
            // Zero is the exact >= 0.0 lower boundary (accepted).
            (
                "zero tol",
                representation_within_constraint("subject", "Bracket", 0.0, DimensionVector::LENGTH),
                Some(0.0),
            ),
            // Non-call outer expr.
            (
                "bare Real-literal",
                CompiledExpr::literal(Value::Real(42.0), Type::dimensionless_scalar()),
                None,
            ),
        ];

        for (label, expr, expected_si) in &cases {
            let purpose = CompiledPurposeBuilder::new("p")
                .param("subject", "Structure")
                .constraint("subject", 0, None, expr.clone())
                .build();
            let scope_bindings = extract_tolerance_bindings(
                &purpose,
                &[("subject".to_string(), "Inst".to_string())],
            );
            let combine_result =
                crate::tolerance_combine::recognize_representation_within(expr);

            let scope_matches = !scope_bindings.is_empty();
            let combine_matches = combine_result.is_some();
            let should_match = expected_si.is_some();

            assert_eq!(
                scope_matches,
                combine_matches,
                "fixture '{label}': scope ({scope_matches}) and combine ({combine_matches}) disagree"
            );
            assert_eq!(
                scope_matches,
                should_match,
                "fixture '{label}': expected {} but got {}",
                if should_match { "match" } else { "no-match" },
                if scope_matches { "match" } else { "no-match" },
            );

            if let Some(expected_tol) = expected_si {
                assert_eq!(
                    scope_bindings.len(),
                    1,
                    "fixture '{label}': expected exactly one binding"
                );
                assert_eq!(
                    scope_bindings[0].subject_entity, "Inst",
                    "fixture '{label}': subject_entity must be 'Inst'"
                );
                assert_eq!(
                    scope_bindings[0].si_tolerance, *expected_tol,
                    "fixture '{label}': si_tolerance must equal {expected_tol}"
                );
            }
        }
    }

    #[test]
    fn extract_tolerance_bindings_threads_each_param_to_its_bound_entity() {
        let constraint_part =
            representation_within_constraint("part", "Bracket", 1e-6, DimensionVector::LENGTH);
        let constraint_env =
            representation_within_constraint("envelope", "Envelope", 5e-6, DimensionVector::LENGTH);

        let purpose = CompiledPurposeBuilder::new("fits")
            .param("part", "Structure")
            .param("envelope", "Structure")
            .constraint("part", 0, None, constraint_part)
            .constraint("envelope", 1, None, constraint_env)
            .build();

        let bindings = vec![
            ("part".to_string(), "PartInst".to_string()),
            ("envelope".to_string(), "EnvInst".to_string()),
        ];
        let result = extract_tolerance_bindings(&purpose, &bindings);

        assert_eq!(result.len(), 2, "must yield exactly two ToleranceBindings");
        let part_binding = result
            .iter()
            .find(|b| b.subject_entity == "PartInst")
            .expect("must have a binding for PartInst (part param)");
        assert_eq!(
            part_binding.si_tolerance, 1e-6,
            "part param must carry its own tolerance (1e-6)"
        );
        let env_binding = result
            .iter()
            .find(|b| b.subject_entity == "EnvInst")
            .expect("must have a binding for EnvInst (envelope param)");
        assert_eq!(
            env_binding.si_tolerance, 5e-6,
            "envelope param must carry its own tolerance (5e-6)"
        );
    }
}
