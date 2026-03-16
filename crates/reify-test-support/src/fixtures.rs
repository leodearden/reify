use reify_compiler::CompiledModule;
use reify_syntax::ParsedModule;
use reify_types::{BinOp, ContentHash, DimensionVector, ModulePath, SourceSpan, Type, Value};

use crate::builders::{CompiledModuleBuilder, TopologyTemplateBuilder};

/// The canonical bracket source code for end-to-end testing.
pub fn bracket_source() -> &'static str {
    r#"structure Bracket {
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm
    param fillet_radius: Scalar = 3mm
    param hole_diameter: Scalar = 6mm

    let volume = width * height * thickness

    constraint thickness > 2mm
    constraint thickness < width / 4
    constraint hole_diameter < thickness * 2

    let body = box(width, height, thickness)
}"#
}

/// Create a `ParsedModule` matching the bracket source.
pub fn bracket_parsed_module() -> ParsedModule {
    use reify_syntax::*;

    let path = ModulePath::single("bracket");
    let content_hash = ContentHash::of_str(bracket_source());

    let structure = StructureDef {
        name: "Bracket".into(),
        members: vec![
            MemberDecl::Param(ParamDecl {
                name: "width".into(),
                type_expr: Some(TypeExpr {
                    name: "Scalar".into(),
                    span: SourceSpan::new(29, 35),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 80.0,
                        unit: "mm".into(),
                    },
                    span: SourceSpan::new(38, 42),
                }),
                span: SourceSpan::new(24, 42),
                content_hash: ContentHash::of_str("param width: Scalar = 80mm"),
            }),
            MemberDecl::Param(ParamDecl {
                name: "height".into(),
                type_expr: Some(TypeExpr {
                    name: "Scalar".into(),
                    span: SourceSpan::new(60, 66),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 100.0,
                        unit: "mm".into(),
                    },
                    span: SourceSpan::new(69, 74),
                }),
                span: SourceSpan::new(47, 74),
                content_hash: ContentHash::of_str("param height: Scalar = 100mm"),
            }),
            MemberDecl::Param(ParamDecl {
                name: "thickness".into(),
                type_expr: Some(TypeExpr {
                    name: "Scalar".into(),
                    span: SourceSpan::new(95, 101),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 5.0,
                        unit: "mm".into(),
                    },
                    span: SourceSpan::new(104, 107),
                }),
                span: SourceSpan::new(79, 107),
                content_hash: ContentHash::of_str("param thickness: Scalar = 5mm"),
            }),
            MemberDecl::Param(ParamDecl {
                name: "fillet_radius".into(),
                type_expr: Some(TypeExpr {
                    name: "Scalar".into(),
                    span: SourceSpan::new(132, 138),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 3.0,
                        unit: "mm".into(),
                    },
                    span: SourceSpan::new(141, 144),
                }),
                span: SourceSpan::new(112, 144),
                content_hash: ContentHash::of_str("param fillet_radius: Scalar = 3mm"),
            }),
            MemberDecl::Param(ParamDecl {
                name: "hole_diameter".into(),
                type_expr: Some(TypeExpr {
                    name: "Scalar".into(),
                    span: SourceSpan::new(169, 175),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 6.0,
                        unit: "mm".into(),
                    },
                    span: SourceSpan::new(178, 181),
                }),
                span: SourceSpan::new(149, 181),
                content_hash: ContentHash::of_str("param hole_diameter: Scalar = 6mm"),
            }),
            MemberDecl::Let(LetDecl {
                name: "volume".into(),
                type_expr: None,
                value: Expr {
                    kind: ExprKind::BinOp {
                        op: "*".into(),
                        left: Box::new(Expr {
                            kind: ExprKind::BinOp {
                                op: "*".into(),
                                left: Box::new(Expr {
                                    kind: ExprKind::Ident("width".into()),
                                    span: SourceSpan::new(200, 205),
                                }),
                                right: Box::new(Expr {
                                    kind: ExprKind::Ident("height".into()),
                                    span: SourceSpan::new(208, 214),
                                }),
                            },
                            span: SourceSpan::new(200, 214),
                        }),
                        right: Box::new(Expr {
                            kind: ExprKind::Ident("thickness".into()),
                            span: SourceSpan::new(217, 226),
                        }),
                    },
                    span: SourceSpan::new(200, 226),
                },
                span: SourceSpan::new(187, 226),
                content_hash: ContentHash::of_str("let volume = width * height * thickness"),
            }),
            MemberDecl::Constraint(ConstraintDecl {
                label: None,
                expr: Expr {
                    kind: ExprKind::BinOp {
                        op: ">".into(),
                        left: Box::new(Expr {
                            kind: ExprKind::Ident("thickness".into()),
                            span: SourceSpan::new(243, 252),
                        }),
                        right: Box::new(Expr {
                            kind: ExprKind::QuantityLiteral {
                                value: 2.0,
                                unit: "mm".into(),
                            },
                            span: SourceSpan::new(255, 258),
                        }),
                    },
                    span: SourceSpan::new(243, 258),
                },
                span: SourceSpan::new(232, 258),
                content_hash: ContentHash::of_str("constraint thickness > 2mm"),
            }),
            MemberDecl::Constraint(ConstraintDecl {
                label: None,
                expr: Expr {
                    kind: ExprKind::BinOp {
                        op: "<".into(),
                        left: Box::new(Expr {
                            kind: ExprKind::Ident("thickness".into()),
                            span: SourceSpan::new(274, 283),
                        }),
                        right: Box::new(Expr {
                            kind: ExprKind::BinOp {
                                op: "/".into(),
                                left: Box::new(Expr {
                                    kind: ExprKind::Ident("width".into()),
                                    span: SourceSpan::new(286, 291),
                                }),
                                right: Box::new(Expr {
                                    kind: ExprKind::NumberLiteral(4.0),
                                    span: SourceSpan::new(294, 295),
                                }),
                            },
                            span: SourceSpan::new(286, 295),
                        }),
                    },
                    span: SourceSpan::new(274, 295),
                },
                span: SourceSpan::new(263, 295),
                content_hash: ContentHash::of_str("constraint thickness < width / 4"),
            }),
            MemberDecl::Constraint(ConstraintDecl {
                label: None,
                expr: Expr {
                    kind: ExprKind::BinOp {
                        op: "<".into(),
                        left: Box::new(Expr {
                            kind: ExprKind::Ident("hole_diameter".into()),
                            span: SourceSpan::new(311, 324),
                        }),
                        right: Box::new(Expr {
                            kind: ExprKind::BinOp {
                                op: "*".into(),
                                left: Box::new(Expr {
                                    kind: ExprKind::Ident("thickness".into()),
                                    span: SourceSpan::new(327, 336),
                                }),
                                right: Box::new(Expr {
                                    kind: ExprKind::NumberLiteral(2.0),
                                    span: SourceSpan::new(339, 340),
                                }),
                            },
                            span: SourceSpan::new(327, 340),
                        }),
                    },
                    span: SourceSpan::new(311, 340),
                },
                span: SourceSpan::new(300, 340),
                content_hash: ContentHash::of_str("constraint hole_diameter < thickness * 2"),
            }),
            // The `let body = box(...)` line — parsed as a let with function call
            MemberDecl::Let(LetDecl {
                name: "body".into(),
                type_expr: None,
                value: Expr {
                    kind: ExprKind::FunctionCall {
                        name: "box".into(),
                        args: vec![
                            Expr {
                                kind: ExprKind::Ident("width".into()),
                                span: SourceSpan::new(360, 365),
                            },
                            Expr {
                                kind: ExprKind::Ident("height".into()),
                                span: SourceSpan::new(367, 373),
                            },
                            Expr {
                                kind: ExprKind::Ident("thickness".into()),
                                span: SourceSpan::new(375, 384),
                            },
                        ],
                    },
                    span: SourceSpan::new(356, 385),
                },
                span: SourceSpan::new(346, 385),
                content_hash: ContentHash::of_str("let body = box(width, height, thickness)"),
            }),
        ],
        span: SourceSpan::new(0, 387),
        content_hash: content_hash,
    };

    ParsedModule {
        path,
        declarations: vec![reify_syntax::Declaration::Structure(structure)],
        errors: vec![],
        content_hash,
    }
}

/// Create a `CompiledModule` matching the bracket source.
/// Uses the test builders to construct a realistic compiled form.
pub fn bracket_compiled_module() -> CompiledModule {
    use reify_types::CompiledExpr;

    let e = "Bracket";

    // Expression builders for bracket params
    let width_ref = || CompiledExpr::value_ref(crate::vcid(e, "width"), Type::length());
    let height_ref = || CompiledExpr::value_ref(crate::vcid(e, "height"), Type::length());
    let thickness_ref = || CompiledExpr::value_ref(crate::vcid(e, "thickness"), Type::length());
    let hole_diam_ref = || CompiledExpr::value_ref(crate::vcid(e, "hole_diameter"), Type::length());

    // Default values as compiled expressions
    let mm_literal = |v: f64| {
        CompiledExpr::literal(crate::mm(v), Type::length())
    };

    // volume = width * height * thickness
    let width_times_height = CompiledExpr::binop(
        BinOp::Mul,
        width_ref(),
        height_ref(),
        Type::Scalar {
            dimension: DimensionVector::AREA,
        },
    );
    let volume_expr = CompiledExpr::binop(
        BinOp::Mul,
        width_times_height,
        thickness_ref(),
        Type::Scalar {
            dimension: DimensionVector::VOLUME,
        },
    );

    // constraint 0: thickness > 2mm
    let constraint_0 = CompiledExpr::binop(
        BinOp::Gt,
        thickness_ref(),
        mm_literal(2.0),
        Type::Bool,
    );

    // constraint 1: thickness < width / 4
    let width_div_4 = CompiledExpr::binop(
        BinOp::Div,
        width_ref(),
        CompiledExpr::literal(Value::Int(4), Type::Int),
        Type::length(),
    );
    let constraint_1 = CompiledExpr::binop(
        BinOp::Lt,
        thickness_ref(),
        width_div_4,
        Type::Bool,
    );

    // constraint 2: hole_diameter < thickness * 2
    let thickness_times_2 = CompiledExpr::binop(
        BinOp::Mul,
        thickness_ref(),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::length(),
    );
    let constraint_2 = CompiledExpr::binop(
        BinOp::Lt,
        hole_diam_ref(),
        thickness_times_2,
        Type::Bool,
    );

    let template = TopologyTemplateBuilder::new("Bracket")
        .param(e, "width", Type::length(), Some(mm_literal(80.0)))
        .param(e, "height", Type::length(), Some(mm_literal(100.0)))
        .param(e, "thickness", Type::length(), Some(mm_literal(5.0)))
        .param(e, "fillet_radius", Type::length(), Some(mm_literal(3.0)))
        .param(e, "hole_diameter", Type::length(), Some(mm_literal(6.0)))
        .let_binding(
            e,
            "volume",
            Type::Scalar {
                dimension: DimensionVector::VOLUME,
            },
            volume_expr,
        )
        .constraint(e, 0, None, constraint_0)
        .constraint(e, 1, None, constraint_1)
        .constraint(e, 2, None, constraint_2)
        .build();

    CompiledModuleBuilder::new(ModulePath::single("bracket"))
        .template(template)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracket_parsed_module_structure() {
        let module = bracket_parsed_module();
        assert_eq!(module.declarations.len(), 1);
        match &module.declarations[0] {
            reify_syntax::Declaration::Structure(s) => {
                assert_eq!(s.name, "Bracket");
                // 5 params + 2 lets + 3 constraints = 10 members
                assert_eq!(s.members.len(), 10);
                let params: Vec<_> = s.members.iter().filter(|m| matches!(m, reify_syntax::MemberDecl::Param(_))).collect();
                let lets: Vec<_> = s.members.iter().filter(|m| matches!(m, reify_syntax::MemberDecl::Let(_))).collect();
                let constraints: Vec<_> = s.members.iter().filter(|m| matches!(m, reify_syntax::MemberDecl::Constraint(_))).collect();
                assert_eq!(params.len(), 5);
                assert_eq!(lets.len(), 2); // volume + body
                assert_eq!(constraints.len(), 3);
            }
            _ => panic!("expected Structure declaration"),
        }
    }

    #[test]
    fn bracket_compiled_module_structure() {
        let module = bracket_compiled_module();
        assert_eq!(module.templates.len(), 1);
        let t = &module.templates[0];
        assert_eq!(t.name, "Bracket");
        // 5 params + 1 let (volume) = 6 value cells
        assert_eq!(t.value_cells.len(), 6);
        assert_eq!(t.constraints.len(), 3);
    }

    #[test]
    fn bracket_source_is_well_formed() {
        let source = bracket_source();
        assert!(source.contains("structure Bracket"));
        assert!(source.contains("param width"));
        assert!(source.contains("constraint thickness > 2mm"));
        assert!(source.contains("let body = box("));
    }
}
