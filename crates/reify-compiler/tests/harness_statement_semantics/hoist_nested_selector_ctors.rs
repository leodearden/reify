//! Task 4370 AXIS-1 step-3 (RED): the compiler must hoist nested selector-ctor
//! `FunctionCall`s out of `StructureInstanceCtor` fields into synthetic
//! top-level `__sel*` `Let` value cells in the enclosing template, rewriting the
//! field to a `ValueRef(__sel*)`.
//!
//! Uses the already-supported `faces` leaf ctor (kernel-free, arity-1) to
//! isolate the hoist mechanism from the named-leaf symbolic stand-in (steps
//! 1-2). Two shapes are covered:
//!   * direct field — `let p = PressureLoad(face: faces(b), magnitude: 1e6)`
//!   * list-nested   — `let loads = [PressureLoad(face: faces(b))]`
//!
//! RED on main: no hoist phase exists, so the nested `faces(b)` `FunctionCall`
//! stays inline in the `PressureLoad` ctor field and no `__sel*` cell is minted.
//! GREENed by step-6 (`phase_hoist_nested_selector_ctors`).

use reify_ir::CompiledExprKind;

const SRC: &str = r#"
structure def Widget {
    let b = box(1m, 1m, 1m)
    let p = PressureLoad(face: faces(b), magnitude: 1.0e6)
    let loads = [PressureLoad(face: faces(b))]
}
"#;

/// Return the `field` expr of a `type_name` `StructureInstanceCtor`
/// (searching both `ordered_args` and `defaults`).
fn ctor_field_arg<'e>(
    expr: &'e reify_ir::CompiledExpr,
    expected_type: &str,
    field: &str,
) -> &'e reify_ir::CompiledExpr {
    match &expr.kind {
        CompiledExprKind::StructureInstanceCtor {
            type_name,
            ordered_args,
            defaults,
            ..
        } => {
            assert_eq!(type_name, expected_type, "expected a {expected_type} ctor");
            ordered_args
                .iter()
                .chain(defaults.iter())
                .find(|(n, _)| n == field)
                .map(|(_, e)| e)
                .unwrap_or_else(|| panic!("{expected_type} ctor must carry a `{field}` field"))
        }
        other => panic!("expected {expected_type} StructureInstanceCtor, got {other:?}"),
    }
}

#[test]
fn hoist_mints_synthetic_sel_cells_and_rewrites_fields() {
    let module = crate::common::compile_with_stdlib_helper(SRC);
    let widget = module
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("Widget template must be present");

    // (1) The hoist minted ≥1 synthetic `__sel*` Let cell, each carrying a
    //     `faces(...)` FunctionCall verbatim as its default_expr.
    let sel_cells: Vec<_> = widget
        .value_cells
        .iter()
        .filter(|c| c.id.member.starts_with("__sel"))
        .collect();
    assert!(
        !sel_cells.is_empty(),
        "expected ≥1 synthetic __sel* value cell; got cells: {:?}",
        widget
            .value_cells
            .iter()
            .map(|c| c.id.member.as_str())
            .collect::<Vec<_>>()
    );
    for c in &sel_cells {
        assert_eq!(
            c.kind,
            reify_compiler::ValueCellKind::Let,
            "__sel cell {} must be a Let",
            c.id.member
        );
        match c.default_expr.as_ref().map(|e| &e.kind) {
            Some(CompiledExprKind::FunctionCall { function, .. }) => assert_eq!(
                function.name, "faces",
                "__sel default_expr must be the hoisted faces(b) ctor"
            ),
            other => panic!(
                "__sel {} default_expr must be a faces FunctionCall, got {:?}",
                c.id.member, other
            ),
        }
    }

    // (2) Direct-field case: the `p` cell's PressureLoad `face` field is now a
    //     ValueRef to a `__sel*` cell (was the inline `faces(b)` FunctionCall).
    let p = widget
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p cell");
    let p_face = ctor_field_arg(
        p.default_expr.as_ref().expect("p default_expr"),
        "PressureLoad",
        "face",
    );
    match &p_face.kind {
        CompiledExprKind::ValueRef(id) => assert!(
            id.member.starts_with("__sel"),
            "p.face ValueRef must target a __sel cell, got {}",
            id.member
        ),
        other => panic!(
            "PressureLoad `face` field must be a ValueRef(__sel*), got {:?}",
            other
        ),
    }

    // (3) List-nested case: `loads = [PressureLoad(face: faces(b))]` — the walk
    //     must descend into the ListLiteral element and hoist its face too.
    let loads = widget
        .value_cells
        .iter()
        .find(|c| c.id.member == "loads")
        .expect("loads cell");
    let elem = match &loads.default_expr.as_ref().expect("loads default_expr").kind {
        CompiledExprKind::ListLiteral(elems) => &elems[0],
        other => panic!("loads default_expr must be a ListLiteral, got {:?}", other),
    };
    let loads_face = ctor_field_arg(elem, "PressureLoad", "face");
    match &loads_face.kind {
        CompiledExprKind::ValueRef(id) => assert!(
            id.member.starts_with("__sel"),
            "list-nested face ValueRef must target a __sel cell, got {}",
            id.member
        ),
        other => panic!(
            "list-nested PressureLoad `face` field must be a ValueRef(__sel*), got {:?}",
            other
        ),
    }
}

// ── task 10352: a callee's captured default is not lifted into the caller ────

const CALLEE_DEFAULT_SRC: &str = r#"
structure def Beam {
    param load : Load = PointLoad(point: vertex(body, "tip"))
    let body = box(1000mm, 100mm, 100mm)
}

structure def NestedAssembly {
    let primary = Beam()
}
"#;

fn template<'m>(
    module: &'m reify_compiler::CompiledModule,
    name: &str,
) -> &'m reify_compiler::TopologyTemplate {
    module
        .templates
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("{name} template must be present"))
}

fn default_of<'m>(
    template: &'m reify_compiler::TopologyTemplate,
    member: &str,
) -> &'m reify_ir::CompiledExpr {
    template
        .value_cells
        .iter()
        .find(|c| c.id.member == member)
        .and_then(|c| c.default_expr.as_ref())
        .unwrap_or_else(|| panic!("{}.{member} must carry a default", template.name))
}

fn sel_cells(template: &reify_compiler::TopologyTemplate) -> Vec<&str> {
    template
        .value_cells
        .iter()
        .map(|c| c.id.member.as_str())
        .filter(|m| m.starts_with("__sel"))
        .collect()
}

/// A let-constructed `Beam()` evaluates its captured `load` default in the
/// instance's member scope, where a caller-template `__sel_N` is invisible, so
/// the hoist must leave that default's selector inline. Beam's own template
/// still hoists it.
#[test]
fn hoist_leaves_a_callee_captured_default_inline() {
    let module = crate::common::compile_with_stdlib_helper(CALLEE_DEFAULT_SRC);
    let nested = template(&module, "NestedAssembly");
    assert!(
        sel_cells(nested).is_empty(),
        "the caller template must mint no __sel cell for a callee default; got {:?}",
        sel_cells(nested)
    );

    let primary_load = match &default_of(nested, "primary").kind {
        CompiledExprKind::StructureInstanceCtor { defaults, .. } => defaults
            .iter()
            .find(|(n, _)| n == "load")
            .map(|(_, e)| e)
            .expect("primary's Beam ctor must capture the `load` default"),
        other => panic!("primary must lower to a StructureInstanceCtor, got {other:?}"),
    };
    match &ctor_field_arg(primary_load, "PointLoad", "point").kind {
        CompiledExprKind::FunctionCall { function, .. } => assert_eq!(function.name, "vertex"),
        other => {
            panic!("the captured default's `point` must stay the inline vertex call, got {other:?}")
        }
    }

    let beam = template(&module, "Beam");
    assert!(
        !sel_cells(beam).is_empty(),
        "Beam's own template must still hoist"
    );
    match &ctor_field_arg(default_of(beam, "load"), "PointLoad", "point").kind {
        CompiledExprKind::ValueRef(id) => assert!(
            id.member.starts_with("__sel"),
            "Beam.load's point must reference a __sel cell, got {id}"
        ),
        other => panic!("Beam.load's point must be a ValueRef(__sel*), got {other:?}"),
    }
}
