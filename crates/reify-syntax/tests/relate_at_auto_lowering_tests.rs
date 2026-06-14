//! Lowering (CST→AST) tests for geometric-relations δ (task 4384).
//!
//! AST-level companion to `tree-sitter-reify/tests/relate_at_auto_grammar_tests.rs`
//! (CST-level). Covers the lowering of:
//!
//!   - step-9 (this file, RED until step-10): the `at <pose>` sub clause where
//!     the pose is an `auto` binding — `at auto`, `at auto(free)`,
//!     `at auto(seed = …)`, `at auto(x = …, orientation = …)` — into
//!     `SubDecl.pose_expr = Some(Expr { kind: ExprKind::Auto { free, params } })`.
//!   - step-11 (added later, RED until step-12): member-level `relate { }` →
//!     `MemberDecl::Relate` and inline `at … where { }` → the `SubDecl`
//!     relations field.
//!
//! δ only PRESERVES the seed / component-fix params in `ExprKind::Auto.params`;
//! actually consuming them (root selection, partial-fix) is ζ's relate-solve.
//!
//! All snippets wrap members in `structure S { … }` so the parser sees them in
//! a valid declaration context. Mirrors the harness in
//! `auto_binding_sites_lowering_tests.rs`.

use reify_ast::*;
use reify_core::ModulePath;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Parse `source`, extract the first `MemberDecl::Sub` from the first
/// declaration (which must be a `Structure`), and return a clone of it.
fn first_sub(source: &str) -> SubDecl {
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors: {:?}",
        module.errors
    );
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    structure
        .members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Sub(s) => Some(s.clone()),
            _ => None,
        })
        .expect("expected a Sub member")
}

/// Parse `source`, locate the first `sub`, and return the `ExprKind` of its
/// `pose_expr` (panicking if there is no `at <pose>` clause).
fn sub_pose_kind(source: &str) -> ExprKind {
    let sub = first_sub(source);
    sub.pose_expr
        .as_ref()
        .expect("expected pose_expr to be Some (the `at <pose>` clause)")
        .kind
        .clone()
}

// ── `at auto` / `at auto(free)` — the free flag ──────────────────────────────

/// `sub b : B at auto` — strict auto at the pose site lowers to
/// `ExprKind::Auto { free: false, params: [] }`.
#[test]
fn at_auto_strict_lowers_pose_to_auto_false_empty_params() {
    let kind = sub_pose_kind("structure S { sub b : B at auto }");
    match kind {
        ExprKind::Auto { free, params } => {
            assert!(!free, "expected free: false for strict `auto`");
            assert!(
                params.is_empty(),
                "expected empty params for bare `auto`, got {:?}",
                params
            );
        }
        other => panic!("expected ExprKind::Auto, got {:?}", other),
    }
}

/// `sub b : B at auto(free)` — the `free` modifier lowers to
/// `ExprKind::Auto { free: true, params: [] }`.
#[test]
fn at_auto_free_lowers_pose_to_auto_true_empty_params() {
    let kind = sub_pose_kind("structure S { sub b : B at auto(free) }");
    match kind {
        ExprKind::Auto { free, params } => {
            assert!(free, "expected free: true for `auto(free)`");
            assert!(
                params.is_empty(),
                "expected empty params for `auto(free)`, got {:?}",
                params
            );
        }
        other => panic!("expected ExprKind::Auto, got {:?}", other),
    }
}

// ── `at auto(seed = …)` — a single seed param ────────────────────────────────

/// `sub b : B at auto(seed = self.frame)` — the seed param is preserved as a
/// single `("seed", <expr>)` entry in `params`, with `free: false`.
#[test]
fn at_auto_seed_param_preserved_in_params() {
    let kind = sub_pose_kind("structure S { sub b : B at auto(seed = self.frame) }");
    match kind {
        ExprKind::Auto { free, params } => {
            assert!(!free, "`auto(seed = …)` is not the `free` form");
            assert_eq!(
                params.len(),
                1,
                "expected exactly one param, got {:?}",
                params
            );
            assert_eq!(params[0].0, "seed", "expected the param name to be `seed`");
        }
        other => panic!("expected ExprKind::Auto, got {:?}", other),
    }
}

// ── `at auto(x = …, orientation = …)` — ordered component-fix params ──────────

/// `sub b : B at auto(x = 5mm, orientation = orient_identity())` — both
/// component-fix params are preserved in source order with their value
/// expressions faithfully lowered (a `QuantityLiteral` and a `FunctionCall`,
/// NOT dropped).
#[test]
fn at_auto_component_fix_params_preserved_in_order() {
    let kind =
        sub_pose_kind("structure S { sub b : B at auto(x = 5mm, orientation = orient_identity()) }");
    match kind {
        ExprKind::Auto { free, params } => {
            assert!(!free, "component-fix `auto(…)` is not the `free` form");
            let names: Vec<&str> = params.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(
                names,
                vec!["x", "orientation"],
                "expected params in source order"
            );
            assert!(
                matches!(params[0].1.kind, ExprKind::QuantityLiteral { .. }),
                "expected `x` value to lower to a QuantityLiteral, got {:?}",
                params[0].1.kind
            );
            assert!(
                matches!(params[1].1.kind, ExprKind::FunctionCall { .. }),
                "expected `orientation` value to lower to a FunctionCall, got {:?}",
                params[1].1.kind
            );
        }
        other => panic!("expected ExprKind::Auto, got {:?}", other),
    }
}

// ── Regression: an ordinary (non-auto) `at <expr>` pose still lowers ──────────

/// `sub b : B at frame3(0mm, 0mm, 0mm)` — a non-auto pose expression must still
/// lower to its ordinary `ExprKind` (here a `FunctionCall`), proving the switch
/// from `lower_expr` to `lower_binding_value` for the pose site (step-10) does
/// not short-circuit ordinary expressions.
#[test]
fn at_non_auto_expr_pose_lowers_normally() {
    let kind = sub_pose_kind("structure S { sub b : B at frame3(0mm, 0mm, 0mm) }");
    assert!(
        matches!(kind, ExprKind::FunctionCall { .. }),
        "expected a FunctionCall for a non-auto pose, got {:?}",
        kind
    );
}
