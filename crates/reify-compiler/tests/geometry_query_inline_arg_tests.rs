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
