//! Integration test: a fn body can construct a structure_def from the SAME module.
//!
//! This file is the acceptance gate for task 3895: after the same-module skeleton
//! pre-pass lands in `phase_functions`, `Widget()` inside `make_widget()` must lower
//! to `CompiledExprKind::StructureInstanceCtor` (not a generic `UserFunctionCall`),
//! and eval must produce a `Value::StructureInstance` with the structure_def defaults.
//!
//! Today (step-1, RED) this fails: `Widget()` in a same-module fn body is not in the
//! prelude_template_registry, so it lowers to a `UserFunctionCall` with no matching
//! overload → poison diagnostic + error.  Step-2 (GREEN) adds the skeleton pre-pass.

use reify_ir::*;
use reify_core::*;

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

// ─── helper ───────────────────────────────────────────────────────────────────

fn load_widget_module() -> reify_compiler::CompiledModule {
    reify_test_support::helpers::compile_source_with_stdlib(WIDGET_SRC)
}

// ─── step-1: zero errors ──────────────────────────────────────────────────────

/// The module must compile with zero `Severity::Error` diagnostics once the
/// same-module skeleton pre-pass (step-2) is in place.  RED today: `Widget()` in
/// a same-module fn body is not in the prelude_template_registry, so it lowers
/// to a `UserFunctionCall` with no matching overload → error diagnostic.
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

// ─── step-2: make_widget body is StructureInstanceCtor ────────────────────────

/// `make_widget()`'s result_expr must lower to
/// `CompiledExprKind::StructureInstanceCtor` with `type_name == "Widget"`.
/// RED today: `Widget()` lowers to `UserFunctionCall` instead.
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

// ─── step-3: eval make_widget() → Value::StructureInstance with defaults ──────

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

    assert_eq!(
        data.fields.len(),
        2,
        "make_widget() StructureInstance.fields should have 2 entries; got: {:?}",
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

// ─── step-4: eval make_widget_partial() → covered positional arg + default ───

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

// ─── step-5: accepted limitation — sibling-param default is pinned ────────────

/// Accepted limitation (task 3895 design decision #3): a structure-def param
/// whose default expression references a sibling param will not resolve in the
/// neutral skeleton scope used by `build_structure_def_skeleton`.
///
/// **Current behavior (pinned by this test):**
/// - The module compiles with **zero `Severity::Error` diagnostics.**
///   `compile_expr` fails to resolve `y = x + 1.0` in the neutral scope; the
///   diagnostic is routed to the throwaway buffer and discarded.
///   `phase_entities` re-compiles `Point` authoritatively (siblings resolve
///   there) and emits no Error.
/// - `make_point()`'s fn body **still lowers to `StructureInstanceCtor`.**
///   The skeleton successfully identifies `Point` as a structure; only `y`'s
///   default expr is a poison value in the skeleton.
/// - Eval is intentionally **not** asserted: `y`'s skeleton-baked poison
///   default produces implementation-defined behavior at eval time.  The
///   authoritative `Point` entity template (from `phase_entities`) has the
///   correct default `x + 1.0` for direct entity instantiation.
///
/// If a future task implements sibling-param resolution in skeleton scope, this
/// test must be updated to assert the correct eval result for `y`.
#[test]
fn sibling_param_default_accepted_limitation_body_is_ctor() {
    let src = r#"
module test.sibling_default
structure def Point { param x : Real = 1.0  param y : Real = x + 1.0 }
pub fn make_point() -> Point { Point() }
"#;
    let module = reify_test_support::helpers::compile_source_with_stdlib(src);

    // Zero errors: skeleton's poisoned y-default diagnostic is discarded;
    // phase_entities re-compiles authoritatively with no error.
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

    // The fn body IS a StructureInstanceCtor: the skeleton correctly identifies
    // Point as a structure even though y's default is poisoned.
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
            "make_point body should be StructureInstanceCtor (sibling-default \
             skeleton still lowers the ctor); got: {:?}",
            other
        ),
    }
}

// ─── step-6: alias-registry dedup isolation guard ────────────────────────────

/// Regression guard: the skeleton pass must not emit spurious
/// `Severity::Info` diagnostics about parametric prelude aliases.
///
/// `build_structure_def_skeleton` clones the caller's `TypeAliasRegistry`
/// before resolving param types (task 3895 bugfix).  Without the clone, the
/// skeleton's neutral-scope type resolution would record source spans in the
/// original registry's `emitted_skipped_parametric_prelude_spans` dedup set,
/// silently suppressing the authoritative `Info` that `phase_entities` should
/// later emit for parametric-alias param types.
///
/// The current stdlib has no parametric prelude aliases (all `pub type`
/// declarations in stdlib modules are non-parametric), so no Info diagnostics
/// of this kind can be triggered via integration tests today.  This test pins
/// the **absence** of spurious Info messages as a guard against false
/// positives.  A companion unit test in `entity.rs` directly verifies the
/// clone-isolation property using a synthetic registry with a mocked
/// parametric prelude name; see
/// `build_structure_def_skeleton_does_not_consume_alias_registry_dedup_slots`.
///
/// When a parametric prelude alias is added to the stdlib, this test should be
/// extended to assert that **exactly one** `Info` diagnostic matching
/// `"parametric prelude alias"` is emitted — not zero (suppressed by skeleton)
/// and not two (double-emitted).
#[test]
fn skeleton_pass_produces_no_spurious_parametric_alias_info_diagnostics() {
    let module = load_widget_module();
    let spurious: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Info && d.message.contains("parametric prelude alias")
        })
        .collect();
    assert!(
        spurious.is_empty(),
        "skeleton pass must not produce spurious parametric-alias Info diagnostics; \
         got: {:#?}",
        spurious
    );
}

// ─── task-4342 step-3b: fn-returned struct carries lets in skeleton ────────────

const TOL_SRC: &str = r#"
module test.tol

structure def Tol {
    param nominal      : Real = 0.0
    param upper_dev    : Real = 0.001
    let upper_limit    = nominal + upper_dev
}

pub fn make_tol() -> Tol { Tol(5.0, 0.02) }
"#;

/// step_3b RED: a same-module fn-returned struct with a sibling-referencing
/// derived let must produce a StructureInstanceCtor that carries `lets` AND
/// whose let expr is NOT poison (result_type != Type::Error).
///
/// RED on current base: `lets` is Vec::new() (build_structure_def_skeleton does
/// not yet attach Let cells — step_4 will fix this).
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
        CompiledExprKind::StructureInstanceCtor { type_name, lets, .. } => {
            assert_eq!(type_name, "Tol", "fn body ctor type_name must be Tol");
            // RED: currently lets is Vec::new() because step_4 skeleton doesn't attach lets.
            assert_eq!(
                lets.len(), 1,
                "fn-returned Tol ctor must carry 1 let (upper_limit); got {} lets: {:?}",
                lets.len(),
                lets.iter().map(|(n, _)| n).collect::<Vec<_>>()
            );
            assert_eq!(
                lets[0].0, "upper_limit",
                "let member name must be upper_limit"
            );
            // The let expr must not be poison — the skeleton must register sibling
            // params before compiling let exprs so the sibling refs resolve.
            assert_ne!(
                lets[0].1.result_type,
                Type::Error,
                "let expr result_type must not be Error (sibling refs must resolve in skeleton)"
            );
        }
        other => panic!("make_tol body should be StructureInstanceCtor; got: {:?}", other),
    }
}
