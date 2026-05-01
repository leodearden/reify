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
//! - `Vector3<Q>` — like Scalar, routes through
//!   `resolve_type_alias_expr_to_dimension`; the alias-DFS `Propagate` path
//!   emits `"cannot resolve '...' to a dimension type in alias expression"`,
//!   which the test pins via the `"dimension type in alias expression"` fragment
//!   — unique to that helper's error path.

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

/// Compile `source`, assert at least one Error-severity diagnostic is emitted,
/// and assert that at least one of those Error diagnostics contains `fragment`
/// in its message.  This pins the assertion to a specific inner-arg error rather
/// than any unrelated error that might happen to occur in the pipeline.
fn assert_error_containing(source: &str, fragment: &str) {
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
    assert!(
        errs.iter().any(|d| d.message.contains(fragment)),
        "no Error-severity diagnostic mentions {fragment:?}.\
         \nErrors: {errs:?}\nSource:\n{source}",
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
/// Error-severity diagnostic, and that diagnostic must mention `NotADim`.
///
/// This is the primary regression test for task #2766.  `Scalar` DOES have a
/// `resolve_type_name` default (`Type::Scalar { dimension: LENGTH }`).  Before
/// the fix, the alias-DFS pre-pass silently discarded the inner-arg diagnostic
/// from `resolve_parameterized_builtin_type`, fell through to the simple-name
/// branch, and registered the alias as `Type::length()`.  The use-site then
/// resolved `Bad` → `Type::length()` with zero errors — a silent type
/// correctness regression.
///
/// After the fix (`AliasInnerDiagPolicy::Propagate` propagates `tmp_diags`
/// into `diagnostics`), the Error surfaces during alias-body resolution.
/// The additional `NotADim` fragment check pins the assertion to the inner-arg
/// diagnostic specifically, preventing a false pass from any unrelated error
/// that might be introduced later in the pipeline.
#[test]
fn non_parametric_alias_scalar_unknown_dimension_produces_error() {
    assert_error_containing(
        "type Bad = Scalar<NotADim>\nstructure def Use { param v : Bad }",
        "NotADim",
    );
}

/// A non-parametric alias `type Bad = Vector3<NotADim>` paired with a use-site
/// `structure def Use { param v : Bad }` must produce at least one
/// Error-severity diagnostic whose message contains the fixed phrase
/// `"dimension type in alias expression"`.
///
/// `Vector3` routes through `resolve_type_alias_expr_to_dimension`, the same
/// dimension-resolver helper used by Scalar.  When `NotADim` cannot be resolved
/// to a known dimension, the helper emits `"cannot resolve 'NotADim' to a
/// dimension type in alias expression"`.  Because the alias has zero type
/// parameters, `seed_alias_entry` selects `AliasInnerDiagPolicy::Propagate`,
/// which extends `diagnostics` with the inner-arg error.
///
/// The `"dimension type in alias expression"` fragment uniquely identifies the
/// dimension-resolver helper's error path — the use-site fallback
/// `"unresolved type: Bad"` does not contain this phrase, and no other code
/// path currently emits it.  The assertion therefore fails if `Propagate`
/// regresses or the dimension-resolver is bypassed, without being confused by
/// any future diagnostic that merely mentions the identifier `NotADim`.
/// This complements the Scalar test by exercising the second
/// `resolve_parameterized_builtin_type` branch (the dimension-resolver branch).
#[test]
fn non_parametric_alias_vector3_unknown_dimension_produces_error() {
    assert_error_containing(
        "type Bad = Vector3<NotADim>\nstructure def Use { param v : Bad }",
        "dimension type in alias expression",
    );
}
