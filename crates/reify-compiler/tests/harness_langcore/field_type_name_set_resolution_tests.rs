//! A field's domain/codomain type name resolves against the module's declared
//! structure and trait name sets, local and prelude alike: a structure name
//! becomes `Type::StructureRef`, a trait name `Type::TraitObject`, and neither
//! raises the "unresolved field type" coercion warning. A name in neither set
//! (bare `Point3`) still coerces to `StructureRef` with that warning.
//!
//! The coercion warning carries no `DiagnosticCode`, so it is located by
//! severity and span: a Warning whose label overlaps the type name's bytes.
//! Severity matters because the `FieldCodomainMismatch` error labels the same
//! codomain span. Fixture names are prefixed `Zq6501` so no stdlib entity
//! shadows them.

use reify_compiler::{CompiledField, CompiledModule};
use reify_core::{Diagnostic, Severity, Type};
use reify_test_support::{compile_source, compile_source_with_stdlib, errors_only};

const DECLS: &str = "structure def Zq6501Str { param a : Real = 1.0 }\ntrait Zq6501Tr {}\n";

/// Byte range of `name` immediately after the unique `prefix` in `source`.
fn span_after(source: &str, prefix: &str, name: &str) -> (u32, u32) {
    assert_eq!(
        source.matches(prefix).count(),
        1,
        "prefix `{prefix}` must occur exactly once in {source:?}"
    );
    let start = source.find(prefix).unwrap() + prefix.len();
    assert!(
        source[start..].starts_with(name),
        "`{name}` must follow `{prefix}` in {source:?}"
    );
    (start as u32, (start + name.len()) as u32)
}

fn coercion_warnings_at(module: &CompiledModule, (start, end): (u32, u32)) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .filter(|d| {
            d.labels
                .iter()
                .any(|l| l.span.start < end && start < l.span.end)
        })
        .collect()
}

fn field<'a>(module: &'a CompiledModule, name: &str) -> &'a CompiledField {
    module
        .fields
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("field `{name}` missing from compiled module"))
}

fn assert_no_coercion_warning(module: &CompiledModule, source: &str, prefix: &str, name: &str) {
    let warnings = coercion_warnings_at(module, span_after(source, prefix, name));
    assert!(
        warnings.is_empty(),
        "`{name}` is declared, so it must resolve without the coercion warning; got {warnings:?}"
    );
}

fn assert_no_errors(module: &CompiledModule) {
    let errors = errors_only(module);
    assert!(errors.is_empty(), "expected no errors; got {errors:?}");
}

#[test]
fn local_structure_domain_resolves_to_structure_ref() {
    let source = [
        DECLS,
        "field def zq6501_sd : Zq6501Str -> Real { source = analytical { |m| 1.0 } }",
    ]
    .concat();
    let module = compile_source(&source);
    assert_eq!(
        field(&module, "zq6501_sd").domain_type,
        Type::StructureRef("Zq6501Str".into())
    );
    assert_no_coercion_warning(&module, &source, "zq6501_sd : ", "Zq6501Str");
    assert_no_errors(&module);
}

#[test]
fn local_trait_domain_resolves_to_trait_object() {
    let source = [
        DECLS,
        "field def zq6501_td : Zq6501Tr -> Real { source = analytical { |m| 1.0 } }",
    ]
    .concat();
    let module = compile_source(&source);
    assert_eq!(
        field(&module, "zq6501_td").domain_type,
        Type::TraitObject("Zq6501Tr".into())
    );
    assert_no_coercion_warning(&module, &source, "zq6501_td : ", "Zq6501Tr");
    assert_no_errors(&module);
}

#[test]
fn local_structure_codomain_resolves_to_structure_ref() {
    let source = [
        DECLS,
        r#"field def zq6501_sc : Real -> Zq6501Str { source = imported { path = "x.vdb" format = OpenVDB grid = "density" } }"#,
    ]
    .concat();
    let module = compile_source(&source);
    assert_eq!(
        field(&module, "zq6501_sc").codomain_type,
        Type::StructureRef("Zq6501Str".into())
    );
    assert_no_coercion_warning(&module, &source, "zq6501_sc : Real -> ", "Zq6501Str");
    assert_no_errors(&module);
}

#[test]
fn local_trait_codomain_resolves_to_trait_object() {
    let source = [
        DECLS,
        r#"field def zq6501_tc : Real -> Zq6501Tr { source = imported { path = "x.vdb" format = OpenVDB grid = "density" } }"#,
    ]
    .concat();
    let module = compile_source(&source);
    assert_eq!(
        field(&module, "zq6501_tc").codomain_type,
        Type::TraitObject("Zq6501Tr".into())
    );
    assert_no_coercion_warning(&module, &source, "zq6501_tc : Real -> ", "Zq6501Tr");
}

#[test]
fn prelude_trait_domain_resolves_to_trait_object() {
    let source =
        "field def zq6501_pt : ConstitutiveLaw -> Real { source = analytical { |m| 1.0 } }";
    let module = compile_source_with_stdlib(source);
    assert_eq!(
        field(&module, "zq6501_pt").domain_type,
        Type::TraitObject("ConstitutiveLaw".into())
    );
    assert_no_coercion_warning(&module, source, "zq6501_pt : ", "ConstitutiveLaw");
}

#[test]
fn undeclared_name_still_coerces_to_structure_ref_with_warning() {
    // Green before and after the fix: proves the span predicate sees the warning.
    let source = "field def zq6501_u : Point3 -> Real { source = analytical { |m| 1.0 } }";
    let module = compile_source(source);
    assert_eq!(
        field(&module, "zq6501_u").domain_type,
        Type::StructureRef("Point3".into())
    );
    let warnings = coercion_warnings_at(&module, span_after(source, "zq6501_u : ", "Point3"));
    assert_eq!(
        warnings.len(),
        1,
        "expected one coercion warning; got {warnings:?}"
    );
}
