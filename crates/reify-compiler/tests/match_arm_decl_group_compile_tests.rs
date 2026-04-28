//! Integration tests for `MemberDecl::MatchArmDeclGroup` compilation (task 2372, step-9/10).
//!
//! These tests hand-construct a `ParsedModule` containing a match-arm decl group
//! (no tree-sitter grammar support yet — grammar wiring is a future task) and
//! verify that:
//! (a) no `"duplicate"` diagnostic is emitted for the shared logical name; and
//! (b) a `GuardedDeclGroup` is registered in the compiled template with the
//!     correct per-arm `arm_type` metadata.
//!
//! The RED→GREEN transition for (b) requires `TopologyTemplate::match_arm_groups`
//! (added in step-10 under `#[cfg(test)]`) and the entity-compilation hook that
//! populates it.

use reify_compiler::GuardedDeclGroup;
use reify_syntax::{
    Declaration, EnumDecl, Expr, ExprKind, MatchArmDeclArmDecl, MatchArmDeclGroupDecl, MemberDecl,
    ParsedModule, ParamDecl, StructureDef, SubDecl, TypeExpr, TypeExprKind,
};
use reify_types::{ContentHash, ModulePath, SourceSpan, Type};

// ─── AST construction helpers ────────────────────────────────────────────────

fn zero_span() -> SourceSpan {
    SourceSpan::new(0, 0)
}

fn make_ident_expr(name: &str) -> Expr {
    Expr {
        kind: ExprKind::Ident(name.to_string()),
        span: zero_span(),
    }
}

fn named_type_expr(name: &str) -> TypeExpr {
    TypeExpr {
        kind: TypeExprKind::Named {
            name: name.to_string(),
            type_args: vec![],
        },
        span: zero_span(),
    }
}

fn param_member(name: &str, type_name: &str) -> MemberDecl {
    MemberDecl::Param(ParamDecl {
        name: name.to_string(),
        doc: None,
        type_expr: Some(named_type_expr(type_name)),
        default: None,
        where_clause: None,
        annotations: vec![],
        span: zero_span(),
        content_hash: ContentHash(0),
    })
}

fn sub_member(name: &str, structure_name: &str) -> MemberDecl {
    MemberDecl::Sub(SubDecl {
        name: name.to_string(),
        structure_name: structure_name.to_string(),
        type_args: vec![],
        args: vec![],
        is_collection: false,
        where_clause: None,
        body: None,
        span: zero_span(),
        content_hash: ContentHash(0),
    })
}

fn match_arm_decl(pattern: &str, member: MemberDecl) -> MatchArmDeclArmDecl {
    MatchArmDeclArmDecl {
        patterns: vec![pattern.to_string()],
        member: Box::new(member),
        span: zero_span(),
    }
}

fn empty_structure(name: &str) -> Declaration {
    Declaration::Structure(StructureDef {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// End-to-end RED→GREEN test for `MemberDecl::MatchArmDeclGroup` compilation.
///
/// Hand-constructs the equivalent of:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure def HexHead {}
/// structure def SocketHead {}
/// structure def Bolt {
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
/// }
/// ```
///
/// Asserts:
/// (a) No diagnostic message contains both `"duplicate"` and `"head"` — the
///     cluster-registration path must never route through `scope.register()`,
///     so future dup-name tightening (task 2375) cannot misfire here.
/// (b) `template.match_arm_groups` contains exactly one `GuardedDeclGroup`
///     for `"head"` with two arms whose `arm_type` fields are
///     `StructureRef("HexHead")` and `StructureRef("SocketHead")` respectively.
#[test]
fn match_arm_decl_group_registers_cluster_without_duplicate_name_diagnostics() {
    // Build the MatchArmDeclGroup AST node.
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    // Build the Bolt structure.
    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![param_member("head_type", "HeadType"), match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    // Assemble the module: enum + referenced structures + Bolt.
    let parsed = ParsedModule {
        path: ModulePath::single("test_match_arm_decl"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                variants: vec!["Hex".to_string(), "Socket".to_string()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) No "duplicate … head" diagnostic.
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

    // (b) GuardedDeclGroup for "head" with correct per-arm arm_type.
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

/// Variant-pipe form: a single arm covers multiple patterns.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket, Button }
/// structure def HexOrButtonHead {}
/// structure def SocketHead {}
/// structure def Bolt {
///     param head_type : HeadType
///     match head_type {
///         Hex | Button => sub head : HexOrButtonHead
///         Socket       => sub head : SocketHead
///     }
/// }
/// ```
///
/// The pipe arm has `patterns: ["Hex", "Button"]`; the cluster should still
/// be registered as a `GuardedDeclGroup` named `"head"` with 2 arms.
#[test]
fn match_arm_decl_group_pipe_patterns_produce_two_arm_cluster() {
    let pipe_arm = MatchArmDeclArmDecl {
        patterns: vec!["Hex".to_string(), "Button".to_string()],
        member: Box::new(sub_member("head", "HexOrButtonHead")),
        span: zero_span(),
    };
    let socket_arm = match_arm_decl("Socket", sub_member("head", "SocketHead"));

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![pipe_arm, socket_arm],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![param_member("head_type", "HeadType"), match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_pipe_patterns"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                variants: vec![
                    "Hex".to_string(),
                    "Socket".to_string(),
                    "Button".to_string(),
                ],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("HexOrButtonHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);

    // No "duplicate head" diagnostic.
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
