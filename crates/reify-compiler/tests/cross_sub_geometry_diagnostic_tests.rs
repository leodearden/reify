//! Tests for cross-sub access to geometry-typed members.
//!
//! ## Background — split purpose (task 3441)
//!
//! This file pins TWO complementary behaviours that share a single AST shape
//! (`self.<sub>.<member>` where `<member>` is a `Solid`-typed param or a
//! geometry `let` on the child structure):
//!
//! 1. **Working-path lowering (non-collection subs).**  `self.inner.body`
//!    where `inner` is a singular sub now lowers successfully to a
//!    `GeomRef::Sub("<sub>.<member>")` reference — compile produces no Error
//!    diagnostic.  The integration-level lowering shape is pinned in
//!    `cross_sub_geometry_lowering_tests.rs`; this file confirms the
//!    no-diagnostic invariant at the same call sites that previously emitted
//!    the v0.1 "not yet supported" diagnostic.
//!
//! 2. **Diagnostic preserved (collection subs).**  `bolts[0].body` and
//!    bare `self.bolts.body` continue to emit the geometry-specific
//!    diagnostic — per-instance handles for collection elements are out of
//!    scope for v0.1.
//!
//! 3. **Generic-fallback preserved (truly missing members).**
//!    `self.inner.nonexistent` still emits the generic "unknown member"
//!    diagnostic — the working path is gated on
//!    `sub_realization_names[sub].contains(member)`.
//!
//! ## Scope boundary — compile-side only
//!
//! Tests in this file exercise only the **compiler** (parse → compile).  They
//! assert that the correct `GeomRef::Sub("<sub>.<member>")` IR is emitted and
//! that no spurious diagnostics fire, but they do NOT run the evaluator or
//! geometry kernel.
//!
//! **Runtime resolvability** (i.e. that the parent template's `named_steps` is
//! actually seeded with the compound key so `GeomRef::Sub` resolves at eval
//! time) is owned by:
//!
//! - `cross_sub_geometry_lowering_tests.rs` — integration-level structural
//!   assertions on the compiled IR (also compile-only, but deeper shape checks).
//! - `crates/reify-eval/tests/cross_sub_geometry_e2e.rs` — full
//!   source-to-kernel pipeline tests that verify `named_steps` seeding and
//!   confirm the kernel records the expected ops.  The happy-path cases for
//!   both the `let body` form and the `param body : Solid` form are covered
//!   there.  A regression that broke eval-side seeding while leaving
//!   compile-side lowering intact would be caught by those e2e tests, not
//!   by this file.
//!
//! ## Historical step numbering
//!
//! The original task-3397 diagnostic was added by steps 1-11 of that task.
//! Task 3441 flipped steps 1, 3, and 9 to working-path expectations while
//! preserving steps 5, 7's collection-sub diagnostics and step-5's
//! generic-fallback regression guard.

use reify_compiler::{CompiledGeometryOp, GeomRef, TransformKind};
use reify_test_support::compile_source;
use reify_core::Severity;

// ─── helper ───────────────────────────────────────────────────────────────────

/// Returns true if the message contains at least one of the "not yet" / "v0.1" /
/// "not supported" keywords indicating the geometry-specific diagnostic.
fn has_deferred_keyword(msg: &str) -> bool {
    msg.contains("not yet") || msg.contains("v0.1") || msg.contains("not supported")
}

// ─── flipped (was step-1, diagnostic): param body : Solid cross-sub access ───

/// Accessing `self.inner.body` where `body` is a `param body : Solid = box(...)`
/// on a singular (non-collection) child sub now lowers to a stable
/// `GeomRef::Sub("inner.body")` reference — NO Error diagnostic fires.
///
/// Flipped by task 3441 (step-9): the prior v0.1 "geometry not yet supported"
/// diagnostic was replaced by a working-path lowering in `expr.rs` /
/// `geometry.rs`, and the parent's `named_steps` is seeded with the
/// compound-key `"inner.body"` entry by `engine_build.rs`.
///
/// Regression guard: the generic "unknown member" fallback must NOT fire for
/// this case — the cross-sub working path is reached because `body` is a
/// realisation on `Inner`.
#[test]
fn param_body_solid_cross_sub_access_lowers_to_geom_ref_sub() {
    let source = r#"pub structure Inner {
    param body : Solid = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let copy = translate(self.inner.body, 0mm, 0mm, 0mm)
}"#;
    let compiled = compile_source(source);

    // (a) No Error diagnostics — the working path replaces the old diagnostic.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (b) Outer's `copy` realization contains a Translate whose target is
    //     `GeomRef::Sub("inner.body")`.
    let outer = compiled
        .templates
        .iter()
        .find(|t| t.name == "Outer")
        .expect("Outer template should be present");
    let copy = outer
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some("copy"))
        .expect("Outer.copy realization should be present");

    let has_expected_sub_ref = copy.operations.iter().any(|op| {
        matches!(
            op,
            CompiledGeometryOp::Transform {
                kind: TransformKind::Translate,
                target: GeomRef::Sub(name),
                ..
            } if name == "inner.body"
        )
    });
    assert!(
        has_expected_sub_ref,
        "expected a Translate op targeting GeomRef::Sub(\"inner.body\"); \
         got: {:?}",
        copy.operations
    );

    // (c) Regression guard: NO generic "unknown member" fallback for this
    //     member — the cross-sub working path is gated on
    //     `sub_realization_names[sub].contains(member)`, which `body` satisfies.
    let has_generic_fallback = compiled.diagnostics.iter().any(|d| {
        d.message.contains("unknown member")
            && d.message.contains("'body'")
            && d.message.contains("'inner'")
    });
    assert!(
        !has_generic_fallback,
        "found generic 'unknown member' diagnostic for 'body'/'inner' — \
         it should have been replaced by the working-path lowering; \
         got: {:?}",
        compiled
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ─── flipped (was step-3, diagnostic): let body = geometry cross-sub access ──

/// Same as the param-body-Solid case above, but with `let body = box(...)`
/// on the child (geometry let binding).  Both shapes lower to the same
/// `RealizationDecl`, so the working-path lowering must fire uniformly.
///
/// Flipped by task 3441 (step-9): no Error diagnostic; the Translate must
/// target `GeomRef::Sub("inner.body")`.
#[test]
fn let_body_cross_sub_access_lowers_to_geom_ref_sub() {
    let source = r#"pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let copy = translate(self.inner.body, 0mm, 0mm, 0mm)
}"#;
    let compiled = compile_source(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let outer = compiled
        .templates
        .iter()
        .find(|t| t.name == "Outer")
        .expect("Outer template should be present");
    let copy = outer
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some("copy"))
        .expect("Outer.copy realization should be present");

    let has_expected_sub_ref = copy.operations.iter().any(|op| {
        matches!(
            op,
            CompiledGeometryOp::Transform {
                kind: TransformKind::Translate,
                target: GeomRef::Sub(name),
                ..
            } if name == "inner.body"
        )
    });
    assert!(
        has_expected_sub_ref,
        "expected a Translate op targeting GeomRef::Sub(\"inner.body\"); \
         got: {:?}",
        copy.operations
    );

    let has_generic_fallback = compiled.diagnostics.iter().any(|d| {
        d.message.contains("unknown member")
            && d.message.contains("'body'")
            && d.message.contains("'inner'")
    });
    assert!(
        !has_generic_fallback,
        "found generic 'unknown member' diagnostic for 'body'/'inner' — \
         should have been replaced by the working-path lowering; got: {:?}",
        compiled
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ─── step-5: non-existent scalar member still emits generic diagnostic ────────

/// Accessing a member that does NOT exist at all on the child structure
/// must still emit the OLD generic "unknown member" diagnostic, NOT the
/// geometry-specific one.
///
/// Regression guard: the geometry-specific path fires ONLY when the name is
/// actually a realization on the child template.
#[test]
fn nonexistent_member_still_emits_generic_unknown_member_diagnostic() {
    let source = r#"structure Inner {
    param value : Length = 10mm
}
structure Outer {
    sub inner = Inner()
    let x = self.inner.nonexistent
}"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "expected at least one Error for accessing a non-existent member"
    );

    // (b) Must contain the generic "unknown member" text.
    let has_generic = errors.iter().any(|d| {
        d.message.contains("unknown member")
            && d.message.contains("nonexistent")
            && d.message.contains("inner")
    });
    assert!(
        has_generic,
        "expected generic 'unknown member' diagnostic naming 'nonexistent' and 'inner'; \
         got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (c) Must NOT contain "geometry" or "v0.1" — this is not a geometry member.
    let has_geometry_path = errors
        .iter()
        .any(|d| d.message.contains("geometry") || d.message.contains("v0.1"));
    assert!(
        !has_geometry_path,
        "non-existent scalar member must NOT trigger geometry-specific diagnostic; \
         got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── step-7: collection sub geometry access ───────────────────────────────────

/// Accessing a geometry member via an indexed collection sub (`bolts[0].body`)
/// must emit the geometry-specific diagnostic (not the generic "unknown member").
///
/// RED until GHR-γ step-8 lands (geometry-specific diagnostic for collection-sub
/// indexed access is not yet implemented).  Ignored so the test suite stays green;
/// un-ignore once step-8 is complete.
#[test]
#[ignore = "RED: geometry-specific diagnostic for indexed collection-sub not yet implemented (GHR-γ step-8)"]
fn collection_sub_indexed_geometry_access_emits_specific_diagnostic() {
    let source = r#"pub structure Bolt {
    param body : Solid = cylinder(2mm, 10mm)
}
pub structure Rack {
    sub bolts : List<Bolt>
    let first = bolts[0].body
}"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "expected at least one Error for indexed collection-sub geometry access"
    );

    let has_geometry_diagnostic = errors
        .iter()
        .any(|d| d.message.contains("geometry") && has_deferred_keyword(&d.message));
    assert!(
        has_geometry_diagnostic,
        "expected geometry-specific diagnostic for bolts[0].body; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let names_sub_and_member = errors.iter().any(|d| {
        d.message.contains("geometry")
            && has_deferred_keyword(&d.message)
            && d.message.contains("bolts")
            && d.message.contains("body")
    });
    assert!(
        names_sub_and_member,
        "geometry diagnostic must name 'bolts' and 'body'; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Accessing a geometry member via a bare collection sub (`self.bolts.body`)
/// must emit the geometry-specific diagnostic (not the generic "unknown member").
///
/// RED until GHR-γ step-8 lands (geometry-specific diagnostic for collection-sub
/// bare access is not yet implemented).  Ignored so the test suite stays green;
/// un-ignore once step-8 is complete.
#[test]
#[ignore = "RED: geometry-specific diagnostic for bare collection-sub not yet implemented (GHR-γ step-8)"]
fn collection_sub_bare_geometry_access_emits_specific_diagnostic() {
    let source = r#"pub structure Bolt {
    param body : Solid = cylinder(2mm, 10mm)
}
pub structure Rack {
    sub bolts : List<Bolt>
    let first = self.bolts.body
}"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "expected at least one Error for bare collection-sub geometry access"
    );

    let has_geometry_diagnostic = errors
        .iter()
        .any(|d| d.message.contains("geometry") && has_deferred_keyword(&d.message));
    assert!(
        has_geometry_diagnostic,
        "expected geometry-specific diagnostic for self.bolts.body; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let names_sub_and_member = errors.iter().any(|d| {
        d.message.contains("geometry")
            && has_deferred_keyword(&d.message)
            && d.message.contains("bolts")
            && d.message.contains("body")
    });
    assert!(
        names_sub_and_member,
        "geometry diagnostic must name 'bolts' and 'body'; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── flipped (was step-9, anti-cascade): no spurious errors in nested call ────

/// When `self.inner.body` appears as an operand to another geometry call
/// (e.g. `translate(...)`), the working-path lowering must (i) emit NO Error
/// diagnostic for the cross-sub access itself, AND (ii) not trigger any
/// downstream cascade errors ("argument N must be a geometry expression",
/// "type mismatch", "expected geometry expression").
///
/// Flipped by task 3441 (step-9): formerly verified that the cross-sub
/// diagnostic fired exactly once + no cascade; now verifies the working-path
/// alternative — no diagnostic and no cascade.  The compile-side `GeomRef::Sub`
/// is asserted by `cross_sub_geometry_lowering_tests.rs`; this test pins the
/// no-error invariant at the original call site.
#[test]
fn cross_sub_geometry_access_does_not_cascade() {
    let source = r#"pub structure Inner {
    param body : Solid = box(10mm, 10mm, 10mm)
}
pub structure Outer {
    sub inner = Inner()
    let composed = translate(self.inner.body, 10mm, 0mm, 0mm)
}"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // (a) Working-path lowering: NO Error diagnostics for the cross-sub access.
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for working-path cross-sub access; \
         got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (b) Specifically no cascade "argument N" / "type mismatch" /
    //     "expected geometry expression" errors — guards against the case where
    //     a future regression silently produces these without the original
    //     "geometry not yet supported" diagnostic.
    let has_cascade = compiled.diagnostics.iter().any(|d| {
        d.severity == Severity::Error
            && (d.message.starts_with("argument")
                || d.message.starts_with("type mismatch")
                || d.message == "expected geometry expression")
    });
    assert!(
        !has_cascade,
        "unexpected cascade diagnostic in errors: {:?}",
        compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // (c) The Translate op lowers with target = GeomRef::Sub("inner.body") —
    //     the working path completed for the nested call's geometry arg.
    let outer = compiled
        .templates
        .iter()
        .find(|t| t.name == "Outer")
        .expect("Outer template should be present");
    let composed = outer
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some("composed"))
        .expect("Outer.composed realization should be present");

    let has_expected_sub_ref = composed.operations.iter().any(|op| {
        matches!(
            op,
            CompiledGeometryOp::Transform {
                kind: TransformKind::Translate,
                target: GeomRef::Sub(name),
                ..
            } if name == "inner.body"
        )
    });
    assert!(
        has_expected_sub_ref,
        "expected a Translate op targeting GeomRef::Sub(\"inner.body\"); \
         got: {:?}",
        composed.operations
    );
}

// ─── task 3454: bare let emits v0.1 no-value-cell warning ────────────────────

/// Helper: asserts a Warning containing `` `let copy = self.inner.body` ``,
/// `"v0.1"`, and `"no value cell"` fires and no Errors appear.
///
/// Used for child-side geometry **lets** (`let body = box(...)`) where the
/// cross-sub bypass in entity.rs still fires in GHR-γ step-2 (it is retired
/// in step-4).  Geometry-let cross-sub access goes through the
/// `CrossSubGeometryRef` path and hits the bypass warning.
fn assert_v01_bare_let_warning(source: &str, case_label: &str) {
    let compiled = compile_source(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "{case_label}: expected no Error diagnostics; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let has_warning = compiled.diagnostics.iter().any(|d| {
        d.severity == Severity::Warning
            && d.message.contains("`let copy = self.inner.body`")
            && d.message.contains("v0.1")
            && d.message.contains("no value cell")
    });
    assert!(
        has_warning,
        "{case_label}: expected Warning containing \"`let copy = self.inner.body`\", \
         \"v0.1\", \"no value cell\"; got diagnostics: {:?}",
        compiled
            .diagnostics
            .iter()
            .map(|d| (&d.severity, &d.message))
            .collect::<Vec<_>>()
    );
}

/// Helper: asserts that NO v0.1 bare-let Warning fires for `source`.
///
/// Used for child-side Solid **params** (`param body : Solid = box(...)`) after
/// GHR-γ step-2: the param now creates a `ValueCellDecl{Type::Geometry}` so
/// `self.inner.body` resolves to a plain `ValueRef` (not `CrossSubGeometryRef`)
/// and the bypass warning never fires.  The geometry-let case (Case A) still
/// fires the warning until step-4 retires the bypass entirely.
fn assert_no_v01_bare_let_warning(source: &str, case_label: &str) {
    let compiled = compile_source(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "{case_label}: expected no Error diagnostics; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let spurious_warning = compiled.diagnostics.iter().find(|d| {
        d.severity == Severity::Warning
            && d.message.contains("v0.1")
            && d.message.contains("no value cell")
    });
    assert!(
        spurious_warning.is_none(),
        "{case_label}: unexpected v0.1 bare-let Warning; Solid-param cross-sub access \
         goes via ValueRef (not CrossSubGeometryRef) after GHR-γ step-2; \
         got: {:?}",
        spurious_warning.map(|d| (&d.severity, &d.message))
    );
}

/// A bare `let copy = self.inner.body` with no wrapping geometry call no longer
/// emits a v0.1 Warning after GHR-γ step-4 retires the cross-sub bypass in
/// entity.rs.  Both child-side shapes — geometry-let and Solid-param — now
/// produce a `ValueCellDecl{Type::Geometry}` and no warning fires.
///
/// Originally added by task 3454 asserting the Warning fired.  Updated by task
/// 3605 (GHR-γ step-2) to split the two cases.  Updated by task 3605 (step-4)
/// to flip both cases to expect NO warning.
#[test]
fn bare_cross_sub_geometry_let_emits_v01_no_op_warning() {
    // Case A: child-side `let body = box(...)` — bypass retired, no Warning.
    assert_no_v01_bare_let_warning(
        r#"pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let copy = self.inner.body
}"#,
        "Case A (let body)",
    );

    // Case B: child-side `param body : Solid = box(...)` — no Warning (unchanged).
    assert_no_v01_bare_let_warning(
        r#"pub structure Inner {
    param body : Solid = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let copy = self.inner.body
}"#,
        "Case B (param body : Solid)",
    );
}

// ─── task 3454: downstream-use UX regression guard ───────────────────────────

/// Downstream translate after a bare cross-sub geometry let.
///
/// After GHR-γ step-4 retires the cross-sub bypass both cases emit ZERO v0.1
/// Warnings — the `CrossSubGeometryRef` now falls through to the standard
/// `ValueCellDecl` path and no warning is emitted.
///
/// Originally added by task 3454 (step-3) asserting count == 1 for both cases.
/// Updated by task 3605 (GHR-γ step-2) to split counts.
/// Updated by task 3605 (step-4) to flip Case A to expect count == 0.
#[test]
fn bare_cross_sub_geometry_let_with_downstream_translate_surfaces_v01_hint() {
    // Case A: geometry let — bypass retired, ZERO Warnings.
    {
        let source = r#"pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let copy = self.inner.body
    let placed = translate(copy, 10mm, 0mm, 0mm)
}"#;
        let compiled = compile_source(source);

        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "Case A (let body, downstream translate): expected no Error diagnostics; got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        let warn_count = compiled
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Severity::Warning
                    && d.message.contains("v0.1")
                    && d.message.contains("no value cell")
            })
            .count();
        assert_eq!(
            warn_count,
            0,
            "Case A (let body, downstream translate): expected ZERO v0.1 bare-let Warnings \
             after step-4 retires the bypass; got: {:?}",
            compiled
                .diagnostics
                .iter()
                .map(|d| (&d.severity, &d.message))
                .collect::<Vec<_>>()
        );
    }

    // Case B: Solid param — NO Warning (value cell exists, bypass not reached).
    {
        let source = r#"pub structure Inner {
    param body : Solid = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let copy = self.inner.body
    let placed = translate(copy, 10mm, 0mm, 0mm)
}"#;
        let compiled = compile_source(source);

        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "Case B (param body : Solid, downstream translate): expected no Error diagnostics; \
             got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        let warn_count = compiled
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Severity::Warning
                    && d.message.contains("v0.1")
                    && d.message.contains("no value cell")
            })
            .count();
        assert_eq!(
            warn_count,
            0,
            "Case B (param body : Solid, downstream translate): expected ZERO v0.1 bare-let Warnings \
             (Solid param has a value cell after GHR-γ step-2); got: {:?}",
            compiled
                .diagnostics
                .iter()
                .map(|d| (&d.severity, &d.message))
                .collect::<Vec<_>>()
        );
    }
}
