//! Tests for improved diagnostics on cross-sub access to geometry-typed members.
//!
//! ## Background
//!
//! `self.<sub>.<member>` fails with a generic "unknown member" error when `<member>`
//! is a `Solid`-typed param or a geometry `let` — because geometry-producing
//! members are lowered as `RealizationDecl`s and never appear in `value_cells`,
//! which is the only source `sub_member_types` maps are built from.
//!
//! ## Scope (v0.1)
//!
//! Full cross-sub geometry composition is deferred. Instead, the compiler emits
//! a specific, actionable diagnostic when the missing member IS a realization on
//! the child template, distinguishing it from genuinely-missing scalar members.
//!
//! ## Test step numbering
//!
//! - Step 1 (test):  `param_body_solid_cross_sub_access_emits_specific_diagnostic`
//! - Step 2 (impl):  add `sub_realization_names` and route to specific diagnostic
//! - Step 3 (test):  `let_body_cross_sub_access_emits_specific_diagnostic`
//! - Step 4 (impl):  verify let-body case (no-op if step-3 passes after step-2)
//! - Step 5 (test):  `nonexistent_member_still_emits_generic_unknown_member_diagnostic`
//! - Step 6 (impl):  verify generic-path short-circuit (no-op if step-5 passes)
//! - Step 7 (test):  `collection_sub_*_geometry_access_emits_specific_diagnostic`
//! - Step 8 (impl):  collection-sub branches
//! - Step 9 (test):  `cross_sub_geometry_access_does_not_cascade`
//! - Step 10 (impl): anti-cascade verification (no-op if step-9 passes)
//! - Step 11 (impl): docs/reify-language-spec.md §8.3 note

use reify_test_support::compile_source;
use reify_types::Severity;

// ─── helper ───────────────────────────────────────────────────────────────────

/// Returns true if the message contains the substring `needle` (case-sensitive).
fn msg_contains(msg: &str, needle: &str) -> bool {
    msg.contains(needle)
}

/// Returns true if the message contains at least one of the "not yet" / "v0.1" /
/// "not supported" keywords indicating the geometry-specific diagnostic.
fn has_deferred_keyword(msg: &str) -> bool {
    msg_contains(msg, "not yet")
        || msg_contains(msg, "v0.1")
        || msg_contains(msg, "not supported")
}

// ─── step-1: param body : Solid cross-sub access ─────────────────────────────

/// Accessing `self.inner.body` where `body` is a `param body : Solid = box(...)`
/// on a child structure must emit a *specific*, actionable diagnostic — not the
/// generic "unknown member 'body' on sub 'inner'" fallback.
///
/// Expected diagnostic shape (keyword-matched, not pinned to exact prose):
///   - severity == Error
///   - message contains "geometry"
///   - message contains "not yet" or "v0.1" or "not supported"
///   - message mentions "inner" and "body"
///   - NO diagnostic contains the old generic text "unknown member 'body' on sub 'inner'"
///
/// RED until step-2 lands.
#[test]
fn param_body_solid_cross_sub_access_emits_specific_diagnostic() {
    let source = r#"pub structure Inner {
    param body : Solid = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let copy = self.inner.body
}"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "expected at least one Error diagnostic for `self.inner.body` (Solid param access)"
    );

    // (b) The message should contain "geometry" AND a "not yet"/"v0.1"/"not supported" keyword.
    let has_geometry_diagnostic = errors.iter().any(|d| {
        msg_contains(&d.message, "geometry") && has_deferred_keyword(&d.message)
    });
    assert!(
        has_geometry_diagnostic,
        "expected a diagnostic containing 'geometry' and ('not yet' | 'v0.1' | 'not supported'); \
         got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (c) The geometry-specific diagnostic should name both the sub and the member.
    let names_sub_and_member = errors.iter().any(|d| {
        msg_contains(&d.message, "geometry")
            && has_deferred_keyword(&d.message)
            && msg_contains(&d.message, "inner")
            && msg_contains(&d.message, "body")
    });
    assert!(
        names_sub_and_member,
        "geometry diagnostic must name both 'inner' (sub) and 'body' (member); \
         got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (d) Regression guard: the OLD generic diagnostic must NOT appear.
    let has_generic_fallback = errors
        .iter()
        .any(|d| d.message == "unknown member 'body' on sub 'inner'");
    assert!(
        !has_generic_fallback,
        "found old generic diagnostic 'unknown member 'body' on sub 'inner'' — \
         it should have been replaced by the geometry-specific diagnostic"
    );
}

// ─── step-3: let body = geometry cross-sub access ────────────────────────────

/// Same as step-1 but `Inner` uses `let body = box(...)` (geometry let binding).
///
/// Both `param body : Solid` and `let body = <geometry>` lower to `RealizationDecl`
/// — so the diagnostic must be uniform. May already be GREEN after step-2.
#[test]
fn let_body_cross_sub_access_emits_specific_diagnostic() {
    let source = r#"pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let copy = self.inner.body
}"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "expected at least one Error diagnostic for `self.inner.body` (geometry let access)"
    );

    let has_geometry_diagnostic = errors.iter().any(|d| {
        msg_contains(&d.message, "geometry") && has_deferred_keyword(&d.message)
    });
    assert!(
        has_geometry_diagnostic,
        "expected geometry-specific diagnostic for let-body cross-sub access; \
         got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let names_sub_and_member = errors.iter().any(|d| {
        msg_contains(&d.message, "geometry")
            && has_deferred_keyword(&d.message)
            && msg_contains(&d.message, "inner")
            && msg_contains(&d.message, "body")
    });
    assert!(
        names_sub_and_member,
        "geometry diagnostic must name both 'inner' and 'body'; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let has_generic_fallback = errors
        .iter()
        .any(|d| d.message == "unknown member 'body' on sub 'inner'");
    assert!(
        !has_generic_fallback,
        "found old generic diagnostic — should have been replaced by geometry-specific one"
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
        msg_contains(&d.message, "unknown member")
            && msg_contains(&d.message, "nonexistent")
            && msg_contains(&d.message, "inner")
    });
    assert!(
        has_generic,
        "expected generic 'unknown member' diagnostic naming 'nonexistent' and 'inner'; \
         got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (c) Must NOT contain "geometry" or "v0.1" — this is not a geometry member.
    let has_geometry_path = errors.iter().any(|d| {
        msg_contains(&d.message, "geometry") || msg_contains(&d.message, "v0.1")
    });
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
/// RED until step-8 lands.
#[test]
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

    let has_geometry_diagnostic = errors.iter().any(|d| {
        msg_contains(&d.message, "geometry") && has_deferred_keyword(&d.message)
    });
    assert!(
        has_geometry_diagnostic,
        "expected geometry-specific diagnostic for bolts[0].body; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let names_sub_and_member = errors.iter().any(|d| {
        msg_contains(&d.message, "geometry")
            && has_deferred_keyword(&d.message)
            && msg_contains(&d.message, "bolts")
            && msg_contains(&d.message, "body")
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
/// RED until step-8 lands.
#[test]
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

    let has_geometry_diagnostic = errors.iter().any(|d| {
        msg_contains(&d.message, "geometry") && has_deferred_keyword(&d.message)
    });
    assert!(
        has_geometry_diagnostic,
        "expected geometry-specific diagnostic for self.bolts.body; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let names_sub_and_member = errors.iter().any(|d| {
        msg_contains(&d.message, "geometry")
            && has_deferred_keyword(&d.message)
            && msg_contains(&d.message, "bolts")
            && msg_contains(&d.message, "body")
    });
    assert!(
        names_sub_and_member,
        "geometry diagnostic must name 'bolts' and 'body'; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── step-9: anti-cascade — geometry access as operand does not cascade ────────

/// When `self.inner.body` (a cross-sub geometry member access) appears as an
/// operand to another expression, the geometry-specific diagnostic fires exactly
/// ONCE and downstream type-checking does NOT emit spurious cascade errors.
///
/// `make_poison_literal` returns `Type::Error`; downstream geometry-call
/// argument checking already short-circuits on `Type::Error`.
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

    // (a) The geometry-cross-sub diagnostic must fire at least once.
    let has_geometry_diagnostic = errors.iter().any(|d| {
        msg_contains(&d.message, "geometry") && has_deferred_keyword(&d.message)
    });
    assert!(
        has_geometry_diagnostic,
        "expected geometry-specific diagnostic to fire; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (b) Total error count is bounded (≤ 2): the cross-sub diagnostic plus at
    //     most one bubble-up from the surrounding translate() call context.
    assert!(
        errors.len() <= 2,
        "expected at most 2 errors (cross-sub + at most one bubble-up), got {}: {:?}",
        errors.len(),
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (c) No cascade "expected geometry expression" / "type mismatch" / "argument N" errors.
    let has_cascade = errors.iter().any(|d| {
        (msg_contains(&d.message, "expected geometry expression")
            || msg_contains(&d.message, "type mismatch")
            || msg_contains(&d.message, "argument 1 must be"))
            && !msg_contains(&d.message, "geometry")
    });
    assert!(
        !has_cascade,
        "unexpected cascade diagnostic in errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
