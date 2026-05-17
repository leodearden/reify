//! Source-based rewrites of hand-built tests in `match_arm_decl_group_compile_tests.rs`.
//! Together they exercise the user-observable leaf signal from
//! phase-3-grammar-fiction-triage-log.md §B2 (task 3564): existing hand-built AST
//! integration tests rewritten to start from .ri source continue to pass.
//!
//! These tests parse `.ri` source through the full
//! `reify_syntax::parse → reify_compiler::compile` pipeline and verify that
//! `MemberDecl::MatchArmDeclGroup` is correctly lowered from CST by
//! `lower_match_arm_decl_group` (ts_parser.rs, task 3564).

use reify_compiler::GuardedDeclGroup;
use reify_types::{ModulePath, Type};

/// Source-based rewrite of `match_arm_decl_group_registers_cluster_without_duplicate_name_diagnostics`.
///
/// Parses the equivalent of:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure HexHead { }
/// structure SocketHead { }
/// structure Bolt {
///     param head_type : HeadType
///     match head_type { Hex => sub head : HexHead, Socket => sub head : SocketHead }
/// }
/// ```
///
/// Asserts:
/// (a) No parse errors — the grammar (task 3563) handles `match_arm_decl_block` syntax.
/// (b) No diagnostic containing both `"duplicate"` and `"head"`.
/// (c) `templates["Bolt"].match_arm_groups` contains exactly one `GuardedDeclGroup`
///     for `"head"` with 2 arms: arm[0].arm_type == StructureRef("HexHead"),
///     arm[1].arm_type == StructureRef("SocketHead").
///
/// RED before `lower_match_arm_decl_group` is wired in `lower_member` — the CST node
/// falls through to `_ => None`, so no `MatchArmDeclGroup` is produced, no cluster
/// is registered, and the `match_arm_groups` assertion fails.
#[test]
fn match_block_decl_lowers_two_arm_union_from_source() {
    let source = r#"
enum HeadType { Hex, Socket }
structure HexHead { }
structure SocketHead { }
structure Bolt {
    param head_type : HeadType
    match head_type { Hex => sub head : HexHead, Socket => sub head : SocketHead }
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_two_arm_union_source"));
    assert!(
        parsed.errors.is_empty(),
        "expected no parse errors, got: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // (b) No "duplicate … head" diagnostic.
    let duplicate_head_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("duplicate") && msg.contains("head")
        })
        .collect();
    assert!(
        duplicate_head_diags.is_empty(),
        "expected no 'duplicate head' diagnostics, got: {:#?}",
        duplicate_head_diags
    );

    // (c) GuardedDeclGroup for "head" with correct per-arm arm_type.
    let bolt_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("Bolt template should be compiled");

    let head_group: &GuardedDeclGroup = bolt_template
        .match_arm_groups
        .iter()
        .find(|g| g.name == "head")
        .expect("match_arm_groups should contain a group named 'head'");

    assert_eq!(
        head_group.arms.len(),
        2,
        "expected 2 arms in GuardedDeclGroup for 'head', got {}",
        head_group.arms.len()
    );

    assert!(
        matches!(&head_group.arms[0].arm_type, Type::StructureRef(s) if s == "HexHead"),
        "arm 0 should have arm_type StructureRef(\"HexHead\"), got: {:?}",
        head_group.arms[0].arm_type
    );

    assert!(
        matches!(&head_group.arms[1].arm_type, Type::StructureRef(s) if s == "SocketHead"),
        "arm 1 should have arm_type StructureRef(\"SocketHead\"), got: {:?}",
        head_group.arms[1].arm_type
    );
}

/// Source-based rewrite of `match_arm_decl_group_pipe_patterns_produce_two_arm_cluster`.
///
/// Parses the pipe form:
/// ```text
/// enum HeadType { Hex, Socket, Button }
/// structure HexOrButtonHead { }
/// structure SocketHead { }
/// structure Bolt {
///     param head_type : HeadType
///     match head_type { Hex | Button => sub head : HexOrButtonHead, Socket => sub head : SocketHead }
/// }
/// ```
///
/// Asserts:
/// (a) No parse errors.
/// (b) No "duplicate head" diagnostic.
/// (c) `match_arm_groups["head"]` has 2 arms (pipe-collapsed first arm + regular second arm).
///
/// RED before lowering — `match_arm_decl_block` is silently dropped, `match_arm_groups` is empty.
#[test]
fn match_block_decl_lowers_variant_pipe_arm_from_source() {
    let source = r#"
enum HeadType { Hex, Socket, Button }
structure HexOrButtonHead { }
structure SocketHead { }
structure Bolt {
    param head_type : HeadType
    match head_type { Hex | Button => sub head : HexOrButtonHead, Socket => sub head : SocketHead }
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_variant_pipe_source"));
    assert!(
        parsed.errors.is_empty(),
        "expected no parse errors, got: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // (b) No "duplicate head" diagnostic.
    let dup_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("duplicate") && msg.contains("head")
        })
        .collect();
    assert!(
        dup_diags.is_empty(),
        "expected no 'duplicate head' diagnostics, got: {:#?}",
        dup_diags
    );

    // (c) GuardedDeclGroup for "head" with 2 arms.
    let bolt_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("Bolt template should be compiled");

    let head_group: &GuardedDeclGroup = bolt_template
        .match_arm_groups
        .iter()
        .find(|g| g.name == "head")
        .expect("match_arm_groups should contain a group named 'head'");

    assert_eq!(
        head_group.arms.len(),
        2,
        "expected 2 arms (pipe arm + socket arm), got {}",
        head_group.arms.len()
    );
}

/// Regression guard for PRD AC 4 (exhaustiveness).
///
/// Parses a single-arm `match` block over a two-variant enum — the match is
/// non-exhaustive (arm for `Socket` is missing):
/// ```text
/// enum HeadType { Hex, Socket }
/// structure HexHead { }
/// structure Bolt {
///     param head_type : HeadType
///     match head_type { Hex => sub head : HexHead }
/// }
/// ```
///
/// Asserts that the compile-time exhaustiveness diagnostic still surfaces after
/// the CST → AST lowering path is wired.  Without this guard, a future lowering
/// refactor could regress AC 4 silently (the existing hand-built tests only cover
/// the AST → compile leg, not the parse → AST leg).
///
/// RED before lowering — `match_arm_decl_block` is silently dropped, so the
/// exhaustiveness gate never fires and the diagnostic is absent.
#[test]
fn match_block_decl_non_exhaustive_single_arm_from_source_emits_existing_diagnostic() {
    let source = r#"
enum HeadType { Hex, Socket }
structure HexHead { }
structure Bolt {
    param head_type : HeadType
    match head_type { Hex => sub head : HexHead }
}
"#;
    let parsed =
        reify_syntax::parse(source, ModulePath::single("test_non_exhaustive_single_arm_source"));
    assert!(
        parsed.errors.is_empty(),
        "expected no parse errors, got: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // The exhaustiveness gate must emit a "non-exhaustive match" diagnostic
    // naming the missing variant "Socket".
    let has_exhaustive_diag = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("non-exhaustive match") && d.message.contains("Socket"));
    assert!(
        has_exhaustive_diag,
        "expected a 'non-exhaustive match' diagnostic naming 'Socket', got: {:#?}",
        compiled.diagnostics
    );
}
