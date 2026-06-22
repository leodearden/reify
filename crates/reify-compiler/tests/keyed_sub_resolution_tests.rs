//! Keyed<T> sub-member resolution + per-key override IR tests (task 3931 γ).
//!
//! Step-3 (IR lowering) asserts `SubComponentDecl.keyed_member_overrides` carries
//! each key's compiled `(name, value)` overrides. RED until step-4 adds the field.
//! Step-5 (resolution) and step-9/step-11 (diagnostics) extend this file.
//!
//! User-observable signal:
//!   cargo test -p reify-compiler --test keyed_sub_resolution_tests

use reify_ir::{CompiledExprKind, MemberKey, Value};
use reify_test_support::compile_source;

/// The compiled `SubComponentDecl` for a keyed sub must carry per-key param
/// overrides on `keyed_member_overrides`: `"intake" => { area = 5mm }` lowers to
/// an entry keyed by `MemberKey("intake")` with a single `("area", 5mm)` override.
///
/// RED today: the `keyed_member_overrides` field does not exist (compile error
/// IS the RED signal) until step-4.
#[test]
fn keyed_sub_lowers_per_key_overrides_to_ir() {
    let source = r#"
structure def Vent {
    param area : Length = 1mm
}
structure def Manifold {
    sub vents : Keyed<Vent> {
        "intake" => { area = 5mm }
    }
}
"#;
    let module = compile_source(source);
    let manifold = module
        .templates
        .iter()
        .find(|t| t.name == "Manifold")
        .expect("Manifold template should compile");
    let vents = manifold
        .sub_components
        .iter()
        .find(|s| s.name == "vents")
        .expect("vents sub-component should be present");

    assert_eq!(
        vents.keyed_member_overrides.len(),
        1,
        "expected exactly one keyed-member-override entry, got {:?}",
        vents.keyed_member_overrides,
    );
    let (key, overrides) = &vents.keyed_member_overrides[0];
    assert_eq!(
        key,
        &MemberKey::new("intake"),
        "keyed override entry must be keyed by MemberKey(\"intake\"), got {key:?}",
    );
    assert_eq!(
        overrides.len(),
        1,
        "intake overrides must carry exactly one (name, value), got {overrides:?}",
    );
    let (name, expr) = &overrides[0];
    assert_eq!(name, "area", "override name must be `area`, got {name:?}");
    match &expr.kind {
        CompiledExprKind::Literal(Value::Scalar { si_value, .. }) => {
            assert!(
                (*si_value - 0.005).abs() < 1e-12,
                "area override must compile to 5mm (si_value 0.005), got {si_value}",
            );
        }
        other => panic!("area override must be a scalar literal, got {other:?}"),
    }
}

/// The per-key override list stays in sync with `keyed_members` (same keep-first
/// dedupe, declaration order). A two-key block records both entries.
#[test]
fn keyed_member_overrides_parallel_keyed_members_order() {
    let source = r#"
structure def Vent {
    param area : Length = 1mm
}
structure def Manifold {
    sub vents : Keyed<Vent> {
        "intake" => { area = 5mm }
        "exhaust" => { area = 8mm }
    }
}
"#;
    let module = compile_source(source);
    let manifold = module
        .templates
        .iter()
        .find(|t| t.name == "Manifold")
        .expect("Manifold template should compile");
    let vents = manifold
        .sub_components
        .iter()
        .find(|s| s.name == "vents")
        .expect("vents sub-component should be present");

    let keys: Vec<&MemberKey> = vents
        .keyed_member_overrides
        .iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(
        keys,
        vec![&MemberKey::new("intake"), &MemberKey::new("exhaust")],
        "keyed_member_overrides must mirror keyed_members keys in declaration order",
    );
}
