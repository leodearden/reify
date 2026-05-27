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
use reify_core::Severity;

/// Shared source for the two Scalar-with-use-site regression tests below.
///
/// Both `non_parametric_alias_scalar_unknown_dimension_produces_error` (task
/// #2766, pinning the inner-arg "NotADim" diagnostic) and
/// `non_parametric_alias_scalar_use_site_emits_unresolved_type_diagnostic`
/// (task #2841, pinning the downstream "unresolved type: Bad" diagnostic)
/// compile the same source but assert different message fragments.  Factoring
/// into a const eliminates the duplication and makes a future rename of the
/// alias or the structure visible in exactly one place.
const SCALAR_BAD_WITH_USE_SITE: &str =
    "type Bad = Scalar<NotADim>\nstructure def Use { param v : Bad }";

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
    assert_error_containing(SCALAR_BAD_WITH_USE_SITE, "NotADim");
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

/// After the alias-DFS pre-pass, the `CompiledTypeAlias` registry entry for
/// `type Bad = Scalar<NotADim>` must have `resolved_type == None`.
///
/// This pins acceptance criterion #1 from task #2841.  `Scalar` has a
/// `resolve_type_name` default (`Type::Scalar { dimension: LENGTH }`).  Before
/// the fix, the `AliasInnerDiagPolicy::Propagate` branch in
/// `resolve_type_alias_expr` propagated `tmp_diags` but then fell through to
/// the simple-name lookup at the bottom of the match arm, which resolved
/// `"Scalar"` (no type args) to `Type::length()` and stored that as the alias
/// entry's `resolved_type`.  Downstream consumers (e.g. `entity.rs` param-type
/// resolution) then saw a wrong-but-typed alias entry and proceeded without the
/// expected `"unresolved type: Bad"` diagnostic — producing a wrong-type
/// cascade.
///
/// After the fix (`return None;` gated on `!tmp_diags.is_empty()` inside the
/// Propagate branch), the alias entry is left with `resolved_type: None`, which
/// is what this test asserts.
///
/// Pre-fix expectation: this assertion FAILS because `resolved_type` is
/// `Some(Scalar { dimension: LENGTH })` (i.e. `Some(Type::length())`).
#[test]
fn non_parametric_alias_scalar_unknown_dimension_leaves_alias_unresolved() {
    let module = compile_with_stdlib_helper("type Bad = Scalar<NotADim>");
    let bad = module
        .type_aliases
        .iter()
        .find(|a| a.name == "Bad")
        .expect("alias 'Bad' not found in module.type_aliases");
    assert!(
        bad.resolved_type.is_none(),
        "expected None, got {:?}",
        bad.resolved_type
    );
}

/// A use-site `param v : Bad` where `type Bad = Scalar<NotADim>` must produce
/// an Error-severity diagnostic containing `"unresolved type: Bad"`.
///
/// This pins acceptance criterion #2 from task #2841.  After the fix, the
/// alias entry for `Bad` has `resolved_type: None`.  When `entity.rs` resolves
/// the param type at line 374-415, it finds no resolved type for `Bad` and
/// emits `"unresolved type: Bad"` (entity.rs:~409).  Before the fix the alias
/// entry held `Some(Type::length())`, so `entity.rs` silently typed `v` as
/// `Length` with no error — a wrong-type cascade.
///
/// This test is causally downstream of
/// `non_parametric_alias_scalar_unknown_dimension_leaves_alias_unresolved` (the
/// alias must be `None` before the use-site diagnostic can fire), but is kept
/// as a separate `#[test]` so a future regression is attributable to either
/// registry state or the use-site diagnostic path independently.
#[test]
fn non_parametric_alias_scalar_use_site_emits_unresolved_type_diagnostic() {
    assert_error_containing(SCALAR_BAD_WITH_USE_SITE, "unresolved type: Bad");
}

/// A parametric alias chain `type Wrapper<T> = List<T>` +
/// `type OuterWrapper<T> = Wrapper<T>` must produce ZERO Error-severity
/// diagnostics mentioning either alias name from the alias-DFS pre-pass.
///
/// This is the Defer-policy regression guard for task #2843.
///
/// (a) This pins acceptance criterion #2 of task #2843 — the Defer policy is
///     honoured for parametric callers.
///
/// (b) During alias-DFS resolution of `OuterWrapper`, the inner type arg `T`
///     cannot resolve because the user-alias branch passes `&empty` as
///     `type_param_names` to `resolve_parameterized_alias`, which writes
///     "unresolved type argument 'T' for alias 'Wrapper'" into `tmp_diags`.
///
/// (c) Under `AliasInnerDiagPolicy::Defer`, that diagnostic must be silently
///     discarded and the alias entry left unresolved (`resolved_type: None`).
///     Substitution at use-site instantiation will resolve `T` correctly via
///     `resolve_type_alias_expr_with_subst`.
///
/// (d) This test WOULD FAIL if the implementation mistakenly extended
///     `tmp_diags` into `diagnostics` unconditionally instead of gating on
///     `inner_diag_policy == AliasInnerDiagPolicy::Propagate`.
///
/// No use-site is needed here: `OuterWrapper` is parametric and never
/// instantiated, so the absence of errors from the DFS pre-pass alone is the
/// assertion under test.
///
/// The filter is scoped to diagnostics whose message mentions `"Wrapper"`,
/// which includes both `"Wrapper"` and `"OuterWrapper"` — the only aliases
/// defined in this fixture.  This insulates the test from any unrelated
/// Error-severity diagnostic that `compile_with_stdlib_helper` might emit
/// from the stdlib in a future version, preventing false failures due to
/// noise outside the contract under test.
#[test]
fn parametric_alias_user_parametric_chain_defers_dfs_diagnostics() {
    let module =
        compile_with_stdlib_helper("type Wrapper<T> = List<T>\ntype OuterWrapper<T> = Wrapper<T>");
    // Filter to diagnostics whose message mentions either alias name in the fixture.
    // "OuterWrapper" is a strict superset of "Wrapper" character-wise, but both
    // are caught by the single `contains("Wrapper")` predicate.
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("Wrapper"))
        .collect();
    assert!(
        errs.is_empty(),
        "expected zero Error-severity diagnostics mentioning 'Wrapper'/'OuterWrapper' \
         from alias-DFS pre-pass of a parametric chain, but got: {errs:?}"
    );
}

/// A non-parametric alias `type Bad = Wrapper<NotAType>` (where
/// `type Wrapper<T> = List<T>` is a user-defined parametric alias) paired with
/// a use-site `structure def Use { param v : Bad }` must produce at least one
/// Error-severity diagnostic that mentions `"NotAType"`.
///
/// This is the primary regression test for task #2843.
///
/// (a) This test pins the task #2843 fix.
///
/// (b) Before the fix, the user-alias-instantiation branch in
///     `resolve_type_alias_expr` (type_resolution.rs:794-815) unconditionally
///     discarded `tmp_diags` from `resolve_parameterized_alias` regardless of
///     `inner_diag_policy`.  The diagnostic "unresolved type argument 'NotAType'
///     for alias 'Wrapper'" written by `resolve_parameterized_alias` into
///     `tmp_diags` was therefore silently dropped during alias-DFS resolution.
///     No Error-severity diagnostic mentioning "NotAType" reached the output —
///     the bug.
///
/// (c) The `structure def Use { param v : Bad }` use-site is included so the
///     alias entry for `Bad` is materialised through the full pipeline, matching
///     the convention of every Error-producing test in this file.
///
/// After the fix (policy-gated `diagnostics.extend(tmp_diags); return None;`
/// inserted between the success path and the silent-discard comment), the
/// inner-arg error surfaces during alias-DFS and the assertion passes.
#[test]
fn non_parametric_alias_user_parametric_unknown_inner_produces_error() {
    assert_error_containing(
        "type Wrapper<T> = List<T>\ntype Bad = Wrapper<NotAType>\nstructure def Use { param v : Bad }",
        "NotAType",
    );
}
