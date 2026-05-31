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
    ConstraintDef, ConstraintInstDecl, Declaration, EnumDecl, EnumVariantDecl, Expr, ExprKind,
    FieldDef, FieldSource, FnBody, FnDef, FnParam, ForallConnectBody, ForallConnectDecl,
    ForallConstraintBody, ForallConstraintDecl, GuardedGroupDecl, ImportDecl, ImportKind,
    LetDecl, MAX_MEMBER_NESTING_DEPTH, MatchArmDeclArmDecl, MatchArmDeclGroupDecl, MaximizeDecl,
    MemberDecl, MemberSpanInfo, MetaBlockDecl, MinimizeDecl, ModuleDecl, NumberClass,
    OccurrenceDef, ParamDecl, ParseError, ParsedModule, PortDecl, PortRef, Pragma, PragmaArg,
    PragmaValue, PurposeDef, PurposeParam, StructureDef, SubDecl, TraitBoundRef, TraitDecl,
    TypeAliasDecl, TypeParamDecl, UnitDecl, VariantPayload, WhereClause, classify_number_literal,
    find_named_member_span, has_test_annotation, walk_specialization_scope_members,
};

// ── module-path imports ──────────────────────────────────────────────────────
use reify_ast::decl::{
    Annotation as AnnotationMod,
    Declaration as DeclarationMod,
    EnumVariantDecl as EnumVariantDeclMod,
    MAX_MEMBER_NESTING_DEPTH as MAX_MOD,
    ModuleDecl as ModuleDeclMod,
    NumberClass as NumberClassMod,
    ParseError as ParseErrorMod,
    ParsedModule as ParsedModuleMod,
    Pragma as PragmaMod,
    PragmaArg as PragmaArgMod,
    PragmaValue as PragmaValueMod,
    StructureDef as StructureDefMod,
    VariantPayload as VariantPayloadMod,
    classify_number_literal as classify_mod,
    has_test_annotation as has_test_mod,
};

// ── reify-core dep edge ──────────────────────────────────────────────────────
use reify_core::{ContentHash, ModulePath, SourceSpan};

// ── ModuleDecl / Declaration::Module / declared_module_path ─────────────────
// Step-3 (RED): these tests fail to compile until ModuleDecl, Declaration::Module,
// and ParsedModule.declared_module_path exist (step-4 impl).

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
        declared_module_path: None,
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
        variants: vec![EnumVariantDecl::unit("In")],
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
        declared_module_path: None,
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

#[test]
fn module_decl_flat_and_module_path_cross_assign() {
    // Build ModuleDecl via flat path, cross-assign to module-path alias.
    let md: ModuleDeclMod = ModuleDecl {
        path: "a.b.c".into(),
        span: SourceSpan::empty(0),
        content_hash: ContentHash(0),
    };
    // Wrap as Declaration::Module and cross-assign proves same type.
    let d: Declaration = Declaration::Module(md);
    let _same: DeclarationMod = d;
}

#[test]
fn parsed_module_with_declared_module_path() {
    // ParsedModule with declared_module_path: Some(...) must compile (step-4).
    let md = ModuleDecl {
        path: "a.b.c".into(),
        span: SourceSpan::empty(0),
        content_hash: ContentHash(0),
    };
    let m = ParsedModule {
        path: ModulePath::single("test"),
        declarations: vec![Declaration::Module(md)],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: Some(ModulePath::from_dotted("a.b.c").unwrap()),
    };
    assert!(m.declared_module_path.is_some());
    assert_eq!(m.declarations.len(), 1);
    // Cross-assign to module-path alias.
    let _same: ParsedModuleMod = m;
}

// ─────────────────────────────────────────────────────────────────────────────
// step-1 RED: AST surface contract for EnumVariantDecl / VariantPayload /
// ExprKind::VariantConstruct.
// These tests FAIL TO COMPILE until step-2 adds the new types.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn enum_variant_decl_unit_constructible() {
    // EnumVariantDecl::unit helper — wraps a bare variant name.
    let v: EnumVariantDecl = EnumVariantDecl::unit("Point");
    assert_eq!(v.name, "Point");
    match &v.payload {
        VariantPayload::Unit => {}
        other => panic!("expected Unit payload, got {:?}", other),
    }
    // Cross-assign via module-path alias proves same type.
    let _same: EnumVariantDeclMod = v;
}

#[test]
fn enum_variant_decl_named_constructible() {
    use reify_ast::ast::{TypeExpr, TypeExprKind};
    // VariantPayload::Named carries a Vec<(String, TypeExpr)>.
    let radius_type = TypeExpr {
        kind: TypeExprKind::Named { name: "Length".into(), type_args: vec![] },
        span: SourceSpan::empty(0),
    };
    let v = EnumVariantDecl {
        name: "Circle".into(),
        payload: VariantPayload::Named(vec![("radius".into(), radius_type)]),
        span: SourceSpan::empty(0),
    };
    match &v.payload {
        VariantPayload::Named(fields) => {
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].0, "radius");
        }
        other => panic!("expected Named payload, got {:?}", other),
    }
    // module-path cross-assign
    let _same: EnumVariantDeclMod = v;
}

#[test]
fn enum_variant_decl_from_str_and_string() {
    // From<&str> and From<String> convenience impls — both produce Unit payloads.
    let a: EnumVariantDecl = EnumVariantDecl::from("In");
    let b: EnumVariantDecl = EnumVariantDecl::from("Out".to_string());
    assert_eq!(a.name, "In");
    assert_eq!(b.name, "Out");
    let _same_a: VariantPayloadMod = a.payload;
    let _same_b: VariantPayloadMod = b.payload;
}

#[test]
fn enum_decl_with_named_field_variants() {
    use reify_ast::ast::{TypeExpr, TypeExprKind};
    let point = EnumVariantDecl::unit("Point");
    let radius_type = TypeExpr {
        kind: TypeExprKind::Named { name: "Length".into(), type_args: vec![] },
        span: SourceSpan::empty(0),
    };
    let circle = EnumVariantDecl {
        name: "Circle".into(),
        payload: VariantPayload::Named(vec![("radius".into(), radius_type)]),
        span: SourceSpan::empty(0),
    };
    let e = EnumDecl {
        name: "Shape".into(),
        doc: None,
        is_pub: false,
        variants: vec![point, circle],
        span: SourceSpan::empty(0),
        content_hash: ContentHash(0),
        annotations: vec![],
    };
    assert_eq!(e.variants.len(), 2);
    assert_eq!(e.variants[0].name, "Point");
    assert_eq!(e.variants[1].name, "Circle");
}

#[test]
fn expr_kind_variant_construct_constructible() {
    use reify_ast::ast::ExprKind;
    // ExprKind::VariantConstruct { name, fields: Vec<(String, Expr)> }
    let width_val = Expr {
        kind: ExprKind::QuantityLiteral {
            value: 20.0,
            unit: reify_ast::ast::UnitExpr::Unit("mm".into()),
        },
        span: SourceSpan::empty(0),
    };
    let vc = ExprKind::VariantConstruct {
        name: "Rect".into(),
        fields: vec![("width".into(), width_val)],
    };
    match &vc {
        ExprKind::VariantConstruct { name, fields } => {
            assert_eq!(name, "Rect");
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].0, "width");
        }
        _ => panic!("unexpected variant"),
    }
}
