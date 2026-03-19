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
