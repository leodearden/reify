//! Tests for `crates/reify-compiler/stdlib/ports.ri` and
//! `crates/reify-compiler/stdlib/ports_mechanical.ri` —
//! `std.ports` module: Directionality enum, Port base trait.
//! `std.ports.mechanical` module: Torque type alias, MechanicalPort, Bore,
//! Shaft, RotaryPort, ThreadedPort, StructurePort traits.
//!
//! Reconstructs the lost std.ports stdlib surface per PRD
//! docs/prds/v0_6/stdlib-reconstruction.md task α.
//!
//! Tests use the production-path `load_stdlib()` helper, modeled on
//! `process_stdlib_compile.rs`.

use reify_compiler::{
    CompiledTrait, DefaultKind, EntityKind, RequirementKind, ValueCellDecl, ValueCellKind,
    stdlib_loader,
};
use reify_ast::ExprKind;
use reify_core::{DimensionVector, Severity, Type};
use reify_ir::{CompiledExpr, CompiledExprKind, EnumDef, Value};
use reify_test_support::compile_source_with_stdlib;

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Return the `CompiledModule` for `path` from the production stdlib loader.
/// Panics if the module is not registered in stdlib_loader.rs.
fn load_module(path: &str) -> &'static reify_compiler::CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == path)
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain module '{}'; available paths: {:?}",
                path,
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up an enum definition by name within the given stdlib module path.
fn find_enum(module_path: &str, name: &str) -> &'static EnumDef {
    let module = load_module(module_path);
    module
        .enum_defs
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| {
            panic!(
                "expected `enum {}` in {}, got enum_defs: {:?}",
                name,
                module_path,
                module.enum_defs.iter().map(|e| &e.name).collect::<Vec<_>>()
            )
        })
}

/// Find a trait by name within the given stdlib module path.
fn find_trait(module_path: &str, name: &str) -> &'static CompiledTrait {
    let module = load_module(module_path);
    module
        .trait_defs
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| {
            panic!(
                "module '{}' should contain trait '{}'; found: {:?}",
                module_path,
                name,
                module.trait_defs.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        })
}

/// Get the Param type for a named required member of a trait within the given
/// stdlib module path, or panic.
fn param_type(module_path: &str, trait_name: &str, member: &str) -> Type {
    let t = find_trait(module_path, trait_name);
    let req = t
        .required_members
        .iter()
        .find(|r| r.name == member)
        .unwrap_or_else(|| {
            panic!(
                "module '{}' trait '{}' should have required member '{}'; found: {:?}",
                module_path,
                trait_name,
                member,
                t.required_members
                    .iter()
                    .map(|r| &r.name)
                    .collect::<Vec<_>>()
            )
        });
    match &req.kind {
        RequirementKind::Param(ty) => ty.clone(),
        other => panic!(
            "module '{}' trait '{}' member '{}' should be RequirementKind::Param, got {:?}",
            module_path, trait_name, member, other
        ),
    }
}

/// Recursively collect ValueRef member names from a compiled expression tree.
/// Mirrors `collect_value_ref_members` in `modal_options_validation_tests.rs`
/// and `buckling_stdlib_compile.rs` — used to assert a `let` RHS references
/// the expected param cells.
fn collect_value_ref_members(expr: &CompiledExpr) -> Vec<&str> {
    match &expr.kind {
        CompiledExprKind::ValueRef(cell_id) => vec![cell_id.member.as_str()],
        CompiledExprKind::BinOp { left, right, .. } => {
            let mut refs = collect_value_ref_members(left);
            refs.extend(collect_value_ref_members(right));
            refs
        }
        CompiledExprKind::UnOp { operand, .. } => collect_value_ref_members(operand),
        _ => vec![],
    }
}

/// Recursively collect numeric-literal values (`Value::Real`/`Value::Scalar`/
/// `Value::Int` operands) from a compiled expression tree.
///
/// `collect_value_ref_members` proves a `let` RHS references the right *params*,
/// but not the literal *coefficients* baked beside them — a regression that
/// changed `pitch * 1.0825` to `pitch * 1.25` would still reference `pitch` and
/// slip past that walk. This pins the exact ISO coefficients instead (see
/// `thread_spec_derived_let_coefficients_pinned`).
fn collect_number_literals(expr: &CompiledExpr) -> Vec<f64> {
    match &expr.kind {
        CompiledExprKind::Literal(Value::Real(v)) => vec![*v],
        CompiledExprKind::Literal(Value::Scalar { si_value, .. }) => vec![*si_value],
        CompiledExprKind::Literal(Value::Int(v)) => vec![*v as f64],
        CompiledExprKind::BinOp { left, right, .. } => {
            let mut lits = collect_number_literals(left);
            lits.extend(collect_number_literals(right));
            lits
        }
        CompiledExprKind::UnOp { operand, .. } => collect_number_literals(operand),
        _ => vec![],
    }
}

// ─── step-1: module loads + Directionality enum ──────────────────────────────

/// The std/ports module must load through the production stdlib path with zero
/// error-severity diagnostics, and enum Directionality must have exactly the
/// three variants [In, Out, Bidi] in that order.
#[test]
fn std_ports_loads_with_no_errors_and_directionality_enum() {
    let module = load_module("std/ports");

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in ports.ri: {:?}",
        errors
    );

    let enum_def = find_enum("std/ports", "Directionality");
    assert_eq!(
        enum_def.variants,
        vec![
            "In".to_string(),
            "Out".to_string(),
            "Bidi".to_string(),
        ],
        "Directionality variants must be [In, Out, Bidi] in order; got: {:?}",
        enum_def.variants
    );
}

// ─── step-1 (task α): Port.direction=Bidi default ────────────────────────────

/// Port base trait has no refinements.  After the Bidi-default lands (task α
/// step-2), `direction` moves from `required_members` (no default) to
/// `defaults` (DefaultKind::Param with Directionality.Bidi).
///
/// Asserts:
///   (a) Port has no refinements.
///   (b) Port.required_members is EMPTY — `direction` is no longer required.
///   (c) Port.defaults contains an entry `name == Some("direction")` with
///       `DefaultKind::Param { cell_type: Type::Enum("Directionality"), .. }`
///       whose `default_decl.default` is
///       `ExprKind::EnumAccess { type_name: "Directionality", variant: "Bidi" }`.
#[test]
fn port_base_trait_requires_direction_directionality() {
    let t = find_trait("std/ports", "Port");

    assert!(
        t.refinements.is_empty(),
        "Port should have no refinements, got: {:?}",
        t.refinements
    );

    // direction now has a default (= Directionality.Bidi), so it is absent from
    // required_members (which only holds members that have no default).
    assert!(
        t.required_members.is_empty(),
        "Port should have 0 required members after Bidi default lands; got: {:?}",
        t.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    // direction must be in defaults with the correct type and Bidi variant.
    let dir_default = t
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("direction"))
        .expect("Port.defaults should contain an entry named 'direction' (= Directionality.Bidi)");

    match &dir_default.kind {
        DefaultKind::Param { cell_type, default_decl } => {
            assert_eq!(
                *cell_type,
                Type::Enum("Directionality".into()),
                "Port.direction default cell_type must be Type::Enum(\"Directionality\")"
            );
            let expr = default_decl
                .default
                .as_ref()
                .expect("Port.direction default_decl must have a default expression");
            match &expr.kind {
                reify_ast::ExprKind::EnumAccess { type_name, variant } => {
                    assert_eq!(
                        type_name, "Directionality",
                        "Port.direction default expr type_name should be \"Directionality\""
                    );
                    assert_eq!(
                        variant, "Bidi",
                        "Port.direction default expr variant should be \"Bidi\""
                    );
                }
                other => panic!(
                    "Port.direction default expr should be \
                     EnumAccess {{ type_name: \"Directionality\", variant: \"Bidi\" }}, \
                     got: {:?}",
                    other
                ),
            }
        }
        other => panic!(
            "Port.direction default must be DefaultKind::Param, got: {:?}",
            other
        ),
    }
}

/// Positive compile test: a structure whose port conforms to Port WITHOUT
/// specifying `direction` must compile with zero Severity::Error diagnostics.
///
/// This directly pins the 'omit direction → defaults to Bidi' behavioural
/// contract that motivated the Port.direction default change.  If the default
/// machinery is broken, a missing-required-param error would fire here.
#[test]
fn port_conforms_without_direction_compiles_clean() {
    let source = r#"
structure def Sender {
    port out_p : out Port {}
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
        "a Port conformer that omits 'direction' should compile without errors \
         (direction defaults to Directionality.Bidi); got: {:?}",
        errors
    );
}

/// std/ports cardinality lock: exactly 1 trait (Port), 1 enum (Directionality),
/// 1 structure (Frame3). Updated incrementally by task α steps:
///   step-4: structures 0→1 (Frame3)
///   step-6: traits 1→2 (+ LocatedPort)
///   step-8: traits 2→3 (+ RegionPort)
#[test]
fn std_ports_module_cardinality_locked() {
    let module = load_module("std/ports");

    let enum_names: Vec<&str> = module.enum_defs.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        module.enum_defs.len(),
        1,
        "std/ports should declare exactly 1 enum (Directionality), got: {:?}",
        enum_names
    );
    assert!(
        module.enum_defs.iter().any(|e| e.name == "Directionality"),
        "std/ports should contain the 'Directionality' enum, got: {:?}",
        enum_names
    );

    let trait_names: Vec<&str> = module
        .trait_defs
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        module.trait_defs.len(),
        3,
        "std/ports should declare exactly 3 traits (Port, LocatedPort, RegionPort) \
         after step-8, got: {:?}",
        trait_names
    );

    let structure_names: Vec<&str> = module
        .templates
        .iter()
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        structure_names.len(),
        1,
        "std/ports should declare exactly 1 structure (Frame3), got: {:?}",
        structure_names
    );
    assert!(
        module
            .templates
            .iter()
            .any(|t| t.name == "Frame3" && t.entity_kind == EntityKind::Structure),
        "std/ports should contain 'Frame3' structure, got: {:?}",
        structure_names
    );
}

// ─── step-3 (task α): Frame3 structure surface ───────────────────────────────

/// std/ports should contain a `structure def Frame3` with exactly 4 Param-kind
/// value cells (origin, x_axis, y_axis, z_axis), each resolving to
/// `Type::Vector { n: 3, quantity: Scalar[LENGTH] }`.
///
/// Frame3 is the port-frame structure added by task α, step-4.
/// RED on current main (no Frame3 in std/ports → template lookup fails).
#[test]
fn frame3_structure_surface() {
    let module = load_module("std/ports");

    let frame3 = module
        .templates
        .iter()
        .find(|t| t.name == "Frame3" && t.entity_kind == EntityKind::Structure)
        .expect(
            "std/ports should contain 'structure def Frame3'; \
             check ports.ri for the Frame3 definition"
        );

    // Collect Param-kind value cells (excluding Let/Auto).
    let param_cells: Vec<&ValueCellDecl> = frame3
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, ValueCellKind::Param))
        .collect();

    let param_names: Vec<&str> = param_cells
        .iter()
        .map(|vc| vc.id.member.as_str())
        .collect();

    assert_eq!(
        param_cells.len(),
        4,
        "Frame3 should have exactly 4 param cells \
         (origin, x_axis, y_axis, z_axis), got: {:?}",
        param_names
    );

    // All 4 params must resolve to Vector3<Length>.
    let expected_type = Type::Vector {
        n: 3,
        quantity: Box::new(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
    };
    for expected_name in &["origin", "x_axis", "y_axis", "z_axis"] {
        let cell = param_cells
            .iter()
            .find(|vc| vc.id.member == *expected_name)
            .unwrap_or_else(|| {
                panic!(
                    "Frame3 missing param '{}'; got: {:?}",
                    expected_name, param_names
                )
            });
        assert_eq!(
            cell.cell_type,
            expected_type,
            "Frame3.{} must be Type::Vector{{n:3, quantity:Scalar[LENGTH]}}, got {:?}",
            expected_name,
            cell.cell_type
        );
    }
}

// ─── step-5 (task α): LocatedPort trait surface ──────────────────────────────

/// LocatedPort refines exactly ["Port"] and has exactly one required member:
/// `frame : Frame3`.  Proves:
///   - LocatedPort.refinements == ["Port"]
///   - LocatedPort.required_members == [{ name: "frame", kind: Param(StructureRef("Frame3")) }]
///   - param_type helper finds "frame" as Type::StructureRef("Frame3")
///
/// RED on current main (LocatedPort absent → find_trait panics).
#[test]
fn located_port_trait_surface() {
    let t = find_trait("std/ports", "LocatedPort");

    assert_eq!(
        t.refinements.as_slice(),
        ["Port".to_string()].as_slice(),
        "LocatedPort should refine exactly [Port], got: {:?}",
        t.refinements
    );

    assert_eq!(
        t.required_members.len(),
        1,
        "LocatedPort should have exactly 1 required member (frame); got: {:?}",
        t.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name, "frame",
        "LocatedPort required_members[0] should be 'frame', got '{}'",
        t.required_members[0].name
    );

    assert_eq!(
        param_type("std/ports", "LocatedPort", "frame"),
        Type::StructureRef("Frame3".into()),
        "LocatedPort.frame must be Type::StructureRef(\"Frame3\")"
    );
}

// ─── step-5: std/ports/mechanical loads + marker traits ──────────────────────

/// std/ports/mechanical must load with zero Severity::Error diagnostics.
/// MechanicalPort refines exactly ["LocatedPort"] (step-8 restructure); Bore,
/// Shaft, StructurePort each refine exactly ["MechanicalPort"] with empty own
/// required_members.
#[test]
fn std_ports_mechanical_loads_with_no_errors_and_marker_traits() {
    let module = load_module("std/ports/mechanical");

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in ports_mechanical.ri: {:?}",
        errors
    );

    let mechanical_port = find_trait("std/ports/mechanical", "MechanicalPort");
    assert_eq!(
        mechanical_port.refinements.as_slice(),
        ["LocatedPort".to_string()].as_slice(),
        "MechanicalPort should refine exactly [LocatedPort] (step-8 restructure: a \
         mechanical port is spatially located), got: {:?}",
        mechanical_port.refinements
    );
    // max_load/max_torque carry `= none` defaults, so they live in `defaults`,
    // not `required_members` (which holds only members with no default).
    assert!(
        mechanical_port.required_members.is_empty(),
        "MechanicalPort should have no own required_members (max_load/max_torque \
         are Option<…> = none, hence in defaults), got: {:?}",
        mechanical_port
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    for name in &["Bore", "Shaft", "StructurePort"] {
        let t = find_trait("std/ports/mechanical", name);
        assert_eq!(
            t.refinements.as_slice(),
            ["MechanicalPort".to_string()].as_slice(),
            "trait '{}' should refine exactly [MechanicalPort], got: {:?}",
            name,
            t.refinements
        );
        assert!(
            t.required_members.is_empty(),
            "trait '{}' should have no own required_members, got: {:?}",
            name,
            t.required_members.iter().map(|r| &r.name).collect::<Vec<_>>()
        );
    }
}

// ─── step-7 (task β): MechanicalPort optional ratings ────────────────────────

/// MechanicalPort carries two optional ratings as `Option<…> = none` defaults:
///   max_load   : Option<Force>  = none
///   max_torque : Option<Torque> = none
/// Both live in `defaults` (DefaultKind::Param), not required_members.
///
/// Mirrors signal_port_trait_surface's impedance-default assertion.
/// RED: MechanicalPort has an empty body (no such defaults).
#[test]
fn mechanical_port_optional_ratings_surface() {
    let t = find_trait("std/ports/mechanical", "MechanicalPort");

    // max_load : Option<Force> = none
    let max_load = t
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("max_load"))
        .expect("MechanicalPort.defaults should contain an entry named 'max_load' (= none)");
    match &max_load.kind {
        DefaultKind::Param { cell_type, default_decl } => {
            assert_eq!(
                *cell_type,
                Type::Option(Box::new(Type::Scalar {
                    dimension: DimensionVector::FORCE
                })),
                "MechanicalPort.max_load default cell_type must be Type::Option(Scalar<FORCE>)"
            );
            assert!(
                default_decl.default.is_some(),
                "MechanicalPort.max_load default_decl must have a default expression (none)"
            );
        }
        other => panic!(
            "MechanicalPort.max_load default must be DefaultKind::Param, got: {:?}",
            other
        ),
    }

    // max_torque : Option<Torque> = none  (Torque = Force·Length/Angle)
    let expected_torque_dim = DimensionVector::FORCE
        .mul(&DimensionVector::LENGTH)
        .div(&DimensionVector::ANGLE);
    let max_torque = t
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("max_torque"))
        .expect("MechanicalPort.defaults should contain an entry named 'max_torque' (= none)");
    match &max_torque.kind {
        DefaultKind::Param { cell_type, default_decl } => {
            assert_eq!(
                *cell_type,
                Type::Option(Box::new(Type::Scalar {
                    dimension: expected_torque_dim
                })),
                "MechanicalPort.max_torque default cell_type must be \
                 Type::Option(Scalar<Torque>) (Force·Length/Angle)"
            );
            assert!(
                default_decl.default.is_some(),
                "MechanicalPort.max_torque default_decl must have a default expression (none)"
            );
        }
        other => panic!(
            "MechanicalPort.max_torque default must be DefaultKind::Param, got: {:?}",
            other
        ),
    }
}

// ─── step-7: RotaryPort/ThreadedPort + cardinality lock ──────────────────────

// ─── step-9 (task β): MotivePort / RotaryPort / LinearPort surfaces ───────────

/// MotivePort is the motive (power-delivering) port base: it refines exactly
/// [MechanicalPort] with no own required members.
///
/// RED: MotivePort is absent → find_trait panics.
#[test]
fn motive_port_trait_surface() {
    let t = find_trait("std/ports/mechanical", "MotivePort");

    assert_eq!(
        t.refinements.as_slice(),
        ["MechanicalPort".to_string()].as_slice(),
        "MotivePort should refine exactly [MechanicalPort], got: {:?}",
        t.refinements
    );
    assert!(
        t.required_members.is_empty(),
        "MotivePort should have no own required_members, got: {:?}",
        t.required_members.iter().map(|r| &r.name).collect::<Vec<_>>()
    );
}

/// RotaryPort refines exactly [MotivePort] (was [MechanicalPort]) with required
/// members [max_speed, max_torque, axis] in order (was [torque_capacity,
/// max_speed]):
///   max_speed  : Scalar<ANGULAR_VELOCITY>
///   max_torque : Scalar<Torque = Force·Length/Angle>  (renamed from torque_capacity)
///   axis       : Vector3<Length>
///
/// The Torque dimension (Force·Length/Angle) is distinct from Energy
/// (kg·m²·s⁻²) via the Angle⁻¹ slot — a regression to Energy is caught here.
///
/// RED: RotaryPort still refines [MechanicalPort] with [torque_capacity, max_speed].
#[test]
fn rotary_port_trait_surface() {
    let t = find_trait("std/ports/mechanical", "RotaryPort");

    assert_eq!(
        t.refinements.as_slice(),
        ["MotivePort".to_string()].as_slice(),
        "RotaryPort should refine exactly [MotivePort], got: {:?}",
        t.refinements
    );

    assert_eq!(
        t.required_members.len(),
        3,
        "RotaryPort should have exactly 3 required members [max_speed, max_torque, axis]; \
         got: {:?}",
        t.required_members.iter().map(|r| &r.name).collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name, "max_speed",
        "RotaryPort required_members[0] should be 'max_speed'"
    );
    assert_eq!(
        t.required_members[1].name, "max_torque",
        "RotaryPort required_members[1] should be 'max_torque'"
    );
    assert_eq!(
        t.required_members[2].name, "axis",
        "RotaryPort required_members[2] should be 'axis'"
    );

    assert_eq!(
        param_type("std/ports/mechanical", "RotaryPort", "max_speed"),
        Type::Scalar {
            dimension: DimensionVector::ANGULAR_VELOCITY
        },
        "RotaryPort.max_speed must have DimensionVector::ANGULAR_VELOCITY"
    );

    // Torque = Force * Length / Angle (distinct from Energy by the Angle⁻¹ slot).
    let expected_torque_dim = DimensionVector::FORCE
        .mul(&DimensionVector::LENGTH)
        .div(&DimensionVector::ANGLE);
    assert_eq!(
        param_type("std/ports/mechanical", "RotaryPort", "max_torque"),
        Type::Scalar {
            dimension: expected_torque_dim
        },
        "RotaryPort.max_torque must be Scalar<Force·Length/Angle> \
         (Torque alias — distinct from Energy via Angle⁻¹ slot)"
    );

    assert_eq!(
        param_type("std/ports/mechanical", "RotaryPort", "axis"),
        Type::Vector {
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH
            }),
        },
        "RotaryPort.axis must be Vector3<Length>"
    );

    // Regression guard: torque_capacity was renamed to max_torque.
    assert!(
        !t.required_members.iter().any(|r| r.name == "torque_capacity"),
        "RotaryPort must not expose 'torque_capacity' (renamed to max_torque)"
    );
}

/// Behavioural (conformance): a concrete structure conforming to RotaryPort that
/// supplies all four required members (frame, max_speed, max_torque, axis)
/// compiles clean.
///
/// End-to-end counterpart to `rotary_port_trait_surface`: it drives the merged
/// RotaryPort requirement set (Port→LocatedPort→MechanicalPort→MotivePort→
/// RotaryPort) through the strict `structure def X : Trait` conformance checker
/// (the lenient `port p : in Trait {}` slot does NOT enforce required-member
/// presence). This is the POSITIVE half of the `max_torque` name-shadowing
/// guarantee: RotaryPort's required `max_torque : Torque` re-declares
/// MechanicalPort's optional `max_torque : Option<Torque> = none`, and supplying a
/// real `Torque` here satisfies it. The NEGATIVE half is
/// `rotary_port_conformer_inherited_option_default_does_not_satisfy_max_torque`.
#[test]
fn rotary_port_concrete_conformer_compiles() {
    let source = r#"
import std.ports.mechanical

structure def RotaryConformer : RotaryPort {
    param frame : Frame3 = Frame3(
        origin: vec3(0mm, 0mm, 0mm),
        x_axis: vec3(1mm, 0mm, 0mm),
        y_axis: vec3(0mm, 1mm, 0mm),
        z_axis: vec3(0mm, 0mm, 1mm),
    )
    param max_speed : AngularVelocity = 1rad / 1s
    param max_torque : Torque = 1N * 1m / 1rad
    param axis : Vector3<Length> = vec3(0mm, 0mm, 1mm)
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
        "a structure conforming to RotaryPort and supplying frame/max_speed/max_torque/axis \
         should compile without errors (the required max_torque:Torque is satisfied by the \
         supplied Torque value); got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Behavioural (conformance): a structure conforming to RotaryPort that omits
/// `max_torque` is REJECTED — the inherited MechanicalPort default
/// `max_torque : Option<Torque> = none` does NOT satisfy RotaryPort's required
/// `max_torque : Torque`.
///
/// This is the load-bearing half of the `max_torque` shadowing guarantee
/// (plan.json design decision 1). The conformance checker compares the inherited
/// `Option<Torque>` default against the required `Torque` and rejects it on type
/// grounds — "type mismatch for trait member 'max_torque' … available default has
/// Option<…>" — rather than silently accepting the optional default. If that
/// shadowing ever regressed so the `Option<Torque>=none` default DID satisfy the
/// requirement, this conformer would compile clean and the assertion would fail.
///
/// Probed against the omit-`max_speed` case (a required member with NO inherited
/// default → plain "missing required member"), which confirms the strict
/// conformance checker is reached for both — so the type-mismatch outcome here is
/// specifically the inherited Option default being rejected, not a generic miss.
#[test]
fn rotary_port_conformer_inherited_option_default_does_not_satisfy_max_torque() {
    let source = r#"
import std.ports.mechanical

structure def RotaryConformerMissingTorque : RotaryPort {
    param frame : Frame3 = Frame3(
        origin: vec3(0mm, 0mm, 0mm),
        x_axis: vec3(1mm, 0mm, 0mm),
        y_axis: vec3(0mm, 1mm, 0mm),
        z_axis: vec3(0mm, 0mm, 1mm),
    )
    param max_speed : AngularVelocity = 1rad / 1s
    param axis : Vector3<Length> = vec3(0mm, 0mm, 1mm)
}
"#;
    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("max_torque") && d.message.contains("Option<")),
        "omitting max_torque from a RotaryPort conformer must be rejected because the inherited \
         Option<Torque>=none default cannot satisfy the required Torque (expected a diagnostic \
         naming max_torque and the rejected Option<…> default); got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// LinearPort refines exactly [MotivePort] with required members [max_speed,
/// max_force, stroke, axis] in order:
///   max_speed : Scalar<Velocity = Length/Time>
///   max_force : Scalar<FORCE>
///   stroke    : Scalar<LENGTH>
///   axis      : Vector3<Length>
///
/// RED: LinearPort is absent → find_trait panics.
#[test]
fn linear_port_trait_surface() {
    let t = find_trait("std/ports/mechanical", "LinearPort");

    assert_eq!(
        t.refinements.as_slice(),
        ["MotivePort".to_string()].as_slice(),
        "LinearPort should refine exactly [MotivePort], got: {:?}",
        t.refinements
    );

    assert_eq!(
        t.required_members.len(),
        4,
        "LinearPort should have exactly 4 required members \
         [max_speed, max_force, stroke, axis]; got: {:?}",
        t.required_members.iter().map(|r| &r.name).collect::<Vec<_>>()
    );
    for (i, name) in ["max_speed", "max_force", "stroke", "axis"].iter().enumerate() {
        assert_eq!(
            t.required_members[i].name, *name,
            "LinearPort required_members[{}] should be '{}'",
            i, name
        );
    }

    // Velocity = Length / Time (alias resolves to the composite dimension).
    let expected_velocity_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);
    assert_eq!(
        param_type("std/ports/mechanical", "LinearPort", "max_speed"),
        Type::Scalar {
            dimension: expected_velocity_dim
        },
        "LinearPort.max_speed must be Scalar<Length/Time> (Velocity alias)"
    );
    assert_eq!(
        param_type("std/ports/mechanical", "LinearPort", "max_force"),
        Type::Scalar {
            dimension: DimensionVector::FORCE
        },
        "LinearPort.max_force must be Scalar<FORCE>"
    );
    assert_eq!(
        param_type("std/ports/mechanical", "LinearPort", "stroke"),
        Type::Scalar {
            dimension: DimensionVector::LENGTH
        },
        "LinearPort.stroke must be Scalar<LENGTH>"
    );
    assert_eq!(
        param_type("std/ports/mechanical", "LinearPort", "axis"),
        Type::Vector {
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH
            }),
        },
        "LinearPort.axis must be Vector3<Length>"
    );
}

// ─── step-11 (task β): GuidePort / LinearGuidePort / RotaryGuidePort ──────────

/// Assert that `trait_name` in std/ports/mechanical carries exactly one
/// trait-level constraint and that it is the unnamed single-DOF invariant
/// `degrees_of_freedom == 1`.
///
/// An unlabeled trait-body `constraint <expr>` compiles to a
/// `TraitDefault { name: None, kind: DefaultKind::Constraint(decl) }`
/// (traits.rs:252-257) with the parsed predicate preserved verbatim, so the
/// AST is `BinOp { op: "==", left: Ident("degrees_of_freedom"),
/// right: NumberLiteral { value: 1.0, is_real: false } }`. Pinning the RHS
/// literal here is what distinguishes the prismatic/revolute `== 1` invariant
/// from any other DOF count.
fn assert_dof_eq_one_constraint(trait_name: &str) {
    let t = find_trait("std/ports/mechanical", trait_name);
    let constraint_count = t
        .defaults
        .iter()
        .filter(|d| matches!(d.kind, DefaultKind::Constraint(_)))
        .count();
    assert_eq!(
        constraint_count, 1,
        "{} should carry exactly one trait-level constraint; got {} total defaults",
        trait_name,
        t.defaults.len()
    );
    let c = t
        .defaults
        .iter()
        .find(|d| matches!(d.kind, DefaultKind::Constraint(_)))
        .unwrap();
    assert!(
        c.name.is_none(),
        "{}'s degrees_of_freedom constraint should be unlabeled (name: None), got: {:?}",
        trait_name, c.name
    );
    let decl = match &c.kind {
        DefaultKind::Constraint(decl) => decl,
        _ => unreachable!("filtered to Constraint above"),
    };
    match &decl.expr.kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "==", "{} constraint op should be '=='", trait_name);
            assert!(
                matches!(&left.kind, ExprKind::Ident(name) if name == "degrees_of_freedom"),
                "{} constraint LHS should be Ident(degrees_of_freedom), got: {:?}",
                trait_name, left.kind
            );
            assert!(
                matches!(&right.kind, ExprKind::NumberLiteral { value, .. } if *value == 1.0),
                "{} constraint RHS should be NumberLiteral(1), got: {:?}",
                trait_name, right.kind
            );
        }
        other => panic!(
            "{} constraint should be a BinOp `degrees_of_freedom == 1`, got: {:?}",
            trait_name, other
        ),
    }
}

/// GuidePort is the kinematic-guide base: it refines exactly [MechanicalPort]
/// with a single required member `degrees_of_freedom : Int` (the count of
/// permitted relative DOFs along/about the guide).
///
/// RED: GuidePort is absent → find_trait panics.
#[test]
fn guide_port_trait_surface() {
    let t = find_trait("std/ports/mechanical", "GuidePort");

    assert_eq!(
        t.refinements.as_slice(),
        ["MechanicalPort".to_string()].as_slice(),
        "GuidePort should refine exactly [MechanicalPort], got: {:?}",
        t.refinements
    );

    assert_eq!(
        t.required_members.len(),
        1,
        "GuidePort should have exactly 1 required member (degrees_of_freedom); got: {:?}",
        t.required_members.iter().map(|r| &r.name).collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name, "degrees_of_freedom",
        "GuidePort required_members[0] should be 'degrees_of_freedom'"
    );
    assert_eq!(
        param_type("std/ports/mechanical", "GuidePort", "degrees_of_freedom"),
        Type::Int,
        "GuidePort.degrees_of_freedom must be Type::Int"
    );
}

/// LinearGuidePort refines exactly [GuidePort], adds no own required params, and
/// pins the single-DOF invariant as a trait-level constraint
/// `degrees_of_freedom == 1` (a prismatic guide permits exactly one
/// translational DOF). A conformer with degrees_of_freedom != 1 is a constraint
/// violation (exercised behaviourally in ports_mechanical_thread_eval.rs).
///
/// RED: LinearGuidePort is absent → find_trait panics.
#[test]
fn linear_guide_port_trait_surface() {
    let t = find_trait("std/ports/mechanical", "LinearGuidePort");

    assert_eq!(
        t.refinements.as_slice(),
        ["GuidePort".to_string()].as_slice(),
        "LinearGuidePort should refine exactly [GuidePort], got: {:?}",
        t.refinements
    );
    assert!(
        t.required_members.is_empty(),
        "LinearGuidePort should add no own required members (dof inherited from \
         GuidePort), got: {:?}",
        t.required_members.iter().map(|r| &r.name).collect::<Vec<_>>()
    );

    assert_dof_eq_one_constraint("LinearGuidePort");
}

/// RotaryGuidePort refines exactly [GuidePort], pins the same single-DOF
/// invariant `degrees_of_freedom == 1` (a revolute guide permits exactly one
/// rotational DOF), and adds two required load ratings:
///   max_radial_load : Scalar<FORCE>
///   max_axial_load  : Scalar<FORCE>
///
/// RED: RotaryGuidePort is absent → find_trait panics.
#[test]
fn rotary_guide_port_trait_surface() {
    let t = find_trait("std/ports/mechanical", "RotaryGuidePort");

    assert_eq!(
        t.refinements.as_slice(),
        ["GuidePort".to_string()].as_slice(),
        "RotaryGuidePort should refine exactly [GuidePort], got: {:?}",
        t.refinements
    );

    assert_eq!(
        t.required_members.len(),
        2,
        "RotaryGuidePort should have exactly 2 required members \
         [max_radial_load, max_axial_load]; got: {:?}",
        t.required_members.iter().map(|r| &r.name).collect::<Vec<_>>()
    );
    for (i, name) in ["max_radial_load", "max_axial_load"].iter().enumerate() {
        assert_eq!(
            t.required_members[i].name, *name,
            "RotaryGuidePort required_members[{}] should be '{}'",
            i, name
        );
    }
    assert_eq!(
        param_type("std/ports/mechanical", "RotaryGuidePort", "max_radial_load"),
        Type::Scalar {
            dimension: DimensionVector::FORCE
        },
        "RotaryGuidePort.max_radial_load must be Scalar<FORCE>"
    );
    assert_eq!(
        param_type("std/ports/mechanical", "RotaryGuidePort", "max_axial_load"),
        Type::Scalar {
            dimension: DimensionVector::FORCE
        },
        "RotaryGuidePort.max_axial_load must be Scalar<FORCE>"
    );

    assert_dof_eq_one_constraint("RotaryGuidePort");
}

/// ThreadedPort refines exactly [MechanicalPort] with a single required member
/// `thread_spec : ThreadSpec` (Type::StructureRef("ThreadSpec")) — replacing the
/// old raw thread_diameter/pitch Length pair (PRD §4 decision 6).
///
/// RED: ThreadedPort still exposes [thread_diameter, pitch].
#[test]
fn threaded_port_trait_surface() {
    let t = find_trait("std/ports/mechanical", "ThreadedPort");

    assert_eq!(
        t.refinements.as_slice(),
        ["MechanicalPort".to_string()].as_slice(),
        "ThreadedPort should refine exactly [MechanicalPort], got: {:?}",
        t.refinements
    );

    assert_eq!(
        t.required_members.len(),
        1,
        "ThreadedPort should have exactly 1 required member (thread_spec); got: {:?}",
        t.required_members.iter().map(|r| &r.name).collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name, "thread_spec",
        "ThreadedPort required_members[0] should be 'thread_spec'"
    );
    assert_eq!(
        param_type("std/ports/mechanical", "ThreadedPort", "thread_spec"),
        Type::StructureRef("ThreadSpec".into()),
        "ThreadedPort.thread_spec must be Type::StructureRef(\"ThreadSpec\")"
    );

    // Regression guards: the raw thread_diameter/pitch pair was replaced.
    assert!(
        !t.required_members.iter().any(|r| r.name == "thread_diameter"),
        "ThreadedPort must not expose 'thread_diameter' (replaced by thread_spec)"
    );
    assert!(
        !t.required_members.iter().any(|r| r.name == "pitch"),
        "ThreadedPort must not expose 'pitch' (replaced by thread_spec)"
    );
}

// ─── step-1 (task β): mechanical thread enums surface ─────────────────────────

/// std/ports/mechanical declares the three thread-system enums with exact
/// variants in order:
///   ThreadSystem              == [ISO_Metric, ISO_Metric_Fine, UNC, UNF]
///   ThreadClass               == [Class_6g6H, Class_4g6H]
///   ThreadTighteningDirection == [Clockwise, Counterclockwise]
///
/// RED: the three enums are absent from the module (find_enum panics).
#[test]
fn mechanical_thread_enums_surface() {
    let thread_system = find_enum("std/ports/mechanical", "ThreadSystem");
    assert_eq!(
        thread_system.variants,
        vec![
            "ISO_Metric".to_string(),
            "ISO_Metric_Fine".to_string(),
            "UNC".to_string(),
            "UNF".to_string(),
        ],
        "ThreadSystem variants must be [ISO_Metric, ISO_Metric_Fine, UNC, UNF] in order; got: {:?}",
        thread_system.variants
    );

    let thread_class = find_enum("std/ports/mechanical", "ThreadClass");
    assert_eq!(
        thread_class.variants,
        vec!["Class_6g6H".to_string(), "Class_4g6H".to_string()],
        "ThreadClass variants must be [Class_6g6H, Class_4g6H] in order; got: {:?}",
        thread_class.variants
    );

    let tightening = find_enum("std/ports/mechanical", "ThreadTighteningDirection");
    assert_eq!(
        tightening.variants,
        vec!["Clockwise".to_string(), "Counterclockwise".to_string()],
        "ThreadTighteningDirection variants must be [Clockwise, Counterclockwise] in order; got: {:?}",
        tightening.variants
    );
}

// ─── step-3 (task β): ThreadSpec structure surface ───────────────────────────

/// std/ports/mechanical declares `structure def ThreadSpec` with exactly 6
/// Param cells (in order) and 4 derived Let cells (in order):
///
///   params: system : ThreadSystem
///           nominal_diameter : Length
///           pitch : Length
///           thread_class : ThreadClass
///           tightening : ThreadTighteningDirection = ThreadTighteningDirection.Clockwise
///           thread_form : Option<Geometry> = none
///   lets:   minor_diameter, pitch_diameter, tap_drill, clearance_hole
///           (each an arithmetic expr over nominal_diameter & pitch)
///
/// RED: ThreadSpec is absent → template lookup panics.
#[test]
fn thread_spec_structure_surface() {
    let module = load_module("std/ports/mechanical");

    let thread_spec = module
        .templates
        .iter()
        .find(|t| t.name == "ThreadSpec" && t.entity_kind == EntityKind::Structure)
        .expect(
            "std/ports/mechanical should contain 'structure def ThreadSpec'; \
             check ports_mechanical.ri for the ThreadSpec definition",
        );

    // ── Param cells: exact names, order, and types ───────────────────────────
    let param_cells: Vec<&ValueCellDecl> = thread_spec
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, ValueCellKind::Param))
        .collect();
    let param_names: Vec<&str> = param_cells.iter().map(|vc| vc.id.member.as_str()).collect();

    let expected_params: [(&str, Type); 6] = [
        ("system", Type::Enum("ThreadSystem".into())),
        (
            "nominal_diameter",
            Type::Scalar { dimension: DimensionVector::LENGTH },
        ),
        ("pitch", Type::Scalar { dimension: DimensionVector::LENGTH }),
        ("thread_class", Type::Enum("ThreadClass".into())),
        ("tightening", Type::Enum("ThreadTighteningDirection".into())),
        ("thread_form", Type::Option(Box::new(Type::Geometry))),
    ];

    assert_eq!(
        param_cells.len(),
        6,
        "ThreadSpec should have exactly 6 param cells \
         [system, nominal_diameter, pitch, thread_class, tightening, thread_form], got: {:?}",
        param_names
    );
    for (i, (name, ty)) in expected_params.iter().enumerate() {
        assert_eq!(
            param_cells[i].id.member, *name,
            "ThreadSpec param #{} should be '{}', got '{}'",
            i, name, param_cells[i].id.member
        );
        assert_eq!(
            param_cells[i].cell_type, *ty,
            "ThreadSpec.{} should be type {:?}, got {:?}",
            name, ty, param_cells[i].cell_type
        );
    }

    // ── tightening default = ThreadTighteningDirection.Clockwise ─────────────
    let tightening = param_cells
        .iter()
        .find(|vc| vc.id.member == "tightening")
        .unwrap();
    match &tightening
        .default_expr
        .as_ref()
        .expect(
            "ThreadSpec.tightening must have a default_expr \
             (= ThreadTighteningDirection.Clockwise)",
        )
        .kind
    {
        CompiledExprKind::Literal(Value::Enum { type_name, variant }) => {
            assert_eq!(
                type_name, "ThreadTighteningDirection",
                "tightening default enum type"
            );
            assert_eq!(variant, "Clockwise", "tightening default enum variant");
        }
        other => panic!(
            "ThreadSpec.tightening default should be Literal(Value::Enum {{ \
             type_name: \"ThreadTighteningDirection\", variant: \"Clockwise\" }}), got: {:?}",
            other
        ),
    }

    // ── thread_form default = none (OptionNone) ──────────────────────────────
    let thread_form = param_cells
        .iter()
        .find(|vc| vc.id.member == "thread_form")
        .unwrap();
    match &thread_form
        .default_expr
        .as_ref()
        .expect("ThreadSpec.thread_form must have a default_expr (= none)")
        .kind
    {
        CompiledExprKind::OptionNone => {}
        other => panic!(
            "ThreadSpec.thread_form default should be \
             CompiledExprKind::OptionNone (none), got: {:?}",
            other
        ),
    }

    // ── Let cells: exact names, order, each references nominal_diameter+pitch ─
    let let_cells: Vec<&ValueCellDecl> = thread_spec
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, ValueCellKind::Let))
        .collect();
    let let_names: Vec<&str> = let_cells.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        let_cells.len(),
        4,
        "ThreadSpec should have exactly 4 let cells \
         [minor_diameter, pitch_diameter, tap_drill, clearance_hole], got: {:?}",
        let_names
    );
    for (i, name) in ["minor_diameter", "pitch_diameter", "tap_drill", "clearance_hole"]
        .iter()
        .enumerate()
    {
        assert_eq!(
            let_cells[i].id.member, *name,
            "ThreadSpec let #{} should be '{}', got '{}'",
            i, name, let_cells[i].id.member
        );
        let rhs = let_cells[i]
            .default_expr
            .as_ref()
            .unwrap_or_else(|| panic!("ThreadSpec.{} (let) must have a RHS default_expr", name));
        let refs = collect_value_ref_members(rhs);
        assert!(
            refs.contains(&"nominal_diameter") && refs.contains(&"pitch"),
            "ThreadSpec.{} RHS must reference both 'nominal_diameter' and 'pitch', got refs: {:?}",
            name,
            refs
        );
    }
}

/// Pin the *exact* ISO coefficients baked into the ThreadSpec derived `let`s.
///
/// `thread_spec_structure_surface` only asserts each `let` RHS references
/// `nominal_diameter` and `pitch`; it would still pass if a coefficient drifted
/// (e.g. `pitch * 1.0825` → `pitch * 1.25`). This walks each RHS's compiled
/// expression tree and pins the literal coefficient against the 60°-flank
/// identities (ISO 68-1 / ISO 273):
///   minor_diameter = D − P·1.0825   (ISO 68-1 d1: D − 1.25H, H = 0.866P)
///   pitch_diameter = D − P·0.6495   (ISO 68-1 d2: D − 0.75H)
///   clearance_hole = D + P·0.5      (ISO 273 medium-fit approximation)
///   tap_drill      = D − P          (no coefficient — pure nominal − pitch)
///
/// Closes the test-coverage gap that a coefficient regression in
/// ports_mechanical.ri would otherwise slip past both this compile test and the
/// eval test (which exercises a locally re-declared copy of ThreadSpec).
#[test]
fn thread_spec_derived_let_coefficients_pinned() {
    let module = load_module("std/ports/mechanical");
    let thread_spec = module
        .templates
        .iter()
        .find(|t| t.name == "ThreadSpec" && t.entity_kind == EntityKind::Structure)
        .expect("std/ports/mechanical should contain 'structure def ThreadSpec'");

    // Coefficient-bearing lets: assert the exact ISO constant is a literal operand.
    let assert_coeff = |member: &str, expected: f64| {
        let cell = thread_spec
            .value_cells
            .iter()
            .find(|vc| matches!(vc.kind, ValueCellKind::Let) && vc.id.member == member)
            .unwrap_or_else(|| panic!("ThreadSpec should have a let cell '{}'", member));
        let rhs = cell
            .default_expr
            .as_ref()
            .unwrap_or_else(|| panic!("ThreadSpec.{} (let) must have a RHS default_expr", member));
        let lits = collect_number_literals(rhs);
        assert!(
            lits.iter().any(|v| (v - expected).abs() < 1e-12),
            "ThreadSpec.{} RHS must bake the ISO coefficient {} (regression guard against a \
             drifted thread-geometry constant); got literals: {:?}",
            member,
            expected,
            lits
        );
    };
    assert_coeff("minor_diameter", 1.0825);
    assert_coeff("pitch_diameter", 0.6495);
    assert_coeff("clearance_hole", 0.5);

    // tap_drill is pure `nominal_diameter − pitch` — no numeric coefficient at all.
    let tap_drill = thread_spec
        .value_cells
        .iter()
        .find(|vc| matches!(vc.kind, ValueCellKind::Let) && vc.id.member == "tap_drill")
        .expect("ThreadSpec should have a let cell 'tap_drill'");
    let tap_lits = collect_number_literals(
        tap_drill
            .default_expr
            .as_ref()
            .expect("ThreadSpec.tap_drill (let) must have a RHS default_expr"),
    );
    assert!(
        tap_lits.is_empty(),
        "ThreadSpec.tap_drill RHS should be pure `nominal_diameter − pitch` with no numeric \
         coefficient; got literals: {:?}",
        tap_lits
    );
}

/// std/ports/mechanical cardinality lock: exactly 6 traits (MechanicalPort,
/// Bore, Shaft, RotaryPort, ThreadedPort, StructurePort), 3 enums (ThreadSystem,
/// ThreadClass, ThreadTighteningDirection), 1 structure (ThreadSpec).
///
/// Bumped incrementally by task β steps (mirroring task α's in-file discipline):
///   step-1: enums 0→3 (ThreadSystem, ThreadClass, ThreadTighteningDirection)
///   step-3: structures 0→1 (ThreadSpec)
///   step-9: traits 6→8 (MotivePort, LinearPort)
///   step-11: traits 8→11 (GuidePort, LinearGuidePort, RotaryGuidePort)
#[test]
fn std_ports_mechanical_module_cardinality_locked() {
    let module = load_module("std/ports/mechanical");

    let enum_names: Vec<&str> = module.enum_defs.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        module.enum_defs.len(),
        3,
        "std/ports/mechanical should declare exactly 3 enums \
         (ThreadSystem, ThreadClass, ThreadTighteningDirection), got: {:?}",
        enum_names
    );
    for expected in &["ThreadSystem", "ThreadClass", "ThreadTighteningDirection"] {
        assert!(
            module.enum_defs.iter().any(|e| e.name == *expected),
            "std/ports/mechanical should contain enum '{}', got: {:?}",
            expected,
            enum_names
        );
    }

    let trait_names: Vec<&str> = module
        .trait_defs
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        module.trait_defs.len(),
        11,
        "std/ports/mechanical should declare exactly 11 traits (MechanicalPort, Bore, \
         Shaft, RotaryPort, ThreadedPort, StructurePort, MotivePort, LinearPort, \
         GuidePort, LinearGuidePort, RotaryGuidePort), got: {:?}",
        trait_names
    );
    for expected in &[
        "MechanicalPort",
        "Bore",
        "Shaft",
        "RotaryPort",
        "ThreadedPort",
        "StructurePort",
        "MotivePort",
        "LinearPort",
        "GuidePort",
        "LinearGuidePort",
        "RotaryGuidePort",
    ] {
        assert!(
            module.trait_defs.iter().any(|t| t.name == *expected),
            "std/ports/mechanical should contain trait '{}', got: {:?}",
            expected,
            trait_names
        );
    }

    let structure_names: Vec<&str> = module
        .templates
        .iter()
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        structure_names.len(),
        1,
        "std/ports/mechanical should declare exactly 1 structure (ThreadSpec), got: {:?}",
        structure_names
    );
    assert!(
        module
            .templates
            .iter()
            .any(|t| t.name == "ThreadSpec" && t.entity_kind == EntityKind::Structure),
        "std/ports/mechanical should contain 'ThreadSpec' structure, got: {:?}",
        structure_names
    );
}

// ─── step-1 (electrical): std/ports/electrical surface ───────────────────────

/// std/ports/electrical must load with zero Severity::Error diagnostics.
/// enum SignalKind has exactly [Analog, Digital, Differential] in that order.
/// ElectricalPort refines exactly [Port] with no own required members.
#[test]
fn std_ports_electrical_loads_with_no_errors_and_signal_kind_enum() {
    let module = load_module("std/ports/electrical");

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in ports_electrical.ri: {:?}",
        errors
    );

    let enum_def = find_enum("std/ports/electrical", "SignalKind");
    assert_eq!(
        enum_def.variants,
        vec![
            "Analog".to_string(),
            "Digital".to_string(),
            "PWM".to_string(),
            "Differential".to_string(),
        ],
        "SignalKind variants must be [Analog, Digital, PWM, Differential] in order; got: {:?}",
        enum_def.variants
    );

    let electrical_port = find_trait("std/ports/electrical", "ElectricalPort");
    assert_eq!(
        electrical_port.refinements.as_slice(),
        ["Port".to_string()].as_slice(),
        "ElectricalPort should refine exactly [Port], got: {:?}",
        electrical_port.refinements
    );
    assert_eq!(
        electrical_port.required_members.len(),
        2,
        "ElectricalPort should have exactly 2 required members \
         (voltage_rating, current_rating); got: {:?}",
        electrical_port
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        electrical_port.required_members[0].name,
        "voltage_rating",
        "ElectricalPort required_members[0] should be 'voltage_rating'"
    );
    assert_eq!(
        electrical_port.required_members[1].name,
        "current_rating",
        "ElectricalPort required_members[1] should be 'current_rating'"
    );
    assert_eq!(
        param_type("std/ports/electrical", "ElectricalPort", "voltage_rating"),
        Type::Scalar { dimension: DimensionVector::VOLTAGE },
        "ElectricalPort.voltage_rating must be Scalar<VOLTAGE>"
    );
    assert_eq!(
        param_type("std/ports/electrical", "ElectricalPort", "current_rating"),
        Type::Scalar { dimension: DimensionVector::CURRENT },
        "ElectricalPort.current_rating must be Scalar<CURRENT>"
    );
}

/// Behavioral: `SignalKind.PWM` must resolve as an enum-access default in user
/// source.  A structure that uses `SignalKind.PWM` as a param default must
/// compile with zero Severity::Error diagnostics.
///
/// RED: SignalKind currently lacks the PWM variant — the default expression
/// would resolve to Undef and emit an error.
#[test]
fn signal_kind_pwm_resolves_in_user_source() {
    let source = r#"
import std.ports.electrical

structure def PwmProbe {
    param k : SignalKind = SignalKind.PWM
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
        "structure using SignalKind.PWM as default should compile without errors; got: {:?}",
        errors
    );
}

/// PowerPort refines [ElectricalPort] with exactly 1 own required member:
/// `power_rating : Power`.  voltage_rating/current_rating are inherited from
/// ElectricalPort and must NOT appear in PowerPort.required_members.
///
/// RED: PowerPort currently has [voltage, max_current], not power_rating.
#[test]
fn power_port_trait_surface() {
    let t = find_trait("std/ports/electrical", "PowerPort");

    assert_eq!(
        t.refinements.as_slice(),
        ["ElectricalPort".to_string()].as_slice(),
        "PowerPort should refine exactly [ElectricalPort], got: {:?}",
        t.refinements
    );

    assert_eq!(
        t.required_members.len(),
        1,
        "PowerPort should have exactly 1 own required member (power_rating); got: {:?}",
        t.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name,
        "power_rating",
        "PowerPort required_members[0] should be 'power_rating'"
    );

    assert_eq!(
        param_type("std/ports/electrical", "PowerPort", "power_rating"),
        Type::Scalar {
            dimension: DimensionVector::POWER
        },
        "PowerPort.power_rating must be Scalar<POWER>"
    );

    // Regression guards: voltage/max_current moved to ElectricalPort.
    assert!(
        !t.required_members.iter().any(|r| r.name == "voltage"),
        "PowerPort must not have 'voltage' in own required_members \
         (it moved to ElectricalPort as voltage_rating)"
    );
    assert!(
        !t.required_members.iter().any(|r| r.name == "max_current"),
        "PowerPort must not have 'max_current' in own required_members \
         (it moved to ElectricalPort as current_rating)"
    );
}

/// SignalPort refines [ElectricalPort] with exactly 1 own required member:
/// `signal_kind : SignalKind`.  `impedance` is now optional (= none) and lives
/// in `defaults` as DefaultKind::Param with cell_type Option<Scalar<RESISTANCE>>.
///
/// impedance uses the Resistance/Ω dimension (no distinct Impedance named dim;
/// dimensionally identical — documented deviation in ports_electrical.ri header).
///
/// RED: impedance is currently a required Scalar<RESISTANCE> member.
#[test]
fn signal_port_trait_surface() {
    let t = find_trait("std/ports/electrical", "SignalPort");

    assert_eq!(
        t.refinements.as_slice(),
        ["ElectricalPort".to_string()].as_slice(),
        "SignalPort should refine exactly [ElectricalPort], got: {:?}",
        t.refinements
    );

    // Only signal_kind is required; impedance has a default (= none).
    assert_eq!(
        t.required_members.len(),
        1,
        "SignalPort should have exactly 1 required member (signal_kind); got: {:?}",
        t.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name,
        "signal_kind",
        "SignalPort required_members[0] should be 'signal_kind'"
    );
    assert_eq!(
        param_type("std/ports/electrical", "SignalPort", "signal_kind"),
        Type::Enum("SignalKind".into()),
        "SignalPort.signal_kind must be Type::Enum(\"SignalKind\")"
    );

    // impedance must NOT be in required_members.
    assert!(
        !t.required_members.iter().any(|r| r.name == "impedance"),
        "SignalPort.impedance must be absent from required_members \
         (it is now Option<Resistance> = none); got required: {:?}",
        t.required_members.iter().map(|r| &r.name).collect::<Vec<_>>()
    );

    // impedance must be in defaults as DefaultKind::Param with Option<Resistance>.
    let imp_default = t
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("impedance"))
        .expect("SignalPort.defaults should contain an entry named 'impedance' (= none)");

    match &imp_default.kind {
        DefaultKind::Param { cell_type, default_decl } => {
            assert_eq!(
                *cell_type,
                Type::Option(Box::new(Type::Scalar { dimension: DimensionVector::RESISTANCE })),
                "SignalPort.impedance default cell_type must be \
                 Type::Option(Scalar<RESISTANCE>)"
            );
            assert!(
                default_decl.default.is_some(),
                "SignalPort.impedance default_decl must have a default expression (none)"
            );
        }
        other => panic!(
            "SignalPort.impedance default must be DefaultKind::Param, got: {:?}",
            other
        ),
    }
}

/// PinPort refines exactly [ElectricalPort, LocatedPort] and has exactly 1 own
/// required member: `pin_id : String`.
///
/// RED: PinPort is absent — find_trait panics; cardinality is 3.
#[test]
fn pin_port_trait_surface() {
    let t = find_trait("std/ports/electrical", "PinPort");

    assert_eq!(
        t.refinements.len(),
        2,
        "PinPort should refine exactly 2 supertraits \
         (ElectricalPort, LocatedPort), got: {:?}",
        t.refinements
    );
    assert!(
        t.refinements.contains(&"ElectricalPort".to_string()),
        "PinPort refinements should contain 'ElectricalPort', got: {:?}",
        t.refinements
    );
    assert!(
        t.refinements.contains(&"LocatedPort".to_string()),
        "PinPort refinements should contain 'LocatedPort', got: {:?}",
        t.refinements
    );

    assert_eq!(
        t.required_members.len(),
        1,
        "PinPort should have exactly 1 own required member (pin_id); got: {:?}",
        t.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name,
        "pin_id",
        "PinPort required_members[0] should be 'pin_id'"
    );
    assert_eq!(
        param_type("std/ports/electrical", "PinPort", "pin_id"),
        Type::String,
        "PinPort.pin_id must be Type::String"
    );
}

/// Behavioral: `PinPort` must be usable as a generic trait bound in user source.
/// A generic structure `PinHeader<P: PinPort>` must compile with zero
/// Severity::Error diagnostics.
///
/// RED: PinPort is absent → compile error on unknown trait bound.
#[test]
fn pin_port_usable_as_trait_bound() {
    let source = r#"
import std.ports.electrical

structure def PinHeader<P: PinPort> {
    port pin : P
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
        "generic structure PinHeader<P: PinPort> should compile without errors; got: {:?}",
        errors
    );

    assert!(
        compiled
            .templates
            .iter()
            .any(|t| t.name == "PinHeader" && t.entity_kind == EntityKind::Structure),
        "PinHeader should be declared as a Structure template; found: {:?}",
        compiled
            .templates
            .iter()
            .map(|t| (&t.name, &t.entity_kind))
            .collect::<Vec<_>>()
    );
}

/// Behavioral: a concrete conformer that supplies ALL inherited+own members of
/// PinPort (the diamond hierarchy) must compile with zero Severity::Error diagnostics.
///
/// PinPort's inheritance diamond:
///
///         Port  (direction : Directionality = Bidi — defaulted, can omit)
///        /    \
///  ElectrPort  LocatedPort
///   (voltage_  (frame : Frame3)
///    current_)
///        \    /
///        PinPort
///         (pin_id : String)
///
/// Required supplied here:
///   voltage_rating : Voltage  (from ElectricalPort)
///   current_rating : Current  (from ElectricalPort)
///   frame          : Frame3   (from LocatedPort)
///   pin_id         : String   (own PinPort)
///   direction                 (omitted — Port default = Directionality.Bidi)
///
/// A diamond-merge bug in inherited-member resolution that still leaves the
/// trait declarable would not be caught by the trait-surface or bound-usage
/// tests alone.  This conformer drives the full merged requirement set through
/// the conformance-checking path.
#[test]
fn pin_port_concrete_conformer_diamond_merge_compiles() {
    let source = r#"
import std.ports.electrical

structure def PinConformer {
    port p : in PinPort {
        param voltage_rating : Voltage = 3.3V
        param current_rating : Current = 0.1A
        param frame : Frame3 = Frame3(
            origin: vec3(0mm, 0mm, 0mm),
            x_axis: vec3(1mm, 0mm, 0mm),
            y_axis: vec3(0mm, 1mm, 0mm),
            z_axis: vec3(0mm, 0mm, 1mm),
        )
        param pin_id : String = "A1"
    }
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
        "PinPort concrete conformer (diamond: ElectricalPort+LocatedPort both refine Port) \
         should compile without errors when all inherited+own params are supplied; got: {:?}",
        errors
    );

    assert!(
        compiled
            .templates
            .iter()
            .any(|t| t.name == "PinConformer" && t.entity_kind == EntityKind::Structure),
        "PinConformer should be declared as a Structure template; found: {:?}",
        compiled
            .templates
            .iter()
            .map(|t| (&t.name, &t.entity_kind))
            .collect::<Vec<_>>()
    );
}

/// std/ports/electrical cardinality lock: exactly 4 traits (ElectricalPort,
/// PowerPort, SignalPort, PinPort), 1 enum (SignalKind), 0 structures.
///
/// RED: PinPort absent → trait count is 3.
#[test]
fn std_ports_electrical_module_cardinality_locked() {
    let module = load_module("std/ports/electrical");

    let enum_names: Vec<&str> = module.enum_defs.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        module.enum_defs.len(),
        1,
        "std/ports/electrical should declare exactly 1 enum (SignalKind), got: {:?}",
        enum_names
    );
    assert!(
        module.enum_defs.iter().any(|e| e.name == "SignalKind"),
        "std/ports/electrical should contain the 'SignalKind' enum, got: {:?}",
        enum_names
    );

    let trait_names: Vec<&str> = module
        .trait_defs
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        module.trait_defs.len(),
        4,
        "std/ports/electrical should declare exactly 4 traits \
         (ElectricalPort, PowerPort, SignalPort, PinPort), got: {:?}",
        trait_names
    );
    for expected in &["ElectricalPort", "PowerPort", "SignalPort", "PinPort"] {
        assert!(
            module.trait_defs.iter().any(|t| t.name == *expected),
            "std/ports/electrical should contain trait '{}', got: {:?}",
            expected,
            trait_names
        );
    }

    let structure_names: Vec<&str> = module
        .templates
        .iter()
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        structure_names.len(),
        0,
        "std/ports/electrical should declare 0 structures, got: {:?}",
        structure_names
    );
}

// ─── step-3 (thermal): std/ports/thermal surface ─────────────────────────────

/// std/ports/thermal must load with zero Severity::Error diagnostics.
/// After task ε expansion:
///   - ThermalPort refines exactly [Port]
///   - required_members == [heat_flow] (len 1; only the required through-variable)
///   - temperature, heat_flux, thermal_resistance are OPTIONAL (= none) in defaults
///
/// Extended Modelica HeatPort convention: temperature (optional potential across
/// variable, K) + heat_flow (required through variable Q̇, W) + heat_flux
/// (optional W/m² surface flux, HeatFlux alias = Power/Area) + thermal_resistance
/// (optional K/W, ThermalResistance alias = Temperature/Power).
/// See plan design decisions: heat_flow stays required; temperature gains optionality.
#[test]
fn std_ports_thermal_loads_with_no_errors_and_thermal_port_trait() {
    let module = load_module("std/ports/thermal");

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in ports_thermal.ri: {:?}",
        errors
    );

    let thermal_port = find_trait("std/ports/thermal", "ThermalPort");
    assert_eq!(
        thermal_port.refinements.as_slice(),
        ["Port".to_string()].as_slice(),
        "ThermalPort should refine exactly [Port], got: {:?}",
        thermal_port.refinements
    );

    // heat_flow is the only required through-variable (no default).
    assert_eq!(
        thermal_port.required_members.len(),
        1,
        "ThermalPort should have exactly 1 required member (heat_flow); got: {:?}",
        thermal_port
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        thermal_port.required_members[0].name,
        "heat_flow",
        "ThermalPort required_members[0] should be 'heat_flow'"
    );
    assert_eq!(
        param_type("std/ports/thermal", "ThermalPort", "heat_flow"),
        Type::Scalar {
            dimension: DimensionVector::POWER
        },
        "ThermalPort.heat_flow must be Scalar<POWER>"
    );

    // temperature : Option<Temperature> = none
    let temp_default = thermal_port
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("temperature"))
        .expect("ThermalPort.defaults should contain 'temperature' (= none)");
    match &temp_default.kind {
        DefaultKind::Param { cell_type, default_decl } => {
            assert_eq!(
                *cell_type,
                Type::Option(Box::new(Type::Scalar {
                    dimension: DimensionVector::TEMPERATURE
                })),
                "ThermalPort.temperature default cell_type must be \
                 Type::Option(Scalar<TEMPERATURE>)"
            );
            assert!(
                default_decl.default.is_some(),
                "ThermalPort.temperature default_decl must have a default expression (none)"
            );
        }
        other => panic!(
            "ThermalPort.temperature default must be DefaultKind::Param, got: {:?}",
            other
        ),
    }

    // heat_flux : Option<HeatFlux> = none  (HeatFlux = Power / Area)
    let expected_heat_flux_dim = DimensionVector::POWER.div(&DimensionVector::AREA);
    let heat_flux_default = thermal_port
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("heat_flux"))
        .expect("ThermalPort.defaults should contain 'heat_flux' (= none)");
    match &heat_flux_default.kind {
        DefaultKind::Param { cell_type, default_decl } => {
            assert_eq!(
                *cell_type,
                Type::Option(Box::new(Type::Scalar {
                    dimension: expected_heat_flux_dim
                })),
                "ThermalPort.heat_flux default cell_type must be \
                 Type::Option(Scalar<POWER/AREA>) (HeatFlux alias = Power/Area)"
            );
            assert!(
                default_decl.default.is_some(),
                "ThermalPort.heat_flux default_decl must have a default expression (none)"
            );
        }
        other => panic!(
            "ThermalPort.heat_flux default must be DefaultKind::Param, got: {:?}",
            other
        ),
    }

    // thermal_resistance : Option<ThermalResistance> = none  (ThermalResistance = Temperature / Power)
    let expected_thermal_resistance_dim =
        DimensionVector::TEMPERATURE.div(&DimensionVector::POWER);
    let thermal_resistance_default = thermal_port
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("thermal_resistance"))
        .expect("ThermalPort.defaults should contain 'thermal_resistance' (= none)");
    match &thermal_resistance_default.kind {
        DefaultKind::Param { cell_type, default_decl } => {
            assert_eq!(
                *cell_type,
                Type::Option(Box::new(Type::Scalar {
                    dimension: expected_thermal_resistance_dim
                })),
                "ThermalPort.thermal_resistance default cell_type must be \
                 Type::Option(Scalar<TEMPERATURE/POWER>) (ThermalResistance alias = \
                 Temperature/Power)"
            );
            assert!(
                default_decl.default.is_some(),
                "ThermalPort.thermal_resistance default_decl must have a \
                 default expression (none)"
            );
        }
        other => panic!(
            "ThermalPort.thermal_resistance default must be DefaultKind::Param, got: {:?}",
            other
        ),
    }
}

/// std/ports/thermal cardinality lock: exactly 2 traits (ThermalPort +
/// ThermalContactPort), 2 type aliases (HeatFlux + ThermalResistance),
/// 0 enums, 0 structures.
///
/// Type-alias count is asserted so that spuriously added aliases are caught
/// (consistent with locking the module's full public surface).
#[test]
fn std_ports_thermal_module_cardinality_locked() {
    let module = load_module("std/ports/thermal");

    assert_eq!(
        module.enum_defs.len(),
        0,
        "std/ports/thermal should declare 0 enums, got: {:?}",
        module
            .enum_defs
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>()
    );

    let trait_names: Vec<&str> = module
        .trait_defs
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        module.trait_defs.len(),
        2,
        "std/ports/thermal should declare exactly 2 traits \
         (ThermalPort + ThermalContactPort), got: {:?}",
        trait_names
    );
    assert!(
        trait_names.contains(&"ThermalPort"),
        "std/ports/thermal trait list should contain 'ThermalPort', got: {:?}",
        trait_names
    );
    assert!(
        trait_names.contains(&"ThermalContactPort"),
        "std/ports/thermal trait list should contain 'ThermalContactPort', got: {:?}",
        trait_names
    );

    let alias_names: Vec<&str> = module
        .type_aliases
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    assert_eq!(
        module.type_aliases.len(),
        2,
        "std/ports/thermal should declare exactly 2 type aliases \
         (HeatFlux + ThermalResistance), got: {:?}",
        alias_names
    );
    assert!(
        alias_names.contains(&"HeatFlux"),
        "std/ports/thermal alias list should contain 'HeatFlux', got: {:?}",
        alias_names
    );
    assert!(
        alias_names.contains(&"ThermalResistance"),
        "std/ports/thermal alias list should contain 'ThermalResistance', got: {:?}",
        alias_names
    );

    let structure_names: Vec<&str> = module
        .templates
        .iter()
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        structure_names.len(),
        0,
        "std/ports/thermal should declare 0 structures, got: {:?}",
        structure_names
    );
}

// ─── step-3b (thermal): ThermalContactPort trait surface ──────────────────────

/// ThermalContactPort refines ThermalPort + RegionPort (multi-supertrait).
/// Own required_members == [contact_area] (len 1); inherited members excluded.
/// contact_conductance : Option<ThermalConductivity> = none in defaults.
///
/// Refinement order is parser-free — asserted by containment + length, not by
/// exact ordered slice (multi-supertrait precedent: Watertight : Closed + Manifold
/// in geometry_traits_tests.rs:31-54).
///
/// Proves: ThermalContactPort resolves as a port-type with both ThermalPort and
/// RegionPort in its inheritance chain, and alias-inside-Option in param position
/// works for ThermalConductivity.
///
/// RED on current main (ThermalContactPort absent → find_trait panics).
#[test]
fn thermal_contact_port_trait_surface() {
    let t = find_trait("std/ports/thermal", "ThermalContactPort");

    // Multi-supertrait refinements: order is parser-free; assert by containment.
    assert_eq!(
        t.refinements.len(),
        2,
        "ThermalContactPort should have exactly 2 refinements \
         (ThermalPort, RegionPort), got: {:?}",
        t.refinements
    );
    assert!(
        t.refinements.contains(&"ThermalPort".to_string()),
        "ThermalContactPort.refinements should contain 'ThermalPort', got: {:?}",
        t.refinements
    );
    assert!(
        t.refinements.contains(&"RegionPort".to_string()),
        "ThermalContactPort.refinements should contain 'RegionPort', got: {:?}",
        t.refinements
    );

    // OWN required member: contact_area : Area (no default).
    // Inherited region/frame/heat_flow are NOT in own required_members.
    assert_eq!(
        t.required_members.len(),
        1,
        "ThermalContactPort should have exactly 1 own required member \
         (contact_area); got: {:?}",
        t.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name,
        "contact_area",
        "ThermalContactPort required_members[0] should be 'contact_area'"
    );
    assert_eq!(
        param_type("std/ports/thermal", "ThermalContactPort", "contact_area"),
        Type::Scalar {
            dimension: DimensionVector::AREA
        },
        "ThermalContactPort.contact_area must be Scalar<AREA>"
    );

    // contact_conductance : Option<ThermalConductivity> = none
    let contact_cond = t
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("contact_conductance"))
        .expect(
            "ThermalContactPort.defaults should contain 'contact_conductance' (= none)"
        );
    match &contact_cond.kind {
        DefaultKind::Param { cell_type, default_decl } => {
            assert_eq!(
                *cell_type,
                Type::Option(Box::new(Type::Scalar {
                    dimension: DimensionVector::THERMAL_CONDUCTIVITY
                })),
                "ThermalContactPort.contact_conductance default cell_type must be \
                 Type::Option(Scalar<THERMAL_CONDUCTIVITY>)"
            );
            assert!(
                default_decl.default.is_some(),
                "ThermalContactPort.contact_conductance default_decl must have \
                 a default expression (none)"
            );
        }
        other => panic!(
            "ThermalContactPort.contact_conductance default must be \
             DefaultKind::Param, got: {:?}",
            other
        ),
    }
}

// ─── step-3c (thermal): behavioral compile signal ──────────────────────────────

/// Inline compile test: a structure parameterized on ThermalContactPort compiles
/// clean. This is the headline behavioral signal — proves ThermalContactPort
/// resolves as a port-type/trait-bound from user source (imports std.ports.thermal),
/// with its ThermalPort and RegionPort supertrait chains fully resolved.
///
/// No concrete conformer is instantiated (mirroring the ActuatorInterface<…>
/// type-param-bound example test and task α's inline RegionPort tests).
///
/// RED on current main (ThermalContactPort absent → trait-bound unresolved →
/// Severity::Error diagnostics).
#[test]
fn reify_check_accepts_thermal_contact_port() {
    let source = "\
import std.ports.thermal
structure def ContactInterface<C: ThermalContactPort> { port contact : C }
";
    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "ContactInterface<C: ThermalContactPort> should compile without errors; \
         got: {:?}",
        errors
    );
}

// ─── amend (thermal): ThermalContactPort concrete conformer ──────────────────

/// Conformer instantiation: a concrete structure that satisfies every required
/// member of ThermalContactPort compiles clean.
///
/// Required members supplied:
///   frame         : Frame3    (from LocatedPort)  — zero-origin identity frame
///   region        : Geometry  (from RegionPort)   — box geometry (Geometry alias
///                                                    == Solid, box() returns Geometry)
///   heat_flow     : Power     (from ThermalPort)  — 1N * 1m / 1s (= 1 W)
///   contact_area  : Area      (own ThermalContactPort) — 1m * 1m (= 1 m²)
///
/// Optional members (temperature, heat_flux, thermal_resistance,
/// contact_conductance) are intentionally omitted — they default to none and
/// must NOT be required by the conformance checker.
///
/// Complements `reify_check_accepts_thermal_contact_port` (which only checks
/// that the trait bound resolves with no errors in a generic context) by
/// driving the strict `structure def X : Trait` conformance checker against
/// the full inherited + own member set.  A structural change that broke
/// conformance while keeping the bound resolvable would be caught here.
///
/// Mirrors the `rotary_port_concrete_conformer_compiles` pattern (~line 701).
#[test]
fn thermal_contact_port_concrete_conformer_compiles() {
    let source = r#"
import std.ports.thermal

structure def ContactPatch : ThermalContactPort {
    param frame : Frame3 = Frame3(
        origin: vec3(0mm, 0mm, 0mm),
        x_axis: vec3(1mm, 0mm, 0mm),
        y_axis: vec3(0mm, 1mm, 0mm),
        z_axis: vec3(0mm, 0mm, 1mm),
    )
    param region : Geometry = box(10mm, 10mm, 1mm)
    param heat_flow : Power = 1N * 1m / 1s
    param contact_area : Area = 1m * 1m
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
        "a structure conforming to ThermalContactPort and supplying \
         frame/region/heat_flow/contact_area should compile without errors; \
         got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── step-5 (fluid): std/ports/fluid surface ──────────────────────────────────

/// std/ports/fluid must load with zero Severity::Error diagnostics.
/// FluidPort refines exactly [Port] with required members [pressure, flow_rate,
/// medium] in order, resolving to:
///   pressure  : Scalar<PRESSURE>
///   flow_rate : Scalar<VOLUME/TIME>   (via VolumetricFlowRate alias)
///   medium    : String
///
/// `flow_rate` is asserted via DimensionVector::VOLUME.div(&DimensionVector::TIME)
/// because the `VolumetricFlowRate = Volume / Time` alias resolves to that
/// composite dimension at compile time — exactly as Torque = Force*Length/Angle
/// resolves in the rotary_port_trait_surface test.
#[test]
fn std_ports_fluid_loads_with_no_errors_and_fluid_port_trait() {
    let module = load_module("std/ports/fluid");

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in ports_fluid.ri: {:?}",
        errors
    );

    let fluid_port = find_trait("std/ports/fluid", "FluidPort");
    assert_eq!(
        fluid_port.refinements.as_slice(),
        ["Port".to_string()].as_slice(),
        "FluidPort should refine exactly [Port], got: {:?}",
        fluid_port.refinements
    );

    assert_eq!(
        fluid_port.required_members.len(),
        4,
        "FluidPort should have exactly 4 required members; got: {:?}",
        fluid_port
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        fluid_port.required_members[0].name,
        "pressure",
        "FluidPort required_members[0] should be 'pressure'"
    );
    assert_eq!(
        fluid_port.required_members[1].name,
        "flow_rate",
        "FluidPort required_members[1] should be 'flow_rate'"
    );
    assert_eq!(
        fluid_port.required_members[2].name,
        "medium",
        "FluidPort required_members[2] should be 'medium'"
    );
    assert_eq!(
        fluid_port.required_members[3].name,
        "fluid_type",
        "FluidPort required_members[3] should be 'fluid_type'"
    );

    assert_eq!(
        param_type("std/ports/fluid", "FluidPort", "pressure"),
        Type::Scalar {
            dimension: DimensionVector::PRESSURE
        },
        "FluidPort.pressure must be Scalar<PRESSURE>"
    );

    // VolumetricFlowRate = Volume / Time; alias resolves to the composite dimension.
    let expected_flow_rate_dim = DimensionVector::VOLUME.div(&DimensionVector::TIME);
    assert_eq!(
        param_type("std/ports/fluid", "FluidPort", "flow_rate"),
        Type::Scalar {
            dimension: expected_flow_rate_dim
        },
        "FluidPort.flow_rate must be Scalar<VOLUME/TIME> \
         (VolumetricFlowRate alias — alias indirection required for binary dim-op; \
         see ports_fluid.ri header deviation)"
    );

    assert_eq!(
        param_type("std/ports/fluid", "FluidPort", "medium"),
        Type::String,
        "FluidPort.medium must be Type::String \
         (open medium set; free-form identifier per io.ri precedent)"
    );

    assert_eq!(
        param_type("std/ports/fluid", "FluidPort", "fluid_type"),
        Type::Enum("FluidType".into()),
        "FluidPort.fluid_type must be Type::Enum(\"FluidType\")"
    );
}

/// std/ports/fluid cardinality lock: exactly 2 enums (FluidType, PipeConnectionType),
/// 2 traits (FluidPort, PipedFluidPort), 0 structures.
///
/// Updated incrementally per task ζ step-3: PipeConnectionType + PipedFluidPort added.
#[test]
fn std_ports_fluid_module_cardinality_locked() {
    let module = load_module("std/ports/fluid");

    let enum_names: Vec<&str> = module.enum_defs.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        module.enum_defs.len(),
        2,
        "std/ports/fluid should declare exactly 2 enums (FluidType, PipeConnectionType), got: {:?}",
        enum_names
    );
    assert!(
        enum_names.contains(&"FluidType"),
        "std/ports/fluid enum_defs should contain 'FluidType'; got: {:?}",
        enum_names
    );
    assert!(
        enum_names.contains(&"PipeConnectionType"),
        "std/ports/fluid enum_defs should contain 'PipeConnectionType'; got: {:?}",
        enum_names
    );

    let trait_names: Vec<&str> = module
        .trait_defs
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        module.trait_defs.len(),
        2,
        "std/ports/fluid should declare exactly 2 traits (FluidPort, PipedFluidPort), got: {:?}",
        trait_names
    );
    assert!(
        trait_names.contains(&"FluidPort"),
        "std/ports/fluid trait_defs should contain 'FluidPort'; got: {:?}",
        trait_names
    );
    assert!(
        trait_names.contains(&"PipedFluidPort"),
        "std/ports/fluid trait_defs should contain 'PipedFluidPort'; got: {:?}",
        trait_names
    );

    let structure_names: Vec<&str> = module
        .templates
        .iter()
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        structure_names.len(),
        0,
        "std/ports/fluid should declare 0 structures, got: {:?}",
        structure_names
    );
}

// ─── task ζ step-1: FluidType enum surface + behavioral resolve ──────────────

/// FluidType enum must declare exactly 3 variants in order:
/// [Liquid, Gas, TwoPhase].
///
/// RED: FluidType is absent → find_enum panics.
#[test]
fn fluid_type_enum_surface() {
    let e = find_enum("std/ports/fluid", "FluidType");
    assert_eq!(
        e.variants.as_slice(),
        ["Liquid", "Gas", "TwoPhase"].as_slice(),
        "FluidType variants should be [Liquid, Gas, TwoPhase] in order; got: {:?}",
        e.variants
    );
}

/// Behavioral: a structure that uses `FluidType.Liquid` as a param default
/// must compile with zero Severity::Error diagnostics after `import std.ports.fluid`.
///
/// This is the PRD §7 ζ signal: "FluidType.Liquid resolves (previously Undef)."
///
/// RED: FluidType absent → enum variant lookup returns Undef → compile error.
#[test]
fn fluid_type_liquid_resolves_in_user_source() {
    let source = r#"
import std.ports.fluid

structure def FluidProbe {
    param k : FluidType = FluidType.Liquid
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
        "structure using FluidType.Liquid as default should compile without errors; got: {:?}",
        errors
    );
}

// ─── task ζ step-3: PipeConnectionType enum + PipedFluidPort trait surface ────

/// PipeConnectionType enum must declare exactly 5 variants in order:
/// [Threaded, Flanged, Compression, PushFit, Welded].
///
/// RED: PipeConnectionType is absent → find_enum panics.
#[test]
fn pipe_connection_type_enum_surface() {
    let e = find_enum("std/ports/fluid", "PipeConnectionType");
    assert_eq!(
        e.variants.as_slice(),
        ["Threaded", "Flanged", "Compression", "PushFit", "Welded"].as_slice(),
        "PipeConnectionType variants should be \
         [Threaded, Flanged, Compression, PushFit, Welded] in order; got: {:?}",
        e.variants
    );
}

/// PipedFluidPort refines exactly [FluidPort, LocatedPort] (multi-supertrait,
/// order not guaranteed) and has exactly 2 own required members:
/// inner_diameter : Length, connection_type : PipeConnectionType.
///
/// RED: PipedFluidPort is absent → find_trait panics.
#[test]
fn piped_fluid_port_trait_surface() {
    let t = find_trait("std/ports/fluid", "PipedFluidPort");

    assert_eq!(
        t.refinements.len(),
        2,
        "PipedFluidPort should refine exactly 2 supertraits \
         (FluidPort, LocatedPort), got: {:?}",
        t.refinements
    );
    assert!(
        t.refinements.contains(&"FluidPort".to_string()),
        "PipedFluidPort refinements should contain 'FluidPort', got: {:?}",
        t.refinements
    );
    assert!(
        t.refinements.contains(&"LocatedPort".to_string()),
        "PipedFluidPort refinements should contain 'LocatedPort', got: {:?}",
        t.refinements
    );

    assert_eq!(
        t.required_members.len(),
        2,
        "PipedFluidPort should have exactly 2 own required members \
         (inner_diameter, connection_type); got: {:?}",
        t.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    assert_eq!(
        param_type("std/ports/fluid", "PipedFluidPort", "inner_diameter"),
        Type::Scalar {
            dimension: DimensionVector::LENGTH
        },
        "PipedFluidPort.inner_diameter must be Scalar<LENGTH>"
    );
    assert_eq!(
        param_type("std/ports/fluid", "PipedFluidPort", "connection_type"),
        Type::Enum("PipeConnectionType".into()),
        "PipedFluidPort.connection_type must be Type::Enum(\"PipeConnectionType\")"
    );
}

/// Behavioral: a concrete port that conforms to PipedFluidPort must compile with
/// zero Severity::Error diagnostics when ALL inherited + own params are supplied.
///
/// PipedFluidPort diamond:
///
///          Port  (direction : Directionality = Bidi — defaulted, can omit)
///         /    \
///   FluidPort  LocatedPort
///   (pressure,  (frame : Frame3)
///    flow_rate,
///    medium,
///    fluid_type)
///         \    /
///       PipedFluidPort
///       (inner_diameter, connection_type)
///
/// This is the PRD §7 ζ signal: "reify check accepts a PipedFluidPort
/// (connection_type: PipeConnectionType.Threaded)".
///
/// RED: PipedFluidPort absent → compile error on unknown trait.
#[test]
fn piped_fluid_port_concrete_conformer_diamond_merge_compiles() {
    // flow_rate uses `1gal / 1s` (gal is the Volume unit declared in units.ri;
    // no m³ SI unit is currently generated — see units.ri §Volume comment).
    // Enum-typed params (fluid_type, connection_type) omit the type annotation:
    // the compiler resolves enum types from the prelude in struct-param position
    // but the port-param type-annotation path doesn't look up prelude enums
    // (port param values are inferred from the provided literal instead).
    let source = r#"
import std.ports.fluid

structure def PipeConformer {
    port p : in PipedFluidPort {
        param pressure : Pressure = 101325Pa
        param flow_rate : VolumetricFlowRate = 1gal / 1s
        param medium : String = "water"
        param fluid_type = FluidType.Liquid
        param frame : Frame3 = Frame3(
            origin: vec3(0mm, 0mm, 0mm),
            x_axis: vec3(1mm, 0mm, 0mm),
            y_axis: vec3(0mm, 1mm, 0mm),
            z_axis: vec3(0mm, 0mm, 1mm),
        )
        param inner_diameter : Length = 25mm
        param connection_type = PipeConnectionType.Threaded
    }
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
        "PipedFluidPort concrete conformer (diamond: FluidPort+LocatedPort both refine Port) \
         should compile without errors when all inherited+own params are supplied; got: {:?}",
        errors
    );

    assert!(
        compiled
            .templates
            .iter()
            .any(|t| t.name == "PipeConformer" && t.entity_kind == EntityKind::Structure),
        "PipeConformer should be declared as a Structure template; found: {:?}",
        compiled
            .templates
            .iter()
            .map(|t| (&t.name, &t.entity_kind))
            .collect::<Vec<_>>()
    );
}

// ─── step-7: capstone example (ports_domains) ────────────────────────────────

/// examples/stdlib/ports_domains.ri must compile without errors and
/// structurally declare a template named "ActuatorInterface" of
/// EntityKind::Structure.
///
/// The example imports std.ports.electrical, std.ports.thermal, and
/// std.ports.fluid; declares
///   `structure def ActuatorInterface<P: PowerPort, T: ThermalPort, F: FluidPort>`
/// with three ports.  No concrete conformer is instantiated (PRD §4 decision 4;
/// mirrors Coupling in examples/stdlib/ports_mechanical.ri).
#[test]
fn example_ports_domains_ri_compiles_clean() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let example_path = manifest_dir
        .join("../../examples/stdlib/ports_domains.ri")
        .canonicalize()
        .expect("examples/stdlib/ports_domains.ri should exist on disk");

    let source = std::fs::read_to_string(&example_path)
        .expect("failed to read examples/stdlib/ports_domains.ri");

    let compiled = compile_source_with_stdlib(&source);

    let example_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        example_errors.is_empty(),
        "examples/stdlib/ports_domains.ri should compile without errors; got: {:?}",
        example_errors
    );

    assert!(
        compiled.templates.iter().any(|t| {
            t.name == "ActuatorInterface" && t.entity_kind == EntityKind::Structure
        }),
        "examples/stdlib/ports_domains.ri should declare \
         'structure def ActuatorInterface<P: PowerPort, T: ThermalPort, F: FluidPort>'; \
         found templates: {:?}",
        compiled
            .templates
            .iter()
            .map(|t| (&t.name, &t.entity_kind))
            .collect::<Vec<_>>()
    );
}

// ─── step-7 (task α): RegionPort trait surface + asymmetric-LocatedPort warning ─

/// RegionPort refines exactly ["LocatedPort"] and has exactly one required member:
/// `region : Geometry`.  Proves dep-4253 Geometry alias resolves in param position.
///
///   - RegionPort.refinements == ["LocatedPort"]
///   - RegionPort.required_members == [{ name: "region", kind: Param(Type::Geometry) }]
///   - param_type helper finds "region" as Type::Geometry
///
/// RED on current main (RegionPort absent → find_trait panics).
#[test]
fn region_port_trait_surface() {
    let t = find_trait("std/ports", "RegionPort");

    assert_eq!(
        t.refinements.as_slice(),
        ["LocatedPort".to_string()].as_slice(),
        "RegionPort should refine exactly [LocatedPort], got: {:?}",
        t.refinements
    );

    assert_eq!(
        t.required_members.len(),
        1,
        "RegionPort should have exactly 1 required member (region); got: {:?}",
        t.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name, "region",
        "RegionPort required_members[0] should be 'region', got '{}'",
        t.required_members[0].name
    );

    assert_eq!(
        param_type("std/ports", "RegionPort", "region"),
        Type::Geometry,
        "RegionPort.region must be Type::Geometry \
         (Geometry alias from dep 4253 / task G)"
    );
}

/// Proves that a `RegionPort` → `Port` connection fires the asymmetric-LocatedPort
/// warning from connect.rs when the stdlib RegionPort trait is in the prelude
/// trait registry.
///
/// Rationale for using RegionPort (not LocatedPort directly): `trait_satisfies`
/// (entity.rs:3747) is REFLEXIVE — a port typed `LocatedPort` matches by name
/// alone, even without the stdlib trait.  `RegionPort` satisfies LocatedPort ONLY
/// via its declared refinement chain (RegionPort→LocatedPort), which requires
/// RegionPort to be in the prelude registry (build_trait_registry merges prelude
/// traits, traits_phase.rs:39-43).  This makes the warning a true behavioral
/// signal of the step-8 edit.
///
/// The fixture deliberately omits frame/region — errors are expected.  We assert
/// only the warning (do NOT assert zero-errors).
///
/// RED on current main: RegionPort absent → not in prelude registry →
/// trait_satisfies("RegionPort","LocatedPort")==false → no asymmetric warning.
#[test]
fn asymmetric_located_port_warning_fires_for_stdlib_region_port() {
    let source = r#"
structure def S {
    port a : out RegionPort {}
    port b : in Port {}
    connect a -> b
}
"#;

    let compiled = compile_source_with_stdlib(source);

    let located_warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.message.contains("LocatedPort")
                && d.message.contains("asymmetric")
        })
        .collect();

    assert!(
        !located_warnings.is_empty(),
        "expected at least one asymmetric-LocatedPort warning when connecting \
         a RegionPort (satisfies LocatedPort via stdlib refinement) to a plain Port; \
         got diagnostics: {:?}",
        compiled.diagnostics
    );
}

// ─── step-9: capstone example compile ─────────────────────────────────────────

/// examples/stdlib/ports_mechanical.ri must compile without errors and
/// structurally declare a template named "Coupling" of EntityKind::Structure.
///
/// Note: direction/Bidi/StructurePort/Bore/Shaft and torque_capacity/max_speed
/// params are not exercised through an actual conformance path in this example
/// (no concrete RotaryPort conformer is instantiated per PRD §4 decision 4).
/// A follow-up that adds a concrete conformer supplying torque_capacity /
/// max_speed / direction literals would close that coverage gap end-to-end.
#[test]
fn example_ports_mechanical_ri_compiles_clean() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let example_path = manifest_dir
        .join("../../examples/stdlib/ports_mechanical.ri")
        .canonicalize()
        .expect("examples/stdlib/ports_mechanical.ri should exist on disk");

    let source = std::fs::read_to_string(&example_path)
        .expect("failed to read examples/stdlib/ports_mechanical.ri");

    let compiled = compile_source_with_stdlib(&source);

    let example_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        example_errors.is_empty(),
        "examples/stdlib/ports_mechanical.ri should compile without errors; got: {:?}",
        example_errors
    );

    assert!(
        compiled.templates.iter().any(|t| {
            t.name == "Coupling" && t.entity_kind == EntityKind::Structure
        }),
        "examples/stdlib/ports_mechanical.ri should declare \
         'structure def Coupling<D: RotaryPort, N: RotaryPort>'; \
         found templates: {:?}",
        compiled
            .templates
            .iter()
            .map(|t| (&t.name, &t.entity_kind))
            .collect::<Vec<_>>()
    );
}
