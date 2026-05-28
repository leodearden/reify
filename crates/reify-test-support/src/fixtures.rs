use reify_compiler::{CompiledModule, RequirementKind, ValueCellDecl, ValueCellKind, Visibility};
use reify_ast::ParsedModule;
use reify_core::{ContentHash, DEPRECATED_ANNOTATION, DimensionVector, ModulePath, OPTIMIZED_ANNOTATION, SOLVER_HINT_ANNOTATION, SourceSpan, TEST_ANNOTATION, Type, ValueCellId};
use reify_ir::{BinOp, ConstraintSolver, SolveResult, Value};

use crate::builders::{
    CompiledFieldBuilder, CompiledModuleBuilder, CompiledPurposeBuilder, CompiledTraitBuilder,
    TopologyTemplateBuilder, TraitDefBuilder, ann_str, annotation, annotation_with_args,
    range_constraint,
};

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

/// Source that reliably produces an "unknown port type" warning (not error).
///
/// Used by tests that need a non-empty `compiled.diagnostics` to exercise
/// post-early-exit code paths in `get_diagnostics`.
/// Validated by `crates/reify-compiler/tests/port_compile_tests.rs:101-124`.
pub fn warn_source_with_unknown_port_type() -> &'static str {
    r#"structure def S {
    port mount : NonExistentTrait {
        param d : Length = 5mm
    }
}"#
}

/// Same as [`warn_source_with_unknown_port_type`] but with an additional `param width : Length = 80mm`.
///
/// Used by tests that need both an unknown-port-type warning AND a `width` field
/// for `get_source_location` lookup.
pub fn warn_source_with_unknown_port_type_with_width() -> &'static str {
    r#"structure def S {
    param width : Length = 80mm
    port mount : NonExistentTrait {
        param d : Length = 5mm
    }
}"#
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
    use reify_ast::*;

    let path = ModulePath::single("bracket");
    let content_hash = ContentHash::of_str(bracket_source());

    let structure = StructureDef {
        name: "Bracket".into(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![
            MemberDecl::Param(ParamDecl {
                name: "width".into(),
                doc: None,
                type_expr: Some(TypeExpr {
                    kind: TypeExprKind::Named {
                        name: "Scalar".into(),
                        type_args: vec![],
                    },
                    span: SourceSpan::new(29, 35),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 80.0,
                        unit: UnitExpr::Unit("mm".to_string()),
                    },
                    span: SourceSpan::new(38, 42),
                }),
                where_clause: None,
                annotations: Vec::new(),
                span: SourceSpan::new(24, 42),
                content_hash: ContentHash::of_str("param width: Scalar = 80mm"),
            }),
            MemberDecl::Param(ParamDecl {
                name: "height".into(),
                doc: None,
                type_expr: Some(TypeExpr {
                    kind: TypeExprKind::Named {
                        name: "Scalar".into(),
                        type_args: vec![],
                    },
                    span: SourceSpan::new(60, 66),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 100.0,
                        unit: UnitExpr::Unit("mm".to_string()),
                    },
                    span: SourceSpan::new(69, 74),
                }),
                where_clause: None,
                annotations: Vec::new(),
                span: SourceSpan::new(47, 74),
                content_hash: ContentHash::of_str("param height: Scalar = 100mm"),
            }),
            MemberDecl::Param(ParamDecl {
                name: "thickness".into(),
                doc: None,
                type_expr: Some(TypeExpr {
                    kind: TypeExprKind::Named {
                        name: "Scalar".into(),
                        type_args: vec![],
                    },
                    span: SourceSpan::new(95, 101),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 5.0,
                        unit: UnitExpr::Unit("mm".to_string()),
                    },
                    span: SourceSpan::new(104, 107),
                }),
                where_clause: None,
                annotations: Vec::new(),
                span: SourceSpan::new(79, 107),
                content_hash: ContentHash::of_str("param thickness: Scalar = 5mm"),
            }),
            MemberDecl::Param(ParamDecl {
                name: "fillet_radius".into(),
                doc: None,
                type_expr: Some(TypeExpr {
                    kind: TypeExprKind::Named {
                        name: "Scalar".into(),
                        type_args: vec![],
                    },
                    span: SourceSpan::new(132, 138),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 3.0,
                        unit: UnitExpr::Unit("mm".to_string()),
                    },
                    span: SourceSpan::new(141, 144),
                }),
                where_clause: None,
                annotations: Vec::new(),
                span: SourceSpan::new(112, 144),
                content_hash: ContentHash::of_str("param fillet_radius: Scalar = 3mm"),
            }),
            MemberDecl::Param(ParamDecl {
                name: "hole_diameter".into(),
                doc: None,
                type_expr: Some(TypeExpr {
                    kind: TypeExprKind::Named {
                        name: "Scalar".into(),
                        type_args: vec![],
                    },
                    span: SourceSpan::new(169, 175),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 6.0,
                        unit: UnitExpr::Unit("mm".to_string()),
                    },
                    span: SourceSpan::new(178, 181),
                }),
                where_clause: None,
                annotations: Vec::new(),
                span: SourceSpan::new(149, 181),
                content_hash: ContentHash::of_str("param hole_diameter: Scalar = 6mm"),
            }),
            MemberDecl::Let(LetDecl {
                name: "volume".into(),
                doc: None,
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
                annotations: Vec::new(),
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
                                unit: UnitExpr::Unit("mm".to_string()),
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
                                    kind: ExprKind::NumberLiteral {
                                        value: 4.0,
                                        is_real: false,
                                    },
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
                                    kind: ExprKind::NumberLiteral {
                                        value: 2.0,
                                        is_real: false,
                                    },
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
                doc: None,
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
                annotations: Vec::new(),
                span: SourceSpan::new(346, 385),
                content_hash: ContentHash::of_str("let body = box(width, height, thickness)"),
            }),
        ],
        span: SourceSpan::new(0, 387),
        content_hash,
        pragmas: vec![],
        annotations: vec![],
    };

    ParsedModule {
        path,
        declarations: vec![reify_ast::Declaration::Structure(structure)],
        errors: vec![],
        content_hash,
        pragmas: vec![],
    }
}

/// Create a `CompiledModule` matching the bracket source.
/// Uses the test builders to construct a realistic compiled form.
pub fn bracket_compiled_module() -> CompiledModule {
    use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
    use reify_ir::CompiledExpr;

    let e = "Bracket";

    // Expression builders for bracket params
    let width_ref = || CompiledExpr::value_ref(crate::vcid(e, "width"), Type::length());
    let height_ref = || CompiledExpr::value_ref(crate::vcid(e, "height"), Type::length());
    let thickness_ref = || CompiledExpr::value_ref(crate::vcid(e, "thickness"), Type::length());
    let hole_diam_ref = || CompiledExpr::value_ref(crate::vcid(e, "hole_diameter"), Type::length());

    // Default values as compiled expressions
    let mm_literal = |v: f64| CompiledExpr::literal(crate::mm(v), Type::length());

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
    let constraint_0 = CompiledExpr::binop(BinOp::Gt, thickness_ref(), mm_literal(2.0), Type::Bool);

    // constraint 1: thickness < width / 4
    let width_div_4 = CompiledExpr::binop(
        BinOp::Div,
        width_ref(),
        CompiledExpr::literal(Value::Int(4), Type::Int),
        Type::length(),
    );
    let constraint_1 = CompiledExpr::binop(BinOp::Lt, thickness_ref(), width_div_4, Type::Bool);

    // constraint 2: hole_diameter < thickness * 2
    let thickness_times_2 = CompiledExpr::binop(
        BinOp::Mul,
        thickness_ref(),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::length(),
    );
    let constraint_2 =
        CompiledExpr::binop(BinOp::Lt, hole_diam_ref(), thickness_times_2, Type::Bool);

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

/// Create a `CompiledModule` with a `Beam` structure with multiple dimensional and labeled constraints.
///
/// Structure `Beam`:
///   - `param width: Scalar(LENGTH) = 50mm`
///   - `param height: Scalar(LENGTH) = 100mm`
///   - range constraints on width: `width > 10mm` and `width < 500mm`
///   - range constraints on height: `height > 10mm` and `height < 1000mm`
///   - ratio constraint: `height > 2 * width` (labeled "slender")
///
/// Used to test constraint checking with dimensional and labeled constraints.
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
    use reify_core::DimensionVector;
    use reify_ir::{TraitBound, TraitRef, TypeParam};

    let mass_type = Type::Scalar {
        dimension: DimensionVector::MASS,
    };

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
        .param(
            "Bolt",
            "mass",
            mass_type,
            Some(crate::builders::literal(Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            })),
        )
        .build();

    // Crate: Container with param count = 1
    let crate_template = TopologyTemplateBuilder::new("Crate")
        .trait_bound("Container")
        .param(
            "Crate",
            "count",
            Type::Int,
            Some(crate::builders::literal(Value::Int(1))),
        )
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
    use reify_ast::{ConstraintDecl, Expr, ExprKind, UnitExpr};
    use reify_core::DimensionVector;

    let mass_type = Type::Scalar {
        dimension: DimensionVector::MASS,
    };

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
                        unit: UnitExpr::Unit("kg".to_string()),
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
        .add_default(
            Some("mass_positive"),
            DefaultKind::Constraint(mass_constraint_decl),
        )
        .build();

    // Bolt: Rigid with param mass: Mass = 1kg (1.0 SI)
    let bolt_template = TopologyTemplateBuilder::new("Bolt")
        .trait_bound("Rigid")
        .param(
            "Bolt",
            "mass",
            mass_type,
            Some(crate::builders::literal(Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            })),
        )
        .build();

    CompiledModuleBuilder::new(ModulePath::single("rigid_trait"))
        .trait_def(rigid_trait)
        .template(bolt_template)
        .build()
}

/// Return a `CompiledModule` containing a "Rigid" trait and a "Plate" structure.
///
/// The "Rigid" trait requires a `thickness: Scalar(LENGTH)` parameter.
/// The "Plate" template has a single `thickness` parameter and satisfies the trait.
pub fn trait_structure_module() -> CompiledModule {
    use reify_ir::CompiledExpr;

    let rigid_trait = CompiledTraitBuilder::new("Rigid")
        .require_param("thickness", Type::length())
        .build();

    let mm_literal = |v: f64| CompiledExpr::literal(crate::mm(v), Type::length());

    let plate_template = TopologyTemplateBuilder::new("Plate")
        .param("Plate", "thickness", Type::length(), Some(mm_literal(5.0)))
        .build();

    CompiledModuleBuilder::new(ModulePath::single("trait_structure"))
        .trait_def(rigid_trait)
        .template(plate_template)
        .build()
}

/// Return a `CompiledModule` containing an analytical field "temp" and a template.
///
/// The "temp" field maps `Geometry -> Real` with an analytical source expression.
/// The module also includes a "TempModel" structure template.
pub fn field_module() -> CompiledModule {
    use reify_ir::CompiledExpr;

    // A simple constant analytical body: f(x) = 273.15 (temperature in Kelvin)
    let body = CompiledExpr::literal(Value::Real(273.15), Type::Real);
    let temp_field = CompiledFieldBuilder::new("temp", Type::Geometry, Type::Real)
        .analytical(body)
        .build();

    let mm_literal = |v: f64| CompiledExpr::literal(crate::mm(v), Type::length());
    let model_template = TopologyTemplateBuilder::new("TempModel")
        .param("TempModel", "size", Type::length(), Some(mm_literal(100.0)))
        .build();

    CompiledModuleBuilder::new(ModulePath::single("field_module"))
        .field(temp_field)
        .template(model_template)
        .build()
}

/// Return a `CompiledModule` containing a "mfg_ready" purpose and a template.
///
/// The "mfg_ready" purpose has a single "subject" param (entity_kind "Structure")
/// and a thickness constraint. The module also includes a "Part" structure template.
pub fn purpose_module() -> CompiledModule {
    use reify_ir::CompiledExpr;

    let mm_literal = |v: f64| CompiledExpr::literal(crate::mm(v), Type::length());

    // constraint: subject.thickness > 2mm
    let thickness_ref =
        CompiledExpr::value_ref(crate::vcid("subject", "thickness"), Type::length());
    let min_thickness = mm_literal(2.0);
    let constraint_expr = CompiledExpr::binop(BinOp::Gt, thickness_ref, min_thickness, Type::Bool);

    let purpose = CompiledPurposeBuilder::new("mfg_ready")
        .param("subject", "Structure")
        .constraint("subject", 0, Some("thick_enough"), constraint_expr)
        .build();

    let part_template = TopologyTemplateBuilder::new("Part")
        .param("Part", "thickness", Type::length(), Some(mm_literal(5.0)))
        .build();

    CompiledModuleBuilder::new(ModulePath::single("purpose_module"))
        .compiled_purpose(purpose)
        .template(part_template)
        .build()
}

/// Return a `CompiledModule` with a single constrained "Beam" structure.
///
/// The Beam has two parameters and five constraints:
/// - `param width: Scalar(LENGTH) = 100mm`
/// - `param height: Scalar(LENGTH) = 200mm`
/// - constraint 0: `width > 10mm`   (from `range_constraint`)
/// - constraint 1: `width < 500mm`  (from `range_constraint`)
/// - constraint 2: `height > 10mm`  (from `range_constraint`)
/// - constraint 3: `height < 1000mm` (from `range_constraint`)
/// - constraint 4 (label "slender"): `height > 2 * width`
///
/// This fixture proves that `range_constraint` and `equality_constraint` work
/// correctly when called multiple times for the same entity.
pub fn constrained_structure_module() -> reify_compiler::CompiledModule {
    use reify_ir::CompiledExpr;

    let entity = "Beam";
    let mm_literal = |v: f64| CompiledExpr::literal(crate::mm(v), Type::length());

    // Build the 4 range expressions using the helper (2 per member)
    let width_range = range_constraint(
        entity,
        "width",
        Type::length(),
        mm_literal(10.0),
        mm_literal(500.0),
    );
    let height_range = range_constraint(
        entity,
        "height",
        Type::length(),
        mm_literal(10.0),
        mm_literal(1000.0),
    );

    // Slender ratio: height > 2 * width
    let height_ref = CompiledExpr::value_ref(crate::vcid(entity, "height"), Type::length());
    let width_ref = CompiledExpr::value_ref(crate::vcid(entity, "width"), Type::length());
    let two_times_width = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(Value::Real(2.0), Type::Real),
        width_ref,
        Type::length(),
    );
    use crate::builders::gt;
    let slender_expr = gt(height_ref, two_times_width);

    let template = TopologyTemplateBuilder::new(entity)
        .param(entity, "width", Type::length(), Some(mm_literal(100.0)))
        .param(entity, "height", Type::length(), Some(mm_literal(200.0)))
        // width range: indices 0, 1
        .constraint(entity, 0, None, width_range[0].clone())
        .constraint(entity, 1, None, width_range[1].clone())
        // height range: indices 2, 3
        .constraint(entity, 2, None, height_range[0].clone())
        .constraint(entity, 3, None, height_range[1].clone())
        // slender ratio: index 4, labeled
        .constraint(entity, 4, Some("slender"), slender_expr)
        .build();

    CompiledModuleBuilder::new(ModulePath::single("constrained_beam"))
        .template(template)
        .build()
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
    use reify_ir::CompiledExpr;

    let child_entity = "Child";
    let parent_entity = "Parent";

    // mm literal helper
    let mm_literal = |v: f64| CompiledExpr::literal(crate::mm(v), Type::length());

    // Child template: param height = 10mm, let half_h = height / 2
    let height_ref =
        || CompiledExpr::value_ref(crate::vcid(child_entity, "height"), Type::length());

    let half_h_expr = CompiledExpr::binop(
        BinOp::Div,
        height_ref(),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::length(),
    );

    let child_template = TopologyTemplateBuilder::new(child_entity)
        .param(
            child_entity,
            "height",
            Type::length(),
            Some(mm_literal(10.0)),
        )
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
        .param(
            parent_entity,
            "width",
            Type::length(),
            Some(mm_literal(80.0)),
        )
        .sub_component("rib", "Child", vec![("height".to_string(), arg_expr)])
        .build();

    CompiledModuleBuilder::new(ModulePath::single("parent_child"))
        .template(child_template)
        .template(parent_template)
        .build()
}

/// Create a `CompiledModule` with a self-referencing `TreeNode` structure.
///
/// Structure `TreeNode`:
///   - `param value: Int = 0`
///   - `sub left = TreeNode` (recursive left child, no args)
///   - `sub right = TreeNode` (recursive right child, no args)
///
/// Both sub-components reference `"TreeNode"` as their `structure_name`, creating
/// a self-referencing topology used to test cycle detection and recursive evaluation.
pub fn recursive_tree_module() -> CompiledModule {
    let e = "TreeNode";

    let tree_template = TopologyTemplateBuilder::new(e)
        .param(
            e,
            "value",
            Type::Int,
            Some(crate::builders::literal(Value::Int(0))),
        )
        .sub_component("left", "TreeNode", vec![])
        .sub_component("right", "TreeNode", vec![])
        .build();

    CompiledModuleBuilder::new(ModulePath::single("recursive_tree"))
        .template(tree_template)
        .build()
}

/// Create a `CompiledModule` with two mutually recursive structures.
///
/// Structures:
///   - `NodeA`: `param a_val: Int = 0`, `sub child = NodeB`
///   - `NodeB`: `param b_val: Int = 0`, `sub ref_back = NodeA`
///
/// The two structures form a mutual recursion cycle (NodeA → NodeB → NodeA),
/// used to test cycle detection algorithms in the evaluation engine.
pub fn mutual_recursion_module() -> CompiledModule {
    let node_a = TopologyTemplateBuilder::new("NodeA")
        .param(
            "NodeA",
            "a_val",
            Type::Int,
            Some(crate::builders::literal(Value::Int(0))),
        )
        .sub_component("child", "NodeB", vec![])
        .build();

    let node_b = TopologyTemplateBuilder::new("NodeB")
        .param(
            "NodeB",
            "b_val",
            Type::Int,
            Some(crate::builders::literal(Value::Int(0))),
        )
        .sub_component("ref_back", "NodeA", vec![])
        .build();

    CompiledModuleBuilder::new(ModulePath::single("mutual_recursion"))
        .template(node_a)
        .template(node_b)
        .build()
}

/// Return a `CompiledModule` covering all four annotation-capable entity kinds.
///
/// - Trait `"Rigid"` with `@deprecated("use Rigid2")` annotation
/// - Template `"Bolt"` with `@test` and `@optimized` annotations (no args)
/// - Field `"temp"` (Geometry → Real, imported) with `@deprecated` annotation
/// - Purpose `"mfg_ready"` with `@solver_hint` annotation
pub fn annotated_module() -> CompiledModule {
    let rigid_trait = CompiledTraitBuilder::new("Rigid")
        .annotation(annotation_with_args(
            DEPRECATED_ANNOTATION,
            vec![ann_str("use Rigid2")],
        ))
        .build();

    let bolt_template = TopologyTemplateBuilder::new("Bolt")
        .annotation(annotation(TEST_ANNOTATION))
        .annotation(annotation(OPTIMIZED_ANNOTATION))
        .build();

    let temp_field = CompiledFieldBuilder::new("temp", Type::Geometry, Type::Real)
        .imported()
        .annotation(annotation(DEPRECATED_ANNOTATION))
        .build();

    let purpose = CompiledPurposeBuilder::new("mfg_ready")
        .annotation(annotation(SOLVER_HINT_ANNOTATION))
        .build();

    CompiledModuleBuilder::new(ModulePath::single("annotated_module"))
        .trait_def(rigid_trait)
        .template(bolt_template)
        .field(temp_field)
        .compiled_purpose(purpose)
        .build()
}

// ─── shared stdlib test constants ──────────────────────────────────────

/// Material traits defined in `materials_mechanical.ri`. The base contract
/// trait is named `MaterialSpec`; the name `Material` is reserved for the
/// first-class canonical struct defined alongside these traits.
pub const EXPECTED_MATERIAL_TRAITS: &[&str] = &[
    "MaterialSpec",
    "Elastic",
    "Strong",
    "Hard",
    "FatigueRated",
    "FractureTough",
    "Ductile",
    "ImpactResistant",
    "Damping",
];

/// Geometry conformance marker traits defined in `geometry_traits.ri`. These
/// are pure markers — no fields, no constraints, no defaults — declared at
/// module path `std.geometry.traits` and intended to be inferred from kernel
/// results and attached as metadata.
pub const EXPECTED_GEOMETRY_TRAITS: &[&str] = &[
    "Bounded",
    "Closed",
    "Manifold",
    "Orientable",
    "Convex",
    "Connected",
    "Watertight",
];

/// Steel:Elastic conformance source — 3 params for the Elastic trait.
pub fn steel_elastic_source() -> &'static str {
    r#"
structure def Steel : Elastic {
    param youngs_modulus : Real = 200.0
    param poissons_ratio : Real = 0.3
    param shear_modulus : Real = 77.0
}
"#
}

/// Steel:Strong conformance source — 3 params for the Strong trait.
pub fn steel_strong_source() -> &'static str {
    r#"
structure def Steel : Strong {
    param yield_strength : Real = 250.0
    param uts : Real = 400.0
    param compressive_strength : Real = 250.0
}
"#
}

/// Steel:MaterialSpec+Elastic conformance source — 5 params for both traits.
pub fn steel_material_elastic_source() -> &'static str {
    r#"
structure def Steel : MaterialSpec + Elastic {
    param density : Real = 7800.0
    param name : String = "A36"
    param youngs_modulus : Real = 200.0
    param poissons_ratio : Real = 0.3
    param shear_modulus : Real = 77.0
}
"#
}

// ────────────────────────────────────────────────────────────────────────────
// Wave2 guard-flip fixture
// ────────────────────────────────────────────────────────────────────────────

/// Shared scenario for the wave2 guard-flip regression tests.
///
/// Both `edit_param_wave2_does_not_corrupt_inactive_members` (guard_eval.rs)
/// and `edit_source_wave2_does_not_corrupt_inactive_members` (edit_source.rs)
/// build the same `structure S`:
///
/// ```text
/// structure S {
///     param x: Length = 10mm          // (or 3mm in module_edited)
///     auto depth: Length              // resolved by solver
///     constraint depth >= x           // [label: "depth_ge_x"] dirty when x changes
///     where x > 5mm {                 // guard depends on x
///         let m = depth               // m reads the auto param
///     }
/// }
/// ```
///
/// Cell-ID layout:
/// - `x_id    = ValueCellId::new("S", "x")`
/// - `depth_id = ValueCellId::new("S", "depth")`
/// - `guard_id = ValueCellId::new("S", "__guard_0")`
/// - `m_id    = ValueCellId::new("S", "m")`
///
/// Solver sequence:
/// - 1st call → `depth = 10mm`  (initial `eval()` with guard=true)
/// - 2nd call → `depth = 3mm`   (post-edit with guard=false)
///
/// The "wave2 corrupts inactive member" bug surfaces when the post-flip wave2
/// re-evaluation rewrites `m` to 3mm even though the guard is now false; the
/// post-wave2 cleanup must re-deactivate `m` to `Value::Undef`.
pub struct Wave2FlipFixture {
    /// Module with `x = 10mm` — used by both tests for the initial `eval()`.
    pub module_initial: CompiledModule,
    /// Module with `x = 3mm` — used by `edit_source` as the new source;
    /// ignored (but available) by `edit_param`.
    pub module_edited: CompiledModule,
    /// Cell ID for the param `x: Length`.
    pub x_id: ValueCellId,
    /// Cell ID for the auto param `depth: Length`.
    pub depth_id: ValueCellId,
    /// Cell ID for the guard sentinel `__guard_0`.
    pub guard_id: ValueCellId,
    /// Cell ID for the let-binding `m: Length`.
    pub m_id: ValueCellId,
    /// Sequenced solver: 1st call → depth=10mm; 2nd call → depth=3mm.
    pub solver: Box<dyn ConstraintSolver>,
}

/// Build the shared wave2 guard-flip fixture.
///
/// See [`Wave2FlipFixture`] for the full scenario description.
pub fn wave2_flip_fixture() -> Wave2FlipFixture {
    use std::collections::HashMap;

    use crate::builders::{ge, gt, literal, value_ref};
    use crate::mocks::SequencedMockConstraintSolver;

    let x_id = ValueCellId::new("S", "x");
    let depth_id = ValueCellId::new("S", "depth");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let m_id = ValueCellId::new("S", "m");

    // `let m = depth` — non-auto Let cell that reads the auto param `depth`.
    // When the guard is false, m must be Value::Undef.
    // Wave2 will try to overwrite it; the post-wave2 cleanup must re-deactivate it.
    let m_decl = ValueCellDecl {
        id: m_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        cell_type: Type::length(),
        default_expr: Some(value_ref("S", "depth")),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    // Build a TopologyTemplate for structure S with the given x default (mm).
    let build_template = |x_default_mm: f64| {
        // Guard expression: `x > 5mm`.  Reads `x`, so guard cell is in dirty_cone(x).
        let guard_expr = gt(value_ref("S", "x"), literal(crate::mm(5.0)));
        TopologyTemplateBuilder::new("S")
            // x: structure_controlling (guard depends on x) AND read by the constraint
            .param(
                "S",
                "x",
                Type::length(),
                Some(literal(crate::mm(x_default_mm))),
            )
            // depth: auto param resolved by the solver
            .auto_param("S", "depth", Type::length())
            // constraint reads both depth and x → dirty when x changes → solver re-runs
            .constraint(
                "S",
                0,
                Some("depth_ge_x"),
                ge(value_ref("S", "depth"), value_ref("S", "x")),
            )
            // guarded group: guard depends on x; member m reads depth (auto param)
            .guarded_group(
                guard_expr,
                guard_id.clone(),
                vec![m_decl.clone()], // members (active when guard = true)
                vec![],               // constraints
                vec![],               // else_members
                vec![],               // else_constraints
            )
            .build()
    };

    let module_initial = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(build_template(10.0))
        .build();
    let module_edited = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(build_template(3.0))
        .build();

    // Sequenced solver: 1st call → depth=10mm (initial eval);
    //                   2nd call → depth=3mm  (post-edit).
    let mut solved1 = HashMap::new();
    solved1.insert(depth_id.clone(), crate::mm(10.0));
    let mut solved2 = HashMap::new();
    solved2.insert(depth_id.clone(), crate::mm(3.0));
    let solver = Box::new(SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
    ])) as Box<dyn ConstraintSolver>;

    Wave2FlipFixture {
        module_initial,
        module_edited,
        x_id,
        depth_id,
        guard_id,
        m_id,
        solver,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_compiler::{ValueCellKind, find_template};
    use reify_core::Severity;

    #[test]
    fn bracket_parsed_module_structure() {
        let module = bracket_parsed_module();
        assert_eq!(module.declarations.len(), 1);
        match &module.declarations[0] {
            reify_ast::Declaration::Structure(s) => {
                assert_eq!(s.name, "Bracket");
                // 5 params + 2 lets + 3 constraints = 10 members
                assert_eq!(s.members.len(), 10);
                let params: Vec<_> = s
                    .members
                    .iter()
                    .filter(|m| matches!(m, reify_ast::MemberDecl::Param(_)))
                    .collect();
                let lets: Vec<_> = s
                    .members
                    .iter()
                    .filter(|m| matches!(m, reify_ast::MemberDecl::Let(_)))
                    .collect();
                let constraints: Vec<_> = s
                    .members
                    .iter()
                    .filter(|m| matches!(m, reify_ast::MemberDecl::Constraint(_)))
                    .collect();
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
        assert!(
            !source.contains("param thickness: Scalar = 5mm"),
            "original 5mm should be replaced"
        );
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

    // step-15: failing test for constrained_structure_module fixture
    #[test]
    fn multi_constraint_fixture_structure() {
        let module = constrained_structure_module();
        assert_eq!(module.templates.len(), 1);
        let beam = &module.templates[0];
        assert_eq!(beam.name, "Beam");
        // Beam has width and height params
        let width = beam.value_cells.iter().find(|vc| vc.id.member == "width");
        let height = beam.value_cells.iter().find(|vc| vc.id.member == "height");
        assert!(width.is_some(), "Beam should have param width");
        assert!(height.is_some(), "Beam should have param height");
        // At least 4 constraints: range on width (2) + range on height (2)
        assert!(
            beam.constraints.len() >= 4,
            "expected at least 4 constraints, got {}",
            beam.constraints.len()
        );
        // The ratio constraint should have label "slender"
        let slender = beam
            .constraints
            .iter()
            .find(|c| c.label.as_deref() == Some("slender"));
        assert!(slender.is_some(), "expected labeled constraint 'slender'");
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
        let bolt = find_template(&module.templates, "Bolt");
        let crate_t = find_template(&module.templates, "Crate");
        let bolt = bolt.expect("should have Bolt template");
        // Bolt conforms to Rigid
        assert!(
            bolt.trait_bounds.contains(&"Rigid".to_string()),
            "Bolt should have Rigid trait bound"
        );
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

    // --- Annotated entity fixture tests (steps 31-32) ---

    #[test]
    fn annotated_module_has_annotated_entities() {
        let module = annotated_module();

        // (a) one trait with @deprecated("use Rigid2") annotation
        assert_eq!(module.trait_defs.len(), 1);
        let rigid = &module.trait_defs[0];
        assert_eq!(rigid.name, "Rigid");
        assert_eq!(rigid.annotations.len(), 1);
        assert_eq!(rigid.annotations[0].name, DEPRECATED_ANNOTATION);
        assert_eq!(rigid.annotations[0].args.len(), 1);
        assert!(matches!(
            &rigid.annotations[0].args[0],
            reify_ir::AnnotationArg {
                value: reify_ir::AnnotationArgValue::String(s),
                ..
            } if s == "use Rigid2"
        ));

        // (b) one template with @test and @optimized annotations (no args)
        assert_eq!(module.templates.len(), 1);
        let bolt = &module.templates[0];
        assert_eq!(bolt.name, "Bolt");
        assert_eq!(bolt.annotations.len(), 2);
        let ann_names: Vec<&str> = bolt.annotations.iter().map(|a| a.name.as_str()).collect();
        assert!(
            ann_names.contains(&TEST_ANNOTATION),
            "expected @test annotation"
        );
        assert!(
            ann_names.contains(&OPTIMIZED_ANNOTATION),
            "expected @optimized annotation"
        );

        // (c) one field with @deprecated annotation
        assert_eq!(module.fields.len(), 1);
        let temp_field = &module.fields[0];
        assert_eq!(temp_field.name, "temp");
        assert_eq!(temp_field.annotations.len(), 1);
        assert_eq!(temp_field.annotations[0].name, DEPRECATED_ANNOTATION);

        // (d) one purpose with @solver_hint annotation
        assert_eq!(module.compiled_purposes.len(), 1);
        let purpose = &module.compiled_purposes[0];
        assert_eq!(purpose.name, "mfg_ready");
        assert_eq!(purpose.annotations.len(), 1);
        assert_eq!(purpose.annotations[0].name, SOLVER_HINT_ANNOTATION);
    }

    #[test]
    fn trait_structure_module_has_trait_and_template() {
        let module = trait_structure_module();
        assert_eq!(module.trait_defs.len(), 1);
        assert_eq!(module.trait_defs[0].name, "Rigid");
        assert_eq!(module.trait_defs[0].required_members.len(), 1);
        assert_eq!(module.trait_defs[0].required_members[0].name, "thickness");
        assert_eq!(module.templates.len(), 1);
        assert_eq!(module.templates[0].name, "Plate");
        assert!(module.diagnostics.is_empty());
    }

    #[test]
    fn field_module_has_field_and_template() {
        let module = field_module();
        assert_eq!(module.fields.len(), 1);
        assert_eq!(module.fields[0].name, "temp");
        assert_eq!(module.templates.len(), 1);
    }

    #[test]
    fn purpose_module_has_purpose_and_template() {
        let module = purpose_module();
        assert_eq!(module.compiled_purposes.len(), 1);
        assert_eq!(module.compiled_purposes[0].name, "mfg_ready");
        assert_eq!(module.compiled_purposes[0].params.len(), 1);
        assert_eq!(module.compiled_purposes[0].params[0].name, "subject");
        assert_eq!(module.templates.len(), 1);
    }

    #[test]
    fn constrained_structure_module_constraint_indices_are_sequential() {
        use reify_core::ConstraintNodeId;
        let module = constrained_structure_module();
        let t = &module.templates[0];
        assert_eq!(t.name, "Beam");
        assert_eq!(t.constraints.len(), 5);
        // All constraints are for entity "Beam" with sequential indices 0-4
        for (i, c) in t.constraints.iter().enumerate() {
            assert_eq!(
                c.id,
                ConstraintNodeId::new("Beam", i as u32),
                "constraint {} should have index {}",
                i,
                i
            );
        }
        // Index 4 has label "slender"
        assert_eq!(t.constraints[4].label.as_deref(), Some("slender"));
        // First 4 have no labels
        for i in 0..4 {
            assert_eq!(
                t.constraints[i].label, None,
                "constraint {} should have no label",
                i
            );
        }
    }

    #[test]
    fn constrained_structure_module_has_beam_template() {
        let module = constrained_structure_module();
        assert_eq!(module.templates.len(), 1);
        let t = &module.templates[0];
        assert_eq!(t.name, "Beam");
        // 2 params: width, height
        assert_eq!(t.value_cells.len(), 2);
        // 5 constraints: 2 for width range, 2 for height range, 1 for slender ratio
        assert_eq!(t.constraints.len(), 5);
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

    // step-21: failing test for recursive_tree_module fixture
    #[test]
    fn recursive_tree_module_structure() {
        let module = recursive_tree_module();
        assert_eq!(module.templates.len(), 1);
        let tree = &module.templates[0];
        assert_eq!(tree.name, "TreeNode");
        // Has param value: Int
        let value_cell = tree.value_cells.iter().find(|vc| vc.id.member == "value");
        assert!(value_cell.is_some(), "TreeNode should have param value");
        // Has two sub-components: left and right, both referencing TreeNode
        assert_eq!(
            tree.sub_components.len(),
            2,
            "TreeNode should have 2 sub-components"
        );
        let left = tree.sub_components.iter().find(|sc| sc.name == "left");
        let right = tree.sub_components.iter().find(|sc| sc.name == "right");
        assert!(left.is_some(), "TreeNode should have sub left");
        assert!(right.is_some(), "TreeNode should have sub right");
        assert_eq!(
            left.unwrap().structure_name,
            "TreeNode",
            "left should reference TreeNode"
        );
        assert_eq!(
            right.unwrap().structure_name,
            "TreeNode",
            "right should reference TreeNode"
        );
    }

    // step-23: failing test for mutual_recursion_module fixture
    #[test]
    fn mutual_recursion_module_structure() {
        let module = mutual_recursion_module();
        assert_eq!(module.templates.len(), 2);
        let node_a = find_template(&module.templates, "NodeA");
        let node_b = find_template(&module.templates, "NodeB");
        assert!(node_a.is_some(), "should have NodeA template");
        assert!(node_b.is_some(), "should have NodeB template");
        let node_a = node_a.unwrap();
        let node_b = node_b.unwrap();
        // NodeA has param a_val: Int and sub child = NodeB
        let a_val = node_a.value_cells.iter().find(|vc| vc.id.member == "a_val");
        assert!(a_val.is_some(), "NodeA should have param a_val");
        assert_eq!(node_a.sub_components.len(), 1);
        assert_eq!(node_a.sub_components[0].name, "child");
        assert_eq!(node_a.sub_components[0].structure_name, "NodeB");
        // NodeB has param b_val: Int and sub ref_back = NodeA
        let b_val = node_b.value_cells.iter().find(|vc| vc.id.member == "b_val");
        assert!(b_val.is_some(), "NodeB should have param b_val");
        assert_eq!(node_b.sub_components.len(), 1);
        assert_eq!(node_b.sub_components[0].name, "ref_back");
        assert_eq!(node_b.sub_components[0].structure_name, "NodeA");
    }

    /// Helper: parse and compile `source`, assert no errors and exactly one
    /// `Severity::Warning` mentioning both "unknown port type" and
    /// "NonExistentTrait". Returns the `CompiledModule` for further assertions.
    fn assert_warning_source_compiles_with_unknown_port_warning(source: &str) -> CompiledModule {
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = reify_compiler::compile(&parsed);

        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

        let warnings: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Severity::Warning
                    && d.message.contains("unknown port type")
                    && d.message.contains("NonExistentTrait")
            })
            .collect();
        assert_eq!(
            warnings.len(),
            1,
            "expected exactly 1 unknown-port-type warning mentioning NonExistentTrait, got: {:?}",
            compiled.diagnostics
        );

        compiled
    }

    #[test]
    fn warn_source_with_unknown_port_type_produces_unknown_port_warning_no_errors() {
        assert_warning_source_compiles_with_unknown_port_warning(
            warn_source_with_unknown_port_type(),
        );
    }

    #[test]
    fn warn_source_with_unknown_port_type_is_well_formed() {
        let src = warn_source_with_unknown_port_type();
        assert!(src.contains("structure def S"), "missing 'structure def S'");
        assert!(
            src.contains("port mount : NonExistentTrait"),
            "missing 'port mount : NonExistentTrait'"
        );
        assert!(
            src.contains("param d : Length = 5mm"),
            "missing 'param d : Length = 5mm'"
        );
    }

    #[test]
    fn warn_source_with_unknown_port_type_with_width_param_cell_has_length_type_span_kind_and_default()
     {
        let source = warn_source_with_unknown_port_type_with_width();
        let compiled = assert_warning_source_compiles_with_unknown_port_warning(source);
        let s_template = find_template(&compiled.templates, "S")
            .expect("expected S template in compiled module");
        let width_cell = s_template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == "width")
            .unwrap_or_else(|| {
                panic!(
                    "expected S template to have a value_cell with member 'width', got: {:?}",
                    s_template
                        .value_cells
                        .iter()
                        .map(|vc| &vc.id.member)
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(
            width_cell.cell_type,
            Type::length(),
            "expected width cell to be Length-typed (Scalar{{dimension=LENGTH}}), got: {:?}",
            width_cell.cell_type
        );
        let (start, end) = (width_cell.span.start as usize, width_cell.span.end as usize);
        let span_text = source.get(start..end).unwrap_or_else(|| {
            panic!(
                "width_cell.span {:?} out of bounds for source of len {}",
                width_cell.span,
                source.len()
            )
        });
        assert_eq!(
            span_text, "param width : Length = 80mm",
            "expected width cell span to cover the full `param width : Length = 80mm` \
             declaration, got span {:?} covering {:?}",
            width_cell.span, span_text,
        );
        assert!(
            matches!(width_cell.kind, ValueCellKind::Param),
            "expected width cell to be ValueCellKind::Param \
             (from `param width : Length = 80mm`), got: {:?}",
            width_cell.kind,
        );
        assert!(
            width_cell.default_expr.is_some(),
            "expected width cell to have a default expression (from `= 80mm`), \
             got default_expr=None",
        );
    }

    #[test]
    fn warn_source_with_unknown_port_type_with_width_is_well_formed() {
        let src = warn_source_with_unknown_port_type_with_width();
        assert!(src.contains("structure def S"), "missing 'structure def S'");
        assert!(
            src.contains("param width : Length = 80mm"),
            "missing 'param width : Length = 80mm'"
        );
        assert!(
            src.contains("port mount : NonExistentTrait"),
            "missing 'port mount : NonExistentTrait'"
        );
    }

    #[test]
    fn wave2_flip_fixture_smoke() {
        let fixture = wave2_flip_fixture();
        assert_eq!(fixture.module_initial.templates.len(), 1);
    }
}
