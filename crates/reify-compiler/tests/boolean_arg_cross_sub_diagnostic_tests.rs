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

use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};
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

    // (c) Independent regression guard: the generic "must be a geometry expression"
    //     fallback must be entirely ABSENT — the early `return None` in
    //     `resolve_boolean_arg` (triggered when `try_emit_cross_sub_geometry`
    //     returns `Some`) suppresses the generic path.  This check is independent
    //     of (b): (b) asserts the specific diagnostic fires; (c) asserts the
    //     generic fallback does NOT fire at all.
    let has_any_generic_fallback = errors
        .iter()
        .any(|d| d.message.contains("must be a geometry expression"));
    assert!(
        !has_any_generic_fallback,
        "generic 'must be a geometry expression' must be absent when the specific \
         cross-sub-deferred diagnostic fires — the early return in resolve_boolean_arg \
         should suppress it; errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── step-3: n-ary boolean op with collection-sub geometry arg ────────────────

/// When an **n-ary** boolean op's argument (`union_all`) is
/// `self.<collection_sub>.<member>` where `<member>` is a geometry-typed
/// realization, the specific v0.1 deferred diagnostic must fire — not the
/// generic fallback.
///
/// The collection-sub arg is in the **middle** position (arg index 2, 1-based)
/// of the `union_all(a, self.bolts.body, b)` call, exercising the loop-iter
/// branch of `compile_boolean_op` rather than the first-arg branch.  Both
/// paths share `resolve_boolean_arg`, so step-2's fix covers both; this test
/// functions as an explicit regression guard so a future refactor that bypasses
/// `resolve_boolean_arg` for n-ary args breaks visibly.
///
/// Passes on arrival after step-2.
#[test]
fn nary_boolean_op_with_collection_sub_geometry_arg_emits_specific_diagnostic() {
    let source = r#"pub structure Bolt {
    param body : Solid = cylinder(2mm, 10mm)
}
pub structure Rack {
    sub bolts : List<Bolt>
    param a : Solid = box(5mm, 5mm, 5mm)
    param b : Solid = box(6mm, 6mm, 6mm)
    let combined = union_all(a, self.bolts.body, b)
}"#;
    // Dependency note: arg-0 (`a`, a `param : Solid`) must resolve successfully
    // via the geometry-let/param path in `resolve_boolean_arg` for the n-ary fold
    // loop in `compile_boolean_op` to proceed and reach `self.bolts.body` (arg-1).
    // If `a` ever stopped resolving, the `?` on the first-arg resolve would
    // short-circuit before the cross-sub diagnostic fires — the test would fail,
    // not silently pass.  Should this become unexpectedly flaky due to
    // param/geometry-let classification changes, consider moving the collection-sub
    // arg to arg-0 position in a separate test.
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // (a) At least one Error-severity diagnostic fires.
    assert!(
        !errors.is_empty(),
        "expected at least one Error diagnostic for collection-sub geometry arg in union_all(); \
         got no diagnostics"
    );

    // (b) At least one Error is the specific cross-sub-deferred diagnostic naming
    //     both 'bolts' and 'body'.
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

    // (c) Independent regression guard: the generic "must be a geometry expression"
    //     fallback must be entirely ABSENT — the early `return None` in
    //     `resolve_boolean_arg` suppresses the generic path when the specific
    //     deferred diagnostic fires.  This check is independent of (b): (b) asserts
    //     the specific diagnostic fires; (c) asserts the generic fallback does NOT
    //     fire at all (including for arg-0 or arg-2 which resolved correctly).
    let has_any_generic_fallback = errors
        .iter()
        .any(|d| d.message.contains("must be a geometry expression"));
    assert!(
        !has_any_generic_fallback,
        "generic 'must be a geometry expression' must be absent when the specific \
         cross-sub-deferred diagnostic fires in union_all() n-ary path; errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── step-4: scalar-member falls back to generic diagnostic ───────────────────

/// When a boolean op argument is `self.<non_collection_sub>.<scalar_member>`,
/// `try_emit_cross_sub_geometry` returns `None` (the member is not in
/// `sub_realization_names`, so it is not a geometry realization) and the
/// existing generic "argument N must be a geometry expression" fallback fires.
///
/// Pins the conditional gate in `try_emit_cross_sub_geometry` on
/// `sub_realization_names`: a future refactor that broadens the diagnostic to
/// any cross-sub shape (dropping the realization-name guard) would cause the
/// cross-sub deferred wording to fire for `value` on `inner`, breaking this test.
///
/// Passes on arrival after step-2 (the helper's gate correctly excludes scalar
/// members not in `sub_realization_names`).
#[test]
fn boolean_op_with_non_realization_scalar_member_falls_back_to_generic_diagnostic() {
    let source = r#"pub structure Inner {
    param value : Scalar = 5mm
}
pub structure Outer {
    sub inner = Inner()
    param base : Solid = box(10mm, 10mm, 10mm)
    let combined = difference(self.inner.value, base)
}"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // (a) The generic "argument 1 must be a geometry expression" fallback fires.
    let has_generic_fallback = errors
        .iter()
        .any(|d| d.message.contains("argument 1 must be a geometry expression"));
    assert!(
        has_generic_fallback,
        "expected generic 'argument 1 must be a geometry expression' for scalar member \
         in boolean arg position; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (b) The cross-sub deferred diagnostic must NOT fire for 'value' on 'inner'
    //     — sub_realization_names[inner] does not contain 'value' (it is a scalar
    //     param, not a geometry realization), so try_emit_cross_sub_geometry
    //     returns None and falls through to the generic path.
    let has_spurious_deferred_diagnostic = errors.iter().any(|d| {
        d.message.contains("value") && has_deferred_keyword(&d.message)
    });
    assert!(
        !has_spurious_deferred_diagnostic,
        "cross-sub deferred diagnostic ('not yet supported in v0.1') must NOT fire for \
         scalar member 'value' on sub 'inner'; the realization-name gate in \
         try_emit_cross_sub_geometry should exclude it. Got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── step-2 (gap 1): binary boolean op — collection-sub geometry RIGHT operand ──

/// When a **binary** boolean op's **right** argument (`args[1]`) is
/// `self.<collection_sub>.<member>` where `<member>` is a geometry-typed
/// realization on the child structure, the compiler must emit the specific v0.1
/// cross-sub deferred diagnostic — not the generic "argument 2 must be a
/// geometry expression" fallback.
///
/// This test covers the **distinct** `resolve_boolean_arg(&args[1], …)` call
/// site at `geometry_boolean.rs:169`, which is separate from the left-operand
/// call at line 155.  A refactor that bypasses `resolve_boolean_arg` for the
/// right operand would break this test visibly.
///
/// Dependency note: `args[0]` (`base`, a `param : Solid`) must resolve
/// successfully via `resolve_boolean_arg` for control to reach `args[1]`.
/// The proof that `param : Solid` resolves correctly is the existing n-ary
/// test above (arg-0 `a` is also a `param : Solid`).  If `base` ever stopped
/// resolving the `?` would short-circuit before the right-operand diagnostic
/// fires — this test would fail loudly, not silently pass.
///
/// GREEN on arrival: `resolve_boolean_arg` is the shared helper for both
/// operands; task-3512's routing (impl at `geometry_boolean.rs:64-85`) already
/// handles this shape.  This test is a regression guard so future changes that
/// bypass the shared helper for the right operand are caught immediately.
#[test]
fn binary_boolean_op_with_collection_sub_geometry_right_operand_emits_specific_diagnostic() {
    let source = r#"pub structure Bolt {
    param body : Solid = cylinder(2mm, 10mm)
}
pub structure Rack {
    sub bolts : List<Bolt>
    param base : Solid = box(10mm, 10mm, 10mm)
    let combined = union(base, self.bolts.body)
}"#;
    // args[0] = `base` (param : Solid) — resolves via geometry-param path.
    // args[1] = `self.bolts.body` (collection-sub member) — exercises the
    //           right-operand resolve_boolean_arg(&args[1], …) call site.
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // (a) At least one Error-severity diagnostic fires.
    assert!(
        !errors.is_empty(),
        "expected at least one Error diagnostic for collection-sub geometry right operand \
         in union(); got no diagnostics"
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
        "expected the specific cross-sub-deferred geometry diagnostic naming 'bolts' and 'body' \
         for the right operand of union(); got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (c) Independent regression guard: the generic "must be a geometry expression"
    //     fallback must be entirely ABSENT.  The early `return None` in
    //     `resolve_boolean_arg` (triggered when `try_emit_cross_sub_geometry`
    //     returns `Some`) suppresses the generic path.  (b) asserts the specific
    //     diagnostic fires; (c) asserts the generic fallback does NOT fire at all.
    let has_any_generic_fallback = errors
        .iter()
        .any(|d| d.message.contains("must be a geometry expression"));
    assert!(
        !has_any_generic_fallback,
        "generic 'must be a geometry expression' must be absent when the specific \
         cross-sub-deferred diagnostic fires for the right operand — the early return \
         in resolve_boolean_arg should suppress it; errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── step-2 (gap 2): indexed collection-sub member falls back to generic ───────

/// When a boolean op argument is `self.<collection_sub>[i].<member>`, the outer
/// object of the `MemberAccess` is an `IndexAccess` rather than another
/// `MemberAccess`.  This shape is **intentionally out of scope** for the
/// task-3512 routing block (`geometry_boolean.rs:59-63` scope-boundary comment).
///
/// Trace for `union(base, self.bolts[0].body)`:
/// - `args[1]` parses as `MemberAccess { object: IndexAccess{…}, member: "body" }`.
/// - `try_resolve_cross_sub_geom_ref` requires the outer object to be a
///   `MemberAccess` (the `self.<sub>.<member>` shape) → fails → returns `None`.
/// - The task-3512 routing block also requires `outer_obj = MemberAccess` →
///   skipped.
/// - `compile_geometry_call` matches `FunctionCall` shapes only; the
///   `MemberAccess` kind returns `None` without recursing into the value-level
///   `try_emit_cross_sub_geometry` in `expr.rs`.
/// - Fallback fires: "argument 2 must be a geometry expression".
///
/// This is a **negative** regression guard.  A future refactor that broadens
/// boolean-arg routing to the indexed `self.<sub>[i].<member>` shape would flip
/// assertion (b) — that's the signal to revisit this boundary deliberately.
///
/// Passes on arrival (the indexed form already falls through to the generic
/// diagnostic).
#[test]
fn boolean_op_with_indexed_collection_sub_member_falls_back_to_generic_diagnostic() {
    let source = r#"pub structure Bolt {
    param body : Solid = cylinder(2mm, 10mm)
}
pub structure Rack {
    sub bolts : List<Bolt>
    param base : Solid = box(10mm, 10mm, 10mm)
    let combined = union(base, self.bolts[0].body)
}"#;
    // args[0] = `base` (param : Solid) — resolves; control reaches args[1].
    // args[1] = `self.bolts[0].body` — MemberAccess{ object: IndexAccess{…}, member: "body" }
    //           The outer object is IndexAccess, NOT MemberAccess, so the
    //           task-3512 routing block is skipped and the generic fallback fires.
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // (a) The generic "argument 2 must be a geometry expression" fallback fires.
    //     The "2" matches the right-operand position: args[0]=base (resolves),
    //     args[1]=self.bolts[0].body (indexed form, falls through to generic).
    let has_generic_fallback = errors
        .iter()
        .any(|d| d.message.contains("argument 2 must be a geometry expression"));
    assert!(
        has_generic_fallback,
        "expected generic 'argument 2 must be a geometry expression' for indexed \
         collection-sub member in boolean arg position; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (b) The cross-sub deferred diagnostic must NOT fire for the indexed form.
    //     The task-3512 routing block (geometry_boolean.rs:59-63) is out-of-scope
    //     for IndexAccess outer objects; try_resolve_cross_sub_geom_ref + the
    //     routing block both require a MemberAccess outer_obj and
    //     compile_geometry_call returns None at its FunctionCall arm without
    //     recursing into the value-level try_emit_cross_sub_geometry.
    let has_spurious_deferred_diagnostic = errors.iter().any(|d| {
        has_deferred_keyword(&d.message)
            && (d.message.contains("bolts") || d.message.contains("body"))
    });
    assert!(
        !has_spurious_deferred_diagnostic,
        "cross-sub deferred diagnostic must NOT fire for indexed form \
         'self.bolts[0].body' — geometry_boolean.rs:59-63 scope-boundary limits \
         the task-3512 routing to MemberAccess outer objects only. Got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── step-5: working-path cross-sub arg lowers without diagnostic ─────────────

/// When a boolean op argument is `self.<non_collection_sub>.<body>` where `body`
/// IS a geometry realization on the child structure, `try_resolve_cross_sub_geom_ref`
/// in step-1 of `resolve_boolean_arg` succeeds and returns `GeomRef::Sub("inner.body")`.
/// The new routing inserted in step-2 of `resolve_boolean_arg` must NOT fire for
/// this case — it only runs after `try_resolve_cross_sub_geom_ref` returns `None`.
///
/// Asserts:
/// (a) NO Error-severity diagnostics — the working path completes silently.
/// (b) `Outer.combined.operations` contains a `CompiledGeometryOp::Boolean` whose
///     `left` is `GeomRef::Sub("inner.body")` — the compound-key lowering is intact.
///
/// Passes both today (before step-2) and after step-2, because the new routing
/// is gated behind the `try_resolve_cross_sub_geom_ref` early return.
/// Closes the regression surface against accidentally calling
/// `try_emit_cross_sub_geometry` for working-path arms.
#[test]
fn boolean_op_with_working_path_cross_sub_arg_lowers_without_diagnostic() {
    let source = r#"pub structure Inner {
    let body = box(10mm, 10mm, 10mm)
}
pub structure Outer {
    sub inner = Inner()
    param base : Solid = box(20mm, 20mm, 20mm)
    let combined = union(self.inner.body, base)
}"#;
    let compiled = compile_source(source);

    // (a) No Error diagnostics — the working path lowers silently.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for working-path cross-sub boolean arg; \
         got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (b) Outer.combined lowers to a Boolean op whose left is GeomRef::Sub("inner.body").
    let outer = compiled
        .templates
        .iter()
        .find(|t| t.name == "Outer")
        .expect("Outer template should be present");
    let combined = outer
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some("combined"))
        .expect("Outer.combined realization should be present");

    let has_expected_boolean = combined.operations.iter().any(|op| {
        matches!(
            op,
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Union,
                left: GeomRef::Sub(name),
                ..
            } if name == "inner.body"
        )
    });
    assert!(
        has_expected_boolean,
        "expected a Boolean(Union) op with left=GeomRef::Sub(\"inner.body\") in \
         Outer.combined; got: {:?}",
        combined.operations
    );
}
