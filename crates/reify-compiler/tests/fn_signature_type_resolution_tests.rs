//! fn-signature type resolution — compilation tests for task 3440.
//!
//! Verifies that `fn` parameter and return types referencing user-defined
//! structures and traits resolve to the correct `Type` variants.  Before the
//! two-pass fix, `compile_function` passed empty `HashSet<String>` for
//! `structure_names` / `trait_names`, so any custom type in a fn signature
//! produced a spurious "unresolved type" Error diagnostic.
//!
//! All tests use either `compile_source` (no stdlib, for locally-defined
//! structures) or `compile_source_with_stdlib` (stdlib in scope, for names
//! like `MaterialSpec`, `ElasticOptions`, `ElasticResult`).

use reify_test_support::{compile_source, compile_source_with_stdlib};
use reify_core::{Severity, Type};

/// Module-local structure name in a fn parameter resolves to
/// `Type::StructureRef("MyS")` with zero Error diagnostics.
///
/// Pins that the two-pass fix makes locally-defined structure names visible to
/// `compile_function` (they live in `ctx.seen_entity_names` after the pre-pass,
/// which is available before `phase_functions` runs).
#[test]
fn fn_signature_resolves_local_structure_name() {
    let source = r#"
        structure def MyS { param x: Length = 1mm }
        fn t(s: MyS) -> Real { 0 }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "t")
        .expect("function 't' should be compiled");

    assert_eq!(
        func.params[0].1,
        Type::StructureRef("MyS".to_string()),
        "fn param typed with local structure name should resolve to Type::StructureRef(\"MyS\")"
    );
}

/// Stdlib trait name in a fn parameter resolves to
/// `Type::TraitObject("MaterialSpec")` with zero Error diagnostics.
///
/// Pins that the two-pass fix makes prelude trait names (`MaterialSpec` from
/// `stdlib/materials_mechanical.ri`) visible to `compile_function`.
#[test]
fn fn_signature_resolves_stdlib_trait_name() {
    let source = r#"fn t(m: MaterialSpec) -> Real { 0 }"#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "t")
        .expect("function 't' should be compiled");

    assert_eq!(
        func.params[0].1,
        Type::TraitObject("MaterialSpec".to_string()),
        "fn param typed with stdlib trait name should resolve to Type::TraitObject(\"MaterialSpec\")"
    );
}

/// Stdlib structure name in a fn parameter resolves to
/// `Type::StructureRef("ElasticOptions")` with zero Error diagnostics.
///
/// Pins that the two-pass fix makes prelude structure names (`ElasticOptions`
/// from `stdlib/solver_elastic.ri`) visible to `compile_function`.
#[test]
fn fn_signature_resolves_stdlib_structure_name() {
    let source = r#"fn t(opts: ElasticOptions) -> Real { 0 }"#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "t")
        .expect("function 't' should be compiled");

    assert_eq!(
        func.params[0].1,
        Type::StructureRef("ElasticOptions".to_string()),
        "fn param typed with stdlib structure name should resolve to Type::StructureRef(\"ElasticOptions\")"
    );
}

/// Stdlib structure name as a fn return type resolves to
/// `Type::StructureRef("ElasticResult")` with no "unresolved type" Error for that name.
///
/// The bare `ElasticResult()` constructor call in the body may produce other
/// diagnostics (since fn body type-checking is a separate pass), but the
/// *signature*-level resolution must succeed.  This test asserts only the
/// signature contract — no Error diagnostic mentioning "ElasticResult" in an
/// "unresolved type" message, and `return_type == StructureRef("ElasticResult")`.
#[test]
fn fn_signature_resolves_stdlib_structure_as_return_type() {
    let source = r#"fn make() -> ElasticResult { 0 }"#;
    let module = compile_source_with_stdlib(source);

    // Filter only the errors that are about *signature* type resolution for
    // "ElasticResult" — body errors are tolerated here.
    let sig_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.contains("unresolved")
                && d.message.contains("ElasticResult")
        })
        .collect();
    assert!(
        sig_errors.is_empty(),
        "expected no 'unresolved type ElasticResult' error in signature, got: {:?}",
        sig_errors
    );

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "make")
        .expect("function 'make' should be compiled");

    assert_eq!(
        func.return_type,
        Type::StructureRef("ElasticResult".to_string()),
        "fn return type named 'ElasticResult' should resolve to Type::StructureRef(\"ElasticResult\")"
    );
}

/// Parametric `List<UserStruct>` in a fn parameter resolves to
/// `Type::List(Box::new(Type::StructureRef("MyLoad")))` with zero Error diagnostics.
///
/// Pins that structure-name resolution flows through the parameterized-type
/// resolver (`resolve_parameterized_builtin_type`) when the element type is a
/// user-defined structure name.
#[test]
fn fn_signature_resolves_parametric_list_with_user_structure() {
    let source = r#"
        structure def MyLoad { param p: Real = 0.0 }
        fn t(loads: List<MyLoad>) -> Real { 0 }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "t")
        .expect("function 't' should be compiled");

    assert_eq!(
        func.params[0].1,
        Type::List(Box::new(Type::StructureRef("MyLoad".to_string()))),
        "fn param typed List<MyLoad> should resolve to Type::List(StructureRef(\"MyLoad\"))"
    );
}

/// Builtin alias `Solid` in a fn parameter still resolves to `Type::Geometry`
/// after the two-pass fix.
///
/// Regression pin: the new resolution path must NOT break the builtin-alias
/// lookup that already works (builtins are resolved before structure/trait
/// name sets are consulted, so this should remain stable).
#[test]
fn fn_signature_resolves_solid_builtin_alias() {
    let source = r#"fn t(body: Solid) -> Real { 0 }"#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics for builtin alias Solid, got: {:?}",
        errors
    );

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "t")
        .expect("function 't' should be compiled");

    assert_eq!(
        func.params[0].1,
        Type::Geometry,
        "fn param typed with builtin alias 'Solid' should resolve to Type::Geometry"
    );
}

/// Both `phase_functions` (fn signature) and `phase_traits` (structure
/// conformance check) work correctly together after the DRY refactor in
/// step-2 (g), where `phase_traits` was changed to read the pre-computed
/// ctx name sets instead of rebuilding local copies.
///
/// Source contains a fn whose parameter type is a stdlib trait name AND a
/// local structure that refines that same trait.  The test pins that both
/// phases integrate without producing Error diagnostics:
/// - `phase_functions` resolves `MaterialSpec` in the fn signature (reads
///   `ctx.resolution_trait_names`).
/// - `phase_traits` compiles the `MyMat : MaterialSpec` conformance
///   relationship via `build_trait_registry` (reads
///   `ctx.resolution_trait_names` / `ctx.resolution_structure_names`).
///
/// Note: because this source contains no local `trait` declarations,
/// `compile_trait` is not invoked; the conformance check exercised here is
/// the `build_trait_registry` / deprecation path in `phase_traits`, not the
/// trait-member type-resolution path.  The zero-errors assertion confirms
/// that both phases share the ctx name sets without interference.
#[test]
fn phase_traits_consumes_shared_names_no_regression() {
    // MyMat must implement all required MaterialSpec members (density: Density,
    // name: String) to avoid conformance errors — those are orthogonal to the
    // type-resolution contract being pinned here.
    let source = r#"
        structure def MyMat : MaterialSpec {
            param density : Density = 7800kg/m^3
            param name : String = "my-mat"
            param young_modulus : Real = 210000.0
        }
        fn use_mat(m: MaterialSpec) -> Real { 0 }
    "#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics — both phase_functions (fn sig) and \
         phase_traits (structure conformance) must read the shared name sets correctly; \
         got: {:?}",
        errors
    );
}

/// A genuinely-unknown type name in a fn parameter still emits an Error
/// diagnostic whose message contains both "unresolved type" and the offending name.
///
/// Negative-path regression pin: the fix must NOT suppress the existing
/// "unresolved type" diagnostic when a name is not found in builtins, aliases,
/// structures, or traits.
#[test]
fn fn_signature_unresolved_type_still_errors() {
    let source = r#"fn t(x: NoSuchType) -> Real { 0 }"#;
    let module = compile_source(source);

    let matching_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.contains("unresolved type")
                && d.message.contains("NoSuchType")
        })
        .collect();
    assert!(
        !matching_errors.is_empty(),
        "expected at least one Error diagnostic containing 'unresolved type' and 'NoSuchType', \
         but none found; all diagnostics: {:?}",
        module.diagnostics
    );
}
