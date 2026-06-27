//! Keyed<T> sub-member resolution + per-key override IR tests (task 3931 γ).
//!
//! Step-3 (IR lowering) asserts `SubComponentDecl.keyed_member_overrides` carries
//! each key's compiled `(name, value)` overrides. RED until step-4 adds the field.
//! Step-5 (resolution) and step-9/step-11 (diagnostics) extend this file.
//!
//! User-observable signal:
//!   cargo test -p reify-compiler --test keyed_sub_resolution_tests

use reify_core::{Severity, Type};
use reify_ir::{CompiledExprKind, MemberKey, Value};
use reify_test_support::{assert_has_diagnostic, compile_source, get_let_expr_in};

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

// ── step-5: keyed-access resolution (RED until step-6) ───────────────────────

/// `vents["intake"].area` must resolve to a `ValueRef` at the key-addressed
/// scoped cell `Manifold.vents["intake"]` with member `area`.
///
/// RED today: keyed subs are absent from `collection_sub_names`, so the access
/// falls through to the "cannot index into non-collection type" poison
/// (Type::Error) instead of this scoped ValueRef. Flips GREEN after step-6.
#[test]
fn keyed_member_access_resolves_to_scoped_value_ref() {
    let source = r#"
structure def Vent {
    param area : Length = 1mm
}
structure def Manifold {
    sub vents : Keyed<Vent> {
        "intake" => { area = 5mm }
    }
    let a = vents["intake"].area
}
"#;
    let module = compile_source(source);
    let expr = get_let_expr_in(&module, "Manifold", "a");
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(
                id.entity, "Manifold.vents[\"intake\"]",
                "keyed member access must scope to the key-addressed child entity",
            );
            assert_eq!(id.member, "area", "accessed member must be `area`");
        }
        other => panic!(
            "vents[\"intake\"].area must resolve to a scoped ValueRef, got {other:?} \
             (result_type {:?})",
            expr.result_type,
        ),
    }
    assert!(
        !expr.result_type.is_error(),
        "resolved keyed member access must not be poisoned (Type::Error)",
    );
}

/// Bare `vents["intake"]` must resolve to a `ValueRef` at the key-addressed
/// StructureInstance cell `Manifold.vents["intake"]` typed `StructureRef("Vent")`.
///
/// RED today: same non-collection poison fall-through as the member case.
#[test]
fn keyed_bare_access_resolves_to_structure_ref() {
    let source = r#"
structure def Vent {
    param area : Length = 1mm
}
structure def Manifold {
    sub vents : Keyed<Vent> {
        "intake" => { area = 5mm }
    }
    let m = vents["intake"]
}
"#;
    let module = compile_source(source);
    let expr = get_let_expr_in(&module, "Manifold", "m");
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(
                id.entity, "Manifold",
                "bare keyed access cell lives under the parent entity",
            );
            assert_eq!(
                id.member, "vents[\"intake\"]",
                "bare keyed access member must be the key-addressed path segment",
            );
        }
        other => panic!("vents[\"intake\"] must resolve to a scoped ValueRef, got {other:?}"),
    }
    assert_eq!(
        expr.result_type,
        Type::StructureRef("Vent".to_string()),
        "bare keyed access must be typed StructureRef(\"Vent\")",
    );
}

// ── step-9: missing-key access → named compile diagnostic + Undef ────────────

/// Accessing a key that is NOT in the keyed sub's author-assigned set
/// (`vents["ghost"]`, ghost ∉ {intake}) must emit a named, actionable compile
/// diagnostic naming both the missing key and the sub — not a generic
/// "cannot index into non-collection" poison. The access still lowers to an
/// `Undef`-resolving literal so eval proceeds without a panic (spec §3.4); the
/// eval-side no-panic behaviour is pinned in keyed_sub_eval.rs.
#[test]
fn missing_keyed_key_emits_named_diagnostic() {
    let source = r#"
structure def Vent {
    param area : Length = 1mm
}
structure def Manifold {
    sub vents : Keyed<Vent> {
        "intake" => { area = 5mm }
    }
    let b = vents["ghost"].area
}
"#;
    let module = compile_source(source);
    assert_has_diagnostic(
        &module.diagnostics,
        Severity::Error,
        "no keyed member 'ghost' in keyed sub 'vents'",
    );

    // The access lowers to an Undef literal (a clean eval-graph failure), not a
    // scoped ValueRef — so no dangling reference reaches eval.
    let expr = get_let_expr_in(&module, "Manifold", "b");
    assert!(
        matches!(&expr.kind, CompiledExprKind::Literal(Value::Undef)),
        "missing-key access must lower to Literal(Undef), got {:?}",
        expr.kind,
    );
}

// ── step-11: Keyed<T>-in-value-position guard (RED until step-12) ─────────────
//
// `Keyed<T>` is a sub-only collection kind (β escalation esc-3930-295): it has
// no `Value::Keyed` form and lowers to a `SubComponentDecl`, never a value cell.
// Using it in a param/let *value* position must be a clear compile-time Error at
// cell construction (entity.rs), upgrading the eval-layer `is_representable_cell_type`
// backstop to an actionable diagnostic. The low-level type RESOLVER stays
// position-blind (see type_resolution.rs anchor); the guard is layered above it.

/// `param x : Keyed<Vent>` (a value/param position) must emit a clear Error
/// naming `Keyed<T>` as a sub-only collection kind.
///
/// RED today: cell construction does not guard `Type::Keyed`, so no diagnostic
/// is emitted (the resolver is position-blind by design). GREEN after step-12.
#[test]
fn keyed_in_param_value_position_is_rejected() {
    let source = r#"
structure def Vent {
    param area : Length = 1mm
}
structure def S {
    param x : Keyed<Vent>
}
"#;
    let module = compile_source(source);
    assert_has_diagnostic(
        &module.diagnostics,
        Severity::Error,
        "sub-only collection kind",
    );
}

/// `let y : Keyed<Vent> = auto` (a value position via the auto-let path, whose
/// cell type is taken straight from the annotation) must likewise be rejected.
///
/// RED today: same missing guard as the param case. GREEN after step-12.
#[test]
fn keyed_in_let_value_position_is_rejected() {
    let source = r#"
structure def Vent {
    param area : Length = 1mm
}
structure def S {
    let y : Keyed<Vent> = auto
}
"#;
    let module = compile_source(source);
    assert_has_diagnostic(
        &module.diagnostics,
        Severity::Error,
        "sub-only collection kind",
    );
}

/// The legitimate `sub vents : Keyed<Vent>` position must NOT trip the guard:
/// subs lower to `SubComponentDecl`, not value cells. Pins that the step-12
/// guard does not misfire on the intended use.
#[test]
fn keyed_in_sub_position_is_not_rejected() {
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
    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("sub-only collection kind")),
        "the sub-only-collection guard must not fire on a legitimate `sub : Keyed<T>`; \
         diagnostics: {:?}",
        module.diagnostics,
    );
}
