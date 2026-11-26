//! Integration test: a fn body can construct a structure_def from the SAME module.
//!
//! `Widget()` inside `make_widget()` must lower to
//! `CompiledExprKind::StructureInstanceCtor` (not a generic `UserFunctionCall`), and
//! eval must produce a `Value::StructureInstance` carrying the structure_def's real
//! defaults.
//!
//! Fn param defaults and fn bodies compile in one source-order pass interleaved with
//! entity templates (`entities_phase::phase_entities`), so a fn body constructs a
//! same-module structure from its authoritative `TopologyTemplate`. Declaration order
//! is load-bearing: constructing a structure declared BELOW the fn is an ordering
//! error (see `fn_body_ctor_of_later_declared_structure_reports_ordering_error`).
//!
//! Rationale: docs/prds/v0_6/stdlib-namespace.md, decision D-8.

use reify_core::*;
use reify_ir::*;

// ─── source fixture ───────────────────────────────────────────────────────────

/// Single ad-hoc module containing both a structure_def and pub fns that
/// construct it — the minimal same-module ctor test case.
const WIDGET_SRC: &str = r#"
module test.widget

structure def Widget {
    param width : Real = 3.5
    param tag   : Bool = true
}

pub fn make_widget() -> Widget { Widget() }
pub fn make_widget_partial() -> Widget { Widget(5.5) }
"#;

// ─── helpers ──────────────────────────────────────────────────────────────────

fn load_widget_module() -> reify_compiler::CompiledModule {
    reify_test_support::helpers::compile_source_with_stdlib(WIDGET_SRC)
}

/// Compile `source` with the stdlib/prelude in scope and evaluate it end-to-end.
fn eval_source_with_stdlib(source: &str) -> reify_eval::EvalResult {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(source);
    let mut engine = reify_eval::Engine::new(
        reify_eval::SolverPosture::SolverFree,
        Box::new(reify_test_support::mocks::MockConstraintChecker::new()),
        None,
    );
    engine.eval(&compiled)
}

// ─── zero errors ──────────────────────────────────────────────────────────────

/// The module must compile with zero `Severity::Error` diagnostics: `Widget()` in
/// a same-module fn body resolves against `Widget`'s authoritative template, which
/// the unified source-order pass has already built by the time the fn body compiles.
#[test]
fn same_module_widget_zero_errors() {
    let module = load_widget_module();
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "same-module Widget module should compile with zero errors; \
         got: {:#?}",
        errors
    );
}

// ─── make_widget body is StructureInstanceCtor ────────────────────────────────

/// `make_widget()`'s result_expr must lower to
/// `CompiledExprKind::StructureInstanceCtor` with `type_name == "Widget"` — not a
/// generic `UserFunctionCall`.
#[test]
fn make_widget_body_is_structure_instance_ctor() {
    let module = load_widget_module();
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "make_widget")
        .unwrap_or_else(|| {
            panic!(
                "expected `make_widget` in compiled module; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    match &func.body.result_expr.kind {
        CompiledExprKind::StructureInstanceCtor { type_name, .. } => {
            assert_eq!(
                type_name, "Widget",
                "make_widget body StructureInstanceCtor.type_name should be \
                 \"Widget\"; got: {}",
                type_name
            );
        }
        other => panic!(
            "make_widget body result_expr.kind should be \
             CompiledExprKind::StructureInstanceCtor; got: {:?}",
            other
        ),
    }
}

// ─── eval make_widget() → Value::StructureInstance with defaults ──────────────

/// Evaluating `make_widget()` must yield `Value::StructureInstance` with
/// `type_name == "Widget"`, `width == Value::Real(3.5)`, `tag == Value::Bool(true)`.
#[test]
fn eval_make_widget_returns_struct_with_defaults() {
    let module = load_widget_module();

    let call_expr = CompiledExpr::user_function_call(
        "make_widget".to_string(),
        vec![],
        Type::StructureRef("Widget".to_string()),
    );
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(&call_expr, &ctx);

    let data = match &result {
        Value::StructureInstance(data) => data,
        other => panic!(
            "make_widget() should return Value::StructureInstance; got: {:?}",
            other
        ),
    };

    assert_eq!(
        data.type_name, "Widget",
        "make_widget() StructureInstance.type_name should be \"Widget\"; got: {}",
        data.type_name
    );

    // Count only user-facing param/let fields, excluding the `@@`-prefixed
    // overlay keys (e.g. task 4089's `@@source_span` construction-site span,
    // which `eval_structure_instance_ctor` attaches to every ctor and which is
    // intentionally excluded from identity/Display — see reify-ir value.rs).
    let user_field_count = data
        .fields
        .iter()
        .filter(|(k, _)| !k.starts_with("@@"))
        .count();
    assert_eq!(
        user_field_count,
        2,
        "make_widget() StructureInstance should have 2 user fields; got: {:?}",
        data.fields.keys().collect::<Vec<_>>()
    );

    // width default = 3.5
    let width = data
        .fields
        .get(&"width".to_string())
        .expect("make_widget().width missing");
    assert_eq!(
        *width,
        Value::Real(3.5),
        "make_widget().width should be Real(3.5) (structure_def default); got: {:?}",
        width
    );

    // tag default = true
    let tag = data
        .fields
        .get(&"tag".to_string())
        .expect("make_widget().tag missing");
    assert_eq!(
        *tag,
        Value::Bool(true),
        "make_widget().tag should be Bool(true) (structure_def default); got: {:?}",
        tag
    );
}

// ─── eval make_widget_partial() → covered positional arg + default ────────────

/// `make_widget_partial()` passes `5.5` as the first positional arg (`width`).
/// Eval must yield `width == Value::Real(5.5)` (covered) and
/// `tag == Value::Bool(true)` (default).
#[test]
fn eval_make_widget_partial_covers_first_param() {
    let module = load_widget_module();

    let call_expr = CompiledExpr::user_function_call(
        "make_widget_partial".to_string(),
        vec![],
        Type::StructureRef("Widget".to_string()),
    );
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(&call_expr, &ctx);

    let data = match &result {
        Value::StructureInstance(data) => data,
        other => panic!(
            "make_widget_partial() should return Value::StructureInstance; got: {:?}",
            other
        ),
    };

    // width covered by positional arg 5.5
    let width = data
        .fields
        .get(&"width".to_string())
        .expect("make_widget_partial().width missing");
    assert_eq!(
        *width,
        Value::Real(5.5),
        "make_widget_partial().width should be Real(5.5) (positional arg); got: {:?}",
        width
    );

    // tag still defaults to true
    let tag = data
        .fields
        .get(&"tag".to_string())
        .expect("make_widget_partial().tag missing");
    assert_eq!(
        *tag,
        Value::Bool(true),
        "make_widget_partial().tag should be Bool(true) (default); got: {:?}",
        tag
    );
}

// ─── sibling-param default survives the fn-returned ctor ──────────────────────

/// A structure-def param whose default expression references a SIBLING param
/// must keep that default's real value when the structure is built through a
/// fn-returned ctor: `App.py == 2.0` requires both the compile-side sibling
/// reference and the eval-side default scoping (see `eval_structure_instance_ctor`
/// in reify-expr).
#[test]
fn sibling_param_default_in_fn_returned_ctor_evaluates_correctly() {
    let src = r#"
module test.sibling_default
structure def Point { param x : Real = 1.0  param y : Real = x + 1.0 }
pub fn make_point() -> Point { Point() }
structure App {
    let p = make_point()
    let py = p.y
}
"#;
    let module = reify_test_support::helpers::compile_source_with_stdlib(src);

    // Zero errors.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "sibling-default module should compile with zero errors; got: {:#?}",
        errors
    );

    // The fn body IS a StructureInstanceCtor.
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "make_point")
        .expect("make_point must be in compiled module");

    match &func.body.result_expr.kind {
        CompiledExprKind::StructureInstanceCtor { type_name, .. } => {
            assert_eq!(type_name, "Point");
        }
        other => panic!(
            "make_point body should be StructureInstanceCtor; got: {:?}",
            other
        ),
    }

    // `y`'s sibling-referencing default must carry its real value
    // (`x + 1.0` == 2.0) through the fn-returned ctor, not a poison.
    let result = eval_source_with_stdlib(src);
    reify_test_support::assert_no_eval_errors(&result);
    let py = reify_test_support::cell_value(&result, "App", "py");
    assert_eq!(
        py,
        Value::Real(2.0),
        "App.py must be Point's sibling-referencing default `x + 1.0` == 2.0; \
         got: {:?}",
        py
    );
}

// ─── task-4342 step-3b: fn-returned struct carries its template's lets ─────────

const TOL_SRC: &str = r#"
module test.tol

structure def Tol {
    param nominal      : Real = 0.0
    param upper_dev    : Real = 0.001
    let upper_limit    = nominal + upper_dev
}

pub fn make_tol() -> Tol { Tol(5.0, 0.02) }
"#;

/// step_3b (task-4342): a same-module fn-returned struct with a sibling-referencing
/// derived let must produce a StructureInstanceCtor that carries `lets` AND
/// whose let expr is NOT poison (result_type != Type::Error).
#[test]
fn fn_returned_struct_ctor_carries_non_poison_lets() {
    let module = reify_test_support::helpers::compile_source_with_stdlib(TOL_SRC);

    // The module must compile without errors.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Tol module should compile without errors; got: {:#?}",
        errors
    );

    // find make_tol in compiled functions
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "make_tol")
        .unwrap_or_else(|| {
            panic!(
                "expected `make_tol` in compiled module; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    // The fn body must be a StructureInstanceCtor.
    match &func.body.result_expr.kind {
        CompiledExprKind::StructureInstanceCtor {
            type_name, lets, ..
        } => {
            assert_eq!(type_name, "Tol", "fn body ctor type_name must be Tol");
            assert_eq!(
                lets.len(),
                1,
                "fn-returned Tol ctor must carry 1 let (upper_limit); got {} lets: {:?}",
                lets.len(),
                lets.iter().map(|(n, _)| n).collect::<Vec<_>>()
            );
            assert_eq!(
                lets[0].0, "upper_limit",
                "let member name must be upper_limit"
            );
            // The let expr must not be poison: the authoritative template
            // registers sibling params before compiling let exprs, so the
            // sibling refs resolve.
            assert_ne!(
                lets[0].1.result_type,
                Type::Error,
                "let expr result_type must not be Error (sibling refs must resolve)"
            );
        }
        other => panic!(
            "make_tol body should be StructureInstanceCtor; got: {:?}",
            other
        ),
    }
}

// ─── task-4342 step-9: guarded lets excluded from the fn-returned ctor ────────

/// A structure with both an unguarded `let` and a per-decl-guarded `let … where cond`
/// in a same-module fn-returned position.
///
/// Per-decl-guarded lets (`where_clause.is_some()`) are routed to `guarded_groups`,
/// never to the ctor's `lets`, so a fn-returned ctor and a `sub` agree.
const GUARDED_LET_SRC: &str = r#"
module test.guarded

structure def G {
    param nominal    : Real = 0.0
    param upper_dev  : Real = 0.001
    param active     : Bool = true
    let plain_limit   = nominal + upper_dev
    let guarded_limit = nominal - upper_dev where active
}

pub fn make_g() -> G { G(5.0, 0.02, true) }
"#;

#[test]
fn fn_returned_struct_ctor_excludes_guarded_lets() {
    let module = reify_test_support::helpers::compile_source_with_stdlib(GUARDED_LET_SRC);

    // (a) Zero errors.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "G module should compile without errors; got: {:#?}",
        errors
    );

    // (b) make_g body is StructureInstanceCtor.
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "make_g")
        .unwrap_or_else(|| {
            panic!(
                "expected `make_g` in compiled module; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    match &func.body.result_expr.kind {
        CompiledExprKind::StructureInstanceCtor {
            type_name, lets, ..
        } => {
            assert_eq!(type_name, "G", "fn body ctor type_name must be G");

            let let_names: Vec<&str> = lets.iter().map(|(n, _)| n.as_str()).collect();

            // (c) Unguarded let is present.
            assert!(
                let_names.contains(&"plain_limit"),
                "ctor `lets` must contain \"plain_limit\" (unguarded); got: {:?}",
                let_names
            );

            // (d) Guarded let is absent — must not be eagerly materialized.
            assert!(
                !let_names.contains(&"guarded_limit"),
                "ctor `lets` must NOT contain \"guarded_limit\" (per-decl guarded); \
                 got: {:?}",
                let_names
            );
        }
        other => panic!(
            "make_g body should be StructureInstanceCtor; got: {:?}",
            other
        ),
    }
}

// ─── task-4342 (esc-4342-21): geometry-let alias excluded from the ctor ────────

/// A structure with a geometry let (`base = box(...)`), an Ident *alias* to that
/// geometry let (`shifted = base`), and a scalar derived let, in a fn-returned
/// position.
///
/// Both `base` and its Ident alias `shifted` are routed out of the ctor `lets`; the
/// scalar `scaled` survives.
const GEOMETRY_ALIAS_SRC: &str = r#"
module test.geomalias

structure def H {
    param w      : Length = 1mm
    param factor : Real   = 2.0
    let base    = box(w, w, w)
    let shifted = base
    let scaled  = w * factor
}

pub fn make_h() -> H { H(1mm, 2.0) }
"#;

#[test]
fn fn_returned_struct_ctor_excludes_geometry_let_alias() {
    let module = reify_test_support::helpers::compile_source_with_stdlib(GEOMETRY_ALIAS_SRC);

    // (a) Zero errors.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "H module should compile without errors; got: {:#?}",
        errors
    );

    // (b) make_h body is StructureInstanceCtor.
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "make_h")
        .unwrap_or_else(|| {
            panic!(
                "expected `make_h` in compiled module; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    match &func.body.result_expr.kind {
        CompiledExprKind::StructureInstanceCtor {
            type_name, lets, ..
        } => {
            assert_eq!(type_name, "H", "fn body ctor type_name must be H");

            let let_names: Vec<&str> = lets.iter().map(|(n, _)| n.as_str()).collect();

            // (c) The direct geometry let is routed out.
            assert!(
                !let_names.contains(&"base"),
                "ctor `lets` must NOT contain \"base\" (direct geometry let); got: {:?}",
                let_names
            );

            // (d) The Ident alias of the geometry let is also routed out.
            assert!(
                !let_names.contains(&"shifted"),
                "ctor `lets` must NOT contain \"shifted\" (alias of geometry let \
                 `base`); got: {:?}",
                let_names
            );

            // (e) The scalar derived let still survives and is non-poison.
            assert!(
                let_names.contains(&"scaled"),
                "ctor `lets` must contain \"scaled\" (scalar derived let); got: {:?}",
                let_names
            );
            let scaled = lets.iter().find(|(n, _)| n == "scaled").unwrap();
            assert_ne!(
                scaled.1.result_type,
                Type::Error,
                "`scaled` let expr result_type must not be Error"
            );
        }
        other => panic!(
            "make_h body should be StructureInstanceCtor; got: {:?}",
            other
        ),
    }
}

// ─── geometry lets carried by a let-constructed ctor (task 10352) ─────────────

const GEOMETRY_LET_CTOR_SRC: &str = r#"
module test.geometry_let_ctor

structure def Beam {
    param len : Length = 1000mm
    let body = box(len, 100mm, 100mm)
    let tip = vertex(body, "tip")
}

structure def Holder {
    let b = Beam()
}
"#;

/// A let-constructed ctor carries its template's geometry let as a
/// `CtorGeometryLet` naming the let's realization and that realization's op
/// args, while the geometry let itself stays out of `lets`.
#[test]
fn let_constructed_ctor_carries_template_geometry_let() {
    let module = reify_test_support::parse_and_compile_with_stdlib(GEOMETRY_LET_CTOR_SRC);
    let template = |name: &str| {
        module
            .templates
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("no template {name}"))
    };
    let body_realization = template("Beam")
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some("body"))
        .expect("Beam's `body` realization");
    let holder_b = template("Holder")
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "b")
        .and_then(|vc| vc.default_expr.as_ref())
        .expect("Holder.b default");

    let CompiledExprKind::StructureInstanceCtor {
        lets,
        geometry_lets,
        ..
    } = &holder_b.kind
    else {
        panic!("Holder.b must lower to a StructureInstanceCtor, got {holder_b:?}");
    };

    let members: Vec<&str> = geometry_lets.iter().map(|g| g.member.as_str()).collect();
    assert_eq!(members, ["body"]);
    let body = &geometry_lets[0];
    assert_eq!(body.realization, body_realization.id);

    let hashed = |args: &mut dyn Iterator<Item = &(String, CompiledExpr)>| {
        args.map(|(name, e)| (name.clone(), e.content_hash))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        hashed(&mut body.upstream_args.iter()),
        hashed(&mut body_realization.operations.iter().flat_map(|op| op.args())),
        "upstream_args must be the `body` realization's op args"
    );

    let let_names: Vec<&str> = lets.iter().map(|(n, _)| n.as_str()).collect();
    assert!(let_names.contains(&"tip"), "lets: {let_names:?}");
    assert!(!let_names.contains(&"body"), "lets: {let_names:?}");
}

const GEOMETRY_LIST_LET_CTOR_SRC: &str = r#"
module test.geometry_list_let_ctor

structure def Rack {
    param len : Length = 1000mm
    let body = box(len, 100mm, 100mm)
    let parts = [box(len, 1mm, 1mm), box(2mm, len, 2mm)]
}

structure def Shelf {
    let r = Rack()
}
"#;

/// A geometry-list let is backed by one list-bound realization per element and
/// by no realization named after the let itself, so a let-constructed ctor
/// carries no `CtorGeometryLet` for it: only the single geometry let `body`.
#[test]
fn let_constructed_ctor_carries_no_geometry_list_let() {
    let module = reify_test_support::parse_and_compile_with_stdlib(GEOMETRY_LIST_LET_CTOR_SRC);
    let template = |name: &str| {
        module
            .templates
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("no template {name}"))
    };
    let list_bound: Vec<&str> = template("Rack")
        .realizations
        .iter()
        .filter(|r| {
            r.list_binding
                .as_ref()
                .is_some_and(|b| b.list_name == "parts")
        })
        .filter_map(|r| r.name.as_deref())
        .collect();
    assert_eq!(
        list_bound,
        ["parts#0", "parts#1"],
        "premise: `parts` lowers to one list-bound realization per element"
    );
    let shelf_r = template("Shelf")
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "r")
        .and_then(|vc| vc.default_expr.as_ref())
        .expect("Shelf.r default");

    let CompiledExprKind::StructureInstanceCtor { geometry_lets, .. } = &shelf_r.kind else {
        panic!("Shelf.r must lower to a StructureInstanceCtor, got {shelf_r:?}");
    };
    let members: Vec<&str> = geometry_lets.iter().map(|g| g.member.as_str()).collect();
    assert_eq!(members, ["body"]);
}

// ─── fn-call param default through a fn-returned ctor ─────────────────────────

/// A structure-def param default that calls a SAME-MODULE fn, consumed through a
/// fn-returned ctor, must evaluate to the fn's real value.
///
/// Declaration order is load-bearing and is the point of the fixture: `tol` is
/// declared before `Widget` (which calls it in a param default), and `Widget` is
/// declared before `make_widget` (which constructs it).  Every edge points
/// backwards, so a strict source-order compilation satisfies all of them.
#[test]
fn fn_returned_ctor_default_referencing_same_module_fn_evaluates_correctly() {
    let src = r#"
module test.slack

fn tol() -> Real { 0.25 }

structure def Widget {
    param w     : Real = 3.5
    param slack : Real = tol()
}

pub fn make_widget() -> Widget { Widget() }

structure App {
    let x = make_widget()
    let s = x.slack
}
"#;

    // (a) No compile-time errors — the whole fixture is correctly source-ordered.
    let module = reify_test_support::helpers::compile_source_with_stdlib(src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "same-module fn-call param default should compile with zero errors; \
         got: {:#?}",
        errors
    );

    // (b) No eval-time errors, and (c) the ctor default carries `tol()`'s REAL value.
    let result = eval_source_with_stdlib(src);
    reify_test_support::assert_no_eval_errors(&result);
    let s = reify_test_support::cell_value(&result, "App", "s");
    assert_eq!(
        s,
        Value::Real(0.25),
        "App.s must be tol()'s real value 0.25; got: {:?}",
        s
    );
}

// ─── nested ctor in a let through a fn-returned ctor ──────────────────────────

/// A `let` inside a same-module structure that itself constructs another
/// same-module structure must lower to a nested `StructureInstanceCtor` on the
/// fn-returned path — not to a generic function call — and must carry the inner
/// structure's own `lets` so `o.h.b` resolves at eval.
///
/// Asserts both the IR shape and the value.
#[test]
fn fn_returned_ctor_nested_ctor_in_let_carries_inner_lets() {
    let src = r#"
module test.nested

structure def Inner {
    param a : Real = 1.0
    let b = a + 1.0
}

structure def Outer {
    param k : Real = 2.0
    let h = Inner()
}

pub fn make_outer() -> Outer { Outer() }

structure App {
    let o = make_outer()
    let v = o.h.b
}
"#;

    let module = reify_test_support::helpers::compile_source_with_stdlib(src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "nested-ctor-in-let module should compile with zero errors; got: {:#?}",
        errors
    );

    // (a) IR shape: Outer's ctor `lets["h"]` must itself be a
    //     StructureInstanceCtor{Inner}, and must not be poison.
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "make_outer")
        .unwrap_or_else(|| {
            panic!(
                "expected `make_outer` in compiled module; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    match &func.body.result_expr.kind {
        CompiledExprKind::StructureInstanceCtor {
            type_name, lets, ..
        } => {
            assert_eq!(type_name, "Outer", "fn body ctor type_name must be Outer");
            let h = lets.iter().find(|(n, _)| n == "h").unwrap_or_else(|| {
                panic!(
                    "Outer ctor `lets` must contain \"h\"; got: {:?}",
                    lets.iter().map(|(n, _)| n).collect::<Vec<_>>()
                )
            });
            assert_ne!(
                h.1.result_type,
                Type::Error,
                "`h` let expr result_type must not be Error"
            );
            match &h.1.kind {
                CompiledExprKind::StructureInstanceCtor { type_name, .. } => {
                    assert_eq!(
                        type_name, "Inner",
                        "`let h = Inner()` must lower to StructureInstanceCtor{{Inner}}"
                    );
                }
                other => panic!(
                    "`let h = Inner()` must lower to a nested StructureInstanceCtor, \
                     not a generic call; got: {:?}",
                    other
                ),
            }
        }
        other => panic!(
            "make_outer body should be StructureInstanceCtor; got: {:?}",
            other
        ),
    }

    // (b) Value: `o.h.b` == Inner's `a + 1.0` == 2.0.
    let result = eval_source_with_stdlib(src);
    reify_test_support::assert_no_eval_errors(&result);
    let v = reify_test_support::cell_value(&result, "App", "v");
    assert_eq!(
        v,
        Value::Real(2.0),
        "App.v must be Inner's derived let `a + 1.0` == 2.0; got: {:?}",
        v
    );
}

// ─── a fn body constructing a LATER-declared structure fails LOUDLY ───────────

/// Constructing a same-module `structure def` declared BELOW the function is an
/// ordering ERROR that names the structure and the remedy.
///
/// A fn body sees only the templates built above it — the same no-forward-reference
/// rule `compile_entity` obeys for entity → entity references.
#[test]
fn fn_body_ctor_of_later_declared_structure_reports_ordering_error() {
    let src = r#"
module test.fwd
pub fn make_it() -> Thing { Thing() }
structure def Thing { param a : Real = 1.0 }
"#;
    let module = reify_test_support::helpers::compile_source_with_stdlib(src);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "constructing a later-declared same-module structure must be exactly one \
         Error — never a Warning-only silent fallback; all diagnostics: {:#?}",
        module
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    assert_eq!(
        errors[0].code,
        Some(DiagnosticCode::StructureDeclaredAfterFunction),
        "the ordering error must carry its dedicated code; got: {:?}",
        errors[0]
    );

    let msg = &errors[0].message;
    assert!(
        msg.contains("Thing"),
        "the ordering error must name the structure it could not construct; got: {msg}"
    );
    assert!(
        msg.contains("declared after"),
        "the ordering error must state the ordering RULE (fix-it shaped: \
         `structure def Thing` is declared after this function), not a bare \
         unknown-function message; got: {msg}"
    );

    // Not a silent success: the body must NOT have lowered to a ctor.
    assert_body_does_not_construct(&module, "make_it");
}

/// Diagnostics carrying the declaration-order code — or the ordering message,
/// so a code-less regression of the same lie is caught too.
fn ordering_diags(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::StructureDeclaredAfterFunction)
                || d.message.contains("is declared after this function")
        })
        .collect()
}

fn diag_lines(module: &reify_compiler::CompiledModule) -> Vec<String> {
    module
        .diagnostics
        .iter()
        .map(|d| format!("[{:?}] {:?} {}", d.severity, d.code, d.message))
        .collect()
}

/// Member-access counterpart of the declared-above rule: a fn may use a
/// same-module structure's template only if the `structure def` is declared above
/// it. A fn body reading a member of a param typed as a same-module structure
/// declared BELOW the fn has no template to check the member against. Without an
/// ordering diagnostic the access takes the permissive registry-miss fallback,
/// and a `priv` port or guarded member is silently NOT priv-gated (#5222's gate
/// reopens for this ordering).
#[test]
fn fn_body_member_access_on_later_declared_structure_reports_ordering_error() {
    for (host, src) in [
        (
            "PortHost",
            r#"
module test.fwd_port
trait Iface {}
pub fn leak(m : PortHost) -> Length { m.secret.main }
structure def PortHost {
    port secret : Iface {
        priv param main : Length = 5mm
    }
}
"#,
        ),
        (
            "GuardHost",
            r#"
module test.fwd_guard
pub fn leak(m : GuardHost) -> Length { m.g }
structure def GuardHost {
    param active : Bool = true
    where active {
        priv param g : Length = 5mm
    }
}
"#,
        ),
    ] {
        let module = reify_test_support::helpers::compile_source_with_stdlib(src);
        let ordering: Vec<_> = module
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Severity::Error
                    && d.code == Some(DiagnosticCode::StructureDeclaredAfterFunction)
            })
            .collect();
        assert!(
            !ordering.is_empty(),
            "{host}: member access on a later-declared same-module structure must \
             draw the ordering Error, never the permissive fallback; all \
             diagnostics: {:#?}",
            diag_lines(&module)
        );
        assert!(
            ordering.iter().any(|d| d.message.contains(host)),
            "{host}: the ordering error must name the structure; got: {:#?}",
            diag_lines(&module)
        );
    }
}

/// Where a same-module `occurrence def` sits relative to the fn that calls it.
enum DefPlacement {
    AboveFn,
    BelowFn,
}

/// What a fn-body call is meant to reach.
enum CalleeKind {
    /// An `occurrence def` in the module under test, placed as given.
    SameModuleOccurrence(DefPlacement),
    /// An `occurrence def` in the stdlib prelude.
    PreludeOccurrence,
    /// A `structure def` in the stdlib prelude.
    PreludeStructure,
}

/// The callee a fn-body call is meant to reach: its `name` and its `kind`.
///
/// An unknown name draws no ordering diagnostic either (only an
/// `UnresolvedFunction` Warning, lowered like an occurrence call), so the absence
/// check cannot tell a deleted callee from a surviving one. The fixture's call is
/// therefore built from `name`, and the test checks that the call resolves (no
/// `UnresolvedFunction`) as well as that the callee is still the kind `kind` names.
struct Callee {
    name: &'static str,
    kind: CalleeKind,
}

impl Callee {
    /// The source of module `t.<module_name>`: `pub fn f` binds `let x` to a call of
    /// this callee, and a same-module occurrence is declared where its placement says.
    fn fixture(&self, module_name: &str) -> String {
        let name = self.name;
        let call_fn = format!("pub fn f() -> Real {{ let x = {name}(); 1.0 }}\n");
        let body = match &self.kind {
            CalleeKind::SameModuleOccurrence(placement) => {
                let def = format!("occurrence def {name} {{ param p : Real = 0 }}\n");
                match placement {
                    DefPlacement::AboveFn => format!("{def}{call_fn}"),
                    DefPlacement::BelowFn => format!("{call_fn}{def}"),
                }
            }
            CalleeKind::PreludeOccurrence | CalleeKind::PreludeStructure => call_fn,
        };
        format!("module t.{module_name}\n{body}")
    }

    /// Panics unless the callee is still the kind `self.kind` names. `call` is the
    /// compiled call expression; `what` labels the failure.
    fn assert_still_its_kind(
        &self,
        module: &reify_compiler::CompiledModule,
        call: &CompiledExpr,
        what: &str,
    ) {
        let name = self.name;
        match self.kind {
            CalleeKind::SameModuleOccurrence(_) => assert_eq!(
                reify_compiler::find_template(&module.templates, name).map(|t| t.entity_kind),
                Some(reify_compiler::EntityKind::Occurrence),
                "{what}: `{name}` must be an occurrence def of this module; \
                 all diagnostics: {:#?}",
                diag_lines(module)
            ),
            CalleeKind::PreludeOccurrence => assert_eq!(
                reify_test_support::stdlib_prelude::template(name).entity_kind,
                reify_compiler::EntityKind::Occurrence,
                "{what}: prelude `{name}` must be an occurrence def; all diagnostics: {:#?}",
                diag_lines(module)
            ),
            CalleeKind::PreludeStructure => assert!(
                matches!(
                    &call.kind,
                    CompiledExprKind::StructureInstanceCtor { type_name, .. } if type_name == name
                ),
                "{what}: `{name}()` must lower to StructureInstanceCtor{{{name}}}; got {:?}; \
                 all diagnostics: {:#?}",
                call.kind,
                diag_lines(module)
            ),
        }
    }
}

/// The ordering rule is about module-local `structure def`s only. A same-module
/// OCCURRENCE and a prelude occurrence or structure never enter the ctor view
/// the way a local structure does, so calling one from a fn body must not draw a
/// message claiming "structure 'X' is declared after this function" — that
/// message would lie. The absence check cannot pass vacuously because each case
/// also witnesses that its fixture, built from the callee's name, compiles
/// without Errors and its call resolves, that `f`'s `let x` was lowered, and that
/// the callee is still the occurrence or structure its kind names.
#[test]
fn fn_body_call_of_non_structure_def_name_draws_no_ordering_error() {
    for (what, module_name, callee) in [
        (
            "same-module occurrence declared below the fn",
            "occ_below",
            Callee {
                name: "Op",
                kind: CalleeKind::SameModuleOccurrence(DefPlacement::BelowFn),
            },
        ),
        (
            "same-module occurrence declared above the fn",
            "occ_above",
            Callee {
                name: "Op",
                kind: CalleeKind::SameModuleOccurrence(DefPlacement::AboveFn),
            },
        ),
        (
            "prelude occurrence",
            "prelude_occ",
            Callee {
                name: "STLOutput",
                kind: CalleeKind::PreludeOccurrence,
            },
        ),
        (
            "prelude structure",
            "prelude_struct",
            Callee {
                name: "Color",
                kind: CalleeKind::PreludeStructure,
            },
        ),
    ] {
        let src = callee.fixture(module_name);
        let module = reify_test_support::helpers::compile_source_with_stdlib(&src);
        assert!(
            ordering_diags(&module).is_empty(),
            "{what}: must draw no declaration-order diagnostic; all diagnostics: {:#?}",
            diag_lines(&module)
        );
        assert!(
            reify_test_support::helpers::errors_only(&module).is_empty(),
            "{what}: a new Error means the fixture no longer compiles to a clean call, so \
             the absence check above proves nothing; all diagnostics: {:#?}",
            diag_lines(&module)
        );
        assert!(
            reify_test_support::helpers::diags_with_code(
                &module.diagnostics,
                DiagnosticCode::UnresolvedFunction
            )
            .is_empty(),
            "{what}: the call `{name}()` must resolve to its callee. An UnresolvedFunction \
             Warning means it reaches nothing, so this case no longer exercises the path; \
             all diagnostics: {:#?}",
            diag_lines(&module),
            name = callee.name
        );
        let call = fn_body_let(&module, "f", "x");
        callee.assert_still_its_kind(&module, call, what);
    }
}

// ─── purpose-nested structures obey the same declared-above rule ──────────────

/// A `structure def` nested in a `purpose` reaches a fn body's ctor view through
/// the same authoritative templates as a top-level one, so it obeys the same
/// declared-above rule. Declared BELOW the fn it must draw the ordering Error,
/// never a silent `Undef`.
#[test]
fn fn_body_ctor_of_later_declared_purpose_nested_structure_reports_ordering_error() {
    let src = r#"
module test.fwd_purpose
pub fn mk() -> InP { InP() }
purpose Explore() {
    structure def InP { param a : Real = 1.0 }
}
structure App { let t = mk() }
"#;
    let module = reify_test_support::helpers::compile_source_with_stdlib(src);
    let ordering: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::StructureDeclaredAfterFunction)
        })
        .collect();
    assert!(
        !ordering.is_empty(),
        "constructing a later-declared purpose-nested structure must draw the \
         ordering Error; all diagnostics: {:#?}",
        diag_lines(&module)
    );
    assert!(
        ordering.iter().any(|d| d.message.contains("InP")),
        "the ordering error must name the structure; got: {:#?}",
        diag_lines(&module)
    );
}

/// Member access on a param typed as a purpose-nested structure declared BELOW
/// the fn has no template to priv-gate against, exactly like the top-level case
/// in `fn_body_member_access_on_later_declared_structure_reports_ordering_error`.
#[test]
fn fn_body_priv_member_access_on_later_declared_purpose_nested_structure_reports_ordering_error() {
    let src = r#"
module test.fwd_purpose_guard
pub fn leak(m : InP) -> Length { m.g }
purpose Explore() {
    structure def InP {
        param active : Bool = true
        where active {
            priv param g : Length = 5mm
        }
    }
}
"#;
    let module = reify_test_support::helpers::compile_source_with_stdlib(src);
    let ordering: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::StructureDeclaredAfterFunction)
        })
        .collect();
    assert!(
        !ordering.is_empty(),
        "member access on a later-declared purpose-nested structure must draw \
         the ordering Error, never the permissive fallback; all diagnostics: {:#?}",
        diag_lines(&module)
    );
    assert!(
        ordering.iter().any(|d| d.message.contains("InP")),
        "the ordering error must name the structure; got: {:#?}",
        diag_lines(&module)
    );
}

/// Guard: a purpose-nested structure declared ABOVE the fn is constructible from
/// its body and evaluates to the structure's real defaults.
#[test]
fn fn_body_ctor_of_earlier_declared_purpose_nested_structure_evaluates() {
    let src = r#"
module test.purpose_above
purpose Explore() {
    structure def InP { param a : Real = 1.0 }
}
pub fn mk() -> InP { InP() }
structure App { let t = mk() }
"#;
    let module = reify_test_support::helpers::compile_source_with_stdlib(src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "a purpose-nested structure declared above the fn must compile clean; \
         all diagnostics: {:#?}",
        diag_lines(&module)
    );
    assert!(
        ordering_diags(&module).is_empty(),
        "no declaration-order diagnostic for a structure declared above; got: {:#?}",
        diag_lines(&module)
    );

    let result = eval_source_with_stdlib(src);
    reify_test_support::assert_no_eval_errors(&result);
    let t = reify_test_support::cell_value(&result, "App", "t");
    let data = match &t {
        Value::StructureInstance(data) => data,
        other => panic!("App.t should be Value::StructureInstance; got: {other:?}"),
    };
    assert_eq!(
        data.type_name, "InP",
        "App.t type_name; got: {}",
        data.type_name
    );
    assert_eq!(
        data.fields.get(&"a".to_string()),
        Some(&Value::Real(1.0)),
        "App.t.a should be InP's default Real(1.0); got fields: {:?}",
        data.fields
    );
}

/// The `fn` named `name` in `module`, panicking with every fn name when absent.
fn function_named<'m>(
    module: &'m reify_compiler::CompiledModule,
    name: &str,
) -> &'m CompiledFunction {
    module
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| {
            panic!(
                "fn `{name}` missing from module.functions; present: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        })
}

/// The compiled expression `fn_name`'s body binds to `let let_name`, panicking
/// with the lets that ARE present (and the module's diagnostics) when absent.
fn fn_body_let<'m>(
    module: &'m reify_compiler::CompiledModule,
    fn_name: &str,
    let_name: &str,
) -> &'m CompiledExpr {
    let lets = &function_named(module, fn_name).body.let_bindings;
    lets.iter()
        .find(|(name, _)| name == let_name)
        .map(|(_, expr)| expr)
        .unwrap_or_else(|| {
            panic!(
                "`let {let_name}` missing from `{fn_name}`'s body; present: {:?}; \
                 all diagnostics: {:#?}",
                lets.iter().map(|(name, _)| name).collect::<Vec<_>>(),
                diag_lines(module)
            )
        })
}

fn assert_body_constructs(module: &reify_compiler::CompiledModule, fn_name: &str, ty: &str) {
    let f = function_named(module, fn_name);
    match &f.body.result_expr.kind {
        CompiledExprKind::StructureInstanceCtor { type_name, .. } => assert_eq!(
            type_name, ty,
            "`{fn_name}` must construct `{ty}`; got StructureInstanceCtor{{{type_name}}}"
        ),
        other => panic!(
            "`{fn_name}` declared below `{ty}` must lower to \
             StructureInstanceCtor{{{ty}}}; got {other:?}; diagnostics: {:#?}",
            diag_lines(module)
        ),
    }
}

fn assert_body_does_not_construct(module: &reify_compiler::CompiledModule, fn_name: &str) {
    if let CompiledExprKind::StructureInstanceCtor { type_name, .. } =
        &function_named(module, fn_name).body.result_expr.kind
    {
        panic!(
            "`{fn_name}` must not lower to StructureInstanceCtor{{{type_name}}} — \
             `{type_name}`'s template does not exist yet at this point in the source"
        );
    }
}

/// Each fn body sees exactly the same-module structure defs declared ABOVE it,
/// across several fns and structures interleaved in one module: the set of
/// structures a fn may not use shrinks as each structure's template is built,
/// never sooner and never later. A pending param default (one constructing a
/// same-module structure) compiles in its fn's slot under the same rule.
#[test]
fn each_fn_body_sees_exactly_the_structure_defs_declared_above_it() {
    let src = r#"
module test.interleaved
pub fn before_alpha() -> Alpha { Alpha() }
structure def Alpha { param a : Real = 1.0 }
pub fn after_alpha() -> Alpha { Alpha() }
pub fn defaulted_after_alpha(o : Alpha = Alpha()) -> Real { o.a }
pub fn before_beta() -> Beta { Beta() }
structure def Beta { param b : Real = 2.0 }
pub fn after_beta() -> Beta { Beta() }
"#;
    let module = reify_test_support::helpers::compile_source_with_stdlib(src);

    let ordering = ordering_diags(&module);
    assert_eq!(
        ordering.len(),
        2,
        "exactly `before_alpha` and `before_beta` construct a structure declared \
         below them; all diagnostics: {:#?}",
        diag_lines(&module)
    );
    for structure in ["Alpha", "Beta"] {
        let needle = format!("structure '{structure}'");
        assert_eq!(
            ordering
                .iter()
                .filter(|d| d.message.contains(&needle))
                .count(),
            1,
            "exactly one ordering diagnostic must name `{structure}`; all diagnostics: {:#?}",
            diag_lines(&module)
        );
    }
    let error_codes: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code)
        .collect();
    assert_eq!(
        error_codes,
        [Some(DiagnosticCode::StructureDeclaredAfterFunction); 2],
        "the two ordering diagnostics must be the module's only Errors; \
         all diagnostics: {:#?}",
        diag_lines(&module)
    );

    match &function_named(&module, "defaulted_after_alpha").param_defaults[..] {
        [
            Some(CompiledExpr {
                kind: CompiledExprKind::StructureInstanceCtor { type_name, .. },
                ..
            }),
        ] => assert_eq!(type_name, "Alpha"),
        other => panic!(
            "`defaulted_after_alpha`'s pending default must compile in its fn's slot \
             to StructureInstanceCtor{{Alpha}}; got {other:?}"
        ),
    }

    assert_body_constructs(&module, "after_alpha", "Alpha");
    assert_body_constructs(&module, "after_beta", "Beta");
    assert_body_does_not_construct(&module, "before_alpha");
    assert_body_does_not_construct(&module, "before_beta");
}

/// The declared-above rule per fn holds for a purpose-nested structure def too:
/// the fn above it draws the ordering diagnostic, the fn below constructs it.
#[test]
fn fn_bodies_on_both_sides_of_a_purpose_nested_structure_def_see_it_only_from_below() {
    let src = r#"
module test.interleaved_purpose
pub fn before_inp() -> InP { InP() }
purpose Explore() {
    structure def InP { param a : Real = 1.0 }
}
pub fn after_inp() -> InP { InP() }
"#;
    let module = reify_test_support::helpers::compile_source_with_stdlib(src);

    let ordering = ordering_diags(&module);
    assert_eq!(
        ordering.len(),
        1,
        "only `before_inp` constructs `InP` from above its declaration; \
         all diagnostics: {:#?}",
        diag_lines(&module)
    );
    assert!(
        ordering[0].message.contains("structure 'InP'"),
        "the ordering diagnostic must name `InP`; all diagnostics: {:#?}",
        diag_lines(&module)
    );

    assert_body_constructs(&module, "after_inp", "InP");
    assert_body_does_not_construct(&module, "before_inp");
}
