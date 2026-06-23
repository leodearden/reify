//! Shadowing warning tests (spec §8.5, PRD docs/prds/shadowing-warning.md).
//!
//! The lint walks AST scopes once and emits a Warning diagnostic with
//! [`DiagnosticCode::Shadowing`] when a child-scope declaration uses the same
//! name as a name visible from an enclosing parent scope.

use reify_test_support::{compile_source, compile_source_with_stdlib, warnings_only};
use reify_core::{DiagnosticCode, Severity};

/// Basic lambda-shadows-entity-param case: a lambda parameter `x` declared
/// inside a structure that already declares `param x` MUST emit exactly one
/// `Shadowing` warning. The warning carries two labels — the lambda's `x`
/// site (child) and the entity's `param x` site (original) — with non-empty,
/// distinct spans.
#[test]
fn lambda_param_shadows_entity_param_emits_w_shadow() {
    let source = r#"
structure S {
    param x : Real = 1
    let f = |x| x * 2
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning, got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert_eq!(warning.severity, Severity::Warning);

    assert_eq!(
        warning.labels.len(),
        2,
        "Shadowing warning must carry two labels (child + original), got: {:?}",
        warning.labels
    );

    let l0 = &warning.labels[0];
    let l1 = &warning.labels[1];
    assert!(
        !l0.span.is_empty(),
        "child-site label span must be non-empty, got: {:?}",
        l0.span
    );
    assert!(
        !l1.span.is_empty(),
        "original-decl label span must be non-empty, got: {:?}",
        l1.span
    );
    assert_ne!(
        l0.span, l1.span,
        "child-site and original-decl spans must be distinct, both = {:?}",
        l0.span
    );
}

/// Quantifier-bound variable case: a `forall x in coll: pred` whose `x`
/// matches an entity-scope `param x` MUST emit one Shadowing warning. The
/// child span overlaps the quantifier expression (per design decision §5
/// — `Expr.span` is the binder span available without an AST extension);
/// the original-decl span is at the entity's `param x`.
#[test]
fn quantifier_variable_shadows_entity_param_emits_w_shadow() {
    let source = r#"
structure S {
    param x : Real = 0
    constraint forall x in [1, 2, 3]: x > 0
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning for quantifier-shadow case, got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert_eq!(warning.severity, Severity::Warning);
    assert_eq!(
        warning.labels.len(),
        2,
        "Shadowing warning must carry two labels (child + original), got: {:?}",
        warning.labels
    );

    let l0 = &warning.labels[0];
    let l1 = &warning.labels[1];
    assert!(
        !l0.span.is_empty(),
        "child-site label span must be non-empty, got: {:?}",
        l0.span
    );
    assert!(
        !l1.span.is_empty(),
        "original-decl label span must be non-empty, got: {:?}",
        l1.span
    );
    assert_ne!(
        l0.span, l1.span,
        "child-site and original-decl spans must be distinct, both = {:?}",
        l0.span
    );

    // The original-decl span should land at the entity's `param x` decl.
    // Locate `param x` in the source and verify the original-decl label
    // (l1) covers it. The simplest verification is that the original-decl
    // span sits BEFORE the quantifier-expression span (the entity's param
    // appears before the constraint that uses it).
    assert!(
        l1.span.start < l0.span.start,
        "original-decl span ({:?}) must precede the child-site span ({:?})",
        l1.span,
        l0.span
    );
}

/// Nearest-visible-parent rule: with `param x` at the entity scope and a
/// nested lambda `|y| (|x| x + y)`, the inner `|x|` MUST shadow the entity's
/// `param x` (NOT the outer lambda's `y`, because `y` is unique). Exactly
/// one Shadowing warning is expected — for the inner lambda's `x` against
/// the entity-scope `x`. The outer lambda's `y` does not shadow anything.
#[test]
fn nested_lambda_shadow_points_at_nearest_visible_parent() {
    let source = r#"
structure S {
    param x : Real = 1
    let f = |y| (|x| x + y)
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning (inner |x| vs entity param x), got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert!(
        warning.message.contains("'x'"),
        "expected the warning to be about `x`, got message: {:?}",
        warning.message
    );
    assert_eq!(warning.labels.len(), 2);

    // Locate the entity's `param x` start (the `param` keyword sits at the
    // beginning of `param x : Real = 1`). Locate the inner lambda's `|x|`
    // start. The original-decl label must point at the entity's `param x`,
    // i.e. earlier than the inner lambda's `x`.
    let param_x = source
        .find("param x")
        .expect("source must contain `param x`");
    let inner_lambda = source
        .rfind("|x|")
        .expect("source must contain inner `|x|`");

    let l0 = &warning.labels[0]; // child site
    let l1 = &warning.labels[1]; // original-decl site
    assert!(
        (l1.span.start as usize) >= param_x && (l1.span.start as usize) < inner_lambda,
        "original-decl span must point at the entity-scope `param x` \
         (between byte {} and {}), got {:?}",
        param_x,
        inner_lambda,
        l1.span
    );
    assert!(
        (l0.span.start as usize) >= inner_lambda,
        "child-site span must point at the inner lambda's `x` \
         (>= byte {}), got {:?}",
        inner_lambda,
        l0.span
    );
}

/// Sibling lambdas under the same parent that both bind `x` must NOT warn —
/// neither lambda is inside the other; they are mutually-disjoint scopes.
/// Without an entity-scope `x`, neither lambda's `x` shadows anything, so
/// the lint must emit zero Shadowing diagnostics.
#[test]
fn sibling_lambdas_with_same_param_do_not_warn() {
    let source = r#"
structure S {
    let f = |x| x
    let g = |x| x * 2
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        0,
        "sibling lambdas with the same param must NOT shadow each other; \
         got: {:?}",
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );
}

/// A lambda parameter with a unique name (different from any visible
/// parent-scope name) must NOT warn. Even with `param x` at the entity
/// scope, a lambda `|y| y * x` introduces no shadow on either `y` (unique)
/// or `x` (referenced, not bound).
#[test]
fn lambda_param_with_unique_name_does_not_warn() {
    let source = r#"
structure S {
    param x : Real = 1
    let f = |y| y * x
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        0,
        "unique lambda param name must NOT trigger a shadow warning; got: {:?}",
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );
}

/// Match-arm-style guarded-group siblings must NOT shadow each other.
///
/// Per spec §6.4, `where cond { … } else { … }` desugaring registers same-name
/// members across the two branches as mutually-exclusive siblings into the
/// SAME parent scope — not as a child scope. Declaring `param head` in both
/// branches is a sibling guarded-decl group, not a shadow.
///
/// The plan's example used `sub head : HexHead/SocketHead`, but `sub` inside
/// guarded blocks is currently rejected as "not yet supported" by the
/// guard-compilation phase (see `recursive_sub_inside_block_guard_emits_
/// unsupported_error`). The shadow-lint contract is identical for any
/// member-decl variant — the lint folds both branches into the same parent
/// frame regardless — so we exercise it with `param`, which is supported.
#[test]
fn match_arm_style_guarded_subs_do_not_warn() {
    let source = r#"
structure S {
    param cond : Bool = true
    where cond {
        param head : Real = 1
    } else {
        param head : Real = 2
    }
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        0,
        "guarded-group sibling members with the same name must NOT shadow; \
         got: {:?}",
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );
}

/// Trait-merge of identical members must NOT shadow.
///
/// Per spec §8.8, when a structure conforms to two traits that both require a
/// member of the same name (e.g. `param mass : Mass`), a single declaration on
/// the structure satisfies BOTH trait requirements — that is "trait-merge", not
/// shadowing. The lint walks ONLY the structure's own member list and never
/// folds trait member sets into the frame, so this case is handled
/// automatically by single-source iteration.
///
/// Compiles with stdlib so `Mass` (and the unit suffix `kg`) resolve.
#[test]
fn trait_merged_member_does_not_warn() {
    let source = r#"
trait TraitA { param mass : Mass }
trait TraitB { param mass : Mass }
structure def S : TraitA + TraitB { param mass : Mass = 1kg }
"#;
    let module = compile_source_with_stdlib(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        0,
        "trait-merged identical-name member (single decl satisfying two trait \
         requirements per §8.8) must NOT trigger a shadow warning; got: {:?}",
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );
}

/// Imported names do NOT form a parent scope (spec §8.11).
///
/// In the source below, both `import std.math.sqrt` and `let sqrt = 1.0` exist
/// at module/structure level, and a lambda `|sqrt|` rebinds it. Only ONE
/// shadow is permissible: the lambda's `sqrt` shadows the structure's
/// `let sqrt`. The IMPORT itself MUST NOT enter any frame, so the
/// structure's `let sqrt` MUST NOT be flagged as shadowing the import.
///
/// We assert the lint emits exactly ONE Shadowing warning (the lambda
/// shadow), and that the warning's child-site span is at the lambda's `|sqrt|`
/// — NOT at the structure's `let sqrt` (which would indicate the import
/// erroneously entered a parent scope).
///
/// Compiles WITHOUT `compile_source_with_stdlib` because the parse-only path
/// is sufficient: an unresolved import still produces a `Declaration::Import`
/// AST node, which the lint must explicitly skip per spec §8.11. Any
/// import-resolution diagnostics produced are unrelated to the Shadowing
/// invariant under test.
#[test]
fn imported_name_does_not_form_parent_scope() {
    let source = r#"
import std.math.sqrt
structure def S {
    let sqrt = 1.0
    let f = |sqrt| sqrt + 1.0
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    // Exactly ONE Shadowing warning is permissible: the lambda's `sqrt`
    // shadows the structure's `let sqrt`. If the import erroneously formed a
    // parent scope, we'd see TWO (the let shadowing the import + the lambda
    // shadowing the let).
    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning (lambda |sqrt| vs structure \
         let sqrt) — imports must not form a parent scope per §8.11; got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    // Verify the single warning's child site is the lambda's `|sqrt|`, not
    // the structure's `let sqrt`. The lambda appears AFTER the let in the
    // source, so the child-site span must come after the `let sqrt` byte.
    let warning = shadow_warnings[0];
    assert_eq!(warning.labels.len(), 2);
    let let_sqrt = source
        .find("let sqrt")
        .expect("source must contain `let sqrt`");
    let lambda_sqrt = source.find("|sqrt|").expect("source must contain `|sqrt|`");
    let child_label = &warning.labels[0];
    assert!(
        (child_label.span.start as usize) >= lambda_sqrt,
        "child-site span must point at the lambda's `|sqrt|` (>= byte {}), \
         not at the structure's `let sqrt` (byte {}); got {:?}",
        lambda_sqrt,
        let_sqrt,
        child_label.span
    );
}

/// A lambda parameter inside a function body shadows the outer fn param.
///
/// `fn area(w: Length, h: Length) -> Length { let f = |w| w * h ; f(w) }`:
/// the lambda's `w` shadows the fn param `w`. Exactly ONE Shadowing warning
/// is expected. The original-decl span points at the fn param `w`; the
/// child-site span at the lambda's `|w|`.
///
/// Uses `;` between the let binding and the result expression because the
/// parser does not currently treat a newline as a statement separator
/// between a let-with-lambda and a function-call result.
#[test]
fn lambda_in_fn_body_shadows_fn_param() {
    let source = r#"fn area(w: Length, h: Length) -> Length { let f = |w| w * h ; f(w) }"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning (lambda |w| vs fn param w), \
         got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert!(
        warning.message.contains("'w'"),
        "expected the warning to be about `w`, got message: {:?}",
        warning.message
    );
    assert_eq!(warning.labels.len(), 2);

    // The fn param `w` appears before the lambda's `|w|`. Verify the
    // original-decl label sits between the `fn area(` and the lambda.
    let fn_param_w = source
        .find("(w:")
        .expect("source must contain `(w:` (fn signature)");
    let lambda_w = source.find("|w|").expect("source must contain `|w|`");

    let l0 = &warning.labels[0]; // child site
    let l1 = &warning.labels[1]; // original-decl site
    assert!(
        (l1.span.start as usize) >= fn_param_w && (l1.span.start as usize) < lambda_w,
        "original-decl span must point at the fn param `w` \
         (between byte {} and {}), got {:?}",
        fn_param_w,
        lambda_w,
        l1.span
    );
    assert!(
        (l0.span.start as usize) >= lambda_w,
        "child-site span must point at the lambda's `|w|` (>= byte {}), \
         got {:?}",
        lambda_w,
        l0.span
    );
}

/// A lambda inside a constraint-def predicate shadows the constraint def's
/// own param. Source:
///
/// ```text
/// constraint def C {
///     param wall : Real
///     (|wall| wall > 0.0)(wall)
/// }
/// ```
///
/// The `(|wall| ...)` lambda binds `wall` in its body, shadowing the
/// constraint def's `param wall`. Exactly ONE Shadowing warning is expected;
/// the original-decl span lies on the constraint def's `param wall` and
/// the child-site span on the lambda's `|wall|`.
#[test]
fn lambda_in_constraint_def_predicate_shadows_param() {
    let source = r#"constraint def C {
    param wall : Real
    (|wall| wall > 0.0)(wall)
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning (lambda |wall| vs constraint def param wall), \
         got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert!(
        warning.message.contains("'wall'"),
        "expected the warning to be about `wall`, got message: {:?}",
        warning.message
    );
    assert_eq!(warning.labels.len(), 2);

    // Locate `param wall` and the inner lambda binder `|wall|`.
    let param_wall = source
        .find("param wall")
        .expect("source must contain `param wall`");
    let lambda_wall = source.find("|wall|").expect("source must contain `|wall|`");

    let l0 = &warning.labels[0]; // child site
    let l1 = &warning.labels[1]; // original-decl site
    assert!(
        (l1.span.start as usize) >= param_wall && (l1.span.start as usize) < lambda_wall,
        "original-decl span must point at the constraint def `param wall` \
         (between byte {} and {}), got {:?}",
        param_wall,
        lambda_wall,
        l1.span
    );
    assert!(
        (l0.span.start as usize) >= lambda_wall,
        "child-site span must point at the lambda's `|wall|` (>= byte {}), \
         got {:?}",
        lambda_wall,
        l0.span
    );
}

/// A lambda inside a trait member shadows the trait's own param. Source:
///
/// ```text
/// trait T {
///     param mass : Real = 1.0
///     let f = |mass| mass * 2.0
/// }
/// ```
///
/// The lambda binds `mass` in its body, shadowing the trait's
/// `param mass`. Exactly ONE Shadowing warning is expected; the
/// original-decl span lies on the trait's `param mass` and the
/// child-site span on the lambda's `|mass|`.
#[test]
fn lambda_in_trait_default_shadows_trait_param() {
    let source = r#"
trait T {
    param mass : Real = 1.0
    let f = |mass| mass * 2.0
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning (lambda |mass| vs trait param mass), \
         got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert!(
        warning.message.contains("'mass'"),
        "expected the warning to be about `mass`, got message: {:?}",
        warning.message
    );
    assert_eq!(warning.labels.len(), 2);

    let param_mass = source
        .find("param mass")
        .expect("source must contain `param mass`");
    let lambda_mass = source.find("|mass|").expect("source must contain `|mass|`");

    let l0 = &warning.labels[0]; // child site
    let l1 = &warning.labels[1]; // original-decl site
    assert!(
        (l1.span.start as usize) >= param_mass && (l1.span.start as usize) < lambda_mass,
        "original-decl span must point at the trait `param mass` \
         (between byte {} and {}), got {:?}",
        param_mass,
        lambda_mass,
        l1.span
    );
    assert!(
        (l0.span.start as usize) >= lambda_mass,
        "child-site span must point at the lambda's `|mass|` (>= byte {}), \
         got {:?}",
        lambda_mass,
        l0.span
    );
}

/// A nested lambda inside a field's analytical-source expression shadows the
/// outer (domain-binding) lambda. Source:
///
/// ```text
/// field def temp : Point3 -> Vector3 {
///     source = analytical { |p| |p| p }
/// }
/// ```
///
/// The OUTER `|p|` is the field's domain binder; the INNER `|p|` shadows
/// it. (Lambdas are right-associative — see the existing
/// `parse_composed_field` test, `|f, g| |p| f(g(p))`, in
/// `crates/reify-syntax/tests/field_tests.rs:81`.) The shadow is detected
/// naturally by `walk_expr`'s Lambda handling (the outer Lambda pushes a
/// frame `{p}`; the inner Lambda's `p` lookup in that frame finds it).
/// Exactly ONE Shadowing warning is expected.
///
/// The plan's example used `|p| (|p| p + 1.0)(p)` — a function-call of a
/// lambda — which does not parse cleanly inside the analytical block. The
/// shadow-lint contract is identical regardless: `walk_expr`'s Lambda arm
/// is the same code path. We exercise it here with the simpler nested-lambda
/// form, mirroring the precedent established in
/// `match_arm_style_guarded_subs_do_not_warn` and
/// `purpose_constraint_lambda_shadows_purpose_param`.
#[test]
fn field_analytical_lambda_inner_lambda_shadows_outer_param() {
    let source = r#"
field def temp : Point3 -> Vector3 {
    source = analytical { |p| |p| p }
}
"#;
    let module = compile_source_with_stdlib(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning (inner |p| vs outer field-domain |p|), \
         got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert!(
        warning.message.contains("'p'"),
        "expected the warning to be about `p`, got message: {:?}",
        warning.message
    );
    assert_eq!(warning.labels.len(), 2);
}

/// A lambda inside a purpose body shadows the purpose's param.
///
/// The plan's example used `constraint (|subject| true)(subject)`, but the
/// parser rejects a function-call-of-lambda expression in the constraint
/// position with "invalid constraint". The shadow-lint contract is identical
/// regardless of which member-decl variant carries the lambda — `walk_members`
/// routes every embedded expression to `walk_expr` — so we exercise it with a
/// `let`-bound lambda, which the parser accepts.
///
/// Source:
///
/// ```text
/// purpose mfg(subject : Structure) {
///     let f = |subject| subject
///     constraint subject.mass > 0
/// }
/// ```
///
/// The lambda's `subject` shadows the purpose param `subject`. Exactly ONE
/// Shadowing warning is expected; the original-decl span lies on the
/// purpose param `subject`.
#[test]
fn purpose_constraint_lambda_shadows_purpose_param() {
    let source = r#"
purpose mfg(subject : Structure) {
    let f = |subject| subject
    constraint subject.mass > 0
}
"#;
    let module = compile_source_with_stdlib(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning (lambda |subject| vs purpose param subject), \
         got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert!(
        warning.message.contains("'subject'"),
        "expected the warning to be about `subject`, got message: {:?}",
        warning.message
    );
    assert_eq!(warning.labels.len(), 2);

    // The purpose param `subject` appears in the signature `(subject :
    // Structure)`. The lambda's `|subject|` appears inside the let-binding.
    // Verify the original-decl span sits before the lambda.
    let purpose_param = source
        .find("(subject :")
        .expect("source must contain `(subject :`");
    let lambda_subject = source
        .find("|subject|")
        .expect("source must contain `|subject|`");

    let l0 = &warning.labels[0]; // child site
    let l1 = &warning.labels[1]; // original-decl site
    assert!(
        (l1.span.start as usize) >= purpose_param && (l1.span.start as usize) < lambda_subject,
        "original-decl span must point at the purpose param `subject` \
         (between byte {} and {}), got {:?}",
        purpose_param,
        lambda_subject,
        l1.span
    );
    assert!(
        (l0.span.start as usize) >= lambda_subject,
        "child-site span must point at the lambda's `|subject|` (>= byte {}), \
         got {:?}",
        lambda_subject,
        l0.span
    );
}

/// Pin the human-readable wording of the Shadowing diagnostic so downstream
/// golden-style tests don't drift. Source identical to step-3 (single-shadow
/// case): a lambda `|x|` inside a structure declaring `param x`. Asserts:
///
/// - Diagnostic message: literal `"declaration of 'x' shadows enclosing
///   declaration"` (not `contains`, full equality).
/// - Child label message: literal `"shadows the enclosing declaration"`.
/// - Original-decl label message: literal `"originally declared here"`.
///
/// The order is `[child, original]` per `walk_expr`'s Lambda arm
/// (`with_label(child).with_label(parent)`), matching the order asserted by
/// the other span tests in this file (e.g. `lambda_in_fn_body_shadows_fn_param`,
/// `lambda_in_trait_default_shadows_trait_param`).
#[test]
fn shadow_diagnostic_message_format_is_pinned() {
    let source = r#"
structure S {
    param x : Real = 1
    let f = |x| x * 2
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning, got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert_eq!(
        warning.message, "declaration of 'x' shadows enclosing declaration",
        "Shadowing diagnostic message must match the canonical form pinned by \
         the PRD (docs/prds/shadowing-warning.md) and `DiagnosticCode::Shadowing`'s \
         doc comment in crates/reify-types/src/diagnostics.rs"
    );
    assert_eq!(
        warning.labels.len(),
        2,
        "Shadowing warning must carry two labels (child + original), got: {:?}",
        warning.labels
    );
    assert_eq!(
        warning.labels[0].message, "shadows the enclosing declaration",
        "child-site label message must be pinned to the canonical form"
    );
    assert_eq!(
        warning.labels[1].message, "originally declared here",
        "original-decl label message must be pinned to the canonical form"
    );
}

/// Regression guard against the new lint upgrading Shadowing to an error or
/// otherwise breaking the existing `lambda_compile_tests::compile_lambda_param_shadows_outer`
/// path. We re-use that test's exact source (`structure S { param x: Real = 5
/// ; let f = |x| x * 2 }`) and assert two invariants the lint must respect:
///
/// 1. Exactly ONE `Shadowing` warning is emitted (the lint runs on this
///    canonical case and produces a single, well-formed diagnostic).
/// 2. ZERO error-severity diagnostics are produced (the lint must NOT flip
///    Shadowing to `Severity::Error`, and must not break any other compile
///    path with a spurious error).
///
/// This pins the contract relied on by `lambda_compile_tests::compile_lambda_param_shadows_outer`,
/// which filters by `Severity::Error` (lambda_compile_tests.rs:194-198) — if a
/// future change accidentally upgrades the warning to an error, that test
/// would start failing AND this regression test would catch the cause
/// directly.
#[test]
fn shadowing_does_not_break_existing_lambda_compile() {
    let source = r#"
structure S {
    param x: Real = 5
    let f = |x| x * 2
}
"#;
    let module = compile_source(source);

    // (1) Exactly one Shadowing warning.
    let shadow_warnings: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();
    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning on the canonical \
         lambda-shadows-outer case, got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.severity, &d.message))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        shadow_warnings[0].severity,
        Severity::Warning,
        "Shadowing must remain Warning severity — never an Error"
    );

    // (2) Zero errors anywhere in the diagnostic stream.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Shadowing lint must NOT introduce error-severity diagnostics on \
         this canonical case — the existing \
         `lambda_compile_tests::compile_lambda_param_shadows_outer` test \
         depends on this. Got errors: {:?}",
        errors
    );
}

/// A function-body `let` whose name matches a fn-param shadows the param.
///
/// Per spec §8.5, an inner-scope binder (the body let) hides an enclosing-
/// scope name (the fn param). The lint treats the fn body as a CHILD scope
/// of the params, not as siblings — `fn f(x: Real) -> Real { let x = 2.0; x }`
/// MUST emit one Shadowing warning. (See review suggestion #1 for the
/// scope-model rationale.)
#[test]
fn fn_body_let_shadows_fn_param() {
    let source = r#"fn f(x: Length) -> Length { let x = 2.0 ; x }"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning (body `let x` vs fn param `x`), \
         got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert!(
        warning.message.contains("'x'"),
        "expected the warning to be about `x`, got message: {:?}",
        warning.message
    );
    assert_eq!(warning.labels.len(), 2);

    // The fn param `x` appears in the signature `(x: Length)`. The body's
    // `let x` appears AFTER the `{`. Verify the original-decl span sits
    // before the let.
    let fn_param_x = source
        .find("(x:")
        .expect("source must contain `(x:` (fn signature)");
    let body_let_x = source.find("let x").expect("source must contain `let x`");

    let l0 = &warning.labels[0]; // child site (body let)
    let l1 = &warning.labels[1]; // original-decl site (fn param)
    assert!(
        (l1.span.start as usize) >= fn_param_x && (l1.span.start as usize) < body_let_x,
        "original-decl span must point at the fn param `x` \
         (between byte {} and {}), got {:?}",
        fn_param_x,
        body_let_x,
        l1.span
    );
    assert!(
        (l0.span.start as usize) >= body_let_x,
        "child-site span must point at the body's `let x` (>= byte {}), \
         got {:?}",
        body_let_x,
        l0.span
    );

    // Regression-lock: the canonical fn-body-let-shadow source must produce
    // zero Severity::Error diagnostics.  A future compiler pass that adds a
    // duplicate-decl error (or any other error) for the same site will trip
    // this assertion and force a deliberate suppress-or-document choice rather
    // than silently doubling up.  Using `module.diagnostics` (not
    // `warnings_only`) because we are explicitly checking the error band that
    // `warnings_only` filters out.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "fn-body-let-shadow source must NOT produce any Severity::Error \
         diagnostics — the Shadowing warning is the only signal for this \
         canonical case.  Got errors: {:?}",
        errors
    );
}

/// A purpose-body `let` whose name matches a purpose-param shadows the param.
///
/// Same scope-model rule as the fn-body case: the purpose body is a CHILD
/// scope of the purpose params (not siblings). `purpose mfg(subject :
/// Structure) { let subject = … }` MUST emit one Shadowing warning. (See
/// review suggestion #1.)
#[test]
fn purpose_body_let_shadows_purpose_param() {
    let source = r#"
purpose mfg(subject : Structure) {
    let subject = 1
    constraint subject > 0
}
"#;
    let module = compile_source_with_stdlib(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning (body `let subject` vs purpose \
         param `subject`), got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert!(
        warning.message.contains("'subject'"),
        "expected the warning to be about `subject`, got message: {:?}",
        warning.message
    );
    assert_eq!(warning.labels.len(), 2);

    let purpose_param = source
        .find("(subject :")
        .expect("source must contain `(subject :`");
    let body_let_subject = source
        .find("let subject")
        .expect("source must contain `let subject`");

    let l0 = &warning.labels[0]; // child site (body let)
    let l1 = &warning.labels[1]; // original-decl site (purpose param)
    assert!(
        (l1.span.start as usize) >= purpose_param && (l1.span.start as usize) < body_let_subject,
        "original-decl span must point at the purpose param `subject` \
         (between byte {} and {}), got {:?}",
        purpose_param,
        body_let_subject,
        l1.span
    );
    assert!(
        (l0.span.start as usize) >= body_let_subject,
        "child-site span must point at the body's `let subject` (>= byte {}), \
         got {:?}",
        body_let_subject,
        l0.span
    );
}

/// Symmetry regression-lock: the fn-body and purpose-body arms of
/// `walk_declaration` emit structurally analogous Shadowing warnings for
/// analogous sources.
///
/// This test pins the helper's centralizing contract (introduced in task 2499):
/// the two arms must produce the same *shape* of diagnostic for a body-let
/// that shadows a param. If a future maintainer modifies one arm without the
/// other, this test fails.
///
/// Sources:
/// - fn: `fn f(x: Length) -> Length { let x = 2.0 ; x }`
///   (mirror of `fn_body_let_shadows_fn_param`)
/// - purpose: `purpose mfg(subject : Structure) { let subject = 1 … }`
///   (mirror of `purpose_body_let_shadows_purpose_param`)
///
/// Per-arm structural assertions:
/// * Exactly 1 Shadowing warning.
/// * Warning has exactly 2 labels (child site + original-decl site).
/// * Warning message matches the canonical form
///   `"declaration of '<name>' shadows enclosing declaration"`.
/// * Child-site label starts AFTER the body `let` keyword.
/// * Original-decl label starts BEFORE the body `let` keyword (in the
///   params signature).
///
/// **Cross-arm equality assertions** (the distinguishing feature vs. the two
/// independent per-arm tests `fn_body_let_shadows_fn_param` /
/// `purpose_body_let_shadows_purpose_param`):
/// * Label count must be equal.
/// * Severity must be equal.
/// * Diagnostic code must be equal.
/// * Message template (with the variable name replaced by `'<name>'`) must
///   be identical — so the two arms can't silently diverge in wording.
///
/// **Label lookup**: labels are found by `.message` (`"originally declared
/// here"` / the other label) rather than by index, so this test is invariant
/// under any future reordering of labels in `push_shadow_diagnostic`. The
/// literal label messages are pinned by `shadow_diagnostic_message_format_is_pinned`.
#[test]
fn fn_and_purpose_body_arm_emit_analogous_shadow_warnings() {
    // ── fn arm ──────────────────────────────────────────────────────────────
    let fn_source = r#"fn f(x: Length) -> Length { let x = 2.0 ; x }"#;
    let fn_module = compile_source(fn_source);
    let fn_shadow_warnings: Vec<_> = fn_module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        fn_shadow_warnings.len(),
        1,
        "fn arm: expected exactly 1 Shadowing warning, got {}: {:?}",
        fn_shadow_warnings.len(),
        fn_shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );
    let fn_warn = fn_shadow_warnings[0];
    assert_eq!(
        fn_warn.labels.len(),
        2,
        "fn arm: Shadowing warning must carry 2 labels, got {:?}",
        fn_warn.labels
    );
    assert!(
        fn_warn.message.contains("'x'")
            && fn_warn.message.contains("shadows enclosing declaration"),
        "fn arm: unexpected message {:?}",
        fn_warn.message
    );
    let fn_param_x = fn_source.find("(x:").expect("fn source must contain `(x:`");
    let fn_body_let_x = fn_source
        .find("let x")
        .expect("fn source must contain `let x`");
    // Look up labels by pinned message rather than by index, so this test is
    // invariant under any future label-order change in push_shadow_diagnostic.
    let fn_orig_label = fn_warn
        .labels
        .iter()
        .find(|l| l.message == "originally declared here")
        .expect("fn arm: shadow warning must include `originally declared here` label");
    let fn_child_label = fn_warn
        .labels
        .iter()
        .find(|l| l.message != "originally declared here")
        .expect("fn arm: shadow warning must include child-site label");
    assert!(
        (fn_orig_label.span.start as usize) >= fn_param_x
            && (fn_orig_label.span.start as usize) < fn_body_let_x,
        "fn arm: original-decl span must be in param signature (byte {} ..< {}), got {:?}",
        fn_param_x,
        fn_body_let_x,
        fn_orig_label.span
    );
    assert!(
        (fn_child_label.span.start as usize) >= fn_body_let_x,
        "fn arm: child-site span must be at or after body `let x` (byte {}), got {:?}",
        fn_body_let_x,
        fn_child_label.span
    );

    // ── purpose arm ─────────────────────────────────────────────────────────
    let purpose_source = r#"
purpose mfg(subject : Structure) {
    let subject = 1
    constraint subject > 0
}
"#;
    let purpose_module = compile_source_with_stdlib(purpose_source);
    let purpose_shadow_warnings: Vec<_> = purpose_module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        purpose_shadow_warnings.len(),
        1,
        "purpose arm: expected exactly 1 Shadowing warning, got {}: {:?}",
        purpose_shadow_warnings.len(),
        purpose_shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );
    let purpose_warn = purpose_shadow_warnings[0];
    assert_eq!(
        purpose_warn.labels.len(),
        2,
        "purpose arm: Shadowing warning must carry 2 labels, got {:?}",
        purpose_warn.labels
    );
    assert!(
        purpose_warn.message.contains("'subject'")
            && purpose_warn
                .message
                .contains("shadows enclosing declaration"),
        "purpose arm: unexpected message {:?}",
        purpose_warn.message
    );
    let purpose_param = purpose_source
        .find("(subject :")
        .expect("purpose source must contain `(subject :`");
    let purpose_body_let = purpose_source
        .find("let subject")
        .expect("purpose source must contain `let subject`");
    // Look up labels by pinned message rather than by index.
    let purpose_orig_label = purpose_warn
        .labels
        .iter()
        .find(|l| l.message == "originally declared here")
        .expect("purpose arm: shadow warning must include `originally declared here` label");
    let purpose_child_label = purpose_warn
        .labels
        .iter()
        .find(|l| l.message != "originally declared here")
        .expect("purpose arm: shadow warning must include child-site label");
    assert!(
        (purpose_orig_label.span.start as usize) >= purpose_param
            && (purpose_orig_label.span.start as usize) < purpose_body_let,
        "purpose arm: original-decl span must be in param signature (byte {} ..< {}), got {:?}",
        purpose_param,
        purpose_body_let,
        purpose_orig_label.span
    );
    assert!(
        (purpose_child_label.span.start as usize) >= purpose_body_let,
        "purpose arm: child-site span must be at or after body `let subject` (byte {}), got {:?}",
        purpose_body_let,
        purpose_child_label.span
    );

    // ── cross-arm symmetry assertions ────────────────────────────────────────
    // These compare fn_warn and purpose_warn directly to each other — the
    // distinguishing feature of this *symmetry* test vs. the two independent
    // per-arm tests.  A change that modifies one arm's diagnostic shape without
    // the other will fail here even if both arms still satisfy their own
    // per-arm structural invariants.
    assert_eq!(
        fn_warn.labels.len(),
        purpose_warn.labels.len(),
        "fn and purpose arms must carry the same number of labels; \
         fn labels={:?}, purpose labels={:?}",
        fn_warn.labels,
        purpose_warn.labels
    );
    assert_eq!(
        fn_warn.severity, purpose_warn.severity,
        "fn and purpose arms must emit the same diagnostic severity"
    );
    assert_eq!(
        fn_warn.code, purpose_warn.code,
        "fn and purpose arms must emit the same diagnostic code"
    );
    // Strip each arm's variable name before comparing so the canonical message
    // template can be tested for parity independently of the name.
    let fn_template = fn_warn.message.replace("'x'", "'<name>'");
    let purpose_template = purpose_warn.message.replace("'subject'", "'<name>'");
    assert_eq!(
        fn_template, purpose_template,
        "fn and purpose arms must produce the same canonical message template \
         (fn={:?}, purpose={:?})",
        fn_warn.message, purpose_warn.message
    );
}

/// Regression-lock (task 2501, updated task 4009 δ): `let subject` in a purpose
/// body shadows purpose-param `subject` and the shadow-lint fires.
///
/// ## Design rationale
///
/// When `purpose mfg(subject : Structure) { let subject = 1 ; constraint
/// subject > 0 }` is compiled, the shadow-lint emits a `Shadowing` warning:
/// body `let subject` hides purpose-param `subject`.
///
/// As of task 4009 δ, purpose-body let bindings are fully supported:
/// `DiagnosticCode::PurposeLetUnsupported` is no longer emitted for this case.
/// The Shadowing warning remains because the shadow is real and the user should
/// be aware of it.
///
/// This test locks the Shadowing warning. If `shadow_lint` is accidentally
/// disabled for purpose-body lets a future change will fail here.
#[test]
fn purpose_body_let_shadow_coexists_with_unsupported_let_error_intentional() {
    let source = r#"
purpose mfg(subject : Structure) {
    let subject = 1
    constraint subject > 0
}
"#;
    let module = compile_source_with_stdlib(source);

    // Exactly one Shadowing warning for the body `let subject`.
    let shadow_warnings: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing) && d.severity == Severity::Warning)
        .collect();
    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing Warning (body `let subject` vs purpose \
         param `subject`), got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    // No PurposeLetUnsupported error: purpose-body lets are now supported (task 4009 δ).
    let unsupported_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::PurposeLetUnsupported))
        .collect();
    assert!(
        unsupported_errors.is_empty(),
        "expected no PurposeLetUnsupported diagnostics after task 4009 δ, \
         got: {:?}",
        unsupported_errors
    );
}

/// A lambda inside a port-internal member shadows a port-internal binder.
///
/// Ports have their own lexical scope (port-internal members are NOT folded
/// into the enclosing entity scope). The lint pushes a port-internal frame
/// onto the stack before walking port members, so a lambda inside a
/// port-internal `let` sees port-internal binders as a parent scope.
///
/// Source (port syntax: `port <name> : <Type> { <members> }`, see
/// `crates/reify-syntax/tests/member_span_tests.rs:115`):
///
/// ```text
/// structure S {
///     port p : MechPort {
///         param q : Real = 1
///         let f = |q| q
///     }
/// }
/// ```
///
/// The lambda's `q` shadows the port's `param q`. Exactly ONE Shadowing
/// warning is expected. (See review suggestion #4 for the scope-model
/// rationale.)
#[test]
fn lambda_in_port_member_shadows_port_internal_binder() {
    let source = r#"
structure S {
    port p : MechPort {
        param q : Real = 1
        let f = |q| q
    }
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning (lambda `|q|` vs port-internal \
         `param q`), got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert!(
        warning.message.contains("'q'"),
        "expected the warning to be about `q`, got message: {:?}",
        warning.message
    );
    assert_eq!(warning.labels.len(), 2);

    let param_q = source
        .find("param q")
        .expect("source must contain `param q`");
    let lambda_q = source.find("|q|").expect("source must contain `|q|`");

    let l0 = &warning.labels[0]; // child site (lambda)
    let l1 = &warning.labels[1]; // original-decl site (port-internal param)
    assert!(
        (l1.span.start as usize) >= param_q && (l1.span.start as usize) < lambda_q,
        "original-decl span must point at the port-internal `param q` \
         (between byte {} and {}), got {:?}",
        param_q,
        lambda_q,
        l1.span
    );
    assert!(
        (l0.span.start as usize) >= lambda_q,
        "child-site span must point at the lambda's `|q|` (>= byte {}), \
         got {:?}",
        lambda_q,
        l0.span
    );
}

/// Statement-form `forall ... : connect ...` whose bound variable matches an
/// entity-scope `param` MUST emit one Shadowing warning. Mirrors the
/// expression-form `quantifier_variable_shadows_entity_param_emits_w_shadow`
/// case but exercises the `MemberDecl::ForallConnect` arm in
/// `walk_members_depth` (shadow_lint.rs:472-499) — the statement-form binder
/// must produce the same shadow diagnostic as the expression-form binder for
/// behavioral consistency between the two `forall` surfaces.
#[test]
fn forall_connect_variable_shadows_entity_param_emits_w_shadow() {
    let source = r#"
structure S {
    param v : Real = 0
    forall v in [1, 2, 3]: connect v.inlet -> housing.air_channel
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning for forall-connect-shadow case, got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert_eq!(warning.severity, Severity::Warning);
    assert_eq!(
        warning.labels.len(),
        2,
        "Shadowing warning must carry two labels (child + original), got: {:?}",
        warning.labels
    );

    let l0 = &warning.labels[0];
    let l1 = &warning.labels[1];
    assert!(
        !l0.span.is_empty(),
        "child-site label span must be non-empty, got: {:?}",
        l0.span
    );
    assert!(
        !l1.span.is_empty(),
        "original-decl label span must be non-empty, got: {:?}",
        l1.span
    );
    assert_ne!(
        l0.span, l1.span,
        "child-site and original-decl spans must be distinct, both = {:?}",
        l0.span
    );

    // The original-decl span (l1) lies on the entity's `param v`, which appears
    // BEFORE the forall statement — assert l1 precedes l0.
    assert!(
        l1.span.start < l0.span.start,
        "original-decl span ({:?}) must precede the child-site span ({:?})",
        l1.span,
        l0.span
    );
}

/// Spec §6.4 first-seen invariant: the original-decl span in a shadow
/// diagnostic for a name declared in BOTH branches of a guarded group must
/// point at the THEN-branch (first-seen) occurrence, not the else-branch.
///
/// Background: `collect_body_frame_into` folds `g.members` (THEN branch) and
/// `g.else_members` (ELSE branch) into the same parent frame because guarded
/// siblings are mutually-exclusive — they MUST NOT shadow each other. Since
/// both branches write the same key, the semantics of the write determines
/// which occurrence is used as the "original declaration" when a LATER child
/// scope (e.g. a lambda) shadows that name.
///
/// With last-writer-wins (`HashMap::insert`), `else_members` is visited last,
/// so its occurrence wins the frame entry and appears as the "originally
/// declared here" span — even though the THEN branch is textually first.  A
/// reader scanning the source top-to-bottom finds the THEN-branch occurrence
/// first and expects the shadow diagnostic to point there.
///
/// With first-seen-wins (`HashMap::entry(...).or_insert(...)`), the THEN-branch
/// occurrence populates the frame entry and the ELSE-branch visit is a no-op,
/// producing the user-visible top-to-bottom expectation.
///
/// This test pins the first-seen-wins contract by asserting the original-decl
/// span lies:
///   (a) at or after the first `param head` (THEN branch, first occurrence), AND
///   (b) strictly before the last `param head` (ELSE branch, last occurrence).
///
/// Because the label lookup uses `labels.iter().find(|l| l.message ==
/// "originally declared here")` rather than a positional index, the test is
/// invariant under any future reordering of labels in `push_shadow_diagnostic`.
/// The literal message is pinned by `shadow_diagnostic_message_format_is_pinned`.
#[test]
fn guarded_group_shadow_original_decl_uses_first_seen_branch() {
    let source = r#"
structure S {
    param cond : Bool = true
    where cond {
        param head : Real = 1
    } else {
        param head : Real = 2
    }
    let f = |head| head * 2
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning (lambda `|head|` shadows guarded-group \
         `param head`), got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert_eq!(
        warning.labels.len(),
        2,
        "Shadowing warning must carry 2 labels (child + original-decl), got {:?}",
        warning.labels
    );

    // Find the original-decl label by its pinned message (see
    // shadow_diagnostic_message_format_is_pinned) rather than by position, so
    // this test is invariant under any future label-order change in
    // push_shadow_diagnostic.
    let orig_label = warning
        .labels
        .iter()
        .find(|l| l.message == "originally declared here")
        .expect("Shadowing warning must include an `originally declared here` label");

    // Byte offsets: THEN-branch's `param head` is the first occurrence;
    // ELSE-branch's `param head` is the last occurrence.
    let then_head = source
        .find("param head")
        .expect("source must contain `param head` (THEN branch)");
    let else_head = source
        .rfind("param head")
        .expect("source must contain a second `param head` (ELSE branch)");

    // (a) The original-decl span must start at or after the THEN-branch decl.
    assert!(
        (orig_label.span.start as usize) >= then_head,
        "original-decl span must be at or after THEN-branch `param head` \
         (byte {}), got {:?}",
        then_head,
        orig_label.span
    );
    // (b) The original-decl span must be STRICTLY before the ELSE-branch decl
    //     (first-seen wins — the else-branch entry is a no-op in the frame).
    assert!(
        (orig_label.span.start as usize) < else_head,
        "original-decl span must be strictly before ELSE-branch `param head` \
         (byte {}), got {:?} — expected first-seen (THEN branch) to win, \
         but the span appears to point at the else-branch (last-writer-wins \
         regression)",
        else_head,
        orig_label.span
    );
}

/// Statement-form `forall ... : constraint ...` whose bound variable matches
/// an entity-scope `param` MUST emit one Shadowing warning. Mirrors the
/// expression-form `quantifier_variable_shadows_entity_param_emits_w_shadow`
/// case but exercises the `MemberDecl::ForallConstraint` arm in
/// `walk_members_depth` (shadow_lint.rs:500-527) — the statement-form binder
/// must produce the same shadow diagnostic as the expression-form binder for
/// behavioral consistency between the two `forall` surfaces.
#[test]
fn forall_constraint_variable_shadows_entity_param_emits_w_shadow() {
    let source = r#"
structure S {
    param v : Real = 0
    forall v in [1, 2, 3]: constraint v > 0
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning for forall-constraint-shadow case, got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert_eq!(warning.severity, Severity::Warning);
    assert_eq!(
        warning.labels.len(),
        2,
        "Shadowing warning must carry two labels (child + original), got: {:?}",
        warning.labels
    );

    let l0 = &warning.labels[0];
    let l1 = &warning.labels[1];
    assert!(
        !l0.span.is_empty(),
        "child-site label span must be non-empty, got: {:?}",
        l0.span
    );
    assert!(
        !l1.span.is_empty(),
        "original-decl label span must be non-empty, got: {:?}",
        l1.span
    );
    assert_ne!(
        l0.span, l1.span,
        "child-site and original-decl spans must be distinct, both = {:?}",
        l0.span
    );

    // The original-decl span (l1) lies on the entity's `param v`, which appears
    // BEFORE the forall statement — assert l1 precedes l0.
    assert!(
        l1.span.start < l0.span.start,
        "original-decl span ({:?}) must precede the child-site span ({:?})",
        l1.span,
        l0.span
    );
}

/// Trait associated-fn body (task ζ 3941): a lambda parameter declared inside a
/// trait `fn` body that re-uses the fn's own parameter name shadows it. ζ makes
/// trait-fn bodies live (the dispatch path now compiles them), so the shadow
/// lint must walk each fn body as a CHILD scope of its params — mirroring the
/// top-level `Declaration::Function` arm — and flag a lambda whose binder
/// collides with a fn param.
///
/// Trait fn bodies are result-expr-only (the grammar admits no body
/// `let`-bindings), so the shadow is created with a lambda directly in the
/// result expression: `|factor| factor * 2.0`. The lambda's `factor` shadows
/// the fn param `factor`; exactly ONE Shadowing warning is expected,
/// original-decl on the fn param and child-site on the lambda binder. (The
/// lambda sits in the result position rather than being immediately invoked
/// because the shadow walker does not descend into a call's callee — only a
/// lambda the walker visits directly is checked.)
///
/// RED until step-12 replaces the `MemberDecl::Fn` no-op arm in shadow_lint with
/// a child-scope body walk: while the arm is a no-op the fn body is never walked
/// and the shadow count is 0.
#[test]
fn lambda_inside_trait_fn_body_shadows_fn_param_emits_w_shadow() {
    let source = r#"
trait Shadowy {
    fn compute(self, factor : Real) -> Real { |factor| factor * 2.0 }
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning (lambda |factor| vs trait fn param factor), \
         got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert_eq!(warning.severity, Severity::Warning);
    assert!(
        warning.message.contains("'factor'"),
        "expected the warning to be about `factor`, got message: {:?}",
        warning.message
    );
    assert_eq!(warning.labels.len(), 2);

    // The fn param `factor` (declared `factor : Real`) appears before the
    // lambda's `|factor|`. Verify the original-decl label sits on the fn param
    // and the child-site label on the lambda binder.
    let fn_param_factor = source
        .find("factor :")
        .expect("source must contain `factor :` (fn signature)");
    let lambda_factor = source
        .find("|factor|")
        .expect("source must contain `|factor|`");

    let l0 = &warning.labels[0]; // child site
    let l1 = &warning.labels[1]; // original-decl site
    assert!(
        (l1.span.start as usize) >= fn_param_factor && (l1.span.start as usize) < lambda_factor,
        "original-decl span must point at the trait fn param `factor` \
         (between byte {} and {}), got {:?}",
        fn_param_factor,
        lambda_factor,
        l1.span
    );
    assert!(
        (l0.span.start as usize) >= lambda_factor,
        "child-site span must point at the lambda's `|factor|` (>= byte {}), \
         got {:?}",
        lambda_factor,
        l0.span
    );
}
