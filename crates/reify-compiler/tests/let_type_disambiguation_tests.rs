//! Tests for DefaultKind::Let cell_type disambiguation (task 366).
//!
//! These tests verify that a trait's `let` binding carries the correct type
//! annotation in `DefaultKind::Let { cell_type, .. }`:
//!   - annotated `let x : Length = …` → `cell_type = Some(Type::length())`
//!   - unannotated `let x = …`        → `cell_type = None`
//!   - explicitly `let x : Real = …`  → `cell_type = Some(Type::Real)`
//!   - unknown annotation `let x : Nonexistent = …` → diagnostic + `Some(Type::Real)` fallback
//!
//! Steps 8 and 9 add integration tests for the conformance check path that
//! produced a false type-mismatch before this fix.

use reify_compiler::DefaultKind;
use reify_test_support::{compile_source, errors_only};
use reify_core::{Diagnostic, DimensionVector, Type};
use reify_ir::{CompiledExprKind, Value};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Compile `source`, find the named trait, and return the `cell_type` from its
/// first `DefaultKind::Let` default.
///
/// Panics if the trait is not found or has no Let default.
fn extract_let_cell_type(source: &str, trait_name: &str) -> Option<Type> {
    let module = compile_source(source);
    let trait_def = module
        .trait_defs
        .iter()
        .find(|t| t.name == trait_name)
        .unwrap_or_else(|| panic!("expected trait {}", trait_name));
    let let_default = trait_def
        .defaults
        .iter()
        .find(|d| matches!(&d.kind, DefaultKind::Let { .. }))
        .unwrap_or_else(|| panic!("expected a Let default in trait {}", trait_name));
    match &let_default.kind {
        DefaultKind::Let { cell_type, .. } => cell_type.clone(),
        other => panic!("expected DefaultKind::Let, got {:?}", other),
    }
}

/// Returns true if any diagnostic's message flags `name` as an unresolved
/// identifier.  Tolerant of minor wording variations — the identifier may
/// appear quoted as `'name'`, ``` `name` ```, or after a colon (`: name`) —
/// so the test remains stable under reasonable message-churn while still
/// pinning the specific semantic error (rather than any error containing
/// the literal word "unresolved").
///
/// Diagnostic today does not carry a stable code/category field (see
/// `crates/reify-types/src/diagnostics.rs`); if one is introduced,
/// prefer asserting on that over this prose-level probe.
fn diagnostic_names_unresolved(diagnostics: &[&Diagnostic], name: &str) -> bool {
    diagnostics.iter().any(|d| {
        d.message.contains("unresolved")
            && (d.message.contains(&format!(": {}", name))
                || d.message.contains(&format!("'{}'", name))
                || d.message.contains(&format!("`{}`", name)))
    })
}

// ── step-1 (test): DefaultKind::Let carries cell_type ────────────────────────

/// A trait with `let x : Length = 5mm` must produce a DefaultKind::Let whose
/// cell_type is Some(Type::Scalar{LENGTH}).
#[test]
fn let_with_length_annotation_carries_cell_type() {
    let source = r#"
trait HasLength {
    let x : Length = 5mm
}
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "expected no errors, got: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        extract_let_cell_type(source, "HasLength"),
        Some(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
        "annotated let x : Length should have cell_type = Some(Type::length())"
    );
}

/// A trait with unannotated `let x = 5.0` must produce a DefaultKind::Let
/// whose cell_type is None.
#[test]
fn let_without_annotation_has_none_cell_type() {
    let source = r#"
trait HasUntyped {
    let x = 5.0
}
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "expected no errors, got: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        extract_let_cell_type(source, "HasUntyped"),
        None,
        "unannotated let should have cell_type = None"
    );
}

/// A trait with `let x : Real = 5.0` must produce a DefaultKind::Let whose
/// cell_type is Some(Type::Real).
#[test]
fn let_with_real_annotation_carries_cell_type_real() {
    let source = r#"
trait HasReal {
    let x : Real = 5.0
}
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "expected no errors, got: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        extract_let_cell_type(source, "HasReal"),
        Some(Type::Real),
        "let x : Real should have cell_type = Some(Type::Real)"
    );
}

/// When the annotation names an unknown type, a diagnostic is emitted and
/// cell_type falls back to Some(Type::Real) for error-recovery (not None).
///
/// This guards against a silent regression where someone changes the fallback
/// from `Some(Type::Real)` to `None`, which would alter conformance semantics.
#[test]
fn let_with_unknown_annotation_emits_diagnostic_and_falls_back_to_real() {
    let source = r#"
trait HasBadType {
    let x : Nonexistent = 5.0
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected a diagnostic for unknown type 'Nonexistent'"
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("Nonexistent") || d.message.contains("unresolved")),
        "diagnostic should mention the unknown type, got: {:?}",
        errors
    );
    assert_eq!(
        extract_let_cell_type(source, "HasBadType"),
        Some(Type::Real),
        "error-recovery fallback must be Some(Type::Real), not None"
    );
}

// ── step-8 (test): conformance integration — annotated let satisfies let requirement ──

/// Trait A provides `let x : Length = 5mm`, trait B requires `let x : Length`.
/// Structure S : A + B should compile without errors.
///
/// Before the fix, available_defaults used Type::Real for all Let defaults,
/// so the conformance check compared Real vs Scalar{LENGTH} → false type-mismatch.
#[test]
fn annotated_let_default_satisfies_let_requirement() {
    let source = r#"
trait A {
    let x : Length = 5mm
}
trait B {
    let x : Length
}
structure S : A + B {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "structure S : A + B should compile without type-mismatch errors, got: {:?}",
        errors
    );
}

// ── step-9 (test): scope registration — annotated let injects correctly ───────

/// Trait with `let x : Length = 5mm` injected into structure S (no override).
/// The injected ValueCellDecl for 'x' should exist in the compiled template.
#[test]
fn annotated_let_default_injects_value_cell() {
    let source = r#"
trait HasX {
    let x : Length = 5mm
}
structure S : HasX {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");

    let x_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("expected value_cell 'x' to be injected from trait HasX");

    // The cell should be a Let kind.
    assert_eq!(
        x_cell.kind,
        reify_compiler::ValueCellKind::Let,
        "injected 'x' should be ValueCellKind::Let"
    );
}

// ── negative conformance test: conflicting Let defaults still produces a diagnostic ──

/// Two traits provide `let x` with different expressions (and different annotated types).
/// Structure S implements both without overriding — must produce a "conflicting let
/// bindings" diagnostic.
///
/// Note: the reify trait DSL requires `= expr` for all `let` bindings, so
/// `RequirementKind::Let` is not reachable from source syntax (see trait_merge_tests.rs:277).
/// This test verifies that the conformance engine still reports errors for genuinely
/// conflicting definitions, so the disambiguation fix did not accidentally suppress
/// all error reporting.
#[test]
fn conflicting_let_annotations_produce_diagnostic() {
    let source = r#"
trait ProvidesLength {
    let x : Length = 5mm
}
trait ProvidesArea {
    let x : Area = 1mm * 1mm
}
structure S : ProvidesLength + ProvidesArea {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "structure S : ProvidesLength + ProvidesArea should produce a conflict diagnostic, got none"
    );
    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("conflicting"),
        "diagnostic should mention 'conflicting', got: {}",
        error_msg
    );
}

// ── task 1834 step-1: annotation-vs-expression cross-check emits diagnostic ──

/// Trait with `let x : Length = 5.0` injected into a structure: the annotation
/// says Length, but 5.0 evaluates to Real — they are not compatible via
/// `implicitly_converts_to`, so conformance must emit an error diagnostic from
/// the annotation-cross-check site.
///
/// Before task 1834 this was silent: the cell_type was taken from the compiled
/// expression (Real), so the annotation had no observable effect.
///
/// Assertion pins the *unique phrase* from the new cross-check diagnostic
/// (see `conformance.rs` injection loop —
/// "type mismatch for trait let '…': annotation expects …, expression
/// evaluates to …") rather than the generic "type mismatch" substring.
/// Task 1834 amendment (reviewer_comprehensive test_coverage fix): the
/// generic substring also matches the pre-existing requirement-vs-member
/// and available-default-vs-requirement type-mismatch diagnostics emitted
/// elsewhere in conformance.rs — so if the cross-check ever stopped firing
/// while any other type-mismatch error fired on the same input, the
/// loose assertion would silently pass.  The tightened assertion rules
/// that out by naming the binding and the two halves of the diagnostic.
#[test]
fn annotated_let_expr_type_mismatch_emits_diagnostic() {
    let source = r#"
trait HasX {
    let x : Length = 5.0
}
structure S : HasX {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected a diagnostic for annotation/expression type mismatch, got none"
    );
    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("type mismatch for trait let 'x'")
            && error_msg.contains("annotation expects")
            && error_msg.contains("expression evaluates to"),
        "diagnostic must come from the annotation-cross-check site \
         (format: \"type mismatch for trait let '<name>': annotation expects \
         <ty>, expression evaluates to <ty>\"); the generic \"type mismatch\" \
         substring is not specific enough because other sites also emit it.  \
         Got: {}",
        error_msg
    );
}

/// Pins the Int→Real widening carve-out that justifies `type_compatible`
/// (rather than `implicitly_converts_to`) at the annotation cross-check site
/// in `conformance.rs` — see the in-code comment near that call.
///
/// Rationale: integer-form literals (no `.`/`e`/`E` in source text) lower as
/// `Type::Int` (see `expr.rs:388-395`). Without the widening carve-out,
/// `let x : Real = 42` would emit a spurious type-mismatch diagnostic even
/// though Int values are usable wherever Real is expected.
///
/// The neighbouring mismatch test (`annotated_let_expr_type_mismatch_emits_diagnostic`)
/// relies on both the Int-lowering behaviour AND the Length/Real cross-dimension
/// incompatibility to trigger; it does not exercise the widening relation.
/// The happy-path test (`annotated_let_injected_cell_uses_annotation_type`)
/// uses `5mm` (already Scalar<Length>) which similarly bypasses widening.
///
/// Without this test, a future refactor that swapped `type_compatible` for
/// `implicitly_converts_to` at the cross-check site would silently start
/// rejecting `let x : Real = 42` and no existing test would catch it.
#[test]
fn annotated_let_int_literal_widens_to_real() {
    let source = r#"
trait HasX {
    let x : Real = 42
}
structure S : HasX {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors — `let x : Real = 42` must widen Int to Real at the \
         annotation cross-check site (`type_compatible`, not `implicitly_converts_to`). \
         Got: {:?}",
        errors
    );
}

// ── task 1834 step-2: injected let uses annotation type, not expr type ───────

/// Trait with `let x : Length = 5mm` injected into structure `S : HasX`.
/// The injected `ValueCellDecl` for `x` must have `cell_type ==
/// Type::Scalar { dimension: DimensionVector::LENGTH }`.
///
/// This pins the "annotation-is-authoritative" semantics of improvement 1:
/// after the fix, even if the expression's inferred type drifts (e.g. via a
/// new implicit-conversion rule that makes expr type and annotation type
/// differ while still being compatible), the annotation stays authoritative
/// on the cell — same invariant as the scope pre-registration, which already
/// uses the annotation via `.unwrap_or`.
#[test]
fn annotated_let_injected_cell_uses_annotation_type() {
    let source = r#"
trait HasX {
    let x : Length = 5mm
}
structure S : HasX {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");

    let x_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("expected value_cell 'x' to be injected from trait HasX");

    assert_eq!(
        x_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "annotation type must be authoritative on the injected cell"
    );
}

// ── task 1834 step-3: compatible annotation+expression stays silent ─────────

/// Happy-path guard: exact-match annotation/expression types — `let x : Real = 5.0`
/// and `let x : Length = 5mm` — must compile without any error diagnostic.
/// Protects against a future over-eager cross-check that would reject the
/// happy path where the expression's inferred type implicitly converts to
/// (or exactly matches) the annotation type.
#[test]
fn annotated_let_compatible_expr_no_diagnostic() {
    // Exact match: Real annotation, Real expression.
    // `5.5` is a fractional literal → `Type::Real` (whole-number `.0` literals
    // are typed as `Int` by the compiler and would trip the cross-check).
    let real_source = r#"
trait HasR {
    let x : Real = 5.5
}
structure S : HasR {
}
    "#;
    let real_module = compile_source(real_source);
    let real_errors = errors_only(&real_module);
    assert!(
        real_errors.is_empty(),
        "let x : Real = 5.5 (exact Real) should not emit any errors, got: {:?}",
        real_errors
    );

    // Exact match: Length annotation, Length expression.
    let len_source = r#"
trait HasL {
    let x : Length = 5mm
}
structure S : HasL {
}
    "#;
    let len_module = compile_source(len_source);
    let len_errors = errors_only(&len_module);
    assert!(
        len_errors.is_empty(),
        "let x : Length = 5mm (exact Length) should not emit any errors, got: {:?}",
        len_errors
    );
}

// ── task 1834 step-5: `let x : Length` without value is a parser-level no-op ──

/// The forward-guard originally proposed for step-5 was
/// `unannotated_let_default_satisfies_typed_let_requirement`: trait A provides
/// `let x = 5mm`, trait B requires `let x : Length`, structure `S : A + B {}`
/// should compile cleanly because A's inferred Length default matches B's
/// Length requirement.  While writing the test we discovered that the reify
/// DSL currently does not syntactically accept `let x : Length` without a
/// value expression (see `lower_let` in ts_parser.rs:1455, which returns
/// `None` when `value` is absent, and trait_merge_tests.rs:280).  Trait B
/// therefore parses as empty: no members, no requirements, no defaults.  The
/// original shape of the test was tautological — it passed equally on pre-
/// and post-1834 code — and therefore provided no regression coverage.
///
/// This replacement test asserts the parser behavior directly so the
/// syntactic limitation is explicit in the test suite: `trait B { let x :
/// Length }` must compile to a trait with zero members (no required_members
/// that would produce a `RequirementKind::Let`, and no defaults for `x`).
/// If `let x : Type` without a value becomes syntactically valid in the
/// future, this test will start failing — at which point it should be
/// replaced (or augmented) with the full A+B conformance scenario the
/// original step-5 envisioned, exercising `available_defaults` for
/// unannotated-let vs. typed-let-requirement matching.
///
/// Tracking the coverage gap: a `RequirementKind::Let`-satisfied-by-
/// inferred-default scenario cannot be constructed from source today.  Filed
/// as a follow-up on task 1834; no separate tracker task exists yet.
#[test]
fn let_with_type_and_no_value_parses_as_empty_trait() {
    let source = r#"
trait B {
    let x : Length
}
    "#;
    let module = compile_source(source);

    let b = module
        .trait_defs
        .iter()
        .find(|t| t.name == "B")
        .expect("expected trait B to be present in the compiled module");

    assert!(
        b.required_members.is_empty(),
        "`let x : Length` without a value must not produce a RequirementKind::Let \
         (the parser returns None when the value child is missing). \
         Got required_members = {:?}",
        b.required_members
    );
    assert!(
        b.defaults.iter().all(|d| d.name.as_deref() != Some("x")),
        "`let x : Length` without a value must not produce a default for `x`. \
         Got defaults = {:?}",
        b.defaults
    );
}

// ── task 1834 amendment: two-pass pre-register restores annotated forward refs ──

/// Regression guard for the two-pass pre-register split
/// (reviewer_comprehensive behavior_regression fix): an *unannotated* let
/// whose expression forward-references an *annotated* member from the same
/// trait-bound set must compile cleanly — the annotated type is registered
/// by Pass 1 before any expression is compiled in Pass 2.
///
/// Before the split, the pre-register loop walked `ctx.defaults` in source
/// order and compiled unannotated-let expressions inline, so `let a = b + 1mm`
/// appearing before `let b : Length = 2mm` produced a spurious
/// `unresolved name: b` diagnostic — a silent regression vs. the pre-1834
/// code, which registered every annotated type up front.  The two-pass
/// structure (annotated-first, then unannotated-with-compile) restores the
/// old tolerance without re-introducing the `Type::Real` fallback.
///
/// Flip-guard for the sibling limitation test: this scenario is `mixed`
/// (one annotated, one unannotated).  The purely-unannotated mutual case
/// stays documented in `mutual_unannotated_lets_documented_limitation`
/// below — only a topological ordering pass would resolve that one.
#[test]
fn unannotated_let_resolves_forward_reference_to_annotated_let() {
    let source = r#"
trait MixedLets {
    let a = b + 1mm
    let b : Length = 2mm
}
structure S : MixedLets {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "an unannotated let (`a = b + 1mm`) forward-referencing an *annotated* \
         let (`b : Length = 2mm`) must resolve cleanly via the two-pass \
         pre-register pass — Pass 1 registers `b : Length` before Pass 2 \
         compiles `a`'s expression.  A single-pass implementation would emit \
         `unresolved name: b` here; got: {:?}",
        errors
    );

    // The injected cell for `a` must have its inferred Length type, confirming
    // Pass 2's `compile_expr` saw `b : Length` in scope and typed `b + 1mm`
    // correctly.
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");
    let a_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "a")
        .expect("expected value_cell 'a' injected from trait MixedLets");
    assert_eq!(
        a_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "cell for `a` must have inferred type Length (Pass 2 saw `b : Length` \
         in scope via Pass 1's annotated-first registration)"
    );
}

/// Regression guard for the Param arm of Pass 1 in the two-pass pre-register
/// split: an *unannotated* let whose expression forward-references an
/// *annotated param* from the same trait-bound set must compile cleanly.
///
/// This completes the matrix started by
/// `unannotated_let_resolves_forward_reference_to_annotated_let` above:
///   - that sibling tests the `DefaultKind::Let { cell_type: Some(…) }` arm of
///     Pass 1 (annotated Let registers in Pass 1).
///   - this test exercises the `DefaultKind::Param` arm: `param x : Length = 2mm`
///     is registered by Pass 1 exactly as annotated Lets are, so Pass 2 sees
///     `x : Length` in scope when it compiles `let a = x + 1mm`.
///
/// Declaration order is inverted (`let a …` before `param x …`) so that a
/// single-pass implementation that walked `ctx.defaults` in source order would
/// fail to resolve `x` when compiling `a`'s expression.  The two-pass split
/// resolves this because Pass 1 registers all Param types before any unannotated
/// Let expression is compiled in Pass 2.
#[test]
fn unannotated_let_resolves_forward_reference_to_annotated_param() {
    let source = r#"
trait WithParam {
    let a = x + 1mm
    param x : Length = 2mm
}
structure S : WithParam {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "an unannotated let (`a = x + 1mm`) forward-referencing an *annotated param* \
         (`param x : Length = 2mm`) must resolve cleanly via the two-pass \
         pre-register pass — Pass 1 registers `x : Length` (Param arm) before \
         Pass 2 compiles `a`'s expression.  Got: {:?}",
        errors
    );

    // The injected cell for `a` must carry the inferred Length type, confirming
    // Pass 2's `compile_expr` saw `x : Length` in scope and typed `x + 1mm`
    // as Length (not Real, which would indicate Pass 1 missed the Param arm).
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");
    let a_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "a")
        .expect("expected value_cell 'a' injected from trait WithParam");
    assert_eq!(
        a_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "cell for `a` must have inferred type Length (Pass 2 saw `x : Length` \
         in scope via Pass 1's Param-arm registration)"
    );
}

// ── task 1834 step-6: unannotated let expression type flows into scope/cell ──

/// Trait T has an unannotated `let x = 5mm` and a co-trait constraint
/// `x + 1mm > 2mm` that references `x`.  The constraint expression compiles in
/// the scope built during the pre-register pass: before task 1834 that scope
/// registered `x : Real` for every unannotated let (the `.unwrap_or(Type::Real)`
/// fallback), so the addition `x + 1mm` became `Real + Length` which trips the
/// "dimensioned + dimensionless" dimension-mismatch check in expr.rs:290.
///
/// After the fix, the pre-register pass infers the let's expression type in the
/// partial scope and registers `x : Length`; the addition becomes
/// `Length + Length` and compiles cleanly.  We assert two things in one test:
///
/// 1. The compilation emits no error diagnostics (covers the scope path —
///    specifically, that the `+` dimension check sees `x : Length`, not Real).
/// 2. The injected `ValueCellDecl.cell_type` for `x` is
///    `Type::Scalar{LENGTH}` (covers the injection-site path).
///
/// Addition — not comparison — is used because reify's comparison operators
/// (`>`, `<`, ...) do not enforce dimensional compatibility between operands
/// (see expr.rs:289–333 — only `BinOp::Add | BinOp::Sub` trigger the check),
/// so a `x > 0mm` constraint would pass trivially even with `x : Real`.
#[test]
fn unannotated_let_scope_uses_inferred_type() {
    let source = r#"
trait T {
    let x = 5mm
    constraint x + 1mm > 2mm
}
structure S : T {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "structure S : T with unannotated `let x = 5mm` and constraint \
         `x + 1mm > 2mm` should compile without dimension-mismatch errors — the \
         pre-register pass must infer `x : Length`, not fall back to `Type::Real` \
         (got: {:?})",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");

    let x_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("expected value_cell 'x' to be injected from trait T");

    assert_eq!(
        x_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "unannotated `let x = 5mm` must infer `cell_type = Type::length()` on \
         the injected cell, not fall back to Type::Real"
    );
}

// ── task 1834 step-10: documented simplification for mutual unannotated-let refs ──

/// Two unannotated lets in the same trait-bound set that forward-reference each
/// other: `let a = b + 1mm` depends on `let b = 2mm`, with no annotation on
/// either.  The type-inference pass in `conformance.rs` proceeds in
/// `ctx.defaults` iteration order, so the binding that appears first and
/// references the second will fail to resolve its forward reference — `b` is
/// not yet in scope when `a`'s expression is compiled.
///
/// Task 1834 acknowledges this as an intentional simplification: adding an
/// annotation to either binding (e.g., `let b : Length = 2mm`) unblocks the
/// pair because the pre-register pass handles annotated lets before doing any
/// expression compilation via the early-branch match.  A topological
/// ordering pass would fix the general case but is out of scope.
///
/// This test pins the current deterministic failure: `a` appears before `b`
/// in `ctx.defaults` iteration order (declaration order, established by
/// `collect_all_requirements`), so `b` is not yet in scope when `a`'s
/// expression is compiled.  The diagnostic must specifically name the
/// unresolved forward reference `b` (matching `unresolved name: b` from
/// expr.rs:199 or any future wording that quotes the identifier) so that an
/// unrelated regression in a different subsystem — e.g., a dimension-mismatch
/// or panic-recovery message that happens to contain the word "scope" —
/// cannot silently satisfy this test.
///
/// A future topological-ordering pass would make this case compile cleanly
/// (zero errors).  When that lands, flip both assertions: drop the
/// `!errors.is_empty()` guard and require zero errors.  Asserting failure
/// today keeps a silent-success regression (e.g., a refactor that quietly
/// preflights all annotations and bypasses the limitation without an
/// architectural fix) from passing unnoticed.
#[test]
fn mutual_unannotated_lets_documented_limitation() {
    let source = r#"
trait MutualLets {
    let a = b + 1mm
    let b = 2mm
}
structure S : MutualLets {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "mutual unannotated-let limitation should surface a diagnostic today \
         (forward reference to `b` from `a`); silent success would mean either \
         the topological pass landed (flip this assertion) or the inference \
         pass started preflighting annotations without an architectural fix"
    );
    assert!(
        diagnostic_names_unresolved(&errors, "b"),
        "mutual unannotated-let diagnostic should surface an unresolved-identifier \
         error that names `b` (the forward reference), got: {:?}",
        errors
    );
}

// ── task 1914 step-1: error-cascade suppression for unannotated-let compile failure ──

/// Regression test for error-cascade suppression when an unannotated `let` default's
/// expression fails to compile (esc-1834-144).
///
/// ## Scenario
///
/// `trait HasA { let a = b + 1mm }` provides an unannotated let default whose
/// expression contains a forward reference to `b`, which is not defined anywhere.
/// `structure S : HasA {}` inherits the default without overriding it.
///
/// ## What this test locks in
///
/// **Single root-cause diagnostic — no secondary phantom cascade.**  Exactly one
/// root-cause "unresolved `b`" error must be present.  No cascade diagnostics of
/// the form "available default has Real" (which would indicate a poisoned
/// `inferred_let_exprs` entry was advertised as a valid default) and no
/// "type mismatch for trait member 'a'" phantom from the same root cause.
///
/// The error count is pinned at `<= 1` as a prose-independent backstop (check (d)):
/// any new cascade diagnostic, regardless of wording, would push the count above 1
/// and fail that check.  Empirically, today's compiler emits exactly one error for
/// this input: `"unresolved name: b"` with label span `[26..27]`.  If a legitimate
/// upstream phase ever begins emitting a second root-cause diagnostic for the same
/// identifier, the bound may need to be relaxed — but any such relaxation must be
/// audited to confirm the extra diagnostic is not a cascade.
///
/// ## Pre-fix behaviour (task 1914 step-2 fixes this)
///
/// Before the fix, Pass 2 of `check_phase_pre_register_default_types` inserted the
/// poisoned `compile_expr` result (Type::Real or Type::Error) into
/// `inferred_let_exprs`, and `check_phase_build_available_defaults_map` advertised a
/// phantom `("a", Let) → Type::Real` entry.  Any trait requirement matching against
/// that phantom entry would then emit a spurious "available default has Real"
/// type-mismatch cascade on top of the original root-cause diagnostic.
#[test]
fn unannotated_let_with_unresolved_ref_does_not_cascade_type_mismatch() {
    let source = r#"
trait HasA {
    let a = b + 1mm
}
structure S : HasA {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // (a) The root-cause "unresolved `b`" error must be present.
    assert!(
        diagnostic_names_unresolved(&errors, "b"),
        "expected at least one diagnostic naming the unresolved identifier `b`; \
         got: {:?}",
        errors
    );

    // (a2) Span-based root-cause assertion (prose-independent).
    // The span is the semantic anchor: it pins the exact source location of the
    // unresolved identifier `b` regardless of any future rewording of the
    // diagnostic message (e.g. "cannot resolve", "name lookup failed").
    // `"b +"` is the unique disambiguating substring — the only `b` in the source
    // that is immediately followed by ` +` — giving us the byte offset of the
    // identifier reference in `let a = b + 1mm`.
    {
        let b_offset = source.find("b +").expect("test source must contain 'b +'") as u32;
        let has_span_on_b = errors.iter().any(|d| {
            d.labels.iter().any(|label| {
                label.span.start == b_offset
                    && label.span.end == b_offset + 1
                    && &source[label.span.start as usize..label.span.end as usize] == "b"
            })
        });
        assert!(
            has_span_on_b,
            "expected at least one error diagnostic with a label whose span \
             covers exactly the `b` identifier at byte offset {}..{}; \
             got errors: {:?}",
            b_offset,
            b_offset + 1,
            errors
        );
    }

    // (b) No cascade "available default has Real" diagnostic.
    // A phantom `("a", Let) -> Type::Real` advertisement would cause
    // check_phase_check_members_against_requirements to emit this substring
    // when comparing against any requirement with a non-Real expected type.
    let cascade_diags: Vec<_> = errors
        .iter()
        .filter(|d| d.message.contains("available default") && d.message.contains("Real"))
        .collect();
    assert!(
        cascade_diags.is_empty(),
        "phantom cascade diagnostic found: a poisoned inferred-let entry was \
         advertised in available_defaults, triggering a secondary type-mismatch \
         on top of the root-cause 'unresolved b' error. \
         Cascade diagnostics: {:?}",
        cascade_diags
    );

    // (c) No "type mismatch for trait member 'a'" phantom diagnostic.
    let trait_member_mismatch: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.message.contains("type mismatch for trait member") && d.message.contains("'a'")
        })
        .collect();
    assert!(
        trait_member_mismatch.is_empty(),
        "phantom 'type mismatch for trait member' diagnostic found for 'a'; \
         this is a cascade from the failed compile_expr for the unannotated let \
         expression. Diagnostics: {:?}",
        trait_member_mismatch
    );

    // (d) Count backstop: cascade suppression means exactly the root-cause
    // "unresolved `b`" diagnostic — nothing else. A count above 1 indicates
    // either a new cascade surface (regardless of wording) or an unrelated
    // regression. Prose-independent.
    assert!(
        errors.len() <= 1,
        "expected at most one root-cause diagnostic after cascade suppression, \
         got {}: {:?}",
        errors.len(),
        errors,
    );
}

/// Reverse-order companion to `mutual_unannotated_lets_documented_limitation`:
/// declaring `let b = 2mm` *before* `let a = b + 1mm` compiles cleanly, because
/// the pre-register/inference pass in `conformance.rs` walks `ctx.defaults` in
/// declaration order and `b` is already in scope by the time `a`'s expression
/// is compiled.
///
/// This test pins declaration order as the contract: the iteration order of
/// `ctx.defaults` is established by `collect_all_requirements` (see
/// `crates/reify-compiler/src/conformance.rs` — push order during the
/// requirements walk), and swapping the two declarations changes the
/// observable outcome from "unresolved `b`" to "clean compile".  If a future
/// refactor reorders `ctx.defaults` (e.g., sorts by name for deterministic
/// output), *both* this test AND
/// `mutual_unannotated_lets_documented_limitation` must be re-evaluated:
/// - Alphabetical sort → `a, b` ordering in both variants → both fail the
///   forward-reference → the `!errors.is_empty()` assertion in the sibling
///   test would still hold, but *this* test would flip to failure, surfacing
///   the contract change.
/// - Topological sort → both variants compile cleanly → flip the sibling's
///   assertion to zero-errors; this test keeps passing.
#[test]
fn unannotated_lets_reverse_order_compiles_cleanly() {
    let source = r#"
trait ReverseOrder {
    let b = 2mm
    let a = b + 1mm
}
structure S : ReverseOrder {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        errors.is_empty(),
        "reverse-order variant (`let b = 2mm` before `let a = b + 1mm`) should \
         compile without errors — `b` is registered in scope by the \
         pre-register pass before `a`'s expression is compiled.  If this \
         starts failing, the declaration-order contract of `ctx.defaults` \
         iteration in conformance.rs may have changed; see \
         `mutual_unannotated_lets_documented_limitation` for the paired \
         expectation.  Got: {:?}",
        errors
    );
}

/// Regression test: chained unannotated lets must not cascade a secondary
/// "unresolved name" diagnostic for the failed let's own name (task 2158 step-3).
///
/// ## Scenario
///
/// `trait T { let a = b + 1mm; let c = a * 2mm }` — `a` depends on `b`, which is
/// undefined.  Without scope poisoning, Pass 2 of `check_phase_pre_register_default_types`
/// compiles `a`'s expression, gets an "unresolved name: b" error, records `a` in
/// `pass2_compile_errors` — but does NOT register `a` in scope.  When it then compiles
/// `c`'s expression, the scope lookup for `a` fails and emits a SECOND "unresolved name: a"
/// cascade on top of the root-cause error.
///
/// ## What this test locks in (companion to `unannotated_let_with_unresolved_ref_does_not_cascade_type_mismatch`)
///
/// - **(a) Root-cause present**: at least one diagnostic names the unresolved `b`.
/// - **(b) No cascade**: NO diagnostic names an unresolved `a`.  After task 2158's
///   scope-poison fix (`register_if_absent(name, Type::Error)` in the compile-error branch),
///   `c`'s `compile_expr` resolves `a` to `Type::Error` (the sentinel) rather than emitting
///   a new unresolved-name diagnostic.
/// - **(c) No phantom advertisement cascade**: no "available default has Real" and no
///   "type mismatch for trait member 'a'" phantom diagnostics — mirroring the single-let
///   companion test so both failure modes are pinned at the chained-let site.
///
/// This test FAILS before task 2158 step-4 (the scope-poison impl): assertion (b) fails
/// because the unregistered `a` causes a cascade "unresolved name: a" when compiling `c`.
#[test]
fn chained_unannotated_lets_with_unresolved_ref_do_not_cascade() {
    let source = r#"
trait T {
    let a = b + 1mm
    let c = a * 2mm
}
structure S : T {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // (a) The root-cause "unresolved `b`" error must be present.
    assert!(
        diagnostic_names_unresolved(&errors, "b"),
        "expected at least one diagnostic naming the unresolved identifier `b` \
         (root cause); got: {:?}",
        errors
    );

    // (b) No cascade "unresolved `a`" diagnostic.
    // Without scope poisoning, compiling `c = a * 2mm` finds `a` unregistered
    // in scope and emits a fresh "unresolved name: a" — a cascade on top of
    // the root-cause "unresolved b" error.  After the fix, Pass 2 poisons `a`'s
    // scope slot with Type::Error so the lookup for `a` succeeds (returning the
    // sentinel) without emitting a new diagnostic.
    assert!(
        !diagnostic_names_unresolved(&errors, "a"),
        "cascade diagnostic found naming unresolved identifier `a`: this indicates \
         Pass 2's compile-failure branch did not register `a` as a Type::Error poison \
         sentinel in scope, so compiling `c = a * 2mm` emitted a secondary unresolved-name \
         diagnostic instead of silently propagating Type::Error.  \
         All error diagnostics: {:?}",
        errors
    );

    // (c) No cascade "available default has Real" diagnostic.
    // A phantom `("a", Let) -> Type::Real` advertisement would cause
    // check_phase_check_members_against_requirements to emit this substring.
    let cascade_diags: Vec<_> = errors
        .iter()
        .filter(|d| d.message.contains("available default") && d.message.contains("Real"))
        .collect();
    assert!(
        cascade_diags.is_empty(),
        "phantom cascade diagnostic found: a poisoned inferred-let entry was \
         advertised in available_defaults, triggering a secondary type-mismatch \
         on top of the root-cause 'unresolved b' error. \
         Cascade diagnostics: {:?}",
        cascade_diags
    );

    // (d) No "type mismatch for trait member 'a'" phantom diagnostic.
    let trait_member_mismatch_a: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.message.contains("type mismatch for trait member") && d.message.contains("'a'")
        })
        .collect();
    assert!(
        trait_member_mismatch_a.is_empty(),
        "phantom 'type mismatch for trait member' diagnostic found for 'a'; \
         this is a cascade from the failed compile_expr for the unannotated let \
         expression. Diagnostics: {:?}",
        trait_member_mismatch_a
    );
}

// ── task 3184: int-vs-real AST distinction — compiler-level tests ─────────────

/// RED test for task 3184: `1.0` (a whole-number decimal literal) must lower to
/// `Value::Real(1.0)` with `result_type == Type::Real`, not `Value::Int(1)`.
///
/// Before the fix, `expr.rs` used a value-based heuristic: any f64 that equals
/// its integer cast becomes `Int`. So `1.0` → `Int(1)`. This test FAILS until
/// step-4 replaces the heuristic with the `is_real` flag added to the AST in
/// step-2.
///
/// The `cell_type` assertion (annotation authoritative) passes both before and
/// after the fix. Only `default_expr.result_type` and `default_expr.kind` are
/// the RED assertions.
#[test]
fn whole_number_real_literal_compiles_as_real() {
    let source = r#"
trait HasX {
    let x : Real = 1.0
}
structure S : HasX {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");

    let x_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("expected value_cell 'x' to be injected from trait HasX");

    assert_eq!(
        x_cell.cell_type,
        Type::Real,
        "cell_type must be Type::Real (annotation authoritative)"
    );

    let default_expr = x_cell
        .default_expr
        .as_ref()
        .expect("expected a default_expr on the let cell for 'x'");

    assert_eq!(
        default_expr.result_type,
        Type::Real,
        "default_expr.result_type must be Type::Real for `1.0`; \
         before task 3184 step-4 it was Type::Int (value-based heuristic). Got: {:?}",
        default_expr.result_type
    );

    match &default_expr.kind {
        CompiledExprKind::Literal(Value::Real(v)) => assert_eq!(
            *v, 1.0,
            "default_expr.kind must be Literal(Value::Real(1.0)), got value {}",
            v
        ),
        other => panic!(
            "default_expr.kind must be Literal(Value::Real(1.0)) — \
             before task 3184 step-4 it was Literal(Value::Int(1)); got: {:?}",
            other
        ),
    }
}

/// Regression guard for task 3184 step-4: `let x : Real = 42` (bare integer token,
/// no `.`/`e`/`E`) must continue to compile cleanly via Int→Real widening.
///
/// The `is_real` flag for `42` is false, so the expression lowers as `Int(42)`.
/// The annotation `: Real` is accepted via `type_compatible` at the cross-check site.
///
/// Pins two invariants:
///   1. No error diagnostic — widening still works after the fix.
///   2. The raw `default_expr` carries `Value::Int(42)`, not `Value::Real(42.0)`.
///      Widening happens at the annotation cross-check layer, not at literal lowering.
#[test]
fn integer_literal_in_real_param_still_widens() {
    let source = r#"
trait HasX {
    let x : Real = 42
}
structure S : HasX {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors — `let x : Real = 42` must widen Int to Real via \
         `type_compatible` at the annotation cross-check site. Got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");

    let x_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("expected value_cell 'x' to be injected from trait HasX");

    assert_eq!(
        x_cell.cell_type,
        Type::Real,
        "cell_type must be Type::Real (annotation authoritative)"
    );

    let default_expr = x_cell
        .default_expr
        .as_ref()
        .expect("expected a default_expr on the let cell for 'x'");

    match &default_expr.kind {
        CompiledExprKind::Literal(Value::Int(v)) => assert_eq!(
            *v, 42,
            "default_expr.kind must be Literal(Value::Int(42)), got value {}",
            v
        ),
        other => panic!(
            "default_expr.kind must be Literal(Value::Int(42)) — integer tokens \
             (no `.`/`e`/`E`) always lower as Int even with a Real annotation; got: {:?}",
            other
        ),
    }
}

// ── task 3249: exponent-form Real literal coverage (esc-3184-54) ──────────────

/// Shared compile-and-assert helper for the three exponent-form regression tests
/// below.  Builds `trait HasX { let x : Real = <literal> } structure S : HasX {}`,
/// compiles it, and asserts:
///
/// 1. Zero error diagnostics.
/// 2. `x_cell.cell_type == Type::Real` (annotation authoritative; sanity only).
/// 3. `default_expr.result_type == Type::Real` (catches Int re-classification).
/// 4. `default_expr.kind == Literal(Value::Real(expected_value))` (catches
///    value-typed regression with the most specific signal).
///
/// `label` is a short human-readable tag included in assertion failure messages
/// (e.g. `"lowercase e (1e6)"`) for quick triage.
fn assert_let_real_literal(literal: &str, expected_value: f64, label: &str) {
    let source =
        format!("trait HasX {{\n    let x : Real = {literal}\n}}\nstructure S : HasX {{}}\n");
    let module = compile_source(&source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no compile errors for `{}` literal, got: {:?}",
        label,
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");

    let x_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("expected value_cell 'x' to be injected from trait HasX");

    assert_eq!(
        x_cell.cell_type,
        Type::Real,
        "cell_type must be Type::Real (annotation authoritative) for `{}`",
        label
    );

    let default_expr = x_cell
        .default_expr
        .as_ref()
        .expect("expected a default_expr on the let cell for 'x'");

    assert_eq!(
        default_expr.result_type,
        Type::Real,
        "default_expr.result_type must be Type::Real for `{}`; \
         a regression dropping a character check from the `is_real` classifier in \
         `lower_number_literal` in reify-syntax/src/ts_parser.rs would emit Type::Int \
         for integer-equal exponent forms. Got: {:?}",
        label,
        default_expr.result_type
    );

    match &default_expr.kind {
        CompiledExprKind::Literal(Value::Real(v)) => assert_eq!(
            *v, expected_value,
            "default_expr.kind must be Literal(Value::Real({:?})) for `{}`, got value {}",
            expected_value, label, v
        ),
        other => panic!(
            "default_expr.kind must be Literal(Value::Real({:?})) for `{}` — \
             a regression dropping an `'e'`/`'E'` check from the `is_real` classifier \
             in `lower_number_literal` (reify-syntax/src/ts_parser.rs) would emit \
             Literal(Value::Int(...)) via the integer-cast branch in the NumberLiteral \
             lowering arm of reify-compiler/src/expr.rs for integer-equal exponent forms; \
             got: {:?}",
            expected_value, label, other
        ),
    }
}

/// Regression guard for esc-3184-54: lowercase-e exponent form `1e6` must lower
/// to `Value::Real(1_000_000.0)` with `result_type == Type::Real`.
///
/// Production code under test: the `is_real` classifier in `lower_number_literal`
/// in `crates/reify-syntax/src/ts_parser.rs` (look for `text.contains('e')`).
///
/// Regression model: if the `text.contains('e')` check were dropped from the
/// `is_real` disjunction, `is_real` would be `false` for `1e6`. The f64 value
/// `1_000_000.0` is integer-equal (`1e6 == (1000000_i64) as f64`), so the literal
/// would silently fall through the integer-cast branch in the NumberLiteral lowering
/// arm of `reify-compiler/src/expr.rs` and be emitted as `Value::Int(1000000)` with
/// `Type::Int` — passing the existing test suite. This test is the tripwire that
/// would catch that silent mis-classification.
///
/// The `cell_type` assertion (annotation authoritative) passes both before and after
/// a hypothetical regression. Only `default_expr.result_type` and `default_expr.kind`
/// are the regression-catching assertions.
#[test]
fn exponent_form_lowercase_e_real_literal_compiles_as_real() {
    assert_let_real_literal("1e6", 1_000_000.0, "lowercase e (1e6)");
}

/// Regression guard for esc-3184-54: uppercase-E exponent form `1E6` must lower
/// to `Value::Real(1_000_000.0)` with `result_type == Type::Real`.
///
/// Production code under test: the `is_real` classifier in `lower_number_literal`
/// in `crates/reify-syntax/src/ts_parser.rs` (look for `text.contains('E')`).
///
/// Regression model: if the `text.contains('E')` check were dropped from the
/// `is_real` disjunction, `is_real` would be `false` for `1E6`. The f64 value
/// `1_000_000.0` is integer-equal (`1E6 == (1000000_i64) as f64`), so the literal
/// would silently fall through the integer-cast branch in the NumberLiteral lowering
/// arm of `reify-compiler/src/expr.rs` and be emitted as `Value::Int(1000000)` with
/// `Type::Int` — passing the existing test suite. This test is the tripwire that
/// would catch that silent mis-classification.
///
/// The `cell_type` assertion (annotation authoritative) passes both before and after
/// a hypothetical regression. Only `default_expr.result_type` and `default_expr.kind`
/// are the regression-catching assertions.
#[test]
fn exponent_form_uppercase_e_real_literal_compiles_as_real() {
    assert_let_real_literal("1E6", 1_000_000.0, "uppercase E (1E6)");
}

/// Value-preservation sanity check for esc-3184-54: negative-exponent form `1e-5`
/// must lower to `Value::Real(1e-5_f64)` with `result_type == Type::Real`.
///
/// Production code under test: the `is_real` classifier in `lower_number_literal`
/// in `crates/reify-syntax/src/ts_parser.rs` (look for `text.contains('e')`).
///
/// **Note:** unlike the `1e6`/`1E6` tests, this case **cannot** serve as a
/// regression tripwire for esc-3184-54. The f64 value `0.00001` is NOT integer-equal,
/// so a regression dropping `'e'` from the `is_real` disjunction would set
/// `is_real = false`, but the literal would still reach `Value::Real(0.00001)` via
/// the non-integer-equal Real fallback in the NumberLiteral lowering arm of
/// `reify-compiler/src/expr.rs` — both `result_type == Type::Real` and
/// `Value::Real(0.00001)` would still hold. This test is a **value-preservation
/// sanity check** for the negative-exponent code path, not a regression tripwire.
#[test]
fn exponent_form_negative_exponent_real_literal_compiles_as_real() {
    assert_let_real_literal("1e-5", 1e-5_f64, "negative exponent (1e-5)");
}
