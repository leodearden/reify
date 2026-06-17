//! Integration tests for compile-time validation that a `sub` declaration's
//! `structure_name` resolves to a known template in (module ∪ prelude).
//!
//! Task 4528: check_sub_structure_existence pass in
//! crates/reify-compiler/src/conformance/sub_component_validation.rs.
//!
//! Assertions (a)/(b)/(e) are RED until step-2 wires the pass.
//! Assertions (c)/(d) are GREEN today (no diagnostic expected).

use reify_core::Severity;

/// True iff `module` has an `Error`-severity diagnostic whose message contains `needle`.
fn has_error_containing(module: &reify_compiler::CompiledModule, needle: &str) -> bool {
    module
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.message.contains(needle))
}

/// Dump every diagnostic as `severity: message` for assert-failure context.
fn dump_diags(module: &reify_compiler::CompiledModule) -> Vec<String> {
    module
        .diagnostics
        .iter()
        .map(|d| format!("{}: {}", d.severity, d.message))
        .collect()
}

/// (a) Scalar-form sub with a completely unknown structure name.
///
/// `structure def P { sub x = Garbage() }` compiled with the stdlib prelude
/// must produce a Severity::Error diagnostic whose message contains BOTH
/// `unknown structure "Garbage"` AND `sub-component "x"`.
///
/// RED today — no such diagnostic is emitted until step-2 wires the pass.
#[test]
fn scalar_sub_unknown_structure_name_emits_error() {
    let source = "structure def P { sub x = Garbage() }";
    let parsed = reify_compiler::parse_with_stdlib(
        source,
        reify_core::ModulePath::single("test"),
    );
    let compiled = reify_compiler::compile_with_stdlib(&parsed);

    assert!(
        has_error_containing(&compiled, "unknown structure \"Garbage\""),
        "expected an Error containing 'unknown structure \"Garbage\"'; diagnostics: {:?}",
        dump_diags(&compiled)
    );
    assert!(
        has_error_containing(&compiled, "sub-component \"x\""),
        "expected an Error containing 'sub-component \"x\"'; diagnostics: {:?}",
        dump_diags(&compiled)
    );
}

/// (b) Collection-form sub with an unknown element structure name.
///
/// `sub ribs : List<Garbage>` — the element type name `Garbage` lands in
/// `structure_name` (is_collection=true), so the check must cover it identically
/// to the scalar form.
///
/// RED today — no such diagnostic is emitted until step-2 wires the pass.
#[test]
fn collection_sub_unknown_element_structure_name_emits_error() {
    let source = "structure def P { sub ribs : List<Garbage> }";
    let parsed = reify_compiler::parse_with_stdlib(
        source,
        reify_core::ModulePath::single("test"),
    );
    let compiled = reify_compiler::compile_with_stdlib(&parsed);

    assert!(
        has_error_containing(&compiled, "unknown structure \"Garbage\""),
        "expected an Error containing 'unknown structure \"Garbage\"' for collection \
         sub with unknown element type; diagnostics: {:?}",
        dump_diags(&compiled)
    );
}

/// (c) Over-rejection guard: a sub targeting a stdlib OCCURRENCE must NOT error.
///
/// `STLOutput` is an occurrence template in the stdlib prelude (io.ri:128).
/// The existence check must include ALL prelude templates (no EntityKind filter),
/// so occurrence templates resolve and produce no diagnostic.
///
/// GREEN today and after step-2 (prelude includes occurrences).
#[test]
fn stdlib_occurrence_sub_no_error_with_prelude() {
    let source = "structure def P { sub o = STLOutput() }";
    let parsed = reify_compiler::parse_with_stdlib(
        source,
        reify_core::ModulePath::single("test"),
    );
    let compiled = reify_compiler::compile_with_stdlib(&parsed);

    assert!(
        !has_error_containing(&compiled, "unknown structure"),
        "stdlib OCCURRENCE 'STLOutput' must resolve via prelude — no 'unknown structure' \
         error expected; diagnostics: {:?}",
        dump_diags(&compiled)
    );
}

/// (d) Positive guard: a sub targeting a locally-defined child structure is valid.
///
/// `structure def Child {}  structure def P { sub c = Child() }` — `Child` is
/// in the same module, so the existence check must accept it.
///
/// GREEN today and after step-2 (local templates are in the union).
#[test]
fn local_structure_sub_no_error() {
    let source = "structure def Child {}\nstructure def P { sub c = Child() }";
    let parsed = reify_compiler::parse_with_stdlib(
        source,
        reify_core::ModulePath::single("test"),
    );
    let compiled = reify_compiler::compile_with_stdlib(&parsed);

    assert!(
        !has_error_containing(&compiled, "unknown structure"),
        "locally-defined 'Child' must resolve — no 'unknown structure' error \
         expected; diagnostics: {:?}",
        dump_diags(&compiled)
    );
}

/// (e) Prelude-consultation pin: the same stdlib-occurrence source compiled with
/// an EMPTY prelude (`reify_compiler::compile`, lib.rs:148) MUST produce an
/// `unknown structure "STLOutput"` Error.
///
/// This proves the check consults the effective prelude rather than hardcoding
/// names: when the prelude is absent, `STLOutput` is unknown.
///
/// RED today — the pass does not exist yet, so zero diagnostics are emitted.
#[test]
fn stdlib_occurrence_sub_errors_with_empty_prelude() {
    // parse_with_stdlib seeds the stdlib enum names so EnumAccess references
    // lower correctly; the prelude is passed separately at compile time.
    let source = "structure def P { sub o = STLOutput() }";
    let parsed = reify_compiler::parse_with_stdlib(
        source,
        reify_core::ModulePath::single("test"),
    );
    // compile() = compile_with_prelude(parsed, &[]) — empty prelude.
    let compiled = reify_compiler::compile(&parsed);

    assert!(
        has_error_containing(&compiled, "unknown structure \"STLOutput\""),
        "with an empty prelude, 'STLOutput' is unknown; expected an Error containing \
         'unknown structure \"STLOutput\"'; diagnostics: {:?}",
        dump_diags(&compiled)
    );
}
