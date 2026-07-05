//! γ (task #4954): top-level geometry `let`s emit a first-class
//! `Type::Geometry` value cell ALONGSIDE their existing `RealizationDecl`
//! (graph-completion lowering, PRD
//! docs/prds/v0_6/eval-uniform-dependency-handling.md task γ).
//!
//! Root cause this closes: pre-γ, a geometry let lowered to a
//! `RealizationDecl` with NO backing value cell (entity.rs `MemberDecl::Let`
//! arm: `if is_geometry_let(...) { continue; }`). With no node in
//! `template.value_cells`, `build_combined_param_let_graph` never saw the
//! geometry let, so Kahn in-degree counting silently dropped every
//! consumer→geometry-let edge and consumers could schedule before the mint.
//!
//! This mirrors the shipping solid-typed-geometry-param pair-shape (see
//! `solid_param_tests.rs`): a `ValueCellDecl{cell_type: Type::Geometry}` AND
//! a `RealizationDecl` both exist for the same declaration.

use reify_compiler::{ValueCellKind, Visibility};
use reify_core::{Diagnostic, Severity, Type, ValueCellId};
use reify_test_support::compile_source_with_stdlib;

fn errors(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Boundary row 1 (compiler half) + row 2 premise (true dependency edge).
///
/// A top-level, non-guarded geometry let (`loc_box`) and a boolean-op
/// geometry let that consumes it (`combined`) must EACH produce:
/// (a) exactly one `ValueCellDecl` with `kind == Let`, `cell_type ==
///     Type::Geometry`, `default_expr.is_some()`, `visibility ==
///     Visibility::Private`;
/// (b) a named `RealizationDecl` still present in `template.realizations`
///     (the existing realization-emission loop is unchanged);
/// (c) no Error-severity diagnostics.
///
/// `combined`'s `default_expr` must ALSO carry a true `ValueRef` dependency
/// edge on `loc_box` — the edge `build_combined_param_let_graph` was silently
/// dropping pre-γ because `loc_box` had no node to point to.
///
/// RED today: entity.rs:2011-2013's `is_geometry_let ⇒ continue` drops the
/// value cell for both `loc_box` and `combined` entirely.
#[test]
fn top_level_geometry_lets_emit_value_cells_and_realizations() {
    let source = r#"structure S {
    param width : Length = 10mm
    param height : Length = 10mm
    param depth : Length = 10mm
    let loc_box = box(width, height, depth)
    let combined = union(loc_box, box(width, height, depth))
}"#;
    let module = compile_source_with_stdlib(source);
    let errs = errors(&module);
    assert!(
        errs.is_empty(),
        "expected no error diagnostics, got: {:#?}",
        errs
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");

    for name in ["loc_box", "combined"] {
        let cells: Vec<_> = template
            .value_cells
            .iter()
            .filter(|c| c.id.member == name)
            .collect();
        assert_eq!(
            cells.len(),
            1,
            "expected exactly 1 ValueCellDecl for '{name}'; got: {:#?}",
            cells
        );
        let cell = cells[0];
        assert_eq!(
            cell.kind,
            ValueCellKind::Let,
            "expected kind=Let for '{name}', got {:?}",
            cell.kind
        );
        assert_eq!(
            cell.cell_type,
            Type::Geometry,
            "expected cell_type=Type::Geometry for '{name}', got {:?}",
            cell.cell_type
        );
        assert!(
            cell.default_expr.is_some(),
            "expected default_expr.is_some() for '{name}'"
        );
        assert_eq!(
            cell.visibility,
            Visibility::Private,
            "expected Visibility::Private for '{name}' (internal-only design, \
             no dot-access/export change), got {:?}",
            cell.visibility
        );

        assert!(
            template
                .realizations
                .iter()
                .any(|r| r.name.as_deref() == Some(name)),
            "expected a named RealizationDecl for '{name}' to still exist; got: {:#?}",
            template.realizations
        );
    }

    // `combined`'s default_expr must read `loc_box` as a true dependency
    // edge — this is the edge the ordering graph was silently dropping.
    let combined_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "combined")
        .expect("combined ValueCellDecl not found");
    let reads = combined_cell
        .default_expr
        .as_ref()
        .expect("combined default_expr.is_some()")
        .collect_value_refs();
    assert!(
        reads.contains(&ValueCellId::new("S", "loc_box")),
        "expected combined's default_expr to read S.loc_box; got reads: {:?}",
        reads
    );
}
