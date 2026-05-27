//! Compile-time surface pin for the declaration AST + parser helpers moved to
//! `reify-ast` from `reify-syntax` (Phase 2 ε of docs/prds/core-ast-ir-layering.md).
//!
//! Pins flat (`reify_ast::ParsedModule`) AND module-path
//! (`reify_ast::decl::ParsedModule`) access for every moved declaration AST
//! symbol, plus `classify_number_literal`/`NumberClass`/`has_test_annotation`
//! behaviour.
//!
//! **This file intentionally fails to compile before step-2 because:**
//!   (1) `reify_ast::decl` module does not yet exist,
//!   (2) declaration types are still defined in `reify-syntax`,
//!   (3) `has_test_annotation` is a new function that does not yet exist anywhere.

#![allow(unused_imports, dead_code)]

// ── flat root imports ────────────────────────────────────────────────────────
use reify_ast::{
    Annotation, AssociatedTypeDecl, ChainDecl, ConnectDecl, ConnectOp, ConstraintDecl,
    ConstraintDef, ConstraintInstDecl, Declaration, EnumDecl, Expr, ExprKind, FieldDef,
    FieldSource, FnBody, FnDef, FnParam, ForallConnectBody, ForallConnectDecl,
    ForallConstraintBody, ForallConstraintDecl, GuardedGroupDecl, ImportDecl, ImportKind,
    LetDecl, MAX_MEMBER_NESTING_DEPTH, MatchArmDeclArmDecl, MatchArmDeclGroupDecl, MaximizeDecl,
    MemberDecl, MemberSpanInfo, MetaBlockDecl, MinimizeDecl, NumberClass, OccurrenceDef,
    ParamDecl, ParseError, ParsedModule, PortDecl, PortRef, Pragma, PragmaArg, PragmaValue,
    PurposeDef, PurposeParam, StructureDef, SubDecl, TraitBoundRef, TraitDecl, TypeAliasDecl,
    TypeParamDecl, UnitDecl, WhereClause, classify_number_literal, find_named_member_span,
    has_test_annotation, walk_specialization_scope_members,
};

// ── module-path imports ──────────────────────────────────────────────────────
use reify_ast::decl::{
    Annotation as AnnotationMod,
    Declaration as DeclarationMod,
    MAX_MEMBER_NESTING_DEPTH as MAX_MOD,
    NumberClass as NumberClassMod,
    ParseError as ParseErrorMod,
    ParsedModule as ParsedModuleMod,
    Pragma as PragmaMod,
    PragmaArg as PragmaArgMod,
    PragmaValue as PragmaValueMod,
    StructureDef as StructureDefMod,
    classify_number_literal as classify_mod,
    has_test_annotation as has_test_mod,
};

// ── reify-core dep edge ──────────────────────────────────────────────────────
use reify_core::{ContentHash, ModulePath, SourceSpan};

// ─────────────────────────────────────────────────────────────────────────────
// Cross-assignment proofs (flat == module-path)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn parsed_module_flat_and_module_path_cross_assign() {
    // Build via flat path, cross-assign to module-path alias.
    let m: ParsedModuleMod = ParsedModule {
        path: ModulePath::single("test"),
        declarations: vec![],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };
    // Cross-assign module-path → flat proves same type.
    let _same: ParsedModule = m;
}

#[test]
fn declaration_flat_and_module_path_cross_assign() {
    let e = EnumDecl {
        name: "Dir".into(),
        doc: None,
        is_pub: false,
        variants: vec!["In".into()],
        span: SourceSpan::empty(0),
        content_hash: ContentHash(0),
        annotations: vec![],
    };
    let d: Declaration = Declaration::Enum(e);
    let _same: DeclarationMod = d;
}

#[test]
fn annotation_flat_and_module_path_cross_assign() {
    let a: AnnotationMod = Annotation {
        name: "test".into(),
        args: vec![],
        span: SourceSpan::empty(0),
    };
    let _same: Annotation = a;
}

#[test]
fn pragma_flat_and_module_path_cross_assign() {
    let p: PragmaMod = Pragma {
        name: "optimize".into(),
        args: vec![],
        span: SourceSpan::empty(0),
    };
    let _same: Pragma = p;
}

#[test]
fn number_class_flat_and_module_path_cross_assign() {
    let n: NumberClassMod = NumberClass::Int(42);
    let _same: NumberClass = n;
}

#[test]
fn max_member_nesting_depth_same_via_both_paths() {
    assert_eq!(MAX_MEMBER_NESTING_DEPTH, MAX_MOD);
}

// ─────────────────────────────────────────────────────────────────────────────
// Constructive shape tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn parsed_module_with_structure_declaration() {
    let structure = StructureDef {
        name: "Bracket".into(),
        doc: None,
        is_pub: true,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![],
        span: SourceSpan::empty(0),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    };
    let module = ParsedModule {
        path: ModulePath::single("test"),
        declarations: vec![Declaration::Structure(structure)],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };
    assert_eq!(module.declarations.len(), 1);
    assert!(module.errors.is_empty());
}

#[test]
fn pragma_with_quantity_arg_constructible() {
    let pv = PragmaValue::Quantity { value: 0.001, unit: "m".into() };
    let arg = PragmaArg::KeyValue { key: "min_wall".into(), value: pv };
    let pragma = Pragma {
        name: "tolerance".into(),
        args: vec![arg],
        span: SourceSpan::empty(0),
    };
    assert_eq!(pragma.name, "tolerance");
    assert_eq!(pragma.args.len(), 1);
    match &pragma.args[0] {
        PragmaArg::KeyValue { key, value: PragmaValue::Quantity { value, unit } } => {
            assert_eq!(key, "min_wall");
            assert!((value - 0.001).abs() < 1e-12);
            assert_eq!(unit, "m");
        }
        _ => panic!("expected KeyValue(Quantity)"),
    }
}
