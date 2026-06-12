//! Tests for the conformance collision rule (PRD v0_6 type-hygiene §7.4, task η).
//!
//! A conformer member that REDECLARES (collides with) a *defaulted* trait member
//! is type-checked against the trait's declared type. The defaults loop
//! (`for default in &ctx.defaults`) inside `check_phase_check_members_against_requirements`
//! (conformance/checker.rs phase 5) handles both `DefaultKind::Param` and annotated
//! `DefaultKind::Let { cell_type: Some(_) }` defaults. An incompatible collision produces
//! a `"type mismatch for trait member '<name>'"` diagnostic.
//!
//! Compatible collisions (same type or implicitly-convertible type) remain legal —
//! the override idiom (§10 row 11: conformer param overrides trait let with a measured value)
//! is preserved. Unannotated trait-let defaults (`cell_type: None`) are silently skipped
//! (their inferred type is not available for colliding names — see deferred gap note in
//! checker.rs). Names already covered by a required-member check (`ctx.requirements`) are
//! also skipped to avoid false-positive double-reports in refinement chains.
//!
//! ## Coverage (Cycle 1 — Param arm)
//!
//! (a) POSITIVE `incompatible_scalar_override_of_defaulted_param_errors`:
//!     `trait T { param x : Length = 5mm }` + `structure def S : T { param x : Mass }`
//!     → expect ≥1 error containing "type mismatch for trait member 'x'". GREEN.
//!
//! (b) POSITIVE `scalar_override_of_tensor_defaulted_trait_param_errors`:
//!     Trait with `param moi : Tensor<2,3,MomentOfInertia>` defaulted via a
//!     self-contained `matrix([...])` literal + conformer with `param moi : MomentOfInertia`
//!     → expect "type mismatch for trait member 'moi'" (probe-7 headline, §10 row 10). GREEN.
//!
//! (c) NEGATIVE `compatible_param_override_of_defaulted_param_conforms`:
//!     `trait T { param x : Length = 5mm }` + `structure def S : T { param x : Length }`
//!     → NO "type mismatch for trait member" diagnostic (override idiom stays legal). GREEN.
//!
//! (d) NEGATIVE `tensor_typed_override_of_tensor_defaulted_param_conforms`:
//!     Conformer overrides with the SAME `Tensor<2,3,MomentOfInertia>` type
//!     → no mismatch (probe-7 TensorOverride, §10 row 11). GREEN.
//!
//! (e) NEGATIVE `unresolved_override_of_defaulted_param_suppresses_cascade`:
//!     `trait T { param x : Length = 5mm }` + `structure def S : T { param x : UnknownType }`
//!     → `resolve_member_annotation_type` returns `Type::Error`;
//!     `implicitly_converts_to(Type::Error, Length) → true` (producer-side wildcard,
//!     type_compat.rs:3–26) → NO "type mismatch for trait member" cascade;
//!     a root-cause "unresolved type in conformance check" error MUST be present. GREEN.

use reify_test_support::{compile_source, errors_only};

// ── (a) POSITIVE: incompatible scalar override of a defaulted param ──────────

/// Conformer declares `param x : Mass` but the trait defaults `param x : Length = 5mm`.
/// The collision check must fire: Mass is not implicitly_converts_to Length.
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

// ── Cycle 2: annotated-Let arm + cross-kind override ──────────────────────────
//
// (a) POSITIVE `param_override_of_annotated_trait_let_default_incompatible_errors`:
//     `trait T { let y : Length = 5mm }` + `structure def S : T { param y : Mass }`
//     → expect ≥1 error containing "type mismatch for trait member 'y'". GREEN.
//
//     The defaults loop covers both `DefaultKind::Param` and `DefaultKind::Let { cell_type: Some }`,
//     so annotated-let defaults are type-checked against conformer members too.
//
// (b) NEGATIVE guard `param_override_of_annotated_trait_let_default_compatible_conforms`:
//     `trait T { let y : Length = 5mm }` + `structure def S : T { param y : Length }`
//     → NO "type mismatch for trait member" (§10 row 11 override idiom). GREEN.

// ── (a) POSITIVE: cross-kind incompatible override — conformer param vs annotated trait let
/// A conformer `param y : Mass` collides with a trait annotated-let default
/// `let y : Length = 5mm`. The collision check must fire (cross-kind, Mass ≠ Length).
#[test]
fn param_override_of_annotated_trait_let_default_incompatible_errors() {
    let source = r#"
trait T {
    let y : Length = 5mm
}
structure def S : T {
    param y : Mass
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member 'y'")),
        "expected ≥1 Severity::Error containing \"type mismatch for trait member 'y'\" \
         (cross-kind collision: conformer param Mass vs trait annotated-let Length); got: {:?}",
        errors,
    );
}

// ── (b) NEGATIVE guard: compatible cross-kind override (§10 row 11) ──────────────

/// `trait T { let y : Length = 5mm }` + `structure def S : T { param y : Length }`
/// — the conformer param overrides a derived trait let with a compatible measured value.
/// `implicitly_converts_to(Length, Length)` is true → no "type mismatch for trait member".
///
/// This is §10 row 11: a conformer can upgrade a trait `let` default to a settable `param`
/// as long as the type is compatible.
#[test]
fn param_override_of_annotated_trait_let_default_compatible_conforms() {
    let source = r#"
trait T {
    let y : Length = 5mm
}
structure def S : T {
    param y : Length
}
"#;
    let module = compile_source(source);

    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member")),
        "compatible cross-kind override (conformer param Length over trait let Length) \
         must NOT produce \"type mismatch for trait member\"; all diagnostics: {:?}",
        module.diagnostics,
    );
}

// ── Suggestion-2 guard: required_names prevents false positive in refinement chains ──

/// When a sub-trait re-declares a parent-defaulted param as REQUIRED (no default),
/// BOTH the requirement AND the old default end up in the merged `ctx`. The requirements
/// loop validates the conformer member against the required type; the defaults loop must
/// SKIP the name (it is in `required_names`) to avoid re-checking against the old default
/// type — which would be a false positive.
///
/// Scenario:
///   trait Parent { param x : Length = 5mm }   // defaulted, goes into ctx.defaults
///   trait Child : Parent { param x : Mass }    // required (no default), goes into ctx.requirements
///   structure def S : Child { param x : Mass = 10kg }
///
/// The requirements loop sees `x : Mass` (required) and validates `Mass` against `Mass` → OK.
/// The defaults loop sees `x : Length` (old default) but skips it because 'x' ∈ required_names.
/// Without the guard a false "expected Length, got Mass" would fire.
#[test]
fn required_names_guard_prevents_false_positive_in_refinement_chain() {
    let source = r#"
trait Parent {
    param x : Length = 5mm
}
trait Child : Parent {
    param x : Mass
}
structure def S : Child {
    param x : Mass = 10kg
}
"#;
    let module = compile_source(source);

    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member")),
        "required_names guard must prevent false 'type mismatch for trait member 'x'' when \
         the name is already validated by the requirements loop; \
         all diagnostics: {:?}",
        module.diagnostics,
    );
}

// ── Suggestion-4a: unannotated trait-let collision is silently accepted (deferred gap) ──

/// When a trait has an UNANNOTATED `let` default (`DefaultKind::Let { cell_type: None }`),
/// the defaults loop skips it (`continue`) — the inferred type is NOT computed for names
/// that collide with a structure member (pre-register Pass 2 skips them, checker.rs:596).
/// So a conformer member colliding with an unannotated trait let is silently accepted.
///
/// This is a documented, intentional limitation (the deferred gap in checker.rs comment).
/// Closing it would require compiling the trait-let expression in the conformer scope.
///
/// Source:
///   trait T { let y = 5mm }           // unannotated — cell_type: None → skipped
///   structure def S : T { param y : Mass }  // type is NOT checked against y's inferred type
///
/// Expected: NO "type mismatch for trait member 'y'" diagnostic.
#[test]
fn unannotated_trait_let_collision_silently_accepted() {
    let source = r#"
trait T {
    let y = 5mm
}
structure def S : T {
    param y : Mass
}
"#;
    let module = compile_source(source);

    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member")),
        "unannotated trait-let collision must NOT produce \"type mismatch for trait member\" \
         (deferred gap: inferred type not available for colliding names); \
         all diagnostics: {:?}",
        module.diagnostics,
    );
}

// ── Suggestion-4b: conformer `let` member overriding a defaulted `param` ────────────

/// The defaults loop looks up the conformer type KIND-AGNOSTICALLY:
/// `structure_param_members.get(name).or_else(|| structure_let_members.get(name))`.
/// This means a conformer `let x : Mass` (stored in `structure_let_members`)
/// IS type-checked against a trait `param x : Length = 5mm` default.
///
/// POSITIVE case: incompatible conformer let → type check fires.
#[test]
fn conformer_let_overriding_defaulted_param_incompatible_errors() {
    let source = r#"
trait T {
    param x : Length = 5mm
}
structure def S : T {
    let x : Mass = 10kg
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member 'x'")),
        "conformer annotated-let (Mass) overriding a defaulted trait param (Length) must \
         produce \"type mismatch for trait member 'x'\"; got: {:?}",
        errors,
    );
}

/// NEGATIVE case: compatible conformer let → no type-mismatch diagnostic.
#[test]
fn conformer_let_overriding_defaulted_param_compatible_conforms() {
    let source = r#"
trait T {
    param x : Length = 5mm
}
structure def S : T {
    let x : Length = 3mm
}
"#;
    let module = compile_source(source);

    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member")),
        "conformer annotated-let (Length) overriding a defaulted trait param (Length) must \
         NOT produce \"type mismatch for trait member\"; all diagnostics: {:?}",
        module.diagnostics,
    );
}

// ── Suggestion-1 (amendment): checked_names dedup — diamond-override Let scenario ──────────
//
// When MULTIPLE traits each provide an annotated-let default for the SAME name AND the
// structure overrides that name, `collect_all_requirements` (trait_requirements.rs) pushes
// EACH default into `ctx.defaults` without dedup (hash-recording is suppressed for overridden
// names). The checked_names guard in the defaults loop ensures only ONE diagnostic fires
// per name regardless of how many redundant default entries are in ctx.defaults.

/// Diamond scenario: two traits both provide `let y : Length = <different-value>` (different
/// content hashes → would normally conflict, but the structure overrides `y` so conflict
/// detection is suppressed and both defaults are pushed into ctx.defaults without dedup).
///
/// Structure overrides with `param y : Mass` — an incompatible type.
///
/// Without the `checked_names` guard the loop would emit TWO identical
/// "type mismatch for trait member 'y'" diagnostics (one per redundant entry).
/// With the guard, EXACTLY ONE fires.
#[test]
fn diamond_override_let_collision_emits_exactly_one_diagnostic() {
    let source = r#"
trait TraitA {
    let y : Length = 5mm
}
trait TraitB {
    let y : Length = 10mm
}
structure def S : TraitA + TraitB {
    param y : Mass
}
"#;
    let module = compile_source(source);
    let mismatch_count = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("type mismatch for trait member 'y'"))
        .count();

    assert_eq!(
        mismatch_count, 1,
        "diamond-override Let collision must emit EXACTLY ONE \
         \"type mismatch for trait member 'y'\" diagnostic (checked_names dedup); \
         all diagnostics: {:?}",
        module.diagnostics,
    );
}

/// Same diamond scenario but the structure overrides with a COMPATIBLE type (`Length`).
/// With the `checked_names` guard, ZERO "type mismatch for trait member" diagnostics fire.
#[test]
fn diamond_override_let_collision_compatible_conforms() {
    let source = r#"
trait TraitA {
    let y : Length = 5mm
}
trait TraitB {
    let y : Length = 10mm
}
structure def S : TraitA + TraitB {
    param y : Length
}
"#;
    let module = compile_source(source);

    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member")),
        "diamond-override Let with compatible conformer type must NOT produce \
         \"type mismatch for trait member\"; all diagnostics: {:?}",
        module.diagnostics,
    );
}
