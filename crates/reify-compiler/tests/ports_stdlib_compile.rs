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

use reify_compiler::{CompiledTrait, EntityKind, RequirementKind, stdlib_loader};
use reify_core::{DimensionVector, Severity, Type};
use reify_ir::EnumDef;
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

// ─── step-3: Port base trait ──────────────────────────────────────────────────

/// Port base trait has no refinements and exactly one required param:
/// direction : Directionality.
#[test]
fn port_base_trait_requires_direction_directionality() {
    let t = find_trait("std/ports", "Port");

    assert!(
        t.refinements.is_empty(),
        "Port should have no refinements, got: {:?}",
        t.refinements
    );

    assert_eq!(
        t.required_members.len(),
        1,
        "Port should have exactly 1 required member (direction); got: {:?}",
        t.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name, "direction",
        "Port required_members[0] should be 'direction', got '{}'",
        t.required_members[0].name
    );
    assert_eq!(
        param_type("std/ports", "Port", "direction"),
        Type::Enum("Directionality".into()),
        "Port.direction must be Type::Enum(\"Directionality\")"
    );
}

/// std/ports cardinality lock: exactly 1 trait (Port), 1 enum (Directionality),
/// 0 structures.
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
        1,
        "std/ports should declare exactly 1 trait (Port), got: {:?}",
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
        "std/ports should declare 0 structures, got: {:?}",
        structure_names
    );
}

// ─── step-5: std/ports/mechanical loads + marker traits ──────────────────────

/// std/ports/mechanical must load with zero Severity::Error diagnostics.
/// MechanicalPort refines exactly ["Port"]; Bore, Shaft, StructurePort each
/// refine exactly ["MechanicalPort"] with empty own required_members.
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
        ["Port".to_string()].as_slice(),
        "MechanicalPort should refine exactly [Port], got: {:?}",
        mechanical_port.refinements
    );
    assert!(
        mechanical_port.required_members.is_empty(),
        "MechanicalPort should have no own required_members, got: {:?}",
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

// ─── step-7: RotaryPort/ThreadedPort + cardinality lock ──────────────────────

/// RotaryPort refines [MechanicalPort] with required members [torque_capacity,
/// max_speed] in order.
///
/// `torque_capacity` is asserted to the exact Torque dimension
/// (Force·Length/Angle = kg·m²·s⁻²·rad⁻¹), which is distinct from Energy
/// (kg·m²·s⁻²) via the Angle⁻¹ slot.  This locks the `Torque = Force *
/// Length / Angle` deviation documented in ports_mechanical.ri and ensures
/// that a regression to Energy or any other scalar would be caught.
///
/// `max_speed` is asserted to Scalar<ANGULAR_VELOCITY>.
#[test]
fn rotary_port_trait_surface() {
    let t = find_trait("std/ports/mechanical", "RotaryPort");

    assert_eq!(
        t.refinements.as_slice(),
        ["MechanicalPort".to_string()].as_slice(),
        "RotaryPort should refine exactly [MechanicalPort], got: {:?}",
        t.refinements
    );

    assert_eq!(
        t.required_members.len(),
        2,
        "RotaryPort should have exactly 2 required members; got: {:?}",
        t.required_members.iter().map(|r| &r.name).collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name, "torque_capacity",
        "RotaryPort required_members[0] should be 'torque_capacity'"
    );
    assert_eq!(
        t.required_members[1].name, "max_speed",
        "RotaryPort required_members[1] should be 'max_speed'"
    );

    // Torque = Force * Length / Angle (distinct from Energy by the Angle⁻¹ slot).
    let expected_torque_dim = DimensionVector::FORCE
        .mul(&DimensionVector::LENGTH)
        .div(&DimensionVector::ANGLE);
    assert_eq!(
        param_type("std/ports/mechanical", "RotaryPort", "torque_capacity"),
        Type::Scalar {
            dimension: expected_torque_dim
        },
        "RotaryPort.torque_capacity must be Scalar<Force·Length/Angle> \
         (Torque alias — distinct from Energy via Angle⁻¹ slot)"
    );

    assert_eq!(
        param_type("std/ports/mechanical", "RotaryPort", "max_speed"),
        Type::Scalar {
            dimension: DimensionVector::ANGULAR_VELOCITY
        },
        "RotaryPort.max_speed must have DimensionVector::ANGULAR_VELOCITY"
    );
}

/// ThreadedPort refines [MechanicalPort] with required members [thread_diameter,
/// pitch] in order; both resolve to Scalar<LENGTH>.
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
        2,
        "ThreadedPort should have exactly 2 required members; got: {:?}",
        t.required_members.iter().map(|r| &r.name).collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name, "thread_diameter",
        "ThreadedPort required_members[0] should be 'thread_diameter'"
    );
    assert_eq!(
        t.required_members[1].name, "pitch",
        "ThreadedPort required_members[1] should be 'pitch'"
    );

    assert_eq!(
        param_type("std/ports/mechanical", "ThreadedPort", "thread_diameter"),
        Type::Scalar {
            dimension: DimensionVector::LENGTH
        },
        "ThreadedPort.thread_diameter must have DimensionVector::LENGTH"
    );
    assert_eq!(
        param_type("std/ports/mechanical", "ThreadedPort", "pitch"),
        Type::Scalar {
            dimension: DimensionVector::LENGTH
        },
        "ThreadedPort.pitch must have DimensionVector::LENGTH"
    );
}

/// std/ports/mechanical cardinality lock: exactly 6 traits (MechanicalPort,
/// Bore, Shaft, RotaryPort, ThreadedPort, StructurePort), 0 enums, 0 structures.
#[test]
fn std_ports_mechanical_module_cardinality_locked() {
    let module = load_module("std/ports/mechanical");

    assert_eq!(
        module.enum_defs.len(),
        0,
        "std/ports/mechanical should declare 0 enums, got: {:?}",
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
        6,
        "std/ports/mechanical should declare exactly 6 traits \
         (MechanicalPort, Bore, Shaft, RotaryPort, ThreadedPort, StructurePort), got: {:?}",
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
        "std/ports/mechanical should declare 0 structures, got: {:?}",
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
            "Differential".to_string(),
        ],
        "SignalKind variants must be [Analog, Digital, Differential] in order; got: {:?}",
        enum_def.variants
    );

    let electrical_port = find_trait("std/ports/electrical", "ElectricalPort");
    assert_eq!(
        electrical_port.refinements.as_slice(),
        ["Port".to_string()].as_slice(),
        "ElectricalPort should refine exactly [Port], got: {:?}",
        electrical_port.refinements
    );
    assert!(
        electrical_port.required_members.is_empty(),
        "ElectricalPort should have no own required_members, got: {:?}",
        electrical_port
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
}

/// PowerPort refines [ElectricalPort] with required members [voltage, max_current]
/// in order, resolving to Scalar<VOLTAGE> and Scalar<CURRENT>.
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
        2,
        "PowerPort should have exactly 2 required members; got: {:?}",
        t.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name,
        "voltage",
        "PowerPort required_members[0] should be 'voltage'"
    );
    assert_eq!(
        t.required_members[1].name,
        "max_current",
        "PowerPort required_members[1] should be 'max_current'"
    );

    assert_eq!(
        param_type("std/ports/electrical", "PowerPort", "voltage"),
        Type::Scalar {
            dimension: DimensionVector::VOLTAGE
        },
        "PowerPort.voltage must be Scalar<VOLTAGE>"
    );
    assert_eq!(
        param_type("std/ports/electrical", "PowerPort", "max_current"),
        Type::Scalar {
            dimension: DimensionVector::CURRENT
        },
        "PowerPort.max_current must be Scalar<CURRENT>"
    );
}

/// SignalPort refines [ElectricalPort] with required members [signal_kind, impedance]
/// in order, resolving to Enum("SignalKind") and Scalar<RESISTANCE>.
/// impedance uses the Resistance/Ω dimension (no distinct Impedance named dim;
/// dimensionally identical — documented deviation in ports_electrical.ri header).
#[test]
fn signal_port_trait_surface() {
    let t = find_trait("std/ports/electrical", "SignalPort");

    assert_eq!(
        t.refinements.as_slice(),
        ["ElectricalPort".to_string()].as_slice(),
        "SignalPort should refine exactly [ElectricalPort], got: {:?}",
        t.refinements
    );

    assert_eq!(
        t.required_members.len(),
        2,
        "SignalPort should have exactly 2 required members; got: {:?}",
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
        t.required_members[1].name,
        "impedance",
        "SignalPort required_members[1] should be 'impedance'"
    );

    assert_eq!(
        param_type("std/ports/electrical", "SignalPort", "signal_kind"),
        Type::Enum("SignalKind".into()),
        "SignalPort.signal_kind must be Type::Enum(\"SignalKind\")"
    );
    assert_eq!(
        param_type("std/ports/electrical", "SignalPort", "impedance"),
        Type::Scalar {
            dimension: DimensionVector::RESISTANCE
        },
        "SignalPort.impedance must be Scalar<RESISTANCE> \
         (no distinct Impedance dimension; electrically identical to Resistance)"
    );
}

/// std/ports/electrical cardinality lock: exactly 3 traits (ElectricalPort,
/// PowerPort, SignalPort), 1 enum (SignalKind), 0 structures.
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
        3,
        "std/ports/electrical should declare exactly 3 traits \
         (ElectricalPort, PowerPort, SignalPort), got: {:?}",
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
        "std/ports/electrical should declare 0 structures, got: {:?}",
        structure_names
    );
}

// ─── step-3 (thermal): std/ports/thermal surface ─────────────────────────────

/// std/ports/thermal must load with zero Severity::Error diagnostics.
/// ThermalPort refines exactly [Port] with required members [temperature,
/// heat_flow] in order, resolving to Scalar<TEMPERATURE> and Scalar<POWER>.
///
/// Implements the Modelica HeatPort convention: temperature (potential across
/// variable) + heat_flow (through variable Q̇).  Resolves PRD open Q3.
/// See design decision in plan.json: deviation from PRD's heat-transfer-
/// coefficient suggestion — both params are named dims, no alias needed.
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

    assert_eq!(
        thermal_port.required_members.len(),
        2,
        "ThermalPort should have exactly 2 required members; got: {:?}",
        thermal_port
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        thermal_port.required_members[0].name,
        "temperature",
        "ThermalPort required_members[0] should be 'temperature'"
    );
    assert_eq!(
        thermal_port.required_members[1].name,
        "heat_flow",
        "ThermalPort required_members[1] should be 'heat_flow'"
    );

    assert_eq!(
        param_type("std/ports/thermal", "ThermalPort", "temperature"),
        Type::Scalar {
            dimension: DimensionVector::TEMPERATURE
        },
        "ThermalPort.temperature must be Scalar<TEMPERATURE>"
    );
    assert_eq!(
        param_type("std/ports/thermal", "ThermalPort", "heat_flow"),
        Type::Scalar {
            dimension: DimensionVector::POWER
        },
        "ThermalPort.heat_flow must be Scalar<POWER>"
    );
}

/// std/ports/thermal cardinality lock: exactly 1 trait (ThermalPort),
/// 0 enums, 0 structures.
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
        1,
        "std/ports/thermal should declare exactly 1 trait (ThermalPort), got: {:?}",
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
        "std/ports/thermal should declare 0 structures, got: {:?}",
        structure_names
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
        3,
        "FluidPort should have exactly 3 required members; got: {:?}",
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
}

/// std/ports/fluid cardinality lock: exactly 1 trait (FluidPort),
/// 0 enums, 0 structures.
#[test]
fn std_ports_fluid_module_cardinality_locked() {
    let module = load_module("std/ports/fluid");

    assert_eq!(
        module.enum_defs.len(),
        0,
        "std/ports/fluid should declare 0 enums, got: {:?}",
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
        1,
        "std/ports/fluid should declare exactly 1 trait (FluidPort), got: {:?}",
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
