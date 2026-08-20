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

mod common;

use reify_ir::CompiledExprKind;

const SRC: &str = r#"
structure def Widget {
    let b = box(1m, 1m, 1m)
    let p = PressureLoad(face: faces(b), magnitude: 1.0e6)
    let loads = [PressureLoad(face: faces(b))]
}
"#;

/// Return the `face` field expr of a `PressureLoad` `StructureInstanceCtor`
/// (searching both `ordered_args` and `defaults`).
fn pressure_face_arg(expr: &reify_ir::CompiledExpr) -> &reify_ir::CompiledExpr {
    match &expr.kind {
        CompiledExprKind::StructureInstanceCtor {
            type_name,
            ordered_args,
            defaults,
            ..
        } => {
            assert_eq!(type_name, "PressureLoad", "expected a PressureLoad ctor");
            ordered_args
                .iter()
                .chain(defaults.iter())
                .find(|(n, _)| n == "face")
                .map(|(_, e)| e)
                .expect("PressureLoad ctor must carry a `face` field")
        }
        other => panic!("expected PressureLoad StructureInstanceCtor, got {:?}", other),
    }
}

#[test]
fn hoist_mints_synthetic_sel_cells_and_rewrites_fields() {
    let module = common::compile_with_stdlib_helper(SRC);
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
    let p_face = pressure_face_arg(p.default_expr.as_ref().expect("p default_expr"));
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
    let loads_face = pressure_face_arg(elem);
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
