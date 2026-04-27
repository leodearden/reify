//! Shadowing warning tests (spec §8.5, PRD docs/prds/shadowing-warning.md).
//!
//! The lint walks AST scopes once and emits a Warning diagnostic with
//! [`DiagnosticCode::Shadowing`] when a child-scope declaration uses the same
//! name as a name visible from an enclosing parent scope.

use reify_test_support::{compile_source, compile_source_with_stdlib, warnings_only};
use reify_types::{DiagnosticCode, Severity};

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
    let param_x = source.find("param x").expect("source must contain `param x`");
    let inner_lambda = source.rfind("|x|").expect("source must contain inner `|x|`");

    let l0 = &warning.labels[0]; // child site
    let l1 = &warning.labels[1]; // original-decl site
    assert!(
        (l1.span.start as usize) >= param_x
            && (l1.span.start as usize) < inner_lambda,
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
    let lambda_sqrt = source
        .find("|sqrt|")
        .expect("source must contain `|sqrt|`");
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
/// `fn area(w: Scalar, h: Scalar) -> Scalar { let f = |w| w * h ; f(w) }`:
/// the lambda's `w` shadows the fn param `w`. Exactly ONE Shadowing warning
/// is expected. The original-decl span points at the fn param `w`; the
/// child-site span at the lambda's `|w|`.
///
/// Uses `;` between the let binding and the result expression because the
/// parser does not currently treat a newline as a statement separator
/// between a let-with-lambda and a function-call result.
#[test]
fn lambda_in_fn_body_shadows_fn_param() {
    let source = r#"fn area(w: Scalar, h: Scalar) -> Scalar { let f = |w| w * h ; f(w) }"#;
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
        (l1.span.start as usize) >= fn_param_w
            && (l1.span.start as usize) < lambda_w,
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
    let lambda_wall = source
        .find("|wall|")
        .expect("source must contain `|wall|`");

    let l0 = &warning.labels[0]; // child site
    let l1 = &warning.labels[1]; // original-decl site
    assert!(
        (l1.span.start as usize) >= param_wall
            && (l1.span.start as usize) < lambda_wall,
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
    let lambda_mass = source
        .find("|mass|")
        .expect("source must contain `|mass|`");

    let l0 = &warning.labels[0]; // child site
    let l1 = &warning.labels[1]; // original-decl site
    assert!(
        (l1.span.start as usize) >= param_mass
            && (l1.span.start as usize) < lambda_mass,
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
        (l1.span.start as usize) >= purpose_param
            && (l1.span.start as usize) < lambda_subject,
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
        warning.message,
        "declaration of 'x' shadows enclosing declaration",
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
