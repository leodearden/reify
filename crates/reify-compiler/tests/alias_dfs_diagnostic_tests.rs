//! Regression tests locking down diagnostic behaviour for non-parametric type
//! alias bodies in the DFS pre-pass (`resolve_alias_dfs` →
//! `resolve_type_alias_expr`).
//!
//! Contract under test: when a type alias has NO free type parameters (i.e.
//! `type Bad = Scalar<NotADim>` rather than `type Bad<Q> = Scalar<Q>`) and its
//! body references an unresolvable type argument, the resulting
//! Error-severity diagnostic must survive to the caller — NOT be silently
//! discarded into a temporary `Vec<Diagnostic>` and dropped.
//!
//! Three parametric builtins are exercised:
//! - `List<T>` — no `resolve_type_name` default; error surfaces via use-site
//!   `unresolved type: Bad`.
//! - `Scalar<Q>` — HAS a `resolve_type_name` default (`Type::length()`); the
//!   discard bug caused the alias to silently resolve to `Length` with zero
//!   diagnostics.  This test drives the narrowing fix in task #2766.
//! - `Vector3<Q>` — no `resolve_type_name` default; error surfaces via
//!   use-site `unresolved type: Bad`.

mod common;

use common::compile_with_stdlib_helper;
use reify_types::Severity;

/// Compile `source` and assert that at least one Error-severity diagnostic is
/// emitted.  Panics with the full diagnostic list on failure so the test output
/// makes the regression immediately visible.
fn assert_produces_error(source: &str) {
    let module = compile_with_stdlib_helper(source);
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errs.is_empty(),
        "source must produce at least one Error-severity diagnostic, but got none.\
         \nAll diagnostics: {:?}\nSource:\n{source}",
        module.diagnostics
    );
}

/// A non-parametric alias `type Bad = List<DefinitelyNotAType>` paired with a
/// use-site `structure def Use { param v : Bad }` must produce at least one
/// Error-severity diagnostic.
///
/// `List` has no `resolve_type_name` default, so when the inner arg
/// `DefinitelyNotAType` cannot be resolved, `resolve_parameterized_builtin_type`
/// returns `None` and the alias entry records `resolved_type: None`.  The
/// use-site structure-param resolver then emits `unresolved type: Bad`
/// (entity.rs:~409), surfacing the error.
///
/// This test naturally passes even before the task-#2766 narrowing fix; it is
/// included to establish the three-fixture contract as a unit and to prevent
/// regression if `List` ever acquires a default in `resolve_type_name`.
#[test]
fn non_parametric_alias_list_unknown_inner_produces_error() {
    assert_produces_error(
        "type Bad = List<DefinitelyNotAType>\nstructure def Use { param v : Bad }",
    );
}

/// A non-parametric alias `type Bad = Scalar<NotADim>` paired with a use-site
/// `structure def Use { param v : Bad }` must produce at least one
/// Error-severity diagnostic.
///
/// This is the primary regression test for task #2766.  `Scalar` DOES have a
/// `resolve_type_name` default (`Type::Scalar { dimension: LENGTH }`).  Before
/// the fix, the alias-DFS pre-pass silently discarded the inner-arg diagnostic
/// from `resolve_parameterized_builtin_type`, fell through to the simple-name
/// branch, and registered the alias as `Type::length()`.  The use-site then
/// resolved `Bad` → `Type::length()` with zero errors — a silent type
/// correctness regression.
///
/// After the fix (`caller_is_parametric = false` propagates `tmp_diags` into
/// `diagnostics`), the Error surfaces during alias-body resolution.
#[test]
fn non_parametric_alias_scalar_unknown_dimension_produces_error() {
    assert_produces_error(
        "type Bad = Scalar<NotADim>\nstructure def Use { param v : Bad }",
    );
}

/// A non-parametric alias `type Bad = Vector3<NotADim>` paired with a use-site
/// `structure def Use { param v : Bad }` must produce at least one
/// Error-severity diagnostic.
///
/// `Vector3` has no `resolve_type_name` default, so when `NotADim` cannot be
/// resolved to a dimension type, `resolve_parameterized_builtin_type` returns
/// `None`, the alias records `resolved_type: None`, and the use-site emits
/// `unresolved type: Bad`.
///
/// Like the List test, this naturally passes before the fix; included to
/// complete the three-builtin contract surface.
#[test]
fn non_parametric_alias_vector3_unknown_dimension_produces_error() {
    assert_produces_error(
        "type Bad = Vector3<NotADim>\nstructure def Use { param v : Bad }",
    );
}
