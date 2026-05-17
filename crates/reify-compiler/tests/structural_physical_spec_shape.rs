//! User-observable-signal pin for **task 3603 / GHR-α (PRD §8 Phase 1)**.
//!
//! This file is the entry point for future readers tracing the
//! `geometry-handle-runtime` PRD Phase 1 wiring. It exercises:
//!
//!   1. Stdlib geometry-query helper calls (`volume`, `centroid`) typecheck
//!      to the correct return Type at compile-time. (Eval-time dispatch
//!      arrives in Phase 6 / GHR-ζ; Phase 1 produces `Value::Undef`.)
//!   2. Spec-shape `Physical` trait: a structure conforming to `Physical`
//!      via `param geometry : Solid` + `param material : Material` (instead
//!      of the legacy flat-scalar `param density / volume / centroid_x/y/z`
//!      params) compiles with NO error-severity diagnostics, gains
//!      `mass` and `centroid` value cells from the trait's let defaults,
//!      and pulls `material.density` via struct-member access (SIR-α).
//!
//! See `docs/prds/v0_3/geometry-handle-runtime.md` §1 + §8.

use reify_compiler::{CompiledModule, DefaultKind, RequirementKind, stdlib_loader};
use reify_test_support::compile_source_with_stdlib;
use reify_types::{DimensionVector, Severity, Type};

/// Return the `std/structural/physical` CompiledModule from the production
/// stdlib loader — mirrors the helper in `structural_physical_tests.rs` so
/// both files exercise the exact same compile path (embedded source,
/// sequential prelude growth, OnceLock cache).
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/structural/physical")
        .expect("stdlib should contain std/structural/physical module")
}

/// `volume(my_box)` where `my_box : Solid` typechecks to `Scalar<Volume>`.
///
/// Pins the dispatch arm in `expr.rs::infer_type` that consults
/// `geometry_query_result_type` (added in step-8). Without that arm, the
/// inference falls through to the first-arg type (`Geometry`), which fails
/// `is_representable_cell_type`.
#[test]
fn spec_shape_volume_call_typechecks_to_scalar_volume() {
    let source = r#"
structure def MyBox {
    param my_box : Solid = box(10mm, 20mm, 30mm)
    let v = volume(my_box)
}
"#;
    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero error-severity diagnostics, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least one template");
    let v_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "v")
        .expect("expected 'v' value cell from `let v = volume(my_box)`");
    assert_eq!(
        v_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::VOLUME
        },
        "`volume(...)` must infer to Scalar<Volume>"
    );
}

/// `centroid(my_box)` where `my_box : Solid` typechecks to `Point3<Length>`.
///
/// Pins the second sample from the GHR-α §1 frozen list; matches the same
/// dispatch arm as the volume test above.
#[test]
fn spec_shape_centroid_call_typechecks_to_point3_length() {
    let source = r#"
structure def MyBox {
    param my_box : Solid = box(10mm, 20mm, 30mm)
    let c = centroid(my_box)
}
"#;
    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero error-severity diagnostics, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least one template");
    let c_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "c")
        .expect("expected 'c' value cell from `let c = centroid(my_box)`");
    assert_eq!(
        c_cell.cell_type,
        Type::point3(Type::length()),
        "`centroid(...)` must infer to Point3<Length>"
    );
}

// ─── headline Phase-1 integration: spec-shape Bracket compiles ───────────────

/// **Headline GHR-α / PRD §8 Phase 1 user-observable signal.**
///
/// A `structure def Bracket : Physical` with spec-shape Physical (geometry +
/// material slots, mass/centroid as trait-injected lets) compiles cleanly:
///
///   1. No error-severity diagnostics — specifically no
///      "member access not yet supported", no "unresolved type", no
///      "missing required member".
///   2. The compiled `Bracket` template carries `Physical` in `trait_bounds`.
///   3. Value cells for `geometry` and `material` exist (from the bracket's
///      own params).
///   4. Value cells for `mass` and `centroid` exist (injected from the
///      Physical trait's `let` defaults).
///
/// Pins the cross-product of:
///   - SIR-α (task 3540) struct-member access on `material.density`,
///   - Step-8's geometry-query dispatch arm in `expr.rs::infer_type` for the
///     `volume(geometry)` and `centroid(geometry)` calls inside the trait's
///     let defaults,
///   - Step-10's rewrite of `Physical` to the spec shape.
///
/// Until step-10 lands, Physical still has the legacy flat-scalar shape, so
/// this fixture fails with "missing required member: density / volume /
/// centroid_x / centroid_y / centroid_z".
#[test]
fn spec_shape_physical_bracket_compiles_with_material_density_access() {
    let source = r#"
structure def Bracket : Physical {
    param geometry : Solid = box(10mm, 20mm, 30mm)
    param material : Material = Material(name: "steel", density: 7850.0, youngs_modulus: 200000000000.0)
}
"#;
    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "spec-shape `Bracket : Physical` should compile clean (no errors); got: {:?}",
        errors
    );

    let bracket = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bracket")
        .expect("Bracket template should be compiled");

    assert!(
        bracket.trait_bounds.iter().any(|b| b == "Physical"),
        "Bracket should carry `Physical` in trait_bounds; got: {:?}",
        bracket.trait_bounds
    );

    for expected_member in &["geometry", "material", "mass", "centroid"] {
        assert!(
            bracket
                .value_cells
                .iter()
                .any(|vc| vc.id.member == *expected_member),
            "Bracket should have a value cell for '{}'; got members: {:?}",
            expected_member,
            bracket
                .value_cells
                .iter()
                .map(|vc| vc.id.member.as_str())
                .collect::<Vec<_>>()
        );
    }
}

/// Companion to the headline test above — pins the Physical trait's own
/// schema (independent of any consumer structure).
///
/// Asserts:
///   - `required_members` contains `geometry : Solid` and `material : Material`.
///   - `refinements` is EMPTY (no MaterialSpec — Material is a struct slot
///     now, not a trait edge — see SIR-α / task 3540 + PRD §1).
///   - `defaults` includes two `DefaultKind::Let` entries, one named `mass`
///     and one named `centroid` (the trait's `let mass = volume(geometry) *
///     material.density` and `let centroid = centroid(geometry)`).
///
/// Fails on the current flat-scalar shape: `required_members` is
/// {volume, centroid_x, centroid_y, centroid_z}, refinements contains
/// "MaterialSpec", and there is no `centroid` Let default.
#[test]
fn spec_shape_physical_trait_required_members_are_geometry_and_material() {
    let module = load_stdlib_module();
    let physical = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Physical")
        .expect("Physical trait should be in std/structural/physical");

    // (1) refinements must be empty — Material is no longer a trait edge.
    assert!(
        physical.refinements.is_empty(),
        "spec-shape Physical should have NO trait refinements (Material is a \
         struct slot now, not a trait edge); got: {:?}",
        physical.refinements
    );

    // (2) required_members: exactly `geometry : Solid` and `material : Material`.
    let geometry_req = physical
        .required_members
        .iter()
        .find(|r| r.name == "geometry")
        .unwrap_or_else(|| {
            panic!(
                "Physical should require `geometry` member; got: {:?}",
                physical
                    .required_members
                    .iter()
                    .map(|r| r.name.as_str())
                    .collect::<Vec<_>>()
            )
        });
    match &geometry_req.kind {
        RequirementKind::Param(ty) => {
            // `Solid` is the surface-syntax alias for `Type::Geometry` — see
            // `type_resolution::resolve_type_name`.
            assert_eq!(
                *ty,
                Type::Geometry,
                "Physical.geometry should be RequirementKind::Param(Geometry) \
                 (the resolved type behind the `Solid` surface alias); got {:?}",
                ty
            );
        }
        other => panic!(
            "Physical.geometry should be RequirementKind::Param, got {:?}",
            other
        ),
    }

    let material_req = physical
        .required_members
        .iter()
        .find(|r| r.name == "material")
        .unwrap_or_else(|| {
            panic!(
                "Physical should require `material` member; got: {:?}",
                physical
                    .required_members
                    .iter()
                    .map(|r| r.name.as_str())
                    .collect::<Vec<_>>()
            )
        });
    match &material_req.kind {
        RequirementKind::Param(ty) => {
            assert_eq!(
                *ty,
                Type::StructureRef("Material".to_string()),
                "Physical.material should be RequirementKind::Param(StructureRef(\"Material\")); got {:?}",
                ty
            );
        }
        other => panic!(
            "Physical.material should be RequirementKind::Param, got {:?}",
            other
        ),
    }

    // (3) defaults: two `Let` entries named `mass` and `centroid`.
    let let_defaults: Vec<_> = physical
        .defaults
        .iter()
        .filter(|d| matches!(d.kind, DefaultKind::Let { .. }))
        .collect();
    for expected_let in &["mass", "centroid"] {
        assert!(
            let_defaults
                .iter()
                .any(|d| d.name.as_deref() == Some(*expected_let)),
            "spec-shape Physical should have a `Let` default named '{}'; got: {:?}",
            expected_let,
            physical
                .defaults
                .iter()
                .map(|d| (&d.name, std::mem::discriminant(&d.kind)))
                .collect::<Vec<_>>()
        );
    }
}
