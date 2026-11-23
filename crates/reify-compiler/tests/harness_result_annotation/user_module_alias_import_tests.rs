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

fn wrapping_importer4(stem: &str, initializer: &str) -> String {
    format!(
        "module {stem}\n\
import prod4\n\
type InnerPubZz = Time\n\
type Wrap<Q: Dimension> = PerInnerPubZz<Q>\n\
structure def App {{\n\
\x20   param x : Wrap<Force> = {initializer}\n\
}}\n"
    )
}

/// The same capture reached through a NESTED instantiation: the importer's own
/// `Wrap<Q>` names the imported `PerInnerPubZz<Q>`, whose body still resolves
/// `InnerPubZz` in prod4, not as the importer's `Time`.
#[test]
fn importer_local_wrapper_cannot_capture_a_nested_imported_parametric_alias_body() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "prod4.ri", PROD4);

    let captured = compile(
        dir.path(),
        "imp4wa",
        &wrapping_importer4("imp4wa", "3N / 1s"),
    );
    assert!(
        has_error_code(captured.entry(), DiagnosticCode::ParamDefaultTypeMismatch),
        "`3N / 1s` is not a Length, so it must be rejected; got {:?}",
        captured.entry().diagnostics
    );

    let bound = compile(dir.path(), "imp4wb", &wrapping_importer4("imp4wb", "3mm"));
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

/// A parametric body naming a parametric alias its defining module imported:
/// `Outer7<Q>` is prod7's spelling of prod6's `Per6<Q>`, and `Per6`'s body
/// resolves `Stiff6` in prod6, two modules away from the entry.
#[test]
fn nested_parametric_alias_imported_by_the_defining_module_resolves_in_its_own_module() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "prod6.ri",
        "module prod6\n\
pub type Stiff6 = Force / Length\n\
pub type Per6<Q: Dimension> = Q / Stiff6\n",
    );
    write(
        dir.path(),
        "prod7.ri",
        "module prod7\nimport prod6\npub type Outer7<Q: Dimension> = Per6<Q>\n",
    );

    let program = compile(
        dir.path(),
        "imp7",
        "module imp7\nimport prod7\nstructure def App {\n    param x : Outer7<Force> = 3mm\n}\n",
    );
    assert!(
        errors(program.entry()).is_empty(),
        "`Outer7<Force>` is a Length; got {:?}",
        errors(program.entry())
    );
    assert_eq!(cell_type(program.entry(), "App", "x"), &LENGTH);
}

fn entity_importer10(stem: &str, local_alias: &str) -> String {
    format!(
        "module {stem}\n\
import prod10\n\
{local_alias}\n\
structure def App {{\n\
\x20   param m : O10<Real>\n\
}}\n"
    )
}

/// An imported parametric body's ENTITY name is not an alias where the body is
/// defined, so an importer's alias of that name is not consulted either:
/// `O10<Real>` means the same with or without the importer's `type E10 = Length`.
/// The non-parametric counterpart holds too (#10354), pinned by
/// `importer_local_alias_named_like_an_imported_plain_alias_body_entity_is_not_consulted`.
#[test]
fn importer_local_alias_named_like_an_imported_parametric_body_entity_is_not_consulted() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "prod10.ri",
        "module prod10\npub enum E10 { A, B }\npub type O10<T> = Option<E10>\n",
    );

    let plain = compile(dir.path(), "imp10a", &entity_importer10("imp10a", ""));
    assert!(
        errors(plain.entry()).is_empty(),
        "got {:?}",
        errors(plain.entry())
    );
    let meant = cell_type(plain.entry(), "App", "m");
    assert!(!meant.is_error(), "`O10<Real>` must resolve; got {meant:?}");

    let shadowed = compile(
        dir.path(),
        "imp10b",
        &entity_importer10("imp10b", "type E10 = Length"),
    );
    assert!(
        errors(shadowed.entry()).is_empty(),
        "got {:?}",
        errors(shadowed.entry())
    );
    assert_eq!(cell_type(shadowed.entry(), "App", "m"), meant);
}

fn plain_entity_importer8(stem: &str, local_alias: &str) -> String {
    format!(
        "module {stem}\n\
import prod8\n\
{local_alias}\n\
structure def App {{\n\
\x20   param m : F8\n\
}}\n"
    )
}

/// The non-parametric counterpart: `F8` names prod8's enum `E8`, and an
/// importer's `type E8 = Length` does not turn it into a Length.
#[test]
fn importer_local_alias_named_like_an_imported_plain_alias_body_entity_is_not_consulted() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "prod8.ri",
        "module prod8\npub enum E8 { A, B }\npub type F8 = E8\n",
    );

    let plain = compile(dir.path(), "imp8a", &plain_entity_importer8("imp8a", ""));
    assert!(
        errors(plain.entry()).is_empty(),
        "got {:?}",
        errors(plain.entry())
    );
    let meant = cell_type(plain.entry(), "App", "m");
    assert!(!meant.is_error(), "`F8` must resolve; got {meant:?}");

    let shadowed = compile(
        dir.path(),
        "imp8b",
        &plain_entity_importer8("imp8b", "type E8 = Length"),
    );
    assert!(
        errors(shadowed.entry()).is_empty(),
        "got {:?}",
        errors(shadowed.entry())
    );
    assert_eq!(cell_type(shadowed.entry(), "App", "m"), meant);
}

const PROD8_MIXED: &str = "module prod8\n\
pub enum E8 { A, B }\n\
type In8 = Length\n\
pub type F8 = Map<In8, E8>\n";

fn mixed_importer8(stem: &str, local_alias: &str) -> String {
    format!(
        "module {stem}\n\
import prod8\n\
{local_alias}\n\
structure def App {{\n\
\x20   param m : F8\n\
\x20   param d : Map<Length, E8>\n\
}}\n"
    )
}

/// A plain pub alias whose body names both an entity and a non-pub alias is
/// accepted where it is defined, and in every importer its alias name `In8`
/// is prod8's `Length`, whatever the importer calls `In8`.
#[test]
fn imported_plain_alias_body_binds_its_defining_modules_alias_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "prod8.ri", PROD8_MIXED);

    let defining = compile(dir.path(), "prod8", PROD8_MIXED);
    assert!(
        errors(defining.entry()).is_empty(),
        "a pub plain alias may name a non-pub alias; got {:?}",
        errors(defining.entry())
    );

    for (stem, local_alias) in [("imp8m", ""), ("imp8n", "type In8 = Time")] {
        let program = compile(dir.path(), stem, &mixed_importer8(stem, local_alias));
        let entry = program.entry();
        assert!(
            errors(entry).is_empty(),
            "with `{local_alias}`: got {:?}",
            errors(entry)
        );
        let aliased = cell_type(entry, "App", "m");
        assert!(
            !aliased.is_error(),
            "with `{local_alias}`: `F8` must resolve; got {aliased:?}"
        );
        assert_eq!(
            aliased,
            cell_type(entry, "App", "d"),
            "with `{local_alias}`: `F8` must mean prod8's `Map<Length, E8>`"
        );
    }
}

const PROD2: &str = "module prod2\n\
type InnerZz = Force / Length\n\
type HiddenZz = Length\n\
pub type OuterZz = InnerZz / Time\n";

/// A plain (non-parametric) pub alias whose body names no entity is resolved
/// where it is defined, so an importer's local `InnerZz` does not touch it.
/// One whose body names an entity resolves at each use site, but its alias
/// names still bind where it is defined (#10354).
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

/// A pub parametric alias's meaning may not depend on an alias its importers
/// cannot see, so `reify check` on its own module rejects one whose body names
/// a non-pub alias, at the pub alias rather than at the non-pub one.
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
        line_start <= span.start as usize && span.end as usize <= line_end,
        "the label must sit on the `pub type PerInnerZz` line \
         ({line_start}..{line_end}); got {span:?}"
    );
}
