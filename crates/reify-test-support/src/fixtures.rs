use reify_compiler::{CompiledModule, RequirementKind};
use reify_syntax::ParsedModule;
use reify_types::{BinOp, ContentHash, DimensionVector, ModulePath, SourceSpan, Type, Value};

use crate::builders::{CompiledModuleBuilder, TopologyTemplateBuilder, TraitDefBuilder};

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

/// Return the bracket source with the default width replaced by `width_str`.
///
/// E.g. `bracket_source_with_width("120mm")` gives `param width: Scalar = 120mm`.
pub fn bracket_source_with_width(width_str: &str) -> String {
    bracket_source().replace("80mm", width_str)
}

/// Return the bracket source with thickness set to 1mm, which violates the
/// `thickness > 2mm` constraint.
pub fn bracket_source_violating() -> String {
    bracket_source().replace(
        "param thickness: Scalar = 5mm",
        "param thickness: Scalar = 1mm",
    )
}

/// Create a `ParsedModule` matching the bracket source.
pub fn bracket_parsed_module() -> ParsedModule {
    use reify_syntax::*;

    let path = ModulePath::single("bracket");
    let content_hash = ContentHash::of_str(bracket_source());

    let structure = StructureDef {
        name: "Bracket".into(),
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![
            MemberDecl::Param(ParamDecl {
                name: "width".into(),
                type_expr: Some(TypeExpr {
                    name: "Scalar".into(),
                    type_args: vec![],
                    span: SourceSpan::new(29, 35),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 80.0,
                        unit: "mm".into(),
                    },
                    span: SourceSpan::new(38, 42),
                }),
                where_clause: None,
                span: SourceSpan::new(24, 42),
                content_hash: ContentHash::of_str("param width: Scalar = 80mm"),
            }),
            MemberDecl::Param(ParamDecl {
                name: "height".into(),
                type_expr: Some(TypeExpr {
                    name: "Scalar".into(),
                    type_args: vec![],
                    span: SourceSpan::new(60, 66),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 100.0,
                        unit: "mm".into(),
                    },
                    span: SourceSpan::new(69, 74),
                }),
                where_clause: None,
                span: SourceSpan::new(47, 74),
                content_hash: ContentHash::of_str("param height: Scalar = 100mm"),
            }),
            MemberDecl::Param(ParamDecl {
                name: "thickness".into(),
                type_expr: Some(TypeExpr {
                    name: "Scalar".into(),
                    type_args: vec![],
                    span: SourceSpan::new(95, 101),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 5.0,
                        unit: "mm".into(),
                    },
                    span: SourceSpan::new(104, 107),
                }),
                where_clause: None,
                span: SourceSpan::new(79, 107),
                content_hash: ContentHash::of_str("param thickness: Scalar = 5mm"),
            }),
            MemberDecl::Param(ParamDecl {
                name: "fillet_radius".into(),
                type_expr: Some(TypeExpr {
                    name: "Scalar".into(),
                    type_args: vec![],
                    span: SourceSpan::new(132, 138),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 3.0,
                        unit: "mm".into(),
                    },
                    span: SourceSpan::new(141, 144),
                }),
                where_clause: None,
                span: SourceSpan::new(112, 144),
                content_hash: ContentHash::of_str("param fillet_radius: Scalar = 3mm"),
            }),
            MemberDecl::Param(ParamDecl {
                name: "hole_diameter".into(),
                type_expr: Some(TypeExpr {
                    name: "Scalar".into(),
                    type_args: vec![],
                    span: SourceSpan::new(169, 175),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 6.0,
                        unit: "mm".into(),
                    },
                    span: SourceSpan::new(178, 181),
                }),
                where_clause: None,
                span: SourceSpan::new(149, 181),
                content_hash: ContentHash::of_str("param hole_diameter: Scalar = 6mm"),
            }),
            MemberDecl::Let(LetDecl {
                name: "volume".into(),
                is_pub: false,
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
                where_clause: None,
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
                where_clause: None,
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
                where_clause: None,
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
                where_clause: None,
                span: SourceSpan::new(300, 340),
                content_hash: ContentHash::of_str("constraint hole_diameter < thickness * 2"),
            }),
            // The `let body = box(...)` line — parsed as a let with function call
            MemberDecl::Let(LetDecl {
                name: "body".into(),
                is_pub: false,
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
                where_clause: None,
                span: SourceSpan::new(346, 385),
                content_hash: ContentHash::of_str("let body = box(width, height, thickness)"),
            }),
        ],
        span: SourceSpan::new(0, 387),
        content_hash,
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
    use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
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
        .realization(
            e,
            0,
            vec![CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".to_string(), width_ref()),
                    ("height".to_string(), height_ref()),
                    ("depth".to_string(), thickness_ref()),
                ],
            }],
        )
        .build();

    CompiledModuleBuilder::new(ModulePath::single("bracket"))
        .template(template)
        .build()
}

/// Create a `CompiledModule` with `Rigid` and `Container<T: Rigid>` traits and conforming structures.
///
/// Traits:
///   - `Rigid`: requires `param mass: Mass`
///   - `Container<T: Rigid>`: requires `param count: Int`
///
/// Structures:
///   - `Bolt: Rigid` with `param mass: Mass = 1kg`
///   - `Crate: Container` with `param count: Int = 1`
///
/// Used to test generic trait conformance checking.
pub fn generic_container_module() -> CompiledModule {
    use reify_types::{DimensionVector, TraitBound, TraitRef, TypeParam};

    let mass_type = Type::Scalar { dimension: DimensionVector::MASS };

    // Rigid trait: requires param mass: Mass
    let rigid_trait = TraitDefBuilder::new("Rigid")
        .requirement("mass", RequirementKind::Param(mass_type.clone()))
        .build();

    // Container<T: Rigid> trait: requires param count: Int
    let t_param = TypeParam {
        name: "T".to_string(),
        bounds: vec![TraitBound {
            trait_ref: TraitRef {
                name: "Rigid".to_string(),
                type_args: vec![],
            },
        }],
        default: None,
    };
    let container_trait = TraitDefBuilder::new("Container")
        .type_param(t_param)
        .requirement("count", RequirementKind::Param(Type::Int))
        .build();

    // Bolt: Rigid with param mass = 1kg
    let bolt_template = TopologyTemplateBuilder::new("Bolt")
        .trait_bound("Rigid")
        .param("Bolt", "mass", mass_type, Some(crate::builders::literal(Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        })))
        .build();

    // Crate: Container with param count = 1
    let crate_template = TopologyTemplateBuilder::new("Crate")
        .trait_bound("Container")
        .param("Crate", "count", Type::Int, Some(crate::builders::literal(Value::Int(1))))
        .build();

    CompiledModuleBuilder::new(ModulePath::single("generic_container"))
        .trait_def(rigid_trait)
        .trait_def(container_trait)
        .template(bolt_template)
        .template(crate_template)
        .build()
}

/// Create a `CompiledModule` with the `Rigid` trait and a `Bolt` structure that conforms to it.
///
/// Trait `Rigid`:
///   - requires `param mass: Mass` (DimensionVector::MASS)
///   - provides default constraint: `mass > 0kg`
///
/// Structure `Bolt: Rigid`:
///   - `param mass: Mass = 1kg` (default 1.0 SI = 1 kg)
///
/// Used to test trait conformance checking.
pub fn rigid_trait_module() -> CompiledModule {
    use reify_compiler::DefaultKind;
    use reify_syntax::{ConstraintDecl, Expr, ExprKind};
    use reify_types::DimensionVector;

    let mass_type = Type::Scalar { dimension: DimensionVector::MASS };

    // Rigid trait: requires param mass: Mass; default constraint mass > 0kg
    let mass_constraint_decl = ConstraintDecl {
        label: Some("mass_positive".to_string()),
        expr: Expr {
            kind: ExprKind::BinOp {
                op: ">".to_string(),
                left: Box::new(Expr {
                    kind: ExprKind::Ident("mass".to_string()),
                    span: SourceSpan::new(0, 0),
                }),
                right: Box::new(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 0.0,
                        unit: "kg".to_string(),
                    },
                    span: SourceSpan::new(0, 0),
                }),
            },
            span: SourceSpan::new(0, 0),
        },
        where_clause: None,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str("constraint mass > 0kg"),
    };

    let rigid_trait = TraitDefBuilder::new("Rigid")
        .requirement("mass", RequirementKind::Param(mass_type.clone()))
        .default(Some("mass_positive"), DefaultKind::Constraint(mass_constraint_decl))
        .build();

    // Bolt: Rigid with param mass: Mass = 1kg (1.0 SI)
    let bolt_template = TopologyTemplateBuilder::new("Bolt")
        .trait_bound("Rigid")
        .param("Bolt", "mass", mass_type, Some(crate::builders::literal(Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        })))
        .build();

    CompiledModuleBuilder::new(ModulePath::single("rigid_trait"))
        .trait_def(rigid_trait)
        .template(bolt_template)
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
    fn bracket_source_with_width_replaces_default() {
        let source = bracket_source_with_width("120mm");
        assert!(source.contains("param width: Scalar = 120mm"));
        assert!(!source.contains("80mm"), "original 80mm should be replaced");
        // Everything else should be intact
        assert!(source.contains("param height: Scalar = 100mm"));
        assert!(source.contains("constraint thickness > 2mm"));
    }

    #[test]
    fn bracket_source_violating_has_small_thickness() {
        let source = bracket_source_violating();
        assert!(source.contains("param thickness: Scalar = 1mm"));
        assert!(!source.contains("param thickness: Scalar = 5mm"), "original 5mm should be replaced");
        // Other params should be unchanged
        assert!(source.contains("param width: Scalar = 80mm"));
        assert!(source.contains("constraint thickness > 2mm"));
    }

    #[test]
    fn bracket_source_is_well_formed() {
        let source = bracket_source();
        assert!(source.contains("structure Bracket"));
        assert!(source.contains("param width"));
        assert!(source.contains("constraint thickness > 2mm"));
        assert!(source.contains("let body = box("));
    }

    // step-11: failing test for generic_container_module fixture
    #[test]
    fn generic_container_module_structure() {
        let module = generic_container_module();
        // 2 traits: Rigid and Container
        assert_eq!(module.trait_defs.len(), 2);
        let rigid = module.trait_defs.iter().find(|t| t.name == "Rigid");
        let container = module.trait_defs.iter().find(|t| t.name == "Container");
        assert!(rigid.is_some(), "should have Rigid trait");
        let container = container.expect("should have Container trait");
        // Container has type_param T with Rigid bound
        assert_eq!(container.type_params.len(), 1);
        assert_eq!(container.type_params[0].name, "T");
        assert_eq!(container.type_params[0].bounds[0].trait_ref.name, "Rigid");
        // Container requires param count: Int
        assert_eq!(container.required_members.len(), 1);
        assert_eq!(container.required_members[0].name, "count");
        // 2 templates: Bolt and Crate
        assert_eq!(module.templates.len(), 2);
        let bolt = module.templates.iter().find(|t| t.name == "Bolt");
        let crate_t = module.templates.iter().find(|t| t.name == "Crate");
        assert!(bolt.is_some(), "should have Bolt template");
        let crate_t = crate_t.expect("should have Crate template");
        // Crate conforms to Container
        assert!(crate_t.trait_bounds.contains(&"Container".to_string()));
    }

    // step-9: failing test for rigid_trait_module fixture
    #[test]
    fn rigid_trait_module_structure() {
        let module = rigid_trait_module();
        // 1 trait: Rigid
        assert_eq!(module.trait_defs.len(), 1);
        let rigid = &module.trait_defs[0];
        assert_eq!(rigid.name, "Rigid");
        // Rigid requires param mass: Mass
        assert_eq!(rigid.required_members.len(), 1);
        assert_eq!(rigid.required_members[0].name, "mass");
        // Rigid has 1 default: constraint mass > 0kg
        assert_eq!(rigid.defaults.len(), 1);
        // 1 template: Bolt
        assert_eq!(module.templates.len(), 1);
        let bolt = &module.templates[0];
        assert_eq!(bolt.name, "Bolt");
        // Bolt conforms to Rigid
        assert_eq!(bolt.trait_bounds.len(), 1);
        assert_eq!(bolt.trait_bounds[0], "Rigid");
        // Bolt has param mass
        let mass_cell = bolt.value_cells.iter().find(|vc| vc.id.member == "mass");
        assert!(mass_cell.is_some(), "Bolt should have param mass");
    }

    #[test]
    fn parent_child_module_structure() {
        let module = parent_child_module();
        assert_eq!(module.templates.len(), 2);

        // Child template is first
        let child = &module.templates[0];
        assert_eq!(child.name, "Child");
        assert_eq!(child.value_cells.len(), 2); // param height + let half_h
        assert!(child.sub_components.is_empty());

        // Parent template is second
        let parent = &module.templates[1];
        assert_eq!(parent.name, "Parent");
        assert_eq!(parent.value_cells.len(), 1); // param width
        assert_eq!(parent.sub_components.len(), 1);
        assert_eq!(parent.sub_components[0].name, "rib");
        assert_eq!(parent.sub_components[0].structure_name, "Child");
        assert_eq!(parent.sub_components[0].args.len(), 1);
        assert_eq!(parent.sub_components[0].args[0].0, "height");
    }
}

/// Create a `CompiledModule` with a parent/child relationship for sub-component testing.
///
/// Returns a module with two templates:
/// - `Child` with `param height: Scalar(LENGTH) = 10mm` (0.01 SI) and
///   `let half_h = height / 2`
/// - `Parent` with `param width: Scalar(LENGTH) = 80mm` (0.08 SI) and
///   `sub rib = Child(height: width * 0.5)`
///
/// Child is listed first so it can be found by structure_name lookup.
pub fn parent_child_module() -> CompiledModule {
    use reify_types::CompiledExpr;

    let child_entity = "Child";
    let parent_entity = "Parent";

    // mm literal helper
    let mm_literal = |v: f64| {
        CompiledExpr::literal(crate::mm(v), Type::length())
    };

    // Child template: param height = 10mm, let half_h = height / 2
    let height_ref = || CompiledExpr::value_ref(crate::vcid(child_entity, "height"), Type::length());

    let half_h_expr = CompiledExpr::binop(
        BinOp::Div,
        height_ref(),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::length(),
    );

    let child_template = TopologyTemplateBuilder::new(child_entity)
        .param(child_entity, "height", Type::length(), Some(mm_literal(10.0)))
        .let_binding(child_entity, "half_h", Type::length(), half_h_expr)
        .build();

    // Parent template: param width = 80mm, sub rib = Child(height: width * 0.5)
    let width_ref = || CompiledExpr::value_ref(crate::vcid(parent_entity, "width"), Type::length());

    let arg_expr = CompiledExpr::binop(
        BinOp::Mul,
        width_ref(),
        CompiledExpr::literal(Value::Real(0.5), Type::Real),
        Type::length(),
    );

    let parent_template = TopologyTemplateBuilder::new(parent_entity)
        .param(parent_entity, "width", Type::length(), Some(mm_literal(80.0)))
        .sub_component("rib", "Child", vec![("height".to_string(), arg_expr)])
        .build();

    CompiledModuleBuilder::new(ModulePath::single("parent_child"))
        .template(child_template)
        .template(parent_template)
        .build()
}
