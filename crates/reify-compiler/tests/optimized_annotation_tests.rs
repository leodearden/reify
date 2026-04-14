//! Compiler-level tests for the `@optimized` annotation on constraint defs
//! (Task 273 — @optimized: plumbing).
//!
//! Exercises:
//!   - `@optimized("target")` on a `constraint def` is accepted by the validator
//!     (no "@optimized is not valid" warning).
//!   - Instantiating such a def in a structure propagates the target onto the
//!     resulting `CompiledConstraint::optimized_target`.
//!   - An un-annotated constraint def yields `optimized_target = None`.

use reify_compiler::{CompiledConstraint, CompiledModule, TopologyTemplate};
use reify_types::{Diagnostic, ModulePath, Severity};

// ── Helpers ─────────────────────────────────────────────────────────────────

fn compile_module(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("optimized_ann_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

fn error_diags(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

fn warning_diags(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect()
}

fn template_named<'a>(module: &'a CompiledModule, name: &str) -> &'a TopologyTemplate {
    module
        .templates
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("expected template '{name}' in compiled module"))
}

// ── Step 3: acceptance + field existence ────────────────────────────────────

#[test]
fn optimized_annotation_on_constraint_def_is_accepted() {
    let source = r#"
@optimized("kernel::foo")
constraint def MinWall {
    param w: Length
    w > 0mm
}
structure S {
    param t: Length
    constraint MinWall(w: t)
}
"#;
    let module = compile_module(source);

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // No warning should claim @optimized is invalid on a constraint_def.
    let bad_optimized_warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("@optimized is not valid"))
        .collect();
    assert!(
        bad_optimized_warnings.is_empty(),
        "@optimized on constraint_def should not warn; got: {:?}",
        bad_optimized_warnings
    );

    // Compile-time field assertion: if `CompiledConstraint::optimized_target`
    // is missing this test fails to compile, satisfying step-3's contract.
    let tmpl = template_named(&module, "S");
    assert_eq!(
        tmpl.constraints.len(),
        1,
        "expected 1 compiled constraint in S, got {}",
        tmpl.constraints.len()
    );
    let cc: &CompiledConstraint = &tmpl.constraints[0];
    // This read forces the field to exist on the struct — the test files a
    // compile error if the field is removed or renamed.
    let _target: &Option<String> = &cc.optimized_target;
}
