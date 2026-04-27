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
