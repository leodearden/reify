//! Integration tests to verify types are exported from the crate root.

#[test]
fn all_resolution_types_exported() {
    // DeterminacyState::Auto variant
    let _auto = reify_types::DeterminacyState::Auto;

    // ResolutionNodeId
    let _rid = reify_types::ResolutionNodeId::new("Bracket", 0);

    // ConstraintDomain
    let _cd = reify_types::ConstraintDomain::Dimensional;

    // AutoParam
    let _ap = reify_types::AutoParam {
        id: reify_types::ValueCellId::new("Bracket", "width"),
        param_type: reify_types::Type::length(),
        bounds: None,
        free: false,
    };

    // OptimizationObjective
    let expr = reify_types::CompiledExpr {
        kind: reify_types::CompiledExprKind::Literal(reify_types::Value::Real(1.0)),
        result_type: reify_types::Type::Real,
        content_hash: reify_types::ContentHash::of(b"test"),
    };
    let _obj = reify_types::OptimizationObjective::Minimize(expr.clone());

    // ResolutionProblem
    let _rp = reify_types::ResolutionProblem {
        auto_params: vec![],
        constraints: vec![],
        current_values: reify_types::ValueMap::new(),
        objective: None,
        functions: Vec::<reify_types::CompiledFunction>::new().into(),
    };

    // SolveResult
    let _sr = reify_types::SolveResult::NoProgress {
        reason: "test".to_string(),
    };

    // ConstraintSolver trait — verify it exists as a trait object type
    fn _assert_trait_object(_: &dyn reify_types::ConstraintSolver) {}
}

#[test]
fn all_m5_types_exported() {
    // --- Value variants ---

    // Value::Enum
    let _ve = reify_types::Value::Enum {
        type_name: "Color".into(),
        variant: "Red".into(),
    };

    // Value::List
    let _vl = reify_types::Value::List(vec![reify_types::Value::Int(1)]);

    // Value::Set
    let _vs = reify_types::Value::Set(std::collections::BTreeSet::new());

    // Value::Map
    let _vm = reify_types::Value::Map(std::collections::BTreeMap::new());

    // Value::Option
    let _vo = reify_types::Value::Option(Some(Box::new(reify_types::Value::Int(1))));
    let _vn = reify_types::Value::Option(None);

    // --- Type variants ---

    // Type::Enum
    let _te = reify_types::Type::Enum("Color".into());

    // Type::List
    let _tl = reify_types::Type::List(Box::new(reify_types::Type::Int));

    // Type::Set
    let _ts = reify_types::Type::Set(Box::new(reify_types::Type::String));

    // Type::Map
    let _tm = reify_types::Type::Map(
        Box::new(reify_types::Type::String),
        Box::new(reify_types::Type::Real),
    );

    // Type::Option
    let _to = reify_types::Type::Option(Box::new(reify_types::Type::Int));

    // Type::Function
    let _tf = reify_types::Type::Function {
        params: vec![reify_types::Type::Int],
        return_type: Box::new(reify_types::Type::Real),
    };

    // --- Trait-related types ---

    // EnumDef
    let _ed = reify_types::EnumDef {
        name: "Shape".into(),
        variants: vec!["Circle".into()],
    };

    // PortDirection
    let _pd = reify_types::PortDirection::In;

    // TraitRef
    let _tr = reify_types::TraitRef {
        name: "Drawable".into(),
        type_args: vec![],
    };

    // TraitBound
    let _tb = reify_types::TraitBound {
        trait_ref: reify_types::TraitRef {
            name: "Measurable".into(),
            type_args: vec![],
        },
    };

    // TypeParam
    let _tp = reify_types::TypeParam {
        name: "T".into(),
        bounds: vec![],
        default: None,
    };

    // TraitMember
    let _tmem = reify_types::TraitMember::Param {
        name: "width".into(),
        ty: reify_types::Type::Real,
        default: None,
    };

    // TraitDef
    let _td = reify_types::TraitDef {
        name: "Component".into(),
        type_params: vec![],
        refinements: vec![],
        members: vec![],
    };

    // --- Point and Vector variants ---

    // Type::Point (direct construction)
    let _tp3 = reify_types::Type::Point {
        n: 3,
        quantity: Box::new(reify_types::Type::length()),
    };

    // Type::Vector (direct construction)
    let _tv3 = reify_types::Type::Vector {
        n: 3,
        quantity: Box::new(reify_types::Type::length()),
    };

    // Factory methods
    let _pp3 = reify_types::Type::point3(reify_types::Type::Real);
    let _pp2 = reify_types::Type::point2(reify_types::Type::Real);
    let _vv3 = reify_types::Type::vec3(reify_types::Type::Real);
    let _vv2 = reify_types::Type::vec2(reify_types::Type::Real);

    // --- Tensor variant ---

    // Type::Tensor (direct construction)
    let _tt2x3 = reify_types::Type::Tensor {
        rank: 2,
        n: 3,
        quantity: Box::new(reify_types::Type::length()),
    };

    // Type::tensor factory method
    let _tt_factory = reify_types::Type::tensor(1, 4, reify_types::Type::Real);

    // Value::Tensor construction
    let _vt =
        reify_types::Value::Tensor(vec![reify_types::Value::Int(1), reify_types::Value::Int(2)]);

    // --- Frame variant ---

    // Type::Frame (direct construction)
    let tf3 = reify_types::Type::Frame(3);

    // Type::frame factory method
    let tf3_factory = reify_types::Type::frame(3);
    assert_eq!(tf3, tf3_factory);

    // Value::Frame construction
    let _vf = reify_types::Value::Frame {
        origin: Box::new(reify_types::Value::Point(vec![
            reify_types::Value::length(0.0),
            reify_types::Value::length(0.0),
            reify_types::Value::length(0.0),
        ])),
        basis: Box::new(reify_types::Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
    };
}

#[test]
fn spanned_ident_exported() {
    let si = reify_types::SpannedIdent {
        name: "Rigid".into(),
        span: reify_types::SourceSpan::new(0, 5),
    };
    assert_eq!(si.name, "Rigid");
    assert_eq!(si.span, reify_types::SourceSpan::new(0, 5));
}

#[test]
fn node_traits_exported() {
    // NodeTraits::IMMEDIATE is accessible at the crate root
    let imm = reify_types::NodeTraits::IMMEDIATE;
    assert!(!imm.is_empty());

    // Compose via | operator
    let warm_committable =
        reify_types::NodeTraits::WARM_STARTABLE | reify_types::NodeTraits::COMMITTABLE;
    assert!(warm_committable.contains(reify_types::NodeTraits::WARM_STARTABLE));
    assert!(warm_committable.contains(reify_types::NodeTraits::COMMITTABLE));

    // ResolutionNode.default_traits() == WARM_STARTABLE | COMMITTABLE
    assert_eq!(
        reify_types::NodeArchKind::ResolutionNode.default_traits(),
        warm_committable
    );

    // ConstraintNode.default_traits().is_empty()
    assert!(reify_types::NodeArchKind::ConstraintNode
        .default_traits()
        .is_empty());
}

#[test]
fn presentation_info_types_exported() {
    // DiagnosticInfo is accessible at the reify_types crate root
    let _d = reify_types::DiagnosticInfo {
        file_path: "test.ri".into(),
        line: 1,
        column: 0,
        end_line: 1,
        end_column: 10,
        severity: "error".into(),
        message: "type mismatch".into(),
        code: None,
    };

    // SourceLocationInfo is accessible at the reify_types crate root
    let _loc = reify_types::SourceLocationInfo {
        file_path: "test.ri".into(),
        line: 1,
        column: 0,
        end_line: 1,
        end_column: 10,
    };
}
