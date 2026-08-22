//! Direct unit tests for `reify_expr::find_matching_compiled_function`.
//!
//! The helper is a small, pure first-match-wins overload-resolution function
//! shared by two call sites:
//!   - `eval_user_function_call` in `reify-expr/src/lib.rs`
//!   - the `@optimized` → `ComputeNode` lowering site in `reify-eval/src/engine_eval.rs`
//!
//! These tests exercise the five distinct behaviour axes of the matcher in
//! isolation, independent of the end-to-end eval paths covered by
//! `lambda_eval_tests.rs` and `compute_dispatch_registry.rs`.

use reify_expr::{EvalContext, eval_expr, find_matching_compiled_function};
use reify_core::{ContentHash, DimensionVector, Type};
use reify_ir::{CompiledExpr, CompiledFnBody, CompiledFunction, TypeParam, Value, ValueMap};

/// Build a minimal `CompiledFunction` with the given name and a single
/// parameter of the given type. The body is a constant `Int(0)` literal —
/// irrelevant for matching purposes.
fn make_fn(name: &str, param_type: Type) -> CompiledFunction {
    let params = vec![("x".to_string(), param_type)];
    CompiledFunction {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: Type::Int,
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::literal(Value::Int(0), Type::Int),
        },
        content_hash: ContentHash::of(name.as_bytes()),
        annotations: vec![],
        optimized_target: None,
        type_params: vec![],
    }
}

/// Build a minimal `CompiledFunction` with the given name and NO parameters.
fn make_fn_nullary(name: &str) -> CompiledFunction {
    let params: Vec<(String, Type)> = vec![];
    CompiledFunction {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: Type::Int,
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::literal(Value::Int(0), Type::Int),
        },
        content_hash: ContentHash::of(name.as_bytes()),
        annotations: vec![],
        optimized_target: None,
        type_params: vec![],
    }
}

/// Build a `CompiledFunction` with an explicit type-param list and an
/// arbitrary number of parameters.
///
/// Both [`make_fn`] and [`make_fn_nullary`] hard-code `type_params: vec![]` and
/// (for `make_fn`) a single param, so the *wildcard* pass of
/// `find_matching_compiled_function` — which is gated on
/// `!type_params.is_empty()` for type-param- and dim-param-carrying params — is
/// unreachable through them. This builder is what makes that pass, and the
/// multi-candidate overload sets that exercise its tie-breaks, testable.
///
/// `tag` seeds `content_hash` so a specific candidate is identifiable in
/// assertions when several same-name overloads are in the table — the same
/// identification trick `first_match_wins` already uses.
///
/// The `TypeParam { name, bounds: vec![], default: None }` literal shape
/// follows the inline test `find_matching_resolves_generic_field_param_call`
/// in `crates/reify-expr/src/lib.rs`.
fn make_generic_fn(
    name: &str,
    type_param_names: &[&str],
    params: Vec<(String, Type)>,
    tag: &[u8],
) -> CompiledFunction {
    CompiledFunction {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: Type::Int,
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::literal(Value::Int(0), Type::Int),
        },
        content_hash: ContentHash::of(tag),
        annotations: vec![],
        optimized_target: None,
        type_params: type_param_names
            .iter()
            .map(|n| TypeParam {
                name: n.to_string(),
                bounds: vec![],
                default: None,
            })
            .collect(),
    }
}

/// Build the REAL merged prelude function table, exactly as the compiler
/// assembles it for a `.ri` file with no user functions.
///
/// `CompiledModule.functions` is user-source-only, so the stdlib bodies are
/// only reachable by flattening `load_stdlib()` and running it through
/// `merge_prelude_functions` — which dedups by the
/// `(name, arity, param_types)` triple rather than by name alone, and is
/// therefore what preserves the same-named `Option` / `Result` overload pairs
/// (`unwrap_or`, `or_else`, `fallback`) *and* their declaration order.
///
/// Tests built on this are higher-fidelity than synthetic fixtures: they fail
/// if the real `.ri` signatures drift out from under the fixtures.
///
/// `reify-compiler` is already a `[dev-dependencies]` entry of `reify-expr`,
/// and both symbols are `pub` — no manifest change is needed for this.
fn stdlib_overload_table() -> Vec<CompiledFunction> {
    let prelude: Vec<CompiledFunction> = reify_compiler::stdlib_loader::load_stdlib()
        .iter()
        .flat_map(|m| m.functions.iter().cloned())
        .collect();
    reify_compiler::merge_prelude_functions(&[], &prelude)
}

/// The `Option` half of a stdlib recovery-combinator pair:
/// `fn <name><T>(o: Option<T>, dflt: T)` — the shape declared in
/// `stdlib/option_recovery.ri`.
fn option_overload(name: &str) -> CompiledFunction {
    make_generic_fn(
        name,
        &["T"],
        vec![
            (
                "o".to_string(),
                Type::Option(Box::new(Type::TypeParam("T".to_string()))),
            ),
            ("dflt".to_string(), Type::TypeParam("T".to_string())),
        ],
        b"option_overload",
    )
}

/// The `Result` half of a stdlib recovery-combinator pair:
/// `fn <name><T, E>(r: Result<T, E>, dflt: T)` — the shape declared in
/// `stdlib/result.ri`, which compiles to an `Applied { name: "Result", .. }`
/// param.
fn result_overload(name: &str) -> CompiledFunction {
    make_generic_fn(
        name,
        &["T", "E"],
        vec![
            (
                "r".to_string(),
                Type::Applied {
                    name: "Result".to_string(),
                    args: vec![
                        Type::TypeParam("T".to_string()),
                        Type::TypeParam("E".to_string()),
                    ],
                },
            ),
            ("dflt".to_string(), Type::TypeParam("T".to_string())),
        ],
        b"result_overload",
    )
}

/// Build the argument list for a stdlib recovery combinator.
///
/// `or_else` takes two same-shaped subjects (`(o: Option<T>, alt: Option<T>)` /
/// `(r: Result<T,E>, alt: Result<T,E>)`); `unwrap_or` and `fallback` take a
/// subject plus a plain default.
fn recovery_args(name: &str, subject: Type) -> Vec<CompiledExpr> {
    match name {
        "or_else" => vec![
            CompiledExpr::literal(Value::Undef, subject.clone()),
            CompiledExpr::literal(Value::Undef, subject),
        ],
        _ => vec![
            CompiledExpr::literal(Value::Undef, subject),
            CompiledExpr::literal(Value::Undef, Type::length()),
        ],
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Test: name mismatch → None
// ────────────────────────────────────────────────────────────────────────────

/// When no function in the slice has the requested name, the helper returns
/// `None` even if arity and types would otherwise match.
#[test]
fn name_mismatch_returns_none() {
    let fns = vec![make_fn("foo", Type::Int)];
    // arg has the right type and arity, but name "bar" doesn't exist
    let args = [CompiledExpr::literal(Value::Int(0), Type::Int)];
    let result = find_matching_compiled_function(&fns, "bar", &args);
    assert!(result.is_none(), "expected None for name mismatch");
}

// ────────────────────────────────────────────────────────────────────────────
// Test: arity mismatch → None
// ────────────────────────────────────────────────────────────────────────────

/// When the function exists by name but takes a different number of parameters,
/// the helper returns `None`.
#[test]
fn arity_mismatch_returns_none() {
    // function takes 1 param; we pass 0 args
    let fns = vec![make_fn("foo", Type::Int)];
    let args: [CompiledExpr; 0] = [];
    let result = find_matching_compiled_function(&fns, "foo", &args);
    assert!(result.is_none(), "expected None for arity mismatch (0 args vs 1 param)");

    // function takes 0 params; we pass 1 arg
    let fns_nullary = vec![make_fn_nullary("bar")];
    let args_one = [CompiledExpr::literal(Value::Int(0), Type::Int)];
    let result2 = find_matching_compiled_function(&fns_nullary, "bar", &args_one);
    assert!(result2.is_none(), "expected None for arity mismatch (1 arg vs 0 params)");
}

// ────────────────────────────────────────────────────────────────────────────
// Test: per-param type mismatch → None
// ────────────────────────────────────────────────────────────────────────────

/// When name and arity match but the parameter type differs from the arg's
/// `result_type`, the helper returns `None`.
///
/// Concretely: function expects `Type::Int`, arg carries `Type::dimensionless_scalar()`.
#[test]
fn param_type_mismatch_returns_none() {
    // function param is Type::Int
    let fns = vec![make_fn("foo", Type::Int)];
    // arg result_type is Type::dimensionless_scalar()  → should not match
    let args = [CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar())];
    let result = find_matching_compiled_function(&fns, "foo", &args);
    assert!(result.is_none(), "expected None for per-param type mismatch (Int param vs Real arg)");
}

// ────────────────────────────────────────────────────────────────────────────
// Test: exact match → Some
// ────────────────────────────────────────────────────────────────────────────

/// When name, arity, and all per-param types match exactly, the helper returns
/// `Some` pointing to the matching `CompiledFunction`.
#[test]
fn exact_match_returns_some() {
    let f = make_fn("foo", Type::Int);
    let fns = vec![f.clone()];
    let args = [CompiledExpr::literal(Value::Int(42), Type::Int)];
    let result = find_matching_compiled_function(&fns, "foo", &args);
    assert!(result.is_some(), "expected Some for exact match");
    assert_eq!(result.unwrap().name, "foo");
}

// ────────────────────────────────────────────────────────────────────────────
// Test: first-match-wins ordering
// ────────────────────────────────────────────────────────────────────────────

/// When multiple functions in the slice all satisfy name + arity + type
/// constraints, the helper returns the *first* one in iteration order.
///
/// This test uses two functions with the same name and compatible signature
/// but different `content_hash` values so we can identify which one was
/// returned.
#[test]
fn first_match_wins() {
    // Two functions, both named "foo", both taking a single Int param.
    let mut first = make_fn("foo", Type::Int);
    first.content_hash = ContentHash::of(b"first");

    let mut second = make_fn("foo", Type::Int);
    second.content_hash = ContentHash::of(b"second");

    let fns = vec![first.clone(), second];

    let args = [CompiledExpr::literal(Value::Int(0), Type::Int)];
    let result = find_matching_compiled_function(&fns, "foo", &args);

    assert!(result.is_some(), "expected Some");
    assert_eq!(
        result.unwrap().content_hash,
        first.content_hash,
        "expected the first matching function to be returned"
    );
}

/// Tier 1 (exact) outranks tier 2 (head) and tier 3 (wildcard) regardless of
/// TABLE ORDER: a candidate matching every param by exact equality wins even
/// when a wildcard-eligible, head-matching candidate is declared BEFORE it.
///
/// This was structurally guaranteed while tier 1 was a separate full pass over
/// `fns`. All three tiers now share ONE scan, so the property is carried by
/// `find_matching_compiled_function` returning early ONLY on an exact hit while
/// tiers 2 and 3 merely record their first candidate — an "optimization" that
/// returned early on a head match too would silently pick the generic overload
/// here. Nothing else in this file pins it, so it is pinned here.
#[test]
fn later_exact_candidate_beats_an_earlier_head_matching_generic() {
    let generic_first = make_generic_fn(
        "f",
        &["T"],
        vec![(
            "x".to_string(),
            Type::Option(Box::new(Type::TypeParam("T".to_string()))),
        )],
        b"tier_order_generic_option",
    );
    let exact_second = make_generic_fn(
        "f",
        &[],
        vec![("x".to_string(), Type::Option(Box::new(Type::length())))],
        b"tier_order_exact_option_length",
    );
    let fns = vec![generic_first, exact_second.clone()];
    let args = [CompiledExpr::literal(
        Value::Undef,
        Type::Option(Box::new(Type::length())),
    )];

    let selected = find_matching_compiled_function(&fns, "f", &args)
        .expect("both candidates match; this must resolve");
    assert_eq!(
        selected.content_hash, exact_second.content_hash,
        "an exact Option<Length> candidate must win over a head-matching generic \
         Option<T> candidate declared before it — tier 1 outranks tier 2 \
         irrespective of table order"
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Characterization: the wildcard pass (pass 2)
//
// The four tests below pin the CURRENT behaviour of the wildcard pass. They
// pass against the matcher as it stands and are the regression net proving
// that any later tie-break tier only ever NARROWS this pass — never widens it,
// never re-admits a candidate this pass rejects.
//
// If one of these ever starts failing, the wildcard pass itself changed shape;
// the correct response is to understand why, not to relax the assertion.
// ────────────────────────────────────────────────────────────────────────────

/// Axis 1 — type-param wildcard. A GENERIC candidate whose param merely
/// *carries* a type param (`Option<T>`, not a bare `T`) is selected for a
/// concrete `Option<Length>` arg, even though the two types are not equal.
#[test]
fn wildcard_pass_generic_type_param_param_matches_concrete_arg() {
    let f = make_generic_fn(
        "f",
        &["T"],
        vec![(
            "o".to_string(),
            Type::Option(Box::new(Type::TypeParam("T".to_string()))),
        )],
        b"generic_option_t",
    );
    let fns = vec![f.clone()];
    let args = [CompiledExpr::literal(
        Value::Undef,
        Type::Option(Box::new(Type::length())),
    )];

    let result = find_matching_compiled_function(&fns, "f", &args);
    assert_eq!(
        result.map(|f| f.content_hash),
        Some(f.content_hash),
        "a generic Option<T> param should wildcard-match a concrete Option<Length> arg"
    );
}

/// Axis 2 — dimension-param wildcard. A GENERIC candidate whose param is a
/// bare `ScalarParam` is selected for a concrete `Length` arg
/// (`type_carries_dim_param`), the dimension-generic sibling of axis 1.
#[test]
fn wildcard_pass_generic_dim_param_param_matches_concrete_arg() {
    let f = make_generic_fn(
        "f",
        &["Q"],
        vec![("q".to_string(), Type::ScalarParam("Q".to_string()))],
        b"generic_scalar_q",
    );
    let fns = vec![f.clone()];
    let args = [CompiledExpr::literal(Value::Undef, Type::length())];

    let result = find_matching_compiled_function(&fns, "f", &args);
    assert_eq!(
        result.map(|f| f.content_hash),
        Some(f.content_hash),
        "a generic ScalarParam param should wildcard-match a concrete Length arg"
    );
}

/// Axis 3 — the type-param wildcard is GATED ON GENERICITY. The identical
/// `Option<T>` param from axis 1, on a candidate with an EMPTY `type_params`
/// list, is NOT selected for the same `Option<Length>` arg.
///
/// This is the load-bearing half of the axis-1 pin: it proves the wildcard
/// comes from the candidate being generic, not from `Option<T>` being special.
#[test]
fn wildcard_pass_type_param_wildcard_is_gated_on_genericity() {
    // Same param type as axis 1, but `type_params` is empty.
    let f = make_generic_fn(
        "f",
        &[],
        vec![(
            "o".to_string(),
            Type::Option(Box::new(Type::TypeParam("T".to_string()))),
        )],
        b"nongeneric_option_t",
    );
    let fns = vec![f];
    let args = [CompiledExpr::literal(
        Value::Undef,
        Type::Option(Box::new(Type::length())),
    )];

    assert!(
        find_matching_compiled_function(&fns, "f", &args).is_none(),
        "an Option<T> param on a NON-generic candidate must not wildcard-match \
         a concrete Option<Length> arg"
    );
}

/// Axis 4 — the trait-object wildcard is NOT gated on genericity. A NON-generic
/// candidate whose param is `List<Load>` IS selected for a concrete
/// `List<StructureRef("PointLoad")>` arg.
///
/// This mirrors compile-side `resolve_function_overload`, which applies the
/// trait-object wildcard to every candidate regardless of genericity. It is the
/// axis that carries the FEA
/// `solve_elastic_static(loads: List<Load>, supports: List<Support>)`
/// signature (esc-4093-152), so it is the one most at risk from a narrowing
/// change to this pass.
#[test]
fn wildcard_pass_trait_object_wildcard_is_ungated_on_genericity() {
    let f = make_generic_fn(
        "f",
        &[],
        vec![(
            "loads".to_string(),
            Type::List(Box::new(Type::TraitObject("Load".to_string()))),
        )],
        b"nongeneric_list_trait",
    );
    let fns = vec![f.clone()];
    let args = [CompiledExpr::literal(
        Value::Undef,
        Type::List(Box::new(Type::StructureRef("PointLoad".to_string()))),
    )];

    let result = find_matching_compiled_function(&fns, "f", &args);
    assert_eq!(
        result.map(|f| f.content_hash),
        Some(f.content_hash),
        "a List<Load> trait-object param on a NON-generic candidate should \
         wildcard-match a concrete List<PointLoad> arg"
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Option-vs-Result overload disambiguation
//
// `unwrap_or`, `or_else` and `fallback` are each declared TWICE in the stdlib:
// once over `Option<T>` (std.option_recovery) and once over `Result<T, E>`
// (std.result). Both declarations are generic, so under the wildcard pass
// pinned above BOTH are eligible for ANY subject — selection degenerates to
// first-match-wins on table order, and std.option_recovery loads first.
//
// The compile side does not have this problem: `resolve_function_overload`
// disambiguates on the subject's constructor head. These tests assert the
// eval-side matcher reaches the same answer.
//
// They assert on the SELECTED CompiledFunction's first param type, never on an
// evaluated Value: reify-expr intercepts all three names via
// `option_recovery::is_combinator` before the matcher is consulted, and
// dispatches on the runtime `Value::Enum` tag, so no evaluated value differs
// either side of this fix. Overload selection is the only observable seam, and
// it is the seam the divergence lives on.
// ────────────────────────────────────────────────────────────────────────────

/// An erased `Enum("Result")` subject — the form variant construction actually
/// produces, since `Ok { .. }` / `Err { .. }` type-erase their result — must
/// select the `Result` overload, not the `Option` one.
#[test]
fn erased_result_subject_selects_result_overload() {
    let fns = vec![option_overload("unwrap_or"), result_overload("unwrap_or")];
    let args = recovery_args("unwrap_or", Type::Enum("Result".to_string()));

    let selected = find_matching_compiled_function(&fns, "unwrap_or", &args)
        .expect("an erased Result subject should resolve to some overload");
    assert!(
        matches!(&selected.params[0].1, Type::Applied { name, .. } if name == "Result"),
        "an erased Enum(\"Result\") subject must select the Result overload; \
         got params[0] = {:?}",
        selected.params[0].1
    );
}

/// A fully-applied `Result<Length, String>` subject must select the `Result`
/// overload too.
///
/// The wildcard pass swallows the non-erased form as readily as the erased one:
/// it never inspects the arg's constructor head, so `Option<T>` matches an
/// `Applied { name: "Result", .. }` arg just as happily.
#[test]
fn applied_result_subject_selects_result_overload() {
    let fns = vec![option_overload("unwrap_or"), result_overload("unwrap_or")];
    let subject = Type::Applied {
        name: "Result".to_string(),
        args: vec![Type::length(), Type::String],
    };
    let args = recovery_args("unwrap_or", subject);

    let selected = find_matching_compiled_function(&fns, "unwrap_or", &args)
        .expect("an applied Result subject should resolve to some overload");
    assert!(
        matches!(&selected.params[0].1, Type::Applied { name, .. } if name == "Result"),
        "an Applied Result<Length, String> subject must select the Result overload; \
         got params[0] = {:?}",
        selected.params[0].1
    );
}

/// The mirror-image direction: with the table order REVERSED (Result declared
/// first), an `Option<Length>` subject must still select the `Option` overload.
///
/// This is what proves the fix is head-driven rather than a re-ordering: a
/// change that merely preferred `Result` would pass the two tests above and
/// fail this one.
#[test]
fn option_subject_selects_option_overload_despite_reversed_table_order() {
    let fns = vec![result_overload("unwrap_or"), option_overload("unwrap_or")];
    let args = recovery_args("unwrap_or", Type::Option(Box::new(Type::length())));

    let selected = find_matching_compiled_function(&fns, "unwrap_or", &args)
        .expect("an Option subject should resolve to some overload");
    assert!(
        matches!(&selected.params[0].1, Type::Option(_)),
        "an Option<Length> subject must select the Option overload even when the \
         Result overload is declared first; got params[0] = {:?}",
        selected.params[0].1
    );
}

/// Fall-through pin: a subject whose head matches NEITHER overload still
/// resolves, to the first wildcard-eligible candidate in table order.
///
/// This locks the contract that head-based narrowing is a filter WITHIN the
/// wildcard set with fall-through when that filter is empty — never a
/// replacement for it. Without the fall-through, adding head narrowing would
/// silently turn "resolved by wildcard" into `None` for every subject the head
/// check cannot classify.
#[test]
fn subject_matching_no_head_falls_through_to_first_wildcard_candidate() {
    let option_fn = option_overload("unwrap_or");
    let fns = vec![option_fn.clone(), result_overload("unwrap_or")];
    // Enum("Option") heads-matches neither Option<T> (a constructor, not an
    // enum ref) nor Applied{"Result", ..} (different name).
    let args = recovery_args("unwrap_or", Type::Enum("Option".to_string()));

    let selected = find_matching_compiled_function(&fns, "unwrap_or", &args)
        .expect("a head-unclassifiable subject must still fall through to the wildcard pass");
    assert_eq!(
        selected.content_hash, option_fn.content_hash,
        "with an empty head-match set the matcher must fall through to the first \
         wildcard-eligible candidate in table order"
    );
}

/// Screening pin: a head match ALONE never selects. A candidate the head
/// predicate accepts but the wildcard predicate rejects must resolve to `None`.
///
/// Sibling of the fall-through pin above; together they pin both directions of
/// the "head narrows WITHIN the wildcard set" contract. That one covers an
/// EMPTY head-match set (narrowing must not turn a resolution into `None`);
/// this one covers the case that makes screening `head` through `wildcard`
/// LOAD-BEARING — see the comment of that name in
/// `crates/reify-expr/src/lib.rs`. `heads_unifiable` is NOT a subset of the
/// wildcard predicate, so a standalone head pass would WIDEN resolution instead
/// of narrowing it.
///
/// `recover<U>(r: Result<Int, String>)` is the witness. Its param's args are
/// concrete, so it carries no type param, no dim param and no trait object, and
/// `Applied { name: "Result", .. }` is never `==` an `Enum("Result")` arg — the
/// wildcard pass rejects it. But the candidate IS generic and
/// `heads_unifiable(Applied { name: "Result", .. }, Enum("Result"))` is `true`
/// via the erased-subject arm, so the head predicate accepts it. A refactor to
/// the more obvious `.find(head).or_else(|| ..find(wildcard))` shape — which
/// that comment explicitly anticipates and argues against — flips this case
/// from `None` to `Some`, accepting eval-side a call the compile side does not,
/// while every other test in this file stays green.
#[test]
fn head_match_alone_never_selects_a_wildcard_ineligible_candidate() {
    let head_only = make_generic_fn(
        "recover",
        &["U"],
        vec![(
            "r".to_string(),
            Type::Applied {
                name: "Result".to_string(),
                args: vec![Type::Int, Type::String],
            },
        )],
        b"head_only",
    );
    let fns = vec![head_only];
    let args = vec![CompiledExpr::literal(
        Value::Undef,
        Type::Enum("Result".to_string()),
    )];

    let selected = find_matching_compiled_function(&fns, "recover", &args);
    assert!(
        selected.is_none(),
        "a candidate that head-matches but is NOT wildcard-eligible must never \
         be selected — `head` is a filter over the wildcard set, never a \
         standalone pass; got params[0] = {:?}",
        selected.map(|f| f.params[0].1.clone())
    );
}

/// Prelude-backed: against the REAL merged stdlib table, an erased
/// `Enum("Result")` subject selects the `Result` overload for all three shared
/// combinator names.
///
/// Higher fidelity than the synthetic fixtures above — this fails if the real
/// `.ri` signatures ever drift out from under them.
#[test]
fn prelude_erased_result_subject_selects_result_overload() {
    let table = stdlib_overload_table();
    for name in ["unwrap_or", "or_else", "fallback"] {
        let args = recovery_args(name, Type::Enum("Result".to_string()));
        let selected = find_matching_compiled_function(&table, name, &args)
            .unwrap_or_else(|| panic!("{name}: no overload resolved for an erased Result subject"));
        assert!(
            matches!(&selected.params[0].1, Type::Applied { name: n, .. } if n == "Result"),
            "{name}: an erased Enum(\"Result\") subject must select the Result overload; \
             got params[0] = {:?}",
            selected.params[0].1
        );
    }
}

/// Prelude-backed regression guard: an `Option<Length>` subject must keep
/// selecting the `Option` overload for all three shared names.
///
/// Passes today; its job is to catch a fix that disambiguates in the wrong
/// direction.
#[test]
fn prelude_option_subject_selects_option_overload() {
    let table = stdlib_overload_table();
    for name in ["unwrap_or", "or_else", "fallback"] {
        let args = recovery_args(name, Type::Option(Box::new(Type::length())));
        let selected = find_matching_compiled_function(&table, name, &args)
            .unwrap_or_else(|| panic!("{name}: no overload resolved for an Option subject"));
        assert!(
            matches!(&selected.params[0].1, Type::Option(_)),
            "{name}: an Option<Length> subject must select the Option overload; \
             got params[0] = {:?}",
            selected.params[0].1
        );
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Mixed generic / non-generic overload sets
// ────────────────────────────────────────────────────────────────────────────

/// Head narrowing can drop a GENERIC candidate and thereby promote a
/// NON-generic one that table order had kept behind it.
///
/// The per-candidate no-op corollary — head narrowing never DROPS a non-generic
/// candidate, because for `type_params.is_empty()` the head predicate is a
/// superset of the wildcard one — is what keeps entirely-non-generic overload
/// sets (`solve_elastic_static`, `solve_load_cases`, `displacement_at`)
/// bit-for-bit unchanged. It says nothing about a MIXED set, which is what this
/// test pins: a generic `f<T>(x: Option<T>)` declared FIRST loses to a
/// non-generic `f(x: List<Load>)` for a `List<PointLoad>` arg, because only the
/// latter head-matches. Both candidates are wildcard-eligible, so before head
/// narrowing existed first-match-wins handed this to the generic one.
///
/// The promotion is the intended answer, not a tolerated side effect: it is
/// what compile-side `resolve_function_overload` resolves to — pinned, not
/// merely asserted in prose, by
/// `overload_mixed_set_head_mismatched_generic_yields_to_non_generic_trait_object`
/// in `crates/reify-compiler/src/type_compat.rs`, which runs the same two
/// candidates through the compiler's own ladder. (That half must live
/// compiler-side: `resolve_function_overload` is `pub(crate)` in a private
/// module.) Recorded as a test so the behaviour is pinned rather than
/// rediscovered from a future escalation — it sits directly adjacent to the
/// esc-4093-152 trait-object path.
#[test]
fn mixed_set_head_mismatched_generic_yields_to_non_generic_trait_object() {
    let generic_first = make_generic_fn(
        "f",
        &["T"],
        vec![(
            "x".to_string(),
            Type::Option(Box::new(Type::TypeParam("T".to_string()))),
        )],
        b"mixed_generic_option",
    );
    let non_generic_second = make_generic_fn(
        "f",
        &[],
        vec![(
            "x".to_string(),
            Type::List(Box::new(Type::TraitObject("Load".to_string()))),
        )],
        b"mixed_nongeneric_trait",
    );
    let fns = vec![generic_first, non_generic_second.clone()];
    let args = [CompiledExpr::literal(
        Value::Undef,
        Type::List(Box::new(Type::StructureRef("PointLoad".to_string()))),
    )];

    let selected = find_matching_compiled_function(&fns, "f", &args)
        .expect("both candidates are wildcard-eligible, so this must resolve");
    assert_eq!(
        selected.content_hash, non_generic_second.content_hash,
        "a List<PointLoad> arg must select the non-generic List<Load> candidate \
         even though a wildcard-eligible generic Option<T> candidate is declared \
         first — only the former head-matches"
    );
}

// ────────────────────────────────────────────────────────────────────────────
// End-to-end: the selected overload's BODY is what evaluates
// ────────────────────────────────────────────────────────────────────────────

/// The stdlib names this tier is motivated by (`unwrap_or` / `or_else` /
/// `fallback`) are all intercepted by `option_recovery::is_combinator` before
/// `eval_user_function_call` consults the matcher, so for THOSE names no
/// evaluated value differs either side of the fix — which is why every test
/// above asserts on the selected `CompiledFunction` instead.
///
/// That leaves the tier's user-visible effect uncovered: a *user-defined*
/// generic overload pair on a non-intercepted name reaches the matcher, and the
/// candidate it picks is the body that runs. This test closes that gap by
/// giving the two overloads DISTINCT constant bodies and asserting on the
/// evaluated `Value` — so a regression that selected the wrong overload for
/// real user code fails here, not merely in a fixture-level assertion.
///
/// Both args must be non-`Undef`: `eval_user_function_call` short-circuits to
/// `Undef` if any evaluated arg is undef, which would mask the selection.
#[test]
fn end_to_end_eval_runs_the_head_matched_overload_body() {
    let len_5mm = Value::Scalar {
        si_value: 0.005,
        dimension: DimensionVector::LENGTH,
    };
    let len_0mm = Value::Scalar {
        si_value: 0.0,
        dimension: DimensionVector::LENGTH,
    };

    // `pick_recovered` is deliberately NOT one of the intercepted combinator
    // names, so the call reaches `find_matching_compiled_function`.
    let mut option_fn = option_overload("pick_recovered");
    option_fn.body.result_expr = CompiledExpr::literal(Value::Int(1), Type::Int);
    let mut result_fn = result_overload("pick_recovered");
    result_fn.body.result_expr = CompiledExpr::literal(Value::Int(2), Type::Int);

    // Option overload FIRST, mirroring stdlib load order (std.option_recovery
    // precedes std.result) — the order that made the erased-Result subject
    // resolve to the Option overload.
    let functions = vec![option_fn, result_fn];
    let values = ValueMap::new();
    let ctx = EvalContext::new(&values, &functions);

    let eval_with = |subject: CompiledExpr| {
        eval_expr(
            &CompiledExpr::user_function_call(
                "pick_recovered".to_string(),
                vec![
                    subject,
                    CompiledExpr::literal(len_0mm.clone(), Type::length()),
                ],
                Type::Int,
            ),
            &ctx,
        )
    };

    // Erased `Enum("Result")` subject — the form `Ok { .. }` actually produces.
    let ok_subject = CompiledExpr::literal(
        Value::Enum {
            type_name: "Result".to_string(),
            variant: "Ok".to_string(),
            payload: vec![("value".to_string(), len_5mm.clone())],
        },
        Type::Enum("Result".to_string()),
    );
    assert_eq!(
        eval_with(ok_subject),
        Value::Int(2),
        "an erased Result subject must run the Result overload's body; Int(1) \
         means the Option overload was bound and evaluated instead"
    );

    // Mirror direction: an Option subject must still run the Option body.
    let some_subject = CompiledExpr::literal(
        Value::Option(Some(Box::new(len_5mm))),
        Type::Option(Box::new(Type::length())),
    );
    assert_eq!(
        eval_with(some_subject),
        Value::Int(1),
        "an Option<Length> subject must run the Option overload's body"
    );
}
