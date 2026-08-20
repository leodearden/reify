//! Compiler typing tests for the fallible parse builtins (task #4535):
//! `parse_length` / `parse_length_r` must resolve their call-site cell types
//! to `Option<Length>` / `Type::Enum("Result")` rather than falling through
//! the `NoUserFunctions` result-type ladder's first-arg (`String`) fallback.
//!
//! Uses `reify_test_support::compile_source_with_stdlib` (NOT the bare
//! `compile_source`) because `parse_length_r`'s result type is the PRELUDE
//! `Result<T,E>` (dependency task #4035, `stdlib/result.ri`) — a plain
//! `compile_source` leaves `Type::Enum("Result")` unresolved and errors.
//!
//! RED: the compiler has no `parse_signatures` ladder arm yet, so both
//! `parse_length(...)` and `parse_length_r(...)` type via the first-arg
//! fallback as `Type::string()`.

use reify_core::Type;
use reify_test_support::compile_source_with_stdlib;

/// Locate the compiled default_expr's result_type of the named value cell
/// within the named template — mirrors `affine_algebra_typing_tests.rs`'s
/// `find_cell_type`, but compiles WITH stdlib so the PRELUDE `Result<T,E>`
/// is in scope.
fn find_cell_type(source: &str, template_name: &str, cell_name: &str) -> Type {
    let compiled = compile_source_with_stdlib(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == template_name)
        .unwrap_or_else(|| panic!("template '{template_name}' not found"));
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member.as_str() == cell_name)
        .unwrap_or_else(|| panic!("value cell '{cell_name}' not found in {template_name}"));
    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("cell '{cell_name}' has no default_expr"))
        .result_type
        .clone()
}

#[test]
fn parse_length_types_as_option_length() {
    let source = r#"
        structure Widget {
            let a = parse_length("12mm")
        }
    "#;
    let ty = find_cell_type(source, "Widget", "a");
    assert_eq!(
        ty,
        Type::Option(Box::new(Type::length())),
        "parse_length(...) must type as Option<Length>, got {:?}",
        ty
    );
}

#[test]
fn parse_length_r_types_as_enum_result() {
    let source = r#"
        structure Widget {
            let b = parse_length_r("12mm")
        }
    "#;
    let ty = find_cell_type(source, "Widget", "b");
    assert_eq!(
        ty,
        Type::Enum("Result".to_string()),
        "parse_length_r(...) must type as Type::Enum(\"Result\"), got {:?}",
        ty
    );
}

/// Behavioral disjointness: `parse_length`/`parse_length_r` resolve
/// correctly even alongside a sibling math-family builtin call
/// (`sqrt`, dispatched via `is_math_typed_fn`) in the same module — the new
/// parse ladder arm neither steals dispatch from, nor loses it to, a sibling
/// family, regardless of ladder position.
#[test]
fn parse_fns_do_not_collide_with_math_family_in_the_same_module() {
    let source = r#"
        structure Widget {
            let a = parse_length("12mm")
            let b = parse_length_r("12mm")
            let c = sqrt(4.0)
        }
    "#;
    assert_eq!(
        find_cell_type(source, "Widget", "a"),
        Type::Option(Box::new(Type::length()))
    );
    assert_eq!(
        find_cell_type(source, "Widget", "b"),
        Type::Enum("Result".to_string())
    );
    assert_eq!(
        find_cell_type(source, "Widget", "c"),
        Type::dimensionless_scalar()
    );
}
