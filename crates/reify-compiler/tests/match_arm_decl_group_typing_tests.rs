//! Integration tests for `MemberDecl::MatchArmDeclGroup` typing (task 2373).
//!
//! These tests hand-construct a `ParsedModule` containing a match-arm decl
//! group and verify that:
//!   (a) `self.<group>` resolves to a `Type::Union` of the per-arm structure
//!       references (PRD `match-block-decls.md` §6.4),
//!   (b) nested `self.<group>.<member>` access typechecks against the
//!       common-field type when present in every arm,
//!   (c) external `<sub>.<group>.<member>` access typechecks the same way,
//!   (d) arm-specific fields produce a precise diagnostic listing missing arms.
//!
//! The test style mirrors `match_arm_decl_group_compile_tests.rs` (helpers
//! inline, not factored into shared common/mod.rs) to keep typing-side and
//! producer-side tests symmetric.

use reify_compiler::ValueCellDecl;
use reify_syntax::{
    Declaration, EnumDecl, Expr, ExprKind, LetDecl, MatchArmDeclArmDecl, MatchArmDeclGroupDecl,
    MemberDecl, ParamDecl, ParsedModule, StructureDef, SubDecl, TypeExpr, TypeExprKind,
};
use reify_types::{ContentHash, ModulePath, Severity, SourceSpan, Type};

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

fn member_access(object: Expr, member: &str) -> Expr {
    Expr {
        kind: ExprKind::MemberAccess {
            object: Box::new(object),
            member: member.to_string(),
        },
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

fn let_member(name: &str, value: Expr) -> MemberDecl {
    MemberDecl::Let(LetDecl {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        type_expr: None,
        value,
        where_clause: None,
        annotations: vec![],
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

fn structure_with_members(name: &str, members: Vec<MemberDecl>) -> Declaration {
    Declaration::Structure(StructureDef {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members,
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    })
}

fn head_type_enum() -> Declaration {
    Declaration::Enum(EnumDecl {
        name: "HeadType".to_string(),
        doc: None,
        is_pub: false,
        variants: vec!["Hex".to_string(), "Socket".to_string()],
        span: zero_span(),
        content_hash: ContentHash(0),
        annotations: vec![],
    })
}

/// Find the let-cell `name` in template `template_name` and return its type.
fn find_cell_type(
    compiled: &reify_compiler::CompiledModule,
    template_name: &str,
    member_name: &str,
) -> Option<Type> {
    let template = compiled.templates.iter().find(|t| t.name == template_name)?;
    template
        .value_cells
        .iter()
        .find(|c: &&ValueCellDecl| c.id.member == member_name)
        .map(|c| c.cell_type.clone())
}

fn error_diagnostics(compiled: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// Step-7: `self.<cluster>` resolves to a `Type::Union` of the per-arm
/// structure references.
///
/// Constructs the equivalent of:
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
///     let probe = self.head
/// }
/// ```
#[test]
fn self_dot_match_cluster_resolves_to_union_of_arm_types() {
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let probe = let_member("probe", member_access(make_ident_expr("self"), "head"));

    let bolt = structure_with_members(
        "Bolt",
        vec![param_member("head_type", "HeadType"), match_group, probe],
    );

    let parsed = ParsedModule {
        path: ModulePath::single("test_self_match_cluster_union"),
        declarations: vec![
            head_type_enum(),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) No Error diagnostics.
    let errors = error_diagnostics(&compiled);
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:#?}",
        errors
    );

    // (b) `probe` cell exists with cell_type Union(HexHead, SocketHead).
    let probe_type = find_cell_type(&compiled, "Bolt", "probe")
        .expect("expected `probe` value cell on Bolt template");

    let expected = Type::Union(vec![
        Type::StructureRef("HexHead".to_string()),
        Type::StructureRef("SocketHead".to_string()),
    ]);
    assert_eq!(
        probe_type, expected,
        "expected probe.cell_type == Union<HexHead | SocketHead>, got {}",
        probe_type
    );
}
