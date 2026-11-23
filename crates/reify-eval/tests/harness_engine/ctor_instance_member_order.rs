//! Ctor-built counterpart of the instance-scope ordering pins in
//! `instance_scope_param_default_order.rs` (task #10098).
//!
//! A structure instance built by a constructor EXPRESSION — `let primary =
//! Beam()`, a param-held ctor default, a fn-returned ctor — is evaluated by
//! reify-expr's structure-ctor evaluator, not by `unfold.rs`. Its param
//! defaults and lets may read one another in either direction, exactly as at
//! template scope and under a `sub`.
//!
//! The property these tests pin is AGREEMENT: each ctor-built field is
//! compared against the template cell (and the `sub` cell where one exists)
//! as an equality AND against the expected literal. The equality alone would
//! be satisfied by every scope degrading together; the literal alone would
//! let the scopes drift apart.

use reify_core::{Severity, ValueCellId};
use reify_eval::{Engine, EvalResult};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile;

/// Build an Engine with an empty prelude for self-contained tests.
fn fresh_engine() -> Engine {
    Engine::with_prelude(
        reify_eval::SolverPosture::SolverFree,
        Box::new(MockConstraintChecker::new()),
        None,
        &[],
    )
}

fn eval_source(source: &str) -> EvalResult {
    fresh_engine().eval(&parse_and_compile(source))
}

/// The value of a plain cell: a template member or a `sub` member.
#[track_caller]
fn cell(result: &EvalResult, entity: &str, member: &str) -> Value {
    result
        .values
        .get(&ValueCellId::new(entity, member))
        .cloned()
        .unwrap_or_else(|| panic!("no cell {entity}.{member} after eval"))
}

/// A field of the structure instance held by cell `host.holder`.
#[track_caller]
fn ctor_field(result: &EvalResult, host: &str, holder: &str, member: &str) -> Value {
    match cell(result, host, holder) {
        Value::StructureInstance(data) => data.fields.get(member).cloned().unwrap_or_else(|| {
            panic!("{host}.{holder} has no `{member}` field: {:?}", data.fields)
        }),
        other => panic!("{host}.{holder} must hold a StructureInstance, got {other:?}"),
    }
}

/// The number a resolved numeric field holds; the Real representation is not
/// pinned, only its value.
#[track_caller]
fn number(value: &Value, what: &str) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| panic!("{what} must be a resolved number, got {value:?}"))
}

/// Assert a ctor-built field agrees with the template cell and equals `expected`.
#[track_caller]
fn assert_ctor_field_agrees(
    result: &EvalResult,
    host: &str,
    holder: &str,
    member: &str,
    expected: f64,
) {
    let template = cell(result, "Beam", member);
    assert_eq!(
        number(&template, &format!("template Beam.{member}")),
        expected,
        "template scope must resolve Beam.{member}; otherwise the agreement \
         below measures nothing"
    );
    assert_eq!(
        ctor_field(result, host, holder, member),
        template,
        "{host}.{holder}.{member} must agree with template Beam.{member}: a \
         ctor-built instance evaluates its members in dependency order, so a \
         forward read must not leave the field Undef"
    );
}

#[track_caller]
fn assert_no_error_diagnostics(result: &EvalResult, what: &str) {
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

/// The task repro and its declaration-order twin. Real representation is not
/// pinned, so the assertion is three-way agreement plus not-Undef.
#[track_caller]
fn assert_default_reading_sibling_let_agrees(beam_body: &str) {
    let source = format!(
        "structure Beam {{ {beam_body} }} \
         structure NestedAssembly {{ \
             let primary = Beam() \
             sub secondary = Beam() \
         }}"
    );
    let result = eval_source(&source);

    let template = cell(&result, "Beam", "n");
    let via_sub = cell(&result, "NestedAssembly.secondary", "n");
    let via_ctor = ctor_field(&result, "NestedAssembly", "primary", "n");

    assert!(!template.is_undef(), "template Beam.n must resolve");
    assert_eq!(
        via_sub, template,
        "the sub instance must agree with template scope"
    );
    assert_eq!(
        via_ctor, template,
        "the let-constructed instance must agree with template scope on a param \
         default reading a sibling let, not evaluate it to Undef"
    );
    assert_no_error_diagnostics(&result, "a param default reading a sibling let");
}

#[test]
fn let_ctor_param_default_reading_a_later_let_agrees_with_template_and_sub() {
    assert_default_reading_sibling_let_agrees("param n : Real = k  let k = 3.0");
}

#[test]
fn let_ctor_param_default_reading_an_earlier_let_agrees_with_template_and_sub() {
    assert_default_reading_sibling_let_agrees("let k = 3.0  param n : Real = k");
}

/// Forward reads in every direction: let→later let, param→let, param→param,
/// let→param. Real, not Int: the compiler types a forward read as Real, so an
/// `Int`-annotated member reading a later one does not compile.
const MIXED_CHAIN_BEAM: &str = "structure Beam { \
    let a = b + 1.0 \
    let b = 3.0 \
    param n : Real = a \
    param m : Real = n * 2.0 \
    let c = m + n \
}";

#[test]
fn let_ctor_mixed_chain_agrees_with_template_scope() {
    let result = eval_source(&format!(
        "{MIXED_CHAIN_BEAM} structure Host {{ let primary = Beam() }}"
    ));

    for (member, expected) in [("a", 4.0), ("b", 3.0), ("c", 12.0), ("m", 8.0), ("n", 4.0)] {
        assert_ctor_field_agrees(&result, "Host", "primary", member, expected);
    }
    assert_no_error_diagnostics(&result, "a mixed default/let chain");
}

#[test]
fn let_ctor_arg_supplied_param_feeds_downstream_members() {
    let result = eval_source(&format!(
        "{MIXED_CHAIN_BEAM} structure Host {{ let withArg = Beam(n: 10.0) }}"
    ));

    for (member, expected) in [("n", 10.0), ("m", 20.0), ("c", 30.0), ("a", 4.0)] {
        let what = format!("Host.withArg.{member}");
        assert_eq!(
            number(&ctor_field(&result, "Host", "withArg", member), &what),
            expected,
            "{what} must be {expected}: an arg-supplied param feeds its readers, \
             and members it does not reach still resolve"
        );
    }
    assert_no_error_diagnostics(&result, "a ctor with an arg-supplied param");
}

#[test]
fn param_held_and_fn_returned_ctors_agree_with_template_scope() {
    let result = eval_source(&format!(
        "{MIXED_CHAIN_BEAM} \
         structure Holder {{ param b : Beam = Beam() }} \
         fn make() -> Beam {{ Beam() }} \
         structure Host {{ let via_fn = make() }}"
    ));

    assert_ctor_field_agrees(&result, "Holder", "b", "n", 4.0);
    assert_ctor_field_agrees(&result, "Host", "via_fn", "n", 4.0);
    assert_no_error_diagnostics(&result, "param-held and fn-returned ctors");
}
