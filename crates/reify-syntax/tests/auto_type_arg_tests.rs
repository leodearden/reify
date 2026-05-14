//! Tests for `auto:` / `auto(free):` in `type_arg_list` position (task 3526).
//!
//! User-observable signal: `reify_syntax::parse` of a function returning a
//! parameterized type whose type-arg list contains `auto: TraitName` or
//! `auto(free): TraitName` produces zero parse errors.
//!
//! Covers:
//!   * bare `auto:` (strict solver) in a single type-arg position
//!   * `auto(free):` in a single type-arg position
//!   * multiple `auto:` type-args in the same list
//!
//! AST-shape assertions (e.g. the bound identifier is surfaced in TypeExprKind)
//! are deferred to sibling task 3477, which wires the lowering extension.

use reify_types::ModulePath;

#[test]
fn auto_type_arg_parses_strict() {
    let source = "fn f() -> Bearing<auto: Seal> { 0 }";
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected zero parse errors for `auto: Seal` in type-arg position, got: {:?}",
        module.errors,
    );
}

#[test]
fn auto_type_arg_parses_free() {
    let source = "fn g() -> Bearing<auto(free): Seal> { 0 }";
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected zero parse errors for `auto(free): Seal` in type-arg position, got: {:?}",
        module.errors,
    );
}

#[test]
fn auto_type_arg_parses_multi_param() {
    let source = "fn h() -> Coupling<auto: A, auto: B> { 0 }";
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected zero parse errors for `auto: A, auto: B` in type-arg list, got: {:?}",
        module.errors,
    );
}
