//! User module to user module type-alias import through `compile_program`, the
//! path `reify check` drives (task #10297).
//!
//! Every fixture is a real multi-file program in a `tempfile::tempdir()`; the
//! importer reaches the imported module through its `import` line, never a
//! hand-built prelude. The hand-built `PreludeContext` seed is the subject of
//! `cross_module_alias_propagation_tests.rs`.
//!
//! The rule under test: an imported pub PARAMETRIC alias resolves the alias
//! names its body mentions in the module that defined it, so an importer's own
//! alias of the same name cannot change its meaning, and a name the defining
//! module itself imported still resolves.

use std::fs;
use std::path::Path;

use reify_compiler::CompiledModule;
use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::ModuleResolver;
use reify_compiler::program::{CompiledProgram, compile_program};
use reify_core::{Diagnostic, DiagnosticCode, DimensionVector, Severity, Type};
use reify_ir::Satisfaction;
use reify_test_support::mocks::MockConstraintChecker;

fn write(dir: &Path, name: &str, source: &str) {
    fs::write(dir.join(name), source).expect("write fixture");
}

fn compile(dir: &Path, stem: &str, source: &str) -> CompiledProgram {
    compile_program(
        &dir.join(format!("{stem}.ri")),
        source,
        &ModuleResolver::new(dir, dir.join("stdlib")),
        &CfgSet::default(),
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
    )
}

fn errors(module: &CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

fn cell_type<'m>(module: &'m CompiledModule, template: &str, member: &str) -> &'m Type {
    let template = module
        .templates
        .iter()
        .find(|t| t.name == template)
        .unwrap_or_else(|| panic!("template `{template}` not found"));
    &template
        .value_cells
        .iter()
        .find(|c| c.id.member == member)
        .unwrap_or_else(|| panic!("value cell `{member}` not found"))
        .cell_type
}

fn has_error_code(module: &CompiledModule, code: DiagnosticCode) -> bool {
    errors(module).iter().any(|d| d.code == Some(code))
}

const LENGTH: Type = Type::Scalar {
    dimension: DimensionVector::LENGTH,
};

const PROD4: &str = "module prod4\n\
pub type InnerPubZz = Force / Length\n\
pub type PerInnerPubZz<Q: Dimension> = Q / InnerPubZz\n";

fn importer4(stem: &str, initializer: &str) -> String {
    format!(
        "module {stem}\n\
import prod4\n\
type InnerPubZz = Time\n\
structure def App {{\n\
\x20   param x : PerInnerPubZz<Force> = {initializer}\n\
}}\n"
    )
}

/// An importer's local alias named like one the imported parametric body
/// mentions does not capture it: `PerInnerPubZz<Force>` is
/// `Force / (Force / Length)`, a Length, whatever the importer calls `InnerPubZz`.
#[test]
fn importer_local_alias_cannot_capture_an_imported_parametric_alias_body() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "prod4.ri", PROD4);

    let captured = compile(dir.path(), "imp4a", &importer4("imp4a", "3N / 1s"));
    assert!(
        has_error_code(captured.entry(), DiagnosticCode::ParamDefaultTypeMismatch),
        "`3N / 1s` is not a Length, so it must be rejected; got {:?}",
        captured.entry().diagnostics
    );

    let bound = compile(dir.path(), "imp4b", &importer4("imp4b", "3mm"));
    assert!(
        errors(bound.entry()).is_empty(),
        "`3mm` is a Length and must be accepted; got {:?}",
        errors(bound.entry())
    );
    assert_eq!(cell_type(bound.entry(), "App", "x"), &LENGTH);
}

/// A parametric body resolves a name its defining module itself imported: the
/// entry never imports prod6, yet `Per7`'s `Stiff6` is prod6's.
#[test]
fn imported_parametric_alias_body_resolves_names_its_defining_module_imported() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "prod6.ri",
        "module prod6\npub type Stiff6 = Force / Length\n",
    );
    write(
        dir.path(),
        "prod7.ri",
        "module prod7\nimport prod6\npub type Per7<Q: Dimension> = Q / Stiff6\n",
    );

    let program = compile(
        dir.path(),
        "imp7",
        "module imp7\nimport prod7\nstructure def App {\n    param x : Per7<Force> = 3mm\n}\n",
    );
    assert!(
        errors(program.entry()).is_empty(),
        "`Per7<Force>` is a Length; got {:?}",
        errors(program.entry())
    );
    assert_eq!(cell_type(program.entry(), "App", "x"), &LENGTH);
}

const PROD2: &str = "module prod2\n\
type InnerZz = Force / Length\n\
type HiddenZz = Length\n\
pub type OuterZz = InnerZz / Time\n";

/// A plain (non-parametric) pub alias is resolved where it is defined, so an
/// importer's local `InnerZz` does not touch it.
#[test]
fn plain_pub_alias_is_immune_to_an_importer_local_of_the_same_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "prod2.ri", PROD2);

    let program = compile(
        dir.path(),
        "imp2",
        "module imp2\n\
import prod2\n\
type InnerZz = Length\n\
type DirectZz = Force / Length / Time\n\
structure def App {\n\
\x20   param x : OuterZz\n\
\x20   param y : DirectZz\n\
}\n",
    );
    assert!(
        errors(program.entry()).is_empty(),
        "got {:?}",
        errors(program.entry())
    );
    assert_eq!(
        cell_type(program.entry(), "App", "x"),
        cell_type(program.entry(), "App", "y"),
        "`OuterZz` must mean prod2's `Force / Length / Time`"
    );
}

fn assert_unresolved(program: &CompiledProgram, name: &str) {
    assert!(
        errors(program.entry())
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::UnresolvedType) && d.message.contains(name)),
        "`{name}` must be an unresolved type; got {:?}",
        program.entry().diagnostics
    );
}

/// A non-pub alias stays private to its module.
#[test]
fn non_pub_alias_is_not_visible_to_an_importer() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "prod2.ri", PROD2);

    let program = compile(
        dir.path(),
        "imp2",
        "module imp2\nimport prod2\nstructure def App {\n    param x : HiddenZz\n}\n",
    );
    assert_unresolved(&program, "HiddenZz");
}

/// A pub alias is visible only to a module that imports its module.
#[test]
fn alias_is_not_visible_without_an_import_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "prod2.ri", PROD2);

    let program = compile(
        dir.path(),
        "noimp",
        "module noimp\nstructure def App {\n    param x : OuterZz\n}\n",
    );
    assert_unresolved(&program, "OuterZz");
}

/// Capturing the body scope of mutually recursive parametric aliases ends, and
/// using one still reports the recursion instead of hanging.
#[test]
fn mutually_recursive_imported_parametric_aliases_still_report_recursion() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "rec.ri",
        "module rec\n\
pub type LoopA<Q: Dimension> = LoopB<Q>\n\
pub type LoopB<Q: Dimension> = LoopA<Q>\n",
    );

    let program = compile(
        dir.path(),
        "imprec",
        "module imprec\nimport rec\nstructure def App {\n    param x : LoopA<Length>\n}\n",
    );
    assert!(
        errors(program.entry())
            .iter()
            .any(|d| d.message.contains("maximum instantiation depth")),
        "got {:?}",
        program.entry().diagnostics
    );
}

const PROD3: &str = "module prod3\n\
type InnerZz = Force / Length\n\
pub type PerInnerZz<Q: Dimension> = Q / InnerZz\n";

/// A pub parametric alias whose body names a non-pub alias cannot be used by
/// any importer, so `reify check` on its own module rejects it, at the pub
/// alias rather than at the non-pub one.
#[test]
fn pub_parametric_alias_naming_a_non_pub_alias_is_rejected_where_it_is_defined() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = compile(dir.path(), "prod3", PROD3);

    let errors = errors(program.entry());
    assert_eq!(errors.len(), 1, "got {errors:?}");
    let rejection = errors[0];
    assert_eq!(rejection.code, Some(DiagnosticCode::NonPubAliasInPubAlias));
    assert!(
        rejection.message.contains("'PerInnerZz'") && rejection.message.contains("'InnerZz'"),
        "the message must name both aliases; got {:?}",
        rejection.message
    );

    let line_start = PROD3.find("pub type PerInnerZz").expect("pub alias line");
    let line_end = line_start + PROD3[line_start..].find('\n').expect("line end");
    let span = rejection.labels.first().expect("a label").span;
    assert!(
        line_start <= span.start && span.end <= line_end,
        "the label must sit on the `pub type PerInnerZz` line \
         ({line_start}..{line_end}); got {span:?}"
    );
}
