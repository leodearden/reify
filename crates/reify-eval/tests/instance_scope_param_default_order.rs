//! Instance-scope counterpart of the template-scope ordering fix in
//! `param_default_sibling_let_order.rs` (task #4317).
//!
//! That file covers `engine_eval.rs`, where task #4317 collapsed the
//! kind-partitioned two-pass into ONE dependency-ordered pass over all non-Auto
//! body cells. THIS file covers `unfold.rs::elaborate_child_params_only` — the
//! phase-1 param loop that runs when a template is instantiated under a `sub`.
//! Instance scope never got the analogous treatment, so a param default reading
//! a sibling param declared AFTER it evaluated against a `child_values` that did
//! not hold the sibling yet and degraded to `Undef`, silently, while the
//! template cell held the evaluated default.
//!
//! The property these tests pin is TWO-SCOPE AGREEMENT: each assertion compares
//! the instance cell against the template cell as an equality AND against the
//! expected literal. The equality alone would be satisfied by both scopes
//! degrading together; the literal alone would let a future regression that
//! degrades both scopes pass. Together they pin what the ticket asks for.

use reify_compiler::CompiledModule;
use reify_core::{Severity, ValueCellId};
use reify_eval::Engine;
use reify_ir::{DeterminacyState, Value};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile;

/// Build an Engine with an empty prelude for self-contained tests.
fn fresh_engine() -> Engine {
    Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[])
}

/// Convenience: parse + compile a source string.
fn compile_source(source: &str) -> CompiledModule {
    parse_and_compile(source)
}

// ──────────────────────────────────────────────────────────────────────────────
// step-1: RED — core repro, a param default reading a later-declared sibling
// ──────────────────────────────────────────────────────────────────────────────

/// The canonical repro. `Inner.p` reads `seed`, declared after it.
///
/// MEASURED RED on base 5efc4c04ce: template `Inner.p == Int(4)`, instance
/// `Outer.a.p == Undef`, `Outer.a.seed == Int(3)`, and no diagnostic at all —
/// the divergence is silent. Plain arithmetic, no `@optimized` anywhere, which
/// is what makes this independent of #6662's reuse gate.
#[test]
fn param_default_reading_a_later_declared_sibling_matches_template_at_instance_scope() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure Inner { \
            param p : Int = seed + 1 \
            param seed : Int = 3 \
        } \
        structure Outer { \
            sub a = Inner() \
        }",
    );

    let result = engine.eval(&module);

    let template_p = ValueCellId::new("Inner", "p");
    let instance_p = ValueCellId::new("Outer.a", "p");

    // The template cell is the reference point. Pinned to the literal too, so
    // this test cannot pass by BOTH scopes degrading together.
    assert_eq!(
        result.values.get(&template_p),
        Some(&Value::Int(4)),
        "template scope resolves the forward read (task #4317); if this is not \
         Int(4) the template-scope fix has regressed and the instance-scope \
         assertion below is measuring nothing"
    );

    assert_eq!(
        result.values.get(&instance_p),
        result.values.get(&template_p),
        "instance scope must agree with template scope on a forward-referencing \
         param default: `elaborate_child_params_only` visits params in dependency \
         order, so `seed` is in `child_values` before `p`'s default runs"
    );
    assert_eq!(
        result.values.get(&instance_p),
        Some(&Value::Int(4)),
        "Outer.a.p must be Int(4); Undef here is the declaration-order gap"
    );

    // `seed` itself was never the problem — it is the control that shows only
    // the forward-reading cell degraded.
    assert_eq!(
        result.values.get(&ValueCellId::new("Outer.a", "seed")),
        Some(&Value::Int(3)),
        "Outer.a.seed has no dependency and resolved even before the fix"
    );

    // Determinacy, not just the value: the gap left the pair (Undef, Undetermined).
    let snap = engine.snapshot().expect("snapshot after eval");
    let (p_val, p_det) = snap
        .values
        .get(&instance_p)
        .expect("snapshot must contain Outer.a.p after eval");
    assert_eq!(
        *p_det,
        DeterminacyState::Determined,
        "Outer.a.p must be Determined, not the (Undef, Undetermined) pair the \
         declaration-order gap produced. Got: ({p_val:?}, {p_det:?})"
    );

    assert_no_error_diagnostics(&result, "a valid forward param->param dependency");
}

/// Transitivity, not a one-hop special case: a 3-deep chain declared entirely in
/// reverse. A fix that only looks one cell ahead resolves `b` and leaves `c`
/// Undef; only a real topological order resolves all three.
///
/// MEASURED RED on base: instance `b` and `c` are both Undef.
#[test]
fn param_defaults_in_reverse_declaration_order_resolve_transitively_at_instance_scope() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure Chain { \
            param c : Int = b + 1 \
            param b : Int = a + 1 \
            param a : Int = 1 \
        } \
        structure Holder { \
            sub s = Chain() \
        }",
    );

    let result = engine.eval(&module);

    for (member, expected) in [("a", 1), ("b", 2), ("c", 3)] {
        let template_id = ValueCellId::new("Chain", member);
        let instance_id = ValueCellId::new("Holder.s", member);
        assert_eq!(
            result.values.get(&template_id),
            Some(&Value::Int(expected)),
            "template scope must resolve the reverse chain at Chain.{member}"
        );
        assert_eq!(
            result.values.get(&instance_id),
            result.values.get(&template_id),
            "instance scope must agree with template scope at Holder.s.{member}"
        );
        assert_eq!(
            result.values.get(&instance_id),
            Some(&Value::Int(expected)),
            "Holder.s.{member} must be Int({expected}); a one-hop-only fix leaves \
             the far end of the chain Undef"
        );
    }

    assert_no_error_diagnostics(&result, "a valid reverse-declared param chain");
}

/// The symmetry control: params ALREADY in dependency order must stay resolved.
/// Green today and must stay green — this is what catches a "fix" that merely
/// inverts the visit order instead of ordering it.
#[test]
fn params_in_declaration_order_still_resolve_at_instance_scope() {
    let mut engine = fresh_engine();
    let module = compile_source(
        "structure Fwd { \
            param a : Int = 1 \
            param b : Int = a + 1 \
        } \
        structure FwdHolder { \
            sub s = Fwd() \
        }",
    );

    let result = engine.eval(&module);

    for (member, expected) in [("a", 1), ("b", 2)] {
        let template_id = ValueCellId::new("Fwd", member);
        let instance_id = ValueCellId::new("FwdHolder.s", member);
        assert_eq!(
            result.values.get(&instance_id),
            result.values.get(&template_id),
            "instance scope must agree with template scope at FwdHolder.s.{member}"
        );
        assert_eq!(
            result.values.get(&instance_id),
            Some(&Value::Int(expected)),
            "FwdHolder.s.{member} must be Int({expected}): dependency-ordering the \
             param loop must not break the already-working declaration order"
        );
    }

    assert_no_error_diagnostics(&result, "params declared in dependency order");
}

// ──────────────────────────────────────────────────────────────────────────────
// shared assertion
// ──────────────────────────────────────────────────────────────────────────────

fn assert_no_error_diagnostics(result: &reify_eval::EvalResult, what: &str) {
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "eval must produce no Error diagnostics for {what}; got: {errors:?}"
    );
}
