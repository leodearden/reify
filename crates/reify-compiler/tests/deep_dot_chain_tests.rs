//! Integration tests for the deep-dot-chain lint (spec §5.7).
//!
//! The lint walks left-to-right `MemberAccess` chains in the parsed AST and
//! emits a Warning diagnostic with [`DiagnosticCode::DeepDotChain`] when the
//! chain length exceeds [`crate::compile_builder::dot_chain_lint::DEEP_DOT_CHAIN_THRESHOLD`]
//! (= 4). `a.b.c.d` (length 4) is at-threshold and does not warn;
//! `a.b.c.d.e` (length 5) warns.
//!
//! These integration tests use the public `compile_source` / `warnings_only`
//! helpers from `reify-test-support`, mirroring the style of
//! `import_warning_tests.rs` and `diagnostic_coverage_checkpoint.rs`.

use reify_test_support::{compile_source, warnings_only};
use reify_types::DiagnosticCode;

/// A chain at exactly the threshold (length 4 — `a.b.c.d`) must not warn.
///
/// This is a regression lock: the gate is `> THRESHOLD`, not `>= THRESHOLD`,
/// so at-threshold chains are explicitly OK per spec §5.7's example.
#[test]
fn chain_at_threshold_does_not_warn() {
    // Source uses a chain of length 4 (root `a` + 3 dot hops `.b.c.d`).
    // The structure body just establishes a scope for the chain to live in;
    // the lint only cares about AST shape, not name resolution.
    let source = r#"
structure S {
    param a: Real = 0
    let x = a.b.c.d
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .collect();

    assert!(
        deep_dot_chain_warnings.is_empty(),
        "expected no DeepDotChain warnings for at-threshold chain `a.b.c.d`, \
         got: {:?}",
        deep_dot_chain_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// A chain just above threshold (length 5 — `a.b.c.d.e`) emits exactly one
/// Warning whose `code == Some(DiagnosticCode::DeepDotChain)`.
#[test]
fn chain_above_threshold_emits_one_warning_with_deep_dot_chain_code() {
    let source = r#"
structure S {
    param a: Real = 0
    let x = a.b.c.d.e
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .collect();

    assert_eq!(
        deep_dot_chain_warnings.len(),
        1,
        "expected exactly 1 DeepDotChain warning for above-threshold chain `a.b.c.d.e`, \
         got: {:?}",
        deep_dot_chain_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// The DeepDotChain warning's message contains the rendered chain text
/// (`a.b.c.d.e`) so that humans reading the diagnostic see the offending
/// chain inline without needing the source span.
#[test]
fn chain_warning_message_contains_full_chain_text() {
    let source = r#"
structure S {
    param a: Real = 0
    let x = a.b.c.d.e
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warning = warnings
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .expect("expected one DeepDotChain warning");

    assert!(
        deep_dot_chain_warning.message.contains("a.b.c.d.e"),
        "expected DeepDotChain warning message to contain `a.b.c.d.e`, got: {:?}",
        deep_dot_chain_warning.message
    );
}

/// The DeepDotChain warning has at least one DiagnosticLabel whose span
/// equals the outermost MemberAccess.span — i.e. starts at byte offset of
/// root identifier `a` and ends after final member `e`.
#[test]
fn chain_warning_has_label_covering_full_chain_span() {
    let source = r#"
structure S {
    param a: Real = 0
    let x = a.b.c.d.e
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warning = warnings
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .expect("expected one DeepDotChain warning");

    assert!(
        !deep_dot_chain_warning.labels.is_empty(),
        "expected at least one label on the DeepDotChain warning, got: {:?}",
        deep_dot_chain_warning
    );

    // Compute the expected chain span by locating "a.b.c.d.e" in the source.
    let needle = "a.b.c.d.e";
    let start = source
        .find(needle)
        .expect("test source must contain `a.b.c.d.e` literally") as u32;
    let end = start + needle.len() as u32;

    let has_full_span_label = deep_dot_chain_warning
        .labels
        .iter()
        .any(|l| l.span.start == start && l.span.end == end);

    assert!(
        has_full_span_label,
        "expected a label whose span covers the full chain (bytes {start}..{end}), \
         got labels: {:?}",
        deep_dot_chain_warning.labels
    );
}

/// `a.b[0].c.d.e.f` parses as the outer chain `<IndexAccess>.c.d.e.f`
/// (root = `IndexAccess` of `a.b` indexed at `[0]`, then 4 hops). Chain length
/// is 5, so EXACTLY ONE DeepDotChain warning fires. The inner `a.b` chain has
/// length 2 (under threshold), so it does not warn. The warning's message must
/// NOT contain `a.b.c.d.e.f` because the IndexAccess naturally breaks the
/// chain.
#[test]
fn index_access_resets_chain_root_emits_one_warning_post_index() {
    let source = r#"
structure S {
    param a: Real = 0
    let x = a.b[0].c.d.e.f
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .collect();

    assert_eq!(
        deep_dot_chain_warnings.len(),
        1,
        "expected exactly 1 DeepDotChain warning for `a.b[0].c.d.e.f`, got: {:?}",
        deep_dot_chain_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let warning = deep_dot_chain_warnings[0];
    assert!(
        !warning.message.contains("a.b.c.d.e.f"),
        "DeepDotChain warning must not flatten across the IndexAccess — \
         expected message to NOT contain `a.b.c.d.e.f`, got: {:?}",
        warning.message
    );

    // Positive control on the rendered chain text. The IndexAccess root is
    // not a bare Ident or EnumAccess, so render_chain_text intentionally
    // substitutes the literal `<expr>` placeholder for the root segment in
    // v0.1 (see the doc on `render_chain_text`). The diagnostic span still
    // anchors the squiggle correctly in editor output, but bare CLI
    // renderings will see the placeholder. This assertion pins that
    // contract so future authors who change the placeholder know to update
    // the test (or to extend the root-rendering arm with prettier output).
    assert!(
        warning.message.contains("<expr>.c.d.e.f"),
        "DeepDotChain warning for an IndexAccess-rooted chain must render \
         the chain text with `<expr>` standing in for the IndexAccess root \
         (v0.1 contract — see `render_chain_text` doc), got: {:?}",
        warning.message
    );
}

/// `Direction.In.a.b.c` parses as `MA(MA(MA(EnumAccess(Direction, In), "a"),
/// "b"), "c")`. Chain length is 4 (root = EnumAccess single segment + 3
/// hops `.a.b.c`) = threshold → NO warn. The lint counts `.field` hops only;
/// enum-variant access is structurally distinct in the AST.
#[test]
fn enum_access_root_within_threshold_does_not_warn() {
    let source = r#"
enum Direction { In, Out }

structure S {
    let x = Direction.In.a.b.c
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .collect();

    assert!(
        deep_dot_chain_warnings.is_empty(),
        "expected no DeepDotChain warnings for `Direction.In.a.b.c` (length 4 = threshold), \
         got: {:?}",
        deep_dot_chain_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// A single fixture exercising every expression-bearing position the lint
/// walker is required to visit. Each labelled position contains an
/// intentionally-deep chain `a.b.c.d.e` (length 5 → above threshold). Asserts
/// that the count of `DeepDotChain` warnings equals the count of
/// intentionally-deep chains in the fixture.
///
/// Positions covered (one chain per label unless noted):
///
///   1. `param p: Real = a.b.c.d.e`            — `MemberDecl::Param.default`
///   2. `let v = a.b.c.d.e`                    — `MemberDecl::Let.value`
///   3. `constraint a.b.c.d.e > 0`             — `MemberDecl::Constraint.expr`
///   4. `sub s = Connector(p = a.b.c.d.e)`     — `MemberDecl::Sub.args`
///   5. `minimize a.b.c.d.e`                   — `MemberDecl::Minimize.expr`
///   6. `maximize a.b.c.d.e`                   — `MemberDecl::Maximize.expr`
///   7. `where a.b.c.d.e > 0 { let g = a.b.c.d.e }`
///                                              — `GuardedGroup.condition` (×1)
///                                                AND nested `Let.value` (×1)
///   8. `port p : ... { frame = a.b.c.d.e }`   — `Port.frame_expr`
///   9. `connect ... : C { p = a.b.c.d.e }`    — `Connect.params`
///  10. `chain st1.out_port -> st2.in_port.x.y.z.w.v`
///                                              — `Chain.elements` (deep on
///                                                second element only, length 7)
///  11. `field def f : Real -> Real { source = analytical { |p| a.b.c.d.e } }`
///                                              — `FieldSource::Analytical.expr`
///  12. `constraint def MinX { ... a.b.c.d.e > 0 }`
///                                              — `ConstraintDef.predicates`
///  13. `unit u : Length = a.b.c.d.e`         — `UnitDecl.conversion`
///  14. `fn f(...) { let z = a.b.c.d.e; a.b.c.d.e }`
///                                              — `FnBody.let_bindings.value` (×1)
///                                                AND `FnBody.result_expr` (×1)
///
/// Total intentional deep chains = 16. Walker must visit each one.
#[test]
fn walker_covers_all_expr_bearing_declaration_positions() {
    let source = r#"
// Position 11: field def's analytical lambda body.
field def my_field : Real -> Real { source = analytical { |p| a.b.c.d.e } }

// Position 12: named constraint def's predicate.
constraint def MinX {
    param x: Real
    a.b.c.d.e > 0
}

// Position 13: unit decl's conversion expression.
unit my_unit : Length = a.b.c.d.e

// Position 14 (×2): function body's let value AND result_expr.
fn my_fn(w: Real) -> Real {
    let z = a.b.c.d.e;
    a.b.c.d.e
}

// Helper: connector type for the connect statement (position 9).
structure def Connector { param p: Real = 0 }

// Helper: port-bearing structure for chain/connect (positions 9-10).
trait MyPort { param diameter: Real }
structure def SubTarget {
    port out_port : out MyPort { param diameter: Real = 1 }
    port in_port  : in  MyPort { param diameter: Real = 1 }
}

// Main structure exercising positions 1-10.
structure def Main {
    // Position 1: param default.
    param p1: Real = a.b.c.d.e

    // Position 2: let value.
    let v2 = a.b.c.d.e

    // Position 3: constraint expr (anonymous bare-expression form).
    constraint a.b.c.d.e > 0

    // Position 4: Sub args.
    sub s4 = Connector(p: a.b.c.d.e)

    // Position 5: minimize expr.
    minimize a.b.c.d.e

    // Position 6: maximize expr.
    maximize a.b.c.d.e

    // Position 7 (×2): GuardedGroup condition + nested member let.
    where a.b.c.d.e > 0 {
        let g7 = a.b.c.d.e
    }

    // Position 8: port frame_expr.
    port p8 : out MyPort {
        param diameter: Real = 1
        frame = a.b.c.d.e
    }

    // Helpers for positions 9-10.
    sub st1 = SubTarget()
    sub st2 = SubTarget()

    // Position 9: connect param.
    connect st1.out_port -> st2.in_port : Connector { p = a.b.c.d.e }

    // Position 10: chain element (second element is a 7-segment chain).
    chain st1.out_port -> st2.in_port.x.y.z.w.v
}
"#;

    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .collect();

    // 1+1+1+1+1+1+2+1+1+1+1+1+1+2 = 16 intentional deep chains across the fixture.
    let expected_count = 16;

    assert_eq!(
        deep_dot_chain_warnings.len(),
        expected_count,
        "expected {} DeepDotChain warnings (one per labelled position), got {}: {:#?}",
        expected_count,
        deep_dot_chain_warnings.len(),
        deep_dot_chain_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}
