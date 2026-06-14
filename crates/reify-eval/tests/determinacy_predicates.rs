//! Determinacy predicate tests.
//!
//! Exercises the determinacy predicate evaluator (task 201):
//! determined(), undetermined(), constrained(), partially_determined().
//!
//! Tests use the parse -> compile -> eval -> verify pipeline for most cases.
//! Programmatic construction via TopologyTemplateBuilder is used for states
//! unreachable through .ri source (e.g., Provisional).

use std::borrow::Cow;

use reify_core::{ContentHash, Type, ValueCellId};
use reify_eval::Engine;
use reify_ir::{
    CompiledExpr, CompiledExprKind, DeterminacyPredicateKind, Satisfaction,
    TAG_DETERMINACY_PREDICATE, Value,
};
use reify_test_support::{make_engine, parse_and_compile};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m9_determinacy.ri"
);

// -- Helper -------------------------------------------------------------------

/// Parse, compile, eval, and return the result values map.
/// Thin wrapper over reify_test_support::eval_source — returns ValueMap instead of EvalResult.
fn eval_source(source: &str) -> reify_ir::ValueMap {
    reify_test_support::eval_source(source).values
}

// == determined() predicate tests =============================================

/// determined(a) should return true when a is a param with a default value.
/// A param with a concrete default is DeterminacyState::Determined after eval.
#[test]
fn determined_true_for_param_with_default() {
    let values = eval_source(
        r#"
        structure S {
            param a : Length = 10mm
            let r = determined(a)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(true),
        "determined(a) should be true for param with default"
    );
}

/// determined(b) should return false when b is an auto param (DeterminacyState::Auto).
#[test]
fn determined_false_for_auto_param() {
    let values = eval_source(
        r#"
        structure S {
            param b : Length = auto
            let r = determined(b)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(false),
        "determined(b) should be false for auto param"
    );
}

/// determined(c) should return false when c is a param without a default
/// (DeterminacyState::Undetermined).
#[test]
fn determined_false_for_undetermined_param() {
    let values = eval_source(
        r#"
        structure S {
            param c : Real
            let r = determined(c)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(false),
        "determined(c) should be false for undetermined param"
    );
}

/// determined() should return false for a cell in Provisional state.
/// Uses programmatic PersistentMap construction since Provisional is only an
/// intermediate solver state, not reachable through the .ri eval pipeline.
#[test]
fn determined_false_for_provisional() {
    use reify_ir::PersistentMap;

    let cell_id = ValueCellId::new("S", "a");

    // Build a DeterminacyPredicate(Determined, cell_a) expression.
    let det_expr = CompiledExpr {
        kind: CompiledExprKind::DeterminacyPredicate {
            kind: DeterminacyPredicateKind::Determined,
            cell: cell_id.clone(),
        },
        result_type: Type::Bool,
        content_hash: ContentHash::of(&[99]),
    };

    // Inject Provisional state directly into a PersistentMap determinacy snapshot.
    let mut det_map: PersistentMap<ValueCellId, (Value, reify_ir::DeterminacyState)> =
        PersistentMap::new();
    det_map.insert(
        cell_id.clone(),
        (Value::Real(2.5), reify_ir::DeterminacyState::Provisional),
    );

    // Build an EvalContext with the determinacy map and no values/functions.
    let values = reify_ir::ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &[]).with_determinacy(&det_map);

    // Evaluate directly — Provisional ≠ Determined, so result should be false.
    let result = reify_expr::eval_expr(&det_expr, &ctx);
    assert_eq!(
        result,
        Value::Bool(false),
        "determined(a) should be false for Provisional state"
    );
}

// == undetermined() predicate tests ==========================================

/// undetermined(a) should return false when a is Determined (has a default).
#[test]
fn undetermined_false_for_determined_param() {
    let values = eval_source(
        r#"
        structure S {
            param a : Length = 10mm
            let r = undetermined(a)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(false),
        "undetermined(a) should be false for param with default"
    );
}

/// undetermined(c) should return true when c has no default and no constraints.
/// A param without default has DeterminacyState::Undetermined.
#[test]
fn undetermined_true_for_undetermined_no_constraints() {
    let values = eval_source(
        r#"
        structure S {
            param c : Real
            let r = undetermined(c)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(true),
        "undetermined(c) should be true for param without default"
    );
}

/// undetermined(b) should return false when b is an auto param (DeterminacyState::Auto).
#[test]
fn undetermined_false_for_auto_param() {
    let values = eval_source(
        r#"
        structure S {
            param b : Length = auto
            let r = undetermined(b)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(false),
        "undetermined(b) should be false for auto param"
    );
}

/// undetermined(c) for a param without default but WITH a constraint.
/// The undetermined() predicate checks DeterminacyState only — constraint
/// presence is NOT considered. A param without default has Undetermined state
/// regardless of constraints, so undetermined(c) returns true even with
/// constraint c > 0.
#[test]
fn undetermined_true_for_undetermined_with_constraints() {
    let values = eval_source(
        r#"
        structure S {
            param c : Real
            constraint c > 0
            let r = undetermined(c)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    // undetermined() only checks DeterminacyState, not constraint presence.
    // A param without default is Undetermined regardless of constraints.
    assert_eq!(
        *r_val,
        Value::Bool(true),
        "undetermined(c) should be true — state is Undetermined despite constraint"
    );
}

// == constrained() predicate tests ===========================================

/// constrained(a) should return false for a determined param (even with constraint).
/// constrained() checks state == Auto || Provisional, not constraint presence.
/// A param with default is Determined, so constrained() returns false.
#[test]
fn constrained_false_for_determined_param_with_constraint() {
    let values = eval_source(
        r#"
        structure S {
            param a : Length = 10mm
            constraint a > 0mm
            let r = constrained(a)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(false),
        "constrained(a) should be false — state is Determined, not Auto/Provisional"
    );
}

/// constrained(b) should return false for a determined param without constraints.
#[test]
fn constrained_false_for_param_without_constraint() {
    let values = eval_source(
        r#"
        structure S {
            param b : Length = 20mm
            let r = constrained(b)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(false),
        "constrained(b) should be false for Determined param"
    );
}

/// constrained(x) should return true for an auto param (DeterminacyState::Auto).
/// constrained() checks state == Auto || Provisional (state-based, not constraint-presence).
#[test]
fn constrained_true_for_auto_param() {
    let values = eval_source(
        r#"
        structure S {
            param x : Length = auto
            let r = constrained(x)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(true),
        "constrained(x) should be true — state is Auto (state-based semantics)"
    );
}

/// constrained(c) should return false for an undetermined param without constraints.
/// State is Undetermined, which is not Auto or Provisional.
#[test]
fn constrained_false_for_undetermined_without_constraint() {
    let values = eval_source(
        r#"
        structure S {
            param c : Real
            let r = constrained(c)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(false),
        "constrained(c) should be false — state is Undetermined"
    );
}

// == partially_determined() predicate tests ==================================

/// partially_determined(a) should return false for a determined param with constraint.
/// partially_determined() checks state == Provisional. Determined ≠ Provisional.
#[test]
fn partially_determined_false_for_determined() {
    let values = eval_source(
        r#"
        structure S {
            param a : Length = 10mm
            constraint a > 0mm
            let r = partially_determined(a)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(false),
        "partially_determined(a) should be false for Determined state"
    );
}

/// partially_determined(x) should return false for an auto param with constraint.
/// partially_determined() checks state == Provisional. Auto ≠ Provisional.
///
/// NOTE: The plan spec originally defined partially_determined as
/// "has constraints AND state != Determined". The implementation was
/// intentionally narrowed to Provisional-only because Auto params already
/// have their own predicate (constrained()), and partially_determined is
/// reserved for the solver's intermediate state (Provisional) where
/// constraints have been partially resolved. This divergence is documented
/// here and in reify-expr/src/lib.rs:279.
#[test]
fn partially_determined_false_for_auto_with_constraint() {
    let values = eval_source(
        r#"
        structure S {
            param x : Length = auto
            constraint x > 0
            let r = partially_determined(x)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(false),
        "partially_determined(x) should be false — state is Auto, not Provisional"
    );
}

/// partially_determined(a) should return false for a determined param without constraint.
#[test]
fn partially_determined_false_for_determined_no_constraints() {
    let values = eval_source(
        r#"
        structure S {
            param a : Length = 10mm
            let r = partially_determined(a)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(false),
        "partially_determined(a) should be false for Determined state"
    );
}

/// partially_determined(c) should return false for an undetermined param without constraints.
#[test]
fn partially_determined_false_for_undetermined_no_constraints() {
    let values = eval_source(
        r#"
        structure S {
            param c : Real
            let r = partially_determined(c)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(false),
        "partially_determined(c) should be false — state is Undetermined, not Provisional"
    );
}

/// partially_determined() should return true for a cell in Provisional state.
/// Uses the same programmatic PersistentMap construction as determined_false_for_provisional
/// since Provisional is only an intermediate solver state, not reachable through .ri eval.
///
/// This is the ONLY true-returning code path for partially_determined() — all other
/// tests assert Bool(false). Without this test, the Provisional branch at
/// reify-expr/src/lib.rs:286 would be completely untested.
#[test]
fn partially_determined_true_for_provisional() {
    use reify_ir::PersistentMap;

    let cell_id = ValueCellId::new("S", "a");

    // Build a DeterminacyPredicate(PartiallyDetermined, cell_a) expression.
    let det_expr = CompiledExpr {
        kind: CompiledExprKind::DeterminacyPredicate {
            kind: DeterminacyPredicateKind::PartiallyDetermined,
            cell: cell_id.clone(),
        },
        result_type: Type::Bool,
        content_hash: ContentHash::of(&[99]),
    };

    // Inject Provisional state directly into a PersistentMap determinacy snapshot.
    let mut det_map: PersistentMap<ValueCellId, (Value, reify_ir::DeterminacyState)> =
        PersistentMap::new();
    det_map.insert(
        cell_id.clone(),
        (Value::Real(2.5), reify_ir::DeterminacyState::Provisional),
    );

    // Build an EvalContext with the determinacy map and no values/functions.
    let values = reify_ir::ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &[]).with_determinacy(&det_map);

    // Evaluate directly — Provisional == Provisional, so result should be true.
    let result = reify_expr::eval_expr(&det_expr, &ctx);
    assert_eq!(
        result,
        Value::Bool(true),
        "partially_determined(a) should be true for Provisional state"
    );
}

// == Composition tests =======================================================

/// All determined: determined(a) and determined(b) and determined(c) should be true
/// when all params have defaults.
#[test]
fn forall_determined_all_true() {
    let values = eval_source(
        r#"
        structure S {
            param a : Length = 10mm
            param b : Length = 20mm
            param c : Length = 30mm
            let r = determined(a) && determined(b) && determined(c)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(true),
        "all three determined() should be true → and composition is true"
    );
}

/// Mixed: determined(a) and determined(b) should be false when b is Auto.
/// Short-circuit: determined(a)=true, determined(b)=false → result is false.
#[test]
fn forall_determined_mixed_false() {
    let values = eval_source(
        r#"
        structure S {
            param a : Length = 10mm
            param b : Length = auto
            let r = determined(a) && determined(b)
        }
    "#,
    );
    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    assert_eq!(
        *r_val,
        Value::Bool(false),
        "determined(b) is false for Auto → and composition is false"
    );
}

// == Where-guard tests =======================================================

/// where determined(a) { let x = a * 2 } should activate when a is Determined.
/// Since a=10mm has a default, determined(a)=true → guard is active → x is evaluated.
#[test]
fn where_guard_determined_activates() {
    let values = eval_source(
        r#"
        structure S {
            param a : Length = 10
            where determined(a) {
                let x = a * 2
            }
        }
    "#,
    );
    let x_id = ValueCellId::new("S", "x");
    let x_val = values.get(&x_id).expect("x should exist in values map");
    // Guard is true → x should be evaluated to a*2 = 20
    assert_ne!(
        *x_val,
        Value::Undef,
        "x should not be Undef — guard determined(a) is true"
    );
}

/// where determined(b) { let y = 5 } else { let z = 10 } when b is Auto.
/// determined(b)=false → y is Undef (guard inactive), z is evaluated (else active).
#[test]
fn where_guard_determined_deactivates() {
    let values = eval_source(
        r#"
        structure S {
            param b : Length = auto
            where determined(b) {
                let y = 5
            } else {
                let z = 10
            }
        }
    "#,
    );
    let y_id = ValueCellId::new("S", "y");
    let z_id = ValueCellId::new("S", "z");
    let y_val = values.get(&y_id).expect("y should exist in values map");
    let z_val = values.get(&z_id).expect("z should exist in values map");
    // Guard is false → y is Undef, z is evaluated
    assert_eq!(
        *y_val,
        Value::Undef,
        "y should be Undef — guard determined(b) is false"
    );
    assert_ne!(
        *z_val,
        Value::Undef,
        "z should not be Undef — else block is active"
    );
}

// == Constraint-expression test ==============================================

/// constraint determined(a) should be Satisfied when a is Determined.
#[test]
fn determinacy_in_constraint() {
    let source = r#"
        structure S {
            param a : Length = 10mm
            constraint determined(a)
        }
    "#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    let _eval = engine.eval(&compiled);
    let check = engine.check(&compiled);

    assert!(
        !check.constraint_results.is_empty(),
        "expected at least one constraint result"
    );
    for entry in &check.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint determined(a) should be Satisfied, got {:?} for {}",
            entry.satisfaction,
            entry.id
        );
    }
}

// == Integration tests for m9_determinacy.ri ==================================

/// The m9_determinacy.ri example file should parse and compile without errors.
#[test]
fn determinacy_ri_parses_and_compiles() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_determinacy.ri should exist");

    let compiled = parse_and_compile(&source);

    // Should have at least 1 template (DeterminacyDemo structure).
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template from m9_determinacy.ri"
    );
}

/// All constraints in m9_determinacy.ri should be Satisfied, with at least 8 results.
#[test]
fn determinacy_ri_all_constraints_satisfied() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_determinacy.ri should exist");

    let compiled = parse_and_compile(&source);

    let mut engine = make_engine();
    let _eval = engine.eval(&compiled);

    let check = engine.check(&compiled);
    assert!(
        check.constraint_results.len() >= 8,
        "expected >=8 constraint results from m9_determinacy.ri, got {}",
        check.constraint_results.len()
    );
    for entry in &check.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

/// Full production path for m9_determinacy.ri: parse → compile → eval →
/// SimpleConstraintChecker with determinacy context → all constraints Satisfied.
///
/// Unlike determinacy_ri_all_constraints_satisfied (which uses MockConstraintChecker
/// and thus doesn't actually evaluate constraint expressions), this test uses the
/// real SimpleConstraintChecker to verify determinacy predicates are correctly
/// evaluated within constraints via the ConstraintInput.determinacy context.
#[test]
fn determinacy_ri_constraints_satisfied_simple_checker() {
    use reify_constraints::SimpleConstraintChecker;

    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_determinacy.ri should exist");

    let compiled = parse_and_compile(&source);

    // Use SimpleConstraintChecker — the real production checker.
    let checker = SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), None);

    // check() internally calls eval() then checks constraints with determinacy context.
    let check = engine.check(&compiled);
    assert!(
        check.constraint_results.len() >= 8,
        "expected >=8 constraint results from m9_determinacy.ri, got {}",
        check.constraint_results.len()
    );
    for entry in &check.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied with SimpleConstraintChecker, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

// == Hash collision regression test ==========================================

/// DeterminacyPredicate and Lambda expressions must use different hash
/// discriminator bytes so their content hashes are structurally distinct.
/// Regression: both originally used discriminator byte [7], enabling contrived
/// collisions where a Lambda's body hash matches the kind string hash and
/// the Lambda's param name matches the cell_id format string.
#[test]
fn determinacy_predicate_hash_differs_from_lambda() {
    let cell_id = ValueCellId::new("S", "a");
    let kind = DeterminacyPredicateKind::Determined;

    // Use the real factory method so the hash always matches the compiler.
    let det_expr = CompiledExpr::determinacy_predicate(kind, cell_id.clone());

    // Build a Lambda whose hash combine sequence matches the above when
    // using the same discriminator byte. Lambda hash formula:
    //   ContentHash::of(&[discriminator])
    //     .combine(body.content_hash)
    //     .combine(of_str(param_name))  // for each param
    //     .combine(of_str(param_id))    // for each param_id
    // By setting body.content_hash = of_str("Determined") and param = "S.a",
    // the combine sequence is identical iff discriminators match.
    let body = CompiledExpr {
        kind: CompiledExprKind::Literal(Value::Bool(false)),
        result_type: Type::Bool,
        content_hash: ContentHash::of_str(&format!("{:?}", kind)), // matches "Determined"
    };
    let lambda_expr = CompiledExpr::lambda(
        vec![(format!("{}", cell_id), None)], // param name = "S.a"
        vec![],                               // no param_ids
        body,
        vec![], // no captures
        Type::Bool,
    );

    // With the same discriminator byte, these hashes are identical — a collision.
    // After fixing the discriminator, they must differ.
    assert_ne!(
        det_expr.content_hash, lambda_expr.content_hash,
        "DeterminacyPredicate and Lambda with same sub-structure should have different hashes"
    );
}

/// DeterminacyPredicate must not collide with OptionNone (both previously
/// used discriminator byte [15]). Regression test for content_hash_collision.
#[test]
fn determinacy_predicate_hash_differs_from_option_none() {
    let cell_id = ValueCellId::new("S", "a");
    let kind = DeterminacyPredicateKind::Determined;

    let det_expr = CompiledExpr::determinacy_predicate(kind, cell_id.clone());

    // OptionNone uses discriminator byte [15].
    let option_none_expr = CompiledExpr::option_none(Type::Option(Box::new(Type::Bool)));

    assert_ne!(
        det_expr.content_hash, option_none_expr.content_hash,
        "DeterminacyPredicate and OptionNone must have different content hashes"
    );
}

// == Constructor + stable hash regression test ===============================

/// The `CompiledExpr::determinacy_predicate()` constructor must produce a stable
/// content hash based on byte discriminators (not Debug repr). This mirrors the
/// pattern used by `CompiledExpr::quantifier()` and other constructors.
///
/// Hash formula: `ContentHash::of(&[17, kind_byte]).combine(of_str(cell_id))`
/// where kind_byte is: Determined=0, Undetermined=1, Constrained=2, PartiallyDetermined=3.
#[test]
fn determinacy_predicate_constructor_produces_stable_hash() {
    let cell_id = ValueCellId::new("S", "a");

    // Test all 4 variants produce correct kind, result_type, and stable hash.
    let variants: Vec<(DeterminacyPredicateKind, u8)> = vec![
        (DeterminacyPredicateKind::Determined, 0),
        (DeterminacyPredicateKind::Undetermined, 1),
        (DeterminacyPredicateKind::Constrained, 2),
        (DeterminacyPredicateKind::PartiallyDetermined, 3),
    ];

    let mut hashes = Vec::new();

    for (kind, kind_byte) in &variants {
        let expr = CompiledExpr::determinacy_predicate(*kind, cell_id.clone());

        // Verify kind is correct.
        if let CompiledExprKind::DeterminacyPredicate {
            kind: ref k,
            cell: ref c,
        } = expr.kind
        {
            assert_eq!(k, kind, "constructor should produce correct kind");
            assert_eq!(c, &cell_id, "constructor should produce correct cell");
        } else {
            panic!("expected DeterminacyPredicate, got {:?}", expr.kind);
        }

        // Verify result_type is Bool.
        assert_eq!(
            expr.result_type,
            Type::Bool,
            "determinacy predicate result_type should be Bool"
        );

        // Verify content_hash matches the stable byte-discriminator formula.
        let expected_hash = ContentHash::of(&[TAG_DETERMINACY_PREDICATE, *kind_byte])
            .combine(ContentHash::of_str(&format!("{}", cell_id)));
        assert_eq!(
            expr.content_hash, expected_hash,
            "content_hash for {:?} should use stable byte encoding [17, {}]",
            kind, kind_byte
        );

        hashes.push(expr.content_hash);
    }

    // All 4 variants must produce distinct hashes.
    for i in 0..hashes.len() {
        for j in (i + 1)..hashes.len() {
            assert_ne!(
                hashes[i], hashes[j],
                "hash for variant {} must differ from variant {}",
                i, j
            );
        }
    }
}

// == Silent failure regression test ==========================================

/// A DeterminacyPredicate referencing a cell NOT present in the determinacy
/// snapshot should panic in debug builds (via debug_assert!) to make wiring
/// bugs noisy at the point of detection, rather than propagating Undef downstream.
///
/// In release builds, the code path returns Value::Undef for graceful degradation.
///
/// This follows the project convention that logic errors should be noisy
/// (see feedback_silent_defaults_pattern.md).
#[test]
#[cfg_attr(
    debug_assertions,
    should_panic(expected = "wiring bug or eval-order violation")
)]
fn determinacy_predicate_missing_cell_panics_in_debug() {
    use reify_ir::PersistentMap;

    let missing_cell = ValueCellId::new("S", "nonexistent");

    // Build a DeterminacyPredicate(Determined, missing_cell).
    let det_expr = CompiledExpr {
        kind: CompiledExprKind::DeterminacyPredicate {
            kind: DeterminacyPredicateKind::Determined,
            cell: missing_cell.clone(),
        },
        result_type: Type::Bool,
        content_hash: ContentHash::of(&[99]),
    };

    // Determinacy map does NOT contain missing_cell.
    let det_map: PersistentMap<ValueCellId, (Value, reify_ir::DeterminacyState)> =
        PersistentMap::new();

    let values = reify_ir::ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &[]).with_determinacy(&det_map);

    let result = reify_expr::eval_expr(&det_expr, &ctx);

    // In release mode only: Missing cell should return Undef.
    assert_eq!(
        result,
        Value::Undef,
        "missing cell in determinacy snapshot should return Undef in release mode"
    );
}

// == SimpleConstraintChecker integration test ================================

/// SimpleConstraintChecker should evaluate determinacy predicates in constraints
/// when determinacy context is provided via ConstraintInput.
///
/// This test builds a structure with `param a : Length = 10mm` and
/// `constraint determined(a)`, evaluates to get the snapshot, then manually
/// constructs a ConstraintInput with the determinacy field and checks via
/// SimpleConstraintChecker.
#[test]
fn simple_constraint_checker_evaluates_determinacy_predicate() {
    use reify_constraints::SimpleConstraintChecker;
    use reify_ir::{ConstraintChecker, ConstraintInput};

    let source = r#"
        structure S {
            param a : Length = 10mm
            constraint determined(a)
        }
    "#;
    let compiled = parse_and_compile(source);

    // Eval to get the snapshot values (which include determinacy states).
    let mut engine = make_engine();
    let eval_result = engine.eval(&compiled);

    // Get constraint expression from the compiled template.
    let template = &compiled.templates[0];
    let constraint_pairs: Vec<_> = template
        .constraints
        .iter()
        .map(|c| (c.id.clone(), &c.expr))
        .collect();

    assert!(
        !constraint_pairs.is_empty(),
        "expected at least one constraint"
    );

    // Use the engine's actual snapshot to get the determinacy map, which
    // correctly reflects all states (Determined, Undetermined, Auto, Provisional)
    // rather than reconstructing from values (which would misclassify Auto as Undetermined).
    let snapshot = engine
        .snapshot()
        .expect("engine should have a snapshot after eval");
    let det_map = &snapshot.values;

    // Construct ConstraintInput with determinacy context.
    let input = ConstraintInput {
        constraints: Cow::Owned(constraint_pairs),
        values: &eval_result.values,
        functions: &compiled.functions,
        determinacy: Some(det_map),
    };

    let simple_checker = SimpleConstraintChecker;
    let results = simple_checker.check(&input);

    assert!(
        !results.is_empty(),
        "expected at least one constraint result"
    );
    for result in &results {
        assert_eq!(
            result.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied with determinacy context, got {:?}",
            result.id,
            result.satisfaction
        );
    }
}

/// Predicates check DeterminacyState, not Value content.
/// x = determined(a) evaluates to Bool(false) since a is Undetermined,
/// but x itself is a Determined let binding (it successfully evaluated).
/// So determined(x) should be true despite x holding Bool(false).
#[test]
fn determined_checks_state_not_value() {
    let values = eval_source(
        r#"
        structure S {
            param a : Real
            let x = determined(a)
            let r = determined(x)
        }
    "#,
    );
    let x_id = ValueCellId::new("S", "x");
    let x_val = values.get(&x_id).expect("x should exist in values map");
    // x evaluates to Bool(false) because a is Undetermined
    assert_eq!(
        *x_val,
        Value::Bool(false),
        "x = determined(a) should be Bool(false) since a is Undetermined"
    );

    let r_id = ValueCellId::new("S", "r");
    let r_val = values.get(&r_id).expect("r should exist in values map");
    // r = determined(x) should be true because x IS Determined (it evaluated successfully)
    assert_eq!(
        *r_val,
        Value::Bool(true),
        "determined(x) should be true — x is Determined despite holding Bool(false)"
    );
}
