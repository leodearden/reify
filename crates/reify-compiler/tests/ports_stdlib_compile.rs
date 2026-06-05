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
