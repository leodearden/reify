//! RED tests for the conformance collision rule (PRD v0_6 type-hygiene §7.4, task η).
//!
//! A conformer member that REDECLARES (collides with) a *defaulted* trait member
//! is currently accepted with NO type check. The requirements loop in
//! `check_phase_check_members_against_requirements` (conformance/checker.rs phase 5)
//! only sees REQUIRED (no-default) members — defaulted params/lets go into
//! `ctx.defaults`, never `ctx.requirements`. So incompatible overrides of defaulted
//! trait members silently pass today.
//!
//! After step-2 adds a `for default in &ctx.defaults` loop (Param arm) inside
//! `check_phase_check_members_against_requirements`, incompatible collisions produce
//! `"type mismatch for trait member '<name>'"` diagnostics.
//!
//! ## Coverage (Cycle 1 — Param arm)
//!
//! (a) POSITIVE `incompatible_scalar_override_of_defaulted_param_errors`:
//!     `trait T { param x : Length = 5mm }` + `structure def S : T { param x : Mass }`
//!     → expect ≥1 error containing "type mismatch for trait member 'x'".
//!     RED today (no check on defaulted params) → GREEN after step-2.
//!
//! (b) POSITIVE `scalar_override_of_tensor_defaulted_trait_param_errors`:
//!     Trait with `param moi : Tensor<2,3,MomentOfInertia>` defaulted via a
//!     self-contained `matrix([...])` literal + conformer with `param moi : MomentOfInertia`
//!     → expect "type mismatch for trait member 'moi'" (probe-7 headline, §10 row 10).
//!     RED today → GREEN after step-2.
//!
//! (c) NEGATIVE `compatible_param_override_of_defaulted_param_conforms`:
//!     `trait T { param x : Length = 5mm }` + `structure def S : T { param x : Length }`
//!     → NO "type mismatch for trait member" diagnostic (override idiom stays legal).
//!     GREEN today and after step-2.
//!
//! (d) NEGATIVE `tensor_typed_override_of_tensor_defaulted_param_conforms`:
//!     Conformer overrides with the SAME `Tensor<2,3,MomentOfInertia>` type
//!     → no mismatch (probe-7 TensorOverride, §10 row 11).
//!     GREEN today and after step-2.
//!
//! (e) NEGATIVE `unresolved_override_of_defaulted_param_suppresses_cascade`:
//!     `trait T { param x : Length = 5mm }` + `structure def S : T { param x : UnknownType }`
//!     → `resolve_member_annotation_type` returns `Type::Error`;
//!     `implicitly_converts_to(Type::Error, Length) → true` (producer-side wildcard,
//!     type_compat.rs:3–26) → NO "type mismatch for trait member" cascade;
//!     a root-cause "unresolved type in conformance check" error MUST be present.
//!     GREEN today and after step-2.

use reify_test_support::{compile_source, errors_only};

// ── (a) POSITIVE: incompatible scalar override of a defaulted param ──────────

/// Conformer declares `param x : Mass` but the trait defaults `param x : Length = 5mm`.
/// The collision check must fire: Mass is not implicitly_converts_to Length.
///
/// ## RED before step-2
/// Today (no collision check on defaulted params) this silently accepts the override.
/// After step-2 adds the `for default in &ctx.defaults` Param arm, the check fires.
#[test]
fn incompatible_scalar_override_of_defaulted_param_errors() {
    let source = r#"
trait T {
    param x : Length = 5mm
}
structure def S : T {
    param x : Mass
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member 'x'")),
        "expected ≥1 Severity::Error containing \"type mismatch for trait member 'x'\" \
         (Length vs Mass collision on defaulted param); got: {:?}",
        errors,
    );
}

// ── (b) POSITIVE: scalar override of a tensor-defaulted trait param ────────

/// The trait declares `param moi : Tensor<2,3,MomentOfInertia>` with a self-contained
/// matrix literal default; the conformer narrows to `param moi : MomentOfInertia`.
/// This is §10 row 10: type reduction (tensor → scalar) must be rejected.
///
/// Uses a SELF-CONTAINED matrix literal (no `moment_of_inertia(geometry, material.density)`)
/// so this test has zero dependency on sibling tasks γ/δ/ε/ζ.
///
/// ## RED before step-2
/// Today this silently accepts the incompatible scalar override.
#[test]
fn scalar_override_of_tensor_defaulted_trait_param_errors() {
    let source = r#"
trait LocalRigid {
    param moi : Tensor<2,3,MomentOfInertia> = matrix([
        [1.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m],
        [0.0 * 1kg * 1m * 1m, 1.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m],
        [0.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m, 1.0 * 1kg * 1m * 1m]
    ])
}
structure def ScalarOverride : LocalRigid {
    param moi : MomentOfInertia
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member 'moi'")),
        "expected ≥1 Severity::Error containing \"type mismatch for trait member 'moi'\" \
         (Tensor<2,3,MomentOfInertia> vs MomentOfInertia scalar collision); got: {:?}",
        errors,
    );
}

// ── (c) NEGATIVE guard: compatible same-type override stays legal ─────────────

/// `trait T { param x : Length = 5mm }` + `structure def S : T { param x : Length }`
/// — the override is type-compatible; `implicitly_converts_to(Length, Length)` is true.
/// No "type mismatch for trait member" should fire.
///
/// This is the §10 row 11 override idiom — conformers can pin a measured value for
/// a parameter that a trait already defaults, as long as the type is compatible.
///
/// Must remain GREEN both before and after step-2.
#[test]
fn compatible_param_override_of_defaulted_param_conforms() {
    let source = r#"
trait T {
    param x : Length = 5mm
}
structure def S : T {
    param x : Length
}
"#;
    let module = compile_source(source);

    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member")),
        "compatible same-type override must NOT produce \"type mismatch for trait member\"; \
         all diagnostics: {:?}",
        module.diagnostics,
    );
}

// ── (d) NEGATIVE guard: tensor-typed override of tensor-defaulted param stays legal ──

/// Conformer overrides with the SAME `Tensor<2,3,MomentOfInertia>` type the trait declares.
/// `implicitly_converts_to(Tensor<2,3,MomentOfInertia>, Tensor<2,3,MomentOfInertia>)` is true.
/// No mismatch should fire. This is probe-7 TensorOverride, §10 row 11.
///
/// Must remain GREEN both before and after step-2.
#[test]
fn tensor_typed_override_of_tensor_defaulted_param_conforms() {
    let source = r#"
trait LocalRigid {
    param moi : Tensor<2,3,MomentOfInertia> = matrix([
        [1.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m],
        [0.0 * 1kg * 1m * 1m, 1.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m],
        [0.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m, 1.0 * 1kg * 1m * 1m]
    ])
}
structure def TensorOverride : LocalRigid {
    param moi : Tensor<2,3,MomentOfInertia>
}
"#;
    let module = compile_source(source);

    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member")),
        "tensor-typed override of tensor-defaulted param must NOT produce \
         \"type mismatch for trait member\"; all diagnostics: {:?}",
        module.diagnostics,
    );
}

// ── (e) NEGATIVE guard: unresolved annotation suppresses cascade ──────────────

/// `trait T { param x : Length = 5mm }` + `structure def S : T { param x : UnknownType }`
///
/// `resolve_member_annotation_type` returns `Type::Error` for `UnknownType`;
/// `implicitly_converts_to(Type::Error, Length) → true` (producer-side wildcard,
/// type_compat.rs:3–26) → NO "type mismatch for trait member" cascade on top of the
/// root-cause "unresolved type in conformance check" error.
///
/// Asserts:
/// (a) ≥1 Severity::Error is present (root-cause pin)
/// (b) NO diagnostic at any severity contains "type mismatch for trait member"
/// (c) ≥1 error contains "unresolved type in conformance check" AND "UnknownType"
///
/// Must remain GREEN both before and after step-2.
#[test]
fn unresolved_override_of_defaulted_param_suppresses_cascade() {
    let source = r#"
trait T {
    param x : Length = 5mm
}
structure def S : T {
    param x : UnknownType
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // (a) Root-cause error must be present — wildcard silence is always paired with one.
    assert!(
        !errors.is_empty(),
        "expected ≥1 Severity::Error for the unresolved 'UnknownType' annotation; \
         got none; all diagnostics: {:?}",
        module.diagnostics,
    );

    // (b) Anti-cascade: no "type mismatch for trait member" on top of the root-cause.
    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member")),
        "unexpected \"type mismatch for trait member\" cascade diagnostic — \
         Type::Error producer-side wildcard must suppress it; \
         all diagnostics: {:?}",
        module.diagnostics,
    );

    // (c) Specific root-cause "unresolved type in conformance check: UnknownType" must exist.
    assert!(
        errors.iter().any(|d| {
            d.message.contains("unresolved type in conformance check")
                && d.message.contains("UnknownType")
        }),
        "expected ≥1 error containing both \"unresolved type in conformance check\" and \
         \"UnknownType\" (the root-cause from resolve_member_annotation_type); \
         got: {:?}",
        errors,
    );
}
