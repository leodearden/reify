//! Compile-structural + oracle-parity pins for the inline geometry-query-argument
//! hoist (task 5345). A whole-handle geometry query (`volume`/`area`/`centroid`/
//! `bounding_box`) whose arg[0] is an INLINE geometry call — e.g.
//! `let v = volume(torus(20mm,5mm))` with no intermediate geometry `let` — must be
//! desugared at compile time into a synthetic geometry `let __geoq_<N>` so the
//! query routes through the identical realized-handle dispatch path as the
//! hand-written let-bound form. This mirrors task 5009's
//! `compile_linear_pattern_2d_nested_target_hoists_into_preceding_op` structural
//! pin (in `compile_api_tests.rs`), applied to a whole-handle query value cell.
//!
//! LAYOUT (C1, PRD `docs/prds/merge-gate-compile-cost.md` §5): this file is a
//! `#[path]`-declared module of the `harness_geometry_solver` compile unit, not
//! a standalone `tests/*.rs` binary — the sanctioned remedy for a NEW test in
//! one of the 5 consolidatable crates. Growing
//! `tests/infra/harness-layout-baseline.manifest` is superseded (esc-5056-11).
//!
//! The e2e numeric acceptance (real-OCCT Volume/Area/Centroid dispatch) lives in
//! `crates/reify-eval/tests/harness_geometry/geometry_query_kernel_dispatch.rs`;
//! this file pins the compile-time IR shape (hoisted `Primitive` op + rewritten
//! value-cell arg) and the oracle name-set, neither of which needs a kernel.

use reify_compiler::{
    CompiledGeometryOp, GEOMETRY_QUERY_NAMES, PrimitiveKind, WHOLE_HANDLE_GEOMETRY_QUERY_NAMES,
    compile,
};
use reify_core::{Severity, ValueCellId};
use reify_ir::CompiledExprKind;

/// `let v = volume(torus(20mm,5mm))` — the inline whole-handle-query form. The
/// hoist must (1) lift the inline `torus(...)` into its own synthetic geometry
/// realization whose op is a `Primitive(Torus)`, and (2) rewrite the `v` value
/// cell's `default_expr` to `volume(<ValueRef to the synthetic cell>)` — NOT a
/// nested `torus(...)` `FunctionCall`, which is exactly what left the inline cell
/// as `Value::Undef` on base (eval's `resolve_geometry_handle_arg` cannot map an
/// inline `FunctionCall` arg to a `named_steps` handle).
#[test]
fn compile_inline_volume_torus_hoists_into_realization() {
    let source = r#"structure def S {
    let v = volume(torus(20mm, 5mm))
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_geoq_inline"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);

    // The fixture must compile with no error-severity diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error-severity diagnostics, got: {:?}",
        errors
    );

    let template = &compiled.templates[0];

    // (1) The inline `torus(...)` hoisted into its own geometry realization. A
    // bare inline query arg yields exactly ONE realization (the synthetic
    // `__geoq_0` torus); `v` is a value cell, not a realization. Asserting
    // exactly 1 also catches a double-realization regression.
    assert_eq!(
        template.realizations.len(),
        1,
        "expected exactly 1 hoisted realization for the inline torus, got {}: {:?}",
        template.realizations.len(),
        template.realizations
    );
    let ops = &template.realizations[0].operations;
    assert!(
        ops.iter().any(|op| matches!(
            op,
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Torus,
                ..
            }
        )),
        "expected a Primitive(Torus) op in the hoisted realization, got {:?}",
        ops
    );

    // (2) The `v` value cell's default_expr is `volume(ValueRef(..))`, NOT
    // `volume(torus(..))`. Locate the cell by id (the synthetic `__geoq_` cell is
    // a realization, never a value cell, so it does not appear here).
    let v_cell = template
        .value_cells
        .iter()
        .find(|c| c.id == ValueCellId::new("S", "v"))
        .expect("value cell S.v should exist");
    let default = v_cell
        .default_expr
        .as_ref()
        .expect("S.v should have a default_expr");
    let CompiledExprKind::FunctionCall { function, args } = &default.kind else {
        panic!(
            "S.v default_expr should be a volume() FunctionCall, got {:?}",
            default.kind
        );
    };
    assert_eq!(
        function.name, "volume",
        "S.v should call volume(), got {}",
        function.name
    );
    assert_eq!(
        args.len(),
        1,
        "volume() should have exactly 1 arg, got {}",
        args.len()
    );
    // The crux of the fix: arg[0] is a `ValueRef` to the hoisted synthetic
    // geometry let, NOT the original inline `torus(...)` `FunctionCall`.
    assert!(
        matches!(args[0].kind, CompiledExprKind::ValueRef(_)),
        "volume() arg[0] must be a ValueRef to the hoisted geometry let, not an \
         inline geometry call; got {:?}",
        args[0].kind
    );
}

/// MIXED fixture (task 5345 review round 3): a structure carrying BOTH real
/// product geometry and an inline geometry query. This is the only shape in
/// which the hoist can change pre-existing behaviour, and the shape in which it
/// regressed: minting `__geoq_0` as an ordinary product realization made it the
/// highest-indexed realization with a terminal handle, so eval's
/// `non_final_realization_indices` put the user's real `body` in the skip set
/// and exported the measurement torus instead — silent geometry loss.
///
/// The compile-side contract that prevents that: BOTH realizations exist, and
/// exactly the synthetic one is flagged `is_query_only`. Asserted in BOTH
/// declaration orders, because the eval-side failure was order-dependent (it
/// only bit when the synthetic realization sorted last).
#[test]
fn mixed_product_geometry_and_inline_query_flags_only_the_synthetic_realization() {
    for (label, source) in [
        (
            "body first",
            r#"structure def Part {
    let body = box(50mm, 50mm, 50mm)
    let v = volume(torus(20mm, 5mm))
}"#,
        ),
        (
            "query first",
            r#"structure def Part {
    let v = volume(torus(20mm, 5mm))
    let body = box(50mm, 50mm, 50mm)
}"#,
        ),
    ] {
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_geoq_mixed"));
        assert!(parsed.errors.is_empty(), "{label}: parse errors");
        let compiled = compile(&parsed);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{label}: unexpected errors: {errors:?}");

        let template = &compiled.templates[0];
        let named: Vec<(&str, bool)> = template
            .realizations
            .iter()
            .map(|r| {
                (
                    r.name.as_deref().expect("compiler always emits a name"),
                    r.is_query_only,
                )
            })
            .collect();

        // The user's real product body survives the hoist...
        assert!(
            named.contains(&("body", false)),
            "{label}: expected a non-query-only `body` realization, got {named:?}"
        );
        // ...alongside exactly one synthetic realization, flagged query-only.
        let synthetic: Vec<_> = named
            .iter()
            .filter(|(n, _)| n.starts_with("__geoq_"))
            .collect();
        assert_eq!(
            synthetic.len(),
            1,
            "{label}: expected exactly 1 synthetic realization, got {named:?}"
        );
        assert!(
            synthetic[0].1,
            "{label}: the synthetic `{}` realization MUST be is_query_only — otherwise it \
             displaces `body` as eval's final realization and the real box vanishes from \
             STEP/STL/3MF; got {named:?}",
            synthetic[0].0
        );
        assert_eq!(
            template.realizations.len(),
            2,
            "{label}: expected exactly 2 realizations, got {named:?}"
        );
    }
}

/// A user-authored member whose name collides with the synthetic `__geoq_`
/// prefix is ordinary product geometry: `is_query_only` is keyed off the
/// desugarer's exact minted-name set, never off a prefix test.
#[test]
fn user_authored_geoq_prefixed_let_is_not_query_only() {
    let source = r#"structure def Part {
    let __geoq_0 = box(50mm, 50mm, 50mm)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_geoq_collide"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1);
    assert!(
        !template.realizations[0].is_query_only,
        "a user-authored `__geoq_`-prefixed let must stay product geometry"
    );
}

/// Oracle parity (task 5345): the compiler's whole-handle-query name-set is
/// EXACTLY {volume, area, centroid, bounding_box}, and every name is a member of
/// the broader `GEOMETRY_QUERY_NAMES`. This ties the compile-time hoist scope to
/// the eval `is_geometry_query_call` handle-dispatch family (which the eval-side
/// `registry_drift_tests` independently pins as a subset of `GEOMETRY_QUERY_NAMES`).
/// A drift in either direction — a query eval can resolve on a handle but the
/// compiler will not hoist, or vice versa — fails here.
#[test]
fn whole_handle_geometry_query_oracle_parity() {
    let mut got: Vec<&str> = WHOLE_HANDLE_GEOMETRY_QUERY_NAMES.to_vec();
    got.sort_unstable();
    let want = ["area", "bounding_box", "centroid", "volume"];
    assert_eq!(
        got.as_slice(),
        want.as_slice(),
        "WHOLE_HANDLE_GEOMETRY_QUERY_NAMES must be exactly the 4 whole-handle queries"
    );
    for name in WHOLE_HANDLE_GEOMETRY_QUERY_NAMES {
        assert!(
            GEOMETRY_QUERY_NAMES.contains(name),
            "whole-handle query {name:?} must also be in GEOMETRY_QUERY_NAMES"
        );
    }
}

// ---------------------------------------------------------------------------
// Binder-scope pins (task 5345 review round 2)
//
// The hoist lifts a matched query's arg[0] VERBATIM out to the STRUCTURE member
// list, so any identifier bound by an enclosing lambda param / quantifier
// variable / match `VariantBind` binder would become UNBOUND at member scope.
// `ExprKind` has exactly three binder-introducing variants (`Lambda`,
// `Quantifier`, `Match` arms), so there are exactly three negative pins below,
// plus one positive over-scoping guard proving the fix suppresses only the
// binder-scoped hoist and never the legitimate member-level one.
//
// HARNESS NOTE — the assertions are deliberately scoped to the two
// HOIST-SPECIFIC invariants ((a) zero `__geoq_` realizations, (b) zero Error
// diagnostics whose message contains "unresolved name") rather than "zero
// Error-severity diagnostics" outright. These tests compile via the bare
// `reify_syntax::parse` + `compile(&parsed)` path with NO prelude / unit
// registry, under which ANY exponent-or-compound unit literal (`1mm^3`, `1m^3`,
// `1mm^2`, even `7850kg/m^3` — a form used throughout `examples/`) emits a
// spurious `unknown unit: <u>` plus a declared-vs-initializer dimension
// mismatch, while the plain `1mm` control is clean. That is a harness artifact
// of the minimal compile entry point (`compile_with_prelude` is the
// registry-carrying entry point — see `user_defined_unit_tests.rs`), NOT a
// product defect and NOT quantifier-specific, so it must not be "fixed" here
// and must not be allowed to make these assertions flaky. The fixtures
// otherwise avoid exponent/compound unit literals where practical.
// ---------------------------------------------------------------------------

/// Parse + compile `source`, returning (every Error-severity diagnostic message,
/// the names of all `__geoq_`-prefixed realizations across all templates).
fn compile_probe(source: &str) -> (Vec<String>, Vec<String>) {
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_geoq_binder"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let errors: Vec<String> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    let hoisted: Vec<String> = compiled
        .templates
        .iter()
        .flat_map(|t| t.realizations.iter())
        .filter_map(|r| r.name.clone())
        .filter(|n| n.starts_with("__geoq_"))
        .collect();
    (errors, hoisted)
}

/// Assert the two hoist-specific invariants: exactly `expected_hoists`
/// `__geoq_` realizations, and zero `unresolved name` Error diagnostics.
fn assert_hoist_scope(what: &str, source: &str, expected_hoists: usize) {
    let (errors, hoisted) = compile_probe(source);
    let unresolved: Vec<&String> = errors
        .iter()
        .filter(|m| m.contains("unresolved name"))
        .collect();
    assert!(
        unresolved.is_empty(),
        "{what}: hoisting out of a binder scope leaked unbound identifiers; \
         expected no \"unresolved name\" diagnostics, got {unresolved:?} \
         (all errors: {errors:?})"
    );
    assert_eq!(
        hoisted.len(),
        expected_hoists,
        "{what}: expected exactly {expected_hoists} `__geoq_` realization(s), \
         got {}: {hoisted:?}",
        hoisted.len()
    );
}

/// `ExprKind::Quantifier` binder: `forall s in sizes: volume(box(s, s, s))`. The
/// quantifier variable `s` is bound only inside the predicate, so hoisting
/// `box(s, s, s)` to the member list makes `s` unbound there. The query under the
/// binder must simply retain the pre-existing base behaviour (`Value::Undef`).
#[test]
fn quantifier_bound_var_is_not_hoisted() {
    let source = r#"structure def G {
    param sizes: List<Length> = [1mm, 2mm]
    let gref = box(2mm, 2mm, 2mm)
    let vmax = volume(gref)
    let ok = forall s in sizes: volume(box(s, s, s)) < vmax
}"#;
    assert_hoist_scope("quantifier-bound `s`", source, 0);
}

/// `ExprKind::Lambda` binder: `flat_map(sizes, |s| [volume(box(s, s, s))])`. The
/// lambda param `s` is bound only inside the lambda body. (The free-function
/// `flat_map(list, |x| [f(x)])` form is used because `map(list, |x| f(x))` does
/// not parse.)
#[test]
fn lambda_param_is_not_hoisted() {
    let source = r#"structure def L {
    param sizes: List<Length> = [1mm, 2mm]
    let vs = flat_map(sizes, |s| [volume(box(s, s, s))])
}"#;
    assert_hoist_scope("lambda-param `s`", source, 0);
}

/// `MatchPattern::VariantBind` binder: a match arm's destructured field binding
/// `Circle { radius: r }` is bound only inside that arm's body.
#[test]
fn match_pattern_binder_is_not_hoisted() {
    let source = r#"enum Sh { Circle { radius: Length }, Flat }

structure def M {
    let shape = Sh.Flat
    let v = match shape {
        Circle { radius: r } => volume(cylinder(r, 1mm)),
        Flat => 0mm^3
    }
}"#;
    assert_hoist_scope("match-pattern binder `r`", source, 0);
}

/// Over-scoping guard: binder opacity must suppress ONLY the binder-scoped
/// hoist. A member-level inline query in the SAME structure as a binder-scoped
/// one still hoists, so exactly one `__geoq_` realization survives here.
#[test]
fn binder_opacity_does_not_suppress_top_level_hoist() {
    let source = r#"structure def J {
    param sizes: List<Length> = [1mm, 2mm]
    let outer = volume(box(1mm, 1mm, 1mm))
    let vs = flat_map(sizes, |s| [volume(box(s, s, s))])
}"#;
    assert_hoist_scope("mixed member-level + lambda-scoped", source, 1);
}
