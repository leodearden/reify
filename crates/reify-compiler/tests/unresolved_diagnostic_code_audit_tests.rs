//! Audit-coverage tests for `DiagnosticCode::UnresolvedType` and
//! `DiagnosticCode::UnresolvedName` (task 3721).
//!
//! # Contract
//!
//! Every "unresolved type" / "unresolved name" emit site in the compiler crate
//! must attach the corresponding `DiagnosticCode` variant via `.with_code(...)`.
//! These tests lock that contract: one test per emit-site scenario, each compiling
//! a minimal `.ri` source through the targeted code path and asserting
//! `d.code == Some(DiagnosticCode::UnresolvedType)` (or `UnresolvedName`) for at
//! least one diagnostic in the output.
//!
//! This verifies *code-level* coverage — that the relevant `DiagnosticCode` is
//! emitted when the targeted source construct is compiled — but does NOT pin a
//! specific emit-site location.  Multiple production sites share format-string
//! prefixes, and message prose is not part of any structured contract.  The
//! `DiagnosticCode` enum itself is the contract downstream consumers depend on.
//!
//! If per-emit-site uniqueness becomes genuinely necessary in a future task, the
//! appropriate approach is to introduce a dedicated `DiagnosticCode` variant per
//! emit-site category, or to assert against a known source position via the
//! diagnostic's span — not to scrape message prose.
//!
//! A future maintainer who adds a new "unresolved type" emit site should also add
//! a test here so the contract remains self-documenting.
//!
//! # Coverage gaps — DimensionalOp arms
//!
//! Four `DimensionalOp` early-reject branches cannot be exercised from an
//! end-to-end `reify_syntax::parse(...)` + `reify_compiler::compile(...)` call
//! because the Reify parser only produces `TypeExprKind::Named` in type-annotation
//! positions (param declarations, field domain/codomain, trait members, conformance
//! member annotations).  The four affected sites are:
//!
//! - `functions.rs:280`  — field domain DimensionalOp arm
//! - `functions.rs:319`  — field codomain DimensionalOp arm
//! - `traits.rs:34-42`   — trait-member type DimensionalOp arm
//! - `conformance/checker.rs:132-138` — conformance-check DimensionalOp arm
//!
//! Dispatch through these arms (at the *message* level) is already exercised by
//! the direct-AST-construction tests in
//! `crates/reify-compiler/tests/type_expr_kind_dispatch_tests.rs`
//! (`dim_op_in_field_domain_emits_exactly_one_diagnostic`,
//! `dim_op_in_trait_param_emits_diagnostic`).  The `.with_code(DiagnosticCode::UnresolvedType)`
//! attachment at those sites is verified only by step-6 code inspection.  No
//! end-to-end tests for these four arms are included here; they are not stubs —
//! they are simply absent because they cannot be triggered via the public parse API.

use reify_types::{DiagnosticCode, ModulePath};

// ── UnresolvedType emit-site tests ──────────────────────────────────────────

/// `functions.rs:122` — function return type fails to resolve.
///
/// Source: `fn f(x: Int) -> Bogus { 0 }`
/// The return type `Bogus` does not resolve; `compile_function` emits
/// "unresolved return type: Bogus" with `DiagnosticCode::UnresolvedType`.
#[test]
fn unresolved_return_type_carries_code() {
    let source = r#"
fn f(x : Int) -> Bogus { 0 }
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("audit_return_type"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.iter().any(|d| d.code == Some(DiagnosticCode::UnresolvedType)),
        "expected UnresolvedType diagnostic, got: {:#?}",
        compiled.diagnostics
    );
}

/// `guards.rs:155` — structure guarded-group `param` member has an unresolved type.
///
/// Source: a structure with a `where active { param x : Bogus }` block.
/// `register_guarded_names` (guards.rs:130) iterates over the block members;
/// when `Bogus` fails to resolve it emits "unresolved type: Bogus" with
/// `DiagnosticCode::UnresolvedType`. Note: top-level structure params go through
/// entity.rs:487, but params nested inside a `where {}` block specifically
/// exercise the guards.rs:155 path via `register_guarded_names`.
#[test]
fn unresolved_purpose_guard_param_type_carries_code() {
    let source = r#"
structure S {
    param active : Bool = true
    where active {
        param x : Bogus
    }
}
"#;
    let parsed =
        reify_syntax::parse(source, ModulePath::single("audit_purpose_guard_param"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.iter().any(|d| d.code == Some(DiagnosticCode::UnresolvedType)),
        "expected UnresolvedType diagnostic, got: {:#?}",
        compiled.diagnostics
    );
}

/// `entity.rs:487` — structure member `param` has an unresolved type.
///
/// Source: `structure S { param x : Bogus }`
/// The entity compiler resolves member param types; when `Bogus` fails to
/// resolve it emits "unresolved type: Bogus" with `DiagnosticCode::UnresolvedType`.
#[test]
fn unresolved_entity_member_param_type_carries_code() {
    let source = r#"
structure S {
    param x : Bogus
}
"#;
    let parsed =
        reify_syntax::parse(source, ModulePath::single("audit_entity_member_param"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.iter().any(|d| d.code == Some(DiagnosticCode::UnresolvedType)),
        "expected UnresolvedType diagnostic, got: {:#?}",
        compiled.diagnostics
    );
}

/// `entity.rs:742-743` — port parameter has an unresolved type.
///
/// Source: a structure with a port whose parameter has an unknown type name.
/// The port-parameter type resolution path emits
/// "unresolved type name 'Bogus' in port parameter" with `DiagnosticCode::UnresolvedType`.
#[test]
fn unresolved_port_parameter_type_carries_code() {
    let source = r#"
structure S {
    port p : MechPort {
        param x : Bogus
    }
}
"#;
    let parsed =
        reify_syntax::parse(source, ModulePath::single("audit_port_parameter_type"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.iter().any(|d| d.code == Some(DiagnosticCode::UnresolvedType)),
        "expected UnresolvedType diagnostic, got: {:#?}",
        compiled.diagnostics
    );
}

/// `expr.rs:2294-2300 / 2305-2311` — lambda parameter has an unresolved type.
///
/// Source: a structure `let` binding with a lambda whose parameter type is unknown.
/// Both the Named arm (2294) and the non-Named arm (2305) share the same guard;
/// the Named arm is triggered by `|x : Bogus| x` where `Bogus` is a Named TypeExpr.
#[test]
fn unresolved_lambda_param_type_carries_code() {
    let source = r#"
structure S {
    let v = |x : Bogus| x
}
"#;
    let parsed =
        reify_syntax::parse(source, ModulePath::single("audit_lambda_param_type"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.iter().any(|d| d.code == Some(DiagnosticCode::UnresolvedType)),
        "expected UnresolvedType diagnostic, got: {:#?}",
        compiled.diagnostics
    );
}

/// `traits.rs:87-92` — trait member type fails to resolve.
///
/// Source: `trait T { param m : Bogus }`
/// The trait compiler resolves member types; when `Bogus` fails to resolve
/// it emits "unresolved type in trait 'T': Bogus" with `DiagnosticCode::UnresolvedType`.
#[test]
fn unresolved_trait_member_type_carries_code() {
    let source = r#"
trait T {
    param m : Bogus
}
"#;
    let parsed =
        reify_syntax::parse(source, ModulePath::single("audit_trait_member_type"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.iter().any(|d| d.code == Some(DiagnosticCode::UnresolvedType)),
        "expected UnresolvedType diagnostic, got: {:#?}",
        compiled.diagnostics
    );
}

/// `conformance/checker.rs:185-188` — structure member type fails to resolve in
/// conformance check.
///
/// Source: a trait + conforming structure where the structure member has an
/// unknown type. The `resolve_member_annotation_type` closure in the checker
/// emits "unresolved type in conformance check: Bogus" with
/// `DiagnosticCode::UnresolvedType`.
#[test]
fn unresolved_conformance_check_type_carries_code() {
    let source = r#"
trait HasLength {
    param size : Length
}
structure Bolt : HasLength {
    param size : Bogus = 5mm
}
"#;
    let parsed =
        reify_syntax::parse(source, ModulePath::single("audit_conformance_check_type"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.iter().any(|d| d.code == Some(DiagnosticCode::UnresolvedType)),
        "expected UnresolvedType diagnostic, got: {:#?}",
        compiled.diagnostics
    );
}

/// `type_resolution.rs:1015-1021` — type alias argument fails to resolve.
///
/// Source: a parametric type alias used with an unknown type argument.
/// The alias resolver emits "unresolved type argument 'Bogus' for alias 'V'"
/// with `DiagnosticCode::UnresolvedType`.
#[test]
fn unresolved_type_alias_argument_carries_code() {
    let source = r#"
type V<Q> = Vector3<Q>
structure S {
    param x : V<Bogus>
}
"#;
    let parsed =
        reify_syntax::parse(source, ModulePath::single("audit_type_alias_argument"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.iter().any(|d| d.code == Some(DiagnosticCode::UnresolvedType)),
        "expected UnresolvedType diagnostic, got: {:#?}",
        compiled.diagnostics
    );
}

// ── UnresolvedName emit-site tests ───────────────────────────────────────────

/// `annotations.rs:321` — solver-hint annotation references an undefined
/// collection name.
///
/// Source: a structure param annotated with `@solver_hint("discrete_set", ...)`
/// where the collection name does not exist in scope.
/// The annotation validator emits "unresolved name: undefined_collection" with
/// `DiagnosticCode::UnresolvedName`.
#[test]
fn unresolved_solver_hint_name_carries_code() {
    let source = r#"
structure S {
    @solver_hint("discrete_set", undefined_collection)
    param x : Real = auto
}
"#;
    let parsed =
        reify_syntax::parse(source, ModulePath::single("audit_solver_hint_name"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.iter().any(|d| d.code == Some(DiagnosticCode::UnresolvedName)),
        "expected UnresolvedName diagnostic, got: {:#?}",
        compiled.diagnostics
    );
}
