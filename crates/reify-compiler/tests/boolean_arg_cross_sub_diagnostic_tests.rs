//! Tests for cross-sub geometry diagnostics at boolean operation argument positions.
//!
//! ## Background (task 3512)
//!
//! When a boolean-op argument (`union`, `intersection`, `difference`, `union_all`,
//! `intersection_all`) is a `self.<sub>.<member>` cross-sub access that the working
//! path (`try_resolve_cross_sub_geom_ref`) cannot lower — because `<sub>` is a
//! **collection** sub — the boolean-arg site should emit the same specific v0.1
//! deferred diagnostic that the value-level call sites in `expr.rs` emit via
//! `try_emit_cross_sub_geometry`, rather than the generic fallback
//! "argument N must be a geometry expression".
//!
//! ## Scope boundary — compile-side only
//!
//! Tests in this file exercise only the **compiler** (parse → compile).  They
//! assert that the correct diagnostics are emitted for near-miss cross-sub boolean
//! args, and that the working-path lowering (`self.<sub>.<member>` for a
//! non-collection sub's realised geometry member) is preserved unchanged.
//!
//! **Runtime resolvability** is owned by:
//! - `cross_sub_geometry_lowering_tests.rs` — integration-level structural
//!   assertions on the compiled IR.
//! - `crates/reify-eval/tests/cross_sub_geometry_e2e.rs` — full
//!   source-to-kernel pipeline tests.
//!
//! ## Relationship to existing test files
//!
//! `cross_sub_geometry_diagnostic_tests.rs` covers value-level call sites
//! (`let copy = self.inner.body` shapes) — mixing boolean-arg-position coverage
//! there would mix concerns.  This file keeps the boolean-op call-site coverage
//! atomic and discoverable by name when a boolean-op regression occurs.

use reify_test_support::compile_source;
use reify_types::Severity;

// ─── helper ───────────────────────────────────────────────────────────────────

/// Returns true if the message contains at least one of the "not yet" / "v0.1" /
/// "not supported" keywords indicating the geometry-specific deferred diagnostic
/// from `make_cross_sub_geometry_error`.
fn has_deferred_keyword(msg: &str) -> bool {
    msg.contains("not yet") || msg.contains("v0.1") || msg.contains("not supported")
}

// ─── step-1: binary boolean op with collection-sub geometry arg ───────────────

/// When a **binary** boolean op's argument is `self.<collection_sub>.<member>`
/// where `<member>` is a geometry-typed realization on the child structure, the
/// compiler must emit the specific v0.1 cross-sub deferred diagnostic (from
/// `try_emit_cross_sub_geometry` / `make_cross_sub_geometry_error`), not the
/// generic "argument N must be a geometry expression" fallback.
///
/// RED until task-3512 step-2 (impl) lands: before the fix, `resolve_boolean_arg`
/// routes collection-sub `MemberAccess` args through `compile_geometry_call` which
/// silently returns `None`, triggering only the bare generic fallback diagnostic.
///
/// After step-2, `resolve_boolean_arg` pattern-matches the `self.<sub>.<member>`
/// shape and routes through `try_emit_cross_sub_geometry` which emits the
/// geometry-specific deferred diagnostic naming the sub and member.
#[test]
fn binary_boolean_op_with_collection_sub_geometry_arg_emits_specific_diagnostic() {
    let source = r#"pub structure Bolt {
    param body : Solid = cylinder(2mm, 10mm)
}
pub structure Rack {
    sub bolts : List<Bolt>
    param base : Solid = box(10mm, 10mm, 10mm)
    let combined = union(self.bolts.body, base)
}"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // (a) At least one Error-severity diagnostic fires.
    assert!(
        !errors.is_empty(),
        "expected at least one Error diagnostic for collection-sub geometry arg in union(); \
         got no diagnostics"
    );

    // (b) At least one Error is the specific cross-sub-deferred diagnostic naming
    //     both the sub ('bolts') and the member ('body').
    let has_specific_diagnostic = errors.iter().any(|d| {
        d.message.contains("geometry")
            && has_deferred_keyword(&d.message)
            && d.message.contains("bolts")
            && d.message.contains("body")
    });
    assert!(
        has_specific_diagnostic,
        "expected the specific cross-sub-deferred geometry diagnostic naming 'bolts' and 'body'; \
         got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (c) Regression guard: the generic fallback "argument 1 must be a geometry
    //     expression" must not be the ONLY diagnostic — a specific deferred
    //     diagnostic must also be present when the generic fires.  Pins that the
    //     fix does not accidentally leave only the generic message.
    let has_generic_without_specific = errors
        .iter()
        .any(|d| d.message.contains("argument 1 must be a geometry expression"))
        && !has_specific_diagnostic;
    assert!(
        !has_generic_without_specific,
        "generic 'argument 1 must be a geometry expression' fired without the specific \
         cross-sub-deferred diagnostic — the routing through try_emit_cross_sub_geometry \
         is missing or not firing; errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
