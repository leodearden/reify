//! A kernel-free selector ctor evaluates to the same value at ANY expression depth
//! as it does let-bound at the top level (task #7875).
//!
//! Each `Probe` cell below pairs a nested form (the ctor inside a fn-call arg, a
//! user-fn body, an `if` branch, a builtin arg) with a let-bound reference form of
//! the same value. Before #7875 every nested form evaluated its selector to
//! `undef`, silently dropping e.g. an inline `PressureLoad(face: face(..))` from
//! an FEA solve. A nested ctor that cannot build reports its warning exactly once.

use reify_core::identity::ValueCellId;
use reify_core::ty::SelectorKind;
use reify_core::{Diagnostic, Severity, Type, VersionId};
use reify_ir::{CompiledExprKind, Value, ValueMap};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

const SOURCE: &str = r#"
structure def MySup {
    param target : EdgeSelector
}
structure def FaceHolder {
    param sel : FaceSelector
}
fn wrap(xs : List<MySup>) -> List<MySup> { xs }
fn wrap_faces(xs : List<FaceHolder>) -> List<FaceHolder> { xs }
fn id_sel(s : EdgeSelector) -> EdgeSelector { s }
fn mk(b : Solid) -> MySup { MySup(target: edge(b, "spine")) }
structure def Probe {
    param length : Length = 100mm
    param flag : Bool = true
    let body = box(length, length, length)

    let direct = edge(body, "spine")
    let top = MySup(target: edge(body, "spine"))
    let listed = [MySup(target: edge(body, "spine"))]
    let top_faces = [FaceHolder(sel: faces(body))]

    let nested = wrap([MySup(target: edge(body, "spine"))])
    let sel_arg = id_sel(edge(body, "spine"))
    let from_fn = mk(body)
    let in_if = if flag then MySup(target: edge(body, "spine")) else MySup(target: edge(body, "spine"))
    let opt = some(MySup(target: edge(body, "spine")))
    let nested_faces = wrap_faces([FaceHolder(sel: faces(body))])
}
"#;

fn probe<'a>(values: &'a ValueMap, member: &str) -> &'a Value {
    values
        .get(&ValueCellId::new("Probe", member))
        .unwrap_or_else(|| panic!("Probe.{member} missing from the eval result"))
}

fn field<'a>(v: &'a Value, name: &str) -> &'a Value {
    match v {
        Value::StructureInstance(data) => data
            .fields
            .get(&name.to_string())
            .unwrap_or_else(|| panic!("{} has no field `{name}`", data.type_name)),
        other => panic!("expected a StructureInstance with field `{name}`, got {other:?}"),
    }
}

fn only_item(v: &Value) -> &Value {
    match v {
        Value::List(items) if items.len() == 1 => &items[0],
        other => panic!("expected a one-element List, got {other:?}"),
    }
}

fn assert_selector(label: &str, v: &Value) {
    assert!(
        matches!(v, Value::Selector(_)),
        "{label} must be a resolved Value::Selector, got {v:?}"
    );
}

fn eval_probe() -> ValueMap {
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    make_simple_engine().eval(&compiled).values
}

#[test]
fn ctor_field_nested_in_fn_call_arg_equals_let_bound() {
    let values = eval_probe();
    let nested = probe(&values, "nested");
    assert_selector("nested[0].target", field(only_item(nested), "target"));
    assert_eq!(nested, probe(&values, "listed"));
}

#[test]
fn selector_ctor_as_direct_fn_arg_equals_let_bound() {
    let values = eval_probe();
    let sel_arg = probe(&values, "sel_arg");
    assert_selector("sel_arg", sel_arg);
    assert_eq!(sel_arg, probe(&values, "direct"));
}

#[test]
fn ctor_built_in_user_fn_body_equals_let_bound() {
    let values = eval_probe();
    let from_fn = probe(&values, "from_fn");
    assert_selector("from_fn.target", field(from_fn, "target"));
    assert_eq!(from_fn, probe(&values, "top"));
}

#[test]
fn ctor_in_if_branch_equals_let_bound() {
    let values = eval_probe();
    let in_if = probe(&values, "in_if");
    assert_selector("in_if.target", field(in_if, "target"));
    assert_eq!(in_if, probe(&values, "top"));
}

#[test]
fn ctor_in_builtin_call_arg_equals_let_bound() {
    let values = eval_probe();
    let top = probe(&values, "top");
    assert_selector("top.target", field(top, "target"));
    assert_eq!(
        probe(&values, "opt"),
        &Value::Option(Some(Box::new(top.clone())))
    );
}

#[test]
fn all_leaf_ctor_nested_in_fn_call_arg_equals_let_bound() {
    let values = eval_probe();
    let nested_faces = probe(&values, "nested_faces");
    assert_selector("nested_faces[0].sel", field(only_item(nested_faces), "sel"));
    assert_eq!(nested_faces, probe(&values, "top_faces"));
}

#[test]
fn nested_forms_stay_equal_after_edit_param() {
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    engine.eval(&compiled);
    let edited = engine
        .edit_param(ValueCellId::new("Probe", "length"), Value::length(0.2))
        .expect("edit_param must succeed after eval");

    let nested = probe(&edited.values, "nested");
    assert_selector("nested[0].target", field(only_item(nested), "target"));
    assert_eq!(nested, probe(&edited.values, "listed"));

    let from_fn = probe(&edited.values, "from_fn");
    assert_selector("from_fn.target", field(from_fn, "target"));
    assert_eq!(from_fn, probe(&edited.values, "top"));
}

#[test]
fn nested_forms_equal_on_eval_cached() {
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let values = make_simple_engine()
        .eval_cached(&compiled, VersionId(1))
        .eval_result
        .values;
    let nested = probe(&values, "nested");
    assert_selector("nested[0].target", field(only_item(nested), "target"));
    assert_eq!(nested, probe(&values, "listed"));
}

/// `[union(faces(body), edges(body))]` as hand-built IR: the compiler rejects the
/// mixed-kind source form (`E_SELECTOR_KIND_MISMATCH`), so the valid all-faces form
/// is compiled and its second operand re-kinded to `edges`.
fn compile_nested_mixed_kind_union() -> reify_compiler::CompiledModule {
    let mut compiled = parse_and_compile_with_stdlib(
        r#"
structure def Probe {
    param length : Length = 100mm
    let body = box(length, length, length)
    let mixed = [union(faces(body), faces(body))]
}
"#,
    );
    let cell = compiled
        .templates
        .iter_mut()
        .flat_map(|t| t.value_cells.iter_mut())
        .find(|c| c.id == ValueCellId::new("Probe", "mixed"))
        .expect("Probe.mixed is a value cell");
    let Some(CompiledExprKind::ListLiteral(items)) =
        cell.default_expr.as_mut().map(|e| &mut e.kind)
    else {
        panic!("Probe.mixed must compile to a list literal");
    };
    let CompiledExprKind::FunctionCall { args, .. } = &mut items[0].kind else {
        panic!("Probe.mixed[0] must compile to the union call");
    };
    let CompiledExprKind::FunctionCall { function, .. } = &mut args[1].kind else {
        panic!("union's second operand must compile to the faces call");
    };
    function.name = "edges".to_string();
    function.qualified_name = "std::edges".to_string();
    args[1].result_type = Type::Selector(SelectorKind::Edge);
    compiled
}

fn kind_closure_warnings(diagnostics: &[Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("kind-closure violation"))
        .count()
}

#[test]
fn nested_kind_closure_violation_warns_exactly_once() {
    let compiled = compile_nested_mixed_kind_union();

    let mut engine = make_simple_engine();
    let evaluated = engine.eval(&compiled);
    assert_eq!(
        probe(&evaluated.values, "mixed"),
        &Value::List(vec![Value::Undef])
    );
    assert_eq!(kind_closure_warnings(&evaluated.diagnostics), 1, "eval");

    let edited = engine
        .edit_param(ValueCellId::new("Probe", "length"), Value::length(0.2))
        .expect("edit_param must succeed after eval");
    assert_eq!(kind_closure_warnings(&edited.diagnostics), 1, "edit_param");

    let cached = make_simple_engine().eval_cached(&compiled, VersionId(1));
    assert_eq!(
        kind_closure_warnings(&cached.eval_result.diagnostics),
        1,
        "eval_cached"
    );
}
