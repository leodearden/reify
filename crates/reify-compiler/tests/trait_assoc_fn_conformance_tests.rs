//! Integration tests for task 3939 δ (trait associated functions): the
//! producer-end signal driven through the FULL compile pipeline via
//! `reify_test_support::compile_source` (grammar + lowering landed by α/β/γ).
//!
//! Unlike the in-crate `conformance/mod.rs` unit tests — which hand-build
//! `CompiledTrait` / `StructureDef` fixtures and call `pub(crate)
//! check_trait_conformance` directly — these compile real `.ri` source, so they
//! pin the end-to-end contract that ζ (3941) will build dispatch on top of.
//!
//! Step-11 (RED): the table-population test fails until step-12 wires
//! `entity.rs` to store the resolved assoc-fn table onto each conformer's
//! `TopologyTemplate.assoc_fns` (today entity.rs discards the resolved table and
//! stores an empty `Vec` on every template).

use reify_core::DiagnosticCode;
use reify_test_support::{compile_source, errors_only};

/// (a) A trait with a bodyless required assoc fn `fn req(self) -> Real` plus a
/// structure that declares conformance but cannot provide `req` must surface
/// `E_TRAIT_FN_NOT_SATISFIED` (`DiagnosticCode::TraitFnNotSatisfied`) naming the
/// declaring trait and the missing fn — through the full pipeline.
#[test]
fn required_assoc_fn_unsatisfied_emits_diagnostic_end_to_end() {
    let source = r#"
trait Shape {
    fn req(self) -> Real
}
structure def S : Shape {
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let fn_not_satisfied: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TraitFnNotSatisfied))
        .collect();
    assert_eq!(
        fn_not_satisfied.len(),
        1,
        "expected exactly one TraitFnNotSatisfied error for the missing required \
         assoc fn 'req'; all diagnostics: {:?}",
        module.diagnostics
    );
    let msg = &fn_not_satisfied[0].message;
    assert!(
        msg.contains("Shape"),
        "diagnostic should name the declaring trait 'Shape'; got: {}",
        msg
    );
    assert!(
        msg.contains("req"),
        "diagnostic should name the missing fn 'req'; got: {}",
        msg
    );
}

/// (b) A trait with a default-providing assoc fn `fn area(self) -> Real { 1.0 }`
/// plus a conforming structure must populate the conformer's
/// `TopologyTemplate.assoc_fns` table with the injected-default `(Shape, area)`
/// entry — through the full pipeline.
///
/// RED until step-12: `entity.rs` currently discards the table resolved by
/// `check_phase_resolve_assoc_fns` and stores `assoc_fns: Vec::new()` on every
/// `TopologyTemplate`, so `template.assoc_fns` is empty here.
#[test]
fn default_assoc_fn_populates_template_table_end_to_end() {
    let source = r#"
trait Shape {
    fn area(self) -> Real { 1.0 }
}
structure def S : Shape {
}
"#;
    let module = compile_source(source);

    // A default-providing fn imposes no requirement, so conformance is clean:
    // no missing-fn or signature-mismatch diagnostics should fire.
    assert!(
        !module.diagnostics.iter().any(|d| {
            d.code == Some(DiagnosticCode::TraitFnNotSatisfied)
                || d.code == Some(DiagnosticCode::TraitFnSignatureMismatch)
        }),
        "a default-providing assoc fn should conform cleanly; got: {:?}",
        module.diagnostics
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("compiled module should contain a template for structure 'S'");

    let entry = template
        .assoc_fns
        .iter()
        .find(|f| f.trait_name == "Shape" && f.fn_name == "area")
        .unwrap_or_else(|| {
            panic!(
                "structure 'S' template should carry an assoc-fn table entry for \
                 (Shape, area); assoc_fns = {:?}",
                template.assoc_fns
            )
        });

    assert_eq!(
        entry.function.name, "area",
        "the table entry's compiled function should be named 'area'"
    );
    assert!(
        !entry.is_override,
        "the structure did not override the default, so is_override must be false; \
         got: {:?}",
        entry
    );
}
