//! E2E fixture + eval tests for decl-level `match { ... => sub head : ... }` blocks (task #3567).
//!
//! Locks the user-observable parse → compile → eval surface for B2 match-arm
//! decl groups.  The compiler-layer typing is covered by
//! `match_arm_decl_group_typing_tests.rs` (hand-built AST); these tests lock the
//! `.ri` source → full pipeline path.
//!
//! Tests:
//!   - `match_block_decl_fixture_elaborates_and_evaluates_cleanly` (step-1):
//!     Happy-path fixture with a pipe arm and a common-field `let probe` access.
//!     Asserts zero Error diagnostics through compile + eval (AC1/AC2/AC5).
//!   - `match_block_decl_arm_specific_field_access_diagnoses_offending_arm` (step-2):
//!     Missing-field access on an arm-specific field; asserts exactly one Error
//!     naming both the field and the offending arm type (AC2 narrowing leg).

use reify_compiler::GuardedDeclGroup;
use reify_core::{ModulePath, Severity, Type};
use reify_constraints::SimpleConstraintChecker;

// ── step-1 ───────────────────────────────────────────────────────────────────

/// Happy-path E2E test: the bolt fixture with a pipe arm and a common-field
/// `let probe = self.head.across_flats` elaborates and evaluates cleanly.
///
/// Asserts:
/// (a) No parse errors — grammar (task #3563) handles `match_arm_decl_block` syntax.
/// (b) `Bolt.match_arm_groups` has exactly one group named "head" with 2 arms:
///     arm[0].arm_type == RecessedHead, arm[1].arm_type == SocketHead (pipe-collapsed).
/// (c) No diagnostics containing both "duplicate" and "head".
/// (d) Zero Error-severity compile diagnostics overall (common-field access typechecks, AC2 common leg).
/// (e) Engine eval adds no Error-severity diagnostics ("elaborate cleanly" end-to-end, AC1/AC5).
///
/// This is a regression lock over the landed compile+eval surface from `.ri` source.
/// If RED, it surfaces a real gap in the lowering/eval wiring.
#[test]
fn match_block_decl_fixture_elaborates_and_evaluates_cleanly() {
    let source = include_str!("fixtures/match_block_decls_bolt.ri");

    // ── parse ─────────────────────────────────────────────────────────────────
    let parsed = reify_syntax::parse(source, ModulePath::single("match_block_decls_bolt"));
    assert!(
        parsed.errors.is_empty(),
        "expected no parse errors, got: {:?}",
        parsed.errors
    );

    // ── compile ───────────────────────────────────────────────────────────────
    let compiled = reify_compiler::compile(&parsed);

    // (c) No "duplicate … head" diagnostic.
    let dup_head_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("duplicate") && msg.contains("head")
        })
        .collect();
    assert!(
        dup_head_diags.is_empty(),
        "expected no 'duplicate head' diagnostics, got: {:#?}",
        dup_head_diags
    );

    // (d) Zero Error-severity compile diagnostics.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected zero Error-severity compile diagnostics, got: {:#?}",
        compile_errors
    );

    // (b) Bolt.match_arm_groups: one group "head" with 2 arms.
    let bolt_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("Bolt template should be compiled");

    assert_eq!(
        bolt_template.match_arm_groups.len(),
        1,
        "Bolt should have exactly 1 match_arm_group, got {}",
        bolt_template.match_arm_groups.len()
    );

    let head_group: &GuardedDeclGroup = bolt_template
        .match_arm_groups
        .iter()
        .find(|g| g.name == "head")
        .expect("match_arm_groups should contain a group named 'head'");

    assert_eq!(
        head_group.arms.len(),
        2,
        "expected 2 arms in GuardedDeclGroup 'head' (Hex|Button pipe-collapsed + Socket), got {}",
        head_group.arms.len()
    );

    assert!(
        matches!(&head_group.arms[0].arm_type, Type::StructureRef(s) if s == "RecessedHead"),
        "arm[0] (Hex|Button pipe) should have arm_type RecessedHead, got: {:?}",
        head_group.arms[0].arm_type
    );

    assert!(
        matches!(&head_group.arms[1].arm_type, Type::StructureRef(s) if s == "SocketHead"),
        "arm[1] (Socket) should have arm_type SocketHead, got: {:?}",
        head_group.arms[1].arm_type
    );

    // (e) Engine eval adds no Error-severity diagnostics.
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let eval_result = engine.eval(&compiled);

    let eval_errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "expected zero Error-severity eval diagnostics (elaborate cleanly), got: {:#?}",
        eval_errors
    );
}

// ── step-2 ───────────────────────────────────────────────────────────────────

/// Missing-field diagnostic names the offending arm type.
///
/// Source: a 2-variant enum (`Hex`, `Socket`), two arm structures where
/// `recess_depth` exists only in `RecessedHead` (not in `SocketHead`), an
/// exhaustive match cluster, and `let probe = self.head.recess_depth`.
///
/// Asserts:
/// (a) No parse errors.
/// (b) Exactly ONE Error diagnostic whose message contains both the field name
///     `recess_depth` and the offending arm type `SocketHead` (AC2 narrowing
///     leg + "missing-field diagnostic names offending arms").
/// (c) Anti-cascade: only one Error diagnostic total.
///
/// Regression lock over the compiler diagnostic surfaced from `.ri` source.
#[test]
fn match_block_decl_arm_specific_field_access_diagnoses_offending_arm() {
    let source = r#"
enum HeadKind { Hex, Socket }

structure RecessedHead {
    param recess_depth : Real = 5
    param across_flats : Real = 10
}

structure SocketHead {
    param across_flats : Real = 8
}

structure Bolt {
    param head_kind : HeadKind = HeadKind.Hex
    match head_kind {
        Hex => sub head : RecessedHead,
        Socket => sub head : SocketHead
    }
    let probe = self.head.recess_depth
}
"#;

    // ── parse ─────────────────────────────────────────────────────────────────
    let parsed = reify_syntax::parse(source, ModulePath::single("match_block_decls_missing_field"));
    assert!(
        parsed.errors.is_empty(),
        "expected no parse errors, got: {:?}",
        parsed.errors
    );

    // ── compile ───────────────────────────────────────────────────────────────
    let compiled = reify_compiler::compile(&parsed);

    // (b) Exactly one Error naming both 'recess_depth' and 'SocketHead'.
    let field_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.contains("recess_depth")
                && d.message.contains("SocketHead")
        })
        .collect();

    assert_eq!(
        field_errors.len(),
        1,
        "expected exactly one Error naming 'recess_depth' + 'SocketHead', \
         got {}: {:#?}",
        field_errors.len(),
        field_errors
    );

    // (c) Anti-cascade: only one Error total.
    let all_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        all_errors.len(),
        1,
        "expected exactly one Error total (anti-cascade), got {}: {:#?}",
        all_errors.len(),
        all_errors
    );
}
