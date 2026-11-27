//! Contract tests for [`reify_test_support::prelude_backed_functions`] — task
//! 5593, PRD docs/prds/v0_6/placeholder-type-eradication-ratchet.md §8 BT10.
//!
//! The helper's whole purpose is to reproduce the PRODUCTION runtime dispatch
//! so that `e2e_*_with_stdlib` guards evaluate the way `reify_eval` does. Two
//! table shapes dispatch identically, and they are NOT the same list:
//!
//!  - UNFILTERED: `module.functions.clone()` then `extend(prelude)`. This is
//!    the helper's shape, and reify-eval's runtime shape until task #5524
//!    replaced it with `reify_eval::DefEnv::function_table`.
//!  - FILTERED: one function per `(name, param_types)` signature, first wins.
//!    This is `reify_eval::DefEnv::function_table`, the table production
//!    evaluation uses now, and `reify_compiler::merge_prelude_functions`, the
//!    COMPILE-TIME overload-resolution table, where a duplicate signature is an
//!    ambiguous-overload error.
//!
//! The two are dispatch-EQUIVALENT under first-match-wins (a shadowed prelude
//! entry is unreachable either way), so the helper's choice is one of fidelity,
//! not behaviour. These tests pin the helper's own unfiltered shape.

use reify_ir::CompiledFunction;

/// Total number of functions across every module `load_stdlib()` returns.
fn prelude_fn_count() -> usize {
    reify_compiler::stdlib_loader::load_stdlib()
        .iter()
        .map(|m| m.functions.len())
        .sum()
}

/// `(name, params)` identity for a compiled fn — `CompiledFunction` itself
/// derives only `Debug, Clone` (reify-ir/src/expr.rs:355), so equality has to be
/// spelled out. `Type` does implement `PartialEq`, which is what
/// `merge_prelude_functions`' own shadow predicate relies on.
fn sig(f: &CompiledFunction) -> (String, Vec<reify_core::Type>) {
    (
        f.name.clone(),
        f.params.iter().map(|(_, t)| t.clone()).collect(),
    )
}

/// (a) SHADOWING ORDER — user functions must come FIRST, in their original
/// order, with the prelude appended after.
///
/// `reify_expr::eval_user_function_call` resolves via a first-match-wins linear
/// scan, so ordering IS the shadowing mechanism: a user fn whose signature
/// matches a prelude fn must win precisely because it is scanned first. That is
/// why `reify_eval::DefEnv::function_table` keeps precedence order too.
#[test]
fn user_functions_come_first_in_original_order() {
    let module = reify_test_support::compile_source_with_stdlib(
        "fn user_a(x: Length) -> Length { x }\n\
         fn user_b(y: Length) -> Length { y }\n\
         structure S { let v = user_a(1mm) }",
    );
    let table = reify_test_support::prelude_backed_functions(&module);
    let n = module.functions.len();

    assert!(
        n >= 2,
        "fixture must contribute at least the two user fns; got {n} \
         (module.functions = {:?})",
        module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    assert!(
        table.len() > n,
        "prelude entries must be appended after the user fns"
    );

    let head: Vec<_> = table[..n].iter().map(sig).collect();
    let user: Vec<_> = module.functions.iter().map(sig).collect();
    assert_eq!(
        head, user,
        "the first module.functions.len() entries must be module.functions, in \
         order — first-match-wins dispatch is what makes user fns shadow prelude \
         fns"
    );
}

/// (b) UNFILTERED APPEND — a prelude entry whose signature COLLIDES with a user
/// fn must still be PRESENT in the table (merely unreachable): the helper's
/// unfiltered shape rather than `merge_prelude_functions`.
///
/// The fixture redefines `through<T>(x: T) -> T`, whose `(name, arity,
/// param_types)` triple is exactly that of the stdlib's
/// `pub fn through<T>(x: T) -> T { x }` (crates/reify-compiler/stdlib/fields.ri:68).
///
/// RED against step-2's `merge_prelude_functions` implementation, which filters
/// the colliding prelude entry out and so produces a table one entry short.
#[test]
fn colliding_prelude_entry_is_appended_not_filtered() {
    let module = reify_test_support::compile_source_with_stdlib(
        "fn through<T>(x: T) -> T { x }\n\
         structure S { let v = through(1mm) }",
    );
    let table = reify_test_support::prelude_backed_functions(&module);

    // Precondition: the fixture really does collide with the stdlib `through`.
    let user_through: Vec<_> = module
        .functions
        .iter()
        .filter(|f| f.name == "through")
        .collect();
    assert_eq!(
        user_through.len(),
        1,
        "fixture precondition: exactly one user `through` must compile; got {:?}",
        module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    let prelude_through = reify_compiler::stdlib_loader::load_stdlib()
        .iter()
        .flat_map(|m| m.functions.iter())
        .filter(|f| f.name == "through" && f.params.len() == 1)
        .collect::<Vec<_>>();
    assert_eq!(
        prelude_through.len(),
        1,
        "fixture precondition: the stdlib must define exactly one `through`/1"
    );
    assert_eq!(
        sig(user_through[0]),
        sig(prelude_through[0]),
        "fixture precondition: the user `through` must SHADOW the stdlib one, \
         i.e. share its (name, arity, param_types) triple — otherwise this test \
         is not exercising the collision path at all"
    );

    assert_eq!(
        table.len(),
        module.functions.len() + prelude_fn_count(),
        "unfiltered append: EVERY prelude entry must be present, including the \
         one shadowed by the user `through`. The helper appends unconditionally \
         because first-match-wins makes shadowed entries unreachable anyway; \
         merge_prelude_functions instead FILTERS them, which is correct for the \
         compile-time overload table but is not the helper's shape"
    );

    // ...and the shadowed duplicate is genuinely in there, twice.
    let through_entries = table.iter().filter(|f| f.name == "through").count();
    assert_eq!(
        through_entries, 2,
        "both the user `through` and the shadowed prelude `through` must appear"
    );
}

/// (c) CONTENTS — with a fixture that defines no user `fn` at all, the table is
/// exactly the flattened prelude and contains the combinators the 17
/// `e2e_*_with_stdlib` guards depend on, plus the `through` liveness control.
///
/// Deliberately asserts on NAMED PRESENCE and relative order, never on the total
/// count: the measured baseline was `module.functions.len() == 0` with a merged
/// `table.len() == 76`, but 76 is a moving stdlib-size number and pinning it
/// would make this test fail on every unrelated stdlib addition.
#[test]
fn table_contains_the_guarded_stdlib_combinators() {
    let module = reify_test_support::compile_source_with_stdlib("structure S { let v = 1mm }");
    let table = reify_test_support::prelude_backed_functions(&module);

    assert_eq!(
        module.functions.len(),
        0,
        "fixture precondition: no user fns, so the table is purely the prelude"
    );
    assert!(!table.is_empty(), "prelude-backed table must be non-empty");

    for name in [
        "unwrap_or",
        "or_else",
        "or_default",
        "fallback",
        "is_some",
        "is_none",
        "get_or",
        "is_ok",
        "is_err",
        "ok_or",
        "map_or",
        "map_err",
        "through",
    ] {
        assert!(
            table.iter().any(|f| f.name == name),
            "prelude-backed table must register the stdlib `.ri` fn `{name}` so \
             its placeholder body genuinely competes with the reify-expr \
             intercept; table has {} entries",
            table.len()
        );
    }

    assert_eq!(
        table.len(),
        prelude_fn_count(),
        "with zero user fns the table must be exactly the flattened prelude"
    );
}
