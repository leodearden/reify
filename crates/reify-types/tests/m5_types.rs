//! Integration tests for M5 type definitions.

#[test]
fn enum_def_construction_and_lookup() {
    let def = reify_types::EnumDef {
        name: "Color".into(),
        variants: vec!["Red".into(), "Green".into(), "Blue".into()],
    };
    assert_eq!(def.name, "Color");
    assert_eq!(def.variants.len(), 3);
    assert!(def.contains_variant("Red"));
    assert!(def.contains_variant("Blue"));
    assert!(!def.contains_variant("Yellow"));
}

#[test]
fn enum_def_debug_clone_eq() {
    let def1 = reify_types::EnumDef {
        name: "Shape".into(),
        variants: vec!["Circle".into(), "Square".into()],
    };
    let def2 = def1.clone();
    assert_eq!(def1, def2);
    let _ = format!("{:?}", def1); // Debug works
}

// --- PortDirection tests (step-19) ---

#[test]
fn port_direction_variants() {
    let _in = reify_types::PortDirection::In;
    let _out = reify_types::PortDirection::Out;
    let _bidi = reify_types::PortDirection::Bidi;
}

#[test]
fn port_direction_debug_clone_eq_copy_hash() {
    let d = reify_types::PortDirection::In;
    let d2 = d; // Copy
    assert_eq!(d, d2);
    let d3 = d.clone();
    assert_eq!(d, d3);
    assert_ne!(d, reify_types::PortDirection::Out);
    let _ = format!("{:?}", d);

    // Hash: usable as HashMap key
    use std::collections::HashMap;
    let mut map = HashMap::new();
    map.insert(d, "in");
    assert_eq!(map.get(&reify_types::PortDirection::In), Some(&"in"));
}

// --- TraitRef and TraitBound tests (step-21) ---

#[test]
fn trait_ref_construction() {
    let tr = reify_types::TraitRef {
        name: "Connectable".into(),
        type_args: vec![reify_types::Type::Int, reify_types::Type::String],
    };
    assert_eq!(tr.name, "Connectable");
    assert_eq!(tr.type_args.len(), 2);
}

#[test]
fn trait_ref_equality() {
    let a = reify_types::TraitRef {
        name: "Foo".into(),
        type_args: vec![],
    };
    let b = a.clone();
    assert_eq!(a, b);
    let c = reify_types::TraitRef {
        name: "Bar".into(),
        type_args: vec![],
    };
    assert_ne!(a, c);
}

#[test]
fn trait_bound_wraps_trait_ref() {
    let tr = reify_types::TraitRef {
        name: "Measurable".into(),
        type_args: vec![reify_types::Type::Real],
    };
    let bound = reify_types::TraitBound {
        trait_ref: tr.clone(),
    };
    assert_eq!(bound.trait_ref, tr);
    let bound2 = bound.clone();
    assert_eq!(bound, bound2);
    let _ = format!("{:?}", bound);
}

// --- TypeParam tests (step-23) ---

#[test]
fn type_param_construction() {
    let tp = reify_types::TypeParam {
        name: "T".into(),
        bounds: vec![reify_types::TraitBound {
            trait_ref: reify_types::TraitRef {
                name: "Measurable".into(),
                type_args: vec![],
            },
        }],
        default: Some(reify_types::Type::Real),
    };
    assert_eq!(tp.name, "T");
    assert_eq!(tp.bounds.len(), 1);
    assert_eq!(tp.default, Some(reify_types::Type::Real));
}

#[test]
fn type_param_no_bounds_no_default() {
    let tp = reify_types::TypeParam {
        name: "U".into(),
        bounds: vec![],
        default: None,
    };
    assert_eq!(tp.name, "U");
    assert!(tp.bounds.is_empty());
    assert!(tp.default.is_none());
    let tp2 = tp.clone();
    assert_eq!(tp, tp2);
}

// --- TraitMember tests (step-25) ---

#[test]
fn trait_member_param() {
    let m = reify_types::TraitMember::Param {
        name: "width".into(),
        ty: reify_types::Type::length(),
        default: Some(reify_types::Value::length(0.08)),
    };
    if let reify_types::TraitMember::Param { name, ty, default } = &m {
        assert_eq!(name, "width");
        assert_eq!(*ty, reify_types::Type::length());
        assert!(default.is_some());
    } else {
        panic!("expected Param");
    }
}

#[test]
fn trait_member_port() {
    let m = reify_types::TraitMember::Port {
        name: "input".into(),
        ty: reify_types::Type::Real,
        direction: reify_types::PortDirection::In,
    };
    if let reify_types::TraitMember::Port { name, direction, .. } = &m {
        assert_eq!(name, "input");
        assert_eq!(*direction, reify_types::PortDirection::In);
    } else {
        panic!("expected Port");
    }
}

#[test]
fn trait_member_sub() {
    let m = reify_types::TraitMember::Sub {
        name: "child".into(),
        trait_ref: reify_types::TraitRef {
            name: "Component".into(),
            type_args: vec![],
        },
    };
    if let reify_types::TraitMember::Sub { name, trait_ref } = &m {
        assert_eq!(name, "child");
        assert_eq!(trait_ref.name, "Component");
    } else {
        panic!("expected Sub");
    }
}

#[test]
fn trait_member_let_and_constraint() {
    let _let = reify_types::TraitMember::Let {
        name: "area".into(),
        ty: reify_types::Type::Real,
        expr: "width * height".into(),
    };
    let _constraint = reify_types::TraitMember::Constraint {
        expr: "width > 0".into(),
    };
    let _ = format!("{:?}", _let);
    let _ = format!("{:?}", _constraint);
}

#[test]
fn trait_member_associated_type() {
    let m = reify_types::TraitMember::AssociatedType {
        name: "Output".into(),
        default: Some(reify_types::Type::Int),
    };
    if let reify_types::TraitMember::AssociatedType { name, default } = &m {
        assert_eq!(name, "Output");
        assert_eq!(*default, Some(reify_types::Type::Int));
    } else {
        panic!("expected AssociatedType");
    }
    let m2 = m.clone();
    assert_eq!(m, m2);
}

// --- TraitDef tests (step-27) ---

#[test]
fn trait_def_full_construction() {
    let def = reify_types::TraitDef {
        name: "Bracket".into(),
        type_params: vec![reify_types::TypeParam {
            name: "T".into(),
            bounds: vec![],
            default: None,
        }],
        refinements: vec!["Structural".into()],
        members: vec![
            reify_types::TraitMember::Param {
                name: "width".into(),
                ty: reify_types::Type::length(),
                default: None,
            },
            reify_types::TraitMember::Port {
                name: "top".into(),
                ty: reify_types::Type::Real,
                direction: reify_types::PortDirection::Out,
            },
            reify_types::TraitMember::Constraint {
                expr: "width > 0".into(),
            },
        ],
    };
    assert_eq!(def.name, "Bracket");
    assert_eq!(def.type_params.len(), 1);
    assert_eq!(def.refinements, vec!["Structural"]);
    assert_eq!(def.members.len(), 3);
    let def2 = def.clone();
    assert_eq!(def, def2);
    let _ = format!("{:?}", def);
}
