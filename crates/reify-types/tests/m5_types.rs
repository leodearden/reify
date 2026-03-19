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
