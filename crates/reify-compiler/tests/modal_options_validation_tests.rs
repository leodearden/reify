//! Tests for `crates/reify-compiler/stdlib/modal_analysis.ri` —
//! `std.modal.analysis` module: `DampingDescriptor`, `NoDamping`,
//! `RayleighDamping`, `Mode`, `ModalResult`, and `ModalOptions` structure
//! definitions for the v0.3 modal-analysis kernel surface.
//!
//! Observable signal for PRD §10 task α
//! (docs/prds/v0_3/modal-analysis.md). Per the PRD, this file parses
//! the structure_defs and confirms type resolution matches the expected
//! shape.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path
//! (mirroring `buckling_stdlib_compile.rs`), that the six structures and one
//! trait are correctly represented in the compiled module, and that the
//! positivity constraints on `ModalOptions.{n_modes, tol, max_iters}` are
//! declared at the structure-def level.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production. This mirrors the helper trio in `buckling_stdlib_compile.rs`.

use reify_compiler::*;
use reify_types::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/modal/analysis` CompiledModule from the production stdlib
/// loader. Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — which is the expected failure mode
/// until step-2 lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/modal/analysis")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/modal/analysis module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up a structure template by name within the `std/modal/analysis` module.
///
/// `Mode`, `ModalResult`, `ModalOptions`, `NoDamping`, and `RayleighDamping`
/// are top-level structures, so we go through `module.templates` and filter on
/// `EntityKind::Structure` to keep the assertion stable against future
/// non-structure additions to the module.
#[allow(dead_code)]
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/modal/analysis, got templates: {:?}",
                name,
                module
                    .templates
                    .iter()
                    .map(|t| (&t.name, &t.entity_kind))
                    .collect::<Vec<_>>()
            )
        })
}

/// Collect the param-kind value cells (ignoring `let` and auto cells) from a
/// template, returning them in the file order they were declared.
#[allow(dead_code)]
fn param_cells(template: &TopologyTemplate) -> Vec<&ValueCellDecl> {
    template
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, ValueCellKind::Param))
        .collect()
}

/// Look up the named param cell on `template` and return its `default_expr`.
/// Panics with a clear message if the cell or its default is missing.
#[allow(dead_code)]
fn require_default<'a>(template: &'a TopologyTemplate, member: &str) -> &'a CompiledExpr {
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("{}.{} missing", template.name, member));
    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("{}.{} missing default_expr", template.name, member))
}

/// Recursively collect ValueRef member names from a compiled expression tree.
/// Mirrors `collect_value_ref_members` in `buckling_stdlib_compile.rs:98-108`.
#[allow(dead_code)]
fn collect_value_ref_members(expr: &CompiledExpr) -> Vec<&str> {
    match &expr.kind {
        CompiledExprKind::ValueRef(cell_id) => vec![cell_id.member.as_str()],
        CompiledExprKind::BinOp { left, right, .. } => {
            let mut refs = collect_value_ref_members(left);
            refs.extend(collect_value_ref_members(right));
            refs
        }
        CompiledExprKind::UnOp { operand, .. } => collect_value_ref_members(operand),
        _ => vec![],
    }
}

// ─── step-1: module loads with zero error diagnostics ────────────────────────

/// The std/modal/analysis module must load through the production stdlib path
/// with zero error-severity diagnostics. The loader-level `assert!` already
/// fails fast on Error diagnostics during init, but this test independently
/// asserts the post-init invariant so a regression is caught at the test
/// boundary rather than at first stdlib touch.
#[test]
fn std_modal_analysis_module_loads_with_no_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in modal_analysis.ri: {:?}",
        errors
    );
}
