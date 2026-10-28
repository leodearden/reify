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

const LOCAL_DECLS: &str = "structure def Zq6501Str { param a : Real = 1.0 }\ntrait Zq6501Tr {}\n";
const FIELD: &str = "zq6501_f";

#[derive(Clone, Copy, Debug)]
enum Position {
    Domain,
    Codomain,
}

impl Position {
    /// A `field def` naming `ty` at this position, and the text preceding `ty`.
    /// A codomain takes an imported source so no lambda codomain check runs.
    fn field_def(self, ty: &str) -> (String, String) {
        match self {
            Position::Domain => (
                format!("field def {FIELD} : {ty} -> Real {{ source = analytical {{ |m| 1.0 }} }}"),
                format!("{FIELD} : "),
            ),
            Position::Codomain => (
                format!(
                    r#"field def {FIELD} : Real -> {ty} {{ source = imported {{ path = "x.vdb" format = OpenVDB grid = "density" }} }}"#
                ),
                format!("{FIELD} : Real -> "),
            ),
        }
    }

    fn resolved_in(self, field: &CompiledField) -> &Type {
        match self {
            Position::Domain => &field.domain_type,
            Position::Codomain => &field.codomain_type,
        }
    }
}

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

/// Compiles `decls` plus one field naming `ty` at `position`; returns the
/// module and the byte range of `ty` in the compiled source.
fn compile_field_def(
    compile: fn(&str) -> CompiledModule,
    decls: &str,
    position: Position,
    ty: &str,
) -> (CompiledModule, (u32, u32)) {
    let (field_def, prefix) = position.field_def(ty);
    let source = [decls, &field_def].concat();
    let span = span_after(&source, &prefix, ty);
    (compile(&source), span)
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

fn field(module: &CompiledModule) -> &CompiledField {
    module
        .fields
        .iter()
        .find(|f| f.name == FIELD)
        .unwrap_or_else(|| panic!("field `{FIELD}` missing from compiled module"))
}

/// Where a field names a declared type, that name, and the `Type` constructor
/// it must resolve through.
type Case = (Position, &'static str, fn(String) -> Type);

/// Every case names a declared structure or trait, so each must resolve to
/// `kind(name)` with no coercion warning and no error.
fn assert_declared_names_resolve(compile: fn(&str) -> CompiledModule, decls: &str, cases: &[Case]) {
    for &(position, ty, kind) in cases {
        let case = format!("{position:?} `{ty}`");
        let (module, span) = compile_field_def(compile, decls, position, ty);
        assert_eq!(
            *position.resolved_in(field(&module)),
            kind(ty.into()),
            "{case}"
        );
        let warnings = coercion_warnings_at(&module, span);
        assert!(
            warnings.is_empty(),
            "{case} is declared, so it must resolve without the coercion warning; got {warnings:?}"
        );
        let errors = errors_only(&module);
        assert!(
            errors.is_empty(),
            "{case}: expected no errors; got {errors:?}"
        );
    }
}

#[test]
fn local_declared_names_resolve_without_coercion() {
    assert_declared_names_resolve(
        compile_source,
        LOCAL_DECLS,
        &[
            (Position::Domain, "Zq6501Str", Type::StructureRef),
            (Position::Domain, "Zq6501Tr", Type::TraitObject),
            (Position::Codomain, "Zq6501Str", Type::StructureRef),
            (Position::Codomain, "Zq6501Tr", Type::TraitObject),
        ],
    );
}

#[test]
fn prelude_declared_names_resolve_without_coercion() {
    assert_declared_names_resolve(
        compile_source_with_stdlib,
        "",
        &[
            (Position::Domain, "ConstitutiveLaw", Type::TraitObject),
            (Position::Domain, "Material", Type::StructureRef),
        ],
    );
}

#[test]
fn undeclared_name_still_coerces_to_structure_ref_with_warning() {
    // Green before and after the fix: proves the span predicate sees the warning.
    let (module, span) = compile_field_def(compile_source, "", Position::Domain, "Point3");
    assert_eq!(
        field(&module).domain_type,
        Type::StructureRef("Point3".into())
    );
    let warnings = coercion_warnings_at(&module, span);
    assert_eq!(
        warnings.len(),
        1,
        "expected one coercion warning; got {warnings:?}"
    );
}
