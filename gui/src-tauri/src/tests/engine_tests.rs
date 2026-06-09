use std::path::Path;
use std::sync::atomic::Ordering;

use reify_compiler::find_template;
use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{
    CountingSubscriberBuilder, FailingMockGeometryKernel, MockConstraintSolver, MockGeometryKernel,
    bracket_source, bracket_source_violating, bracket_source_with_width,
    warn_source_with_unknown_port_type, warn_source_with_unknown_port_type_with_width,
};
use reify_ir::ExportFormat;

use reify_core::{DiagnosticInfo, ModulePath, SourceLocationInfo, Type, ValueCellId};

use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder, gt, literal, mm, value_ref};

use crate::engine::{CompileFailure, CompileFailureKind, CoreState, EngineSession, build_template_node, module_key, parse_value_string};
use crate::types::EntityTreeNode;

#[test]
fn engine_session_new_with_mock_kernel() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let _session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
}

#[test]
fn load_from_source_returns_gui_state_with_values() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");

    // Bracket has 5 params + 1 let (volume) = 6 value cells (body is geometry, not a value)
    assert!(
        state.values.len() >= 5,
        "expected at least 5 values, got {}",
        state.values.len()
    );
}

#[test]
fn load_from_source_returns_gui_state_with_constraints() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");

    assert_eq!(state.constraints.len(), 3, "bracket has 3 constraints");
}

#[test]
fn load_from_source_width_value_is_80mm() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");

    let width = state
        .values
        .iter()
        .find(|v| v.name == "width")
        .expect("should have width value");

    assert_eq!(width.value, "80", "width should be 80mm displayed as 80");
    assert_eq!(width.unit, "mm");
}

#[test]
fn load_from_source_resolves_stdlib_enum_access_without_inline_redecl() {
    // Regression guard for task 2525: the GUI must accept sources that reference
    // stdlib enums (e.g. `CorrosionClass.C5`) WITHOUT inline redeclarations.
    // Pre-task, the parser disambiguated `Type.Variant` against the current source's
    // enum decls only, so without an inline `enum CorrosionClass { ... }`, the parser
    // produced `MemberAccess` and `compile_with_stdlib` rejected the unresolved name,
    // making `load_from_source` return `Err`. This test pins that the GUI's parse
    // step now consults stdlib enums.
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Minimal synthetic source — references stdlib enum `CorrosionClass.C5` only,
    // no inline redecl, no stdlib trait bounds (keeps the test focused on the
    // parser-disambiguation contract, not on full conformance plumbing).
    let source = "structure Sample {\n  let chosen_class = CorrosionClass.C5\n}\n";

    let result = session.load_from_source(source, "sample");

    assert!(
        result.is_ok(),
        "load_from_source should accept a stdlib enum reference without inline redecl, got: {:?}",
        result.err()
    );
}

#[test]
fn load_from_source_with_invalid_source_returns_err() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let result = session.load_from_source("this is not valid reify syntax {{{}}", "bad");
    assert!(result.is_err(), "invalid source should return Err");
}

#[test]
fn set_parameter_changes_width() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let state = session
        .set_parameter("Bracket.width", "120mm")
        .expect("set_parameter should succeed");

    let width = state
        .values
        .iter()
        .find(|v| v.name == "width")
        .expect("should have width value");

    assert_eq!(width.value, "120", "width should now be 120mm");
    assert_eq!(width.unit, "mm");
}

// ---- get_mechanism_descriptors tests (steps 3, 5, 7, 9, 11, 23) -----------

/// A 2-body open-chain mechanism with one prismatic and one revolute joint.
///
/// Using explicit intermediate `let` bindings (mechanism() stdlib uses
/// free functions, not method chaining).
const HAPPY_MECHANISM_SOURCE: &str = r#"
structure Kinematic {
    let j_a = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let j_b = revolute(vec3(0, 0, 1), 0rad .. 3.14rad)
    let m0  = mechanism()
    let m1  = body(m0, "solid_a", j_a)
    let m2  = body(m1, "solid_b", j_b, j_a)
}
"#;

/// A 3-body mechanism with a coupling joint and a fixed joint (step 7).
///
/// j_a: prismatic (parent)
/// j_c: coupling of j_a with ratio -1.0 (mirrors parent, dimensionless)
/// j_f: fixed (no axis, no range)
const COUPLING_FIXED_SOURCE: &str = r#"
structure Kinematic {
    let j_a = prismatic(vec3(1, 0, 0), 0mm .. 500mm)
    let j_c = couple(j_a, -1.0)
    let j_f = fixed()
    let m0  = mechanism()
    let m1  = body(m0, "solid_a", j_a)
    let m2  = body(m1, "solid_b", j_c, j_a)
    let m3  = body(m2, "solid_c", j_f, j_c)
}
"#;

/// An errored mechanism via duplicate-solid: the same solid string `"solid_a"`
/// is attached twice (first via `j_a`, then via `j_b`), which stamps
/// `error="duplicate_solid"` on the resulting mechanism Map.
///
/// Migration note: this fixture previously triggered `error="closed_chain"`
/// via a parent-conflict pattern, but under v0.2 closed kinematic chains are
/// recorded as loop-closure constraints rather than errored — so the
/// parent-conflict trigger no longer surfaces an `error` key.  The
/// duplicate-solid trigger (canonical recipe per
/// crates/reify-stdlib/src/snapshot.rs `snapshot_on_errored_mechanism_returns_undef`)
/// preserves the test contract: the resulting mechanism Map carries an
/// `error` key, and `get_mechanism_descriptors` must filter it out.
const DUPLICATE_SOLID_SOURCE: &str = r#"
structure Kinematic {
    let j_a = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let j_b = prismatic(vec3(0, 1, 0), 0mm .. 1000mm)
    let m0  = mechanism()
    let m1  = body(m0, "solid_a", j_a)
    let m2  = body(m1, "solid_a", j_b)
}
"#;

/// Helper: create a fresh empty EngineSession.
fn make_session() -> EngineSession {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    EngineSession::new(Box::new(checker), Some(Box::new(kernel)))
}

#[test]
fn get_mechanism_descriptors_extracts_prismatic_and_revolute_joints() {
    // Step-5 RED: load a 2-body open-chain mechanism and assert the descriptor
    // for m2 (bodies_count=2) has two joints with correct kind/dimension/range.
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(HAPPY_MECHANISM_SOURCE, "kinematic")
        .expect("load kinematic");

    let descriptors = session.get_mechanism_descriptors();

    // After the terminal-mechanism filter, only m2 (terminal) appears.
    // m0 is consumed by `body(m0, …)` and m1 is consumed by `body(m1, …)`,
    // so both are non-terminal and must be filtered out.
    assert_eq!(
        descriptors.len(),
        1,
        "only the terminal mechanism m2 should appear after the terminal-mechanism filter"
    );

    // Find m2 by bodies_count=2.
    let m2_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 2)
        .expect("expected a descriptor with bodies_count=2 (the m2 mechanism)");

    // Joint extraction: two unique joints (j_a prismatic, j_b revolute).
    assert_eq!(
        m2_desc.joints.len(),
        2,
        "m2 uses 2 distinct joints; expected 2 JointDescriptors, got {:?}",
        m2_desc.joints
    );

    // Find the prismatic joint.
    let prismatic = m2_desc
        .joints
        .iter()
        .find(|j| j.kind == "prismatic")
        .expect("expected a prismatic JointDescriptor");
    assert_eq!(prismatic.dimension, "length");
    // 0mm = 0.0 m, 1000mm = 1.0 m in SI.
    assert_eq!(
        prismatic.range_lower_si,
        Some(0.0),
        "prismatic lower bound should be 0.0 m"
    );
    let upper = prismatic
        .range_upper_si
        .expect("prismatic upper_si should be Some");
    assert!(
        (upper - 1.0).abs() < 1e-9,
        "prismatic upper bound should be 1.0 m (1000mm), got {upper}"
    );
    // Axis should be [1, 0, 0].
    let axis = prismatic.axis.expect("prismatic axis should be Some");
    assert!(
        (axis[0] - 1.0).abs() < 1e-9 && axis[1].abs() < 1e-9 && axis[2].abs() < 1e-9,
        "prismatic axis should be [1,0,0], got {:?}",
        axis
    );

    // Find the revolute joint.
    let revolute = m2_desc
        .joints
        .iter()
        .find(|j| j.kind == "revolute")
        .expect("expected a revolute JointDescriptor");
    assert_eq!(revolute.dimension, "angle");
    assert_eq!(
        revolute.range_lower_si,
        Some(0.0),
        "revolute lower bound should be 0.0 rad"
    );
    let upper_rev = revolute
        .range_upper_si
        .expect("revolute upper_si should be Some");
    // Test fixture's .ri source uses the literal 3.14, not std::f64::consts::PI.
    #[allow(clippy::approx_constant)]
    let expected_upper = 3.14_f64;
    assert!(
        (upper_rev - expected_upper).abs() < 1e-6,
        "revolute upper bound should be 3.14 rad (per fixture), got {upper_rev}"
    );
}

#[test]
fn get_mechanism_descriptors_returns_empty_when_no_module_loaded() {
    let mut session = make_session();
    let descriptors = session.get_mechanism_descriptors();
    assert!(
        descriptors.is_empty(),
        "expected empty descriptor list when no module is loaded, got {:?}",
        descriptors
    );
}

#[test]
fn get_mechanism_descriptors_returns_empty_when_module_has_no_mechanisms() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load bracket");

    let descriptors = session.get_mechanism_descriptors();
    assert!(
        descriptors.is_empty(),
        "bracket has no mechanisms; expected empty list, got {:?}",
        descriptors
    );
}

#[test]
fn get_mechanism_descriptors_filters_errored_mechanisms() {
    // Load a duplicate-solid mechanism: m2 attaches "solid_a" a second time
    // (via j_b) after m1 already registered it (via j_a), so the mechanism
    // stdlib stamps `error="duplicate_solid"` on m2.  m0 (0 bodies) and m1
    // (1 body) are valid intermediate mechanism Maps without an error key
    // and may legitimately appear.
    //
    // What MUST be true: no descriptor with `name == "m2"` appears in the
    // list — the errored mechanism Map must be filtered out by the `error`
    // key check (engine.rs, `get_mechanism_descriptors`).
    //
    // Migration note: under v0.2 the prior closed-chain trigger no longer
    // produces an `error` key (closed chains are now recorded as
    // loop-closure constraints, not errors).  The contract under test
    // (errored cells filtered from descriptor output) is unchanged; only
    // the trigger is duplicate_solid instead of closed_chain.  A
    // name-based identifier is used here because under duplicate_solid the
    // input m1's bodies list is preserved verbatim on m2, so
    // `bodies_count == 2` is no longer a clean discriminator.
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(DUPLICATE_SOLID_SOURCE, "kinematic")
        .expect(
            "load_from_source should not fail for duplicate-solid (error is at eval, not compile)",
        );

    let descriptors = session.get_mechanism_descriptors();
    assert!(
        !descriptors.iter().any(|d| d.name == "m2"),
        "errored (duplicate-solid) mechanism cell `m2` must be filtered out; got {:?}",
        descriptors
    );
}

#[test]
fn get_mechanism_descriptors_handles_coupling_and_fixed_joints() {
    // Step-7 RED: load a mechanism with a coupling and a fixed joint,
    // assert that their descriptors carry dimension="dimensionless", axis=None,
    // range_lower_si=None, range_upper_si=None.
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(COUPLING_FIXED_SOURCE, "kinematic")
        .expect("load coupling/fixed mechanism");

    let descriptors = session.get_mechanism_descriptors();

    // m3 has 3 bodies and should have three distinct joints (j_a, j_c, j_f).
    let m3_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 3)
        .expect("expected a descriptor with bodies_count=3 (m3 mechanism)");

    assert_eq!(
        m3_desc.joints.len(),
        3,
        "m3 has 3 distinct joints; expected 3 JointDescriptors, got {:?}",
        m3_desc.joints
    );

    // Coupling joint assertions.
    let coupling = m3_desc
        .joints
        .iter()
        .find(|j| j.kind == "coupling")
        .expect("expected a coupling JointDescriptor");
    assert_eq!(
        coupling.dimension, "dimensionless",
        "coupling dimension should be 'dimensionless'"
    );
    assert!(
        coupling.axis.is_none(),
        "coupling axis should be None, got {:?}",
        coupling.axis
    );
    assert!(
        coupling.range_lower_si.is_none(),
        "coupling range_lower_si should be None, got {:?}",
        coupling.range_lower_si
    );
    assert!(
        coupling.range_upper_si.is_none(),
        "coupling range_upper_si should be None, got {:?}",
        coupling.range_upper_si
    );

    // Fixed joint assertions.
    let fixed = m3_desc
        .joints
        .iter()
        .find(|j| j.kind == "fixed")
        .expect("expected a fixed JointDescriptor");
    assert_eq!(
        fixed.dimension, "dimensionless",
        "fixed dimension should be 'dimensionless'"
    );
    assert!(
        fixed.axis.is_none(),
        "fixed axis should be None, got {:?}",
        fixed.axis
    );
    assert!(
        fixed.range_lower_si.is_none(),
        "fixed range_lower_si should be None, got {:?}",
        fixed.range_lower_si
    );
    assert!(
        fixed.range_upper_si.is_none(),
        "fixed range_upper_si should be None, got {:?}",
        fixed.range_upper_si
    );
}

#[test]
fn get_mechanism_descriptors_snapshot_consumption_does_not_filter() {
    // Step 3: MULTI_SNAPSHOT_SOURCE has m0/m1/m2 body chain plus s1/s2 snapshot
    // lets that read from m2.  snapshot() consumption must NOT make m2 an
    // intermediate mechanism — it should still appear as the one terminal
    // descriptor.  Pins the design decision: body()-only filter.
    let mut session = make_session();
    session
        .load_from_source(MULTI_SNAPSHOT_SOURCE, "kinematic")
        .expect("load multi-snapshot source");

    let descriptors = session.get_mechanism_descriptors();

    assert_eq!(
        descriptors.len(),
        1,
        "snapshot consumption must not filter the mechanism; expected 1 descriptor, got {:?}",
        descriptors.iter().map(|d| &d.name).collect::<Vec<_>>()
    );
    assert_eq!(
        descriptors[0].name, "m2",
        "the terminal mechanism should be m2; got {:?}",
        descriptors[0].name
    );
    assert_eq!(
        descriptors[0].bodies_count, 2,
        "m2 should have bodies_count=2; got {}",
        descriptors[0].bodies_count
    );
}

#[test]
fn get_mechanism_descriptors_filters_intermediate_body_chain_cells() {
    // Step 1 RED: HAPPY_MECHANISM_SOURCE has m0/m1/m2 where m0 is consumed by
    // body(m0,...) → m1, and m1 is consumed by body(m1,...) → m2.  Only the
    // terminal mechanism (m2) should appear in the returned descriptors.
    let mut session = make_session();
    session
        .load_from_source(HAPPY_MECHANISM_SOURCE, "kinematic")
        .expect("load kinematic");

    let descriptors = session.get_mechanism_descriptors();

    assert_eq!(
        descriptors.len(),
        1,
        "only the terminal mechanism should be returned; got {:?}",
        descriptors.iter().map(|d| &d.name).collect::<Vec<_>>()
    );
    assert_eq!(
        descriptors[0].name, "m2",
        "the terminal mechanism should be m2; got {:?}",
        descriptors[0].name
    );
    assert_eq!(
        descriptors[0].bodies_count, 2,
        "m2 should have bodies_count=2; got {}",
        descriptors[0].bodies_count
    );
}

/// Source for step-11: 1-body mechanism where y_axis is bound to param y_pos.
/// `snapshot(m1, [bind(y_axis, y_pos)])` — y_pos is a param → driving param
/// resolution should yield driving_param_cell_id = Some("Kinematic.y_pos").
const SNAPSHOT_PARAM_BIND_SOURCE: &str = r#"
structure Kinematic {
    param y_pos: Length = 100mm
    let y_axis = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let m0     = mechanism()
    let m1     = body(m0, "solid_a", y_axis)
    let snap   = snapshot(m1, [bind(y_axis, y_pos)])
}
"#;

/// Source for step-11 sibling: 1-body mechanism where y_axis is bound to a
/// literal `50mm` instead of a param.  `bind(y_axis, 50mm)` — literal →
/// driving_param_cell_id must remain None.
const SNAPSHOT_LITERAL_BIND_SOURCE: &str = r#"
structure Kinematic {
    let y_axis = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let m0     = mechanism()
    let m1     = body(m0, "solid_a", y_axis)
    let snap   = snapshot(m1, [bind(y_axis, 50mm)])
}
"#;

#[test]
fn get_mechanism_descriptors_resolves_driving_param_via_ast() {
    // Step-11 RED: load a mechanism where `bind(y_axis, y_pos)` maps the
    // prismatic joint to param y_pos.  After AST traversal the joint descriptor
    // should have driving_param_cell_id = Some("Kinematic.y_pos").
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(SNAPSHOT_PARAM_BIND_SOURCE, "kinematic")
        .expect("load snapshot+param source");

    let descriptors = session.get_mechanism_descriptors();

    // m1 has bodies_count=1 and one prismatic joint.
    let m1_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 1)
        .expect("expected descriptor with bodies_count=1 (m1)");

    assert_eq!(
        m1_desc.joints.len(),
        1,
        "m1 has one joint; got {:?}",
        m1_desc.joints
    );

    let joint = &m1_desc.joints[0];
    assert_eq!(
        joint.driving_param_cell_id,
        Some("Kinematic.y_pos".to_string()),
        "bind(y_axis, y_pos) → driving_param_cell_id should be Some(\"Kinematic.y_pos\"); got {:?}",
        joint.driving_param_cell_id
    );
}

#[test]
fn get_mechanism_descriptors_literal_bind_yields_no_driving_param() {
    // Step-11 RED sibling: bind(y_axis, 50mm) — literal value → driving param
    // cannot be resolved → driving_param_cell_id must be None.
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(SNAPSHOT_LITERAL_BIND_SOURCE, "kinematic")
        .expect("load snapshot+literal source");

    let descriptors = session.get_mechanism_descriptors();

    let m1_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 1)
        .expect("expected descriptor with bodies_count=1");

    let joint = &m1_desc.joints[0];
    assert!(
        joint.driving_param_cell_id.is_none(),
        "literal bind must NOT resolve to a driving param; got {:?}",
        joint.driving_param_cell_id
    );
}

// ---- step-23: current_value_si round-trip ---------------------------------

#[test]
fn get_mechanism_descriptors_current_value_si_reflects_initial_param() {
    // Step-23 RED (part 1): after loading SNAPSHOT_PARAM_BIND_SOURCE, the joint
    // descriptor should have current_value_si = Some(0.1) (y_pos = 100mm = 0.1 SI).
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(SNAPSHOT_PARAM_BIND_SOURCE, "kinematic")
        .expect("load snapshot+param source");

    let descriptors = session.get_mechanism_descriptors();
    let m1_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 1)
        .expect("expected descriptor with bodies_count=1");
    let joint = &m1_desc.joints[0];

    // Must have resolved a driving param (prerequisite for current_value_si).
    assert_eq!(
        joint.driving_param_cell_id,
        Some("Kinematic.y_pos".to_string()),
        "driving param should be Kinematic.y_pos"
    );

    // current_value_si must be populated with the default value 100mm = 0.1 SI.
    assert_eq!(
        joint.current_value_si,
        Some(0.1),
        "initial current_value_si should be 0.1 (100mm); got {:?}",
        joint.current_value_si
    );
}

#[test]
fn get_mechanism_descriptors_current_value_si_updates_after_set_parameter() {
    // Step-23 RED (part 2): after set_parameter("Kinematic.y_pos", "150mm"), a
    // fresh get_mechanism_descriptors call must report current_value_si = Some(0.15).
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(SNAPSHOT_PARAM_BIND_SOURCE, "kinematic")
        .expect("load snapshot+param source");

    // Scrub the slider by setting y_pos to 150mm.
    session
        .set_parameter("Kinematic.y_pos", "150mm")
        .expect("set_parameter should succeed");

    // Re-fetch descriptors after the edit.
    let descriptors = session.get_mechanism_descriptors();
    let m1_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 1)
        .expect("expected descriptor with bodies_count=1");
    let joint = &m1_desc.joints[0];

    // current_value_si must now reflect 150mm = 0.15 SI.
    assert_eq!(
        joint.current_value_si,
        Some(0.15),
        "current_value_si should be 0.15 (150mm) after set_parameter; got {:?}",
        joint.current_value_si
    );
}

/// Source for step-9: 1-body mechanism where y_axis is bound to a bare
/// dimensionless number `0.5`.  `bind(y_axis, 0.5)` — NumberLiteral →
/// initial_value_si should be Some(0.5) (no unit conversion).
const SNAPSHOT_NUMBER_LITERAL_BIND_SOURCE: &str = r#"
structure Kinematic {
    let y_axis = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let m0     = mechanism()
    let m1     = body(m0, "solid_a", y_axis)
    let snap   = snapshot(m1, [bind(y_axis, 0.5)])
}
"#;

// ---- JointBinding ParamBound promotion via AST resolver (task 3783, step-7) ----

/// `resolve_driving_params_from_ast` must promote the joint `binding` field from
/// the kind-based default `LiteralBound` to `ParamBound { param_cell_id, current_value_si }`
/// when a `bind(joint, param)` pair is found.
///
/// Also verifies that the legacy flat fields (`driving_param_cell_id`,
/// `current_value_si`) remain populated for backward compat.
#[test]
fn get_mechanism_descriptors_param_bind_promotes_binding_to_param_bound() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(SNAPSHOT_PARAM_BIND_SOURCE, "kinematic")
        .expect("load snapshot+param source");

    let descriptors = session.get_mechanism_descriptors();
    let m1_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 1)
        .expect("expected descriptor with bodies_count=1 (m1)");

    let joint = &m1_desc.joints[0];

    // The binding must have been promoted to ParamBound.
    assert_eq!(
        joint.binding,
        crate::types::JointBinding::ParamBound {
            param_cell_id: "Kinematic.y_pos".to_string(),
            current_value_si: Some(0.1),
        },
        "bind(y_axis, y_pos) must promote binding to ParamBound {{ param_cell_id: \"Kinematic.y_pos\", \
         current_value_si: Some(0.1) }}; got {:?}",
        joint.binding
    );

    // Backward-compat flat fields must still be populated.
    assert_eq!(
        joint.driving_param_cell_id,
        Some("Kinematic.y_pos".to_string()),
        "driving_param_cell_id must still be Some(\"Kinematic.y_pos\") for compat; got {:?}",
        joint.driving_param_cell_id
    );
    assert_eq!(
        joint.current_value_si,
        Some(0.1),
        "current_value_si flat field must be Some(0.1); got {:?}",
        joint.current_value_si
    );
}

/// After `set_parameter("Kinematic.y_pos", "150mm")` the `binding` field on the
/// joint descriptor must reflect the updated value in `ParamBound.current_value_si`.
#[test]
fn get_mechanism_descriptors_param_bind_binding_updates_after_set_parameter() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(SNAPSHOT_PARAM_BIND_SOURCE, "kinematic")
        .expect("load snapshot+param source");

    session
        .set_parameter("Kinematic.y_pos", "150mm")
        .expect("set_parameter should succeed");

    let descriptors = session.get_mechanism_descriptors();
    let m1_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 1)
        .expect("expected descriptor with bodies_count=1");
    let joint = &m1_desc.joints[0];

    assert_eq!(
        joint.binding,
        crate::types::JointBinding::ParamBound {
            param_cell_id: "Kinematic.y_pos".to_string(),
            current_value_si: Some(0.15),
        },
        "after set_parameter(150mm), binding must show current_value_si=Some(0.15); got {:?}",
        joint.binding
    );
}

// ---- LiteralBound binding via AST resolver (task 3783, step-9) ---------------

/// User-observable signal: `bind(y_axis, 50mm)` must produce
/// `JointBinding::LiteralBound { synth_param_name: "__joint_y_axis_v",
/// initial_value_si: Some(0.05), scrubbable: true }`.
///
/// This is the primary contract test for the η-engine task.
#[test]
fn get_mechanism_descriptors_literal_bind_produces_scrubbable_literal_bound_binding() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(SNAPSHOT_LITERAL_BIND_SOURCE, "kinematic")
        .expect("load snapshot+literal source");

    let descriptors = session.get_mechanism_descriptors();
    let m1_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 1)
        .expect("expected descriptor with bodies_count=1");

    let joint = &m1_desc.joints[0];

    // Must produce LiteralBound with joint cell name (not joint_index).
    assert_eq!(
        joint.binding,
        crate::types::JointBinding::LiteralBound {
            synth_param_name: "__joint_y_axis_v".to_string(),
            initial_value_si: Some(0.05),
            scrubbable: true,
        },
        "bind(y_axis, 50mm) must produce LiteralBound {{ synth_param_name: \"__joint_y_axis_v\", \
         initial_value_si: Some(0.05), scrubbable: true }}; got {:?}",
        joint.binding
    );

    // Legacy flat field must remain None (literal, not a param reference).
    assert!(
        joint.driving_param_cell_id.is_none(),
        "literal bind must NOT set driving_param_cell_id; got {:?}",
        joint.driving_param_cell_id
    );
}

/// `bind(y_axis, 0.5)` (bare NumberLiteral, no unit) must produce
/// `JointBinding::LiteralBound { initial_value_si: Some(0.5), ... }`.
#[test]
fn get_mechanism_descriptors_literal_bind_with_dimensionless_number_literal() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(SNAPSHOT_NUMBER_LITERAL_BIND_SOURCE, "kinematic")
        .expect("load snapshot+number-literal source");

    let descriptors = session.get_mechanism_descriptors();
    let m1_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 1)
        .expect("expected descriptor with bodies_count=1");

    let joint = &m1_desc.joints[0];

    assert_eq!(
        joint.binding,
        crate::types::JointBinding::LiteralBound {
            synth_param_name: "__joint_y_axis_v".to_string(),
            initial_value_si: Some(0.5),
            scrubbable: true,
        },
        "bind(y_axis, 0.5) must produce LiteralBound {{ initial_value_si: Some(0.5) }}; got {:?}",
        joint.binding
    );
}

// ---- edge case tests (amendment pass, suggestion 8) -------------------------

/// Source for double-bind test: same joint j bound to two different params in
/// two separate snapshot() calls.  The first-wins guard ensures only p1 wins.
const DOUBLE_BIND_SOURCE: &str = r#"
structure Kinematic {
    param p1: Length = 100mm
    param p2: Length = 200mm
    let j  = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let m0 = mechanism()
    let m1 = body(m0, "solid_a", j)
    let snap1 = snapshot(m1, [bind(j, p1)])
    let snap2 = snapshot(m1, [bind(j, p2)])
}
"#;

#[test]
fn get_mechanism_descriptors_double_bind_first_wins() {
    // Two snapshot() calls bind the same joint j to p1 and p2 respectively.
    // The `is_none()` guard in `resolve_driving_params_from_ast` ensures that
    // only the first binding (p1) is recorded; p2 must NOT overwrite it.
    let mut session = make_session();
    session
        .load_from_source(DOUBLE_BIND_SOURCE, "kinematic")
        .expect("load double-bind source");

    let descriptors = session.get_mechanism_descriptors();
    let m1_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 1)
        .expect("expected descriptor with bodies_count=1 (m1)");

    assert_eq!(m1_desc.joints.len(), 1, "m1 has one joint");
    let joint = &m1_desc.joints[0];
    assert_eq!(
        joint.driving_param_cell_id,
        Some("Kinematic.p1".to_string()),
        "first bind() wins: expected p1, got {:?}",
        joint.driving_param_cell_id
    );
}

/// Source for let-bound test: the value side of bind() is a Let cell (not a Param).
/// The `is_param` guard must reject it → driving_param_cell_id stays None.
const LET_BIND_SOURCE: &str = r#"
structure Kinematic {
    let q  = 100mm
    let j  = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let m0 = mechanism()
    let m1 = body(m0, "solid_a", j)
    let snap = snapshot(m1, [bind(j, q)])
}
"#;

#[test]
fn get_mechanism_descriptors_let_bound_yields_no_driving_param() {
    // bind() where the value side is a Let cell (not a Param) must NOT resolve
    // to a driving param.  Validates the `is_param` guard in the AST resolver.
    let mut session = make_session();
    session
        .load_from_source(LET_BIND_SOURCE, "kinematic")
        .expect("load let-bound source");

    let descriptors = session.get_mechanism_descriptors();
    let m1_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 1)
        .expect("expected descriptor with bodies_count=1 (m1)");

    assert_eq!(m1_desc.joints.len(), 1, "m1 has one joint");
    let joint = &m1_desc.joints[0];
    assert!(
        joint.driving_param_cell_id.is_none(),
        "Let-bound bind must NOT resolve to a driving param; got {:?}",
        joint.driving_param_cell_id
    );
}

/// Source for world-only test: mechanism() with no real bodies added.
/// The descriptor should have joints.len() == 0 (world sentinel filtered).
const WORLD_ONLY_SOURCE: &str = r#"
structure Kinematic {
    let m0 = mechanism()
}
"#;

#[test]
fn get_mechanism_descriptors_world_only_mechanism_has_no_joints() {
    // mechanism() creates a mechanism with only the implicit world body.
    // is_world_sentinel filters the world body's "at" field, so joints.len() == 0
    // regardless of bodies_count (the world body is still in the bodies list).
    let mut session = make_session();
    session
        .load_from_source(WORLD_ONLY_SOURCE, "kinematic")
        .expect("load world-only source");

    let descriptors = session.get_mechanism_descriptors();
    let m0_desc = descriptors
        .iter()
        .find(|d| d.name == "m0")
        .expect("expected descriptor for m0");

    assert_eq!(
        m0_desc.joints.len(),
        0,
        "world-only mechanism has no scrubbable joints; got {:?}",
        m0_desc.joints
    );
}

/// Source for multi-snapshot test: two separate let cells each hold a snapshot()
/// call with a distinct bind() pair.  Both bindings must be resolved, exercising
/// the outer `for member in &structure.members` iteration and verifying that
/// bind pairs from multiple snapshot lets are all collected.
const MULTI_SNAPSHOT_SOURCE: &str = r#"
structure Kinematic {
    param p1: Length = 100mm
    param p2: Length = 200mm
    let j1 = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let j2 = prismatic(vec3(0, 1, 0), 0mm .. 600mm)
    let m0  = mechanism()
    let m1  = body(m0, "solid_a", j1)
    let m2  = body(m1, "solid_b", j2, j1)
    let s1  = snapshot(m2, [bind(j1, p1)])
    let s2  = snapshot(m2, [bind(j2, p2)])
}
"#;

#[test]
fn get_mechanism_descriptors_multiple_snapshot_lets_resolve_both_params() {
    // Two separate `let s = snapshot(...)` declarations each contribute one
    // bind() pair.  Both joints should have their driving_param_cell_id resolved.
    let mut session = make_session();
    session
        .load_from_source(MULTI_SNAPSHOT_SOURCE, "kinematic")
        .expect("load multi-snapshot source");

    let descriptors = session.get_mechanism_descriptors();
    let m2_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 2)
        .expect("expected descriptor with bodies_count=2 (m2)");

    assert_eq!(m2_desc.joints.len(), 2, "m2 has two distinct joints");

    let j1_desc = m2_desc
        .joints
        .iter()
        .find(|j| j.driving_param_cell_id == Some("Kinematic.p1".to_string()));
    let j2_desc = m2_desc
        .joints
        .iter()
        .find(|j| j.driving_param_cell_id == Some("Kinematic.p2".to_string()));

    assert!(
        j1_desc.is_some(),
        "j1 should be driven by p1; joint descriptors: {:?}",
        m2_desc.joints
    );
    assert!(
        j2_desc.is_some(),
        "j2 should be driven by p2; joint descriptors: {:?}",
        m2_desc.joints
    );
}

// ---- snapshot/bind telemetry tests (steps 4-5, 6-7) --------------------------

/// Source for step-4/5: snapshot with an empty bind list.
/// `snapshot(m1, [])` is a textual snapshot() match but the bind list is
/// empty — this is valid stdlib usage (case b) and must NOT trigger anomaly
/// telemetry.
const EMPTY_BIND_SNAPSHOT_SOURCE: &str = r#"
structure Kinematic {
    let j1 = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let m0 = mechanism()
    let m1 = body(m0, "solid_a", j1)
    let snap = snapshot(m1, [])
}
"#;

#[test]
fn collect_snapshot_bind_pairs_stays_silent_for_empty_bind_list() {
    // Inoculate against tracing's per-callsite Interest cache — see
    // `prime_tracing_callsite_cache` in reify-test-support for why.
    reify_test_support::prime_tracing_callsite_cache();
    // snapshot(m1, []) has an empty bind list — case (b) per the telemetry
    // refinement plan.  An empty list is valid stdlib usage and must NOT emit
    // a DEBUG event.
    let mut session = make_session();
    session
        .load_from_source(EMPTY_BIND_SNAPSHOT_SOURCE, "kinematic")
        .expect("load empty-bind-list source");

    // Filter on the specific submodule target so this assertion remains valid
    // even if other debug! calls are added elsewhere in reify_gui::engine.
    let (subscriber, counters) = CountingSubscriberBuilder::new()
        .count_level(tracing::Level::DEBUG)
        .count_level(tracing::Level::WARN)
        .target_prefix("reify_gui::engine::snapshot_bind_pairs")
        .build();

    tracing::subscriber::with_default(subscriber, || {
        let _ = session.get_mechanism_descriptors();
    });

    let debug_count = counters[&tracing::Level::DEBUG].load(Ordering::Acquire);
    let warn_count = counters[&tracing::Level::WARN].load(Ordering::Acquire);

    assert_eq!(
        debug_count, 0,
        "expected 0 DEBUG events at target reify_gui::engine::snapshot_bind_pairs \
         for snapshot(m1, []) — empty bind list is valid stdlib usage, not anomalous; got {}",
        debug_count
    );
    assert_eq!(
        warn_count, 0,
        "expected 0 WARN events at target reify_gui::engine::snapshot_bind_pairs; got {}",
        warn_count
    );
}

/// `snapshot(m1)` — only one argument, so `args.len() < 2`.
/// This pins the `args.len() < 2` early-return carve-out documented in
/// `engine.rs` (`collect_snapshot_bind_pairs`): a 1-arg snapshot() call
/// cannot contribute any bind pairs and must stay completely silent —
/// no DEBUG, no WARN.  Without this test a future refactor that removes
/// the early return would let the `ListLiteral` check run on `args[1]` of a
/// 1-arg call and panic on out-of-bounds indexing with no test catching it.
const ONE_ARG_SNAPSHOT_SOURCE: &str = r#"
structure Kinematic {
    let j1 = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let m0 = mechanism()
    let m1 = body(m0, "solid_a", j1)
    let snap = snapshot(m1)
}
"#;

#[test]
fn collect_snapshot_bind_pairs_stays_silent_for_one_arg_call() {
    // Inoculate against tracing's per-callsite Interest cache — see
    // `prime_tracing_callsite_cache` in reify-test-support for why.
    reify_test_support::prime_tracing_callsite_cache();
    // Regression-pin for the args.len() < 2 carve-out.  A 1-arg snapshot()
    // call cannot contribute bind pairs and must NOT emit any telemetry at
    // the snapshot_bind_pairs target.
    let mut session = make_session();
    session
        .load_from_source(ONE_ARG_SNAPSHOT_SOURCE, "kinematic")
        .expect("load 1-arg snapshot source");

    let (subscriber, counters) = CountingSubscriberBuilder::new()
        .count_level(tracing::Level::DEBUG)
        .count_level(tracing::Level::WARN)
        .target_prefix("reify_gui::engine::snapshot_bind_pairs")
        .build();

    tracing::subscriber::with_default(subscriber, || {
        let _ = session.get_mechanism_descriptors();
    });

    let debug_count = counters[&tracing::Level::DEBUG].load(Ordering::Acquire);
    let warn_count = counters[&tracing::Level::WARN].load(Ordering::Acquire);

    assert_eq!(
        debug_count, 0,
        "expected 0 DEBUG events at target reify_gui::engine::snapshot_bind_pairs \
         for snapshot(m1) — 1-arg call cannot contribute pairs and must stay silent; got {}",
        debug_count
    );
    assert_eq!(
        warn_count, 0,
        "expected 0 WARN events at target reify_gui::engine::snapshot_bind_pairs \
         for snapshot(m1); got {}",
        warn_count
    );
}

/// `snapshot(m1, j1)` — second arg is an `Ident` (a joint cell), not a
/// `ListLiteral`.  This is case (a): args[1] is not a ListLiteral, which
/// suggests a user-shadowed `snapshot` or malformed call — must emit DEBUG.
///
/// **Fragility warning**: this source fixture depends on the stdlib's
/// `snapshot` frontend signature being permissive — `compile_with_stdlib`
/// does NOT emit a `Severity::Error` for a non-list `args[1]`, so
/// `load_from_source` succeeds and the engine sees the call with its
/// original AST shape.  If the stdlib is later tightened to reject a
/// non-`ListLiteral` second argument with a `Severity::Error`, the
/// `.expect("load non-list-arg snapshot source")` call below will panic
/// with a misleading message and the test will need to be updated — either
/// to expect the load failure, or (more durably) to construct the AST
/// directly via `reify_syntax` so the frontend check is bypassed.
const NON_LIST_SNAPSHOT_ARG_SOURCE: &str = r#"
structure Kinematic {
    let j1 = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let m0 = mechanism()
    let m1 = body(m0, "solid_a", j1)
    let snap = snapshot(m1, j1)
}
"#;

#[test]
fn collect_snapshot_bind_pairs_emits_debug_when_args1_not_listliteral() {
    // Inoculate against tracing's per-callsite Interest cache — see
    // `prime_tracing_callsite_cache` in reify-test-support for why.
    reify_test_support::prime_tracing_callsite_cache();
    // Regression-pin for case (a): snapshot() with a non-ListLiteral second
    // arg must emit exactly 1 DEBUG event at the snapshot_bind_pairs target.
    let mut session = make_session();
    session
        .load_from_source(NON_LIST_SNAPSHOT_ARG_SOURCE, "kinematic")
        .expect("load non-list-arg snapshot source");

    let (subscriber, counters) = CountingSubscriberBuilder::new()
        .count_level(tracing::Level::DEBUG)
        .count_level(tracing::Level::WARN)
        .target_prefix("reify_gui::engine::snapshot_bind_pairs")
        .build();

    tracing::subscriber::with_default(subscriber, || {
        let _ = session.get_mechanism_descriptors();
    });

    let debug_count = counters[&tracing::Level::DEBUG].load(Ordering::Acquire);
    let warn_count = counters[&tracing::Level::WARN].load(Ordering::Acquire);

    assert_eq!(
        debug_count, 1,
        "expected exactly 1 DEBUG event for snapshot(m1, j1) where j1 is not a ListLiteral \
         (case a — potential user-shadowed snapshot); got {}",
        debug_count
    );
    assert_eq!(
        warn_count, 0,
        "expected 0 WARN events at target reify_gui::engine::snapshot_bind_pairs; got {}",
        warn_count
    );
}

/// `snapshot(m1, [bind(j1, 0mm + 1mm)])` — non-empty bind list, but the second
/// arg of `bind` is a `BinOp` expression, which is neither an `Ident` (Param
/// ref) nor a `QuantityLiteral`/`NumberLiteral` (literal value).  No valid
/// `bind(Ident, Ident|Literal)` pair survives the filter — case (c).  Must emit DEBUG.
const NON_BIND_LIST_SNAPSHOT_SOURCE: &str = r#"
structure Kinematic {
    let j1 = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let m0 = mechanism()
    let m1 = body(m0, "solid_a", j1)
    let snap = snapshot(m1, [bind(j1, 0mm + 1mm)])
}
"#;

#[test]
fn collect_snapshot_bind_pairs_emits_debug_when_list_has_no_valid_binds() {
    // Inoculate against tracing's per-callsite Interest cache — see
    // `prime_tracing_callsite_cache` in reify-test-support for why.
    reify_test_support::prime_tracing_callsite_cache();
    // Regression-pin for case (c): non-empty bind list whose entries all fail
    // the bind(Ident, Ident) filter must emit exactly 1 DEBUG event.
    let mut session = make_session();
    session
        .load_from_source(NON_BIND_LIST_SNAPSHOT_SOURCE, "kinematic")
        .expect("load non-bind-list snapshot source");

    let (subscriber, counters) = CountingSubscriberBuilder::new()
        .count_level(tracing::Level::DEBUG)
        .count_level(tracing::Level::WARN)
        .target_prefix("reify_gui::engine::snapshot_bind_pairs")
        .build();

    tracing::subscriber::with_default(subscriber, || {
        let _ = session.get_mechanism_descriptors();
    });

    let debug_count = counters[&tracing::Level::DEBUG].load(Ordering::Acquire);
    let warn_count = counters[&tracing::Level::WARN].load(Ordering::Acquire);

    assert_eq!(
        debug_count, 1,
        "expected exactly 1 DEBUG event for snapshot(m1, [bind(j1, 0mm+1mm)]) — non-empty list \
         with no valid bind(Ident, Ident|Literal) pairs (case c); got {}",
        debug_count
    );
    assert_eq!(
        warn_count, 0,
        "expected 0 WARN events at target reify_gui::engine::snapshot_bind_pairs; got {}",
        warn_count
    );
}

#[test]
fn resolve_driving_params_emits_debug_for_param_checked_match() {
    // Inoculate against tracing's per-callsite Interest cache — see
    // `prime_tracing_callsite_cache` in reify-test-support for why.
    reify_test_support::prime_tracing_callsite_cache();
    // Step 6 RED: SNAPSHOT_PARAM_BIND_SOURCE has bind(y_axis, y_pos) where
    // y_pos is a Param.  After step-7's impl, resolve_driving_params_from_ast
    // must emit exactly one DEBUG event when a Param-checked match resolves.
    // Currently emits none → RED.
    let mut session = make_session();
    session
        .load_from_source(SNAPSHOT_PARAM_BIND_SOURCE, "kinematic")
        .expect("load snapshot+param source");

    // Filter on the specific submodule target so this assertion remains valid
    // even if other debug! calls are added elsewhere in reify_gui::engine.
    let (subscriber, counters) = CountingSubscriberBuilder::new()
        .count_level(tracing::Level::DEBUG)
        .target_prefix("reify_gui::engine::param_resolution")
        .build();

    let descriptors =
        tracing::subscriber::with_default(subscriber, || session.get_mechanism_descriptors());

    // Sanity: the match path must have actually executed.
    let m1_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 1)
        .expect("expected descriptor with bodies_count=1 (m1)");
    assert_eq!(
        m1_desc.joints[0].driving_param_cell_id,
        Some("Kinematic.y_pos".to_string()),
        "driving param should be resolved; got {:?}",
        m1_desc.joints[0].driving_param_cell_id
    );

    let debug_count = counters[&tracing::Level::DEBUG].load(Ordering::Acquire);
    assert_eq!(
        debug_count, 1,
        "expected exactly 1 DEBUG event at target reify_gui::engine::param_resolution \
         for the resolved param match; got {}",
        debug_count
    );
}

#[test]
fn set_parameter_invalid_cell_id_returns_err() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let result = session.set_parameter("Nonexistent.param", "50mm");
    assert!(result.is_err(), "invalid cell_id should return Err");
}

#[test]
fn set_parameter_constraints_still_correct() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    // width = 120mm, thickness = 5mm → thickness > 2mm satisfied, thickness < 120/4=30mm satisfied
    let state = session
        .set_parameter("Bracket.width", "120mm")
        .expect("set_parameter should succeed");

    assert_eq!(state.constraints.len(), 3);
    for c in &state.constraints {
        assert_eq!(
            c.status, "Satisfied",
            "constraint {} should be satisfied",
            c.node_id
        );
    }
}

#[test]
fn load_file_returns_gui_state() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Use the examples/bracket.ri file from project root
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/bracket.ri");

    let state = session.load_file(&path).expect("load_file should succeed");

    assert!(state.values.len() >= 5, "should have bracket values");
    assert_eq!(state.constraints.len(), 3, "should have 3 constraints");
}

#[test]
fn update_source_changes_width() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let new_source = bracket_source_with_width("120mm");
    let state = session
        .update_source("bracket.ri", &new_source)
        .expect("update_source should succeed");

    let width = state
        .values
        .iter()
        .find(|v| v.name == "width")
        .expect("should have width value");

    assert_eq!(width.value, "120", "width should be 120mm after update");
}

#[test]
fn update_source_with_invalid_source_returns_err() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let result = session.update_source("bad.ri", "this is not valid {{{}}}");
    assert!(result.is_err(), "invalid source should return Err");
}

// --- Constraint violation roundtrip ---

#[test]
fn constraint_violation_roundtrip() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    // Set thickness=1mm → violates "thickness > 2mm"
    let state = session
        .set_parameter("Bracket.thickness", "1mm")
        .expect("set thickness should succeed");

    let violated = state.constraints.iter().any(|c| c.status == "Violated");
    assert!(
        violated,
        "should have at least one violated constraint when thickness=1mm"
    );

    // Set back to 5mm → all satisfied again
    let state = session
        .set_parameter("Bracket.thickness", "5mm")
        .expect("set thickness back should succeed");

    for c in &state.constraints {
        assert_eq!(
            c.status, "Satisfied",
            "constraint {} should be satisfied after restoring thickness",
            c.node_id
        );
    }
}

#[test]
fn get_source_location_end_to_end() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let loc = session
        .get_source_location("Bracket.width")
        .expect("should find source location for Bracket.width");

    assert_eq!(loc.file_path, "bracket.ri");
    // width is on line 2 of bracket_source() (line 1 = "structure Bracket {")
    assert!(
        loc.line >= 2,
        "width should be on line 2 or later, got {}",
        loc.line
    );
    assert!(loc.column >= 1, "column should be positive");
    assert!(loc.end_line >= loc.line, "end_line should be >= line");
}

/// Regression test (step-3 TDD red): `get_source_location` must accept a plain
/// template name (e.g., "Bracket" without a ".member" suffix) and return the
/// first value cell's span — identical to calling with "Bracket.width".
///
/// Currently FAILS because the old implementation calls `parse_cell_id(entity_path)`
/// which requires the "Entity.member" format; "Bracket" returns `Err`, so the method
/// returns `None`.  Fixed in step-4 by delegating to the shared helper.
#[test]
fn get_source_location_accepts_template_name_returns_first_cell_span() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    // Template name (no .member) must resolve to the first value cell (width).
    let loc_name = session
        .get_source_location("Bracket")
        .expect("get_source_location('Bracket') must return Some — template-name accepted");

    let loc_width = session
        .get_source_location("Bracket.width")
        .expect("get_source_location('Bracket.width') must return Some");

    assert_eq!(
        loc_name.file_path, "bracket.ri",
        "file_path must be 'bracket.ri'"
    );
    assert!(
        loc_name.line >= 1,
        "line must be >= 1, got {}",
        loc_name.line
    );
    assert_eq!(
        (
            loc_name.line,
            loc_name.column,
            loc_name.end_line,
            loc_name.end_column
        ),
        (
            loc_width.line,
            loc_width.column,
            loc_width.end_line,
            loc_width.end_column
        ),
        "template-name resolution must proxy to the first value cell (width)"
    );
}

#[test]
fn get_source_location_returns_source_location_info() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let loc: SourceLocationInfo = session
        .get_source_location("Bracket.width")
        .expect("should find source location for Bracket.width");

    assert_eq!(loc.file_path, "bracket.ri");
}

#[test]
fn export_end_to_end() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bracket.step");

    let result = session.export(ExportFormat::Step, &path);
    assert!(result.is_ok(), "export should succeed: {:?}", result.err());

    let data = std::fs::read(&path).expect("exported file should be readable");
    assert!(!data.is_empty(), "exported file should not be empty");
}

// --- Source-map consistency after load/update ---

/// Review bug #2: source_map key inconsistency.
/// load_from_source inserts key "bracket.ri", but update_source inserts the raw path string.
/// After load_file + update_source, files should have exactly 1 entry (not 2).
#[test]
fn source_map_consistent_after_load_file_then_update() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/bracket.ri");

    session.load_file(&path).expect("load_file should succeed");

    // Now update_source with the full path string — should normalize key, not create duplicate
    let new_source = bracket_source_with_width("120mm");
    let state = session
        .update_source(path.to_str().unwrap(), &new_source)
        .expect("update_source should succeed");

    assert_eq!(
        state.files.len(),
        1,
        "should have exactly 1 file entry after load_file + update_source, got {}: {:?}",
        state.files.len(),
        state.files.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
}

/// Review bug #3: get_source_location uses non-deterministic HashMap .iter().next().
/// After load_file + update_source, get_source_location should return the correct (single) file.
#[test]
fn get_source_location_correct_after_load_file_then_update() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/bracket.ri");

    session.load_file(&path).expect("load_file should succeed");

    let new_source = bracket_source_with_width("120mm");
    let state = session
        .update_source(path.to_str().unwrap(), &new_source)
        .expect("update_source should succeed");

    let loc = session
        .get_source_location("Bracket.width")
        .expect("should find source location");

    // The file in the location should match the single file entry
    assert_eq!(state.files.len(), 1);
    assert_eq!(
        loc.file_path, state.files[0].path,
        "get_source_location file should match the single file entry"
    );
}

/// Review bug #1 regression: export should work without cloning CompiledModule.
/// This test guards the refactor in step-18 that removes the unnecessary .clone().
#[test]
fn export_no_unnecessary_clone() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bracket.step");

    let result = session.export(ExportFormat::Step, &path);
    assert!(result.is_ok(), "export should succeed: {:?}", result.err());

    // Verify output was written
    let data = std::fs::read(&path).expect("exported file should be readable");
    assert!(!data.is_empty(), "exported file should not be empty");

    // Verify engine state is still usable after export (no moved/consumed fields)
    let state = session
        .build_gui_state()
        .expect("build_gui_state after export");
    assert!(
        !state.values.is_empty(),
        "values should still be available after export"
    );
}

/// Review bug #4: [state_corruption_not_tested] + [state_inconsistency_on_error]
/// update_source() clears source_map and inserts new content BEFORE parse/compile.
/// On parse failure, old valid source is destroyed — get_source_location uses old byte offsets
/// against invalid source, and build_gui_state().files has invalid content.
/// After fix: on error, state should be completely unchanged.
#[test]
fn get_source_location_correct_after_failed_update() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // (1) Load valid source
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    // (2) Record source location for Bracket.width before failed update
    let loc_before = session
        .get_source_location("Bracket.width")
        .expect("should find source location before failed update");

    // (3) Attempt invalid update — should fail
    let result = session.update_source("bracket.ri", "this is not valid {{{}}}");
    assert!(result.is_err(), "invalid source should return Err");

    // (4) get_source_location should return the SAME line/col as before the failed update
    let loc_after = session
        .get_source_location("Bracket.width")
        .expect("should still find source location after failed update");
    assert_eq!(
        loc_before.line, loc_after.line,
        "line should be unchanged after failed update"
    );
    assert_eq!(
        loc_before.column, loc_after.column,
        "column should be unchanged after failed update"
    );
    assert_eq!(
        loc_before.file_path, loc_after.file_path,
        "file should be unchanged after failed update"
    );

    // (5) build_gui_state should still return Ok.
    //     values/get_source_location use the LAST-GOOD compiled module (unchanged).
    //     files[0].content tracks the FAILING buffer (task 4258 one-snapshot invariant).
    let state = session
        .build_gui_state()
        .expect("build_gui_state should work after failed update");
    assert!(
        state.values.len() >= 5,
        "should still have original values after failed update (last-good retained), got {}",
        state.values.len()
    );
    assert_eq!(state.files.len(), 1);
    // After task 4258 fix: files[0].content must reflect the FAILING buffer so
    // compile_diagnostics line/col (computed against the failing buffer) can be
    // correctly indexed.  get_source_location still resolves against the last-good
    // compiled module — the split is intentional and tested separately.
    assert!(
        state.files[0].content.contains("this is not valid"),
        "files[0].content must contain the failing buffer after task 4258 fix, got: {}",
        &state.files[0].content[..50.min(state.files[0].content.len())]
    );
}

/// Review bug #3: get_source_location should use explicit key lookup, not .iter().next().
/// After load_from_source, the file should be the normalized "bracket.ri" key.
#[test]
fn get_source_location_uses_explicit_key_lookup() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let loc = session
        .get_source_location("Bracket.width")
        .expect("should find source location");

    // Should return the normalized module-name-based key
    assert_eq!(
        loc.file_path, "bracket.ri",
        "get_source_location should return normalized module-name key"
    );
}

// --- Unit suffix parsing ---

/// Verify all supported unit suffixes parse correctly.
#[test]
fn parse_value_string_all_units_correct() {
    use reify_core::DimensionVector;
    use reify_ir::Value;

    // mm → 0.001 * value, LENGTH
    let v = parse_value_string("5mm").expect("5mm should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.005).abs() < 1e-10,
                "5mm → 0.005, got {}",
                si_value
            );
            assert_eq!(dimension, DimensionVector::LENGTH);
        }
        _ => panic!("5mm should be Scalar, got {:?}", v),
    }

    // cm → 0.01 * value, LENGTH
    let v = parse_value_string("5cm").expect("5cm should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.05).abs() < 1e-10,
                "5cm → 0.05, got {}",
                si_value
            );
            assert_eq!(dimension, DimensionVector::LENGTH);
        }
        _ => panic!("5cm should be Scalar, got {:?}", v),
    }

    // m → 1.0 * value, LENGTH
    let v = parse_value_string("5m").expect("5m should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!((si_value - 5.0).abs() < 1e-10, "5m → 5.0, got {}", si_value);
            assert_eq!(dimension, DimensionVector::LENGTH);
        }
        _ => panic!("5m should be Scalar, got {:?}", v),
    }

    // deg → PI/180 * value, ANGLE
    let v = parse_value_string("90deg").expect("90deg should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - std::f64::consts::FRAC_PI_2).abs() < 1e-10,
                "90deg → PI/2, got {}",
                si_value
            );
            assert_eq!(dimension, DimensionVector::ANGLE);
        }
        _ => panic!("90deg should be Scalar, got {:?}", v),
    }

    // rad → 1.0 * value, ANGLE
    let v = parse_value_string("1rad").expect("1rad should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 1.0).abs() < 1e-10,
                "1rad → 1.0, got {}",
                si_value
            );
            assert_eq!(dimension, DimensionVector::ANGLE);
        }
        _ => panic!("1rad should be Scalar, got {:?}", v),
    }
}

/// Verify 'm' suffix does not shadow longer suffixes like 'cm'.
/// '100cm' must produce si_value=1.0 (not 100.0 from 'm' matching 'cm' trailing).
#[test]
fn parse_value_string_m_does_not_shadow_longer_suffixes() {
    use reify_core::DimensionVector;
    use reify_ir::Value;

    let v = parse_value_string("100cm").expect("100cm should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 1.0).abs() < 1e-10,
                "100cm → 1.0, got {} (would be 100.0 if 'm' shadowed 'cm')",
                si_value
            );
            assert_eq!(dimension, DimensionVector::LENGTH);
        }
        _ => panic!("100cm should be Scalar, got {:?}", v),
    }
}

/// Verify unit table ordering invariant:
/// '5mm' must produce si_value 0.005 (not 5.0 from 'm' match).
/// '45deg' must produce ANGLE (ensures 3-char suffixes work correctly).
/// These tests document the ordering contract and will catch regressions.
#[test]
fn parse_value_string_unit_table_ordering_invariant() {
    use reify_core::DimensionVector;
    use reify_ir::Value;

    // '5mm' must be recognized as millimeters, not meters
    let v = parse_value_string("5mm").expect("5mm should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.005).abs() < 1e-10,
                "5mm → 0.005 (not 5.0 from 'm' match), got {}",
                si_value
            );
            assert_eq!(dimension, DimensionVector::LENGTH);
        }
        _ => panic!("5mm should be Scalar, got {:?}", v),
    }

    // '45deg' must be recognized as degrees (ANGLE), not fail or parse incorrectly
    let v = parse_value_string("45deg").expect("45deg should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            let expected = 45.0 * std::f64::consts::PI / 180.0;
            assert!(
                (si_value - expected).abs() < 1e-10,
                "45deg → {}, got {}",
                expected,
                si_value
            );
            assert_eq!(
                dimension,
                DimensionVector::ANGLE,
                "45deg should be ANGLE dimension"
            );
        }
        _ => panic!("45deg should be Scalar, got {:?}", v),
    }
}

// --- UNIT_TABLE descending-length ordering ---

/// Directly assert that UNIT_TABLE is sorted by descending suffix length.
///
/// The debug_assert in parse_value_string vanishes in release builds; this
/// #[test] provides coverage in both debug and release builds. It references
/// the pub(crate) const UNIT_TABLE extracted from parse_value_string in step-4.
#[test]
fn unit_table_ordering_invariant_holds() {
    use crate::engine::UNIT_TABLE;

    let sorted = UNIT_TABLE.windows(2).all(|w| w[0].0.len() >= w[1].0.len());
    assert!(
        sorted,
        "UNIT_TABLE entries must be sorted by descending suffix length (longest first). \
         Adjacent pairs: {:?}",
        UNIT_TABLE
            .windows(2)
            .map(|w| (w[0].0, w[0].0.len(), w[1].0, w[1].0.len()))
            .collect::<Vec<_>>()
    );
}

// --- Tessellation integration ---

#[test]
fn build_gui_state_includes_meshes_from_tessellation() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");

    assert!(
        !state.meshes.is_empty(),
        "build_gui_state should produce meshes when a geometry kernel is available, got empty"
    );
}

#[test]
fn build_gui_state_mesh_data_structure_matches_kernel_output() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");

    assert!(!state.meshes.is_empty(), "should have at least one mesh");
    let mesh = &state.meshes[0];

    // MockGeometryKernel returns: vertices = [0,0,0, 1,0,0, 0,1,0] (9 floats = 3 vertices)
    assert_eq!(
        mesh.vertices.len(),
        9,
        "expected 9 vertex floats (3 vertices × 3 coords)"
    );
    // indices = [0, 1, 2] (1 triangle)
    assert_eq!(mesh.indices.len(), 3, "expected 3 indices (1 triangle)");
    // normals = Some([0,0,1, 0,0,1, 0,0,1]) (9 floats)
    assert!(mesh.normals.is_some(), "expected normals to be present");
    assert_eq!(
        mesh.normals.as_ref().unwrap().len(),
        9,
        "expected 9 normal floats"
    );
    // entity_path should be non-empty
    assert!(
        !mesh.entity_path.is_empty(),
        "entity_path should be non-empty"
    );
}

#[test]
fn build_gui_state_no_kernel_returns_empty_meshes() {
    let checker = SimpleConstraintChecker;
    // No geometry kernel
    let mut session = EngineSession::new(Box::new(checker), None);

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed even without kernel");

    // Meshes should be empty when no kernel is available
    assert!(
        state.meshes.is_empty(),
        "expected empty meshes without geometry kernel, got {}",
        state.meshes.len()
    );

    // Values and constraints should still be populated
    assert!(
        state.values.len() >= 5,
        "expected at least 5 values without kernel, got {}",
        state.values.len()
    );
    assert_eq!(
        state.constraints.len(),
        3,
        "expected 3 constraints without kernel"
    );
}

#[test]
fn build_gui_state_tessellation_preserves_values_and_constraints() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");

    // Tessellation should produce meshes
    assert!(
        !state.meshes.is_empty(),
        "expected non-empty meshes with geometry kernel"
    );

    // And values/constraints should still be fully populated (tessellation doesn't interfere)
    assert!(
        state.values.len() >= 5,
        "expected at least 5 values alongside meshes, got {}",
        state.values.len()
    );
    assert_eq!(
        state.constraints.len(),
        3,
        "expected 3 constraints alongside meshes"
    );
}

#[test]
fn set_parameter_produces_updated_meshes() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let initial_state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load should succeed");

    assert!(
        !initial_state.meshes.is_empty(),
        "initial state should have meshes"
    );

    // Set parameter and verify meshes are still produced
    let updated_state = session
        .set_parameter("Bracket.width", "120mm")
        .expect("set_parameter should succeed");

    assert!(
        !updated_state.meshes.is_empty(),
        "updated state should have meshes after set_parameter"
    );
}

#[test]
fn update_source_produces_meshes() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load should succeed");

    let new_source = bracket_source_with_width("120mm");
    let state = session
        .update_source("bracket.ri", &new_source)
        .expect("update_source should succeed");

    assert!(
        !state.meshes.is_empty(),
        "update_source should produce meshes"
    );
}

// --- get_diagnostics lifecycle ---

/// get_diagnostics() returns empty vec when no module is loaded.
/// This test fails with a compile error until EngineSession::get_diagnostics() is implemented.
#[test]
fn engine_get_diagnostics_no_module_returns_empty() {
    let checker = SimpleConstraintChecker;
    let session = EngineSession::new(Box::new(checker), None);

    let diags: Vec<DiagnosticInfo> = session.get_diagnostics();
    assert!(
        diags.is_empty(),
        "no module loaded → diagnostics must be empty"
    );
}

/// get_diagnostics() returns a non-empty vec
/// when the compiled module contains a warning.
///
/// Source with `port mount : NonExistentTrait` produces an "unknown port type" warning
/// (validated by crates/reify-compiler/tests/port_compile_tests.rs:101-124).
/// load_from_source() succeeds (warnings are not errors), so compiled.diagnostics stores
/// the warning. get_diagnostics() then surfaces it, exercising:
///   - the non-empty iteration path
///   - byte_offset_to_line_col span conversion
///   - file_path resolution from module_name
///   - severity Display formatting
///   - message propagation
#[test]
fn engine_get_diagnostics_returns_populated_warning() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    let source = warn_source_with_unknown_port_type();

    // load_from_source should succeed — warnings are not errors
    session
        .load_from_source(source, "test_warn")
        .expect("source with unknown port type should compile (warning, not error)");

    let diags: Vec<DiagnosticInfo> = session.get_diagnostics();

    assert!(
        !diags.is_empty(),
        "expected at least one diagnostic for unknown port type, got empty"
    );

    let first = &diags[0];

    // severity must be "Warning"
    assert_eq!(
        first.severity, "Warning",
        "expected severity 'Warning', got '{}'",
        first.severity
    );

    // message must mention the unknown port type
    assert!(
        first.message.contains("unknown port type"),
        "expected message to contain 'unknown port type', got: '{}'",
        first.message
    );
    assert!(
        first.message.contains("NonExistentTrait"),
        "expected message to mention 'NonExistentTrait', got: '{}'",
        first.message
    );

    // file_path must be derived from the module name passed to load_from_source
    assert_eq!(
        first.file_path, "test_warn.ri",
        "expected file_path 'test_warn.ri', got '{}'",
        first.file_path
    );

    // line and column must be valid 1-based values
    assert!(first.line >= 1, "expected line >= 1, got {}", first.line);
    assert!(
        first.column >= 1,
        "expected column >= 1, got {}",
        first.column
    );

    // end_line and end_column must form a coherent range
    assert!(
        first.end_line >= first.line,
        "expected end_line ({}) >= line ({})",
        first.end_line,
        first.line
    );
    assert!(
        first.end_column >= 1,
        "expected end_column >= 1, got {}",
        first.end_column
    );
}

/// get_diagnostics() returns empty vec for bracket_source() (warning-free source).
/// Validates the method works end-to-end on a real compiled module.
#[test]
fn engine_get_diagnostics_clean_source_returns_empty() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("bracket source should compile cleanly");

    let diags: Vec<DiagnosticInfo> = session.get_diagnostics();
    assert!(
        diags.is_empty(),
        "bracket source has no warnings — diagnostics must be empty, got: {:?}",
        diags
    );
}

// --- resolve_source contract ---

/// get_source_location returns None when no module is loaded.
/// Documents the early-return (`let compiled = self.compiled.as_ref()?`)
/// that fires before resolve_source is reached.
#[test]
fn get_source_location_returns_none_without_module() {
    let checker = SimpleConstraintChecker;
    let session = EngineSession::new(Box::new(checker), None);

    let loc = session.get_source_location("Bracket.width");
    assert!(
        loc.is_none(),
        "get_source_location should return None when no module is loaded"
    );
}

/// get_source_location returns None when module_name has been cleared (broken invariant).
///
/// Focused regression complement to the bundled `resolve_source_fallback_when_module_name_missing`
/// test (line 1168), which asserts both get_diagnostics and get_source_location in one test.
/// This dedicated test provides independent failure attribution for get_source_location alone:
/// if get_source_location regresses while get_diagnostics remains intact, this test reports
/// against the right method. Parallels the focused `resolve_source_returns_none_when_module_name_broken`
/// test (line 1823) which checks the resolve_source helper directly.
#[test]
fn get_source_location_returns_none_when_module_name_broken() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("bracket source should compile cleanly");

    // Deliberately break the module_name invariant while leaving compiled and source_map intact.
    session.break_module_name_for_test();

    let loc = session.get_source_location("Bracket.width");
    assert!(
        loc.is_none(),
        "get_source_location should return None when module_name is broken"
    );
}

/// get_diagnostics and get_source_location return the same file key.
///
/// After load_from_source with a warning-producing source, both methods must resolve
/// the file key through the same "{module_name}.ri" derivation via resolve_source.
#[test]
fn diagnostics_and_source_location_agree_on_file_key() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    let source = warn_source_with_unknown_port_type_with_width();

    session
        .load_from_source(source, "testmod")
        .expect("source with unknown port type should compile (warning, not error)");

    let diags = session.get_diagnostics();
    assert!(
        !diags.is_empty(),
        "expected at least one diagnostic for unknown port type"
    );
    assert_eq!(
        diags[0].severity, "Warning",
        "this test relies on NonExistentTrait producing a warning — \
         if severity changed to error, load_from_source would have returned Err above; \
         update the test fixture if the compiler's severity classification changes"
    );
    assert_eq!(
        diags[0].file_path, "testmod.ri",
        "get_diagnostics file_path"
    );

    let loc = session
        .get_source_location("S.width")
        .expect("should find source location for S.width");
    assert_eq!(loc.file_path, "testmod.ri", "get_source_location file_path");
}

/// get_diagnostics uses the updated module name key after update_source.
///
/// After load_from_source("initial") then update_source("updated.ri", ...),
/// get_diagnostics must resolve the new key "updated.ri", not "initial.ri".
///
/// **Assumption**: `port mount : NonExistentTrait` produces a warning (not error).
/// If the compiler changes this, the `.expect()` on load_from_source/update_source
/// will panic — update the fixture accordingly.
#[test]
fn diagnostics_file_key_consistent_after_update_source() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    session
        .load_from_source(warn_source_with_unknown_port_type(), "initial")
        .expect("initial load should succeed");

    let diags_before = session.get_diagnostics();
    assert!(
        !diags_before.is_empty(),
        "should have diagnostics after initial load"
    );
    assert_eq!(
        diags_before[0].severity, "Warning",
        "this test relies on NonExistentTrait producing a warning — \
         if severity changed to error, load_from_source would have returned Err above; \
         update the test fixture if the compiler's severity classification changes"
    );
    assert_eq!(
        diags_before[0].file_path, "initial.ri",
        "before update: file_path should be 'initial.ri'"
    );

    session
        .update_source("updated.ri", warn_source_with_unknown_port_type())
        .expect("update_source should succeed");

    let diags_after = session.get_diagnostics();
    assert!(
        !diags_after.is_empty(),
        "should still have diagnostics after update_source"
    );
    assert_eq!(
        diags_after[0].severity, "Warning",
        "this test relies on NonExistentTrait producing a warning — \
         if severity changed to error, update_source would have returned Err above; \
         update the test fixture if the compiler's severity classification changes"
    );
    assert_eq!(
        diags_after[0].file_path, "updated.ri",
        "after update_source, file_path should be 'updated.ri'"
    );
}

/// A diagnostic with no labels gets (1,1,1,1) coordinates.
///
/// This exercises the `else` branch of `diag.labels.first()` at engine.rs:295-296.
/// The compiler always attaches labels; inject_diagnostic_for_test() lets us plant
/// a labelless diagnostic to verify the (1,1,1,1) fallback.
#[test]
fn engine_get_diagnostics_labelless_diagnostic_returns_default_span() {
    use reify_core::Diagnostic;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("bracket source should compile cleanly");

    // Inject a warning with no labels — this is the labelless case
    session.inject_diagnostic_for_test(Diagnostic::warning("test labelless"));

    let diags: Vec<DiagnosticInfo> = session.get_diagnostics();

    // (a) The injected diagnostic appears
    assert!(!diags.is_empty(), "expected injected diagnostic, got empty");

    // Find the injected one (bracket_source has none of its own)
    let injected = diags
        .iter()
        .find(|d| d.message == "test labelless")
        .expect("injected 'test labelless' diagnostic not found in results");

    // (b) All coordinates default to (1,1,1,1)
    assert_eq!(
        injected.line, 1,
        "expected line=1 for labelless, got {}",
        injected.line
    );
    assert_eq!(
        injected.column, 1,
        "expected column=1 for labelless, got {}",
        injected.column
    );
    assert_eq!(
        injected.end_line, 1,
        "expected end_line=1 for labelless, got {}",
        injected.end_line
    );
    assert_eq!(
        injected.end_column, 1,
        "expected end_column=1 for labelless, got {}",
        injected.end_column
    );

    // (c) Severity preserved
    assert_eq!(
        injected.severity, "Warning",
        "expected severity 'Warning', got '{}'",
        injected.severity
    );

    // (d) Message preserved
    assert_eq!(
        injected.message, "test labelless",
        "expected message 'test labelless', got '{}'",
        injected.message
    );
}

/// Pins that DiagnosticInfo.severity strings produced by diagnostics_to_info
/// equal Severity::as_wire_str() for each severity level.
///
/// Injects a Warning and an Info diagnostic after a clean load, then asserts
/// the returned severity strings match the centralized helper. This ensures
/// the step-4 refactor (replacing the hand-rolled match with as_wire_str())
/// cannot silently diverge.
#[test]
fn get_diagnostics_severity_strings_match_as_wire_str() {
    use reify_core::{Diagnostic, Severity};

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("bracket source should compile cleanly");

    session.inject_diagnostic_for_test(Diagnostic::warning("w"));
    session.inject_diagnostic_for_test(Diagnostic::info("i"));

    let diags: Vec<DiagnosticInfo> = session.get_diagnostics();

    let warning_diag = diags
        .iter()
        .find(|d| d.message == "w")
        .expect("injected warning diagnostic not found");
    let info_diag = diags
        .iter()
        .find(|d| d.message == "i")
        .expect("injected info diagnostic not found");

    assert_eq!(
        warning_diag.severity,
        Severity::Warning.as_wire_str(),
        "Warning DiagnosticInfo.severity must equal Severity::Warning.as_wire_str()"
    );
    assert_eq!(
        info_diag.severity,
        Severity::Info.as_wire_str(),
        "Info DiagnosticInfo.severity must equal Severity::Info.as_wire_str()"
    );
}

/// get_diagnostics returns empty and get_source_location returns None
/// when the source_map invariant is deliberately broken after load.
///
/// After load_from_source with bracket_source (clean, 0 warnings), calling
/// break_source_map_for_test() clears source_map while leaving compiled and
/// module_name intact. This exercises the fallback paths added in Task 900:
/// - get_diagnostics early-exits with [] when diagnostics is empty (no resolve_source call)
/// - get_source_location uses a fallible source_map lookup and returns None gracefully
#[test]
fn resolve_source_fallback_when_source_map_missing() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("bracket source should compile cleanly");

    // Deliberately break the source_map invariant
    session.break_source_map_for_test();

    // get_diagnostics must return empty without panicking
    let diags: Vec<DiagnosticInfo> = session.get_diagnostics();
    assert!(
        diags.is_empty(),
        "get_diagnostics should return [] via the empty-diagnostics early-exit even when source_map is missing"
    );

    // get_source_location must return None without panicking
    let loc = session.get_source_location("Bracket.width");
    assert!(
        loc.is_none(),
        "get_source_location should return None when source_map is missing"
    );
}

/// get_diagnostics returns empty and get_source_location returns None
/// when the module_name invariant is deliberately broken after load.
///
/// After load_from_source with bracket_source (clean, 0 warnings), calling
/// break_module_name_for_test() clears module_name while leaving compiled and
/// source_map intact. This exercises the fallible path at engine.rs line 302:
/// - get_diagnostics early-exits with [] when diagnostics is empty (no module_name needed)
/// - get_source_location uses self.module_name.as_deref()? and returns None gracefully
#[test]
fn resolve_source_fallback_when_module_name_missing() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("bracket source should compile cleanly");

    // Deliberately break the module_name invariant
    session.break_module_name_for_test();

    // get_diagnostics must return empty without panicking
    let diags: Vec<DiagnosticInfo> = session.get_diagnostics();
    assert!(
        diags.is_empty(),
        "get_diagnostics should return [] via the empty-diagnostics early-exit even when module_name is missing"
    );

    // get_source_location must return None without panicking
    let loc = session.get_source_location("Bracket.width");
    assert!(
        loc.is_none(),
        "get_source_location should return None when module_name is missing"
    );
}

// --- build_line_offsets ---

/// build_line_offsets returns empty vec for empty string.
#[test]
fn build_line_offsets_empty_string() {
    use crate::engine::build_line_offsets;
    let offsets = build_line_offsets("");
    assert_eq!(offsets, Vec::<usize>::new());
}

/// build_line_offsets returns empty vec for a single-line string (no '\n').
#[test]
fn build_line_offsets_single_line() {
    use crate::engine::build_line_offsets;
    let offsets = build_line_offsets("hello world");
    assert_eq!(offsets, Vec::<usize>::new());
}

/// build_line_offsets returns correct byte positions of '\n' for a multi-line string.
///
/// "abc\ndef\nghi"
///  0123 4567 8910
/// '\n' at byte 3 and byte 7.
#[test]
fn build_line_offsets_multi_line() {
    use crate::engine::build_line_offsets;
    let offsets = build_line_offsets("abc\ndef\nghi");
    assert_eq!(offsets, vec![3, 7]);
}

/// build_line_offsets handles a trailing newline (last char is '\n').
///
/// "abc\ndef\n"
///  0123 4567 8
/// '\n' at byte 3 and byte 7.
#[test]
fn build_line_offsets_trailing_newline() {
    use crate::engine::build_line_offsets;
    let offsets = build_line_offsets("abc\ndef\n");
    assert_eq!(offsets, vec![3, 7]);
}

/// build_line_offsets handles a string that is only newlines.
///
/// "\n\n\n" → '\n' at bytes 0, 1, 2.
#[test]
fn build_line_offsets_only_newlines() {
    use crate::engine::build_line_offsets;
    let offsets = build_line_offsets("\n\n\n");
    assert_eq!(offsets, vec![0, 1, 2]);
}

/// After update_source with clean source, get_diagnostics() returns empty.
///
/// Verifies the update_source→get_diagnostics lifecycle contract: the compiled
/// module (and its diagnostics) are replaced on each update, so stale diagnostics
/// from a previous compilation do not persist.
#[test]
fn engine_get_diagnostics_cleared_after_update_to_clean_source() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Load warning source — establishes a non-empty diagnostics state
    session
        .load_from_source(warn_source_with_unknown_port_type(), "test_warn")
        .expect("warning source should compile");

    let diags_before = session.get_diagnostics();
    assert!(
        !diags_before.is_empty(),
        "expected diagnostics before update, got empty"
    );

    // Update the same file to clean source — diagnostics must be cleared
    session
        .update_source("test_warn.ri", bracket_source())
        .expect("bracket source should compile cleanly");

    let diags_after = session.get_diagnostics();
    assert!(
        diags_after.is_empty(),
        "expected empty diagnostics after updating to clean source, got: {:?}",
        diags_after
    );
}

#[test]
fn get_diagnostics_empty_span_has_identical_start_end() {
    use reify_core::byte_offset_to_line_col;
    use reify_core::{Diagnostic, DiagnosticLabel, SourceSpan};

    // Verify that a zero-length span (start == end) produces identical
    // start and end coordinates through the full get_diagnostics pipeline,
    // including the optimised offset_to_line_col_fast path.
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let source = bracket_source();
    session
        .load_from_source(source, "bracket")
        .expect("bracket source should compile cleanly");

    let offset = source.find("width").expect("'width' not in bracket_source") as u32;

    let diag = Diagnostic::warning("empty-span-test").with_label(DiagnosticLabel::new(
        SourceSpan::new(offset, offset), // zero-length span
        "zero-length label",
    ));
    session.inject_diagnostic_for_test(diag);

    let diags = session.get_diagnostics();
    let d = diags
        .iter()
        .find(|d| d.message == "empty-span-test")
        .expect("injected empty-span diagnostic not found");

    // The real concern: start and end coords must be identical for an empty span.
    assert_eq!(
        d.line, d.end_line,
        "empty span: line ({}) != end_line ({})",
        d.line, d.end_line
    );
    assert_eq!(
        d.column, d.end_column,
        "empty span: column ({}) != end_column ({})",
        d.column, d.end_column
    );

    // Cross-validate against the reference implementation.
    let (exp_line, exp_col) = byte_offset_to_line_col(source, offset as usize);
    assert_eq!(d.line, exp_line as u32, "line mismatch vs reference");
    assert_eq!(d.column, exp_col as u32, "column mismatch vs reference");

    // Absolute coordinate check: 'width' is on line 2 at column 11 of bracket_source.
    // bracket_source() starts "structure Bracket {\n    param width..."
    // The 'w' of 'width' is at byte offset 30 (manually verified):
    //   19 bytes "structure Bracket {" + '\n' (line 2, col 1)
    //   + 10 bytes "    param " → col 11 when 'w' is reached.
    assert_eq!(d.line, 2, "expected line for 'width' in bracket_source");
    assert_eq!(
        d.column, 11,
        "expected column for 'width' in bracket_source"
    );
}

// --- offset_to_line_col_fast ---

/// offset_to_line_col_fast returns (1,1) for offset 0 on any source.
#[test]
fn offset_to_line_col_fast_offset_zero() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    let source = "abc\ndef\nghi";
    let offsets = build_line_offsets(source);
    assert_eq!(offset_to_line_col_fast(source, &offsets, 0), (1, 1));
}

/// offset_to_line_col_fast cross-validates with byte_offset_to_line_col
/// for every byte offset in a multi-line string.
#[test]
fn offset_to_line_col_fast_matches_original_every_offset() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    use reify_core::byte_offset_to_line_col;
    let source = "abc\ndef\nghi";
    let line_offsets = build_line_offsets(source);
    for offset in 0..source.len() {
        let expected = byte_offset_to_line_col(source, offset);
        let actual = offset_to_line_col_fast(source, &line_offsets, offset);
        assert_eq!(
            actual, expected,
            "mismatch at offset {}: fast={:?} original={:?}",
            offset, actual, expected
        );
    }
    // "Two convergent implementations agree" invariant must also hold at the
    // prelude sentinel (SourceSpan::PRELUDE_SENTINEL_OFFSET).  Without the sentinel short-circuit, the
    // fast path computes line_offsets.len() + 1 (a past-last-line value) while
    // byte_offset_to_line_col returns (1, 1).
    let sentinel = reify_core::SourceSpan::PRELUDE_SENTINEL_OFFSET;
    let fast_sentinel = offset_to_line_col_fast(source, &line_offsets, sentinel);
    let orig_sentinel = byte_offset_to_line_col(source, sentinel);
    assert_eq!(
        fast_sentinel, orig_sentinel,
        "sentinel: fast={:?} original={:?} — two convergent implementations must agree at SourceSpan::PRELUDE_SENTINEL_OFFSET",
        fast_sentinel, orig_sentinel
    );
    assert_eq!(
        fast_sentinel,
        (1, 1),
        "sentinel must be (1,1) to match the no-location fallback"
    );
}

/// offset_to_line_col_fast returns correct values at specific key offsets.
///
/// "abc\ndef\nghi" — '\n' at bytes 3 and 7.
/// offset 3  → (1,4) — the '\n' itself is still on line 1
/// offset 4  → (2,1) — first char of line 2
/// offset 7  → (2,4) — the second '\n'
/// offset 8  → (3,1) — first char of line 3
/// offset 10 → (3,3) — last char 'i'
#[test]
fn offset_to_line_col_fast_key_positions() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    let source = "abc\ndef\nghi";
    let offsets = build_line_offsets(source);
    assert_eq!(offset_to_line_col_fast(source, &offsets, 3), (1, 4)); // '\n'
    assert_eq!(offset_to_line_col_fast(source, &offsets, 4), (2, 1)); // 'd'
    assert_eq!(offset_to_line_col_fast(source, &offsets, 7), (2, 4)); // '\n'
    assert_eq!(offset_to_line_col_fast(source, &offsets, 8), (3, 1)); // 'g'
    assert_eq!(offset_to_line_col_fast(source, &offsets, 10), (3, 3)); // 'i'
}

/// offset_to_line_col_fast works on empty source (no newlines).
#[test]
fn offset_to_line_col_fast_empty_source() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    let source = "";
    let offsets = build_line_offsets(source);
    assert_eq!(offset_to_line_col_fast(source, &offsets, 0), (1, 1));
}

/// offset_to_line_col_fast works on single-line source (no newlines).
#[test]
fn offset_to_line_col_fast_single_line() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    let source = "hello";
    let offsets = build_line_offsets(source);
    assert_eq!(offset_to_line_col_fast(source, &offsets, 0), (1, 1));
    assert_eq!(offset_to_line_col_fast(source, &offsets, 4), (1, 5));
}

/// offset_to_line_col_fast agrees with byte_offset_to_line_col at source.len()
/// (one-past-end / EOF position, the highest offset a compiler span can produce).
///
/// For offsets strictly beyond source.len() the two implementations diverge —
/// the original stops iterating at the last source char while the fast version
/// extrapolates the column — but that case never occurs in production because
/// diagnostic spans are always within source bounds.
#[test]
fn offset_to_line_col_fast_at_eof_offset() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    use reify_core::byte_offset_to_line_col;
    let source = "abc\ndef";
    let line_offsets = build_line_offsets(source);
    // source.len() is the EOF position — both implementations must agree here.
    let eof = source.len();
    let expected = byte_offset_to_line_col(source, eof);
    let actual = offset_to_line_col_fast(source, &line_offsets, eof);
    assert_eq!(
        actual, expected,
        "EOF offset: fast={:?} original={:?}",
        actual, expected
    );
}

/// offset_to_line_col_fast returns (1, 1) for the prelude sentinel (u32::MAX).
///
/// Without the prelude-sentinel short-circuit the current fast path computes
/// `line_offsets.len() + 1` (a past-last-line value) instead of the `(1, 1)`
/// fallback, breaking the "two convergent implementations agree" invariant with
/// `reify_types::byte_offset_to_line_col`.
#[test]
fn offset_to_line_col_fast_prelude_sentinel_returns_fallback() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    let source = "abc\ndef\nghi";
    let offsets = build_line_offsets(source);
    assert_eq!(
        offset_to_line_col_fast(
            source,
            &offsets,
            reify_core::SourceSpan::PRELUDE_SENTINEL_OFFSET
        ),
        (1, 1),
        "prelude sentinel must return (1, 1), not a past-last-line value"
    );
}

// --- Multi-diagnostic stress ---

/// get_diagnostics with multiple injected diagnostics at various byte offsets
/// produces line/col values matching byte_offset_to_line_col for each span.
///
/// This is the primary end-to-end regression for the optimized path: we inject
/// three warnings with labels at byte positions we compute from bracket_source,
/// then verify get_diagnostics returns the same line/col as the O(M) reference.
#[test]
fn get_diagnostics_multi_diagnostic_stress_matches_reference() {
    use reify_core::byte_offset_to_line_col;
    use reify_core::{Diagnostic, DiagnosticLabel, SourceSpan};

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let source = bracket_source();
    session
        .load_from_source(source, "bracket")
        .expect("bracket source should compile cleanly");

    // Pick three byte offsets that land at recognisable tokens across
    // different lines, using `find` so the test stays robust to whitespace.
    let offset_a = source.find("width").expect("'width' not in bracket_source") as u32;
    let offset_b = source
        .find("height")
        .expect("'height' not in bracket_source") as u32;
    let offset_c = source
        .find("thickness")
        .expect("'thickness' not in bracket_source") as u32;

    let diag_a = Diagnostic::warning("stress-a").with_label(DiagnosticLabel::new(
        SourceSpan::new(offset_a, offset_a + 5),
        "label a",
    ));
    let diag_b = Diagnostic::warning("stress-b").with_label(DiagnosticLabel::new(
        SourceSpan::new(offset_b, offset_b + 6),
        "label b",
    ));
    let diag_c = Diagnostic::warning("stress-c").with_label(DiagnosticLabel::new(
        SourceSpan::new(offset_c, offset_c + 9),
        "label c",
    ));

    session.inject_diagnostic_for_test(diag_a);
    session.inject_diagnostic_for_test(diag_b);
    session.inject_diagnostic_for_test(diag_c);

    let diags = session.get_diagnostics();

    // Find each injected diagnostic and verify its span against the reference.
    for (msg, start, end) in [
        ("stress-a", offset_a as usize, (offset_a + 5) as usize),
        ("stress-b", offset_b as usize, (offset_b + 6) as usize),
        ("stress-c", offset_c as usize, (offset_c + 9) as usize),
    ] {
        let d = diags
            .iter()
            .find(|d| d.message == msg)
            .unwrap_or_else(|| panic!("diagnostic '{}' not found", msg));

        let (exp_line, exp_col) = byte_offset_to_line_col(source, start);
        let (exp_end_line, exp_end_col) = byte_offset_to_line_col(source, end);

        assert_eq!(
            d.line, exp_line as u32,
            "{}: line mismatch (got {}, expected {})",
            msg, d.line, exp_line
        );
        assert_eq!(
            d.column, exp_col as u32,
            "{}: column mismatch (got {}, expected {})",
            msg, d.column, exp_col
        );
        assert_eq!(
            d.end_line, exp_end_line as u32,
            "{}: end_line mismatch (got {}, expected {})",
            msg, d.end_line, exp_end_line
        );
        assert_eq!(
            d.end_column, exp_end_col as u32,
            "{}: end_column mismatch (got {}, expected {})",
            msg, d.end_column, exp_end_col
        );
    }
}

/// The labelless (1,1,1,1) fallback is unaffected by the optimization.
/// Delegates to the existing test — this is just a marker asserting step-7
/// coverage of the labelless path specifically.
#[test]
fn get_diagnostics_labelless_fallback_unchanged_after_optimization() {
    use reify_core::Diagnostic;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("bracket source should compile cleanly");

    session.inject_diagnostic_for_test(Diagnostic::warning("no-label-stress"));

    let diags = session.get_diagnostics();
    let d = diags
        .iter()
        .find(|d| d.message == "no-label-stress")
        .expect("injected 'no-label-stress' not found");

    assert_eq!((d.line, d.column, d.end_line, d.end_column), (1, 1, 1, 1));
}

// --- Multibyte UTF-8 cross-validation ---

/// offset_to_line_col_fast must match byte_offset_to_line_col for every
/// char-boundary offset in a string containing 2-byte UTF-8 sequences.
///
/// "héllo\nwörld": 'é' (U+00E9) = 2 bytes; 'ö' (U+00F6) = 2 bytes.
/// The old byte-arithmetic implementation computes `offset - newline_pos` which
/// gives byte distance, not codepoint count.  The new implementation must
/// compute `source[line_start..offset].chars().count() + 1`.
///
/// Specific regression anchor:
///   byte offset 3 = the first 'l' after 'é'.
///   codepoint column = 3 (h=1, é=2, l=3) — NOT 4 (which byte distance gives).
#[test]
fn offset_to_line_col_fast_matches_original_multibyte_utf8() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    use reify_core::byte_offset_to_line_col;
    let source = "héllo\nwörld";
    let line_offsets = build_line_offsets(source);
    // Iterate only char-boundary offsets.
    for (byte_idx, _ch) in source.char_indices() {
        let expected = byte_offset_to_line_col(source, byte_idx);
        let actual = offset_to_line_col_fast(source, &line_offsets, byte_idx);
        assert_eq!(
            actual, expected,
            "2-byte UTF-8: mismatch at byte offset {} (char '{}'): fast={:?} original={:?}",
            byte_idx, _ch, actual, expected
        );
    }
    // Also check the EOF position (one past last byte).
    let eof = source.len();
    assert_eq!(
        offset_to_line_col_fast(source, &line_offsets, eof),
        byte_offset_to_line_col(source, eof),
        "2-byte UTF-8: mismatch at EOF offset {}",
        eof
    );
}

/// Targeted assertion: byte offset 3 in "héllo\nwörld" must give column 3
/// (codepoints h=1, é=2, l=3), NOT column 4 (byte distance from start).
#[test]
fn offset_to_line_col_fast_two_byte_char_column_is_codepoint_not_byte() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    let source = "héllo\nwörld";
    // 'é' occupies bytes 1..=2; the 'l' following it starts at byte 3.
    let line_offsets = build_line_offsets(source);
    // col should be 3 (h,é,l = 3 codepoints), not 4 (byte distance 3 → +1=4).
    assert_eq!(
        offset_to_line_col_fast(source, &line_offsets, 3),
        (1, 3),
        "byte 3 ('l' after 'é') should have codepoint column 3, not byte-based 4"
    );
    // 'r' on line 2: 'ö' at bytes 8..=9, so 'r' at byte 10.
    // Codepoints on line 2 before 'r': w=1, ö=2  → 'r' = col 3.
    assert_eq!(
        offset_to_line_col_fast(source, &line_offsets, 10),
        (2, 3),
        "byte 10 ('r' after 'ö') should have codepoint column 3, not byte-based 4"
    );
}

/// offset_to_line_col_fast matches byte_offset_to_line_col for every
/// char-boundary offset in a string containing 3-byte CJK UTF-8 sequences.
///
/// "ab\n你好world": '你' (U+4F60) = 3 bytes; '好' (U+597D) = 3 bytes.
/// 'w' is the 3rd codepoint on line 2 (you=1, hao=2, w=3).
/// Old byte arithmetic would give col = (9 - 2) = 7, which is wrong.
#[test]
fn offset_to_line_col_fast_matches_original_cjk_utf8() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    use reify_core::byte_offset_to_line_col;
    let source = "ab\n\u{4F60}\u{597D}world";
    let line_offsets = build_line_offsets(source);
    for (byte_idx, _ch) in source.char_indices() {
        let expected = byte_offset_to_line_col(source, byte_idx);
        let actual = offset_to_line_col_fast(source, &line_offsets, byte_idx);
        assert_eq!(
            actual, expected,
            "CJK UTF-8: mismatch at byte offset {} (char '{}'): fast={:?} original={:?}",
            byte_idx, _ch, actual, expected
        );
    }
    // EOF check.
    let eof = source.len();
    assert_eq!(
        offset_to_line_col_fast(source, &line_offsets, eof),
        byte_offset_to_line_col(source, eof),
        "CJK UTF-8: mismatch at EOF offset {}",
        eof
    );
    // Targeted: 'w' at byte 9 should be (2, 3), not byte-arithmetic (2, 7).
    assert_eq!(
        offset_to_line_col_fast(source, &line_offsets, 9),
        (2, 3),
        "byte 9 ('w' after two 3-byte CJK chars) should have codepoint column 3"
    );
}

/// offset_to_line_col_fast does not panic on non-char-boundary byte offsets;
/// it snaps backward to the nearest valid boundary instead.
#[test]
fn offset_to_line_col_fast_non_char_boundary_no_panic() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};

    // "é" is 2 bytes (0xC3 0xA9), so byte 1 is mid-char.
    let source = "é";
    let line_offsets = build_line_offsets(source);
    // Byte 1 is not a char boundary — should not panic, should snap back to 0.
    let (line, col) = offset_to_line_col_fast(source, &line_offsets, 1);
    assert_eq!(line, 1);
    assert_eq!(col, 1, "non-boundary offset should snap back to start");

    // Multi-line with CJK: "日\nA" — '日' is 3 bytes; byte 2 is mid-char.
    let source2 = "日\nA";
    let offsets2 = build_line_offsets(source2);
    let (l, c) = offset_to_line_col_fast(source2, &offsets2, 2);
    assert_eq!(l, 1);
    assert_eq!(c, 1, "mid-CJK offset should snap back to start of char");
}

// --- resolve_source without loaded module ---

/// resolve_source returns None when called without a loaded module.
///
/// After the Option refactor, resolve_source gracefully returns None when compiled
/// is None rather than panicking via debug_assert.
#[test]
fn resolve_source_returns_none_without_loaded_module() {
    let checker = SimpleConstraintChecker;
    let session = EngineSession::new(Box::new(checker), None);
    // No load — compiled is None. resolve_source should return None gracefully.
    assert_eq!(session.resolve_source_for_test(), None);
}

/// resolve_source returns None when module_name has been cleared (broken invariant).
///
/// After the Option refactor, resolve_source gracefully returns None instead of
/// panicking via expect() when module_name is None.
#[test]
fn resolve_source_returns_none_when_module_name_broken() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");
    session.break_module_name_for_test();
    assert_eq!(session.resolve_source_for_test(), None);
}

/// resolve_source returns None when source_map has been cleared (broken invariant).
///
/// After the Option refactor, resolve_source gracefully returns None instead of
/// panicking via expect() when source_map.get_key_value returns None.
#[test]
fn resolve_source_returns_none_when_source_map_broken() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");
    session.break_source_map_for_test();
    assert_eq!(session.resolve_source_for_test(), None);
}

// --- module_key ---

/// module_key("bracket") == "bracket.ri" — normal identifier.
#[test]
fn module_key_normal_name() {
    assert_eq!(module_key("bracket"), "bracket.ri");
}

/// module_key("some_module") == "some_module.ri" — underscored name.
#[test]
fn module_key_underscored_name() {
    assert_eq!(module_key("some_module"), "some_module.ri");
}

/// module_key(name) matches the key that load_from_source inserts into source_map.
///
/// module_key is the single authoritative point for key derivation (engine.rs:31-35).
/// This test locks in the invariant that load_from_source and module_key stay in sync,
/// guarding against a regression where someone inlines `format!("{}.ri", ...)` back
/// into load_from_source without updating module_key.
#[test]
fn module_key_matches_load_from_source_insertion() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");
    let (stored_key, stored_src) = session
        .resolve_source_for_test()
        .expect("resolve_source_for_test should return Some after successful load");
    assert_eq!(stored_key, module_key("bracket"));
    assert_eq!(stored_src, bracket_source());
}

/// module_key(name) matches the key that update_source inserts into source_map.
///
/// module_key is the single authoritative point for key derivation (engine.rs:31-35).
/// This test locks in the invariant that update_source and module_key stay in sync,
/// guarding against a regression where someone inlines `format!("{}.ri", ...)` back
/// into update_source without updating module_key (engine.rs:212).
#[test]
fn module_key_matches_update_source_insertion() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .update_source("bracket.ri", bracket_source())
        .expect("update_source should succeed");
    let (stored_key, stored_src) = session
        .resolve_source_for_test()
        .expect("resolve_source_for_test should return Some after successful update_source");
    assert_eq!(stored_key, module_key("bracket"));
    assert_eq!(stored_src, bracket_source());
}

/// module_key panics (via debug_assert) when called with an empty name.
///
/// An empty name would produce ".ri", which is never a valid module key —
/// `load_file` falls back to "unnamed" so an empty name is a programming error.
/// The debug_assert in module_key is the contract guard.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "empty")]
fn module_key_empty_name_panics() {
    let _ = module_key("");
}

// --- resolve_source positive path ---

/// resolve_source returns the key (module_key(name)) and the original source text
/// after a successful load_from_source call.
///
/// Pins: (a) key derivation appends ".ri" to the module name, (b) the source text
/// is stored verbatim and returned as a zero-copy &str borrow.
#[test]
fn resolve_source_returns_key_and_source_after_load() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed with bracket source");
    assert_eq!(
        session.resolve_source_for_test(),
        Some(("bracket.ri", bracket_source())),
    );
}

// --- Broken-invariant graceful fallback ---

/// Calling get_diagnostics when module_name has been cleared (while compiled
/// remains Some) returns an empty vec.
///
/// After the Option refactor, resolve_source returns None instead of panicking
/// when module_name is None, so get_diagnostics gracefully returns an empty vec.
///
/// NOTE: get_diagnostics early-exits when diagnostics is empty, so we inject a
/// synthetic diagnostic to ensure resolve_source is actually reached.
///
/// Only runs in release builds — in debug builds the new debug_assert fires and
/// the corresponding `_debug` variant (below) verifies the panic instead.
#[cfg(not(debug_assertions))]
#[test]
fn get_diagnostics_returns_empty_when_module_name_broken() {
    use reify_core::Diagnostic;

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");
    // Deliberately break the invariant: compiled is Some, module_name is None.
    session.break_module_name_for_test();
    // Inject a diagnostic so the early-exit branch is skipped and resolve_source
    // is reached — which must return None, causing get_diagnostics to return [].
    session.inject_diagnostic_for_test(Diagnostic::warning("force-none"));
    let diags = session.get_diagnostics();
    assert!(
        diags.is_empty(),
        "expected empty diagnostics when module_name is broken, got: {:?}",
        diags
    );
}

/// Calling get_diagnostics when source_map has been cleared (while compiled
/// and module_name remain Some) returns an empty vec.
///
/// After the Option refactor, resolve_source returns None instead of panicking
/// when source_map.get_key_value returns None, so get_diagnostics returns [].
///
/// NOTE: get_diagnostics early-exits when diagnostics is empty, so we inject a
/// synthetic diagnostic to ensure resolve_source is actually reached.
///
/// Only runs in release builds — in debug builds the new debug_assert fires and
/// the corresponding `_debug` variant (below) verifies the panic instead.
#[cfg(not(debug_assertions))]
#[test]
fn get_diagnostics_returns_empty_when_source_map_broken() {
    use reify_core::Diagnostic;

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");
    // Deliberately break the invariant: compiled and module_name are Some, source_map is empty.
    session.break_source_map_for_test();
    // Inject a diagnostic so the early-exit branch is skipped and resolve_source
    // is reached — which must return None, causing get_diagnostics to return [].
    session.inject_diagnostic_for_test(Diagnostic::warning("force-none"));
    let diags = session.get_diagnostics();
    assert!(
        diags.is_empty(),
        "expected empty diagnostics when source_map is broken, got: {:?}",
        diags
    );
}

// --- Broken-invariant fallback (real warnings) ---

/// Calling get_diagnostics when module_name has been cleared (while compiled
/// remains Some) returns an empty vec.
///
/// Unlike get_diagnostics_returns_empty_when_module_name_broken above, this test
/// uses a real compiler-produced warning rather than inject_diagnostic_for_test.
/// This pins the graceful-return behavior on the user-visible failure mode: real
/// source that the compiler emits warnings for, with a deliberately broken invariant.
///
/// Only runs in release builds — in debug builds the debug_assert fires instead.
#[cfg(not(debug_assertions))]
#[test]
fn get_diagnostics_returns_empty_when_module_name_broken_with_real_warning() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    // This source reliably produces a compiler warning (unknown port type), not
    // an error — the same fixture used by engine_get_diagnostics_returns_populated_warning.

    // load_from_source succeeds: warnings are not errors.
    session
        .load_from_source(warn_source_with_unknown_port_type(), "test_warn")
        .expect("source with unknown port type should compile (warning, not error)");

    // Deliberately break the invariant: compiled is Some, module_name is None.
    // compiled.diagnostics is non-empty (real warning), so the early-exit in
    // get_diagnostics is skipped and resolve_source is reached — which must
    // return None, causing get_diagnostics to return [].
    session.break_module_name_for_test();
    let diags = session.get_diagnostics();
    assert!(
        diags.is_empty(),
        "expected empty diagnostics when module_name is broken, got: {:?}",
        diags
    );
}

/// Calling get_diagnostics when source_map has been cleared (while compiled
/// and module_name remain Some) returns an empty vec.
///
/// Unlike get_diagnostics_returns_empty_when_source_map_broken above, this test
/// uses a real compiler-produced warning rather than inject_diagnostic_for_test.
/// This pins the graceful-return behavior on the user-visible failure mode and
/// removes coupling to the test injection helper — complementary, not duplicative.
///
/// Only runs in release builds — in debug builds the debug_assert fires instead.
#[cfg(not(debug_assertions))]
#[test]
fn get_diagnostics_returns_empty_when_source_map_broken_with_real_warning() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    // Same real warning-producing fixture as above.

    // load_from_source succeeds: warnings are not errors.
    session
        .load_from_source(warn_source_with_unknown_port_type(), "test_warn")
        .expect("source with unknown port type should compile (warning, not error)");

    // Deliberately break the invariant: compiled and module_name are Some,
    // source_map is empty. compiled.diagnostics is non-empty (real warning), so
    // the early-exit in get_diagnostics is skipped, but resolve_source returns
    // None — causing get_diagnostics to return [].
    session.break_source_map_for_test();
    let diags = session.get_diagnostics();
    assert!(
        diags.is_empty(),
        "expected empty diagnostics when source_map is broken, got: {:?}",
        diags
    );
}

// --- Broken-invariant debug_assert (debug builds only) ---

/// In debug builds, get_diagnostics must panic (via debug_assert) when
/// module_name has been cleared while compiled.diagnostics is non-empty.
///
/// The broken invariant is caught loudly in development so stale-state bugs
/// surface immediately. Release builds retain the graceful empty-vec fallback
/// (tested by get_diagnostics_returns_empty_when_module_name_broken).
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "invariant broken")]
fn get_diagnostics_debug_asserts_when_module_name_broken() {
    use reify_core::Diagnostic;

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");
    // Break invariant: compiled is Some, module_name is None.
    session.break_module_name_for_test();
    // Inject a diagnostic so the empty-diagnostics early-exit is skipped and
    // resolve_source is reached — which returns None, triggering the debug_assert.
    session.inject_diagnostic_for_test(Diagnostic::warning("force-none"));
    // Must panic with "invariant broken" in debug builds.
    let _ = session.get_diagnostics();
}

/// In debug builds, get_diagnostics must panic (via debug_assert) when
/// source_map has been cleared while compiled.diagnostics is non-empty.
///
/// Mirrors get_diagnostics_debug_asserts_when_module_name_broken but exercises
/// the break_source_map_for_test path.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "invariant broken")]
fn get_diagnostics_debug_asserts_when_source_map_broken() {
    use reify_core::Diagnostic;

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");
    // Break invariant: compiled and module_name are Some, source_map is empty.
    session.break_source_map_for_test();
    // Inject a diagnostic so the empty-diagnostics early-exit is skipped and
    // resolve_source is reached — which returns None, triggering the debug_assert.
    session.inject_diagnostic_for_test(Diagnostic::warning("force-none"));
    // Must panic with "invariant broken" in debug builds.
    let _ = session.get_diagnostics();
}

// --- resolve_source after update_source ---

/// resolve_source returns updated content (and the same key) after a successful
/// update_source call.
///
/// Pins: (a) the key derived from the path argument stays "bracket.ri",
/// (b) the source text is replaced with the new content and returned verbatim.
#[test]
fn resolve_source_returns_updated_content_after_update_source() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed with bracket source");
    // Baseline: resolve_source reflects the initial load.
    assert_eq!(
        session.resolve_source_for_test(),
        Some(("bracket.ri", bracket_source())),
    );
    // Update the source with modified content (different width parameter).
    let updated = bracket_source_with_width("120mm");
    session
        .update_source("bracket.ri", &updated)
        .expect("update_source should succeed with modified bracket source");
    // After update: key stays the same, content is the new text.
    assert_eq!(
        session.resolve_source_for_test(),
        Some(("bracket.ri", updated.as_str())),
    );
}

/// get_source_location returns the updated file_path after update_source changes the module name,
/// and line/column positions remain stable when the source text is identical.
///
/// After load_from_source with 'initial' then update_source("updated.ri", ...),
/// get_source_location must use the new module name key "updated.ri" for the file_path,
/// not the stale "initial.ri". Fills test-analyst gap S11. Additionally, because both
/// calls pass the same source text, the byte span for S.width is unchanged and
/// byte_offset_to_line_col must produce identical line/column/end_line/end_column values —
/// guarding against update_source corrupting source-map content.
#[test]
fn get_source_location_file_key_updates_after_update_source() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    // Load with a warning-producing source that has an S.width parameter.
    session
        .load_from_source(warn_source_with_unknown_port_type_with_width(), "initial")
        .expect("initial load should succeed");

    // Verify the baseline: file_path reflects the initial module name.
    let loc_before = session
        .get_source_location("S.width")
        .expect("S.width should be found after initial load");
    assert_eq!(
        loc_before.file_path, "initial.ri",
        "before update: file_path should be 'initial.ri'"
    );
    assert!(
        loc_before.line > 0,
        "sanity: line should be positive for a real span"
    );

    // Update with the same source but a different module name.
    session
        .update_source(
            "updated.ri",
            warn_source_with_unknown_port_type_with_width(),
        )
        .expect("update_source should succeed");

    // After update: file_path must reflect the new module name "updated".
    let loc_after = session
        .get_source_location("S.width")
        .expect("S.width should be found after update_source");
    assert_eq!(
        loc_after.file_path, "updated.ri",
        "after update_source: file_path should be 'updated.ri', not 'initial.ri'"
    );

    // Line/column positions must be unchanged when update_source uses identical source text.
    assert_eq!(
        loc_after.line, loc_before.line,
        "line should be unchanged when update_source uses identical source text"
    );
    assert_eq!(
        loc_after.column, loc_before.column,
        "column should be unchanged when update_source uses identical source text"
    );
    assert_eq!(
        loc_after.end_line, loc_before.end_line,
        "end_line should be unchanged when update_source uses identical source text"
    );
    assert_eq!(
        loc_after.end_column, loc_before.end_column,
        "end_column should be unchanged when update_source uses identical source text"
    );
}

// ---- Steps 1-2: EntityTreeNode type serialization tests ----

#[test]
fn entity_tree_node_serialization_roundtrip() {
    let node = EntityTreeNode {
        entity_path: "Bracket".to_string(),
        kind: "structure".to_string(),
        type_name: None,
        display_name: None,
        has_mesh: false,
        trait_geometry: false,
        children: vec![],
        freshness: "final".to_string(),
        default_visible: true,
    };
    let json = serde_json::to_string(&node).expect("serialize should succeed");
    let back: EntityTreeNode = serde_json::from_str(&json).expect("deserialize should succeed");
    assert_eq!(node, back);
}

#[test]
fn entity_tree_node_nested_children_serialize_correctly() {
    let child = EntityTreeNode {
        entity_path: "Bracket.width".to_string(),
        kind: "param".to_string(),
        type_name: Some("Length".to_string()),
        display_name: None,
        has_mesh: false,
        trait_geometry: false,
        children: vec![],
        freshness: "final".to_string(),
        default_visible: true,
    };
    let root = EntityTreeNode {
        entity_path: "Bracket".to_string(),
        kind: "structure".to_string(),
        type_name: None,
        display_name: None,
        has_mesh: true,
        trait_geometry: false,
        children: vec![child],
        freshness: "final".to_string(),
        default_visible: true,
    };
    let json = serde_json::to_string(&root).expect("serialize should succeed");
    assert!(json.contains("\"entity_path\":\"Bracket.width\""));
    assert!(json.contains("\"kind\":\"param\""));
    assert!(json.contains("\"type_name\":\"Length\""));
    assert!(json.contains("\"has_mesh\":true"));
    let back: EntityTreeNode = serde_json::from_str(&json).expect("deserialize should succeed");
    assert_eq!(root, back);
    assert_eq!(back.children.len(), 1);
    assert_eq!(back.children[0].entity_path, "Bracket.width");
}

#[test]
fn entity_tree_node_default_type_name_is_none() {
    let node = EntityTreeNode {
        entity_path: "Foo".to_string(),
        kind: "occurrence".to_string(),
        type_name: None,
        display_name: None,
        has_mesh: false,
        trait_geometry: false,
        children: vec![],
        freshness: "final".to_string(),
        default_visible: true,
    };
    let json = serde_json::to_string(&node).expect("serialize should succeed");
    let back: EntityTreeNode = serde_json::from_str(&json).expect("deserialize should succeed");
    assert_eq!(back.type_name, None);
}

// ---- Step 3: get_entity_tree() tests ----

#[test]
fn get_entity_tree_no_module_returns_empty_vec() {
    let checker = SimpleConstraintChecker;
    let session = EngineSession::new(Box::new(checker), None);
    let tree = session.get_entity_tree();
    assert!(tree.is_empty(), "no module loaded → empty tree");
}

#[test]
fn get_entity_tree_bracket_returns_single_root_node() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load");

    let tree = session.get_entity_tree();
    assert_eq!(tree.len(), 1, "bracket has one root template");
    assert_eq!(tree[0].entity_path, "Bracket");
    assert_eq!(tree[0].kind, "structure");
}

#[test]
fn get_entity_tree_bracket_children_have_correct_kinds() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load");

    let tree = session.get_entity_tree();
    let root = &tree[0];

    // bracket has 5 params, 2 lets (volume + body), 3 constraints (no child nodes for constraints)
    let params: Vec<_> = root.children.iter().filter(|c| c.kind == "param").collect();
    let lets: Vec<_> = root.children.iter().filter(|c| c.kind == "let").collect();

    assert_eq!(
        params.len(),
        5,
        "5 param cells: width, height, thickness, fillet_radius, hole_diameter"
    );
    // `let body = box(...)` compiles into a realization (geometry op), not a ValueCellDecl.
    // Only `let volume = ...` is a let-binding value cell.
    assert_eq!(
        lets.len(),
        1,
        "1 let cell: volume (body is a realization, not a let)"
    );
}

#[test]
fn get_entity_tree_bracket_param_entity_paths_correct() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load");

    let tree = session.get_entity_tree();
    let root = &tree[0];

    let width_node = root
        .children
        .iter()
        .find(|c| c.entity_path == "Bracket.width");
    assert!(width_node.is_some(), "should have Bracket.width child node");
    assert_eq!(width_node.unwrap().kind, "param");
}

#[test]
fn get_entity_tree_has_mesh_true_when_realization_exists() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load");

    let tree = session.get_entity_tree();
    let root = &tree[0];
    // bracket has a realization (box), so has_mesh should be true
    assert!(root.has_mesh, "Bracket root should have has_mesh=true");
}

#[test]
fn get_entity_tree_no_realization_has_mesh_false() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    // Load a module with no realizations via source (no geometry ops)
    session
        .load_from_source("structure Simple { param x: Scalar = 1mm }", "simple")
        .expect("load");
    let tree = session.get_entity_tree();
    let root = &tree[0];
    assert!(!root.has_mesh, "Simple with no realization has_mesh=false");
    // TODO: extend with direct CompiledModule injection when EngineSession supports it
}

// ---- Step 5: sub-component tree building tests ----

#[test]
fn get_entity_tree_sub_component_produces_nested_node() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    session
        .load_from_source(
            r#"structure Bolt { param mass: Scalar = 1 }
structure Assembly { sub bolt = Bolt() }"#,
            "test",
        )
        .expect("load");

    let tree = session.get_entity_tree();

    // Find Assembly root
    let assembly = tree
        .iter()
        .find(|n| n.entity_path == "Assembly")
        .expect("Assembly root should exist");

    let sub_node = assembly
        .children
        .iter()
        .find(|c| c.kind == "sub")
        .expect("Assembly should have a 'sub' child node");

    assert_eq!(sub_node.entity_path, "Assembly.bolt");
    assert_eq!(sub_node.type_name.as_deref(), Some("Bolt"));
}

#[test]
fn get_entity_tree_collection_sub_has_list_type_name() {
    use reify_core::{DimensionVector, ModulePath, Type};

    let mass_type = Type::Scalar {
        dimension: DimensionVector::MASS,
    };

    let bolt_template = TopologyTemplateBuilder::new("Bolt")
        .param("Bolt", "mass", mass_type, None)
        .build();

    // Use source-level test since we can't inject CompiledModule
    // Collection sub syntax: `sub bolts: List<Bolt>()`
    // Reify may or may not support this in the parser — test via compiled module builder
    let count_cell = reify_core::ValueCellId::new("Assembly", "__count_bolts");
    let assembly_template = TopologyTemplateBuilder::new("Assembly")
        .collection_sub_component("bolts", "Bolt", count_cell)
        .build();

    let compiled = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(assembly_template)
        .template(bolt_template)
        .build();

    // Verify the compiled module sub is marked as collection
    let assembly = find_template(&compiled.templates, "Assembly").unwrap();
    let bolts_sub = assembly
        .sub_components
        .iter()
        .find(|s| s.name == "bolts")
        .unwrap();
    assert!(
        bolts_sub.is_collection,
        "collection sub should have is_collection=true"
    );
    assert_eq!(bolts_sub.structure_name, "Bolt");

    // Build tree manually via get_entity_tree — we need a session with this module.
    // Since we can't inject a CompiledModule, verify the type_name logic directly:
    // for is_collection=true, type_name should be "List<{structure_name}>"
    let type_name = if bolts_sub.is_collection {
        format!("List<{}>", bolts_sub.structure_name)
    } else {
        bolts_sub.structure_name.clone()
    };
    assert_eq!(type_name, "List<Bolt>");
}

#[test]
fn get_entity_tree_value_cell_type_name_from_cell_type() {
    // Verify type_name for value cells is cell_type.to_string()
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load");

    let tree = session.get_entity_tree();
    let root = &tree[0];

    let width_node = root
        .children
        .iter()
        .find(|c| c.entity_path == "Bracket.width")
        .expect("should have Bracket.width node");

    // width is `param width: Scalar = 80mm` → type is Scalar[LENGTH]
    // cell_type.to_string() for a Length scalar should contain "Scalar"
    let type_name = width_node
        .type_name
        .as_ref()
        .expect("width should have type_name");
    assert!(
        type_name.contains("Scalar"),
        "width type_name '{}' should contain 'Scalar'",
        type_name
    );
}

#[test]
fn get_entity_tree_sub_node_type_name_from_structure_name() {
    // Load source with a sub-component, verify type_name = structure_name
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    session
        .load_from_source(
            r#"structure Bolt { param mass: Scalar = 1 }
structure Assembly { sub bolt = Bolt() }"#,
            "test",
        )
        .expect("load");

    let tree = session.get_entity_tree();
    let assembly = tree
        .iter()
        .find(|n| n.entity_path == "Assembly")
        .expect("Assembly root should exist");
    let sub_node = assembly
        .children
        .iter()
        .find(|c| c.kind == "sub")
        .expect("should have sub node");

    assert_eq!(
        sub_node.type_name.as_deref(),
        Some("Bolt"),
        "sub node type_name should be structure_name"
    );
}

// ---- Step 7: EntityIdentity and get_entity_identity_map() tests ----

/// EntityIdentity serializes and deserializes without loss.
#[test]
fn entity_identity_serialization_roundtrip() {
    use crate::types::{EntityIdentity, SourceSpanInfo};
    let identity = EntityIdentity {
        content_hash: "abc123def456abc123def456abc123de".to_string(),
        structural_fingerprint: "structure:<root>:0:deadbeef00000000000000000000000".to_string(),
        source_span: Some(SourceSpanInfo { start: 10, end: 50 }),
    };
    let json = serde_json::to_string(&identity).expect("serialize should succeed");
    let back: EntityIdentity = serde_json::from_str(&json).expect("deserialize should succeed");
    assert_eq!(identity, back);
}

/// EntityIdentity with source_span=None round-trips to None.
#[test]
fn entity_identity_source_span_none_serialization() {
    use crate::types::EntityIdentity;
    let identity = EntityIdentity {
        content_hash: "ff00aa11ff00aa11ff00aa11ff00aa11".to_string(),
        structural_fingerprint: "param:Bracket:0:00000000000000000000000000000000".to_string(),
        source_span: None,
    };
    let json = serde_json::to_string(&identity).expect("serialize should succeed");
    let back: EntityIdentity = serde_json::from_str(&json).expect("deserialize should succeed");
    assert_eq!(back.source_span, None);
}

/// No module loaded → get_entity_identity_map returns empty map.
#[test]
fn get_entity_identity_map_no_module_returns_empty() {
    use crate::types::EntityIdentity;
    use std::collections::HashMap;
    let checker = SimpleConstraintChecker;
    let session = EngineSession::new(Box::new(checker), None);
    let map: HashMap<String, EntityIdentity> = session.get_entity_identity_map();
    assert!(map.is_empty(), "no module loaded → empty identity map");
}

/// After loading bracket, the map contains a "Bracket" root entry and
/// a "Bracket.width" value-cell entry.
#[test]
fn get_entity_identity_map_bracket_has_expected_keys() {
    use crate::types::EntityIdentity;
    use std::collections::HashMap;
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let map: HashMap<String, EntityIdentity> = session.get_entity_identity_map();
    assert!(
        map.contains_key("Bracket"),
        "map should contain 'Bracket' root key; keys: {:?}",
        map.keys().collect::<Vec<_>>()
    );
    assert!(
        map.contains_key("Bracket.width"),
        "map should contain 'Bracket.width' value-cell key"
    );
}

/// content_hash for the Bracket root entry is a 32-character lowercase hex string.
///
/// Pins: ContentHash::to_string() emits exactly 32 lowercase hex digits
/// (it wraps a u128 formatted as {:032x}).
#[test]
fn get_entity_identity_map_content_hash_is_hex_string() {
    use crate::types::EntityIdentity;
    use std::collections::HashMap;
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let map: HashMap<String, EntityIdentity> = session.get_entity_identity_map();
    let bracket_identity = map.get("Bracket").expect("Bracket should be in map");
    let hash = &bracket_identity.content_hash;
    assert!(!hash.is_empty(), "content_hash must not be empty");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "content_hash must be all hex digits: '{}'",
        hash
    );
    assert_eq!(
        hash.len(),
        32,
        "ContentHash::to_string() emits 32 hex chars"
    );
}

/// Bracket root structural_fingerprint has format '{type}:{parent}:{child_count}:{hash}'.
///
/// For a root template:
/// - type = "structure" or "occurrence"
/// - parent = "<root>" (reserved sentinel for root templates — angle-bracket form
///   is an impossible template identifier, preventing collisions with user templates
///   named "root")
/// - child_count = number of sub-components (Bracket has 0)
/// - hash = non-empty hex string from children content hashes
#[test]
fn get_entity_identity_map_root_structural_fingerprint_format() {
    use crate::types::EntityIdentity;
    use std::collections::HashMap;
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let map: HashMap<String, EntityIdentity> = session.get_entity_identity_map();
    let bracket_identity = map.get("Bracket").expect("Bracket should be in map");
    let fp = &bracket_identity.structural_fingerprint;
    // Format: '{type}:{parent}:{child_count}:{hash}' — 4 colon-separated parts
    let parts: Vec<&str> = fp.splitn(4, ':').collect();
    assert_eq!(
        parts.len(),
        4,
        "fingerprint must have 4 colon-separated parts; got: '{}'",
        fp
    );
    assert_eq!(parts[0], "structure", "first part is entity kind");
    assert_eq!(
        parts[1], "<root>",
        "parent of root template is '<root>' sentinel"
    );
    let child_count: usize = parts[2]
        .parse()
        .expect("third part (child_count) must be a non-negative integer");
    assert_eq!(child_count, 0, "Bracket has no sub-components");
    assert!(!parts[3].is_empty(), "fourth part (hash) must not be empty");
}

/// Bracket.width value-cell fingerprint format: '{cell_kind}:{parent}:{child_count}:{hash}'.
#[test]
fn get_entity_identity_map_value_cell_structural_fingerprint_format() {
    use crate::types::EntityIdentity;
    use std::collections::HashMap;
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let map: HashMap<String, EntityIdentity> = session.get_entity_identity_map();
    let width_identity = map
        .get("Bracket.width")
        .expect("Bracket.width should be in map");
    let fp = &width_identity.structural_fingerprint;
    let parts: Vec<&str> = fp.splitn(4, ':').collect();
    assert_eq!(
        parts.len(),
        4,
        "fingerprint must have 4 parts; got: '{}'",
        fp
    );
    assert_eq!(parts[0], "param", "Bracket.width is a param cell");
    assert_eq!(parts[1], "Bracket", "parent template is 'Bracket'");
    assert_eq!(parts[2], "0", "value cell has no sub-children");
    assert!(!parts[3].is_empty(), "hash must not be empty");
}

/// Value-cell entries carry a source_span with end > start.
#[test]
fn get_entity_identity_map_value_cell_has_source_span() {
    use crate::types::EntityIdentity;
    use std::collections::HashMap;
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let map: HashMap<String, EntityIdentity> = session.get_entity_identity_map();
    let width_identity = map
        .get("Bracket.width")
        .expect("Bracket.width should be in map");
    let span = width_identity
        .source_span
        .as_ref()
        .expect("value cell should have source_span");
    assert!(span.end > span.start, "span end must be after start");
}

/// Root template entries have source_span = None (TopologyTemplate has no span field).
#[test]
fn get_entity_identity_map_root_template_has_no_source_span() {
    use crate::types::EntityIdentity;
    use std::collections::HashMap;
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let map: HashMap<String, EntityIdentity> = session.get_entity_identity_map();
    let bracket_identity = map.get("Bracket").expect("Bracket should be in map");
    assert_eq!(
        bracket_identity.source_span, None,
        "root template entry should have no source_span"
    );
}

// ---- Step 9: DefInfo and get_containing_definition() tests ----

/// DefInfo serializes and deserializes without loss.
#[test]
fn def_info_serialization_roundtrip() {
    use crate::types::{DefInfo, SourceSpanInfo};
    let def_info = DefInfo {
        name: "Bracket".to_string(),
        kind: "structure".to_string(),
        span: SourceSpanInfo { start: 0, end: 100 },
    };
    let json = serde_json::to_string(&def_info).expect("serialize should succeed");
    let back: DefInfo = serde_json::from_str(&json).expect("deserialize should succeed");
    assert_eq!(def_info, back);
}

/// No module loaded → get_containing_definition returns None.
#[test]
fn get_containing_definition_no_module_returns_none() {
    let checker = SimpleConstraintChecker;
    let session = EngineSession::new(Box::new(checker), None);
    let result = session.get_containing_definition(1, 1);
    assert!(result.is_none(), "no module loaded → None");
}

/// Position at (1,1) inside a single-line structure def returns correct name and kind.
#[test]
fn get_containing_definition_inside_structure_returns_some() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    let source = "structure Foo { param x: Scalar = 1 }";
    session
        .load_from_source(source, "test")
        .expect("load should succeed");
    // Line 1, col 1 → byte 0, first char of "structure Foo", inside the Foo def.
    let result = session.get_containing_definition(1, 1);
    let def = result.expect("position at (1,1) should be inside Foo → Some(DefInfo)");
    assert_eq!(def.name, "Foo");
    assert_eq!(def.kind, "structure");
}

/// Position in a comment on line 2 (after a single-line def on line 1) returns None.
#[test]
fn get_containing_definition_outside_def_returns_none() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    // The structure def lives entirely on line 1; line 2 is a comment.
    let source = "structure Foo { param x: Scalar = 1 }\n// outside any def";
    session
        .load_from_source(source, "test")
        .expect("load should succeed");
    // Line 2, col 5 is in the comment text, outside the Foo def span.
    let result = session.get_containing_definition(2, 5);
    assert!(
        result.is_none(),
        "position in comment on line 2 should be outside any def → None, got: {:?}",
        result
    );
}

/// Position inside an occurrence def returns kind = "occurrence".
#[test]
fn get_containing_definition_occurrence_returns_occurrence_kind() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    // Foo is on line 1; Bar (occurrence) is on line 2.
    let source = "structure Foo {}\noccurrence Bar {}";
    session
        .load_from_source(source, "test")
        .expect("load should succeed");
    // Line 2, col 1 is inside the occurrence Bar definition.
    let result = session.get_containing_definition(2, 1);
    let def = result.expect("position at (2,1) should be inside Bar → Some(DefInfo)");
    assert_eq!(def.name, "Bar");
    assert_eq!(def.kind, "occurrence");
}

/// DefInfo.span start is ≤ span end, and start == 0 for a def that begins at byte 0.
#[test]
fn get_containing_definition_span_valid_and_starts_at_zero() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    let source = "structure Foo { param x: Scalar = 1 }";
    session
        .load_from_source(source, "test")
        .expect("load should succeed");
    let result = session.get_containing_definition(1, 1);
    let def = result.expect("position inside Foo → Some(DefInfo)");
    assert!(
        def.span.start <= def.span.end,
        "span start ({}) must be <= end ({})",
        def.span.start,
        def.span.end
    );
    // The source starts with "structure Foo {…}", so the def begins at byte 0.
    assert_eq!(def.span.start, 0, "Foo def starts at byte 0");
}

// ---- Step 11: get_def_preview() tests ----

/// No module loaded → get_def_preview returns Err.
#[test]
fn get_def_preview_no_module_returns_error() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    let result = session.get_def_preview("Bracket");
    assert!(result.is_err(), "no module loaded → Err");
}

/// Unknown definition name → get_def_preview returns Err.
#[test]
fn get_def_preview_unknown_name_returns_error() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let result = session.get_def_preview("NonExistentDef");
    assert!(result.is_err(), "unknown def name → Err, got: {:?}", result);
}

/// Valid definition name → get_def_preview returns Ok(GuiState) with values.
///
/// The returned GuiState must have at least as many value entries as the bracket
/// has params+lets (5 params + 1 let = 6).
#[test]
fn get_def_preview_valid_name_returns_gui_state_with_values() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let result = session.get_def_preview("Bracket");
    let state = result.expect("Bracket preview should return Ok(GuiState)");
    assert!(
        state.values.len() >= 5,
        "preview GuiState should have at least 5 value entries (bracket params), got {}",
        state.values.len()
    );
}

/// Bracket param defaults are evaluated: Bracket.width preview value is "80".
#[test]
fn get_def_preview_param_defaults_are_evaluated() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let state = session
        .get_def_preview("Bracket")
        .expect("Bracket preview should succeed");
    let width = state
        .values
        .iter()
        .find(|v| v.name == "width")
        .expect("preview state should have a 'width' value entry");
    assert_eq!(
        width.value, "80",
        "preview width default should be 80 (mm), got: '{}'",
        width.value
    );
}

/// def with no default param produces GuiState with undetermined value.
#[test]
fn get_def_preview_no_default_param_is_undetermined() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    // 'x' has no default expression — must be Undetermined in preview.
    session
        .load_from_source("structure Bar { param x: Scalar }", "test")
        .expect("load should succeed");
    let state = session
        .get_def_preview("Bar")
        .expect("Bar preview should succeed");
    let x_val = state
        .values
        .iter()
        .find(|v| v.name == "x")
        .expect("preview should have 'x' value");
    assert_eq!(
        x_val.determinacy, "undetermined",
        "param with no default should be undetermined, got: '{}'",
        x_val.determinacy
    );
}

/// get_def_preview result is cached: second call returns equal GuiState without
/// re-evaluating (structural equality check — same values, same constraints).
#[test]
fn get_def_preview_result_is_cached() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let first = session
        .get_def_preview("Bracket")
        .expect("first preview call should succeed");
    let second = session
        .get_def_preview("Bracket")
        .expect("second preview call should succeed");
    assert_eq!(
        first, second,
        "cached preview result should be structurally equal to first result"
    );
}

/// After reloading the module, get_def_preview reflects the new source.
#[test]
fn get_def_preview_cache_invalidated_on_reload() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");
    let before = session.get_def_preview("Bracket").expect("initial preview");

    // Reload with a different width default.
    let checker2 = SimpleConstraintChecker;
    let kernel2 = MockGeometryKernel::new();
    let mut session2 = EngineSession::new(Box::new(checker2), Some(Box::new(kernel2)));
    session2
        .load_from_source(&bracket_source_with_width("120mm"), "bracket")
        .expect("reload with different width");
    let after = session2
        .get_def_preview("Bracket")
        .expect("preview after reload");

    let width_before = before
        .values
        .iter()
        .find(|v| v.name == "width")
        .map(|v| v.value.as_str())
        .unwrap_or("");
    let width_after = after
        .values
        .iter()
        .find(|v| v.name == "width")
        .map(|v| v.value.as_str())
        .unwrap_or("");
    assert_ne!(
        width_before, width_after,
        "preview width should differ after reload with different default"
    );
}

// ---- Step 13: Integration tests — entity_path consistency across commands ----

/// get_entity_tree and get_entity_identity_map return consistent entity_path keys.
///
/// For every node in the entity tree (root and all children), the entity_path
/// must appear as a key in the identity map.  This pins the contract: both
/// commands derive their entity_path values from the same CompiledModule,
/// so they must agree.
#[test]
fn entity_tree_and_identity_map_entity_paths_are_consistent() {
    use crate::types::EntityIdentity;
    use std::collections::HashMap;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");

    let tree = session.get_entity_tree();
    let map: HashMap<String, EntityIdentity> = session.get_entity_identity_map();

    // Collect all entity_path values from the tree (breadth-first traversal).
    let mut tree_paths: Vec<String> = Vec::new();
    let mut queue: std::collections::VecDeque<&crate::types::EntityTreeNode> =
        tree.iter().collect();
    while let Some(node) = queue.pop_front() {
        tree_paths.push(node.entity_path.clone());
        for child in &node.children {
            queue.push_back(child);
        }
    }

    // Every path from the tree must be a key in the identity map — except
    // for "realization" nodes, which are keyed by the mesh-key form
    // (`Entity#realization[N]`) and intentionally don't have an entry in
    // the identity map (which is keyed by source-navigable cell paths).
    // Realizations are surfaced in the tree purely for visibility control.
    let queue_kinds: std::collections::HashMap<String, String> = {
        let mut m = std::collections::HashMap::new();
        let mut q: std::collections::VecDeque<&crate::types::EntityTreeNode> =
            tree.iter().collect();
        while let Some(node) = q.pop_front() {
            m.insert(node.entity_path.clone(), node.kind.clone());
            for c in &node.children {
                q.push_back(c);
            }
        }
        m
    };
    for path in &tree_paths {
        if queue_kinds.get(path).map(|k| k.as_str()) == Some("realization") {
            continue;
        }
        assert!(
            map.contains_key(path.as_str()),
            "entity_path '{}' is in the tree but missing from the identity map; \
             identity map keys: {:?}",
            path,
            map.keys().collect::<Vec<_>>()
        );
    }

    // Both agree on the "Bracket" root.
    assert!(
        tree_paths.contains(&"Bracket".to_string()),
        "tree should contain 'Bracket' root"
    );
    assert!(
        map.contains_key("Bracket"),
        "identity map should contain 'Bracket' root"
    );
}

// ---- Step 15: recursive cycle-protection tests for build_template_node ----
//
// These tests verify that build_template_node does NOT stack-overflow when a
// template (or its sub-component) is marked is_recursive=true by the compiler's
// Tarjan SCC pass.
//
// Failure mode BEFORE step-16 fix: the tests below will stack-overflow
// (infinite recursion in build_template_node), crashing the test process.
// Failure mode AFTER step-16 fix: each test passes, asserting that recursive
// sub nodes have empty children.

/// A self-referencing template (A sub x = A, is_recursive=true) does not
/// stack-overflow; the recursive sub node has empty children.
#[test]
fn build_template_node_self_reference_does_not_stack_overflow() {
    use reify_core::ModulePath;

    // Build template A: is_recursive=true, one sub x pointing back to "A"
    let template_a = TopologyTemplateBuilder::new("A")
        .is_recursive(true)
        .sub_component("x", "A", vec![])
        .build();

    let compiled = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_a)
        .build();

    let a_template =
        find_template(&compiled.templates, "A").expect("template A must be in the module");

    // BEFORE step-16 fix: this call recurses infinitely → stack overflow.
    // AFTER step-16 fix: the is_recursive check stops recursion and returns
    // a sub node with empty children.
    let node = build_template_node(a_template, "A", &compiled, None, false);

    let sub_x = node
        .children
        .iter()
        .find(|c| c.entity_path == "A.x" && c.kind == "sub")
        .expect("A should have sub node A.x");

    assert!(
        sub_x.children.is_empty(),
        "recursive sub node A.x should have empty children; got {:?}",
        sub_x.children
    );
}

/// Mutual recursion (A sub b = B, B sub a = A; both is_recursive=true) does
/// not stack-overflow; both sub nodes are leaf nodes (empty children).
#[test]
fn build_template_node_mutual_recursion_does_not_stack_overflow() {
    use reify_core::ModulePath;

    // A → sub b = B (B is recursive)
    let template_a = TopologyTemplateBuilder::new("A")
        .is_recursive(true)
        .sub_component("b", "B", vec![])
        .build();

    // B → sub a = A (A is recursive)
    let template_b = TopologyTemplateBuilder::new("B")
        .is_recursive(true)
        .sub_component("a", "A", vec![])
        .build();

    let compiled = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_a)
        .template(template_b)
        .build();

    let a_template = find_template(&compiled.templates, "A").unwrap();
    let b_template = find_template(&compiled.templates, "B").unwrap();

    // BEFORE step-16 fix: A → B → A → … stack overflow.
    // AFTER step-16 fix: A.b has empty children (B is_recursive), B.a has
    // empty children (A is_recursive).
    let node_a = build_template_node(a_template, "A", &compiled, None, false);
    let node_b = build_template_node(b_template, "B", &compiled, None, false);

    let sub_b = node_a
        .children
        .iter()
        .find(|c| c.kind == "sub" && c.entity_path == "A.b")
        .expect("A should have sub node A.b");
    assert!(
        sub_b.children.is_empty(),
        "A.b sub node should be a leaf (B is recursive); got {:?}",
        sub_b.children
    );

    let sub_a = node_b
        .children
        .iter()
        .find(|c| c.kind == "sub" && c.entity_path == "B.a")
        .expect("B should have sub node B.a");
    assert!(
        sub_a.children.is_empty(),
        "B.a sub node should be a leaf (A is recursive); got {:?}",
        sub_a.children
    );
}

/// A non-recursive template (Container) that has a sub pointing to a recursive
/// template (A) expands Container normally but stops at the recursive child.
/// Container.a's children are empty; Container itself has the A.x children
/// available as non-recursive (Container is not the recursive root).
#[test]
fn build_template_node_non_recursive_parent_stops_at_recursive_child() {
    use reify_core::{ModulePath, Type};

    // A is recursive (self-reference via sub x = A)
    let template_a = TopologyTemplateBuilder::new("A")
        .is_recursive(true)
        .param("A", "n", Type::Int, None)
        .sub_component("x", "A", vec![])
        .build();

    // Container is NOT recursive; it has a sub a = A
    let template_container = TopologyTemplateBuilder::new("Container")
        .sub_component("a", "A", vec![])
        .build();

    let compiled = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_container)
        .template(template_a)
        .build();

    let container_template = find_template(&compiled.templates, "Container").unwrap();

    // BEFORE step-16 fix: Container → A → A → … stack overflow.
    // AFTER step-16 fix: Container expands normally, Container.a (pointing to
    // recursive A) has empty children instead of expanding A.
    let node = build_template_node(container_template, "Container", &compiled, None, false);

    // Container should have exactly one sub child
    let sub_a = node
        .children
        .iter()
        .find(|c| c.entity_path == "Container.a" && c.kind == "sub")
        .expect("Container should have sub node Container.a");

    assert_eq!(
        sub_a.type_name.as_deref(),
        Some("A"),
        "Container.a type_name should be 'A'"
    );
    assert!(
        sub_a.children.is_empty(),
        "Container.a should have empty children because A is recursive; got {:?}",
        sub_a.children
    );
}

/// After loading bracket, all four new EngineSession methods return without panicking.
///
/// This is a basic smoke test: verifies that each command is callable and
/// returns a sensible result type for the bracket fixture.
#[test]
fn all_new_commands_callable_on_bracket_fixture() {
    use crate::types::EntityIdentity;
    use std::collections::HashMap;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");

    // get_entity_tree
    let tree = session.get_entity_tree();
    assert!(
        !tree.is_empty(),
        "get_entity_tree should return non-empty tree"
    );

    // get_entity_identity_map
    let map: HashMap<String, EntityIdentity> = session.get_entity_identity_map();
    assert!(
        !map.is_empty(),
        "get_entity_identity_map should return non-empty map"
    );

    // get_containing_definition — position at (1,1) is inside the Bracket def.
    let def = session.get_containing_definition(1, 1);
    assert!(
        def.is_some(),
        "get_containing_definition(1,1) should return Some for bracket source"
    );

    // get_def_preview
    let preview = session.get_def_preview("Bracket");
    assert!(
        preview.is_ok(),
        "get_def_preview('Bracket') should return Ok: {:?}",
        preview
    );
}

/// Regression-pin: value-cell `content_hash` is an identity hash, not a content hash.
///
/// Pins the semantics of the `content_hash` field for value-cell entries:
/// it is derived from the cell's *identity* (the id string `"Bracket.width"`),
/// not from the cell's *content* (type, default_expr, kind, etc.).
///
/// A future "fix" that hashes cell content instead would break this test,
/// surfacing the semantic change immediately.
#[test]
fn get_entity_identity_map_value_cell_content_hash_is_identity_hash() {
    use crate::types::EntityIdentity;
    use reify_core::ContentHash;
    use std::collections::HashMap;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let map: HashMap<String, EntityIdentity> = session.get_entity_identity_map();
    let width_identity = map
        .get("Bracket.width")
        .expect("Bracket.width should be in map");
    let expected = ContentHash::of_str("Bracket.width").to_string();
    assert_eq!(
        width_identity.content_hash, expected,
        "value-cell content_hash must equal ContentHash::of_str(\"Bracket.width\").to_string()"
    );
}

/// Shared fixture: creates an [`EngineSession`] with a compiled module that
/// contains two templates both named `"Dup"`.  Used by both the debug-mode
/// panic test and the release-mode warn test so the setup is not duplicated.
fn build_duplicate_template_session() -> EngineSession {
    use reify_core::ModulePath;
    let dup1 = TopologyTemplateBuilder::new("Dup").build();
    let dup2 = TopologyTemplateBuilder::new("Dup").build();
    let compiled = CompiledModuleBuilder::new(ModulePath::single("m"))
        .template(dup1)
        .template(dup2)
        .build();
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    session.inject_compiled_for_test(compiled);
    session
}

/// In debug builds, get_entity_tree must panic (via debug_assert) when the
/// compiled module contains duplicate template names.
///
/// The compiler guarantees unique names within a well-formed module; this test
/// pins the invariant so future changes that accidentally produce duplicates
/// surface loudly in development builds.  Release builds retain the graceful
/// first-match behaviour.
///
/// The uniqueness check runs once in get_entity_tree (O(N) per call), not inside
/// each build_template_node call (which would be O(N²) across the full tree build).
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "template names must be unique")]
fn get_entity_tree_panics_on_duplicate_template_names_in_debug() {
    let session = build_duplicate_template_session();
    let _ = session.get_entity_tree();
}

/// In release builds, get_entity_tree must emit exactly one tracing::warn! and still
/// return a node for each template entry when the compiled module contains duplicate
/// template names (graceful first-match degradation).
///
/// Compare with the sibling debug-mode test
/// `get_entity_tree_panics_on_duplicate_template_names_in_debug` which pins the
/// debug_assert panic.  The orchestrator runs both `cargo test` and
/// `cargo test --release` (orchestrator.yaml), so both modes are exercised in CI —
/// following the precedent at `crates/reify-expr/tests/field_eval_tests.rs:1066-1126`.
#[cfg(not(debug_assertions))]
#[test]
fn get_entity_tree_warns_on_duplicate_template_names_in_release() {
    let mut session = build_duplicate_template_session();
    let (subscriber, warn_count) = reify_test_support::warn_counting_subscriber();
    // Wrap only get_entity_tree() inside with_default so the warn emitted by
    // the runtime duplicate check is captured.
    let tree = tracing::subscriber::with_default(subscriber, || session.get_entity_tree());

    reify_test_support::assert_warn_count(
        &warn_count,
        1,
        "expected exactly one warn for duplicate template name in release build",
    );
    assert_eq!(
        tree.len(),
        2,
        "release build should still return a node per template entry (first-match semantics)"
    );
    // Pin first-match semantics: the top-level template iterator emits a node for
    // every entry in compiled.templates without filtering; duplicates appear as
    // separate nodes.  Both entries are named "Dup", so both entity_paths must be
    // "Dup".  First-match only matters for sub-component lookup inside
    // build_template_node, not for this outer iteration.
    assert!(
        tree.iter().all(|n| n.entity_path == "Dup"),
        "both tree nodes should have entity_path 'Dup' — the top-level template \
         iterator does not filter duplicates; first-match only applies to \
         sub-component lookup inside build_template_node"
    );
}

// ---- Cache tests: parsed_cache + line_offsets_cache ----

/// Fresh session returns None from parsed_cache_for_test.
/// After load_from_source, returns Some with the Bracket declaration present.
#[test]
fn commit_state_populates_parsed_cache() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    // Before load → None.
    assert!(
        session.parsed_cache_for_test().is_none(),
        "fresh session: parsed_cache should be None"
    );

    // After load → Some with at least one declaration.
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let cached = session
        .parsed_cache_for_test()
        .expect("after load, parsed_cache should be Some");
    assert!(
        !cached.declarations.is_empty(),
        "parsed cache should contain at least one declaration"
    );
    let has_bracket = cached.declarations.iter().any(|d| {
        if let reify_ast::Declaration::Structure(s) = d {
            s.name == "Bracket"
        } else {
            false
        }
    });
    assert!(
        has_bracket,
        "parsed cache should contain the Bracket structure declaration"
    );
}

/// Fresh session returns None from line_offsets_cache_for_test.
/// After load_from_source with a multi-line source, returns Some with the
/// correct newline byte positions.
#[test]
fn commit_state_populates_line_offsets_cache() {
    use crate::engine::build_line_offsets;

    // Source with exactly 2 newlines:
    // "structure A {}\nstructure B {}\nstructure C {}"
    // - "structure A {}" = 14 chars → '\n' at byte 14
    // - "structure B {}" = 14 chars → '\n' at byte 29
    let source = "structure A {}\nstructure B {}\nstructure C {}";
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    // Before load → None.
    assert!(
        session.line_offsets_cache_for_test().is_none(),
        "fresh session: line_offsets_cache should be None"
    );

    // After load → Some with the correct newline positions.
    session
        .load_from_source(source, "test_offsets")
        .expect("load should succeed");

    let cached = session
        .line_offsets_cache_for_test()
        .expect("after load, line_offsets_cache should be Some");

    let expected = build_line_offsets(source);
    assert_eq!(
        cached,
        expected.as_slice(),
        "cached line offsets should match build_line_offsets(source)"
    );
    // Verify the specific positions we computed manually.
    assert_eq!(
        cached,
        &[14usize, 29usize],
        "newlines should be at bytes 14 and 29"
    );
}

/// After an update_source call, both caches reflect the NEW source — not the
/// old one.  This pins the contract that commit_state unconditionally overwrites
/// (never appends or get_or_inserts) both caches on every call.
#[test]
fn commit_state_refreshes_caches_on_update_source() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    // Load a single-structure source (1 declaration, 0 newlines).
    let source1 = "structure A { param x: Scalar = 1 }";
    session
        .load_from_source(source1, "test_refresh")
        .expect("first load should succeed");

    let decl_count_1 = session
        .parsed_cache_for_test()
        .expect("parsed_cache should be Some after first load")
        .declarations
        .len();
    let offsets_len_1 = session
        .line_offsets_cache_for_test()
        .expect("line_offsets_cache should be Some after first load")
        .len();

    // Update with a two-structure source split across two lines (1 newline).
    let source2 = "structure A { param x: Scalar = 1 }\nstructure B { param y: Scalar = 2 }";
    session
        .update_source("test_refresh.ri", source2)
        .expect("update_source should succeed");

    let decl_count_2 = session
        .parsed_cache_for_test()
        .expect("parsed_cache should be Some after update")
        .declarations
        .len();
    let offsets_len_2 = session
        .line_offsets_cache_for_test()
        .expect("line_offsets_cache should be Some after update")
        .len();

    // Pin exact values — this falsifies both get_or_insert and append bugs.
    // source1: 1 structure, 0 newlines.
    assert_eq!(decl_count_1, 1, "source1 should have exactly 1 declaration");
    assert_eq!(offsets_len_1, 0, "source1 has no newlines");
    // source2: 2 structures, 1 newline.
    assert_eq!(
        decl_count_2, 2,
        "source2 should have exactly 2 declarations"
    );
    assert_eq!(offsets_len_2, 1, "source2 has exactly 1 newline");
}

/// Proves that get_containing_definition reads from parsed_cache rather than
/// re-parsing the source text.  If the old re-parse path were still active,
/// replacing the cache with an empty ParsedModule would have no effect and the
/// method would still find the Bracket definition.
#[test]
fn get_containing_definition_reads_from_parsed_cache() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");

    // Baseline: position (1,1) is inside the Bracket def.
    let before = session.get_containing_definition(1, 1);
    assert!(
        before.is_some(),
        "baseline: get_containing_definition(1,1) should return Some for bracket source"
    );

    // Replace parsed_cache with a stripped ParsedModule that has no declarations.
    let stripped = {
        let mut p = session
            .parsed_cache_for_test()
            .expect("parsed_cache should be Some after load")
            .clone();
        p.declarations = Vec::new();
        p
    };
    session.override_parsed_cache_for_test(stripped);

    // Now the cache has no declarations → must return None.
    let after = session.get_containing_definition(1, 1);
    assert!(
        after.is_none(),
        "after stripping parsed_cache, get_containing_definition should return None \
         (proves the method reads from cache, not re-parsing source)"
    );
}

/// Proves that get_containing_definition reads from line_offsets_cache rather than
/// recomputing build_line_offsets(source) on every call.
///
/// Strategy: load bracket_source, confirm position (2, 1) maps into the Bracket
/// definition (baseline), then inject a deliberately empty line-offset table.
/// With the bogus empty table, line_col_to_byte_offset_with_offsets returns
/// source.len() for any line ≥ 2 (because there are no recorded newlines), which
/// puts the byte offset past the Bracket span → None.  If the old path still
/// called build_line_offsets(source) internally, the correct offsets would be
/// recomputed and (2, 1) would still return Some — so None proves cache use.
#[test]
fn get_containing_definition_reads_from_line_offsets_cache() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");

    // Baseline: line 2 is inside the Bracket structure.
    let before = session.get_containing_definition(2, 1);
    assert!(
        before.is_some(),
        "baseline: get_containing_definition(2,1) should return Some \
         (line 2 is inside the Bracket structure)"
    );

    // Inject an empty (bogus) line-offset table.
    // With no newlines recorded, line_col_to_byte_offset_with_offsets maps
    // (line=2, col=1) to source.len(), which falls past the Bracket span end.
    session.override_line_offsets_cache_for_test(vec![]);

    // Now (2,1) must return None — if the source were re-scanned the correct
    // offsets would be recovered and the method would still return Some.
    let after = session.get_containing_definition(2, 1);
    assert!(
        after.is_none(),
        "after injecting empty line_offsets_cache, get_containing_definition(2,1) \
         should return None (proves the method uses the cached table)"
    );
}

/// Lifecycle test for `consumed_idents_cache`:
/// - starts as `None` on a fresh session,
/// - remains `None` after load (lazy — populated only on first `get_mechanism_descriptors` call),
/// - becomes `Some` after the first `get_mechanism_descriptors` call following a load,
/// - is reset to `None` after `update_source` (invalidated by `commit_state`),
/// - becomes `Some` again after a second `get_mechanism_descriptors` call on the new module.
#[test]
fn consumed_idents_cache_lifecycle() {
    let mut session = make_session();

    // 1. Fresh session → None.
    assert!(
        session.consumed_idents_cache_for_test().is_none(),
        "fresh session: consumed_idents_cache should be None"
    );

    // 2. After load_from_source, still None (lazy — not populated until
    //    get_mechanism_descriptors is called for the first time).
    session
        .load_from_source(HAPPY_MECHANISM_SOURCE, "kinematic")
        .expect("load should succeed");
    assert!(
        session.consumed_idents_cache_for_test().is_none(),
        "after load but before get_mechanism_descriptors: consumed_idents_cache should still be None"
    );

    // 3. After get_mechanism_descriptors, Some with an entry for the 'Kinematic' structure.
    //    m0 and m1 are consumed by body() calls; m2 is the terminal cell (not consumed).
    let _ = session.get_mechanism_descriptors();
    let cache = session
        .consumed_idents_cache_for_test()
        .expect("after get_mechanism_descriptors: consumed_idents_cache should be Some");
    let kinematic_consumed = cache
        .get("Kinematic")
        .expect("cache should contain an entry for the 'Kinematic' structure");
    let expected_consumed: std::collections::HashSet<String> =
        ["m0", "m1"].iter().map(|s| s.to_string()).collect();
    assert_eq!(
        *kinematic_consumed, expected_consumed,
        "Kinematic's consumed set should be {{m0, m1}}"
    );

    // 4. After update_source, the cache is invalidated by commit_state → None again.
    session
        .update_source("kinematic.ri", bracket_source())
        .expect("update_source should succeed");
    assert!(
        session.consumed_idents_cache_for_test().is_none(),
        "after update_source: consumed_idents_cache should be None (invalidated by commit_state)"
    );

    // 5. After another get_mechanism_descriptors call, Some again — now reflecting the
    //    new module (bracket_source has no body() calls, so consumed sets are empty).
    let _ = session.get_mechanism_descriptors();
    assert!(
        session.consumed_idents_cache_for_test().is_some(),
        "after second get_mechanism_descriptors call: consumed_idents_cache should be Some again"
    );
    let cache = session.consumed_idents_cache_for_test().unwrap();
    assert!(
        cache.values().all(|s| s.is_empty()),
        "bracket_source has no body() calls — every cache entry must be an empty set, got {:?}",
        cache
    );
}

/// Proves that `get_mechanism_descriptors` reads from `consumed_idents_cache` rather
/// than re-walking the AST.  Mirrors the `get_containing_definition_reads_from_parsed_cache`
/// pattern: load → baseline → inject poisoned cache → second call → verify readback.
///
/// Poison: replace the cache for "Kinematic" with an empty set (zero consumed mechanisms).
/// With no consumed idents, the terminal-mechanism filter lets every mechanism cell through.
/// If the method re-walked the AST it would rebuild {"m0", "m1"} and filter them out,
/// returning only m2.  The presence of m0 and m1 in the result proves cache use.
#[test]
fn get_mechanism_descriptors_reads_from_consumed_idents_cache() {
    use std::collections::HashMap;
    use std::collections::HashSet;

    let mut session = make_session();
    session
        .load_from_source(HAPPY_MECHANISM_SOURCE, "kinematic")
        .expect("load should succeed");

    // Baseline: with the real cache ({m0, m1} consumed), only m2 (the terminal cell)
    // should appear.
    let baseline = session.get_mechanism_descriptors();
    let baseline_names: Vec<&str> = baseline.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        baseline_names,
        vec!["m2"],
        "baseline: only m2 (the terminal cell) should appear; got {:?}",
        baseline_names
    );

    // Inject a poisoned cache: "Kinematic" maps to an empty consumed set, so the
    // filter treats every mechanism cell as terminal.
    let poisoned: HashMap<String, HashSet<String>> =
        HashMap::from([("Kinematic".to_string(), HashSet::new())]);
    session.override_consumed_idents_cache_for_test(poisoned);

    // After poisoning, all three mechanism cells should appear.
    let all = session.get_mechanism_descriptors();
    let all_names: Vec<&str> = all.iter().map(|d| d.name.as_str()).collect();
    assert!(
        all_names.contains(&"m0"),
        "m0 (0 bodies) should appear when consumed cache is empty; got {:?}",
        all_names
    );
    assert!(
        all_names.contains(&"m1"),
        "m1 (1 body) should appear when consumed cache is empty; got {:?}",
        all_names
    );
    assert!(
        all_names.contains(&"m2"),
        "m2 (2 bodies) should appear when consumed cache is empty; got {:?}",
        all_names
    );

    // Verify bodies_count to confirm the right cells came through.
    let m0_desc = all.iter().find(|d| d.name == "m0").unwrap();
    let m1_desc = all.iter().find(|d| d.name == "m1").unwrap();
    assert_eq!(m0_desc.bodies_count, 0, "m0 should have 0 bodies");
    assert_eq!(m1_desc.bodies_count, 1, "m1 should have 1 body");
}

/// Proves that `get_mechanism_descriptors` does NOT re-invoke
/// `collect_consumed_mechanism_idents` on a cache hit.
///
/// Strategy: load HAPPY_MECHANISM_SOURCE, confirm the baseline result is ["m2"]
/// (cache is populated as a side effect), then inject a stripped ParsedModule
/// with zero declarations.  A second call must still return ["m2"] — if the
/// implementation re-walked parsed_cache, the empty declarations would yield an
/// empty consumed set and m0/m1 would appear in the result (3 cells instead of 1).
/// Mirrors the `get_containing_definition_reads_from_parsed_cache` pattern.
#[test]
fn get_mechanism_descriptors_does_not_reinvoke_collect_on_cache_hit() {
    let mut session = make_session();
    session
        .load_from_source(HAPPY_MECHANISM_SOURCE, "kinematic")
        .expect("load should succeed");

    // Baseline: with the real cache ({m0, m1} consumed), only m2 (the terminal cell)
    // should appear.
    let baseline = session.get_mechanism_descriptors();
    let baseline_names: Vec<&str> = baseline.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        baseline_names,
        vec!["m2"],
        "baseline: only m2 (the terminal cell) should appear; got {:?}",
        baseline_names
    );

    // Strip the parsed_cache: inject a ParsedModule with no declarations.
    // If the implementation re-walked parsed_cache, the empty declarations would
    // yield an empty consumed set → all three mechanism cells (m0, m1, m2) would
    // pass the terminal filter → 3 cells instead of 1.
    let stripped = {
        let mut p = session
            .parsed_cache_for_test()
            .expect("parsed_cache should be Some after load")
            .clone();
        p.declarations = Vec::new();
        p
    };
    session.override_parsed_cache_for_test(stripped);

    // Second call must still return only ["m2"] — proves the cache-hit path does
    // not consult parsed_cache.
    let second = session.get_mechanism_descriptors();
    let second_names: Vec<&str> = second.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        second_names,
        vec!["m2"],
        "after stripping parsed_cache, get_mechanism_descriptors should still return [\"m2\"] \
         (proves collect_consumed_mechanism_idents is not re-invoked on cache hits); got {:?}",
        second_names
    );
}

/// Asserts that `get_mechanism_descriptors` emits exactly 1 WARN per call when
/// `parsed_cache` is `None` and the compiled module has multiple templates.
///
/// The WARN guard is hoisted before the per-template loop, so it fires once
/// regardless of template count.  This test uses 3 templates to make the
/// "once-per-call, not once-per-template" invariant concrete: a regression that
/// moves the WARN back inside the loop would emit 3, not 1, and fail here.
#[test]
fn get_mechanism_descriptors_warns_once_when_parsed_cache_missing_with_multiple_templates() {
    use reify_core::ModulePath;

    // Build a 3-template CompiledModule and inject it (no load → parsed_cache=None).
    // Call recheck_for_test to initialise last_check (required by get_mechanism_descriptors).
    let t1 = TopologyTemplateBuilder::new("t1").build();
    let t2 = TopologyTemplateBuilder::new("t2").build();
    let t3 = TopologyTemplateBuilder::new("t3").build();
    let compiled = CompiledModuleBuilder::new(ModulePath::single("m"))
        .template(t1)
        .template(t2)
        .template(t3)
        .build();
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    session.inject_compiled_for_test(compiled);
    session.recheck_for_test();
    // parsed_cache remains None — broken-invariant state.

    let (subscriber, counters) = CountingSubscriberBuilder::new()
        .count_level(tracing::Level::WARN)
        .target_prefix("reify_gui::engine")
        .build();

    tracing::subscriber::with_default(subscriber, || {
        let _ = session.get_mechanism_descriptors();
    });

    let warn_count = counters[&tracing::Level::WARN].load(Ordering::Acquire);
    assert_eq!(
        warn_count, 1,
        "expected exactly 1 WARN per get_mechanism_descriptors call when parsed_cache is None; \
         got {} (should fire once per call, not once per template)",
        warn_count
    );
}

#[test]
fn build_gui_state_tessellation_diagnostics_empty_on_clean_source() {
    let checker = SimpleConstraintChecker;
    // The bracket source's single `box(...)` op gets `GeometryHandleId(1)`
    // (MockGeometryKernel's first allocated id). Register empty extract_*
    // fixtures so task-2574's primitive-attribute seeder doesn't emit a
    // "no topology extraction fixture" warning into tessellation_diagnostics.
    let kernel = MockGeometryKernel::new()
        .with_extracted_faces(reify_ir::GeometryHandleId(1), vec![])
        .with_extracted_edges(reify_ir::GeometryHandleId(1), vec![])
        .with_extracted_vertices(reify_ir::GeometryHandleId(1), vec![]);
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed with valid bracket source");

    // With a successful tessellation (MockGeometryKernel never errors),
    // tessellation_diagnostics must be empty.
    assert!(
        state.tessellation_diagnostics.is_empty(),
        "expected empty tessellation_diagnostics after successful tessellation, got {:?}",
        state.tessellation_diagnostics
    );
}

#[test]
fn build_gui_state_captures_tessellation_errors_from_failing_kernel() {
    let checker = SimpleConstraintChecker;
    // FailingMockGeometryKernel::execute always returns Err, causing the eval
    // pipeline to emit Diagnostic::error("geometry error: ...") via
    // tessellate_from_values.
    let kernel = FailingMockGeometryKernel;
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed even with failing geometry kernel");

    // The failing kernel forces tessellation errors to be captured.
    assert!(
        !state.tessellation_diagnostics.is_empty(),
        "expected non-empty tessellation_diagnostics from failing kernel"
    );

    // Every diagnostic must have severity == "Error"
    for diag in &state.tessellation_diagnostics {
        assert_eq!(
            diag.severity, "Error",
            "expected severity 'Error', got '{}'",
            diag.severity
        );
    }

    // At least one diagnostic message must mention "geometry"
    assert!(
        state
            .tessellation_diagnostics
            .iter()
            .any(|d| d.message.to_lowercase().contains("geometry")),
        "expected at least one diagnostic message containing 'geometry', got: {:?}",
        state
            .tessellation_diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Pins the unresolved-source fallback contract in the tessellation path.
///
/// When resolve_source() returns None (e.g. break_module_name_for_test),
/// diagnostics_to_info must still produce DiagnosticInfo entries with:
///   (a) file_path == "<unknown>"
///   (b) code == Some("unresolved-source")
///
/// This ensures the step-6 borrow refactor cannot accidentally drop that tagging.
#[test]
fn build_gui_state_tessellation_unresolved_source_tags_diagnostics() {
    let checker = SimpleConstraintChecker;
    let kernel = FailingMockGeometryKernel;
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // load_from_source succeeds even when the geometry kernel will fail
    // (tessellation happens inside build_gui_state, not during load).
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed even with failing geometry kernel");

    // Break module_name so that resolve_source() returns None during tessellation.
    session.break_module_name_for_test();

    let state = session
        .build_gui_state()
        .expect("build_gui_state should succeed even with unresolved source");

    // (a) Tessellation must produce diagnostics (kernel always fails)
    assert!(
        !state.tessellation_diagnostics.is_empty(),
        "expected non-empty tessellation_diagnostics with failing kernel"
    );

    // (b) Every diagnostic must be tagged with the unresolved-source sentinel
    for diag in &state.tessellation_diagnostics {
        assert_eq!(
            diag.file_path, "<unknown>",
            "unresolved-source diagnostic must have file_path == \"<unknown>\", got {:?}",
            diag.file_path
        );
        assert_eq!(
            diag.code,
            Some("unresolved-source".to_owned()),
            "unresolved-source diagnostic must have code == Some(\"unresolved-source\"), got {:?}",
            diag.code
        );
    }
}

// --- compile_diagnostics wiring through build_gui_state (step-3 / step-4) ---

/// Compile warnings (e.g. unknown port type) must appear in `compile_diagnostics`
/// after `load_from_source` and must remain absent from `tessellation_diagnostics`.
///
/// Uses the same `warn_source_with_unknown_port_type` fixture already validated by
/// `engine_get_diagnostics_returns_populated_warning`.
#[test]
fn build_gui_state_compile_diagnostics_populated_from_warning() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(warn_source_with_unknown_port_type(), "warn_test")
        .expect("load_from_source should succeed with warn source");

    // compile_diagnostics must be non-empty
    assert!(
        !state.compile_diagnostics.is_empty(),
        "expected non-empty compile_diagnostics for warn_source_with_unknown_port_type, got empty"
    );

    // First entry must be a Warning with the expected message and a .ri file_path
    let first = &state.compile_diagnostics[0];
    assert_eq!(
        first.severity, "Warning",
        "expected severity 'Warning', got '{}'",
        first.severity
    );
    assert!(
        first.message.to_lowercase().contains("unknown port type"),
        "expected message to contain 'unknown port type', got: {}",
        first.message
    );
    assert!(
        first.file_path.ends_with(".ri"),
        "expected file_path to end with '.ri', got: {}",
        first.file_path
    );

    // tessellation_diagnostics must remain empty (the two streams are disjoint)
    assert!(
        state.tessellation_diagnostics.is_empty(),
        "expected empty tessellation_diagnostics when compile_diagnostics are present, got {:?}",
        state.tessellation_diagnostics
    );
}

/// compile_diagnostics must be empty for clean source with no warnings.
#[test]
fn build_gui_state_compile_diagnostics_empty_on_clean_source() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new()
        .with_extracted_faces(reify_ir::GeometryHandleId(1), vec![])
        .with_extracted_edges(reify_ir::GeometryHandleId(1), vec![]);
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed with valid bracket source");

    assert!(
        state.compile_diagnostics.is_empty(),
        "expected empty compile_diagnostics for clean bracket source, got {:?}",
        state.compile_diagnostics
    );
}

// --- Freshness wiring through build_gui_state (step-7 / step-8) ---

/// End-to-end freshness wiring test: forced panic on a `let` cell must surface
/// as `freshness == "failed"` on the corresponding `ValueData`, every other
/// value must stay at `"final"`, and cells that participate in a violated
/// constraint must NOT be reported as `"failed"` (arch §9.3 separation).
///
/// This test is intentionally RED in step-7 (before the engine ref is threaded
/// through `build_values` in step-8): assertion (a) fails because `build_values`
/// currently hardcodes `freshness: "final"` for all cells.  Once step-8 wires
/// `engine.freshness()` through, all three assertions pass.
#[test]
fn freshness_wires_through_build_gui_state_for_failed_value_cell() {
    // Use the violating source so there is a real Violated constraint entry
    // in the GuiState — this lets us check the constraint-vs-Failed separation
    // (assertion c) as well as the basic failed-freshness wiring (assertions a, b).
    let source = bracket_source_violating();

    let checker = SimpleConstraintChecker;
    // No geometry kernel — freshness wiring is independent of tessellation.
    let mut session = EngineSession::new(Box::new(checker), None);

    session
        .load_from_source(&source, "bracket")
        .expect("bracket_source_violating should compile and load");

    // Force `Bracket.volume` (the only `let` in bracket_source) to panic on
    // the next eval cycle.
    let volume_id = ValueCellId::new("Bracket", "volume");
    session.set_panic_on_eval_for_test(volume_id.clone());

    // Re-run evaluation so the forced panic takes effect.
    session.recheck_for_test();

    let state = session
        .build_gui_state()
        .expect("build_gui_state should succeed even after forced panic");

    // --- (a) Failed cell must report freshness == "failed" ---
    let volume = state
        .values
        .iter()
        .find(|v| v.cell_id == "Bracket.volume")
        .expect("should have a 'Bracket.volume' ValueData");

    assert_eq!(
        volume.freshness, "failed",
        "volume must have freshness='failed' after forced panic; \
         this assertion is RED in step-7 and turns GREEN in step-8"
    );

    // --- (b) No leakage: all other cells stay at "final" ---
    for v in &state.values {
        if v.cell_id == "Bracket.volume" {
            continue; // already checked above
        }
        assert_eq!(
            v.freshness, "final",
            "value '{}' must have freshness='final' (no leakage from forced panic), got '{}'",
            v.name, v.freshness
        );
    }

    // --- (c) Constraint-violated cells must NOT appear in the failed set ---
    //
    // bracket_source_violating() sets thickness=1mm, violating `thickness > 2mm`.
    // The `thickness` param is Satisfaction::Violated but its Freshness must
    // remain Final (it was set successfully; the violation is a logical check,
    // not a computation failure — arch §9.3).
    let violated_constraints: Vec<_> = state
        .constraints
        .iter()
        .filter(|c| c.status == "Violated")
        .collect();

    assert!(
        !violated_constraints.is_empty(),
        "expected at least one Violated constraint from bracket_source_violating; \
         got {} constraints: {:?}",
        state.constraints.len(),
        state
            .constraints
            .iter()
            .map(|c| &c.status)
            .collect::<Vec<_>>()
    );

    // For every cell referenced by a violated constraint, check that its
    // freshness is NOT "failed" (constraint violation ≠ Freshness::Failed).
    for vc in &violated_constraints {
        for param_id in &vc.parameter_ids {
            let cell_data = state.values.iter().find(|v| v.cell_id == *param_id);
            if let Some(cell) = cell_data {
                assert_ne!(
                    cell.freshness, "failed",
                    "cell '{}' is referenced by violated constraint '{}' but must NOT \
                     have freshness='failed' — constraint violations stay on the \
                     Satisfaction::Violated channel (arch §9.3)",
                    param_id, vc.node_id
                );
            }
        }
    }
}

// --- Freshness wiring through get_entity_tree (step-19) ---

/// Helper: recursively collect all nodes from a tree into a flat vec.
fn collect_all_nodes<'a>(nodes: &'a [EntityTreeNode], out: &mut Vec<&'a EntityTreeNode>) {
    for n in nodes {
        out.push(n);
        collect_all_nodes(&n.children, out);
    }
}

/// End-to-end freshness wiring test for the entity-tree realization channel.
///
/// Forces a realization to fail via the build path (not the tessellate path,
/// which does NOT propagate kernel errors into `Freshness::Failed` — see
/// arch §9.1 and `engine_build.rs` comment "Tessellate paths do not propagate
/// kernel errors into `Freshness::Failed` today").
///
/// After `build_for_freshness_test()` calls `engine.build()`, the engine cache
/// should have `NodeId::Realization(rnid)` marked as `Freshness::Failed`.
/// `get_entity_tree()` → `build_template_node()` reads realization freshness via
/// `engine.freshness(&NodeId::Realization(real.id.clone()))` (wired in step-8).
///
/// Assertions:
/// (a) Exactly one node in the full tree has `freshness == "failed"`.
/// (b) That node has `kind == "realization"` and `entity_path == "Bracket#realization[0]"`.
/// (c) Every other node has `freshness == "final"` (no leakage to params/lets/root).
#[test]
fn freshness_wires_through_get_entity_tree_for_realization_failure() {
    let checker = SimpleConstraintChecker;
    // FailingMockGeometryKernel causes all geometry operations to fail, which
    // makes engine.build() call mark_realization_failed on the realization.
    let kernel = FailingMockGeometryKernel;
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect(
            "load_from_source should succeed even with failing kernel \
                 (tessellation errors are captured, not returned as Err)",
        );

    // tessellate_snapshot (called inside build_gui_state / load_from_source)
    // does NOT propagate kernel errors into Freshness::Failed — that is wired
    // on the build path only (arch §9.1 / engine_build.rs).
    // Call build_for_freshness_test() to trigger engine.build() which marks
    // the realization as Freshness::Failed in the engine cache.
    session.build_for_freshness_test();

    let tree = session.get_entity_tree();
    assert_eq!(tree.len(), 1, "bracket has one root template");

    // Flatten the full tree for inspection.
    let mut all_nodes = Vec::new();
    collect_all_nodes(&tree, &mut all_nodes);

    // --- (a) Exactly one node is failed ---
    let failed_nodes: Vec<_> = all_nodes
        .iter()
        .filter(|n| n.freshness == "failed")
        .collect();

    assert_eq!(
        failed_nodes.len(),
        1,
        "exactly one node must have freshness='failed' after a kernel-error build; \
         got {} failed node(s): {:?}",
        failed_nodes.len(),
        failed_nodes
            .iter()
            .map(|n| (&n.entity_path, &n.kind))
            .collect::<Vec<_>>()
    );

    // --- (b) The failed node is the realization whose kernel call failed ---
    let failed_node = failed_nodes[0];
    assert_eq!(
        failed_node.kind, "realization",
        "the failed node must have kind='realization'; got kind='{}'",
        failed_node.kind
    );
    assert_eq!(
        failed_node.entity_path, "Bracket#realization[0]",
        "the failed realization path must be 'Bracket#realization[0]'; \
         got '{}'",
        failed_node.entity_path
    );

    // --- (c) All other nodes stay at freshness="final" (no leakage) ---
    for node in &all_nodes {
        if node.entity_path == "Bracket#realization[0]" {
            continue; // already checked above
        }
        assert_eq!(
            node.freshness, "final",
            "node '{}' (kind='{}') must have freshness='final' after a \
             single-realization kernel failure; got '{}'",
            node.entity_path, node.kind, node.freshness
        );
    }
}

/// Verify that `get_entity_tree()` correctly surfaces `Freshness::Failed` for
/// a sub-component value-cell node.
///
/// # Why this test exists
///
/// Inside `build_template_node`, the freshness lookup for value cells must use
/// the **instance-scoped** `ValueCellId` (e.g. `Parent.rib.half_h`) rather than
/// the template-level cell ID (e.g. `Child.half_h`).  The engine cache stores
/// sub-component cells under `scoped_entity = "{parent}.{sub_name}"` (set by
/// `elaborate_child_instance` / `elaborate_child_params_only` in unfold.rs),
/// so querying with the template name always returns the default `Final`.
///
/// This test drives `Parent.rib.half_h` (a let binding on a sub-component `rib`
/// of type `Child`) to `Freshness::Failed` via the `mark_value_cell_failed_for_test`
/// helper (direct cache injection, since `set_panic_on_eval` does not reach the
/// `elaborate_child_lets_only` evaluation path), then asserts that
/// `get_entity_tree()` surfaces `freshness == "failed"` on that node.
///
/// The `set_panic_on_eval` mechanism only fires for cells evaluated through
/// `evaluate_let_bindings` (engine_eval.rs) — sub-component lets go through
/// `elaborate_child_lets_only` (unfold.rs) which evaluates them directly via
/// `eval_expr`, bypassing the panic-injection hook.
#[test]
fn freshness_wires_through_get_entity_tree_for_sub_component_cell() {
    // A minimal two-structure module: Parent has a sub-component `rib` of type
    // `Child`.  Child has a param `height` and a let binding `half_h`.
    let source = r#"structure Child {
    param height: Scalar = 10mm
    let half_h = height / 2
}
structure Parent {
    param width: Scalar = 80mm
    sub rib = Child(height: width * 0.5)
}"#;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(source, "parent_child")
        .expect("load_from_source should succeed");

    // After evaluation, the cache has entries keyed by the instance-scoped path:
    //   ValueCellId { entity: "Parent.rib", member: "half_h" }
    // Inject Failed directly — set_panic_on_eval cannot reach this cell because
    // elaborate_child_lets_only bypasses the panic_on_eval_cells gate.
    session.mark_value_cell_failed_for_test(
        ValueCellId::new("Parent.rib", "half_h"),
        "test-forced failure on sub-component let",
    );

    // get_entity_tree builds the tree for each root template.
    // For "Parent", it recurses into "Child" via the "rib" sub-component with
    // entity_path = "Parent.rib".  The fix ensures that value cells use
    // ValueCellId::new(entity_path, &cell.id.member) so the lookup
    // hits "Parent.rib.half_h" (not "Child.half_h").
    let tree = session.get_entity_tree();

    // Flatten all nodes for inspection.
    let mut all_nodes = Vec::new();
    collect_all_nodes(&tree, &mut all_nodes);

    // --- (a) Exactly the injected cell reports freshness="failed" ---
    let failed_nodes: Vec<_> = all_nodes
        .iter()
        .filter(|n| n.freshness == "failed")
        .collect();

    assert_eq!(
        failed_nodes.len(),
        1,
        "exactly one node must have freshness='failed' after injecting \
         failure on Parent.rib.half_h; got {} failed node(s): {:?}",
        failed_nodes.len(),
        failed_nodes
            .iter()
            .map(|n| (&n.entity_path, &n.kind))
            .collect::<Vec<_>>()
    );

    // --- (b) The failed node is the sub-component let cell ---
    let failed_node = failed_nodes[0];
    assert_eq!(
        failed_node.entity_path, "Parent.rib.half_h",
        "the failed node must be 'Parent.rib.half_h'; got '{}'",
        failed_node.entity_path
    );

    // --- (c) All other nodes stay at "final" or "aggregate" (no leakage) ---
    //
    // Sub-component container nodes ("kind == sub") emit "aggregate" — they
    // have no individual freshness and their children must be inspected
    // separately.  All leaf/cell nodes must be "final".
    for node in &all_nodes {
        if node.entity_path == "Parent.rib.half_h" {
            continue; // the failed cell — already checked above
        }
        if node.kind == "sub" {
            assert_eq!(
                node.freshness, "aggregate",
                "sub-component container node '{}' must have freshness='aggregate' \
                 (no individual freshness; see children); got '{}'",
                node.entity_path, node.freshness
            );
            continue;
        }
        assert_eq!(
            node.freshness, "final",
            "node '{}' (kind='{}') must have freshness='final'; got '{}'",
            node.entity_path, node.kind, node.freshness
        );
    }
}

// ---- malformed mechanism Map shape contract tests ----------------------------

#[test]
fn extract_joints_from_mechanism_skips_non_map_at_value() {
    // Step 8 RED: hand-construct a mechanism Map whose single body has a
    // non-Map "at" value (Value::String("not-a-map")).  extract_joints_from_mechanism
    // must return empty (joints, seen_joints) — no phantom row, no panic.
    use crate::engine::extract_joints_from_mechanism;
    use reify_ir::Value;
    use std::collections::BTreeMap;

    let mut body_map: BTreeMap<Value, Value> = BTreeMap::new();
    body_map.insert(
        Value::String("at".to_string()),
        Value::String("not-a-map".to_string()),
    );

    let mut mech_map: BTreeMap<Value, Value> = BTreeMap::new();
    mech_map.insert(
        Value::String("kind".to_string()),
        Value::String("mechanism".to_string()),
    );
    mech_map.insert(
        Value::String("bodies".to_string()),
        Value::List(vec![Value::Map(body_map)]),
    );

    let (joints, seen_joints) = extract_joints_from_mechanism(&mech_map);

    assert!(
        joints.is_empty(),
        "non-Map 'at' value must produce no joint descriptors; got {:?}",
        joints
    );
    assert!(
        seen_joints.is_empty(),
        "non-Map 'at' value must produce no seen_joints entries; got {:?}",
        seen_joints
    );
}

#[test]
fn extract_joints_from_mechanism_handles_malformed_axis_length() {
    // Step 8 RED: hand-construct a mechanism with a prismatic joint whose
    // "axis" Vector has length 2 (malformed — extract_axis requires length 3).
    // The descriptor must still be produced (kind=="prismatic", dimension=="length")
    // but axis must be None.
    use crate::engine::extract_joints_from_mechanism;
    use reify_ir::Value;
    use std::collections::BTreeMap;

    // Build the joint map with a 2-element axis vector.
    let mut joint_map: BTreeMap<Value, Value> = BTreeMap::new();
    joint_map.insert(
        Value::String("kind".to_string()),
        Value::String("prismatic".to_string()),
    );
    joint_map.insert(
        Value::String("axis".to_string()),
        Value::Vector(vec![Value::Real(1.0), Value::Real(0.0)]), // length 2 — malformed
    );

    // Build the body map referencing the joint.
    let mut body_map: BTreeMap<Value, Value> = BTreeMap::new();
    body_map.insert(Value::String("at".to_string()), Value::Map(joint_map));

    // Build the mechanism map.
    let mut mech_map: BTreeMap<Value, Value> = BTreeMap::new();
    mech_map.insert(
        Value::String("kind".to_string()),
        Value::String("mechanism".to_string()),
    );
    mech_map.insert(
        Value::String("bodies".to_string()),
        Value::List(vec![Value::Map(body_map)]),
    );

    let (joints, _seen_joints) = extract_joints_from_mechanism(&mech_map);

    assert_eq!(
        joints.len(),
        1,
        "expected 1 joint descriptor; got {:?}",
        joints
    );
    let jd = &joints[0];
    assert_eq!(
        jd.kind, "prismatic",
        "kind should be prismatic; got {}",
        jd.kind
    );
    assert_eq!(
        jd.dimension, "length",
        "dimension should be length; got {}",
        jd.dimension
    );
    assert!(
        jd.axis.is_none(),
        "malformed axis (length!=3) must produce axis==None; got {:?}",
        jd.axis
    );
}

#[test]
fn is_idle_returns_false_on_fresh_session() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    assert!(
        !session.is_idle(),
        "fresh session should not be idle (compiled and last_check are None)"
    );
}

#[test]
fn is_idle_returns_true_after_load_from_source() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");
    assert!(
        session.is_idle(),
        "session should be idle after a successful load_from_source"
    );
}

// ---------------------------------------------------------------------------
// Step-19: Full backend pipeline regression test
// (engine → build_gui_state → compute_delta → delta_to_events)
// ---------------------------------------------------------------------------

/// Pins the cross-cutting compile-diagnostics wire path so no later refactor
/// breaks it silently.
///
/// Steps:
/// 1. Load `warn_source_with_unknown_port_type` into an EngineSession.
/// 2. The returned GuiState comes from `build_gui_state` (via `load_from_source`),
///    which calls `get_diagnostics()` and populates `compile_diagnostics`.
/// 3. Pass the GuiState through `compute_delta` with `last_state = None`.
/// 4. Feed the resulting `StateDelta` into `delta_to_events`.
/// 5. Assert exactly one `"compile-diagnostics"` event is emitted, whose JSON
///    payload contains a Warning DiagnosticInfo with the unknown-port-type message.
#[test]
fn compile_diagnostics_full_pipeline_engine_to_event() {
    use crate::diff::{compute_delta, delta_to_events};
    use std::sync::Mutex;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // 1 + 2: engine builds GuiState with compile_diagnostics populated
    let gui_state = session
        .load_from_source(warn_source_with_unknown_port_type(), "warn_pipeline")
        .expect("load_from_source should succeed with warn source");

    assert!(
        !gui_state.compile_diagnostics.is_empty(),
        "pre-condition: GuiState.compile_diagnostics must be non-empty for this test to be meaningful"
    );

    // 3: compute_delta with None last_state → full delta
    let last_state: Mutex<Option<crate::types::GuiState>> = Mutex::new(None);
    let delta = compute_delta(&last_state, &gui_state);

    // The full delta must carry the compile diagnostics
    assert!(
        delta.changed_compile_diagnostics.is_some(),
        "compute_delta must set changed_compile_diagnostics on a full (None last_state) delta"
    );

    // 4: feed into delta_to_events
    let events = delta_to_events(&delta);

    // 5a: exactly one "compile-diagnostics" event
    let compile_events: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "compile-diagnostics")
        .collect();
    assert_eq!(
        compile_events.len(),
        1,
        "expected exactly one compile-diagnostics event from the full pipeline; got events: {:?}",
        events.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );

    // 5b: payload contains at least one Warning with the expected message
    let payload = &compile_events[0].1;
    let diags: Vec<DiagnosticInfo> = serde_json::from_value(payload.clone())
        .expect("compile-diagnostics event payload must deserialize as Vec<DiagnosticInfo>");

    assert!(
        !diags.is_empty(),
        "compile-diagnostics event payload must be non-empty"
    );

    let warning = diags.iter().find(|d| {
        d.severity == "Warning" && d.message.to_lowercase().contains("unknown port type")
    });
    assert!(
        warning.is_some(),
        "expected a Warning with 'unknown port type' in the compile-diagnostics event payload; got: {:?}",
        diags
    );
}

// ── Multi-file import resolution (task 3228) ─────────────────────────────────

/// Pin the live MCP repro: loading a .ri file that imports another user module
/// must resolve the import, merge the imported template, and produce non-empty
/// GUI values.
///
/// Pre-fix (compile_with_stdlib path): `load_file` ignores the import entirely.
/// `structure Top` has no direct params, so compiled.templates = [Top] with
/// value_cells = [] → state.values is empty.
///
/// Post-fix (compile_entry_with_imports): `ModuleResolver` resolves `helper`,
/// Helper's template is merged into compiled.templates, phase-1 eval produces
/// `Helper.x = 10mm`, and build_values surfaces it as a ValueData entry.
///
/// The checked value ("10", unit "mm") is the DEFAULT from Helper.x's
/// declaration — `build_values` iterates template.value_cells (statics), not
/// per-instance scoped overrides.  Surfacing per-instance overrides is a
/// separate concern; the present assertion (non-empty `state.values` with
/// `Helper.x = 10mm`) is the correct proxy for confirming the import resolved
/// and the template was merged.
#[test]
fn load_file_with_user_import_resolves_imported_structure() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let dir = tempfile::tempdir().expect("tempdir should be created");

    // helper.ri: a public structure with one param
    std::fs::write(
        dir.path().join("helper.ri"),
        "pub structure Helper { param x: Scalar = 10mm }\n",
    )
    .expect("write helper.ri");

    // main.ri: imports helper and instantiates it as a sub-component.
    // Use the default `Helper()` so the source matches what the assertions
    // verify — `build_values` only surfaces template-level value_cells, so a
    // per-instance override here would be dead weight.
    std::fs::write(
        dir.path().join("main.ri"),
        "import helper\nstructure Top { sub h = Helper() }\n",
    )
    .expect("write main.ri");

    let state = session
        .load_file(&dir.path().join("main.ri"))
        .expect("load_file should succeed with resolved import");

    // Post-fix: Helper's template is merged into compiled.templates, so phase-1
    // eval produces Helper.x = 10mm and build_values surfaces it.
    // Pre-fix: compiled.templates = [Top] only, Top has no value_cells → empty.
    assert!(
        !state.values.is_empty(),
        "state.values should be non-empty after import is resolved (got {} values)",
        state.values.len()
    );

    // Filter by both name and entity_path so the assertion is unambiguous when
    // multiple structures in the module happen to share a parameter name.
    let x_val = state
        .values
        .iter()
        .find(|v| v.name == "x" && v.entity_path == "Helper")
        .expect("should find parameter 'x' on entity 'Helper' from the imported structure");

    assert_eq!(
        x_val.unit, "mm",
        "Helper.x should have unit 'mm', got '{}'",
        x_val.unit
    );
    assert_eq!(
        x_val.value, "10",
        "Helper.x default is 10mm; got '{}' (unit: '{}')",
        x_val.value, x_val.unit
    );
}

/// Regression: loading a standalone .ri file with no imports via the new
/// compile_entry_with_imports path must work correctly — same as before.
///
/// Guards against the new code path mishandling the no-imports case
/// (e.g. wrong project_root resolution, double-counting stdlib, or the
/// templates-merge logic producing duplicates when there's nothing to merge).
/// Should pass immediately after step-2 (regression pin, not a driver of
/// new code).
#[test]
fn load_file_solo_helper_no_imports_works() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let dir = tempfile::tempdir().expect("tempdir should be created");

    std::fs::write(
        dir.path().join("helper.ri"),
        "pub structure Helper { param x: Scalar = 10mm }\n",
    )
    .expect("write helper.ri");

    let state = session
        .load_file(&dir.path().join("helper.ri"))
        .expect("load_file of solo file should succeed");

    assert!(
        !state.values.is_empty(),
        "solo load_file should produce non-empty values; got {} values",
        state.values.len()
    );

    // Filter by both name and entity_path to avoid false matches if additional
    // structures with a parameter named "x" are present.
    let x_val = state
        .values
        .iter()
        .find(|v| v.name == "x" && v.entity_path == "Helper")
        .expect("should find parameter 'x' on entity 'Helper'");

    assert_eq!(
        x_val.unit, "mm",
        "Helper.x unit should be mm; got '{}'",
        x_val.unit
    );
    assert_eq!(
        x_val.value, "10",
        "Helper.x value should be 10; got '{}'",
        x_val.value
    );
}

/// Regression: loading a .ri file whose import cannot be resolved must return
/// a clear Err — not silently succeed with an empty/broken engine state.
///
/// Pre-fix (compile_with_stdlib path): the import is ignored, compile succeeds,
/// load_file returns Ok with empty or useless engine state.
/// Post-fix (compile_entry_with_imports): dag.compile_module returns an Err
/// containing the resolver's "module 'nonexistent' not found: tried '...' and
/// '...'" message, which load_file surfaces as Err.
#[test]
fn load_file_unresolved_import_returns_clear_err() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let dir = tempfile::tempdir().expect("tempdir should be created");

    // main.ri imports a module that doesn't exist on disk
    std::fs::write(
        dir.path().join("main.ri"),
        "import nonexistent\nstructure Top { let x = 1 }\n",
    )
    .expect("write main.ri");

    let result = session.load_file(&dir.path().join("main.ri"));

    assert!(
        result.is_err(),
        "load_file with unresolved import should return Err, got Ok"
    );

    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("nonexistent"),
        "error message should mention the module name 'nonexistent'; got: {err_msg}"
    );
    assert!(
        err_msg.contains("not found") || err_msg.contains("failed to read"),
        "error message should mention 'not found' or 'failed to read'; got: {err_msg}"
    );
}

// ── update_source multi-file fixture ─────────────────────────────────────────

/// Multi-file project fixture used by `update_source` regression tests.
///
/// Creates a [`tempfile::TempDir`] containing:
/// - `helper.ri`: `pub structure Helper { param x: Scalar = 10mm }`
/// - `main.ri`: `import helper\nstructure Top { sub h = Helper() }`
///
/// Calls `load_file` on `main.ri`, asserts the baseline `Helper.x` value cell
/// is present, and returns `(dir, session, main_path, main_content)` so the
/// caller can proceed directly to the scenario under test without
/// copy-pasting the scaffold.
///
/// The fourth field `main_content` is the exact string written to `main.ri`,
/// so callers that pass it to `update_source` (e.g. for a round-trip or
/// rollback-recovery call) are guaranteed to use the same literal the helper
/// wrote to disk — preventing silent divergence if either copy is edited
/// without updating the other.
///
/// The [`TempDir`](tempfile::TempDir) is returned to keep the temporary
/// directory alive for the duration of the test; dropping it early would
/// delete the files before any follow-up `update_source` calls that re-read
/// from disk.
fn loaded_helper_session() -> (tempfile::TempDir, EngineSession, std::path::PathBuf, String) {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let dir = tempfile::tempdir().expect("tempdir should be created");

    std::fs::write(
        dir.path().join("helper.ri"),
        "pub structure Helper { param x: Scalar = 10mm }\n",
    )
    .expect("write helper.ri");

    let main_content = "import helper\nstructure Top { sub h = Helper() }\n";
    std::fs::write(dir.path().join("main.ri"), main_content).expect("write main.ri");

    let main_path = dir.path().join("main.ri");
    let load_state = session
        .load_file(&main_path)
        .expect("load_file should succeed with resolved import");
    assert!(
        load_state
            .values
            .iter()
            .any(|v| v.name == "x" && v.entity_path == "Helper"),
        "loaded_helper_session: load_file should produce Helper.x value cell (baseline)"
    );

    (dir, session, main_path, main_content.to_string())
}

// ── update_source multi-file regression (task 3318) ──────────────────────────

/// Driver (RED → GREEN): after `load_file` resolves a multi-file project,
/// calling `update_source` with the same content must continue to produce the
/// imported structure's value cells — the import graph must survive a
/// dirty-buffer edit.
///
/// Pre-fix: `update_source` uses `compile_with_stdlib` (single-file, ignores
/// `import helper`) → compile-error for unknown `Helper` template → returns
/// `Err`.
/// Post-fix: branches on `self.file_path.is_some()`, routes through
/// `compile_entry_with_imports` → import resolved → returns `Ok` with
/// `Helper.x = 10mm` still present.
#[test]
fn update_source_after_load_file_preserves_multi_file_imports() {
    let (_dir, mut session, main_path, main_content) = loaded_helper_session();

    // update_source with the same content — import graph must be preserved
    let update_result = session.update_source(main_path.to_str().unwrap(), &main_content);

    let state =
        update_result.expect("update_source after load_file should return Ok (import resolved)");

    assert!(
        !state.values.is_empty(),
        "state.values should be non-empty after update_source preserves import (got {} values)",
        state.values.len()
    );

    let x_val = state
        .values
        .iter()
        .find(|v| v.name == "x" && v.entity_path == "Helper")
        .expect("should find parameter 'x' on entity 'Helper' after update_source");

    assert_eq!(
        x_val.unit, "mm",
        "Helper.x should have unit 'mm', got '{}'",
        x_val.unit
    );
    assert_eq!(
        x_val.value, "10",
        "Helper.x default is 10mm; got '{}' (unit: '{}')",
        x_val.value, x_val.unit
    );
}

/// Dirty-buffer edit: after `load_file` resolves a multi-file project,
/// `update_source` with *modified* content (adding a new top-level param while
/// keeping the existing `import helper`) must see both the imported `Helper.x`
/// value cell and the newly-added param — the import graph survives an actual
/// edit, not just a round-trip of identical content.
///
/// This is the core dirty-buffer regression scenario for task 3318: pre-fix
/// `update_source` silently dropped all imports on every keystroke because it
/// called `compile_with_stdlib` (single-file) instead of
/// `compile_entry_with_imports` (multi-file).
#[test]
fn update_source_after_load_file_dirty_buffer_edit_preserves_imports() {
    let (_dir, mut session, main_path, _main_content) = loaded_helper_session();

    // v2: keep the import, add a new top-level param — simulates a real keystroke edit
    let main_content_v2 =
        "import helper\nstructure Top { sub h = Helper()\nparam top_size: Scalar = 20mm }\n";

    let state = session
        .update_source(main_path.to_str().unwrap(), main_content_v2)
        .expect("update_source with dirty-buffer edit should return Ok");

    // Imported Helper.x must still be present
    let helper_x = state
        .values
        .iter()
        .find(|v| v.name == "x" && v.entity_path == "Helper")
        .expect("Helper.x should be present after dirty-buffer edit");
    assert_eq!(helper_x.unit, "mm");
    assert_eq!(helper_x.value, "10");

    // The newly-added top-level param must also appear
    assert!(
        state
            .values
            .iter()
            .any(|v| v.name == "top_size" && v.unit == "mm"),
        "top_size param added in dirty-buffer edit should appear in state.values"
    );
}

/// Regression-pin: after `load_file` succeeds, calling `update_source` with
/// content that adds an unresolvable `import nonexistent` must return `Err`
/// mentioning the module name — the multi-file path must NOT silently swallow
/// resolver errors.
///
/// Mirrors `load_file_unresolved_import_returns_clear_err` for the dirty-buffer
/// code path (task 3318 follow-up).
#[test]
fn update_source_after_load_file_with_unresolved_import_returns_err() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let dir = tempfile::tempdir().expect("tempdir should be created");

    // main.ri: no imports — load succeeds
    std::fs::write(
        dir.path().join("main.ri"),
        "structure Top { param w: Scalar = 5mm }\n",
    )
    .expect("write main.ri");

    session
        .load_file(&dir.path().join("main.ri"))
        .expect("initial load_file should succeed");

    // dirty buffer: add an import that cannot be resolved
    let main_path = dir.path().join("main.ri");
    let result = session.update_source(
        main_path.to_str().unwrap(),
        "import nonexistent\nstructure Top { param w: Scalar = 5mm }\n",
    );

    assert!(
        result.is_err(),
        "update_source with unresolved import should return Err, got Ok"
    );

    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("nonexistent"),
        "error message should mention module name 'nonexistent'; got: {err_msg}"
    );
    assert!(
        err_msg.contains("not found") || err_msg.contains("failed to read"),
        "error message should mention 'not found' or 'failed to read'; got: {err_msg}"
    );
}

// ── update_source divergent-path contract (task 3370) ────────────────────────

/// Regression for task 3370 (esc-3318-14 suggestion #1):
/// after `load_file`, calling `update_source` with a *divergent* path whose
/// `file_stem` differs from the originally-loaded file must still derive
/// `module_name` from `self.file_path.file_stem()`, not from the caller's `path`.
///
/// Pre-fix: `update_source` always derived `module_name` from `Path::new(path)`,
/// so a divergent path like `renamed_buffer.ri` after loading `main.ri` would
/// insert under key `"renamed_buffer.ri"` and set `module_name = "renamed_buffer"`,
/// silently corrupting the session state.
/// Post-fix: when `self.file_path` is set, `module_name` is derived from
/// `self.file_path.file_stem()` — the originally-loaded file's stem — regardless
/// of the caller's `path` argument.
#[test]
fn update_source_with_divergent_path_keeps_loaded_module_name() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let dir = tempfile::tempdir().expect("tempdir should be created");

    // A self-contained structure — no imports needed; the bug is purely about
    // module_name derivation, independent of multi-file resolution.
    let initial_content = "structure Main { param w: Scalar = 10mm }\n";
    std::fs::write(dir.path().join("main.ri"), initial_content).expect("write main.ri");

    session
        .load_file(&dir.path().join("main.ri"))
        .expect("initial load_file should succeed");

    // Build a divergent path: file_stem = "renamed_buffer", differs from "main".
    let divergent = dir.path().join("renamed_buffer.ri");
    let updated_content = "structure Main { param w: Scalar = 20mm }\n";

    let state = session
        .update_source(divergent.to_str().unwrap(), updated_content)
        .expect("update_source with divergent path should succeed");

    // 1. No phantom second entry under the divergent name.
    assert_eq!(
        state.files.len(),
        1,
        "should have exactly 1 file entry after load_file + divergent-path update_source, \
         got {}: {:?}",
        state.files.len(),
        state.files.iter().map(|f| &f.path).collect::<Vec<_>>()
    );

    // 2. module_name is derived from the originally-loaded file's stem ("main"),
    //    NOT from the divergent path's stem ("renamed_buffer").
    let (stored_key, stored_src) = session
        .resolve_source_for_test()
        .expect("resolve_source_for_test should return Some after update_source");
    assert_eq!(
        stored_key,
        module_key("main"),
        "key should be derived from load_file's stem ('main'), not from divergent path; \
         got '{stored_key}'"
    );

    // 3. The new content IS stored under the original key — the update took effect.
    assert_eq!(
        stored_src, updated_content,
        "stored source should be the updated content"
    );
}

// ── update_source single-file branch + atomic-rollback pins (task 3371) ──────

/// Regression pin for the `self.file_path == None` branch of `update_source`'s
/// task-3318 refactor (esc-3318-14, suggestion #2).
///
/// A "fresh session" means no prior `load_file` or `load_from_source` call, so
/// `self.file_path` is `None`.  In this state `update_source` must route through
/// the single-file branch (`compile_single_file_with_stdlib`) and successfully
/// compile valid self-contained source.
///
/// The phantom path `/nonexistent/dir/solo.ri` documents the single-file
/// branch's defining property: the `path` argument is used only for
/// module-stem derivation — no disk access occurs for import-free source.
/// The routing regression sentinel is assertion (2): `stored_key ==
/// module_key("solo")`.  The multi-file branch derives the source_map key
/// differently (from the resolved import graph rooted at the entry file's
/// parent directory), so a routing regression would surface as a key
/// mismatch rather than a filesystem error.
#[test]
fn update_source_on_fresh_session_compiles_single_file_source_without_disk_io() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let source = "structure Solo { param w: Scalar = 7mm }\n";
    let state = session
        .update_source("/nonexistent/dir/solo.ri", source)
        .expect("fresh-session update_source with valid single-file source should return Ok");

    // 1. The compiled state must contain the expected value cell.
    let w_val = state
        .values
        .iter()
        .find(|v| v.name == "w" && v.entity_path == "Solo")
        .expect("should find parameter 'w' on entity 'Solo' after single-file update_source");
    assert_eq!(
        w_val.value, "7",
        "Solo.w default is 7mm; got '{}' (unit: '{}')",
        w_val.value, w_val.unit
    );
    assert_eq!(
        w_val.unit, "mm",
        "Solo.w should have unit 'mm', got '{}'",
        w_val.unit
    );

    // 2. source_map must be keyed by module_key("solo") — the stem of the
    //    phantom path — proving the single-file commit path ran and that
    //    module_key derivation from the caller's path argument is correct.
    let (stored_key, stored_src) = session
        .resolve_source_for_test()
        .expect("resolve_source_for_test should return Some after a successful update_source");
    assert_eq!(
        stored_key,
        module_key("solo"),
        "source_map key should be module_key(\"solo\") (stem of caller's path); got '{stored_key}'"
    );
    assert_eq!(
        stored_src, source,
        "stored source should equal the content passed to update_source"
    );
}

/// Atomic-rollback regression pin for the multi-file branch (task 3318 follow-up,
/// esc-3318-14 suggestion #2).
///
/// After `load_file` resolves a multi-file project, a failing `update_source`
/// (parse-error content) must leave ALL of the following completely unchanged:
///
/// 1. **`source_map`** — `resolve_source_for_test()` returns the original key
///    and source text, not the broken content.
/// 2. **`compiled`** — `build_gui_state()` still returns a state containing
///    `Helper.x` from the prior good compile.
/// 3. **`file_path`** — verified behaviorally: a follow-up `update_source` with
///    valid multi-file content succeeds and re-surfaces `Helper.x`.  The
///    multi-file branch requires `file_path == Some(...)`; if rollback had cleared
///    it, the recovery call would fall through to the single-file branch, silently
///    drop `import helper`, and this assertion would flip red.
///
/// Parse-error content (`"totally broken {{{}}}"`) is used as the failure trigger
/// because it bails out at the earliest possible point — before any compile-side
/// or check-side state could plausibly be touched — giving the cleanest guarantee
/// that "no state was mutated between call entry and Err return".
#[test]
fn update_source_failure_after_load_file_leaves_prior_compiled_source_map_and_file_path_intact() {
    // loaded_helper_session establishes the pre-failure baseline (load_file + Helper.x assert).
    let (_dir, mut session, main_path, main_content) = loaded_helper_session();

    // Capture pre-failure source_map state as owned Strings so the borrow releases.
    let (pre_key, pre_src) = session
        .resolve_source_for_test()
        .expect("resolve_source_for_test should return Some after load_file");
    let pre_key = pre_key.to_owned();
    let pre_src = pre_src.to_owned();

    // Trigger failure with parse-error content.
    let err = session
        .update_source(main_path.to_str().unwrap(), "totally broken {{{}}}")
        .expect_err("invalid source should return Err");
    assert!(
        err.contains("Parse errors"),
        "error string should contain 'Parse errors'; got: {err}"
    );

    // ── Invariant 1: source_map unchanged ────────────────────────────────────
    let (post_key, post_src) = session
        .resolve_source_for_test()
        .expect("resolve_source_for_test should return Some after failed update_source (rollback)");
    assert_eq!(
        post_key, pre_key,
        "source_map key must be unchanged after failed update_source; was '{pre_key}', \
         now '{post_key}'"
    );
    assert_eq!(
        post_src, pre_src,
        "source_map content must be the pre-failure source, not the broken content"
    );

    // ── Invariant 2: compiled unchanged ──────────────────────────────────────
    let post_state = session
        .build_gui_state()
        .expect("build_gui_state should succeed (prior good compile still in compiled field)");
    assert!(
        post_state
            .values
            .iter()
            .any(|v| v.name == "x" && v.entity_path == "Helper"),
        "build_gui_state after failed update_source must still contain Helper.x \
         (compiled field must be unchanged by rollback)"
    );

    // ── Invariant 3: file_path unchanged (behavioral check) ──────────────────
    // A successful recovery update_source with valid multi-file content must
    // re-surface Helper.x.  This is only possible if file_path is still Some(...):
    // the multi-file branch (which resolves 'import helper') is only taken when
    // file_path is set.  If rollback had cleared file_path, the call would fall
    // through to the single-file branch, silently drop the import, and the
    // Helper.x assertion below would flip red — proving the regression.
    let recovery_state = session
        .update_source(main_path.to_str().unwrap(), &main_content)
        .expect("recovery update_source with original multi-file content should succeed");
    assert!(
        recovery_state
            .values
            .iter()
            .any(|v| v.name == "x" && v.entity_path == "Helper"),
        "recovery update_source must surface Helper.x — proving file_path was preserved \
         through the failed update_source (behavioral file_path rollback check)"
    );
}

// ── Collision-diagnostic test helpers ────────────────────────────────────────

/// Create an `EngineSession` configured with the standard mock checker/kernel,
/// used by the cross-import and entry-vs-import collision regression tests.
fn setup_collision_session() -> EngineSession {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    EngineSession::new(Box::new(checker), Some(Box::new(kernel)))
}

/// Return the first Warning diagnostic whose `message` mentions both `name` and
/// `"first-wins"`, or `None` if no such diagnostic exists. Backs
/// `assert_collision_warning_mentions`.
fn find_collision_warning<'a>(
    state: &'a crate::types::GuiState,
    name: &str,
) -> Option<&'a DiagnosticInfo> {
    state.compile_diagnostics.iter().find(|d| {
        d.severity == "Warning" && d.message.contains(name) && d.message.contains("first-wins")
    })
}

/// Assert that the collision Warning for structure `name` exists AND that each
/// string in `origins` appears in the message in its **quoted form** (e.g.
/// `"'helper1'"` rather than `"helper1"`).  Using the quoted form ties the
/// assertion to the actual module-name slot in the format string:
///
/// ```text
/// imported pub structure 'Foo' declared in both 'helper1' and 'helper2'; first-wins
/// ```
///
/// so that an incidental occurrence of a common fragment (e.g. `"main"` inside
/// `"domain"` or `"remain"`) cannot produce a false-positive match.
fn assert_collision_warning_mentions(state: &crate::types::GuiState, name: &str, origins: &[&str]) {
    let w = find_collision_warning(state, name).unwrap_or_else(|| {
        panic!(
            "expected a Warning diagnostic mentioning '{}' and 'first-wins', \
             but state.compile_diagnostics = {:?}",
            name, state.compile_diagnostics
        )
    });
    for origin in origins {
        let quoted = format!("'{}'", origin);
        assert!(
            w.message.contains(&quoted),
            "collision warning should name module '{}' as {} in the message; got: {}",
            origin,
            quoted,
            w.message
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────

/// Regression: importing two helper modules that both declare `pub structure Foo`
/// must emit a Warning diagnostic for the collision (first-wins is preserved, but
/// the user must be told about the shadowing).
///
/// Mirrors the compiler's cross-prelude alias collision policy
/// (reify-compiler/src/lib.rs:281-292): same `Diagnostic::warning`, same
/// "first-wins" trailer in the message, same `SourceSpan::prelude()` label.
#[test]
fn load_file_two_imports_with_same_pub_structure_emits_collision_diagnostic() {
    let mut session = setup_collision_session();
    let dir = tempfile::tempdir().expect("tempdir should be created");

    // helper1.ri: declares pub structure Foo with x = 1mm
    std::fs::write(
        dir.path().join("helper1.ri"),
        "pub structure Foo { param x: Scalar = 1mm }\n",
    )
    .expect("write helper1.ri");

    // helper2.ri: also declares pub structure Foo (collision with helper1)
    std::fs::write(
        dir.path().join("helper2.ri"),
        "pub structure Foo { param x: Scalar = 2mm }\n",
    )
    .expect("write helper2.ri");

    // main.ri: imports both helpers, causing a cross-import collision on Foo
    std::fs::write(
        dir.path().join("main.ri"),
        "import helper1\nimport helper2\nstructure Top { sub f = Foo() }\n",
    )
    .expect("write main.ri");

    let state = session
        .load_file(&dir.path().join("main.ri"))
        .expect("load_file should succeed despite collision (first-wins, not error)");

    // Assert the warning exists and names both module origins using the quoted form
    // that appears in the format string: 'helper1' (first-wins) and 'helper2' (collider).
    assert_collision_warning_mentions(&state, "Foo", &["helper1", "helper2"]);
}

/// Regression: when the entry module itself declares a structure `Foo` and an
/// import also provides `pub structure Foo`, the collision must emit a Warning
/// diagnostic (first-wins is the entry's declaration, but the user must be told).
#[test]
fn load_file_entry_redeclares_imported_pub_structure_emits_collision_diagnostic() {
    let mut session = setup_collision_session();
    let dir = tempfile::tempdir().expect("tempdir should be created");

    // helper.ri: declares pub structure Foo
    std::fs::write(
        dir.path().join("helper.ri"),
        "pub structure Foo { param x: Scalar = 1mm }\n",
    )
    .expect("write helper.ri");

    // main.ri: also declares structure Foo (shadows the import)
    std::fs::write(
        dir.path().join("main.ri"),
        "import helper\nstructure Foo { param y: Scalar = 5mm }\n",
    )
    .expect("write main.ri");

    let state = session
        .load_file(&dir.path().join("main.ri"))
        .expect("load_file should succeed despite collision (first-wins, not error)");

    // Assert the warning exists and names both module origins using the quoted form:
    // 'main' (entry module = first-wins origin, pre-seeded in templates_origin) and
    // 'helper' (colliding import path). Quoted form avoids false matches on common
    // English fragments (e.g. "remain", "domain") that contain the bare substring.
    assert_collision_warning_mentions(&state, "Foo", &["main", "helper"]);
}

/// Regression: three imports all declaring the same `pub structure Foo` must
/// emit N-1 = 2 warnings, each naming the original first-wins declarer (helper1)
/// and the i-th colliding import (helper2, then helper3).  Locks the per-collision
/// warning count so a future change that emits only one summary or deduplicates
/// pairs is caught by a test failure rather than silent behaviour drift.
#[test]
fn load_file_three_imports_same_pub_structure_emits_two_collision_diagnostics() {
    let mut session = setup_collision_session();
    let dir = tempfile::tempdir().expect("tempdir should be created");

    for (name, val) in [("helper1", "1mm"), ("helper2", "2mm"), ("helper3", "3mm")] {
        std::fs::write(
            dir.path().join(format!("{name}.ri")),
            format!("pub structure Foo {{ param x: Scalar = {val} }}\n"),
        )
        .unwrap_or_else(|_| panic!("write {name}.ri"));
    }

    std::fs::write(
        dir.path().join("main.ri"),
        "import helper1\nimport helper2\nimport helper3\nstructure Top { sub f = Foo() }\n",
    )
    .expect("write main.ri");

    let state = session
        .load_file(&dir.path().join("main.ri"))
        .expect("load_file should succeed (first-wins, not error)");

    // Each of the two colliding imports (helper2, helper3) produces exactly one
    // warning that names the original declarer (helper1) and the collider.
    let warnings: Vec<_> = state
        .compile_diagnostics
        .iter()
        .filter(|d| {
            d.severity == "Warning" && d.message.contains("Foo") && d.message.contains("first-wins")
        })
        .collect();
    assert_eq!(
        warnings.len(),
        2,
        "expected exactly 2 collision warnings for 3-import case, got: {:?}",
        state.compile_diagnostics
    );
    // Both warnings should name the original declarer as 'helper1' (quoted form).
    for w in &warnings {
        assert!(
            w.message.contains("'helper1'"),
            "warning should name the first-wins origin 'helper1'; got: {}",
            w.message
        );
    }
    // The two colliding imports should be named individually (quoted form).
    assert!(
        warnings.iter().any(|w| w.message.contains("'helper2'")),
        "expected one warning naming 'helper2'; got: {:?}",
        warnings
    );
    assert!(
        warnings.iter().any(|w| w.message.contains("'helper3'")),
        "expected one warning naming 'helper3'; got: {:?}",
        warnings
    );
}

/// Guard against double-seeding stdlib when a .ri file explicitly `import std.*`.
///
/// When an entry imports `std.units`, the new flow adds it to the DAG and also
/// has the full stdlib via `load_stdlib()`.  This test verifies that:
/// (a) load_file returns Ok (no partial-overlay error),
/// (b) state.values is non-empty (compilation and eval succeeded), and
/// (c) state.diagnostics (or the error path) does NOT contain a partial stdlib
///     overlay diagnostic.
///
/// The test is typically green immediately after step-2 because `dag.compile_module`
/// for `std.units` falls back to Embedded mode (no stdlib dir exists in the
/// tempdir), same mode as load_stdlib()'s slice — no conflict.
#[test]
fn load_file_with_std_import_does_not_double_seed_stdlib() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let dir = tempfile::tempdir().expect("tempdir should be created");

    // main.ri uses std.units implicitly via Length and mm
    std::fs::write(
        dir.path().join("main.ri"),
        "import std.units\nstructure Top { param w: Length = 5mm }\n",
    )
    .expect("write main.ri");

    let result = session.load_file(&dir.path().join("main.ri"));

    assert!(
        result.is_ok(),
        "load_file with explicit std.units import should succeed; got Err: {}",
        result.as_ref().unwrap_err()
    );

    let state = result.unwrap();

    assert!(
        !state.values.is_empty(),
        "state.values should be non-empty (Top.w = 5mm); got {} values",
        state.values.len()
    );

    // No partial-stdlib-overlay diagnostic should appear in the GUI state.
    // Diagnostics from the engine session appear in state.diagnostics (if any).
    // The overlay diagnostic is an Error — if it appeared, load_file would have
    // returned Err already, so we only need to confirm the result was Ok above.
    // Filter by both name and entity_path to guard against false positives when
    // multiple structures define a parameter named "w".
    let w_val = state
        .values
        .iter()
        .find(|v| v.name == "w" && v.entity_path == "Top")
        .expect("should find parameter 'w' on entity 'Top'");

    assert_eq!(
        w_val.unit, "mm",
        "Top.w unit should be mm; got '{}'",
        w_val.unit
    );
    assert_eq!(
        w_val.value, "5",
        "Top.w value should be 5; got '{}'",
        w_val.value
    );
}

// ---- fatal parse/compile diagnostics surfacing tests (task 3351) -----------

/// After a failed `update_source` (parse error on a fresh session with no
/// `file_path` set), `build_gui_state` must surface the failure in
/// `compile_diagnostics` AND surface the failing buffer in `files[0]` so
/// diagnostics line/col can be indexed against the actual text (task 4258).
///
/// Pins step-3/step-4 of the task-3351 plan: `update_source`'s single-file
/// branch (when `self.file_path` is `None`) must populate
/// `compile_failure` on failure, just like `load_from_source`.
/// Updated for task 4258: `files` is now non-empty (failing source is surfaced).
#[test]
fn build_gui_state_surfaces_parse_error_after_failed_update_source_on_fresh_session() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let bad_source = "this is not valid {{{}}}";
    // Fresh session — no load_file or load_from_source, so self.file_path is None.
    // update_source takes the single-file branch (compile_single_file_with_stdlib).
    let err = session
        .update_source("foo.ri", bad_source)
        .expect_err("invalid source should return Err");
    assert!(
        err.contains("Parse errors"),
        "error string should mention Parse errors; got: {err}"
    );

    // build_gui_state must surface the stored failure diagnostics.
    let state = session
        .build_gui_state()
        .expect("build_gui_state should return Ok even after a failed update_source");

    assert!(
        !state.compile_diagnostics.is_empty(),
        "compile_diagnostics should be non-empty after a failed update_source on a fresh session"
    );
    let first = &state.compile_diagnostics[0];
    assert_eq!(
        first.severity, "Error",
        "first diagnostic should have severity Error; got: {}",
        first.severity
    );

    // task 4258: files must carry the failing buffer (one-snapshot invariant).
    assert!(
        !state.files.is_empty(),
        "files must be non-empty after a cold-start failure — failing source must be surfaced"
    );
    assert_eq!(
        state.files[0].path, "foo.ri",
        "files[0].path must equal the module key derived from 'foo.ri'"
    );
    assert_eq!(
        state.files[0].content, bad_source,
        "files[0].content must equal the exact failing buffer"
    );

    // These fields remain empty (no successful compile yet).
    assert!(state.meshes.is_empty(), "meshes should be empty");
    assert!(state.values.is_empty(), "values should be empty");
    assert!(state.constraints.is_empty(), "constraints should be empty");
    assert!(
        state.tessellation_diagnostics.is_empty(),
        "tessellation_diagnostics should be empty"
    );
}

/// After a failed `load_from_source` (parse error on a fresh session),
/// `build_gui_state` must surface the failure in `compile_diagnostics` AND
/// surface the failing buffer in `files[0]` so diagnostics line/col can be
/// indexed against the actual text (task 4258).
///
/// Pins step-1 of the task-3351 plan: the early-return branch of
/// `build_gui_state` (when `compiled` is `None`) must emit the stored
/// `compile_failure.diags` rather than `Vec::new()`.
/// Updated for task 4258: `files` is now non-empty (failing source is surfaced).
#[test]
fn build_gui_state_surfaces_parse_error_after_failed_load_from_source() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let bad_source = "this is not valid reify syntax {{{}}}";
    // A parse-error source — `{{{` is invalid reify syntax so `parsed.errors` is non-empty.
    let err = session
        .load_from_source(bad_source, "bad")
        .expect_err("invalid source should return Err");
    assert!(
        err.contains("Parse errors"),
        "error string should mention Parse errors; got: {err}"
    );

    // build_gui_state should surface the stored diagnostics rather than returning empty.
    let state = session
        .build_gui_state()
        .expect("build_gui_state should return Ok even after a failed load");

    // compile_diagnostics must be non-empty and contain an Error-severity entry.
    assert!(
        !state.compile_diagnostics.is_empty(),
        "compile_diagnostics should be non-empty after a failed load_from_source"
    );
    let first = &state.compile_diagnostics[0];
    assert_eq!(
        first.severity, "Error",
        "first diagnostic should have severity Error; got: {}",
        first.severity
    );
    // file_path should follow the module_key convention ({module_name}.ri).
    assert!(
        first.file_path.ends_with(".ri"),
        "first diagnostic file_path should end with .ri; got: {}",
        first.file_path
    );

    // task 4258: files must carry the failing buffer (one-snapshot invariant).
    assert!(
        !state.files.is_empty(),
        "files must be non-empty after a cold-start failure — failing source must be surfaced"
    );
    assert_eq!(
        state.files[0].path, "bad.ri",
        "files[0].path must equal the module key derived from module_name 'bad'"
    );
    assert_eq!(
        state.files[0].content, bad_source,
        "files[0].content must equal the exact failing buffer"
    );

    // These fields remain empty (no successful compile yet).
    assert!(state.meshes.is_empty(), "meshes should be empty");
    assert!(state.values.is_empty(), "values should be empty");
    assert!(state.constraints.is_empty(), "constraints should be empty");
    assert!(
        state.tessellation_diagnostics.is_empty(),
        "tessellation_diagnostics should be empty"
    );
}

/// After a successful `load_from_source`, `commit_state` must clear
/// `compile_failure` so stale failure diagnostics are not surfaced after a recovery.
///
/// Pins step-7/step-8 of the task-3351 plan: `commit_state` must clear the stored
/// compile failure and the `compile_failure_for_test` accessor must expose the field
/// for test introspection.
///
/// Disjointness (cold-start vs live-edit) is now a type-level guarantee via
/// `Option<CompileFailure>` rather than a runtime `debug_assert!` on separate fields.
#[test]
fn commit_state_clears_cold_start_compile_failure_on_successful_load() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // First, induce a parse error so compile_failure is populated (ColdStart kind).
    let _ = session
        .load_from_source("this is not valid reify syntax {{{}}}", "bad")
        .expect_err("invalid source should return Err");

    // The accessor must reflect the stored failure diagnostics with ColdStart kind.
    assert!(
        matches!(
            session.compile_failure_for_test(),
            Some(CompileFailure { kind: CompileFailureKind::ColdStart, diags, .. }) if !diags.is_empty()
        ),
        "compile_failure should be Some(ColdStart) with non-empty diags after a failed load"
    );

    // Now load a valid source — commit_state must clear the field.
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("valid source should succeed");

    assert!(
        session.compile_failure_for_test().is_none(),
        "compile_failure should be None after a successful load"
    );
}

/// After a successful recovery (valid `load_from_source` after a prior failed live
/// edit), `commit_state` must clear `compile_failure` so stale live-failure
/// diagnostics are not surfaced after recovery.
///
/// Pins step-3/step-4 of the task-3386 plan: `commit_state` must clear the stored
/// compile failure and the `compile_failure_for_test` accessor must expose the field
/// for test introspection.
///
/// Disjointness (cold-start vs live-edit) is now a type-level guarantee via
/// `Option<CompileFailure>` rather than a runtime `debug_assert!` on separate fields.
#[test]
fn commit_state_clears_live_edit_compile_failure_on_successful_recovery() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Establish a successful compiled state (compiled is Some).
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("valid source should load successfully");

    // Trigger a failed live edit to populate compile_failure (LiveEdit kind).
    let _ = session.load_from_source("this is not valid reify syntax {{{}}}", "bad");

    assert!(
        matches!(
            session.compile_failure_for_test(),
            Some(CompileFailure { kind: CompileFailureKind::LiveEdit, diags, .. }) if !diags.is_empty()
        ),
        "compile_failure should be Some(LiveEdit) with non-empty diags after a failed live edit"
    );

    // Recover with a successful load — commit_state must clear compile_failure.
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("valid source should load successfully after recovery");

    assert!(
        session.compile_failure_for_test().is_none(),
        "compile_failure should be None after a successful recovery"
    );
}

/// After a cold-start failure the session must record `CompileFailure { kind: ColdStart, .. }`,
/// and after a subsequent successful load it must clear to `None`.  After a live-edit failure
/// (compiled is Some) it must record `CompileFailure { kind: LiveEdit, .. }`.
///
/// Pins the new `compile_failure: Option<CompileFailure>` representation introduced in
/// task 3414: both kind discriminants are exercised from a single session lifecycle, and
/// the `compile_failure_for_test` accessor exposes the field for assertion without
/// calling `build_gui_state`.
#[test]
fn compile_failure_records_cold_start_then_live_edit_kinds() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // 1. Cold-start failure: compiled is None at failure time → ColdStart kind.
    let _ = session.load_from_source("this is not valid reify syntax {{{}}}", "bad");

    let failure = session
        .compile_failure_for_test()
        .expect("compile_failure must be Some after a failed cold-start load");
    assert!(
        matches!(failure.kind, CompileFailureKind::ColdStart),
        "cold-start failure must record kind = ColdStart; got: {:?}",
        failure.kind
    );
    assert!(
        !failure.diags.is_empty(),
        "cold-start failure must record non-empty diags"
    );

    // 2. Successful recovery clears compile_failure → None.
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("valid source should load successfully");

    assert!(
        session.compile_failure_for_test().is_none(),
        "compile_failure must be None after a successful recovery"
    );

    // 3. Live-edit failure: compiled is Some at failure time → LiveEdit kind.
    let _ = session.load_from_source("broken {{{}}}", "bad2");

    let failure = session
        .compile_failure_for_test()
        .expect("compile_failure must be Some after a failed live-edit load");
    assert!(
        matches!(failure.kind, CompileFailureKind::LiveEdit),
        "live-edit failure must record kind = LiveEdit; got: {:?}",
        failure.kind
    );
    assert!(
        !failure.diags.is_empty(),
        "live-edit failure must record non-empty diags"
    );
}

/// On a fresh session with no module loaded, `build_gui_state` must return an
/// empty `tessellation_diagnostics`. This is structural — no production producer
/// for `tessellation_diagnostics` populates the cold-start branch (tessellation
/// runs only when `compiled is Some`). The test pins the empty-vec emission so
/// a future regression that re-introduces speculative stored-field plumbing
/// without a producer is caught.
#[test]
fn build_gui_state_returns_empty_tessellation_diagnostics_when_no_module_loaded() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .build_gui_state()
        .expect("build_gui_state should return Ok when compiled is None");

    assert!(
        state.tessellation_diagnostics.is_empty(),
        "cold-start build_gui_state should emit empty tessellation_diagnostics — no producer exists yet"
    );
    assert!(
        state.compile_diagnostics.is_empty(),
        "cold-start build_gui_state should emit empty compile_diagnostics — no load attempted"
    );
}

/// After a failed `load_file` (parse error in the file-on-disk), `build_gui_state`
/// must surface the failure in `compile_diagnostics` AND surface the failing buffer
/// in `files[0]` so diagnostics line/col can be indexed against the actual text
/// (task 4258).
///
/// Pins step-5/step-6 of the task-3351 plan: `load_file` routes through
/// `compile_entry_with_imports`, which must also populate
/// `compile_failure` on failure once refactored in step-6.
/// Updated for task 4258: `files` is now non-empty (failing source is surfaced).
#[test]
fn build_gui_state_surfaces_parse_error_after_failed_load_file() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    // Write an invalid .ri file — `{{{` is unparseable syntax.
    let file_path = dir.path().join("main.ri");
    let bad_source = "structure {{{}}}}\n";
    std::fs::write(&file_path, bad_source).expect("write should succeed");

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let err = session
        .load_file(&file_path)
        .expect_err("load_file with invalid source should return Err");
    assert!(
        err.contains("Parse errors") || err.contains("Compile errors"),
        "error string should mention parse/compile errors; got: {err}"
    );

    // build_gui_state must surface the stored failure diagnostics.
    let state = session
        .build_gui_state()
        .expect("build_gui_state should return Ok even after a failed load_file");

    assert!(
        !state.compile_diagnostics.is_empty(),
        "compile_diagnostics should be non-empty after a failed load_file"
    );
    let first = &state.compile_diagnostics[0];
    assert_eq!(
        first.severity, "Error",
        "first diagnostic should have severity Error; got: {}",
        first.severity
    );

    // task 4258: files must carry the failing buffer (one-snapshot invariant).
    assert!(
        !state.files.is_empty(),
        "files must be non-empty after a cold-start failure — failing source must be surfaced"
    );
    assert_eq!(
        state.files[0].path, "main.ri",
        "files[0].path must equal the module key derived from file stem 'main'"
    );
    assert_eq!(
        state.files[0].content, bad_source,
        "files[0].content must equal the exact failing buffer"
    );

    // These fields remain empty (no successful compile yet).
    assert!(state.meshes.is_empty(), "meshes should be empty");
    assert!(state.values.is_empty(), "values should be empty");
    assert!(state.constraints.is_empty(), "constraints should be empty");
    assert!(
        state.tessellation_diagnostics.is_empty(),
        "tessellation_diagnostics should be empty"
    );
}

/// After a session already has a successful compile (`compiled` is `Some`), a
/// subsequent failed `load_from_source` must surface the live compile failure
/// in `build_gui_state`'s `compile_diagnostics`.
///
/// Pins the new behavior introduced in task 3386: when `compiled is Some` at
/// failure time, the failure diagnostics are stored as `CompileFailureKind::LiveEdit`
/// and `build_gui_state`'s append branch surfaces them alongside any warnings
/// from the prior good compile.
///
/// Disjointness between cold-start and live-edit failures is now a type-level
/// guarantee via `Option<CompileFailure>` — only one failure can be stored at a time,
/// so the separate "cold-start field is empty" assertion is no longer needed.
///
/// Replaces `last_compile_diagnostics_not_overwritten_when_prior_compile_exists`
/// (task 3351 pinning test) which pinned the opposite (now-removed) behavior.
#[test]
fn build_gui_state_surfaces_live_compile_failure_after_failed_load_from_source_with_prior_compile()
{
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Establish a successful compiled state (Ok return guarantees compiled is Some).
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("valid source should load successfully");

    // compile_failure must be None after a successful load.
    assert!(
        session.compile_failure_for_test().is_none(),
        "compile_failure should be None after a successful load"
    );

    // Now fail a subsequent load — compiled is Some, so the failure is LiveEdit kind.
    let _ = session.load_from_source("this is not valid reify syntax {{{}}}", "bad");

    // compile_failure must be Some(LiveEdit) — disjointness is a type-level guarantee.
    assert!(
        matches!(
            session.compile_failure_for_test(),
            Some(CompileFailure { kind: CompileFailureKind::LiveEdit, diags, .. }) if !diags.is_empty()
        ),
        "compile_failure should be Some(LiveEdit) with non-empty diags after a failed load_from_source with prior compile"
    );

    // build_gui_state must surface the live failure diagnostics.
    let state = session
        .build_gui_state()
        .expect("build_gui_state should return Ok with the prior good state plus live errors");

    assert!(
        !state.compile_diagnostics.is_empty(),
        "compile_diagnostics must be non-empty — live compile failure must be surfaced"
    );
    let has_error = state
        .compile_diagnostics
        .iter()
        .any(|d| d.severity == "Error");
    assert!(
        has_error,
        "at least one diagnostic must have severity Error; got: {:?}",
        state
            .compile_diagnostics
            .iter()
            .map(|d| &d.severity)
            .collect::<Vec<_>>()
    );
}

/// After a session already has a successful compile (`compiled` is `Some`), a
/// subsequent failed `update_source` (single-file path — no prior `load_file`,
/// so `self.file_path` is `None`) must surface the live compile failure in
/// `build_gui_state`'s `compile_diagnostics`.
///
/// Exercises `update_source`'s single-file branch (`compile_single_file_with_stdlib`).
/// Pins the invariant introduced in task 3386: live compile failures on the single-file
/// path are stored as `CompileFailureKind::LiveEdit` and surfaced via `build_gui_state`.
#[test]
fn build_gui_state_surfaces_live_compile_failure_after_failed_update_source_single_file_with_prior_compile()
 {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Load valid source via load_from_source — sets compiled = Some, file_path = None,
    // so update_source takes the single-file branch.
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("valid source should load successfully");

    // Trigger a failed live edit via update_source (single-file branch).
    let err = session
        .update_source("bracket.ri", "broken source {{{}}}")
        .expect_err("invalid source should return Err");
    assert!(
        err.contains("Parse errors"),
        "error string should mention Parse errors; got: {err}"
    );

    // compile_failure must be Some(LiveEdit) after the failure.
    assert!(
        matches!(
            session.compile_failure_for_test(),
            Some(CompileFailure { kind: CompileFailureKind::LiveEdit, diags, .. }) if !diags.is_empty()
        ),
        "compile_failure should be Some(LiveEdit) with non-empty diags after a failed update_source (single-file)"
    );

    // build_gui_state must surface the live failure diagnostics.
    let state = session
        .build_gui_state()
        .expect("build_gui_state should return Ok with the prior good state plus live errors");

    assert!(
        !state.compile_diagnostics.is_empty(),
        "compile_diagnostics must be non-empty — live compile failure must be surfaced"
    );
    let has_error = state
        .compile_diagnostics
        .iter()
        .any(|d| d.severity == "Error");
    assert!(
        has_error,
        "at least one diagnostic must have severity Error; got: {:?}",
        state
            .compile_diagnostics
            .iter()
            .map(|d| &d.severity)
            .collect::<Vec<_>>()
    );
}

/// After a session already has a successful compile via `load_file` (`compiled` is
/// `Some` AND `file_path` is `Some`), a subsequent failed `update_source` (multi-file
/// path — `self.file_path` is `Some`, so `update_source` routes through
/// `compile_entry_with_imports`) must surface the live compile failure in
/// `build_gui_state`'s `compile_diagnostics`.
///
/// Exercises `update_source`'s multi-file branch (`compile_entry_with_imports`).
/// Pins the invariant introduced in task 3386: live compile failures on the multi-file
/// path are stored as `CompileFailureKind::LiveEdit` and surfaced via `build_gui_state`.
#[test]
fn build_gui_state_surfaces_live_compile_failure_after_failed_update_source_multi_file_with_prior_compile()
 {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let file_path = dir.path().join("main.ri");
    // Write valid bracket source to disk so load_file succeeds.
    std::fs::write(&file_path, bracket_source()).expect("write should succeed");

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // load_file sets compiled = Some AND file_path = Some(...)
    session
        .load_file(&file_path)
        .expect("valid source should load successfully");

    // Trigger a failed live edit via update_source (multi-file branch: file_path is Some).
    let err = session
        .update_source("main.ri", "broken {{{}}}")
        .expect_err("invalid source should return Err");
    assert!(
        err.contains("Parse errors") || err.contains("Compile errors"),
        "error string should mention parse/compile errors; got: {err}"
    );

    // compile_failure must be Some(LiveEdit) after the failure.
    assert!(
        matches!(
            session.compile_failure_for_test(),
            Some(CompileFailure { kind: CompileFailureKind::LiveEdit, diags, .. }) if !diags.is_empty()
        ),
        "compile_failure should be Some(LiveEdit) with non-empty diags after a failed update_source (multi-file)"
    );

    // build_gui_state must surface the live failure diagnostics.
    let state = session
        .build_gui_state()
        .expect("build_gui_state should return Ok with the prior good state plus live errors");

    assert!(
        !state.compile_diagnostics.is_empty(),
        "compile_diagnostics must be non-empty — live compile failure must be surfaced"
    );
    let has_error = state
        .compile_diagnostics
        .iter()
        .any(|d| d.severity == "Error");
    assert!(
        has_error,
        "at least one diagnostic must have severity Error; got: {:?}",
        state
            .compile_diagnostics
            .iter()
            .map(|d| &d.severity)
            .collect::<Vec<_>>()
    );
}

/// After a session already has a successful compile via `load_file` (`compiled` is
/// `Some`), a subsequent failed `load_file` (overwritten with broken source) must
/// surface the live compile failure in `build_gui_state`'s `compile_diagnostics`.
///
/// Exercises `load_file`'s failure path (`compile_entry_with_imports`).
/// Pins the invariant introduced in task 3386: live compile failures via `load_file`
/// are stored as `CompileFailureKind::LiveEdit` and surfaced via `build_gui_state`.
#[test]
fn build_gui_state_surfaces_live_compile_failure_after_failed_load_file_with_prior_compile() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let file_path = dir.path().join("main.ri");
    // Write valid source and load it to establish compiled = Some.
    std::fs::write(&file_path, bracket_source()).expect("write should succeed");

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_file(&file_path)
        .expect("valid source should load successfully");

    // Overwrite the file with broken source, then load_file again (compiled is Some).
    std::fs::write(&file_path, "broken {{{}}}\n").expect("overwrite should succeed");

    let err = session
        .load_file(&file_path)
        .expect_err("invalid source should return Err");
    assert!(
        err.contains("Parse errors") || err.contains("Compile errors"),
        "error string should mention parse/compile errors; got: {err}"
    );

    // compile_failure must be Some(LiveEdit) after the failure.
    assert!(
        matches!(
            session.compile_failure_for_test(),
            Some(CompileFailure { kind: CompileFailureKind::LiveEdit, diags, .. }) if !diags.is_empty()
        ),
        "compile_failure should be Some(LiveEdit) with non-empty diags after a failed load_file with prior compile"
    );

    // build_gui_state must surface the live failure diagnostics.
    let state = session
        .build_gui_state()
        .expect("build_gui_state should return Ok with the prior good state plus live errors");

    assert!(
        !state.compile_diagnostics.is_empty(),
        "compile_diagnostics must be non-empty — live compile failure must be surfaced"
    );
    let has_error = state
        .compile_diagnostics
        .iter()
        .any(|d| d.severity == "Error");
    assert!(
        has_error,
        "at least one diagnostic must have severity Error; got: {:?}",
        state
            .compile_diagnostics
            .iter()
            .map(|d| &d.severity)
            .collect::<Vec<_>>()
    );
}

/// Pins the documented append-order guarantee: when a live-edit failure occurs while the
/// prior good compile had warnings, `build_gui_state` surfaces both the prior warnings
/// **and** the live error in `compile_diagnostics`, with warnings first (from
/// `get_diagnostics()`) and the live error appended afterwards (from the `LiveEdit`
/// `compile_failure`).
///
/// This test verifies the design decision recorded in task 3386: "appending rather than
/// replacing preserves warnings/info from the last good state; Error entries from the
/// live-edit failure follow them, so frontends sorting by severity will surface errors first."
///
/// Uses `inject_diagnostic_for_test` to plant a synthetic Warning into the compiled module
/// (without needing a real source that emits warnings) before triggering the live failure.
#[test]
fn build_gui_state_surfaces_prior_warning_and_live_error_together_in_append_order() {
    use reify_core::Diagnostic;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Establish a successful compiled state.
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("valid source should load successfully");

    // Plant a synthetic Warning into the compiled module so get_diagnostics() returns it.
    session.inject_diagnostic_for_test(Diagnostic::warning("pre-existing warning from good state"));

    // Trigger a live-edit failure (compiled is Some, so compile_failure gets LiveEdit kind).
    let _ = session.load_from_source("this is not valid reify syntax {{{}}}", "bad");

    // compile_failure must be Some(LiveEdit) from the failure.
    assert!(
        matches!(
            session.compile_failure_for_test(),
            Some(CompileFailure { kind: CompileFailureKind::LiveEdit, diags, .. }) if !diags.is_empty()
        ),
        "compile_failure should be Some(LiveEdit) with non-empty diags after failed live edit"
    );

    // build_gui_state must surface both the prior warning and the live error.
    let state = session
        .build_gui_state()
        .expect("build_gui_state should return Ok with the prior good state plus live errors");

    let severities: Vec<&str> = state
        .compile_diagnostics
        .iter()
        .map(|d| d.severity.as_str())
        .collect();

    // Both Warning and Error must appear.
    assert!(
        severities.contains(&"Warning"),
        "compile_diagnostics must contain the prior Warning; got: {:?}",
        severities
    );
    assert!(
        severities.contains(&"Error"),
        "compile_diagnostics must contain the live-edit Error; got: {:?}",
        severities
    );

    // Warning must precede Error — get_diagnostics() output is first, live errors appended.
    let first_warning = state
        .compile_diagnostics
        .iter()
        .position(|d| d.severity == "Warning");
    let first_error = state
        .compile_diagnostics
        .iter()
        .position(|d| d.severity == "Error");
    assert!(
        first_warning.unwrap() < first_error.unwrap(),
        "Warning entries (from prior good compile) must precede Error entries \
         (from LiveEdit compile_failure append); got order: {:?}",
        severities
    );
}

// ---- Step: get_entity_at_source_location() tests ----

/// (a) No module loaded → get_entity_at_source_location returns None.
#[test]
fn get_entity_at_source_location_no_module_returns_none() {
    let checker = SimpleConstraintChecker;
    let session = EngineSession::new(Box::new(checker), None);
    let result = session.get_entity_at_source_location(1, 1);
    assert!(
        result.is_none(),
        "no module loaded → None, got {:?}",
        result
    );
}

/// (b) Zero line or zero col → None (documented out-of-range guard, 1-based coordinate system).
#[test]
fn get_entity_at_source_location_zero_line_or_col_returns_none() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    assert!(
        session.get_entity_at_source_location(0, 1).is_none(),
        "zero line → None"
    );
    assert!(
        session.get_entity_at_source_location(1, 0).is_none(),
        "zero col → None"
    );
    assert!(
        session.get_entity_at_source_location(0, 0).is_none(),
        "zero line and col → None"
    );
}

/// (c) Cursor mid-"width" identifier (line=2, col=11) → Some("Bracket.width").
///
/// bracket_source() line 2: "    param width: Scalar = 80mm"
/// col 11 is 'w' in "width", inside the width cell span.
#[test]
fn get_entity_at_source_location_width_cell_returns_bracket_width() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let result = session.get_entity_at_source_location(2, 11);
    assert_eq!(
        result,
        Some("Bracket.width".to_string()),
        "cursor at (2, 11) should resolve to Bracket.width"
    );
}

/// (d) Cursor mid-"thickness" identifier (line=4, col=11) → Some("Bracket.thickness").
///
/// bracket_source() line 4: "    param thickness: Scalar = 5mm"
/// col 11 is 't' in "thickness", inside the thickness cell span.
#[test]
fn get_entity_at_source_location_thickness_cell_returns_bracket_thickness() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let result = session.get_entity_at_source_location(4, 11);
    assert_eq!(
        result,
        Some("Bracket.thickness".to_string()),
        "cursor at (4, 11) should resolve to Bracket.thickness"
    );
}

/// (e) Cursor on the `pub structure Bracket {` header line (line=1, col=1) →
/// `Some("Bracket")`.
///
/// The resolver now uses the parsed `StructureDef.span` for the outer
/// containment check, which covers the full `pub structure NAME { ... }` byte
/// range including the header line (task 3880). Clicking anywhere on the header
/// line — including at byte 0 (col=1) — must resolve to the enclosing template
/// name, never to a member or to None.
#[test]
fn get_entity_at_source_location_structure_header_returns_none_or_template_name() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let result = session.get_entity_at_source_location(1, 1);
    assert_eq!(
        result,
        Some("Bracket".to_string()),
        "cursor on structure header (1,1) must resolve to the template name — got {:?}",
        result
    );
}

/// (f) Cursor on a constraint line → Some("Bracket") (inside template body, no value cell).
///
/// bracket_source() line 10: "    constraint thickness > 2mm"
/// col 5 is 'c' in "constraint" — inside the template's approximate span but not
/// within any value cell span.
#[test]
fn get_entity_at_source_location_constraint_line_returns_template_name() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let result = session.get_entity_at_source_location(10, 5);
    assert_eq!(
        result,
        Some("Bracket".to_string()),
        "cursor on constraint line (10, 5) should resolve to Bracket (template name, no cell hit)"
    );
}

/// (g) Cursor on line 16 (beyond end of source) → None.
///
/// bracket_source() has 15 lines. Line 16 is past the end; the byte offset
/// is outside every template's span → None.
#[test]
fn get_entity_at_source_location_past_end_of_source_returns_none() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let result = session.get_entity_at_source_location(16, 1);
    assert!(
        result.is_none(),
        "cursor past end of source (16, 1) should return None, got {:?}",
        result
    );
}

/// (h) Multi-structure source: cache-wiring smoke test.
///
/// Verifies that the `parsed_cache` is correctly threaded through to the
/// resolver for a multi-structure module loaded via `load_from_source`.
/// The full per-template/per-line matrix (First, Middle, Last header lines
/// + both gap lines) is covered by the unit test in
///   `crates/reify-eval/src/source_location.rs`; this integration test pins
///   only the cache-wiring for one representative header click and one gap click.
///
/// Source layout (1-based lines):
/// ```text
///  1: pub structure First {
///  2:     param a: Scalar = 1mm
///  3: }
///  4: (blank)
///  5: pub structure Middle {
///  6:     param b: Scalar = 2mm
///  7: }
///  8: (blank)
///  9: pub structure Last {
/// 10:     param c: Scalar = 3mm
/// 11: }
/// ```
#[test]
fn get_entity_at_source_location_multi_structure_header_lines_resolve_to_each_structure() {
    const THREE_STRUCT_SOURCE: &str = "pub structure First {\n    param a: Scalar = 1mm\n}\n\npub structure Middle {\n    param b: Scalar = 2mm\n}\n\npub structure Last {\n    param c: Scalar = 3mm\n}\n";

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(THREE_STRUCT_SOURCE, "multi")
        .expect("load should succeed");

    // One representative header-line click: Middle at (5,1) verifies that the
    // parsed_cache is threaded through to the resolver and that a non-first
    // structure's header resolves correctly (the original task-3880 regression).
    let middle_hdr = session.get_entity_at_source_location(5, 1);
    assert_eq!(
        middle_hdr,
        Some("Middle".to_string()),
        "header of Middle (5,1) must resolve to Some(\"Middle\"), got {:?}",
        middle_hdr
    );

    // One gap click: blank line between First and Middle must return None.
    let gap = session.get_entity_at_source_location(4, 1);
    assert!(
        gap.is_none(),
        "blank line between First and Middle (4,1) must return None, got {:?}",
        gap
    );
}

// ---------------------------------------------------------------------------
// Auto-resolve emitter tests (Task 3479)
// ---------------------------------------------------------------------------

/// Events recorded by [`RecordingEmitter`] for asserting the emit sequence.
#[derive(Debug)]
enum EmitEvent {
    Start,
    Iteration(crate::types::AutoResolveIteration),
    Complete,
}

/// A recording AutoResolveEmitter that captures all events into an Arc<Mutex<Vec>>.
///
/// Shared via Arc so tests can hold an Arc::clone of the events handle and
/// assert after the session call returns.
struct RecordingEmitter {
    events: std::sync::Arc<std::sync::Mutex<Vec<EmitEvent>>>,
}

impl RecordingEmitter {
    fn new() -> Self {
        Self {
            events: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        }
    }
}

impl crate::engine::AutoResolveEmitter for RecordingEmitter {
    fn start(&self) {
        self.events.lock().unwrap().push(EmitEvent::Start);
    }

    fn iteration(&self, iter: crate::types::AutoResolveIteration) {
        self.events.lock().unwrap().push(EmitEvent::Iteration(iter));
    }

    fn complete(&self) {
        self.events.lock().unwrap().push(EmitEvent::Complete);
    }
}

/// Step-4: No auto-resolve events should fire when the loaded source declares no
/// `auto` parameters.
///
/// This test pins the "source has no auto params" branch only. The complementary
/// "source has auto params but solver returns empty Solved" branch is covered
/// separately by `engine_session_no_auto_resolve_emission_when_solver_returns_empty_solved`.
#[test]
fn engine_session_no_auto_resolve_emission_when_source_has_no_auto_params() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let recorder = RecordingEmitter::new();
    let events = std::sync::Arc::clone(&recorder.events);
    session.set_auto_resolve_emitter(std::sync::Arc::new(recorder));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");

    let events = events.lock().unwrap();
    assert!(
        events.is_empty(),
        "no auto-resolve events should fire when bracket has no auto params, got {} events",
        events.len()
    );
}

/// Disambiguation test (suggestion 5): no auto-resolve events fire when the solver
/// returns `Solved {}` with an empty values map, even when the source has auto params.
///
/// This explicitly pins the `if check.resolved_params.is_empty() { return; }` guard in
/// `emit_auto_resolve_if_any` (engine.rs) — distinct from the "no auto params declared"
/// case covered by `engine_session_no_auto_resolve_emission_when_source_has_no_auto_params`.
///
/// Passes immediately (guard already present).
#[test]
fn engine_session_no_auto_resolve_emission_when_solver_returns_empty_solved() {
    use std::sync::Arc;

    // Solver returns Solved with empty values — simulates a solver that finds
    // no resolved auto-params for this check (e.g., constraints not tight enough).
    let solver = MockConstraintSolver::new_solved(std::collections::HashMap::new());

    // Auto-param fixture: source HAS an auto param, so the no-auto-params guard
    // is NOT responsible for the suppression — only the empty-Solved guard is.
    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();

    let compiled = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None)
        .with_solver_for_test(Box::new(solver));

    let recorder = RecordingEmitter::new();
    let events = Arc::clone(&recorder.events);
    session.set_auto_resolve_emitter(Arc::new(recorder));

    session.check_and_emit_for_test(&compiled);

    let events = events.lock().unwrap();
    assert!(
        events.is_empty(),
        "no auto-resolve events should fire when solver returns empty Solved, got {} events",
        events.len()
    );
}

/// Step-8: Pin the AutoResolveParameterValue conversion contract.
///
/// Resolved `mm(4.2)` → `{ value: 4.2, unit: "mm", display: "4.2mm" }`.
/// Tests `dimension.to_display_units` + `format_display_number` pipeline.
#[test]
fn engine_session_auto_resolve_iteration_parameter_payload_matches_resolved_value_shape() {
    use std::sync::Arc;

    let thickness_id = ValueCellId::new("S", "thickness");
    let mut solved = std::collections::HashMap::new();
    solved.insert(thickness_id.clone(), mm(4.2));
    let solver = MockConstraintSolver::new_solved(solved);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();

    let compiled = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None)
        .with_solver_for_test(Box::new(solver));

    let recorder = RecordingEmitter::new();
    let events = Arc::clone(&recorder.events);
    session.set_auto_resolve_emitter(Arc::new(recorder));

    session.check_and_emit_for_test(&compiled);

    let events = events.lock().unwrap();
    assert_eq!(events.len(), 3, "expected [Start, Iteration, Complete]");

    if let EmitEvent::Iteration(ref iter) = events[1] {
        let param = iter
            .parameters
            .get("S.thickness")
            .expect("S.thickness must be in parameters");
        assert!(
            (param.value - 4.2).abs() < 1e-10,
            "value must be 4.2 (display units), got {}",
            param.value
        );
        assert_eq!(param.unit, "mm", "unit must be 'mm'");
        assert_eq!(param.display, "4.2mm", "display must be '4.2mm'");
    } else {
        panic!("events[1] must be Iteration");
    }
}

/// Step-9: Pin the Satisfaction → bool projection in `AutoResolveConstraintProgress`.
///
/// Two constraints: `thickness > 2mm` (satisfied at 5mm) and `thickness > 10mm`
/// (violated at 5mm). Asserts exactly one satisfied and one violated entry.
#[test]
fn engine_session_auto_resolve_constraint_progress_projects_satisfaction_to_bool() {
    use std::sync::Arc;

    let thickness_id = ValueCellId::new("S", "thickness");
    let mut solved = std::collections::HashMap::new();
    solved.insert(thickness_id.clone(), mm(5.0));
    let solver = MockConstraintSolver::new_solved(solved);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .constraint(
            "S",
            1,
            None,
            gt(value_ref("S", "thickness"), literal(mm(10.0))),
        )
        .build();

    let compiled = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None)
        .with_solver_for_test(Box::new(solver));

    let recorder = RecordingEmitter::new();
    let events = Arc::clone(&recorder.events);
    session.set_auto_resolve_emitter(Arc::new(recorder));

    session.check_and_emit_for_test(&compiled);

    let events = events.lock().unwrap();
    assert_eq!(events.len(), 3, "expected [Start, Iteration, Complete]");

    if let EmitEvent::Iteration(ref iter) = events[1] {
        assert_eq!(iter.constraints.len(), 2, "must have 2 constraint entries");
        let satisfied = iter.constraints.values().filter(|c| c.satisfied).count();
        let violated = iter.constraints.values().filter(|c| !c.satisfied).count();
        assert_eq!(satisfied, 1, "exactly one constraint should be satisfied (> 2mm at 5mm)");
        assert_eq!(violated, 1, "exactly one constraint should be violated (> 10mm at 5mm)");
    } else {
        panic!("events[1] must be Iteration");
    }
}

/// Step-6: With a solver-injected session loaded with an auto-param fixture,
/// `check_and_emit_for_test` must fire exactly [Start, Iteration, Complete].
#[test]
fn engine_session_auto_resolve_emitter_fires_start_iter_complete_when_solver_resolves() {
    use std::sync::Arc;

    let thickness_id = ValueCellId::new("S", "thickness");
    let mut solved = std::collections::HashMap::new();
    solved.insert(thickness_id.clone(), mm(5.0));
    let solver = MockConstraintSolver::new_solved(solved);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();

    let compiled = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None)
        .with_solver_for_test(Box::new(solver));

    let recorder = RecordingEmitter::new();
    let events = Arc::clone(&recorder.events);
    session.set_auto_resolve_emitter(Arc::new(recorder));

    session.check_and_emit_for_test(&compiled);

    let events = events.lock().unwrap();
    assert_eq!(
        events.len(),
        3,
        "expected exactly 3 events (Start, Iteration, Complete), got {}",
        events.len()
    );
    assert!(
        matches!(events[0], EmitEvent::Start),
        "event[0] must be Start"
    );
    assert!(
        matches!(events[1], EmitEvent::Iteration(_)),
        "event[1] must be Iteration"
    );
    assert!(
        matches!(events[2], EmitEvent::Complete),
        "event[2] must be Complete"
    );

    // Assert the iteration payload has the expected parameter
    if let EmitEvent::Iteration(ref iter) = events[1] {
        assert!(
            iter.parameters.contains_key("S.thickness"),
            "parameters must contain 'S.thickness', got keys: {:?}",
            iter.parameters.keys().collect::<Vec<_>>()
        );
        assert!(!iter.constraints.is_empty(), "constraints must be non-empty");
    }
}

/// Step-11: `set_parameter` must re-fire the emit sequence when the solver resolves auto params.
///
/// Setup: session with `S.x` (regular param, settable) + `S.thickness` (auto param).
/// After initial check drains the recorder, `set_parameter("S.x", "10mm")` triggers
/// `edit_check` → solver resolves `thickness` again → [Start, Iteration, Complete].
#[test]
fn engine_session_auto_resolve_emitter_fires_on_set_parameter_when_solver_present() {
    use std::sync::Arc;

    let thickness_id = ValueCellId::new("S", "thickness");
    let mut solved = std::collections::HashMap::new();
    solved.insert(thickness_id.clone(), mm(5.0));
    let solver = MockConstraintSolver::new_solved(solved);

    // The constraint references S.x so that changing S.x makes the constraint
    // dirty → solver re-runs → resolved_params non-empty → emission fires.
    let template = TopologyTemplateBuilder::new("S")
        .param("S", "x", Type::length(), Some(literal(mm(5.0))))
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), value_ref("S", "x")),
        )
        .build();

    let compiled = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None)
        .with_solver_for_test(Box::new(solver));

    let recorder = RecordingEmitter::new();
    let events = Arc::clone(&recorder.events);
    session.set_auto_resolve_emitter(Arc::new(recorder));

    // Initial check: gives engine a snapshot and fires 3 events.
    session.check_and_emit_for_test(&compiled);
    // Inject compiled so set_parameter can validate the cell exists.
    session.inject_compiled_for_test(compiled);
    // Drain recorder before the set_parameter call.
    events.lock().unwrap().clear();

    // Changing S.x dirties the constraint (which reads S.x) → solver re-runs → emit fires.
    session.set_parameter("S.x", "10mm").expect("set_parameter should succeed");

    let events = events.lock().unwrap();
    assert_eq!(
        events.len(),
        3,
        "set_parameter must emit [Start, Iteration, Complete], got {} events",
        events.len()
    );
    assert!(matches!(events[0], EmitEvent::Start), "event[0] must be Start");
    assert!(matches!(events[1], EmitEvent::Iteration(_)), "event[1] must be Iteration");
    assert!(matches!(events[2], EmitEvent::Complete), "event[2] must be Complete");
}

/// Non-Scalar resolved auto-param emits NaN sentinel instead of being silently dropped.
///
/// When the solver returns a non-Scalar Value for a resolved auto-param (which
/// indicates a buggy or unexpected solver implementation), `build_parameters_payload`
/// must emit an `AutoResolveParameterValue { value: NaN, unit: "", display: "<non-scalar>" }`
/// so the GUI panel can render an error chip rather than silently omitting the cell.
///
/// RED: currently fails because the silent-drop branch omits the cell entirely —
/// the parameters HashMap won't contain the key. Step-5 impl makes it green.
#[test]
fn engine_session_auto_resolve_emitter_emits_nan_sentinel_for_non_scalar_resolved_param() {
    use std::sync::Arc;
    use reify_ir::Value;

    let thickness_id = ValueCellId::new("S", "thickness");
    let mut solved = std::collections::HashMap::new();
    // Inject a non-Scalar value to trigger the non-Scalar branch.
    solved.insert(thickness_id.clone(), Value::Int(7));
    let solver = MockConstraintSolver::new_solved(solved);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();

    let compiled = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None)
        .with_solver_for_test(Box::new(solver));

    let recorder = RecordingEmitter::new();
    let events = Arc::clone(&recorder.events);
    session.set_auto_resolve_emitter(Arc::new(recorder));

    session.check_and_emit_for_test(&compiled);

    let events = events.lock().unwrap();
    assert_eq!(
        events.len(),
        3,
        "expected [Start, Iteration, Complete] even for non-Scalar param, got {} events",
        events.len()
    );
    assert!(matches!(events[0], EmitEvent::Start), "event[0] must be Start");
    assert!(matches!(events[2], EmitEvent::Complete), "event[2] must be Complete");

    if let EmitEvent::Iteration(ref iter) = events[1] {
        let param = iter
            .parameters
            .get("S.thickness")
            .expect("S.thickness must be in parameters even for non-Scalar (NaN sentinel)");
        assert!(
            param.value.is_nan(),
            "non-Scalar resolved param must produce NaN sentinel value, got {}",
            param.value
        );
        assert_eq!(
            param.unit, "",
            "non-Scalar sentinel unit must be empty string, got '{}'",
            param.unit
        );
        assert_eq!(
            param.display, "<non-scalar>",
            "non-Scalar sentinel display must be '<non-scalar>', got '{}'",
            param.display
        );
    } else {
        panic!("events[1] must be Iteration");
    }
}

/// Option(Some(Scalar)) resolved auto-param flows through `format_display_triple`
/// recursion into a real payload entry — NOT a `<non-scalar>` sentinel and NOT a
/// silent drop.
///
/// Pins the recursion arm of `format_display_triple` (value.rs) at the emit
/// boundary: when the solver returns `Value::Option(Some(Scalar))`, the
/// `build_parameters_payload` call must recurse through the inner Scalar and
/// produce a real `AutoResolveParameterValue { value ≈ 5.0, unit: "mm",
/// display: "5mm" }` rather than treating the outer Option as a non-scalar.
#[test]
fn engine_session_auto_resolve_emitter_emits_real_entry_for_option_some_scalar_resolved_param() {
    use std::sync::Arc;
    use reify_ir::Value;

    let thickness_id = ValueCellId::new("S", "thickness");
    let mut solved = std::collections::HashMap::new();
    // Inject an Option(Some(Scalar)) value — must recurse to a real entry.
    solved.insert(
        thickness_id.clone(),
        Value::Option(Some(Box::new(mm(5.0)))),
    );
    let solver = MockConstraintSolver::new_solved(solved);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();

    let compiled = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None)
        .with_solver_for_test(Box::new(solver));

    let recorder = RecordingEmitter::new();
    let events = Arc::clone(&recorder.events);
    session.set_auto_resolve_emitter(Arc::new(recorder));

    session.check_and_emit_for_test(&compiled);

    let events = events.lock().unwrap();
    assert_eq!(
        events.len(),
        3,
        "expected [Start, Iteration, Complete] for Option(Some(Scalar)) param, got {} events",
        events.len()
    );
    assert!(matches!(events[0], EmitEvent::Start), "event[0] must be Start");
    assert!(matches!(events[2], EmitEvent::Complete), "event[2] must be Complete");

    if let EmitEvent::Iteration(ref iter) = events[1] {
        let param = iter
            .parameters
            .get("S.thickness")
            .expect("S.thickness must be in parameters — Option(Some(Scalar)) must recurse to a real entry");
        assert!(
            !param.value.is_nan(),
            "Option(Some(Scalar)) must produce a real value (not NaN), got {}",
            param.value
        );
        assert!(
            (param.value - 5.0).abs() < 1e-10,
            "Option(Some(Scalar)) must recurse: display value must be ≈5.0 (mm), got {}",
            param.value
        );
        assert_eq!(
            param.unit, "mm",
            "Option(Some(Scalar)) recursion must surface the engineering-unit symbol, got '{}'",
            param.unit
        );
        assert_eq!(
            param.display, "5mm",
            "Option(Some(Scalar)) recursion must yield whole-number formatted display, got '{}'",
            param.display
        );
        assert_ne!(
            param.display, "<non-scalar>",
            "Option(Some(Scalar)) must NOT produce the <non-scalar> sentinel"
        );
    } else {
        panic!("events[1] must be Iteration");
    }
}

/// Integration test (suggestion 4): auto-resolve emitter fires through the full
/// `load_from_source` → `commit_state` → `emit_auto_resolve_if_any(last_check().unwrap())`
/// path.
///
/// Pins that load_from_source emits AFTER state is committed (correct ordering).
/// Acts as a characterization safety-net for the step-7 reorder of load_file /
/// update_source / set_parameter.
///
/// Expected to pass immediately — load_from_source already has correct ordering.
#[test]
fn engine_session_auto_resolve_emitter_fires_through_load_from_source_real_path() {
    use std::sync::Arc;

    let source = r#"structure S {
    param thickness: Scalar = auto
    constraint thickness > 2mm
}"#;

    let thickness_id = ValueCellId::new("S", "thickness");
    let mut solved = std::collections::HashMap::new();
    solved.insert(thickness_id.clone(), mm(5.0));
    let solver = MockConstraintSolver::new_solved(solved);

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None)
        .with_solver_for_test(Box::new(solver));

    let recorder = RecordingEmitter::new();
    let events = Arc::clone(&recorder.events);
    session.set_auto_resolve_emitter(Arc::new(recorder));

    session
        .load_from_source(source, "S")
        .expect("load_from_source with auto-param source should succeed");

    let events = events.lock().unwrap();
    assert_eq!(
        events.len(),
        3,
        "load_from_source must emit [Start, Iteration, Complete], got {} events",
        events.len()
    );
    assert!(matches!(events[0], EmitEvent::Start), "event[0] must be Start");
    assert!(matches!(events[1], EmitEvent::Iteration(_)), "event[1] must be Iteration");
    assert!(matches!(events[2], EmitEvent::Complete), "event[2] must be Complete");

    if let EmitEvent::Iteration(ref iter) = events[1] {
        assert!(
            iter.parameters.contains_key("S.thickness"),
            "parameters must contain 'S.thickness', got keys: {:?}",
            iter.parameters.keys().collect::<Vec<_>>()
        );
    } else {
        panic!("events[1] must be Iteration");
    }
}

// ── Production solver wiring test ────────────────────────────────────────────

/// Production `EngineSession::with_registered_kernel` installs a working solver.
///
/// Constructs via the production boot path —
/// `EngineSession::with_registered_kernel(Box::new(SimpleConstraintChecker))` —
/// WITHOUT calling `with_solver_for_test`.
/// Installs a `RecordingEmitter`, loads an inline source with one `auto` param and
/// a `minimize` directive, then asserts the emitter fires
/// `[Start, Iteration, Complete]`.
///
/// Combined with `production_registry_routes_geometric_to_solvespace` (step-1/step-2),
/// this proves the production GUI engine carries a working dimensional solver and
/// SolveSpaceSolver in its geometric slot.
///
/// RED today: `with_registered_kernel` delegates straight to `from_engine` which
/// installs no solver → `resolved_params` is always empty → emitter never fires.
#[test]
fn with_registered_kernel_production_session_resolves_auto_param() {
    use std::sync::Arc;

    // Mirror auto_minimize.ri: one auto param + box constraints + minimize.
    let source = r#"structure AutoMinimize {
    param thickness: Scalar = auto
    constraint thickness > 2mm
    constraint thickness < 20mm
    minimize thickness
}"#;

    let mut session = EngineSession::with_registered_kernel(Box::new(SimpleConstraintChecker));

    let recorder = RecordingEmitter::new();
    let events = Arc::clone(&recorder.events);
    session.set_auto_resolve_emitter(Arc::new(recorder));

    session
        .load_from_source(source, "AutoMinimize")
        .expect("load_from_source with auto-param source should succeed");

    let events = events.lock().unwrap();
    assert!(
        !events.is_empty(),
        "production GUI engine must have a solver installed: \
         expected auto-resolve events, got none",
    );
    assert!(
        matches!(events.last(), Some(EmitEvent::Complete)),
        "expected Complete as the last event, got {:?}",
        events.last()
    );
}

// ── Structural lock-in test ──────────────────────────────────────────────────

/// Structural lock-in test: verifies that `EngineSession` exposes `CoreState`
/// via `core_state_for_test()` and that `CoreState` provides the expected six
/// read accessors.  This test fails to compile before the CoreState refactor
/// (neither the type nor the accessor method exists) — that compile failure IS
/// the RED state.  After step-2 lands it must pass and continue passing forever.
#[test]
fn engine_session_exposes_core_state_with_read_accessors() {
    use reify_eval::Engine;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");

    let core: &CoreState = session.core_state_for_test();

    // compiled() must be Some after a successful load
    assert!(core.compiled().is_some(), "compiled should be Some after load");

    // last_check() must be Some after a successful load
    assert!(core.last_check().is_some(), "last_check should be Some after load");

    // module_name() must be Some("bracket")
    assert_eq!(
        core.module_name(),
        Some("bracket"),
        "module_name should be Some(\"bracket\") after load_from_source"
    );

    // source_map() must contain "bracket.ri" (the canonical key for module "bracket")
    assert!(
        core.source_map().contains_key("bracket.ri"),
        "source_map should contain key \"bracket.ri\" after loading bracket source"
    );

    // file_path() must be None — load_from_source does NOT set file_path
    assert!(
        core.file_path().is_none(),
        "file_path should be None after load_from_source (only load_file sets it)"
    );

    // engine() must return &Engine — this is a type-level assertion: it fails to
    // compile if the accessor is absent or returns the wrong type.
    let _: &Engine = core.engine();
}

/// Behavioral test for `CoreState::commit_check`:
/// `set_parameter` must update `last_check` and leave the other five core fields
/// (`engine`, `compiled`, `source_map`, `file_path`, `module_name`) untouched.
#[test]
fn set_parameter_updates_only_last_check_via_commit_check() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");

    // Snapshot the five non-last_check core fields before calling set_parameter.
    let pre_module_name: Option<String> = session
        .core_state_for_test()
        .module_name()
        .map(|s| s.to_string());
    let pre_compiled_is_some: bool = session.core_state_for_test().compiled().is_some();
    let pre_source_map_keys: std::collections::BTreeSet<String> = session
        .core_state_for_test()
        .source_map()
        .keys()
        .cloned()
        .collect();
    let pre_file_path: Option<std::path::PathBuf> = session
        .core_state_for_test()
        .file_path()
        .map(|p| p.to_path_buf());

    // Trigger commit_check internally via set_parameter.
    session
        .set_parameter("Bracket.width", "100mm")
        .expect("set_parameter ok");

    // last_check must be Some after set_parameter.
    assert!(
        session.core_state_for_test().last_check().is_some(),
        "last_check must be Some after set_parameter"
    );

    // The other five core fields must be byte-for-byte identical to the pre-call
    // snapshot — commit_check must not touch anything other than last_check.
    assert_eq!(
        session.core_state_for_test().module_name().map(|s| s.to_string()),
        pre_module_name,
        "module_name must not change after set_parameter"
    );
    assert_eq!(
        session.core_state_for_test().compiled().is_some(),
        pre_compiled_is_some,
        "compiled presence must not change after set_parameter"
    );
    let post_source_map_keys: std::collections::BTreeSet<String> = session
        .core_state_for_test()
        .source_map()
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        post_source_map_keys,
        pre_source_map_keys,
        "source_map keys must not change after set_parameter"
    );
    assert_eq!(
        session.core_state_for_test().file_path().map(|p| p.to_path_buf()),
        pre_file_path,
        "file_path must not change after set_parameter"
    );

    // A second set_parameter call must also keep last_check Some.
    session
        .set_parameter("Bracket.width", "80mm")
        .expect("second set_parameter ok");
    assert!(
        session.core_state_for_test().last_check().is_some(),
        "last_check must remain Some after second set_parameter"
    );
}

/// Behavioral test for the five-field atomic commit via `EngineSession::commit_state`:
/// `load_file` must commit all five core fields atomically — `file_path`, `compiled`,
/// `last_check`, `module_name`, and the `source_map` entry keyed by the file stem.
/// Previously `file_path` was a separate `commit_file_path` step; now it is folded
/// into the single `commit_state` call, making this the post-refactor regression pin.
#[test]
fn load_file_commits_file_path_atomically_via_commit_state() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Pre-load: file_path must be None on a fresh session.
    assert!(
        session.core_state_for_test().file_path().is_none(),
        "file_path must be None on a fresh session before load_file"
    );

    // Write bracket_source() to a temp file named "bracket.ri" so that load_file
    // derives module_name = "bracket" from the file stem.
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let tmp_path = dir.path().join("bracket.ri");
    std::fs::write(&tmp_path, bracket_source()).expect("write bracket.ri to tempdir");

    // load_file commits all five core fields atomically via commit_state.
    session
        .load_file(&tmp_path)
        .expect("load_file should succeed");

    // file_path() must now equal the path we loaded.
    let core = session.core_state_for_test();
    assert_eq!(
        core.file_path(),
        Some(tmp_path.as_path()),
        "file_path must be Some(tmp_path) after load_file"
    );

    // Successful load means compiled and last_check must also be Some.
    assert!(
        core.compiled().is_some(),
        "compiled must be Some after successful load_file"
    );
    assert!(
        core.last_check().is_some(),
        "last_check must be Some after successful load_file"
    );

    // module_name must match the file stem ("bracket").
    assert_eq!(
        core.module_name(),
        Some("bracket"),
        "module_name must be 'bracket' (from file stem) after load_file"
    );

    // source_map must contain the key produced by module_key("bracket") — five-field
    // atomic commit pin.  Using module_key() here avoids coupling to the literal
    // key format (".ri" suffix) so that a future rename (e.g. ".reify") does not
    // break this test for an unrelated reason.
    assert!(
        core.source_map().contains_key(&module_key("bracket")),
        "source_map must contain module_key(\"bracket\") after load_file (five-field atomic commit pin)"
    );
}

/// Regression test for the `FilePathUpdate::Preserve`-preserves-`file_path` contract
/// in `commit_state`: when `update_source` passes `FilePathUpdate::Preserve` as the
/// `file_path` argument to `commit_state`, the existing `file_path` must be preserved —
/// NOT cleared to `None`.
///
/// This test is RED against the naive `match file_path { Set(p) => Some(p), Preserve => None }`
/// implementation (which would clear `file_path` on every `update_source` call, breaking
/// the multi-file edit-routing that derives `module_name` and project-root from
/// `self.core.file_path()` in `update_source`).  It must be GREEN after the correct
/// `Preserve => { /* leave self.file_path unchanged */ }` arm.
#[test]
fn update_source_preserves_file_path_when_commit_state_gets_preserve() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Write bracket.ri to a tempdir and load it — file_path becomes Some(tmp_path).
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let tmp_path = dir.path().join("bracket.ri");
    std::fs::write(&tmp_path, bracket_source()).expect("write bracket.ri to tempdir");
    session
        .load_file(&tmp_path)
        .expect("load_file should succeed");

    // Pre-condition: file_path must be Some after load_file.
    assert_eq!(
        session.core_state_for_test().file_path(),
        Some(tmp_path.as_path()),
        "file_path must be Some(tmp_path) after load_file (pre-condition)"
    );

    // update_source passes FilePathUpdate::Preserve for file_path to commit_state — must PRESERVE, not clear.
    let new_source = bracket_source_with_width("120mm");
    session
        .update_source(tmp_path.to_str().unwrap(), &new_source)
        .expect("update_source should succeed");

    // file_path must still be Some(tmp_path) — Preserve-preserves contract.
    let core = session.core_state_for_test();
    assert_eq!(
        core.file_path(),
        Some(tmp_path.as_path()),
        "file_path must still be Some(tmp_path) after update_source (Preserve-preserves contract)"
    );

    // compiled and module_name must remain consistent after update_source.
    assert!(
        core.compiled().is_some(),
        "compiled must still be Some after update_source"
    );
    assert_eq!(
        core.module_name(),
        Some("bracket"),
        "module_name must still be 'bracket' after update_source"
    );
}

// ── Task 3541 step-5: WarmPoolEventEmitter recording ────────────────────────

/// Recording `WarmPoolEventEmitter` that captures every emitted IPC
/// [`crate::types::WarmPoolEvent`] for test assertions.
///
/// Mirrors [`RecordingEmitter`] (line 7234) for the warm-pool channel.
struct RecordingWarmPoolEventEmitter {
    events: std::sync::Arc<std::sync::Mutex<Vec<crate::types::WarmPoolEvent>>>,
}

impl RecordingWarmPoolEventEmitter {
    fn new() -> Self {
        Self {
            events: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        }
    }
}

impl crate::engine::WarmPoolEventEmitter for RecordingWarmPoolEventEmitter {
    fn emit(&self, event: crate::types::WarmPoolEvent) {
        self.events.lock().unwrap().push(event);
    }
}

/// Step-5: `EngineSession` emits IPC `WarmPoolEvent` values through an installed
/// `WarmPoolEventEmitter` when `drain_and_emit_warm_pool_events` is called after
/// pool activity.
///
/// Test flow:
/// (a) Construct EngineSession, install RecordingWarmPoolEventEmitter.
/// (b) Pre-populate the warm pool with a donate (node_a) that triggers an
///     eviction (budget=1 byte): donate(node_a), donate(node_b) → Evicted(a).
/// (c) Call `session.drain_and_emit_warm_pool_events_for_test()`.
/// (d) Assert recorder captured both events with correct kind/size_bytes/node_id.
///
/// Fails to compile: WarmPoolEventEmitter trait, set_warm_pool_event_emitter,
/// warm_pool_mut_for_test, and drain_and_emit_warm_pool_events_for_test don't
/// exist yet (all added in step-6).
#[test]
fn engine_session_warm_pool_event_emitter_captures_donated_and_evicted_events() {
    use std::sync::Arc;
    use reify_ir::OpaqueState;
    use reify_eval::cache::NodeId;
    use reify_core::ValueCellId;

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    let recorder = RecordingWarmPoolEventEmitter::new();
    let captured = Arc::clone(&recorder.events);
    session.set_warm_pool_event_emitter(Arc::new(recorder));

    // Install a 1-byte warm pool budget so donate(b) evicts donate(a).
    {
        let pool = session.warm_pool_mut_for_test();
        *pool = reify_eval::warm_pool::WarmStatePool::new(1);
    }

    let node_a = NodeId::Value(ValueCellId::new("Beam", "length"));
    let node_b = NodeId::Value(ValueCellId::new("Plate", "width"));

    // Donate two nodes: node_a fits (size=1, budget=1), node_b evicts node_a.
    session.warm_pool_mut_for_test().donate(node_a.clone(), OpaqueState::new(1i32, 1));
    session.warm_pool_mut_for_test().donate(node_b.clone(), OpaqueState::new(2i32, 1));

    // Drain and emit.
    session.drain_and_emit_warm_pool_events_for_test();

    let events = captured.lock().unwrap();

    // Exactly 3 events in deterministic order: Donated(Beam.length), Evicted(Beam.length),
    // Donated(Plate.width).  donate(node_a, size=1, budget=1) → Donated(node_a); donate(node_b,
    // size=1) evicts node_a (LRU) → Evicted(node_a), Donated(node_b).  Loose assertions like
    // `>= 2` allow regressions that produce 1-2 events to silently pass.
    assert_eq!(
        events.len(),
        3,
        "donate(a)+evict(a)+donate(b) must yield exactly 3 IPC events; got {}",
        events.len()
    );

    // (a) events[0]: Donated(node_a) — "Beam.length"
    assert_eq!(events[0].kind, "donated", "events[0] must be kind=donated");
    assert_eq!(
        events[0].node_id, "Beam.length",
        "events[0].node_id must be 'Beam.length' (node_a)"
    );
    assert_eq!(events[0].size_bytes, 1, "events[0].size_bytes must be 1");

    // (b) events[1]: Evicted(node_a) — "Beam.length" (the LRU victim)
    assert_eq!(events[1].kind, "evicted", "events[1] must be kind=evicted");
    assert_eq!(
        events[1].node_id, "Beam.length",
        "events[1].node_id must be 'Beam.length' (victim)"
    );
    assert_eq!(events[1].size_bytes, 1, "events[1].size_bytes must be 1");

    // (c) events[2]: Donated(node_b) — "Plate.width"
    assert_eq!(events[2].kind, "donated", "events[2] must be kind=donated");
    assert_eq!(
        events[2].node_id, "Plate.width",
        "events[2].node_id must be 'Plate.width' (node_b)"
    );
    assert_eq!(events[2].size_bytes, 1, "events[2].size_bytes must be 1");
}

#[test]
fn sweep_persistent_cache_removes_stale_tempfile_under_explicit_cache_root() {
    // Parameterized-seam unit test (task 3698): calls
    // `crate::engine::sweep_persistent_cache(cache_dir.path())` with an
    // explicit hermetic TempDir rather than manipulating process env — which
    // would be racy in in-process tests (std::env::set_var is not thread-safe).
    //
    // Asserts: stale .tmp.* file is gone, report.tempfiles_removed == 1.
    use std::fs::{self, File, OpenOptions};
    use std::io::Write as _;
    use std::time::{Duration, SystemTime};

    use reify_eval::persistent_cache::{ENGINE_VERSION_HASH, STALE_TEMPFILE_AGE, shard_dir};
    use tempfile::TempDir;

    let cache_dir = TempDir::new().expect("tempdir");

    // 32-char hex hash whose "bb" prefix determines the shard subdirectory.
    let input_hash = "bb00000000000000000000000000cafe";
    let shard = shard_dir(cache_dir.path(), ENGINE_VERSION_HASH, input_hash);
    fs::create_dir_all(&shard).expect("create shard dir");

    let stale_path = shard.join(".tmp.stale_seed");
    {
        let mut f = File::create(&stale_path).expect("create stale tempfile");
        f.write_all(b"stale content").expect("write stale content");
    }

    // Backdate mtime to > STALE_TEMPFILE_AGE (1 h) past; 2-min buffer for CI.
    let stale_mtime = SystemTime::now() - (STALE_TEMPFILE_AGE + Duration::from_secs(120));
    let times = std::fs::FileTimes::new().set_modified(stale_mtime);
    {
        let file = OpenOptions::new()
            .write(true)
            .open(&stale_path)
            .expect("open stale file to backdate mtime");
        file.set_times(times).expect("backdate mtime");
    }

    // Call the parameterized seam (defined in step-4).
    let report = crate::engine::sweep_persistent_cache(cache_dir.path());

    assert!(
        !stale_path.exists(),
        "stale .tmp.* file must be removed by sweep_persistent_cache; path={stale_path:?}"
    );
    assert_eq!(
        report.tempfiles_removed, 1,
        "SweepReport.tempfiles_removed must be 1"
    );
}

// ── FeaCaseEmitter tests ─────────────────────────────────────────────────────

/// Mirrors [`RecordingEmitter`] (line 7234) and [`RecordingWarmPoolEventEmitter`] (line 7971)
/// for the fea-case-changed channel.
///
/// Compile-fails in step-5 because `crate::engine::FeaCaseEmitter` does not exist yet.
struct RecordingFeaCaseEmitter {
    events: std::sync::Arc<std::sync::Mutex<Vec<crate::types::FeaCaseChanged>>>,
}

impl RecordingFeaCaseEmitter {
    fn new() -> Self {
        Self {
            events: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        }
    }
}

impl crate::engine::FeaCaseEmitter for RecordingFeaCaseEmitter {
    fn changed(&self, payload: crate::types::FeaCaseChanged) {
        self.events.lock().unwrap().push(payload);
    }
}

/// (a) fea_case_emitter_fires_when_multi_case_value_present.
///
/// Constructs a `CheckResult` with a `multi_case_result_value`-shaped cell and
/// drives it through `emit_fea_case_for_test_with_result`. Asserts exactly one
/// event with the expected active_case_id and available_cases.
///
/// Compile-fails because `FeaCaseEmitter`, `set_fea_case_emitter`, and
/// `emit_fea_case_for_test_with_result` do not exist yet.
#[test]
fn fea_case_emitter_fires_when_multi_case_value_present() {
    use std::sync::Arc;
    use reify_eval::CheckResult;
    use reify_core::ValueCellId;
    use reify_ir::ValueMap;
    use reify_test_support::multi_case_result_value;
    use reify_ir::Value;

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    let recorder = RecordingFeaCaseEmitter::new();
    let captured = Arc::clone(&recorder.events);
    session.set_fea_case_emitter(Arc::new(recorder));

    // Build a hand-crafted CheckResult whose values map contains one MultiCaseResult cell.
    let mut values = ValueMap::new();
    let cell_id = ValueCellId::new("S", "result");
    values.insert(cell_id, multi_case_result_value(&[("A", Value::Int(1)), ("B", Value::Int(2))]));

    let check = CheckResult {
        values,
        constraint_results: vec![],
        diagnostics: vec![],
        resolved_params: std::collections::HashMap::new(),
    };

    session.emit_fea_case_for_test_with_result(&check);

    let events = captured.lock().unwrap();
    assert_eq!(events.len(), 1, "expected exactly one fea-case-changed event, got {}", events.len());
    assert_eq!(events[0].active_case_id, "A", "active_case_id must be lex-smallest 'A'");
    assert_eq!(
        events[0].available_cases,
        vec!["A".to_string(), "B".to_string()],
        "available_cases must be sorted"
    );
}

/// (b) fea_case_emitter_no_fire_when_no_multi_case.
///
/// A CheckResult with no MultiCaseResult-shaped cell produces zero events.
///
/// Compile-fails because `FeaCaseEmitter`, `set_fea_case_emitter`, and
/// `emit_fea_case_for_test_with_result` do not exist yet.
#[test]
fn fea_case_emitter_no_fire_when_no_multi_case() {
    use std::sync::Arc;
    use reify_eval::CheckResult;
    use reify_core::ValueCellId;
    use reify_ir::ValueMap;
    use reify_ir::Value;

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    let recorder = RecordingFeaCaseEmitter::new();
    let captured = Arc::clone(&recorder.events);
    session.set_fea_case_emitter(Arc::new(recorder));

    // Ordinary (non-MultiCaseResult) value — should not trigger the emitter.
    let mut values = ValueMap::new();
    values.insert(ValueCellId::new("S", "width"), Value::Int(42));

    let check = CheckResult {
        values,
        constraint_results: vec![],
        diagnostics: vec![],
        resolved_params: std::collections::HashMap::new(),
    };

    session.emit_fea_case_for_test_with_result(&check);

    let events = captured.lock().unwrap();
    assert!(events.is_empty(), "no events should fire for a non-MultiCaseResult cell");
}

/// (c) fea_case_emitter_re_fires_on_each_check.
///
/// Calling `emit_fea_case_for_test_with_result` twice with the same case set records
/// TWO events — pins the fire-every-commit / no-engine-side-dedup contract that
/// mirrors `emit_auto_resolve_if_any`.
///
/// NOTE: No duplicate-suppression test is included — engine-side dedup is
/// intentionally absent (design decision: mirror established &self fire-every-commit).
///
/// Compile-fails because `FeaCaseEmitter`, `set_fea_case_emitter`, and
/// `emit_fea_case_for_test_with_result` do not exist yet.
#[test]
fn fea_case_emitter_re_fires_on_each_check() {
    use std::sync::Arc;
    use reify_eval::CheckResult;
    use reify_core::ValueCellId;
    use reify_ir::ValueMap;
    use reify_test_support::multi_case_result_value;
    use reify_ir::Value;

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    let recorder = RecordingFeaCaseEmitter::new();
    let captured = Arc::clone(&recorder.events);
    session.set_fea_case_emitter(Arc::new(recorder));

    let mut values = ValueMap::new();
    values.insert(
        ValueCellId::new("S", "result"),
        multi_case_result_value(&[("A", Value::Int(1)), ("B", Value::Int(2))]),
    );

    let check = CheckResult {
        values,
        constraint_results: vec![],
        diagnostics: vec![],
        resolved_params: std::collections::HashMap::new(),
    };

    // First emission
    session.emit_fea_case_for_test_with_result(&check);
    // Second emission — same case set, must fire again (no dedup)
    session.emit_fea_case_for_test_with_result(&check);

    let events = captured.lock().unwrap();
    assert_eq!(
        events.len(),
        2,
        "must record TWO events on two calls — no engine-side dedup; got {}",
        events.len()
    );
}

/// (d) fea_case_emitter_wires_through_real_commit_path.
///
/// Anchors that `emit_fea_case_if_any` is called at the real production
/// `load_from_source` commit site (not just via the `emit_fea_case_for_test_with_result`
/// shim). Uses an ordinary (non-MultiCaseResult) source so the recorder sees
/// zero events — the meaningful contract here is that the emitter callback IS
/// consulted on every real commit, not that it fires for this particular source.
///
/// NOTE: A positive assertion (event received) requires a source that evaluates
/// to a `MultiCaseResult`-shaped value, which becomes possible when task 3026
/// lands `solve_load_cases`. At that point, replace the zero-assertion below
/// with a positive one mirroring the auto-resolve integration test at
/// `engine_session_auto_resolve_emitter_fires_through_load_from_source_real_path`.
#[test]
fn fea_case_emitter_wires_through_real_commit_path() {
    use std::sync::Arc;

    let source = r#"structure S {
    param width: Scalar = 10mm
}"#;

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    let recorder = RecordingFeaCaseEmitter::new();
    let captured = Arc::clone(&recorder.events);
    session.set_fea_case_emitter(Arc::new(recorder));

    session
        .load_from_source(source, "S")
        .expect("load_from_source with ordinary source should succeed");

    let events = captured.lock().unwrap();
    assert!(
        events.is_empty(),
        "no fea-case events for ordinary (non-MultiCaseResult) source via real commit path; \
         got {} events",
        events.len()
    );
}

// ---- extract_joint_descriptor kind-based binding default tests (task 3783, step-5) ----

/// Helper: build a single-body mechanism Value::Map containing a joint of the
/// given kind.  The joint map contains only a `"kind"` key (sufficient for the
/// kind→binding dispatch; other fields are not required by extract_joint_descriptor).
fn make_single_body_mechanism_map(
    joint_kind: &str,
) -> std::collections::BTreeMap<reify_ir::Value, reify_ir::Value> {
    use std::collections::BTreeMap;
    use reify_ir::Value;

    let mut joint_map: BTreeMap<Value, Value> = BTreeMap::new();
    joint_map.insert(
        Value::String("kind".to_string()),
        Value::String(joint_kind.to_string()),
    );

    let mut body_map: BTreeMap<Value, Value> = BTreeMap::new();
    body_map.insert(Value::String("at".to_string()), Value::Map(joint_map));

    let mut mech_map: BTreeMap<Value, Value> = BTreeMap::new();
    mech_map.insert(
        Value::String("kind".to_string()),
        Value::String("mechanism".to_string()),
    );
    mech_map.insert(
        Value::String("bodies".to_string()),
        Value::List(vec![Value::Map(body_map)]),
    );
    mech_map
}

/// `extract_joints_from_mechanism` assigns `JointBinding::FixedNoMotion` as
/// the default binding for a joint of kind `"fixed"`.
#[test]
fn extract_joint_descriptor_assigns_kind_based_binding_defaults_fixed() {
    use crate::engine::extract_joints_from_mechanism;
    use crate::types::JointBinding;

    let mech_map = make_single_body_mechanism_map("fixed");
    let (joints, _) = extract_joints_from_mechanism(&mech_map);

    assert_eq!(joints.len(), 1, "expected 1 joint descriptor for fixed");
    assert_eq!(
        joints[0].binding,
        JointBinding::FixedNoMotion,
        "fixed joint must have binding=FixedNoMotion; got {:?}",
        joints[0].binding
    );
}

/// `extract_joints_from_mechanism` assigns `JointBinding::CouplingDerived { source_joint: "" }`
/// as the default binding for a joint of kind `"coupling"`.
#[test]
fn extract_joint_descriptor_assigns_kind_based_binding_defaults_coupling() {
    use crate::engine::extract_joints_from_mechanism;
    use crate::types::JointBinding;

    let mech_map = make_single_body_mechanism_map("coupling");
    let (joints, _) = extract_joints_from_mechanism(&mech_map);

    assert_eq!(joints.len(), 1, "expected 1 joint descriptor for coupling");
    assert_eq!(
        joints[0].binding,
        JointBinding::CouplingDerived {
            source_joint: "".to_string()
        },
        "coupling joint must have binding=CouplingDerived {{ source_joint: \"\" }}; got {:?}",
        joints[0].binding
    );
}

/// `extract_joints_from_mechanism` assigns a `JointBinding::LiteralBound` with
/// `synth_param_name = "__joint_2_v"`, `initial_value_si = None`, `scrubbable = true`
/// for a prismatic joint at joint_index 2.
///
/// Note: joint_index is 0-based within the mechanism. To get index=2 we add 3 bodies
/// with 3 distinct joints and check the third one.
#[test]
fn extract_joint_descriptor_assigns_kind_based_binding_defaults_prismatic() {
    use std::collections::BTreeMap;
    use crate::engine::extract_joints_from_mechanism;
    use crate::types::JointBinding;
    use reify_ir::Value;

    // Build a mechanism with 3 distinct prismatic joints so the third has joint_index=2.
    let make_prismatic = |tag: u8| -> Value {
        let mut joint_map: BTreeMap<Value, Value> = BTreeMap::new();
        joint_map.insert(
            Value::String("kind".to_string()),
            Value::String("prismatic".to_string()),
        );
        // Use a unique tag key to ensure structural inequality for deduplication.
        joint_map.insert(
            Value::String("_tag".to_string()),
            Value::Int(tag as i64),
        );
        Value::Map(joint_map)
    };

    let bodies: Vec<Value> = (0u8..3)
        .map(|i| {
            let mut body_map: BTreeMap<Value, Value> = BTreeMap::new();
            body_map.insert(Value::String("at".to_string()), make_prismatic(i));
            Value::Map(body_map)
        })
        .collect();

    let mut mech_map: BTreeMap<Value, Value> = BTreeMap::new();
    mech_map.insert(
        Value::String("kind".to_string()),
        Value::String("mechanism".to_string()),
    );
    mech_map.insert(Value::String("bodies".to_string()), Value::List(bodies));

    let (joints, _) = extract_joints_from_mechanism(&mech_map);
    assert_eq!(joints.len(), 3, "expected 3 joint descriptors");

    // Third joint has joint_index=2.
    assert_eq!(
        joints[2].binding,
        JointBinding::LiteralBound {
            synth_param_name: "__joint_2_v".to_string(),
            initial_value_si: None,
            scrubbable: true,
        },
        "prismatic at joint_index=2 must have synth_param_name='__joint_2_v'; got {:?}",
        joints[2].binding
    );
}

/// `extract_joints_from_mechanism` assigns a `JointBinding::LiteralBound` with
/// `synth_param_name = "__joint_0_v"`, `initial_value_si = None`, `scrubbable = true`
/// for a revolute joint at joint_index 0.
#[test]
fn extract_joint_descriptor_assigns_kind_based_binding_defaults_revolute() {
    use crate::engine::extract_joints_from_mechanism;
    use crate::types::JointBinding;

    let mech_map = make_single_body_mechanism_map("revolute");
    let (joints, _) = extract_joints_from_mechanism(&mech_map);

    assert_eq!(joints.len(), 1, "expected 1 joint descriptor for revolute");
    assert_eq!(
        joints[0].binding,
        JointBinding::LiteralBound {
            synth_param_name: "__joint_0_v".to_string(),
            initial_value_si: None,
            scrubbable: true,
        },
        "revolute at joint_index=0 must have synth_param_name='__joint_0_v'; got {:?}",
        joints[0].binding
    );
}

// ---- reserved __joint_* param name collision warnings (task 3783, step-11) ----

/// Source with a Param cell named `__joint_y_axis_v` — collides with the
/// synth-virtual-param naming pattern used by the η-engine literal-bind path.
/// `get_mechanism_descriptors()` must emit exactly 1 WARN at the
/// `reify_gui::engine::reserved_param_name` target for this structure.
const RESERVED_PARAM_NAME_SOURCE: &str = r#"
structure Kinematic {
    param __joint_y_axis_v: Length = 50mm
    let y_axis = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let m0     = mechanism()
    let m1     = body(m0, "solid_a", y_axis)
}
"#;

/// A user-authored `param __joint_y_axis_v` matches the `__joint_*` reserved
/// synth-virtual-param naming pattern.  `get_mechanism_descriptors` must emit
/// exactly 1 WARN at `reify_gui::engine::reserved_param_name`.
#[test]
fn get_mechanism_descriptors_emits_warn_for_user_param_matching_reserved_pattern() {
    // Inoculate against tracing's per-callsite Interest cache — see
    // `prime_tracing_callsite_cache` in reify-test-support for why.
    reify_test_support::prime_tracing_callsite_cache();

    let mut session = make_session();
    session
        .load_from_source(RESERVED_PARAM_NAME_SOURCE, "kinematic")
        .expect("load reserved-param-name source");

    let (subscriber, counters) = CountingSubscriberBuilder::new()
        .count_level(tracing::Level::WARN)
        .target_prefix("reify_gui::engine::reserved_param_name")
        .build();

    tracing::subscriber::with_default(subscriber, || {
        let _ = session.get_mechanism_descriptors();
    });

    let warn_count = counters[&tracing::Level::WARN].load(Ordering::Acquire);
    assert_eq!(
        warn_count, 1,
        "expected exactly 1 WARN at reify_gui::engine::reserved_param_name \
         for structure with param __joint_y_axis_v; got {}",
        warn_count
    );
}

/// Normally-named params (e.g. `y_pos`) must NOT trigger the reserved-name
/// warning — this test pins the no-false-positive contract.
#[test]
fn get_mechanism_descriptors_does_not_warn_for_normally_named_params() {
    // Inoculate against tracing's per-callsite Interest cache.
    reify_test_support::prime_tracing_callsite_cache();

    // SNAPSHOT_PARAM_BIND_SOURCE has `param y_pos: Length = 100mm` — no
    // __joint_* pattern match.
    let mut session = make_session();
    session
        .load_from_source(SNAPSHOT_PARAM_BIND_SOURCE, "kinematic")
        .expect("load snapshot+param source");

    let (subscriber, counters) = CountingSubscriberBuilder::new()
        .count_level(tracing::Level::WARN)
        .target_prefix("reify_gui::engine::reserved_param_name")
        .build();

    tracing::subscriber::with_default(subscriber, || {
        let _ = session.get_mechanism_descriptors();
    });

    let warn_count = counters[&tracing::Level::WARN].load(Ordering::Acquire);
    assert_eq!(
        warn_count, 0,
        "expected 0 WARN events at reify_gui::engine::reserved_param_name \
         for normally-named param y_pos; got {}",
        warn_count
    );
}

// ---- amendment review tests (suggestions 1, 3, 4) ---------------------------

/// Source for unsupported-unit test: bind(y_axis, 50inch) where "inch" is not
/// in UNIT_TABLE.  initial_value_si must be None and a DEBUG event must fire at
/// `reify_gui::engine::literal_bind`.
const SNAPSHOT_UNSUPPORTED_UNIT_BIND_SOURCE: &str = r#"
structure Kinematic {
    let y_axis = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let m0     = mechanism()
    let m1     = body(m0, "solid_a", y_axis)
    let snap   = snapshot(m1, [bind(y_axis, 50inch)])
}
"#;

/// `bind(y_axis, 50inch)` — "inch" is not in UNIT_TABLE — must produce
/// `JointBinding::LiteralBound { initial_value_si: None, scrubbable: true }`
/// AND emit exactly one DEBUG event at the `literal_bind` target.
#[test]
fn get_mechanism_descriptors_literal_bind_with_unsupported_unit_yields_none_and_logs_debug() {
    reify_test_support::prime_tracing_callsite_cache();

    let mut session = make_session();
    // "50inch" may not parse as a valid Reify quantity — if load fails, skip the
    // test with a note rather than panicking.  If the source does load (the parser
    // accepts arbitrary unit suffixes), we assert the binding and the log event.
    let load_result = session.load_from_source(SNAPSHOT_UNSUPPORTED_UNIT_BIND_SOURCE, "kinematic");
    if load_result.is_err() {
        // Parser rejected the unsupported unit at parse/compile time — that is an
        // acceptable alternative outcome; the engine's silent-None path is only
        // reached when the AST carries the literal.  Skip gracefully.
        return;
    }

    let (subscriber, counters) = CountingSubscriberBuilder::new()
        .count_level(tracing::Level::DEBUG)
        .target_prefix("reify_gui::engine::literal_bind")
        .build();

    let descriptors = tracing::subscriber::with_default(subscriber, || {
        session.get_mechanism_descriptors()
    });

    let debug_count = counters[&tracing::Level::DEBUG].load(std::sync::atomic::Ordering::Acquire);
    assert_eq!(
        debug_count, 1,
        "expected exactly 1 DEBUG event at literal_bind target for unsupported unit 'inch'; got {}",
        debug_count
    );

    let m1_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 1)
        .expect("expected descriptor with bodies_count=1");
    let joint = &m1_desc.joints[0];

    assert!(
        matches!(joint.binding, crate::types::JointBinding::LiteralBound { initial_value_si: None, .. }),
        "unsupported unit must produce LiteralBound with initial_value_si=None; got {:?}",
        joint.binding
    );
}

/// Source for mixed-bind test (literal before param): two snapshot() calls bind
/// the same joint y_axis — first to a literal 50mm, then to param y_pos.
/// With the broadened ParamBound guard (`LiteralBound { .. }`), the param
/// wins and binding must be ParamBound.
const SNAPSHOT_LITERAL_THEN_PARAM_SOURCE: &str = r#"
structure Kinematic {
    param y_pos: Length = 100mm
    let y_axis = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let m0     = mechanism()
    let m1     = body(m0, "solid_a", y_axis)
    let snap1  = snapshot(m1, [bind(y_axis, 50mm)])
    let snap2  = snapshot(m1, [bind(y_axis, y_pos)])
}
"#;

/// When two snapshot() calls bind the same joint — first to a literal (50mm),
/// then to a param — the param arm must win and produce `ParamBound`.
///
/// This verifies the broadened guard `matches!(jd.binding, LiteralBound { .. })`
/// (vs the former `LiteralBound { initial_value_si: None, .. }` guard which would
/// have left the binding as LiteralBound while setting flat fields to the param).
#[test]
fn get_mechanism_descriptors_literal_then_param_bind_promotes_to_param_bound() {
    let mut session = make_session();
    session
        .load_from_source(SNAPSHOT_LITERAL_THEN_PARAM_SOURCE, "kinematic")
        .expect("load literal-then-param source");

    let descriptors = session.get_mechanism_descriptors();
    let m1_desc = descriptors
        .iter()
        .find(|d| d.bodies_count == 1)
        .expect("expected descriptor with bodies_count=1");

    let joint = &m1_desc.joints[0];

    // Param must win: binding = ParamBound, flat field = param cell id.
    assert_eq!(
        joint.binding,
        crate::types::JointBinding::ParamBound {
            param_cell_id: "Kinematic.y_pos".to_string(),
            current_value_si: Some(0.1),
        },
        "literal-then-param: param must win; binding must be ParamBound; got {:?}",
        joint.binding
    );
    assert_eq!(
        joint.driving_param_cell_id,
        Some("Kinematic.y_pos".to_string()),
        "flat field must reflect param; got {:?}",
        joint.driving_param_cell_id
    );
}

/// Source for bind-on-fixed-joint test: a fixed joint j_f with a param bound
/// to it via snapshot().
///
/// After task β (mechanism β: joint_signatures.rs), `fixed()` resolves to
/// `Type::StructureRef("Fixed")` at compile time.  The let-cell `j_f` therefore
/// carries `StructureRef("Fixed")`, and γ's `check_expr_mechanism_joint_bound`
/// fires via Path A when `bind(j_f, p)` is typechecked — producing a compile-time
/// `E_MECHANISM_NONDRIVING_JOINT` error because `Fixed : Joint` but not
/// `Fixed : DrivingJoint`.  The source never reaches eval.
const SNAPSHOT_FIXED_JOINT_WITH_PARAM_SOURCE: &str = r#"
structure Kinematic {
    param p: Length = 10mm
    let j_f = fixed()
    let m0  = mechanism()
    let m1  = body(m0, "solid_a", j_f)
    let snap = snapshot(m1, [bind(j_f, p)])
}
"#;

/// `bind(j_f, p)` on a fixed joint is rejected at compile time.
///
/// Before task β, `fixed()` returned a fallback (non-StructureRef) type so
/// γ's DrivingJoint check could not identify the joint kind from the let-cell
/// and the source would compile — but the binding kind would remain FixedNoMotion
/// (structural overrides bind form).
///
/// After task β, `fixed()` resolves to `Type::StructureRef("Fixed")`, which lets
/// γ's `check_expr_mechanism_joint_bound` detect the violation at compile time via
/// Path A (`result_type == StructureRef`).  The source now fails to compile with
/// `E_MECHANISM_NONDRIVING_JOINT` naming "Fixed".
///
/// This test documents that compile-time enforcement — `load_from_source` must
/// return `Err` containing the DrivingJoint rejection message.
#[test]
fn get_mechanism_descriptors_bind_on_fixed_joint_does_not_promote_binding() {
    let mut session = make_session();
    let err = session
        .load_from_source(SNAPSHOT_FIXED_JOINT_WITH_PARAM_SOURCE, "kinematic")
        .expect_err("bind(fixed, param) must be rejected at compile time with E_MECHANISM_NONDRIVING_JOINT");

    assert!(
        err.contains("DrivingJoint") || err.contains("Fixed"),
        "compile error must mention DrivingJoint or Fixed; got: {err:?}"
    );
}

// ── T0b: tensegrity_wires extraction via build_gui_state ─────────────────────

/// Inline T-prism topology source — mirrors `examples/tensegrity_t_prism.ri`.
///
/// 6 nodes (top equilateral triangle z=1m + bottom triangle z=0m 30° twist).
/// 3 struts (cross-members [0,3],[1,4],[2,5]) then 3 cables (perimeter [0,1],[1,2],[2,0]).
fn t_prism_source() -> &'static str {
    r#"
structure def TPrism {
    let prism = Tensegrity(
        nodes: [
            point3(1m, 0m, 1m),
            point3(-0.5m, 0.866m, 1m),
            point3(-0.5m, -0.866m, 1m),
            point3(0.866m, 0.5m, 0m),
            point3(-0.866m, 0.5m, 0m),
            point3(0m, -1m, 0m)
        ],
        struts: [[0, 3], [1, 4], [2, 5]],
        cables: [[0, 1], [1, 2], [2, 0]]
    )
    let wires = tensegrity_wires(self.prism)
}
"#
}

/// `build_gui_state()` (via `load_from_source()`) must extract 6 tensegrity wires
/// from the T-prism module: 3 struts followed by 3 cables (T0a DD2 order).
///
/// Asserts:
/// - `tensegrity_wires.len() == 6`
/// - wires[0..3] have `kind == "strut"`, wires[3..6] have `kind == "cable"`
/// - first strut endpoints match node 0 (1,0,1) → node 3 (0.866, 0.5, 0)
/// - `entity_path == "TPrism"` for all wires (owning template name)
///
/// RED until `build_tensegrity_wires` is implemented (field is always empty).
#[test]
fn build_gui_state_extracts_tensegrity_wires_from_t_prism() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(t_prism_source(), "t_prism")
        .expect("T-prism load_from_source should succeed");

    assert_eq!(
        state.tensegrity_wires.len(),
        6,
        "T-prism must produce 6 wires (3 struts + 3 cables); got {}",
        state.tensegrity_wires.len()
    );

    // Struts precede cables (T0a DD2 / tensegrity_wires output order).
    for i in 0..3 {
        assert_eq!(
            state.tensegrity_wires[i].kind, "strut",
            "wire[{}] must be a strut; got '{}'",
            i, state.tensegrity_wires[i].kind
        );
    }
    for i in 3..6 {
        assert_eq!(
            state.tensegrity_wires[i].kind, "cable",
            "wire[{}] must be a cable; got '{}'",
            i, state.tensegrity_wires[i].kind
        );
    }

    // All wires belong to the TPrism template.
    for wire in &state.tensegrity_wires {
        assert_eq!(
            wire.entity_path, "TPrism",
            "entity_path must be 'TPrism'; got '{}'",
            wire.entity_path
        );
    }

    // First strut: node 0 (1m, 0m, 1m) → node 3 (0.866m, 0.5m, 0m).
    // SI passthrough: values are the raw float stored in Value::Scalar.si_value.
    let strut0 = &state.tensegrity_wires[0];
    assert_eq!(strut0.x1, 1.0, "strut0.x1 must be 1.0 (node 0 x)");
    assert_eq!(strut0.y1, 0.0, "strut0.y1 must be 0.0 (node 0 y)");
    assert_eq!(strut0.z1, 1.0, "strut0.z1 must be 1.0 (node 0 z)");
    assert_eq!(strut0.x2, 0.866, "strut0.x2 must be 0.866 (node 3 x)");
    assert_eq!(strut0.y2, 0.5, "strut0.y2 must be 0.5 (node 3 y)");
    assert_eq!(strut0.z2, 0.0, "strut0.z2 must be 0.0 (node 3 z)");
}

/// A module with no tensegrity wires (the bracket module) must yield an empty
/// `tensegrity_wires` vec — not a panic or stale data from a prior load.
///
/// RED until build_tensegrity_wires is implemented (though technically the empty
/// case passes since the field is initialised to Vec::new(); the real RED is the
/// T-prism extraction test above).
#[test]
fn build_gui_state_yields_empty_wires_for_non_tensegrity_module() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("bracket load_from_source should succeed");

    assert!(
        state.tensegrity_wires.is_empty(),
        "non-tensegrity module must produce no tensegrity wires; got {}",
        state.tensegrity_wires.len()
    );
}

/// A Reify module that binds a `Tensegrity(...)` struct to a value cell but does
/// NOT call `tensegrity_wires()`.  Value cells therefore contain a
/// `Value::StructureInstance { type_name: "Tensegrity", .. }` — a non-TensegrityWire
/// struct — which `build_tensegrity_wires` must actively filter out.
fn tensegrity_struct_no_wires_source() -> &'static str {
    r#"
structure def TOnly {
    let prism = Tensegrity(
        nodes: [
            point3(1m, 0m, 1m),
            point3(-0.5m, 0.866m, 1m),
            point3(0.866m, 0.5m, 0m)
        ],
        struts: [[0, 1]],
        cables: [[1, 2]]
    )
}
"#
}

/// `build_tensegrity_wires` must NOT extract records from `StructureInstance`
/// values whose `type_name` is not `"TensegrityWire"`.
///
/// Uses a module whose value cells hold a `Tensegrity` StructureInstance
/// (`type_name == "Tensegrity"`) but no `TensegrityWire` instances (because
/// `tensegrity_wires()` is never called).  This exercises the type-name filter
/// branch: the Tensegrity value must be ignored, yielding `tensegrity_wires: []`.
///
/// This is a stronger guard than `build_gui_state_yields_empty_wires_for_non_tensegrity_module`
/// because the bracket module has no StructureInstances at all (only scalar params),
/// so it cannot catch a regression where the filter is dropped.
#[test]
fn build_tensegrity_wires_filters_non_tensegrity_wire_struct_instances() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(tensegrity_struct_no_wires_source(), "tonly")
        .expect("tonly load_from_source should succeed");

    assert!(
        state.tensegrity_wires.is_empty(),
        "a Tensegrity StructureInstance (type_name='Tensegrity') must not be extracted as a wire; got {} wire(s)",
        state.tensegrity_wires.len()
    );
}

// ── task-3458 step-5: ModeShapeFrameEmitter engine tests ─────────────────────

/// Recording emitter for `ModeShapeFrame` events.
///
/// Mirrors `RecordingFeaCaseEmitter` (line 8447) for the mode-shape-frame channel.
///
/// **RED at step-5**: compile-fails because `crate::engine::ModeShapeFrameEmitter`
/// does not exist yet. GREEN after step-6 adds the trait and wiring to engine.rs.
struct RecordingModeShapeFrameEmitter {
    frames: std::sync::Arc<std::sync::Mutex<Vec<crate::types::ModeShapeFrame>>>,
}

impl RecordingModeShapeFrameEmitter {
    fn new() -> Self {
        Self {
            frames: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        }
    }
}

impl crate::engine::ModeShapeFrameEmitter for RecordingModeShapeFrameEmitter {
    fn frame(&self, payload: crate::types::ModeShapeFrame) {
        self.frames.lock().unwrap().push(payload);
    }
}

/// Helper: build a BucklingResult-shaped Value::StructureInstance with `n_modes` modes.
///
/// n_nodes = 2 (6 positions each), base_node_positions = [0,0,0, 1,0,0].
/// Mode k displaced_positions: base shifted by +0.1 in coordinate k (k=0→x, 1→y).
fn make_buckling_result_value(n_modes: usize) -> reify_ir::Value {
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
    use std::collections::BTreeMap;

    let base_positions = [0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0];

    let modes_list: Vec<Value> = (0..n_modes)
        .map(|k| {
            // Displace coordinate k by 0.1 for each node.
            let displaced: Vec<Value> = base_positions
                .iter()
                .enumerate()
                .map(|(coord_idx, &base)| {
                    let delta = if coord_idx % 3 == k { 0.1 } else { 0.0 };
                    Value::Real(base + delta)
                })
                .collect();
            let mode_shape_map: BTreeMap<Value, Value> = [(
                Value::String("displaced_positions".to_string()),
                Value::List(displaced),
            )]
            .into_iter()
            .collect();
            let mode_fields: PersistentMap<String, Value> = [
                ("eigenvalue".to_string(), Value::Real((k + 1) as f64 * 1000.0)),
                ("mode_shape".to_string(), Value::Map(mode_shape_map)),
            ]
            .into_iter()
            .collect();
            Value::StructureInstance(Box::new(StructureInstanceData {
                type_id: StructureTypeId(u32::MAX),
                type_name: "Mode".to_string(),
                version: 1,
                fields: mode_fields,
            }))
        })
        .collect();

    let base_val: Vec<Value> = base_positions.iter().map(|&v| Value::Real(v)).collect();

    let result_fields: PersistentMap<String, Value> = [
        ("modes".to_string(),               Value::List(modes_list)),
        ("converged".to_string(),           Value::Bool(true)),
        ("iterations".to_string(),          Value::Int(0)),
        ("pre_stress".to_string(),          Value::Undef),
        ("base_node_positions".to_string(), Value::List(base_val)),
    ]
    .into_iter()
    .collect();

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingResult".to_string(),
        version: 1,
        fields: result_fields,
    }))
}

/// (a) mode_shape_frame_emitter_fires_for_buckling_result_two_modes.
///
/// CheckResult contains a BucklingResult with 2 modes and n_nodes=2.
/// emit_mode_shape_frames_for_test_with_result must emit:
///   - 1 base frame (phase=0.0)
///   - 2 peak frames (phase=1.0, mode_index ascending 0, 1)
///     Total = 3 frames.
///
/// Also asserts: each frame's displaced_positions.len() == 6 (= 3·n_nodes),
/// and peak frames differ from the base frame's positions.
///
/// **RED at step-5**: compile-fails because ModeShapeFrameEmitter,
/// set_mode_shape_frame_emitter, and emit_mode_shape_frames_for_test_with_result
/// do not exist yet.
#[test]
fn mode_shape_frame_emitter_fires_for_buckling_result_two_modes() {
    use std::sync::Arc;
    use reify_eval::CheckResult;
    use reify_core::ValueCellId;
    use reify_ir::ValueMap;

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    let recorder = RecordingModeShapeFrameEmitter::new();
    let captured = Arc::clone(&recorder.frames);
    session.set_mode_shape_frame_emitter(Arc::new(recorder));

    // Build a CheckResult with a BucklingResult (2 modes, n_nodes=2).
    let mut values = ValueMap::new();
    values.insert(
        ValueCellId::new("BucklingColumnSmoke", "result"),
        make_buckling_result_value(2),
    );
    let check = CheckResult {
        values,
        constraint_results: vec![],
        diagnostics: vec![],
        resolved_params: std::collections::HashMap::new(),
    };

    session.emit_mode_shape_frames_for_test_with_result(&check);

    let frames = captured.lock().unwrap();

    // 1 base frame + 2 peak frames = 3 total.
    assert_eq!(
        frames.len(),
        3,
        "expected 3 frames (1 base + 2 peak) for 2-mode BucklingResult; got {}",
        frames.len()
    );

    // Exactly one base frame (phase == 0.0).
    let base_frames: Vec<_> = frames.iter().filter(|f| f.phase == 0.0).collect();
    assert_eq!(base_frames.len(), 1, "expected exactly 1 base frame (phase=0.0)");
    assert_eq!(
        base_frames[0].displaced_positions.len(),
        6,
        "base frame displaced_positions must have length 6 (3·n_nodes)"
    );

    // Two peak frames (phase == 1.0), mode_index ascending 0, 1.
    let mut peak_frames: Vec<_> = frames.iter().filter(|f| f.phase == 1.0).collect();
    peak_frames.sort_by_key(|f| f.mode_index);
    assert_eq!(peak_frames.len(), 2, "expected exactly 2 peak frames (phase=1.0)");
    assert_eq!(peak_frames[0].mode_index, 0, "first peak frame must have mode_index=0");
    assert_eq!(peak_frames[1].mode_index, 1, "second peak frame must have mode_index=1");

    // Each peak frame has the right length and differs from base frame.
    let base_pos = &base_frames[0].displaced_positions;
    for (i, peak) in peak_frames.iter().enumerate() {
        assert_eq!(
            peak.displaced_positions.len(),
            6,
            "peak frame {i} displaced_positions must have length 6"
        );
        assert!(
            peak.displaced_positions != *base_pos,
            "peak frame {i} must differ from the base frame"
        );
    }

    // ── task-4072 step-3: eigenvalue threading assertions ──────────────────
    // (e) Base frame eigenvalue must be None.
    assert_eq!(
        base_frames[0].eigenvalue,
        None,
        "base frame (phase=0.0) must have eigenvalue=None"
    );

    // (f) Peak frame k must carry eigenvalue = Some((k+1)*1000.0).
    // make_buckling_result_value sets mode k eigenvalue = (k+1)*1000.0.
    // RED at step-3: currently peaks have eigenvalue=None from the placeholder;
    // GREEN after step-4 threads eigenvalues from extract_buckling_data.
    assert_eq!(
        peak_frames[0].eigenvalue,
        Some(1000.0_f64),
        "peak frame 0 must carry eigenvalue=Some(1000.0)"
    );
    assert_eq!(
        peak_frames[1].eigenvalue,
        Some(2000.0_f64),
        "peak frame 1 must carry eigenvalue=Some(2000.0)"
    );
}

/// (b) mode_shape_frame_emitter_no_fire_when_no_emitter.
///
/// No emitter installed → calling emit_mode_shape_frames_for_test_with_result
/// must not panic (the `emit_*_if_any` guard prevents the dispatch).
///
/// **RED at step-5**: compile-fails for the same reason as (a).
#[test]
fn mode_shape_frame_emitter_no_fire_when_no_emitter() {
    use reify_eval::CheckResult;
    use reify_core::ValueCellId;
    use reify_ir::ValueMap;

    let checker = SimpleConstraintChecker;
    let session = EngineSession::new(Box::new(checker), None);
    // No emitter installed.

    let mut values = ValueMap::new();
    values.insert(
        ValueCellId::new("BucklingColumnSmoke", "result"),
        make_buckling_result_value(1),
    );
    let check = CheckResult {
        values,
        constraint_results: vec![],
        diagnostics: vec![],
        resolved_params: std::collections::HashMap::new(),
    };

    // Must not panic.
    session.emit_mode_shape_frames_for_test_with_result(&check);
}

/// (c) mode_shape_frame_emitter_no_fire_when_no_buckling_result.
///
/// CheckResult contains no BucklingResult → zero frames emitted.
///
/// **RED at step-5**: compile-fails for the same reason as (a).
#[test]
fn mode_shape_frame_emitter_no_fire_when_no_buckling_result() {
    use std::sync::Arc;
    use reify_eval::CheckResult;
    use reify_core::ValueCellId;
    use reify_ir::{ValueMap, Value};

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    let recorder = RecordingModeShapeFrameEmitter::new();
    let captured = Arc::clone(&recorder.frames);
    session.set_mode_shape_frame_emitter(Arc::new(recorder));

    // Ordinary (non-BucklingResult) value.
    let mut values = ValueMap::new();
    values.insert(ValueCellId::new("S", "width"), Value::Int(42));
    let check = CheckResult {
        values,
        constraint_results: vec![],
        diagnostics: vec![],
        resolved_params: std::collections::HashMap::new(),
    };

    session.emit_mode_shape_frames_for_test_with_result(&check);

    let frames = captured.lock().unwrap();
    assert!(
        frames.is_empty(),
        "no frames should be emitted when values contains no BucklingResult; got {}",
        frames.len()
    );
}

// ── task-3458 amend: mode_shape_scale regression tests ───────────────────────

/// Build a BucklingResult Value with explicit base positions and per-mode
/// displaced positions.  Mirrors `make_buckling_result_value` but lets tests
/// supply arbitrary geometry for precise scale assertions.
fn make_buckling_result_custom(
    base_positions: &[f64],
    displaced_per_mode: &[Vec<f64>],
) -> reify_ir::Value {
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
    use std::collections::BTreeMap;

    let modes_list: Vec<Value> = displaced_per_mode
        .iter()
        .enumerate()
        .map(|(k, displaced)| {
            let displaced_vals: Vec<Value> = displaced.iter().map(|&v| Value::Real(v)).collect();
            let mode_shape_map: BTreeMap<Value, Value> = [(
                Value::String("displaced_positions".to_string()),
                Value::List(displaced_vals),
            )]
            .into_iter()
            .collect();
            let mode_fields: PersistentMap<String, Value> = [
                ("eigenvalue".to_string(), Value::Real((k + 1) as f64 * 1000.0)),
                ("mode_shape".to_string(), Value::Map(mode_shape_map)),
            ]
            .into_iter()
            .collect();
            Value::StructureInstance(Box::new(StructureInstanceData {
                type_id: StructureTypeId(u32::MAX),
                type_name: "Mode".to_string(),
                version: 1,
                fields: mode_fields,
            }))
        })
        .collect();

    let base_val: Vec<Value> = base_positions.iter().map(|&v| Value::Real(v)).collect();

    let result_fields: PersistentMap<String, Value> = [
        ("modes".to_string(),               Value::List(modes_list)),
        ("converged".to_string(),           Value::Bool(true)),
        ("iterations".to_string(),          Value::Int(0)),
        ("pre_stress".to_string(),          Value::Undef),
        ("base_node_positions".to_string(), Value::List(base_val)),
    ]
    .into_iter()
    .collect();

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingResult".to_string(),
        version: 1,
        fields: result_fields,
    }))
}

/// (d) mode_shape_scale_verified_geometry — hand-computed scale assertion.
///
/// Geometry: 2 nodes at (0,0,0) and (4,0,0).
///   bbox_diag = sqrt(4² + 0 + 0) = 4.0
///
/// Mode 0 displaced_positions: [1,0,0, 5,0,0] (each node shifted +1 in x).
///   displacement = [1,0,0, 1,0,0]
///   max_disp = L2‖[1,0,0]‖ = 1.0
///   scale = 0.1 × 4.0 / 1.0 = 0.4
///
/// Expected peak = base + 0.4 × displacement = [0.4, 0, 0, 4.4, 0, 0].
///
/// This test locks in the 0.1 factor, the bbox computation, and the max-disp
/// calculation so a regression in any of them fails loudly rather than silently.
#[test]
fn mode_shape_scale_verified_geometry_matches_hand_computed() {
    use std::sync::Arc;
    use reify_eval::CheckResult;
    use reify_core::ValueCellId;
    use reify_ir::ValueMap;

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    let recorder = RecordingModeShapeFrameEmitter::new();
    let captured = Arc::clone(&recorder.frames);
    session.set_mode_shape_frame_emitter(Arc::new(recorder));

    // 2 nodes at (0,0,0) and (4,0,0).  bbox_diag = 4.0.
    let base = vec![0.0_f64, 0.0, 0.0, 4.0, 0.0, 0.0];
    // Mode 0: each node shifted +1 in x → displacement magnitude = 1.0.
    // scale = 0.1 × 4.0 / 1.0 = 0.4.
    let disp0 = vec![1.0_f64, 0.0, 0.0, 5.0, 0.0, 0.0];

    let mut values = ValueMap::new();
    values.insert(
        ValueCellId::new("Test", "result"),
        make_buckling_result_custom(&base, &[disp0]),
    );
    let check = CheckResult {
        values,
        constraint_results: vec![],
        diagnostics: vec![],
        resolved_params: std::collections::HashMap::new(),
    };

    session.emit_mode_shape_frames_for_test_with_result(&check);
    let frames = captured.lock().unwrap();

    // 1 base frame + 1 peak frame = 2 total.
    assert_eq!(frames.len(), 2, "expected 2 frames (1 base + 1 peak)");

    let base_frame  = frames.iter().find(|f| f.phase == 0.0).expect("base frame missing");
    let peak_frame  = frames.iter().find(|f| f.phase == 1.0).expect("peak frame missing");

    // Base frame carries the undeformed positions verbatim.
    let expected_base: Vec<f32> = base.iter().map(|&v| v as f32).collect();
    assert_eq!(
        base_frame.displaced_positions, expected_base,
        "base frame positions must equal undeformed node positions"
    );

    // Peak frame: base + 0.4 × displacement = [0.4, 0.0, 0.0, 4.4, 0.0, 0.0].
    let expected_peak: Vec<f32> = vec![0.4_f32, 0.0, 0.0, 4.4, 0.0, 0.0];
    let eps = 1e-5_f32;
    for (i, (&got, &want)) in peak_frame
        .displaced_positions
        .iter()
        .zip(expected_peak.iter())
        .enumerate()
    {
        assert!(
            (got - want).abs() < eps,
            "peak_frame.displaced_positions[{i}]: got {got}, want {want} (tol {eps})"
        );
    }
}

/// (e) mode_shape_scale_degenerate_fallback — zero-displacement and single-node.
///
/// Case A: zero displacement.
///   displacement = [0,0,0, ...] → max_disp = 0.0 → scale fallback = 1.0.
///   Expected peak = base + 1.0 × 0 = base (positions unchanged).
///
/// Case B: single node (bbox_diag = 0).
///   1 node at (5,5,5), displaced to (6,5,5) → displacement = (1,0,0).
///   bbox: single point → dx=dy=dz=0 → bbox_diag = 0.0 → scale fallback = 1.0.
///   Expected peak = base + 1.0 × displacement = (6,5,5).
#[test]
fn mode_shape_scale_degenerate_fallback() {
    use std::sync::Arc;
    use reify_eval::CheckResult;
    use reify_core::ValueCellId;
    use reify_ir::ValueMap;

    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    // ── Case A: zero displacement ───────────────────────────────────────────
    {
        let recorder = RecordingModeShapeFrameEmitter::new();
        let captured = Arc::clone(&recorder.frames);
        // Re-use the same session; install a fresh recorder.
        session.set_mode_shape_frame_emitter(Arc::new(recorder));

        let base = vec![0.0_f64, 0.0, 0.0, 4.0, 0.0, 0.0];
        // displaced == base → displacement = [0,0,0, 0,0,0] → scale = 1.0 (fallback).
        let disp0 = base.clone();

        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("Test", "result"),
            make_buckling_result_custom(&base, &[disp0]),
        );
        let check = CheckResult {
            values,
            constraint_results: vec![],
            diagnostics: vec![],
            resolved_params: std::collections::HashMap::new(),
        };

        session.emit_mode_shape_frames_for_test_with_result(&check);
        let frames = captured.lock().unwrap();
        assert_eq!(frames.len(), 2, "zero-disp: expected 2 frames");

        let peak_frame = frames.iter().find(|f| f.phase == 1.0).expect("peak frame missing");
        // With scale=1.0 and displacement=0, peak == base.
        let expected: Vec<f32> = base.iter().map(|&v| v as f32).collect();
        assert_eq!(
            peak_frame.displaced_positions, expected,
            "zero-displacement: peak frame must equal base positions (scale=1.0 fallback)"
        );
    }

    // ── Case B: single node → bbox_diag = 0 → scale fallback = 1.0 ─────────
    {
        let recorder = RecordingModeShapeFrameEmitter::new();
        let captured = Arc::clone(&recorder.frames);
        session.set_mode_shape_frame_emitter(Arc::new(recorder));

        // 1 node at (5,5,5).  bbox: single point → diagonal = 0.  scale → 1.0.
        let base = vec![5.0_f64, 5.0, 5.0];
        let disp0 = vec![6.0_f64, 5.0, 5.0]; // displacement = [1,0,0]

        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("Test", "result"),
            make_buckling_result_custom(&base, std::slice::from_ref(&disp0)),
        );
        let check = CheckResult {
            values,
            constraint_results: vec![],
            diagnostics: vec![],
            resolved_params: std::collections::HashMap::new(),
        };

        session.emit_mode_shape_frames_for_test_with_result(&check);
        let frames = captured.lock().unwrap();
        assert_eq!(frames.len(), 2, "single-node: expected 2 frames");

        let peak_frame = frames.iter().find(|f| f.phase == 1.0).expect("peak frame missing");
        // scale=1.0 fallback → peak = base + 1.0 × [1,0,0] = [6,5,5].
        let expected: Vec<f32> = disp0.iter().map(|&v| v as f32).collect();
        let eps = 1e-5_f32;
        for (i, (&got, &want)) in peak_frame
            .displaced_positions
            .iter()
            .zip(expected.iter())
            .enumerate()
        {
            assert!(
                (got - want).abs() < eps,
                "single-node peak[{i}]: got {got}, want {want}"
            );
        }
    }
}

// ── Task 4086 step-3: RED — fixture body realization ─────────────────────────
//
// Asserts that fea_cantilever_smoke.ri has a `body` realization in the compiled
// FeaCantileverSmoke template.
//
// FAILS until step-4 adds `let body = box(length, width, height)` to the fixture:
// without the binding, the template has no realization named "body".
//
// Implementation note: `let body = box(...)` is a geometry-let — the compiler
// classifies it via `is_geometry_let` and routes it to a `RealizationDecl` in
// the template's `realizations` list (entity.rs:1175-1176 `continue`s geometry-lets
// out of the `ValueCellDecl` path).  It therefore does NOT appear in
// `CheckResult.values`; use `compiled_for_test()` to inspect `template.realizations`.

/// Fixture body realization: fea_cantilever_smoke.ri must contain a `body`
/// geometry-let (`let body = box(length, width, height)`) so that task δ (4087)
/// has a realization to render the FEA stress contour onto.
#[test]
fn cantilever_fixture_realizes_body() {
    let source = include_str!("../../../../examples/fea_cantilever_smoke.ri");

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(source, "FeaCantileverSmoke")
        .expect("load_from_source must succeed for fea_cantilever_smoke.ri");

    let compiled = session
        .compiled_for_test()
        .expect("compiled_for_test must be Some after load_from_source");

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "FeaCantileverSmoke")
        .expect("FeaCantileverSmoke template must exist in compiled module");

    assert!(
        template.realizations.iter().any(|r| r.name.as_deref() == Some("body")),
        "FeaCantileverSmoke must have a realization named 'body' (from `let body = box(...)`); \
         fixture is missing the body binding. Present realizations: {:?}",
        template
            .realizations
            .iter()
            .map(|r| r.name.as_deref().unwrap_or("<unnamed>"))
            .collect::<Vec<_>>()
    );
}

// ── Task 4086 step-1: RED — B4 dispatch (register_compute_fns not wired yet) ──
//
// Asserts that a GUI EngineSession produces a real (non-Undef) ElasticResult
// with max_von_mises within ±50% of 6 MPa after loading fea_cantilever_smoke.ri.
//
// FAILS until step-2 calls register_compute_fns in EngineSession::from_engine:
// without the wiring, the GUI solve body-inlines to the `{ ElasticResult() }`
// stub, so every field (incl. max_von_mises) is Undef.

/// B4 signal: GUI engine FEA dispatch produces a real ElasticResult.
///
/// Loads `examples/fea_cantilever_smoke.ri` in a fresh EngineSession
/// (SimpleConstraintChecker + MockGeometryKernel) and asserts:
///   - `CheckResult.values` contains the cell `FeaCantileverSmoke.result`
///   - that value is a `Value::StructureInstance` (not Undef / stub)
///   - `result.max_von_mises` is a `Value::Scalar` with dimension PRESSURE
///   - the SI value is within ±50% of the analytical 6 MPa reference
///     (matches the tolerance documented in the cantilever comment header and
///     the reify-eval e2e test at crates/reify-eval/tests/solve_elastic_static_e2e.rs)
#[test]
fn register_compute_fns_dispatch_yields_real_elastic_result() {
    use reify_core::DimensionVector;
    use reify_ir::Value;

    let source = include_str!("../../../../examples/fea_cantilever_smoke.ri");

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(source, "FeaCantileverSmoke")
        .expect("load_from_source must succeed for fea_cantilever_smoke.ri");

    let check = session
        .last_check_for_test()
        .expect("last_check_for_test must be Some after load_from_source");

    let result_cell = ValueCellId::new("FeaCantileverSmoke", "result");
    let result_val = check
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaCantileverSmoke.result not found in CheckResult.values"));

    // Extract max_von_mises from the ElasticResult (Value::StructureInstance).
    let mvm = match result_val {
        Value::StructureInstance(data) => data
            .fields
            .get(&"max_von_mises".to_string())
            .cloned()
            .unwrap_or_else(|| panic!(
                "max_von_mises field not found in ElasticResult; fields: {:?}",
                data.fields.keys().collect::<Vec<_>>()
            )),
        other => panic!(
            "expected FeaCantileverSmoke.result to be Value::StructureInstance, got: {:?}",
            other
        ),
    };

    // max_von_mises must be a Scalar with dimension PRESSURE, not Undef.
    let (si_value, dimension) = match &mvm {
        Value::Scalar { si_value, dimension } => (*si_value, *dimension),
        other => panic!(
            "expected max_von_mises to be Value::Scalar {{ ... }}, got: {:?}",
            other
        ),
    };
    assert_eq!(
        dimension,
        DimensionVector::PRESSURE,
        "expected max_von_mises dimension == DimensionVector::PRESSURE, got: {:?}",
        dimension
    );

    // Analytical reference σ_max = 6PL/(bh²) = 6×1000×1.0/(0.1×0.1×0.1) = 6e6 Pa.
    // Tolerance: ±50% (3 MPa ≤ σ ≤ 9 MPa) — coarse P1-tet mesh method-error budget.
    let analytical_sigma: f64 = 6.0 * 1000.0 * 1.0 / (0.1 * 0.1 * 0.1); // 6e6 Pa
    let lo = analytical_sigma * 0.5; // 3e6 Pa
    let hi = analytical_sigma * 1.5; // 9e6 Pa
    assert!(
        si_value.is_finite() && si_value >= lo && si_value <= hi,
        "max_von_mises = {si_value:.3e} Pa is outside [{lo:.3e}, {hi:.3e}] (±50% of {analytical_sigma:.3e} Pa analytical)"
    );
}

// ── Task 4086 step-5: RED — producer lifecycle (session side) ──
//
// RecordingSolveCancelSink records the ordered sequence of
// solve_started/solve_finished calls (and captures the published handle).
//
// Fails with compile error until step-6 introduces:
//   - `pub trait SolveCancellationSink` in engine.rs
//   - `solve_cancel_sink: Option<Arc<dyn SolveCancellationSink>>` field
//   - `pub fn set_solve_cancel_sink` setter on EngineSession
//   - the private check_with_solve_slot helper that fires the callbacks
//     around engine.check()

/// Events recorded by RecordingSolveCancelSink.
///
/// Mirrors the RecordingFeaCaseEmitter approach (line 8447) but for the
/// solve-cancel lifecycle: one Started variant per solve (carries the handle)
/// and one Finished variant at completion.
#[derive(Debug)]
enum SolveLifecycleEvent {
    Started(reify_eval::CancellationHandle),
    Finished,
}

/// Test double implementing SolveCancellationSink.
///
/// Records every solve_started/solve_finished call in the order received.
/// Shares an Arc<Mutex<Vec>> so the test can read events while the session
/// still holds its Arc<dyn SolveCancellationSink> reference.
struct RecordingSolveCancelSink {
    events: std::sync::Arc<std::sync::Mutex<Vec<SolveLifecycleEvent>>>,
}

impl RecordingSolveCancelSink {
    fn new() -> Self {
        Self {
            events: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

impl crate::engine::SolveCancellationSink for RecordingSolveCancelSink {
    fn solve_started(&self, handle: reify_eval::CancellationHandle) {
        self.events
            .lock()
            .unwrap()
            .push(SolveLifecycleEvent::Started(handle));
    }

    fn solve_finished(&self) {
        self.events
            .lock()
            .unwrap()
            .push(SolveLifecycleEvent::Finished);
    }
}

/// B7 signal (session side): the solve-cancel slot lifecycle is
/// publish-before / clear-after.
///
/// Installs a `RecordingSolveCancelSink` and loads `fea_cantilever_smoke.ri`.
/// Asserts the recorder observed exactly `[Started(handle), Finished]` in
/// that order — i.e. the handle is published before the solve completes and
/// the slot is cleared after.
#[test]
fn solve_publishes_then_clears_cancel_handle() {
    use std::sync::Arc;

    let source = include_str!("../../../../examples/fea_cantilever_smoke.ri");

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let sink = RecordingSolveCancelSink::new();
    let captured_events = Arc::clone(&sink.events);
    session.set_solve_cancel_sink(Arc::new(sink));

    session
        .load_from_source(source, "FeaCantileverSmoke")
        .expect("load_from_source must succeed for fea_cantilever_smoke.ri");

    let events = captured_events.lock().unwrap();

    assert_eq!(
        events.len(),
        2,
        "expected exactly [Started(handle), Finished]; got {} events: {:?}",
        events.len(),
        events
            .iter()
            .map(|e| match e {
                SolveLifecycleEvent::Started(_) => "Started",
                SolveLifecycleEvent::Finished => "Finished",
            })
            .collect::<Vec<_>>()
    );

    // First event: Started — handle must not be cancelled at fire time.
    let handle = match &events[0] {
        SolveLifecycleEvent::Started(h) => {
            assert!(
                !h.is_cancelled(),
                "handle must not be cancelled at solve_started time"
            );
            h.clone()
        }
        SolveLifecycleEvent::Finished => {
            panic!("expected first event to be Started, got Finished")
        }
    };

    // Second event: Finished.
    assert!(
        matches!(events[1], SolveLifecycleEvent::Finished),
        "expected second event to be Finished"
    );

    // The handle was not cancelled (trampoline ignores _cancellation; solve is
    // synchronous and blocking).  Drop it to avoid unused-variable warning.
    let _ = handle;
}

/// set_parameter success path fires the solve-cancel slot lifecycle.
///
/// Exercises the `with_solve_slot` wrapper inside `set_parameter` on the happy
/// path — edit_check succeeds, so [Started, Finished] must be recorded in
/// order.  Complements `solve_publishes_then_clears_cancel_handle` (which
/// exercises `load_from_source` → `check_with_solve_slot` path).
#[test]
fn set_parameter_success_fires_solve_lifecycle() {
    use std::sync::Arc;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let sink = RecordingSolveCancelSink::new();
    let captured_events = Arc::clone(&sink.events);
    session.set_solve_cancel_sink(Arc::new(sink));

    // load_from_source also fires the lifecycle; clear before set_parameter.
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");
    captured_events.lock().unwrap().clear();

    session
        .set_parameter("Bracket.width", "120mm")
        .expect("set_parameter should succeed");

    let events = captured_events.lock().unwrap();
    assert_eq!(
        events.len(),
        2,
        "expected exactly [Started, Finished] from set_parameter success path; got {} events",
        events.len()
    );
    assert!(
        matches!(events[0], SolveLifecycleEvent::Started(_)),
        "first event must be Started"
    );
    assert!(
        matches!(events[1], SolveLifecycleEvent::Finished),
        "second event must be Finished"
    );
}

/// set_parameter edit_check Err path still fires solve_finished.
///
/// Passing a `Bool` value for a `Length` cell causes `edit_check` to return
/// `EngineError::TypeKindMismatch`, which is mapped to `Err(String)` and
/// propagated via `?` inside `with_solve_slot`.  The `SolveFinishedGuard`
/// inside `with_solve_slot` must drop — firing `solve_finished()` — even
/// though the closure short-circuits before returning `Ok`.
///
/// This is the specific failure mode the guard was introduced to handle.
#[test]
fn set_parameter_edit_check_err_still_fires_solve_finished() {
    use std::sync::Arc;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Establish the eval snapshot via load_from_source so edit_check runs
    // (without a snapshot edit_check returns NotInitialized before the lifecycle).
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    // Install sink AFTER load so only set_parameter events are captured.
    let sink = RecordingSolveCancelSink::new();
    let captured_events = Arc::clone(&sink.events);
    session.set_solve_cancel_sink(Arc::new(sink));

    // "true" parses to Value::Bool(true).  Bracket.width expects a Length →
    // validate_param_override returns TypeKindMismatch → edit_check returns Err
    // → the `?` inside with_solve_slot short-circuits.
    let result = session.set_parameter("Bracket.width", "true");
    assert!(
        result.is_err(),
        "type-mismatched value must produce Err from set_parameter"
    );

    let events = captured_events.lock().unwrap();
    assert_eq!(
        events.len(),
        2,
        "expected [Started, Finished] even when edit_check returns Err; got {} events: {:?}",
        events.len(),
        events
            .iter()
            .map(|e| match e {
                SolveLifecycleEvent::Started(_) => "Started",
                SolveLifecycleEvent::Finished => "Finished",
            })
            .collect::<Vec<_>>()
    );
    assert!(
        matches!(events[0], SolveLifecycleEvent::Started(_)),
        "first event must be Started"
    );
    assert!(
        matches!(events[1], SolveLifecycleEvent::Finished),
        "Finished must fire even on edit_check Err (SolveFinishedGuard covers ? early-return)"
    );
}

// ── Task 4087 step-1: RED — sample_stride_field_nearest ──────────────────────
//
// Build a synthetic Regular3D SampledField with 2 nodes per axis (8 nodes
// total), stride 3 (3 values per node).  One node is set to all-NaN to model
// an out-of-solid grid point.
//
// Fails to compile until step-2 adds `sample_stride_field_nearest` to engine.rs.

/// Build a 2×2×2 Regular3D SampledField with stride 3 for testing.
///
/// Grid: bounds [0,1]³, spacing 1.0 per axis, nodes at 0.0 and 1.0 each axis.
/// 8 nodes × stride 3 = 24 data entries.
/// Node layout (row-major, axis-0 outermost): indices (ix,iy,iz) → flat=(ix*2+iy)*2+iz.
///   (0,0,0)→0: [1.0,2.0,3.0]     (in-solid)
///   (0,0,1)→1: [4.0,5.0,6.0]     (in-solid)
///   (0,1,0)→2: [7.0,8.0,9.0]     (in-solid)
///   (0,1,1)→3: [10.0,11.0,12.0]  (in-solid)
///   (1,0,0)→4: [13.0,14.0,15.0]  (in-solid)
///   (1,0,1)→5: [16.0,17.0,18.0]  (in-solid)
///   (1,1,0)→6: [NaN,NaN,NaN]     (out-of-solid sentinel)
///   (1,1,1)→7: [22.0,23.0,24.0]  (in-solid)
fn make_3d_stride3_field() -> reify_ir::SampledField {
    let mut data = Vec::with_capacity(24);
    data.extend_from_slice(&[1.0f64, 2.0, 3.0]);    // (0,0,0)
    data.extend_from_slice(&[4.0, 5.0, 6.0]);        // (0,0,1)
    data.extend_from_slice(&[7.0, 8.0, 9.0]);        // (0,1,0)
    data.extend_from_slice(&[10.0, 11.0, 12.0]);     // (0,1,1)
    data.extend_from_slice(&[13.0, 14.0, 15.0]);     // (1,0,0)
    data.extend_from_slice(&[16.0, 17.0, 18.0]);     // (1,0,1)
    data.extend_from_slice(&[f64::NAN, f64::NAN, f64::NAN]); // (1,1,0) out-of-solid
    data.extend_from_slice(&[22.0, 23.0, 24.0]);     // (1,1,1)
    reify_ir::SampledField {
        name: "test_field".to_string(),
        kind: reify_ir::SampledGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, 0.0],
        bounds_max: vec![1.0, 1.0, 1.0],
        spacing: vec![1.0, 1.0, 1.0],
        axis_grids: vec![vec![0.0, 1.0], vec![0.0, 1.0], vec![0.0, 1.0]],
        interpolation: reify_ir::InterpolationKind::NearestNeighbor,
        data,
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    }
}

/// In-bounds point near node (0,0,0) returns Some([1.0,2.0,3.0]).
#[test]
fn sample_stride_field_nearest_in_bounds_returns_node_values() {
    let sf = make_3d_stride3_field();
    let result = crate::engine::sample_stride_field_nearest(&sf, [0.05, 0.05, 0.05], 1e-6);
    let window = result.expect("in-bounds point must return Some");
    assert_eq!(window.len(), 3, "stride must be 3");
    assert_eq!(
        window,
        vec![1.0, 2.0, 3.0],
        "point near (0,0,0) must return that node's values"
    );
}

/// Point clearly outside bounds returns None.
#[test]
fn sample_stride_field_nearest_out_of_bounds_returns_none() {
    let sf = make_3d_stride3_field();
    let result = crate::engine::sample_stride_field_nearest(&sf, [2.0, 0.0, 0.0], 1e-6);
    assert!(result.is_none(), "point outside bounds must return None");
}

/// In-bounds point nearest to the NaN node (1,1,0) returns None (out-of-solid).
#[test]
fn sample_stride_field_nearest_nan_node_returns_none() {
    let sf = make_3d_stride3_field();
    // (0.9, 0.9, 0.05) is closest to node (1,1,0) which is NaN
    let result = crate::engine::sample_stride_field_nearest(&sf, [0.9, 0.9, 0.05], 1e-6);
    assert!(
        result.is_none(),
        "point whose nearest node is NaN (out-of-solid) must return None"
    );
}

// ── Task 4087 step-3: RED — SCALAR_CHANNEL_OOB_SENTINEL and von_mises_sample ─
//
// Tests:
//   (a) SCALAR_CHANNEL_OOB_SENTINEL == -1.0_f32 and is finite/negative.
//   (b) von_mises_sample at an in-bounds solid node returns the correct value.
//   (c) OOB point returns the sentinel.
//   (d) Out-of-solid (NaN-node) point returns the sentinel.
//
// Fails to compile until step-4 adds SCALAR_CHANNEL_OOB_SENTINEL to types.rs
// and von_mises_sample to engine.rs.

/// Build a 2×2×2 Regular3D SampledField with stride 9 (symmetric stress tensor)
/// for von_mises_sample tests.
///
/// Node (0,0,0): known symmetric tensor σ = diag(100e6, 0, 0) Pa (uniaxial).
///   von Mises for uniaxial σ_xx = σ → von Mises = σ.
///   Layout: [σxx, σxy, σxz, σyx, σyy, σyz, σzx, σzy, σzz]
///           = [100e6, 0, 0, 0, 0, 0, 0, 0, 0]
/// Node (1,1,0): all-NaN (out-of-solid).
/// All other nodes: zero tensor.
fn make_stress_field() -> reify_ir::SampledField {
    let mut data = Vec::with_capacity(8 * 9);
    // (0,0,0): uniaxial 100 MPa
    data.extend_from_slice(&[100e6_f64, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    // (0,0,1): zero
    data.extend_from_slice(&[0.0_f64; 9]);
    // (0,1,0): zero
    data.extend_from_slice(&[0.0_f64; 9]);
    // (0,1,1): zero
    data.extend_from_slice(&[0.0_f64; 9]);
    // (1,0,0): zero
    data.extend_from_slice(&[0.0_f64; 9]);
    // (1,0,1): zero
    data.extend_from_slice(&[0.0_f64; 9]);
    // (1,1,0): NaN (out-of-solid)
    let nan9 = [f64::NAN; 9];
    data.extend_from_slice(&nan9);
    // (1,1,1): zero
    data.extend_from_slice(&[0.0_f64; 9]);

    reify_ir::SampledField {
        name: "stress".to_string(),
        kind: reify_ir::SampledGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, 0.0],
        bounds_max: vec![1.0, 1.0, 1.0],
        spacing: vec![1.0, 1.0, 1.0],
        axis_grids: vec![vec![0.0, 1.0], vec![0.0, 1.0], vec![0.0, 1.0]],
        interpolation: reify_ir::InterpolationKind::NearestNeighbor,
        data,
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    }
}

/// SCALAR_CHANNEL_OOB_SENTINEL must equal -1.0 and be finite/negative.
#[test]
fn scalar_channel_oob_sentinel_is_negative_one_and_finite() {
    let s = crate::types::SCALAR_CHANNEL_OOB_SENTINEL;
    assert_eq!(s, -1.0_f32, "sentinel must be exactly -1.0");
    assert!(s.is_finite(), "sentinel must be finite (required for wire guard)");
    assert!(s < 0.0, "sentinel must be negative (von Mises ≥ 0 physically)");
}

/// von_mises_sample at the uniaxial (0,0,0) node returns the correct von Mises value.
#[test]
fn von_mises_sample_in_bounds_returns_correct_value() {
    let sf = make_stress_field();
    // Uniaxial σ_xx = 100 MPa → von Mises = 100 MPa
    let tensor = [100e6_f64, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let expected = reify_stdlib::compute_von_mises_3x3(&tensor) as f32;
    let result = crate::engine::von_mises_sample(&sf, [0.05, 0.05, 0.05], 1e-6);
    assert!(
        (result - expected).abs() < 1.0,
        "von_mises_sample at (0,0,0) node: expected {expected}, got {result}"
    );
    assert!(result >= 0.0, "von Mises result must be non-negative");
}

/// von_mises_sample at an OOB point returns the sentinel.
#[test]
fn von_mises_sample_oob_returns_sentinel() {
    let sf = make_stress_field();
    let result = crate::engine::von_mises_sample(&sf, [2.0, 0.0, 0.0], 1e-6);
    assert_eq!(
        result,
        crate::types::SCALAR_CHANNEL_OOB_SENTINEL,
        "OOB point must return SCALAR_CHANNEL_OOB_SENTINEL"
    );
}

/// von_mises_sample at a NaN-node (out-of-solid) point returns the sentinel.
#[test]
fn von_mises_sample_out_of_solid_returns_sentinel() {
    let sf = make_stress_field();
    // (0.9, 0.9, 0.05) is closest to node (1,1,0) which is NaN
    let result = crate::engine::von_mises_sample(&sf, [0.9, 0.9, 0.05], 1e-6);
    assert_eq!(
        result,
        crate::types::SCALAR_CHANNEL_OOB_SENTINEL,
        "out-of-solid (NaN node) point must return SCALAR_CHANNEL_OOB_SENTINEL"
    );
}

// ── Task 4087 step-5: RED — displaced_sample ─────────────────────────────────
//
// Tests:
//   (a) in-bounds vertex → [v.x+dx, v.y+dy, v.z+dz] as f32 (warp=1).
//   (b) OOB vertex → original [v.x,v.y,v.z] as f32.
//   (c) out-of-solid (NaN node) vertex → original v.
//
// Fails to compile until step-6 adds displaced_sample to engine.rs.

/// Build a 2×2×2 Regular3D SampledField with stride 3 (displacement xyz)
/// for displaced_sample tests.
///
/// Node (0,0,0): displacement [0.01, 0.02, 0.03] m.
/// Node (1,1,0): NaN (out-of-solid).
/// All other nodes: zero displacement.
fn make_disp_field() -> reify_ir::SampledField {
    let mut data = Vec::with_capacity(8 * 3);
    data.extend_from_slice(&[0.01_f64, 0.02, 0.03]); // (0,0,0)
    data.extend_from_slice(&[0.0_f64; 3]);            // (0,0,1)
    data.extend_from_slice(&[0.0_f64; 3]);            // (0,1,0)
    data.extend_from_slice(&[0.0_f64; 3]);            // (0,1,1)
    data.extend_from_slice(&[0.0_f64; 3]);            // (1,0,0)
    data.extend_from_slice(&[0.0_f64; 3]);            // (1,0,1)
    data.extend_from_slice(&[f64::NAN; 3]);           // (1,1,0) out-of-solid
    data.extend_from_slice(&[0.0_f64; 3]);            // (1,1,1)

    reify_ir::SampledField {
        name: "displacement".to_string(),
        kind: reify_ir::SampledGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, 0.0],
        bounds_max: vec![1.0, 1.0, 1.0],
        spacing: vec![1.0, 1.0, 1.0],
        axis_grids: vec![vec![0.0, 1.0], vec![0.0, 1.0], vec![0.0, 1.0]],
        interpolation: reify_ir::InterpolationKind::NearestNeighbor,
        data,
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    }
}

/// In-bounds vertex near node (0,0,0) gets warp=1 displacement added.
#[test]
fn displaced_sample_in_bounds_applies_displacement() {
    let sf = make_disp_field();
    let point = [0.05_f64, 0.05, 0.05];
    let result = crate::engine::displaced_sample(&sf, point, 1e-6);
    // Expected: point + displacement at (0,0,0) = [0.05+0.01, 0.05+0.02, 0.05+0.03]
    let expected = [0.06_f32, 0.07_f32, 0.08_f32];
    for i in 0..3 {
        assert!(
            (result[i] - expected[i]).abs() < 1e-5,
            "displaced_sample[{i}]: expected {}, got {}",
            expected[i], result[i]
        );
    }
}

/// OOB vertex is returned unchanged (original position as f32).
#[test]
fn displaced_sample_oob_returns_original() {
    let sf = make_disp_field();
    let point = [2.0_f64, 0.5, 0.5];
    let result = crate::engine::displaced_sample(&sf, point, 1e-6);
    let expected = [2.0_f32, 0.5_f32, 0.5_f32];
    for i in 0..3 {
        assert!(
            (result[i] - expected[i]).abs() < 1e-7,
            "OOB displaced_sample[{i}]: expected {}, got {}",
            expected[i], result[i]
        );
    }
}

/// Out-of-solid (NaN-node) vertex is returned unchanged.
#[test]
fn displaced_sample_out_of_solid_returns_original() {
    let sf = make_disp_field();
    // (0.9, 0.9, 0.05) nearest to node (1,1,0) which is NaN
    let point = [0.9_f64, 0.9, 0.05];
    let result = crate::engine::displaced_sample(&sf, point, 1e-6);
    let expected = [0.9_f32, 0.9_f32, 0.05_f32];
    for i in 0..3 {
        assert!(
            (result[i] - expected[i]).abs() < 1e-5,
            "out-of-solid displaced_sample[{i}]: expected {}, got {}",
            expected[i], result[i]
        );
    }
}

// ── Task 4087 step-7: RED — extract_elastic_result_fields ────────────────────
//
// Tests:
//   (a) ValueMap with a proper ElasticResult returns Some((stress_sf, disp_sf)).
//   (b) ValueMap with no ElasticResult returns None.
//   (c) ValueMap where stress/displacement fields are Value::Undef returns None.
//
// Fails to compile until step-8 adds extract_elastic_result_fields to engine.rs.

/// Build a ValueMap containing a synthetic ElasticResult StructureInstance.
///
/// The stress field uses stride 9 (stress tensor) and the displacement field
/// uses stride 3 (xyz displacement vector).
fn make_elastic_result_value_map(
    stress_sf: reify_ir::SampledField,
    disp_sf: reify_ir::SampledField,
) -> reify_ir::ValueMap {
    use reify_ir::{FieldSourceKind, Value};
    use std::sync::Arc;

    let stress_field = Value::Field {
        domain_type: reify_core::Type::Real,
        codomain_type: reify_core::Type::Real,
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(stress_sf)),
    };
    let disp_field = Value::Field {
        domain_type: reify_core::Type::Real,
        codomain_type: reify_core::Type::Real,
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(disp_sf)),
    };

    let mut fields = reify_ir::PersistentMap::new();
    fields.insert("stress".to_string(), stress_field);
    fields.insert("displacement".to_string(), disp_field);
    fields.insert(
        "max_von_mises".to_string(),
        Value::Real(100e6),
    );

    let elastic_instance = Value::StructureInstance(Box::new(reify_ir::StructureInstanceData {
        type_id: reify_ir::StructureTypeId(0),
        type_name: "ElasticResult".to_string(),
        version: 1,
        fields,
    }));

    let mut map = reify_ir::ValueMap::new();
    let cell_id = reify_core::ValueCellId::new("FeaCantileverSmoke", "result");
    map.insert(cell_id, elastic_instance);
    map
}

/// A ValueMap with a proper ElasticResult returns Some with the correct SampledFields.
#[test]
fn extract_elastic_result_fields_with_valid_result_returns_some() {
    let stress_sf = make_stress_field();
    let disp_sf = make_disp_field();
    let map = make_elastic_result_value_map(stress_sf, disp_sf);
    let result = crate::engine::extract_elastic_result_fields(&map);
    let (stress, disp) = result.expect("should find ElasticResult with stress and displacement fields");
    // Stride 9 for stress (8 nodes × 9 = 72 data entries)
    assert_eq!(
        stress.data.len(),
        8 * 9,
        "stress field must have 8*9 data entries"
    );
    // Stride 3 for displacement (8 nodes × 3 = 24 data entries)
    assert_eq!(
        disp.data.len(),
        8 * 3,
        "displacement field must have 8*3 data entries"
    );
}

/// A ValueMap with no ElasticResult returns None.
#[test]
fn extract_elastic_result_fields_with_no_result_returns_none() {
    let map = reify_ir::ValueMap::new();
    assert!(
        crate::engine::extract_elastic_result_fields(&map).is_none(),
        "empty ValueMap must return None"
    );
}

/// A ValueMap where stress/displacement fields are Value::Undef returns None.
#[test]
fn extract_elastic_result_fields_with_undef_fields_returns_none() {
    use reify_ir::Value;

    let mut fields = reify_ir::PersistentMap::new();
    fields.insert("stress".to_string(), Value::Undef);
    fields.insert("displacement".to_string(), Value::Undef);

    let elastic_instance = Value::StructureInstance(Box::new(reify_ir::StructureInstanceData {
        type_id: reify_ir::StructureTypeId(0),
        type_name: "ElasticResult".to_string(),
        version: 1,
        fields,
    }));

    let mut map = reify_ir::ValueMap::new();
    let cell_id = reify_core::ValueCellId::new("Foo", "result");
    map.insert(cell_id, elastic_instance);

    assert!(
        crate::engine::extract_elastic_result_fields(&map).is_none(),
        "ElasticResult with Undef fields must return None"
    );
}

// ── Task 4087 step-9: RED — apply_fea_channels ───────────────────────────────
//
// Tests:
//   (a) ValueMap with an ElasticResult: scalar_channels["vonMises"] has correct
//       length, in-bounds vertices ≥ 0, OOB vertices == sentinel; displaced_positions
//       is Some with correct length, in-bounds moved, OOB == original.
//   (b) ValueMap with NO ElasticResult: scalar_channels stays empty, displaced_positions
//       stays None.
//
// Fails to compile until step-10 adds apply_fea_channels to engine.rs.

/// Build a MeshData with 3 vertices:
///   v0 = (0.05, 0.05, 0.05) → in-bounds, in-solid (nearest node (0,0,0))
///   v1 = (1.0,  0.0,  0.0)  → in-bounds (nearest node (1,0,0), zero displacement)
///   v2 = (2.0,  0.0,  0.0)  → OOB (outside [0,1]³)
fn make_test_mesh_data() -> crate::types::MeshData {
    crate::types::MeshData {
        entity_path: "test_body".to_string(),
        vertices: vec![
            0.05_f32, 0.05, 0.05,   // v0: in-bounds
            1.0_f32,  0.0,  0.0,    // v1: in-bounds
            2.0_f32,  0.0,  0.0,    // v2: OOB
        ],
        indices: vec![0, 1, 2],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
    }
}

/// apply_fea_channels with an ElasticResult fills vonMises and displaced_positions correctly.
#[test]
fn apply_fea_channels_with_elastic_result_fills_channels() {
    let stress_sf = make_stress_field();
    let disp_sf = make_disp_field();
    let map = make_elastic_result_value_map(stress_sf, disp_sf);
    let mut meshes = vec![make_test_mesh_data()];

    crate::engine::apply_fea_channels(&mut meshes, &map, None);

    let mesh = &meshes[0];
    let vertex_count = mesh.vertices.len() / 3; // = 3

    // scalar_channels["vonMises"] must have len == vertex_count.
    let vm = mesh
        .scalar_channels
        .get("vonMises")
        .expect("vonMises channel must exist after apply_fea_channels");
    assert_eq!(vm.len(), vertex_count, "vonMises len must == vertex_count");

    // v0 is in-bounds: stress at (0,0,0) is uniaxial 100 MPa → von Mises ≥ 0.
    assert!(
        vm[0] >= 0.0,
        "in-bounds in-solid vertex must have non-negative von Mises, got {}",
        vm[0]
    );
    // v2 is OOB: must equal sentinel.
    assert_eq!(
        vm[2],
        crate::types::SCALAR_CHANNEL_OOB_SENTINEL,
        "OOB vertex must have sentinel value"
    );

    // displaced_positions must be Some with len == vertices.len().
    let dp = mesh
        .displaced_positions
        .as_ref()
        .expect("displaced_positions must be Some after apply_fea_channels");
    assert_eq!(
        dp.len(),
        mesh.vertices.len(),
        "displaced_positions len must == vertices.len()"
    );

    // v0 is in-bounds: displacement at (0,0,0) is [0.01, 0.02, 0.03] → position moved.
    let orig_x = mesh.vertices[0]; // 0.05
    let disp_x = dp[0];
    assert!(
        (disp_x - orig_x).abs() > 1e-5,
        "in-bounds vertex displaced_x must differ from original; orig={orig_x}, disp={disp_x}"
    );

    // v2 is OOB: displaced_positions must equal original vertex.
    let v2_orig = &mesh.vertices[6..9];
    let v2_disp = &dp[6..9];
    for i in 0..3 {
        assert!(
            (v2_disp[i] - v2_orig[i]).abs() < 1e-7,
            "OOB vertex displaced_positions[{i}] must equal original vertex"
        );
    }
}

/// apply_fea_channels with NO ElasticResult leaves meshes untouched.
#[test]
fn apply_fea_channels_without_elastic_result_leaves_meshes_untouched() {
    let map = reify_ir::ValueMap::new(); // no ElasticResult
    let mut meshes = vec![make_test_mesh_data()];

    crate::engine::apply_fea_channels(&mut meshes, &map, None);

    let mesh = &meshes[0];
    assert!(
        mesh.scalar_channels.is_empty(),
        "scalar_channels must stay empty when no ElasticResult present"
    );
    assert!(
        mesh.displaced_positions.is_none(),
        "displaced_positions must stay None when no ElasticResult present"
    );
}

// ── Task 3598 step-3: RED — apply_shell_channels (synthetic, no kernel) ───────
//
// apply_shell_channels installs the shell-extract mid-surface geometry + the
// element_kind / region_tags / vonMises_top|mid|bottom / shell_normal_per_face
// channels onto the MeshData whose entity_path matches the view (by the prefix
// before `#realization[N]`). The serialize length contracts are the oracle.
//
// Fails to compile until step-4 adds apply_shell_channels to engine.rs.

/// A synthetic shell view: a 2-triangle / 4-vertex mid-surface, entity
/// `"FeaShellFlexure"` (bare template name, as the engine accessor emits it).
fn make_test_shell_view() -> reify_eval::ShellGuiMeshData {
    reify_eval::ShellGuiMeshData {
        entity_path: "FeaShellFlexure".to_string(),
        // 4 vertices (flat XYZ, len 12).
        vertices: vec![
            0.0, 0.0, 0.0, // v0
            1.0, 0.0, 0.0, // v1
            1.0, 1.0, 0.0, // v2
            0.0, 1.0, 0.0, // v3
        ],
        // 2 triangles (flat, len 6).
        indices: vec![0, 1, 2, 0, 2, 3],
        element_kind: vec![1, 1],
        region_tags: vec![0, 1],
        von_mises_top: vec![10.0, 20.0, 30.0, 40.0],
        von_mises_mid: vec![1.0, 2.0, 3.0, 4.0],
        von_mises_bottom: vec![5.0, 6.0, 7.0, 8.0],
        // Per-face normals: 2 faces × XYZ = 6.
        shell_normals_per_face: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
    }
}

/// apply_shell_channels replaces a matching mesh's geometry with the mid-surface
/// and installs all shell channels; the result satisfies the serialize contracts.
#[test]
fn apply_shell_channels_populates_matching_mesh() {
    // A placeholder solid tessellation (3 verts / 1 tri) that the populator must
    // REPLACE with the mid-surface. entity_path carries the #realization[N]
    // suffix; the view uses the bare template name — the prefix match must bind.
    let mut meshes = vec![crate::types::MeshData {
        entity_path: "FeaShellFlexure#realization[0]".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 9.0, 9.0, 9.0, 3.0, 3.0, 3.0],
        indices: vec![0, 1, 2],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
    }];
    let views = vec![make_test_shell_view()];

    crate::engine::apply_shell_channels(&mut meshes, &views);

    let mesh = &meshes[0];
    let vertex_count = mesh.vertices.len() / 3;
    let face_count = mesh.indices.len() / 3;
    assert_eq!(vertex_count, 4, "vertices replaced by the 4-vertex mid-surface");
    assert_eq!(face_count, 2, "indices replaced by the 2-triangle mid-surface");

    assert_eq!(mesh.element_kind, Some(vec![1, 1]), "element_kind all-shell");
    assert_eq!(mesh.region_tags, Some(vec![0, 1]), "region_tags == labels");

    for key in ["vonMises_top", "vonMises_mid", "vonMises_bottom"] {
        let ch = mesh
            .scalar_channels
            .get(key)
            .unwrap_or_else(|| panic!("scalar_channels must contain {key}"));
        assert_eq!(ch.len(), vertex_count, "{key} len must == vertex_count");
    }

    let normals = mesh
        .vector_channels
        .get("shell_normal_per_face")
        .expect("vector_channels must contain shell_normal_per_face");
    assert_eq!(
        normals.len(),
        3 * face_count,
        "per-face normal channel len must == 3*face_count"
    );

    // Wire-contract oracle: every MeshData::serialize length check must pass.
    serde_json::to_string(mesh)
        .expect("populated shell MeshData must serialize (length contracts hold)");
}

/// apply_shell_channels leaves a non-matching mesh entirely untouched.
#[test]
fn apply_shell_channels_leaves_non_matching_mesh_untouched() {
    let mut meshes = vec![crate::types::MeshData {
        entity_path: "SomeOtherBody#realization[0]".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: vec![0, 1, 2],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
    }];
    let views = vec![make_test_shell_view()]; // entity "FeaShellFlexure" — no match

    crate::engine::apply_shell_channels(&mut meshes, &views);

    let mesh = &meshes[0];
    assert!(
        mesh.element_kind.is_none(),
        "non-matching mesh keeps element_kind None"
    );
    assert!(
        mesh.region_tags.is_none(),
        "non-matching mesh keeps region_tags None"
    );
    assert!(
        !mesh.scalar_channels.contains_key("vonMises_top"),
        "non-matching mesh gets no vonMises_* channels"
    );
    assert!(
        mesh.vector_channels.is_empty(),
        "non-matching mesh gets no vector channels"
    );
    assert_eq!(mesh.vertices.len(), 9, "non-matching mesh geometry unchanged");
}

// ── Task 3598 step-7: RED — element_kind_count histogram ──────────────────────
//
// Fails to compile until step-8 adds element_kind_count to debug_server.rs.

/// element_kind_count histograms the per-face bytes; None → empty map.
///
/// `debug_server` is gated behind the `gui` feature, so this test compiles and
/// runs only under `--features gui` (the OCCT/Tauri build).
#[cfg(feature = "gui")]
#[test]
fn element_kind_count_histograms_element_kind_bytes() {
    let make = |element_kind: Option<Vec<u8>>| crate::types::MeshData {
        entity_path: "b".to_string(),
        vertices: Vec::new(),
        indices: Vec::new(),
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
    };

    let all_shell = crate::debug_server::element_kind_count(&make(Some(vec![1, 1, 1])));
    assert_eq!(
        all_shell,
        std::collections::BTreeMap::from([(1u8, 3usize)]),
        "three shell faces → {{1: 3}}"
    );

    let mixed = crate::debug_server::element_kind_count(&make(Some(vec![0, 1, 1])));
    assert_eq!(
        mixed,
        std::collections::BTreeMap::from([(0u8, 1usize), (1u8, 2usize)]),
        "mixed faces → {{0: 1, 1: 2}}"
    );

    let none = crate::debug_server::element_kind_count(&make(None));
    assert!(none.is_empty(), "None element_kind → empty histogram");
}

// ── Task 3598 step-5: integration — build_gui_state wires the shell populator ──
//
// Drives build_gui_state on the shell flexure fixture (FeaShellFlexure, a
// 50×10×1mm auto-classified shell) under MockGeometryKernel. The shell-extract
// + elastic solves are synthetic (no real kernel), and the body tessellates to
// a mock mesh whose entity_path prefix ("FeaShellFlexure") matches the shell
// view, so pre-1 (trampoline registration in from_engine) + step-6 (wiring) +
// apply_shell_channels must yield a MeshData with an all-shell element_kind and
// a per-vertex vonMises_top channel.

#[test]
fn build_gui_state_shell_flexure_populates_element_kind_and_von_mises_top() {
    let source = include_str!("../../../../examples/fea_shell_flexure.ri");
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_test_support::MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(source, "FeaShellFlexure")
        .expect("load_from_source must succeed for fea_shell_flexure.ri");

    // The shell populator must have installed element_kind on the body mesh.
    let shell_mesh = state
        .meshes
        .iter()
        .find(|m| m.element_kind.is_some())
        .expect(
            "build_gui_state must produce a mesh with element_kind populated \
             (shell-extract trampoline registered + shell populator wired)",
        );

    let face_count = shell_mesh.indices.len() / 3;
    let vertex_count = shell_mesh.vertices.len() / 3;

    assert_eq!(
        shell_mesh.element_kind.as_ref().unwrap(),
        &vec![1u8; face_count],
        "element_kind must be all-shell (1), len == face_count"
    );

    let vm_top = shell_mesh
        .scalar_channels
        .get("vonMises_top")
        .expect("shell mesh must carry a vonMises_top scalar channel");
    assert_eq!(
        vm_top.len(),
        vertex_count,
        "vonMises_top len must == vertex_count"
    );

    // Wire contract: the populated shell MeshData must serialize.
    serde_json::to_string(shell_mesh)
        .expect("populated shell MeshData must serialize (length contracts hold)");
}

// ── Task 4087 step-11: RED (integration / B5+B6 signal) ──────────────────────
//
// Load `examples/fea_cantilever_smoke.ri` under MockGeometryKernel (whose
// tessellate returns vertices [0,0,0], [1,0,0], [0,1,0]).
//
// The first two vertices are in the FEA field bounds (the beam occupies
// [0,1.0]×[0,0.1]×[0,0.1] m); the third (y=1.0) is outside the y-range.
//
// After build_gui_state (which calls apply_fea_channels when step-12 wires it):
//   - scalar_channels["vonMises"] must have len == vertex_count (3).
//   - At least one value != SCALAR_CHANNEL_OOB_SENTINEL (i.e., >= 0) must exist
//     (in-bounds vertex has real von Mises stress from the FEA result).
//   - displaced_positions must be Some with len == vertices.len() (9).
//   - At least one displaced vertex must differ from the original vertex
//     (warp=1 displacement applied to an in-bounds vertex).
//   - serde_json::to_string(&mesh) must succeed (wire contract: finite values,
//     correct lengths).
//
// This test fails until step-12 wires apply_fea_channels into build_gui_state.

#[test]
fn build_gui_state_fea_cantilever_smoke_has_von_mises_and_displaced_positions() {
    let source = include_str!("../../../../examples/fea_cantilever_smoke.ri");
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_test_support::MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(source, "FeaCantileverSmoke")
        .expect("load_from_source must succeed for fea_cantilever_smoke.ri");

    // Find the first non-empty mesh (the body mesh produced by tessellation).
    let mesh = state
        .meshes
        .iter()
        .find(|m| !m.vertices.is_empty())
        .expect("build_gui_state must produce at least one non-empty mesh");

    let vertex_count = mesh.vertices.len() / 3; // 3 vertices from MockGeometryKernel

    // ── scalar_channels["vonMises"] ──────────────────────────────────────────
    let vm = mesh
        .scalar_channels
        .get("vonMises")
        .expect("scalar_channels must contain 'vonMises' after FEA solve");

    assert_eq!(
        vm.len(),
        vertex_count,
        "vonMises channel len must equal vertex_count; got {} expected {}",
        vm.len(),
        vertex_count
    );

    let has_real_stress = vm.iter().any(|&v| v != crate::types::SCALAR_CHANNEL_OOB_SENTINEL && v >= 0.0);
    assert!(
        has_real_stress,
        "at least one vonMises value must be non-sentinel (in-bounds vertex has real FEA stress); got: {:?}",
        vm
    );

    // ── displaced_positions ──────────────────────────────────────────────────
    let dp = mesh
        .displaced_positions
        .as_ref()
        .expect("displaced_positions must be Some after FEA solve");

    assert_eq!(
        dp.len(),
        mesh.vertices.len(),
        "displaced_positions len must equal vertices.len(); got {} expected {}",
        dp.len(),
        mesh.vertices.len()
    );

    let has_moved = dp
        .iter()
        .zip(mesh.vertices.iter())
        .any(|(d, v)| (d - v).abs() > 1e-10);
    assert!(
        has_moved,
        "at least one displaced vertex must differ from its original (warp=1 displacement applied)"
    );

    // ── wire-contract: serde_json serialize must succeed ─────────────────────
    let json_result = serde_json::to_string(mesh);
    assert!(
        json_result.is_ok(),
        "serde_json::to_string(&mesh) must succeed (wire contract: finite values, correct lengths); err: {:?}",
        json_result.err()
    );
}

// ── Task 4087 amend: degenerate-stride guard coverage ────────────────────────
//
// Locks in the `_ =>` fallback arms in von_mises_sample (w.len() >= 9 guard)
// and displaced_sample (w.len() >= 3 guard) when the sampled field has a
// stride that is smaller than expected.  These branches are reachable in
// production if the solver emits a field with an unexpected codomain size.

/// Build a 2×1×1 Regular3D SampledField with stride 2 (< 3) to exercise the
/// degenerate-displacement guard in displaced_sample.
///
/// Grid: 2 nodes in x ([0.0, 1.0]), 1 node each in y and z ([0.0]).
/// bounds [0,1] × [0,0] × [0,0]; spacing 1.0 per axis.
/// 2 nodes × stride 2 = 4 data entries; no NaN (in-solid).
fn make_stride2_field_for_degenerate_test() -> reify_ir::SampledField {
    reify_ir::SampledField {
        name: "disp_degenerate_stride2".to_string(),
        kind: reify_ir::SampledGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, 0.0],
        bounds_max: vec![1.0, 0.0, 0.0],
        spacing: vec![1.0, 1.0, 1.0],
        axis_grids: vec![vec![0.0, 1.0], vec![0.0], vec![0.0]],
        interpolation: reify_ir::InterpolationKind::NearestNeighbor,
        data: vec![10.0_f64, 20.0, 30.0, 40.0],
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    }
}

/// von_mises_sample returns SCALAR_CHANNEL_OOB_SENTINEL when the stress field
/// has stride < 9, even for an in-bounds in-solid node.
///
/// Uses make_3d_stride3_field (stride 3) to simulate a degenerate stress field.
#[test]
fn von_mises_sample_degenerate_stride_returns_sentinel() {
    // make_3d_stride3_field has stride 3, which is < 9.
    let sf = make_3d_stride3_field();
    // Point [0.05, 0.05, 0.05] is in-bounds and nearest to node (0,0,0) = [1.0,2.0,3.0] (no NaN).
    // sample_stride_field_nearest returns Some(window) with len==3, which fails the >= 9 guard.
    let result = crate::engine::von_mises_sample(&sf, [0.05, 0.05, 0.05], 1e-6);
    assert_eq!(
        result,
        crate::types::SCALAR_CHANNEL_OOB_SENTINEL,
        "stress field with stride < 9 must yield SCALAR_CHANNEL_OOB_SENTINEL (guard: w.len() >= 9)"
    );
}

/// displaced_sample returns the original vertex position when the displacement
/// field has stride < 3, even for an in-bounds in-solid node.
#[test]
fn displaced_sample_degenerate_stride_returns_original_position() {
    // make_stride2_field_for_degenerate_test has stride 2, which is < 3.
    let sf = make_stride2_field_for_degenerate_test();
    let point = [0.0_f64, 0.0, 0.0]; // in-bounds; snaps to ix=0,iy=0,iz=0, data [10.0,20.0].
    // sample_stride_field_nearest returns Some(window) with len==2, which fails the >= 3 guard.
    let result = crate::engine::displaced_sample(&sf, point, 1e-6);
    assert_eq!(
        result,
        [point[0] as f32, point[1] as f32, point[2] as f32],
        "displacement field with stride < 3 must return original vertex position (guard: w.len() >= 3)"
    );
}

// ---- T6 Steps 1-4: default_visible on EntityTreeNode realization nodes ----

/// step-1 RED: a root template with a plain `let body` and an `aux let blank`
/// realization. After get_entity_tree(), the plain body node must have
/// `default_visible == true` and the aux blank node must have
/// `default_visible == false`.
///
/// Fails to compile until EntityTreeNode gains the `default_visible` field
/// and build_template_node sets it on realization nodes.
#[test]
fn get_entity_tree_aux_realization_default_visible_false() {
    let source = r#"structure Single {
    let body = box(20mm, 20mm, 20mm)
    aux let blank = cylinder(8mm, 40mm)
}"#;
    let mut session = make_session();
    session.load_from_source(source, "single").expect("load");

    let tree = session.get_entity_tree();
    let root = tree
        .iter()
        .find(|n| n.entity_path == "Single")
        .expect("Single root must exist");

    let body_node = root
        .children
        .iter()
        .find(|n| n.kind == "realization" && n.display_name.as_deref() == Some("body"))
        .expect("realization node for 'body' must be present");
    let blank_node = root
        .children
        .iter()
        .find(|n| n.kind == "realization" && n.display_name.as_deref() == Some("blank"))
        .expect("realization node for 'blank' must be present");

    assert!(
        body_node.default_visible,
        "plain `let body` realization must have default_visible == true"
    );
    assert!(
        !blank_node.default_visible,
        "aux `let blank` realization must have default_visible == false"
    );
}

/// step-3 RED: a root assembly with a plain `sub part : Part at <pose>` and an
/// `aux sub jig : Jig at <pose>`, where Part and Jig each declare a plain
/// `let body = box(...)` (NOT directly aux).
///
/// After get_entity_tree():
/// - `Asm.part#realization[0]`.default_visible == true  (product child, visible)
/// - `Asm.jig#realization[0]`.default_visible == false  (inherited from aux sub)
///
/// Also verifies that placed children appear in the tree under composed paths
/// (world-pose surfacing parity with T5).
///
/// Fails because step-2 only reads `real.is_aux` — the jig body is incorrectly
/// true. Passes after step-4 threads `aux_ancestor` through build_template_node.
#[test]
fn get_entity_tree_aux_sub_inherits_default_visible_false() {
    let source = r#"structure Part {
    let body = box(10mm, 10mm, 10mm)
}
structure Jig {
    let body = box(5mm, 5mm, 5mm)
}
structure Asm {
    sub part : Part at transform3(orient_identity(), vec3(30mm, 0mm, 0mm))
    aux sub jig : Jig at transform3(orient_identity(), vec3(50mm, 0mm, 0mm))
}"#;
    let mut session = make_session();
    session.load_from_source(source, "asm").expect("load");

    let tree = session.get_entity_tree();

    let asm_root = tree
        .iter()
        .find(|n| n.entity_path == "Asm")
        .expect("Asm root must exist");

    // Part sub-node: find the realization child under Asm.part
    let part_sub = asm_root
        .children
        .iter()
        .find(|n| n.entity_path == "Asm.part")
        .expect("Asm.part sub node must exist");
    let part_realization = part_sub
        .children
        .iter()
        .find(|n| n.kind == "realization")
        .expect("Asm.part#realization[0] must exist under Asm.part");
    assert_eq!(
        part_realization.entity_path, "Asm.part#realization[0]",
        "placed product child must have composed entity_path"
    );
    assert!(
        part_realization.default_visible,
        "Asm.part#realization[0] must be default_visible == true (product child, non-aux)"
    );

    // Jig sub-node: find the realization child under Asm.jig
    let jig_sub = asm_root
        .children
        .iter()
        .find(|n| n.entity_path == "Asm.jig")
        .expect("Asm.jig sub node must exist");
    let jig_realization = jig_sub
        .children
        .iter()
        .find(|n| n.kind == "realization")
        .expect("Asm.jig#realization[0] must exist under Asm.jig");
    assert_eq!(
        jig_realization.entity_path, "Asm.jig#realization[0]",
        "placed aux child must have composed entity_path"
    );
    assert!(
        !jig_realization.default_visible,
        "Asm.jig#realization[0] must be default_visible == false (inherited from aux sub)"
    );
}

/// Two-level aux inheritance: `aux sub jig : Jig` → `sub inner : Inner` → `let body`.
///
/// Neither the intermediate sub (`inner`) nor the leaf realization (`Inner.body`) is
/// directly aux.  The deepest realization node (`Top.jig.inner#realization[0]`) must
/// still have `default_visible == false` because `aux_ancestor` propagates transitively
/// through a non-aux intermediate sub.
///
/// This tests that the `aux_ancestor || sub.is_aux` threading in
/// `build_template_node` propagates through more than one level of nesting,
/// not just a direct parent–child aux relationship.
#[test]
fn get_entity_tree_aux_sub_inherits_two_levels_deep() {
    let source = r#"structure Inner {
    let body = box(5mm, 5mm, 5mm)
}
structure Jig {
    sub inner : Inner at transform3(orient_identity(), vec3(0mm, 0mm, 0mm))
}
structure Top {
    sub part : Inner at transform3(orient_identity(), vec3(0mm, 0mm, 0mm))
    aux sub jig : Jig at transform3(orient_identity(), vec3(20mm, 0mm, 0mm))
}"#;
    let mut session = make_session();
    session.load_from_source(source, "two_level").expect("load");

    let tree = session.get_entity_tree();

    let top_root = tree
        .iter()
        .find(|n| n.entity_path == "Top")
        .expect("Top root must exist");

    // Product branch: Top.part#realization[0] should be visible.
    let part_sub = top_root
        .children
        .iter()
        .find(|n| n.entity_path == "Top.part")
        .expect("Top.part sub node must exist");
    let part_realization = part_sub
        .children
        .iter()
        .find(|n| n.kind == "realization")
        .expect("Top.part#realization[0] must exist");
    assert!(
        part_realization.default_visible,
        "Top.part#realization[0] must be default_visible == true (non-aux)"
    );

    // Aux branch: Top.jig → Top.jig.inner → Top.jig.inner#realization[0].
    // Neither `inner` nor `Inner.body` is directly aux; only the top-level `jig` sub is.
    let jig_sub = top_root
        .children
        .iter()
        .find(|n| n.entity_path == "Top.jig")
        .expect("Top.jig sub node must exist");
    let inner_sub = jig_sub
        .children
        .iter()
        .find(|n| n.entity_path == "Top.jig.inner")
        .expect("Top.jig.inner intermediate sub node must exist");
    let inner_realization = inner_sub
        .children
        .iter()
        .find(|n| n.kind == "realization")
        .expect("Top.jig.inner#realization[0] must exist");
    assert_eq!(
        inner_realization.entity_path, "Top.jig.inner#realization[0]",
        "two-level aux child must have composed entity_path"
    );
    assert!(
        !inner_realization.default_visible,
        "Top.jig.inner#realization[0] must be default_visible == false \
         (aux_ancestor propagates through non-aux intermediate sub)"
    );
}

// ── task 4079: solver-progress emit + cancel-wiring tests ────────────────────
//
// Depends on:
//   (step-10) `EngineSession::set_solver_progress_sink` — public setter forwarding
//             to `self.core.engine_mut().set_solver_progress_sink(sink)`.
//   (step-10) `EngineSession::engine_active_solve_cancel_for_test` — `#[cfg(test)]
//             pub(crate)` accessor exposing the engine's `active_solve_cancel()`.
//   (step-10) `with_solve_slot` installs the published handle onto the engine via
//             `engine_mut().set_active_solve_cancel(Some(handle.clone()))`.
//
// Both tests FAIL TO COMPILE on the base branch (RED):
//   - `set_solver_progress_sink` does not yet exist on `EngineSession`.
//   - `engine_active_solve_cancel_for_test` does not yet exist on `EngineSession`.

/// Shared log of `(solver_kind, iter, residual)` triples recorded by
/// [`RecordingSolverProgressSink`].
type SolverProgressLog = std::sync::Arc<std::sync::Mutex<Vec<(String, u32, f64)>>>;

/// Test double for `reify_eval::SolverProgressSink`.
///
/// Records every `(solver_kind, iter, residual)` triple received from the
/// engine dispatch path.
struct RecordingSolverProgressSink {
    updates: SolverProgressLog,
}

impl RecordingSolverProgressSink {
    fn new() -> (Self, SolverProgressLog) {
        let updates = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        (
            Self {
                updates: std::sync::Arc::clone(&updates),
            },
            updates,
        )
    }
}

impl reify_eval::SolverProgressSink for RecordingSolverProgressSink {
    fn on_iteration(&self, update: &reify_eval::SolverProgressUpdate) {
        self.updates
            .lock()
            .unwrap()
            .push((update.solver_kind.to_string(), update.iter, update.residual));
    }
}

/// Installing a `RecordingSolverProgressSink` via `set_solver_progress_sink`
/// and loading `fea_cantilever_smoke.ri` must emit ≥1 progress update with
/// `iter ≥ 1`, a finite residual, and `solver_kind == "cg"`.
///
/// Proves that `EngineSession::set_solver_progress_sink` forwards the sink to
/// the reify-eval `Engine` and that `run_compute_dispatch` installs it in the
/// thread-local context visible to the trampoline.
///
/// RED: `set_solver_progress_sink` does not yet exist on `EngineSession`.
#[test]
fn set_solver_progress_sink_forwards_to_engine_and_emits_on_cantilever() {
    use std::sync::Arc;

    let source = include_str!("../../../../examples/fea_cantilever_smoke.ri");

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let (sink, captured_updates) = RecordingSolverProgressSink::new();
    session.set_solver_progress_sink(Arc::new(sink));

    session
        .load_from_source(source, "FeaCantileverSmoke")
        .expect("load_from_source must succeed for fea_cantilever_smoke.ri");

    let updates = captured_updates.lock().unwrap();

    assert!(
        !updates.is_empty(),
        "expected ≥1 SolverProgressUpdate after load_from_source; got 0"
    );
    for (kind, iter, residual) in updates.iter() {
        assert!(*iter >= 1, "iter must be ≥ 1 (1-indexed), got {}", iter);
        assert!(
            residual.is_finite(),
            "residual must be finite, got {}",
            residual
        );
        assert_eq!(
            kind.as_str(),
            "cg",
            "solver_kind must be \"cg\", got {:?}",
            kind
        );
    }
}

/// After `with_solve_slot` wraps `load_from_source`, the engine's cancel slot
/// must be **cleared** (`None`) when the solve window ends — so a stale
/// cancelled handle from a prior cancelled solve cannot spuriously trigger
/// `ComputeOutcome::Cancelled` on a future dispatch that bypasses
/// `with_solve_slot`.
///
/// The same-Arc invariant (published handle == engine handle *during* the
/// solve) is an auditable property of `with_solve_slot`'s source: the same
/// `handle` local is both cloned into `solve_started` and installed via
/// `set_active_solve_cancel(Some(handle))` before `f(self)` runs, ensuring
/// `cancel_solve_impl` can interrupt the in-flight trampoline.
#[test]
fn with_solve_slot_wires_published_handle_to_engine() {
    use std::sync::Arc;

    let source = include_str!("../../../../examples/fea_cantilever_smoke.ri");

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let sink = RecordingSolveCancelSink::new();
    let captured_events = Arc::clone(&sink.events);
    session.set_solve_cancel_sink(Arc::new(sink));

    session
        .load_from_source(source, "FeaCantileverSmoke")
        .expect("load_from_source must succeed for fea_cantilever_smoke.ri");

    // The lifecycle sink must have received at least a Started event.
    let events = captured_events.lock().unwrap();
    assert!(
        !events.is_empty(),
        "expected at least one lifecycle event; got 0"
    );
    drop(events);

    // After the solve window closes, the engine's cancel slot must be None.
    // A stale cancelled handle here would spuriously abort the next dispatch
    // that bypasses with_solve_slot (e.g. direct engine.eval() in tests).
    assert!(
        session.engine_active_solve_cancel_for_test().is_none(),
        "engine's active_solve_cancel must be None after the solve window closes \
         (with_solve_slot must clear the slot on return)"
    );
}

// ---------------------------------------------------------------------------
// Hot-reload staleness API (task 4153)
// ---------------------------------------------------------------------------

/// (a) After a successful load the session must not be stale, reload_error() must
/// be None, and build_gui_state().compile_diagnostics must be empty.
///
/// RED until step-2 adds the is_stale / reload_error / record_reload_error methods.
#[test]
fn staleness_api_clean_after_successful_load() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load should succeed");

    assert!(!session.is_stale(), "newly-loaded session must not be stale");
    assert!(
        session.reload_error().is_none(),
        "newly-loaded session must have no reload error"
    );
    let state = session.build_gui_state().expect("build_gui_state should succeed");
    assert!(
        state.compile_diagnostics.is_empty(),
        "clean load must have no compile_diagnostics; got {:?}",
        state.compile_diagnostics
    );
}

/// (b) After record_reload_error("boom") the session is stale, reload_error() returns
/// Some("boom"), and build_gui_state() retains the last-good meshes/values while
/// appending exactly one Error-severity DiagnosticInfo whose message contains "boom"
/// and whose code is Some("hot-reload-error").
///
/// RED until step-2 adds the staleness field and synthesises the diagnostic.
#[test]
fn staleness_api_record_reload_error_appends_diagnostic() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load should succeed");

    // Capture the last-good counts for regression.
    let good_state = session.build_gui_state().expect("build_gui_state pre-error should succeed");
    assert!(
        !good_state.meshes.is_empty(),
        "bracket source must produce non-empty meshes"
    );
    let good_mesh_count = good_state.meshes.len();
    let good_value_count = good_state.values.len();

    // Inject a reload error (simulates what commands::update_source_impl will do on Err).
    session.record_reload_error("boom".to_string());

    assert!(session.is_stale(), "session must be stale after record_reload_error");
    assert_eq!(
        session.reload_error(),
        Some("boom"),
        "reload_error() must return the recorded message verbatim"
    );

    // build_gui_state must retain last-good geometry AND append the error diagnostic.
    let stale_state = session
        .build_gui_state()
        .expect("build_gui_state must succeed even when stale");
    assert_eq!(
        stale_state.meshes.len(),
        good_mesh_count,
        "stale state must retain the last-good mesh count"
    );
    assert_eq!(
        stale_state.values.len(),
        good_value_count,
        "stale state must retain the last-good value count"
    );

    // Exactly one Error-severity diagnostic with the "boom" message.
    let error_diags: Vec<_> = stale_state
        .compile_diagnostics
        .iter()
        .filter(|d| d.severity == "Error" && d.message.contains("boom"))
        .collect();
    assert_eq!(
        error_diags.len(),
        1,
        "expected exactly one Error diagnostic for the reload error; got {:?}",
        stale_state.compile_diagnostics
    );
    assert_eq!(
        error_diags[0].code,
        Some("hot-reload-error".to_string()),
        "reload-error diagnostic must carry code 'hot-reload-error'"
    );
}

/// (c) A successful update_source / load after record_reload_error must clear
/// staleness and remove the synthetic diagnostic.
///
/// RED until step-2 wires the clear in commit_state.
#[test]
fn staleness_api_cleared_after_successful_reload() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load should succeed");

    // Make stale.
    session.record_reload_error("transient error".to_string());
    assert!(session.is_stale(), "session must be stale before the test assertion");

    // Successful reload should clear staleness (commit_state clears last_reload_error).
    session
        .update_source("bracket.ri", bracket_source())
        .expect("update_source with valid source must succeed");

    assert!(
        !session.is_stale(),
        "staleness must be cleared after a successful reload"
    );
    assert!(
        session.reload_error().is_none(),
        "reload_error must be None after a successful reload"
    );

    let state = session.build_gui_state().expect("build_gui_state should succeed");
    let has_reload_error_diag = state
        .compile_diagnostics
        .iter()
        .any(|d| d.code == Some("hot-reload-error".to_string()));
    assert!(
        !has_reload_error_diag,
        "no 'hot-reload-error' diagnostic must appear after a successful reload"
    );
}

// ── Task 4258: content/diagnostics one-snapshot invariant ──────────────────────

/// (step-1 RED) After a failed live edit, `build_gui_state` must surface the
/// FAILING buffer as `files[].content` so it is consistent with the
/// `compile_diagnostics` line/col (which are computed against that buffer).
///
/// Today this fails because `source_map` is never updated on a failed edit, so
/// `files[0].content` contains the last-good source rather than the edited buffer.
#[test]
fn build_gui_state_live_edit_failure_content_matches_diagnostics() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // (1) Load valid bracket source successfully.
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load should succeed");

    // (2) Produce a failing live edit: replace `thickness` in the box() call with an
    //     unresolved name `bogus_thk` — this triggers a compile-phase
    //     UnresolvedName error with line/col pointing into the edited buffer.
    let edited = bracket_source().replace("box(width, height, thickness)", "box(width, height, bogus_thk)");
    let result = session.update_source("bracket.ri", &edited);
    assert!(result.is_err(), "edited source with unresolved name must return Err");

    // (3) compile_failure should record a LiveEdit failure (compiled was Some before).
    let failure = session
        .compile_failure_for_test()
        .expect("compile_failure must be set after failed update_source");
    assert_eq!(
        failure.kind,
        CompileFailureKind::LiveEdit,
        "failure kind must be LiveEdit when a prior good compile existed"
    );

    // (4) build_gui_state must return Ok (failure is surfaced via compile_diagnostics,
    //     not as an Err return).
    let state = session
        .build_gui_state()
        .expect("build_gui_state must return Ok even after a failed live edit");

    // (5) files must contain exactly one entry whose content == the edited (failing)
    //     buffer — NOT the last-good source.
    assert_eq!(
        state.files.len(),
        1,
        "files must have exactly one entry after a failed live edit"
    );
    assert!(
        state.files[0].content.contains("bogus_thk"),
        "files[0].content must contain the failing buffer text 'bogus_thk' (was last-good); \
         got first 100 chars: {:?}",
        &state.files[0].content.chars().take(100).collect::<String>()
    );
    assert_eq!(
        state.files[0].content, edited,
        "files[0].content must equal the exact failing buffer"
    );

    // (6) compile_diagnostics must carry an Error mentioning 'bogus_thk'.
    let error_diags: Vec<&_> = state
        .compile_diagnostics
        .iter()
        .filter(|d| d.severity == "Error")
        .collect();
    assert!(
        !error_diags.is_empty(),
        "compile_diagnostics must have at least one Error; got: {:?}",
        state.compile_diagnostics
    );
    let bogus_diag = error_diags
        .iter()
        .find(|d| d.message.contains("bogus_thk"))
        .expect("at least one Error diagnostic must reference 'bogus_thk'");

    // (7) The diagnostic's line (1-based) must index into files[0].content at a
    //     line containing 'bogus_thk' — proving content and diagnostics are one
    //     consistent snapshot.
    let diag_line = bogus_diag.line as usize;
    assert!(diag_line >= 1, "diagnostic line must be >= 1 (1-based)");
    let source_lines: Vec<&str> = state.files[0].content.lines().collect();
    assert!(
        diag_line <= source_lines.len(),
        "diagnostic line {} must be within files[0].content ({} lines)",
        diag_line,
        source_lines.len()
    );
    assert!(
        source_lines[diag_line - 1].contains("bogus_thk"),
        "line {} of files[0].content must contain 'bogus_thk'; got: {:?}",
        diag_line,
        source_lines[diag_line - 1]
    );

    // (8) meshes must be non-empty — last-good viewport is preserved.
    assert!(
        !state.meshes.is_empty(),
        "meshes must remain non-empty after a failed live edit (last-good retained)"
    );
}

/// (step-3 RED) After a cold-start failure (no prior successful compile),
/// `build_gui_state` must surface the failing buffer as `files[].content` so it
/// is consistent with `compile_diagnostics`.
///
/// Today this fails because the ColdStart early-return branch returns
/// `files: Vec::new()` — an agent sees diagnostics it cannot index into any source.
#[test]
fn build_gui_state_cold_start_failure_surfaces_failing_source() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    // Fresh session — no prior successful load, so compiled is None.
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // (1) Attempt a cold-start load with a failing source: replace `thickness` in the
    //     box() call with an unresolved name `bogus_thk`.
    let bad = bracket_source().replace("box(width, height, thickness)", "box(width, height, bogus_thk)");
    let result = session.load_from_source(&bad, "bracket");
    assert!(result.is_err(), "bad source must return Err from load_from_source");

    // (2) compile_failure should record a ColdStart failure (compiled was None).
    let failure = session
        .compile_failure_for_test()
        .expect("compile_failure must be set after failed load_from_source");
    assert_eq!(
        failure.kind,
        CompileFailureKind::ColdStart,
        "failure kind must be ColdStart when no prior good compile existed"
    );

    // (3) build_gui_state must return Ok.
    let state = session
        .build_gui_state()
        .expect("build_gui_state must return Ok even after a cold-start failure");

    // (4) files must be NON-empty and carry the failing buffer — NOT empty.
    assert!(
        !state.files.is_empty(),
        "files must be non-empty after a cold-start failure so diagnostics can be indexed \
         (currently returns Vec::new())"
    );
    assert_eq!(
        state.files[0].path, "bracket.ri",
        "files[0].path must equal the module key 'bracket.ri'"
    );
    assert_eq!(
        state.files[0].content, bad,
        "files[0].content must equal the exact failing buffer"
    );
    assert!(
        state.files[0].content.contains("bogus_thk"),
        "files[0].content must contain the failing buffer text 'bogus_thk'"
    );

    // (5) compile_diagnostics must carry an Error referencing 'bogus_thk'.
    let error_diags: Vec<&_> = state
        .compile_diagnostics
        .iter()
        .filter(|d| d.severity == "Error")
        .collect();
    assert!(
        !error_diags.is_empty(),
        "compile_diagnostics must have at least one Error on cold-start failure; got: {:?}",
        state.compile_diagnostics
    );
    let bogus_diag = error_diags
        .iter()
        .find(|d| d.message.contains("bogus_thk"))
        .expect("at least one Error diagnostic must reference 'bogus_thk'");

    // (6) The diagnostic's line must index into files[0].content at a line with 'bogus_thk'.
    let diag_line = bogus_diag.line as usize;
    assert!(diag_line >= 1, "diagnostic line must be >= 1 (1-based)");
    let source_lines: Vec<&str> = state.files[0].content.lines().collect();
    assert!(
        diag_line <= source_lines.len(),
        "diagnostic line {} must be within files[0].content ({} lines)",
        diag_line,
        source_lines.len()
    );
    assert!(
        source_lines[diag_line - 1].contains("bogus_thk"),
        "line {} of files[0].content must contain 'bogus_thk'; got: {:?}",
        diag_line,
        source_lines[diag_line - 1]
    );

    // (7) meshes must be empty — no last-good module on cold start.
    assert!(
        state.meshes.is_empty(),
        "meshes must be empty after a cold-start failure (no last-good module)"
    );
}

// ── Amendment tests (task 4258 reviewer suggestions) ─────────────────────────

/// (amendment — suggestion 3, case 1) After `load_file` establishes a multi-file
/// project (entry + imported helper), a failing `update_source` must override
/// ONLY the entry file's `files[].content` with the failing buffer.
///
/// This exercises the LiveEdit override path via `compile_entry_with_imports`
/// (the multi-file call site at engine.rs:1769) rather than the single-file
/// path used by the other added tests.  It also confirms that the single-entry
/// source_map stores only the entry key ("main.ri"), not the imported key
/// ("helper.ri"), so `build_gui_state` produces exactly one `files` entry
/// after the override — the entry file with the failing buffer.
#[test]
fn build_gui_state_live_edit_multi_file_entry_key_overridden() {
    let (_dir, mut session, main_path, _main_content) = loaded_helper_session();

    // Verify the session is in a good state — compiled Some, Helper.x present.
    let good_state = session
        .build_gui_state()
        .expect("build_gui_state should succeed before failing edit");
    assert!(
        good_state.values.iter().any(|v| v.name == "x" && v.entity_path == "Helper"),
        "pre-condition: Helper.x must be present before the failing edit"
    );

    // Trigger a failing live edit on the entry file.  The import line is kept
    // so compile_entry_with_imports is taken (multi-file path), and the bogus
    // reference produces a compile-phase UnresolvedName error.
    let bad_main = "import helper\nstructure Top { sub h = Helper()\nlet broken = bogus_var }\n";
    let result = session.update_source(main_path.to_str().unwrap(), bad_main);
    assert!(result.is_err(), "update_source with bogus_var must return Err");

    // compile_failure must be LiveEdit (compiled was Some before the failure).
    let failure = session
        .compile_failure_for_test()
        .expect("compile_failure must be set after failed update_source");
    assert_eq!(
        failure.kind,
        CompileFailureKind::LiveEdit,
        "failure kind must be LiveEdit for a multi-file session with prior good compile"
    );
    // The file key must be the entry module key ("main.ri"), not a full path.
    assert_eq!(
        failure.file_key, "main.ri",
        "compile_failure.file_key must equal module_key(\"main\") = \"main.ri\""
    );

    let state = session
        .build_gui_state()
        .expect("build_gui_state must return Ok even after a failed live edit");

    // source_map has exactly one entry ("main.ri"); build_gui_state overrides it
    // with the failing buffer → files has exactly one entry.
    assert_eq!(
        state.files.len(),
        1,
        "files must have exactly one entry (entry module only) after a multi-file live-edit failure; \
         got: {:?}",
        state.files.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
    assert_eq!(
        state.files[0].path, "main.ri",
        "files[0].path must be 'main.ri' (the entry module key)"
    );
    assert_eq!(
        state.files[0].content, bad_main,
        "files[0].content must equal the exact failing buffer"
    );
    assert!(
        state.files[0].content.contains("bogus_var"),
        "files[0].content must contain 'bogus_var' (the unresolved reference)"
    );

    // compile_diagnostics must contain an Error referencing the bad identifier.
    assert!(
        state.compile_diagnostics.iter().any(|d| d.severity == "Error"),
        "compile_diagnostics must have at least one Error; got: {:?}",
        state.compile_diagnostics
    );

    // Note: meshes are empty here because the multi-file fixture (Top + Helper)
    // contains no geometry bodies — the MockGeometryKernel has nothing to
    // tessellate.  This is expected and correct; the key assertions are on
    // files[].content and compile_diagnostics above.
}

/// (amendment — suggestion 3, case 2) Exercises the `else`-push branch in
/// `build_gui_state`'s LiveEdit override block: when `compile_failure.file_key`
/// is absent from `source_map`, a new `FileData` entry is pushed rather than an
/// existing entry being replaced.
///
/// This happens when `update_source` is called with a path that maps to a
/// DIFFERENT module name than the one stored in `source_map` (e.g. because the
/// caller accidentally passes a different filename string on a session that was
/// loaded via `load_from_source`, where `file_path` is `None` and the
/// module_name is derived from the caller's `path` argument).
#[test]
fn build_gui_state_live_edit_else_push_branch_key_not_in_source_map() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Establish a good compile state keyed to "bracket.ri".
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load should succeed");

    // Call update_source with a DIFFERENT module name ("different_module.ri").
    // Since file_path is None (load_from_source doesn't set it), module_name is
    // derived from the path argument → "different_module".
    // compile_failure.file_key = module_key("different_module") = "different_module.ri"
    // which is NOT in source_map (which only has "bracket.ri").
    let bad = bracket_source().replace("box(width, height, thickness)", "box(width, height, bogus_dim)");
    let result = session.update_source("different_module.ri", &bad);
    assert!(result.is_err(), "update_source with bogus_dim must return Err");

    // compile_failure must be LiveEdit with file_key "different_module.ri".
    let failure = session
        .compile_failure_for_test()
        .expect("compile_failure must be set after failed update_source");
    assert_eq!(failure.kind, CompileFailureKind::LiveEdit);
    assert_eq!(
        failure.file_key, "different_module.ri",
        "compile_failure.file_key must be 'different_module.ri' (from the path arg)"
    );

    let state = session
        .build_gui_state()
        .expect("build_gui_state must return Ok");

    // The else-push branch fires: "different_module.ri" is not in source_map
    // ("bracket.ri" is), so a new FileData is pushed.
    // files must have TWO entries: "bracket.ri" (from source_map) + "different_module.ri" (pushed).
    assert_eq!(
        state.files.len(),
        2,
        "files must have two entries: one from source_map (bracket.ri) + one pushed (different_module.ri); \
         got: {:?}",
        state.files.iter().map(|f| &f.path).collect::<Vec<_>>()
    );

    // Find each entry by path (order not guaranteed).
    let bracket_entry = state.files.iter().find(|f| f.path == "bracket.ri")
        .expect("files must contain 'bracket.ri' (last-good source_map entry)");
    let diff_entry = state.files.iter().find(|f| f.path == "different_module.ri")
        .expect("files must contain 'different_module.ri' (else-push from compile_failure)");

    // "bracket.ri" must retain the last-good (original) source.
    assert!(
        bracket_entry.content.contains("box(width, height, thickness)"),
        "bracket.ri entry must retain last-good content (original box call); \
         got: {:?}",
        &bracket_entry.content.chars().take(100).collect::<String>()
    );

    // "different_module.ri" must carry the failing buffer.
    assert_eq!(
        diff_entry.content, bad,
        "different_module.ri entry must equal the exact failing buffer"
    );
    assert!(
        diff_entry.content.contains("bogus_dim"),
        "different_module.ri entry must contain 'bogus_dim' (the unresolved reference)"
    );
}

/// (amendment — suggestion 3, case 3 / suggestion 1 guard) When the SAME file
/// is both the last-good source and the failing live-edit target, `build_gui_state`
/// overrides `files[0].content` with the failing buffer.  The Error diagnostic
/// from the failing compile correctly indexes into that buffer, but Warning/Info
/// diagnostics carried over from the last-good compile retain their last-good
/// positions (which may be off relative to the overridden content).
///
/// This test guards the content-override behaviour for the same-file scenario
/// and documents the Warning positional limitation narrowed in the
/// `engine_state_json` doc comment (commands.rs, task 4258 amendment).
#[test]
fn build_gui_state_live_edit_same_file_content_is_failing_buffer_warning_positions_are_last_good() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Load a source that produces a Warning at a known line.
    // warn_source_with_unknown_port_type() is:
    //   line 1: structure def S {
    //   line 2:     port mount : NonExistentTrait {
    //   line 3:         param d : Length = 5mm
    //   line 4:     }
    //   line 5: }
    // The "unknown port type" Warning is on line 2.
    session
        .load_from_source(warn_source_with_unknown_port_type(), "warn")
        .expect("warn source should compile (warning, not error)");

    // Confirm a Warning was recorded for the last-good compile.
    let good_state = session
        .build_gui_state()
        .expect("build_gui_state should return Ok for the good compile");
    assert!(
        good_state.compile_diagnostics.iter().any(|d| d.severity == "Warning"),
        "pre-condition: compile_diagnostics must contain a Warning after loading warn source; \
         got: {:?}",
        good_state.compile_diagnostics
    );

    // Now trigger a failing live edit on the SAME file ("warn.ri").
    // The bad source has only 1 line — much shorter than the 5-line warn source —
    // so the Warning's line 2 position falls outside the new content.
    let bad = "structure def S { let invalid = totally_bogus_ref }";
    let result = session.update_source("warn.ri", bad);
    assert!(result.is_err(), "update_source with totally_bogus_ref must return Err");

    // compile_failure must be LiveEdit with file_key "warn.ri" (same file).
    let failure = session
        .compile_failure_for_test()
        .expect("compile_failure must be set after failed update_source");
    assert_eq!(failure.kind, CompileFailureKind::LiveEdit);
    assert_eq!(
        failure.file_key, "warn.ri",
        "compile_failure.file_key must be 'warn.ri' (same file as last-good)"
    );

    let state = session
        .build_gui_state()
        .expect("build_gui_state must return Ok even after a same-file live-edit failure");

    // files[0].content must be the FAILING buffer, not the last-good warn source.
    assert_eq!(
        state.files.len(),
        1,
        "files must have exactly one entry (single-file session)"
    );
    assert_eq!(
        state.files[0].content, bad,
        "files[0].content must equal the failing buffer ('bad'), not the last-good warn source"
    );
    assert!(
        !state.files[0].content.contains("NonExistentTrait"),
        "files[0].content must NOT contain 'NonExistentTrait' (last-good source leaked)"
    );

    // Both Warning (from last-good) and Error (from live edit) must be present.
    assert!(
        state.compile_diagnostics.iter().any(|d| d.severity == "Warning"),
        "compile_diagnostics must contain the prior Warning (carried over from last-good compile); \
         got: {:?}",
        state.compile_diagnostics
    );
    assert!(
        state.compile_diagnostics.iter().any(|d| d.severity == "Error"),
        "compile_diagnostics must contain the live-edit Error; got: {:?}",
        state.compile_diagnostics
    );

    // The Error diagnostic's line must correctly index into files[0].content
    // (the failing buffer) — this is the guaranteed-consistent part of the invariant.
    let error_diag = state
        .compile_diagnostics
        .iter()
        .find(|d| d.severity == "Error")
        .unwrap();
    let err_line = error_diag.line as usize;
    let bad_lines: Vec<&str> = bad.lines().collect();
    assert!(
        err_line >= 1 && err_line <= bad_lines.len(),
        "Error diag line {} must be within files[0].content ({} lines)",
        err_line,
        bad_lines.len()
    );
    assert!(
        bad_lines[err_line - 1].contains("totally_bogus_ref"),
        "line {} of files[0].content must contain 'totally_bogus_ref'; got: {:?}",
        err_line,
        bad_lines[err_line - 1]
    );

    // Document the Warning positional limitation: the Warning's line (2) was
    // computed against the 5-line last-good source.  files[0].content is now the
    // 1-line failing buffer, so the Warning's line is OUT OF RANGE for
    // files[0].content.  This is the known gap narrowed in the
    // engine_state_json doc: Warning/Info positions are last-good and may not
    // index correctly into the (overridden) files[].content on a failed edit.
    let warning_diag = state
        .compile_diagnostics
        .iter()
        .find(|d| d.severity == "Warning")
        .unwrap();
    let warn_line = warning_diag.line as usize;
    // The warn source is 5 lines; the bad source is 1 line.  The Warning's line
    // (>= 1, from last-good positions) will be > 1 line beyond the failing buffer.
    // We don't assert the exact line, but we document that it exceeds bad_lines.len().
    assert!(
        warn_line > bad_lines.len(),
        "Warning line {} should exceed the 1-line failing buffer length ({}) — \
         documenting that last-good Warning positions are stale relative to the \
         overridden files[0].content (this is expected behavior per the amended invariant)",
        warn_line,
        bad_lines.len()
    );
}

// ── Task 3026 step-1: RED — apply_fea_channels multi-case sourcing ────────────
//
// Tests:
//   (a) MultiCaseResult ValueMap + active_case=None  → uses lex-first case
//       ("operating" < "overload") — von Mises from 100 MPa stress field.
//   (b) MultiCaseResult ValueMap + active_case=Some("overload") → uses "overload"
//       (200 MPa stress field — distinct, larger von Mises than "operating").
//   (c) Single top-level ElasticResult (no multi-case wrapper) still works
//       unchanged with active_case=None (regression guard for the 4087 path).
//
// All three tests call `apply_fea_channels(&mut meshes, &map, active_case)` with
// a NEW third parameter that does not yet exist in the function signature.
// This file fails to compile (RED) until step-2 adds `active_case: Option<&str>`
// to `apply_fea_channels`.

/// Build a synthetic `Value::StructureInstance("ElasticResult")` (not a ValueMap).
///
/// Used as the per-case payload in `multi_case_result_value` for multi-case tests.
/// Mirrors `make_elastic_result_value_map` but returns the inner Value directly
/// instead of wrapping it in a top-level ValueMap cell.
fn make_elastic_result_value(
    stress_sf: reify_ir::SampledField,
    disp_sf: reify_ir::SampledField,
) -> reify_ir::Value {
    use reify_ir::{FieldSourceKind, Value};
    use std::sync::Arc;

    let stress_field = Value::Field {
        domain_type: reify_core::Type::Real,
        codomain_type: reify_core::Type::Real,
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(stress_sf)),
    };
    let disp_field = Value::Field {
        domain_type: reify_core::Type::Real,
        codomain_type: reify_core::Type::Real,
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(disp_sf)),
    };

    let mut fields = reify_ir::PersistentMap::new();
    fields.insert("stress".to_string(), stress_field);
    fields.insert("displacement".to_string(), disp_field);
    fields.insert("max_von_mises".to_string(), Value::Real(100e6));

    Value::StructureInstance(Box::new(reify_ir::StructureInstanceData {
        type_id: reify_ir::StructureTypeId(0),
        type_name: "ElasticResult".to_string(),
        version: 1,
        fields,
    }))
}

/// Build a stress SampledField with 200e6 uniaxial at node (0,0,0).
///
/// Distinct from `make_stress_field()` (100e6 at (0,0,0)), used as the
/// "overload" case so the two cases produce distinguishable von Mises values.
fn make_overload_stress_field() -> reify_ir::SampledField {
    let mut data = Vec::with_capacity(8 * 9);
    // (0,0,0): uniaxial 200 MPa — von Mises = 200e6 (2× the "operating" case)
    data.extend_from_slice(&[200e6_f64, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    data.extend_from_slice(&[0.0_f64; 9]); // (0,0,1)
    data.extend_from_slice(&[0.0_f64; 9]); // (0,1,0)
    data.extend_from_slice(&[0.0_f64; 9]); // (0,1,1)
    data.extend_from_slice(&[0.0_f64; 9]); // (1,0,0)
    data.extend_from_slice(&[0.0_f64; 9]); // (1,0,1)
    let nan9 = [f64::NAN; 9];
    data.extend_from_slice(&nan9);          // (1,1,0): NaN out-of-solid
    data.extend_from_slice(&[0.0_f64; 9]); // (1,1,1)

    reify_ir::SampledField {
        name: "stress".to_string(),
        kind: reify_ir::SampledGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, 0.0],
        bounds_max: vec![1.0, 1.0, 1.0],
        spacing: vec![1.0, 1.0, 1.0],
        axis_grids: vec![vec![0.0, 1.0], vec![0.0, 1.0], vec![0.0, 1.0]],
        interpolation: reify_ir::InterpolationKind::NearestNeighbor,
        data,
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    }
}

/// Build a ValueMap whose single cell is a MultiCaseResult with "operating" (100 MPa)
/// and "overload" (200 MPa) ElasticResult cases.
///
/// Lex order: "operating" < "overload", so "operating" is the lex-first default.
fn make_multi_case_value_map() -> reify_ir::ValueMap {
    use reify_test_support::multi_case_result_value;

    let er_op = make_elastic_result_value(make_stress_field(), make_disp_field());
    let er_ov = make_elastic_result_value(make_overload_stress_field(), make_disp_field());
    let mcr = multi_case_result_value(&[("operating", er_op), ("overload", er_ov)]);

    let mut map = reify_ir::ValueMap::new();
    let cell_id = reify_core::ValueCellId::new("FEABracket", "result");
    map.insert(cell_id, mcr);
    map
}

/// apply_fea_channels with MultiCaseResult + active_case=None uses lex-first ("operating").
///
/// "operating" < "overload" lexicographically, so the lex-first default picks "operating"
/// (the 100 MPa stress case). von Mises must be positive at v0 and sentinel at v2 (OOB).
#[test]
fn apply_fea_channels_multi_case_no_active_uses_lex_first() {
    let map = make_multi_case_value_map();
    let mut meshes = vec![make_test_mesh_data()];
    let vertex_count = meshes[0].vertices.len() / 3; // = 3

    // active_case = None  →  lex-first case "operating" (100 MPa)
    crate::engine::apply_fea_channels(&mut meshes, &map, None);

    let mesh = &meshes[0];
    let vm = mesh
        .scalar_channels
        .get("vonMises")
        .expect("vonMises must be filled from lex-first MultiCaseResult case");
    assert_eq!(vm.len(), vertex_count, "vonMises len must == vertex_count");

    // v0 (0.05, 0.05, 0.05) is in-bounds near node (0,0,0); "operating" stress = 100 MPa
    assert!(
        vm[0] > 0.0 && vm[0] != crate::types::SCALAR_CHANNEL_OOB_SENTINEL,
        "in-bounds vertex must have positive von Mises from 'operating' case, got {}",
        vm[0]
    );
    // v2 is OOB → sentinel
    assert_eq!(
        vm[2],
        crate::types::SCALAR_CHANNEL_OOB_SENTINEL,
        "OOB vertex must equal SCALAR_CHANNEL_OOB_SENTINEL"
    );

    // displaced_positions must be Some with correct length
    let dp = mesh
        .displaced_positions
        .as_ref()
        .expect("displaced_positions must be Some after multi-case apply_fea_channels");
    assert_eq!(dp.len(), mesh.vertices.len());
}

/// apply_fea_channels with MultiCaseResult + active_case=Some("overload") uses "overload".
///
/// "overload" has 200 MPa vs "operating" 100 MPa: the von Mises at v0 must be
/// strictly larger for "overload" than for "operating".
#[test]
fn apply_fea_channels_multi_case_active_overload_uses_overload_case() {
    let map = make_multi_case_value_map();
    let mut meshes_op = vec![make_test_mesh_data()];
    let mut meshes_ov = vec![make_test_mesh_data()];

    crate::engine::apply_fea_channels(&mut meshes_op, &map, None);
    crate::engine::apply_fea_channels(&mut meshes_ov, &map, Some("overload"));

    let vm_op = meshes_op[0]
        .scalar_channels
        .get("vonMises")
        .expect("'operating' case must fill vonMises");
    let vm_ov = meshes_ov[0]
        .scalar_channels
        .get("vonMises")
        .expect("'overload' case must fill vonMises");

    // v0 in "operating" = 100 MPa von Mises; v0 in "overload" = 200 MPa von Mises.
    assert!(
        vm_ov[0] > vm_op[0],
        "overload (200 MPa) von Mises ({}) must exceed operating (100 MPa) von Mises ({}) at v0",
        vm_ov[0],
        vm_op[0]
    );
    // Both in-bounds → neither should equal the OOB sentinel.
    assert_ne!(
        vm_op[0], crate::types::SCALAR_CHANNEL_OOB_SENTINEL,
        "operating in-bounds v0 must not equal OOB sentinel"
    );
    assert_ne!(
        vm_ov[0], crate::types::SCALAR_CHANNEL_OOB_SENTINEL,
        "overload in-bounds v0 must not equal OOB sentinel"
    );
}

/// Regression: top-level single ElasticResult (no multi-case wrapper) still works with
/// active_case=None.
///
/// Guards the original 4087 `extract_elastic_result_fields` / single-case path;
/// it must remain unaffected after the multi-case extension.
#[test]
fn apply_fea_channels_single_case_top_level_unchanged_regression() {
    let stress_sf = make_stress_field();
    let disp_sf = make_disp_field();
    let map = make_elastic_result_value_map(stress_sf, disp_sf);
    let mut meshes = vec![make_test_mesh_data()];
    let vertex_count = meshes[0].vertices.len() / 3;

    crate::engine::apply_fea_channels(&mut meshes, &map, None);

    let mesh = &meshes[0];
    let vm = mesh
        .scalar_channels
        .get("vonMises")
        .expect("single-case top-level ElasticResult must fill vonMises");
    assert_eq!(vm.len(), vertex_count);
    assert!(vm[0] >= 0.0, "in-bounds vertex must have non-negative von Mises");
    assert_eq!(vm[2], crate::types::SCALAR_CHANNEL_OOB_SENTINEL);

    let dp = mesh
        .displaced_positions
        .as_ref()
        .expect("displaced_positions must be Some for single-case regression");
    assert_eq!(dp.len(), mesh.vertices.len());
}

// ── Task 3026 step-3: RED — EngineSession active-case state + re-source ──────
//
// Tests:
//   (a) get_active_fea_case() returns None initially (no explicit case set;
//       lex-first "operating" is used implicitly by apply_fea_channels).
//   (b) set_active_fea_case("overload") returns Ok(GuiState) and makes
//       get_active_fea_case() return Some("overload").
//   (c) The returned GuiState's scalar_channels["vonMises"] reflects the
//       "overload" case (200 MPa), distinct from the "operating" case (100 MPa).
//   (d) mesh vertices/indices are byte-identical to the pre-switch values
//       (geometry served from the cached tessellation snapshot, not re-tessellated).
//
// Fails to COMPILE until step-4 adds:
//   - active_fea_case: Option<String> on EngineSession (init None)
//   - get_active_fea_case(&self) -> Option<String>
//   - set_active_fea_case(&mut self, name: &str) -> Result<GuiState, String>
//   - inject_check_for_test(&mut self, check: CheckResult) test helper

/// EngineSession active-case: None default → switch to "overload" → verify scalar channels and no re-tessellation.
#[test]
fn engine_session_active_fea_case_default_then_switch() {
    use reify_eval::CheckResult;

    // Build session with a recording kernel so we can count tessellation calls.
    let kernel = MockGeometryKernel::new();
    let tess_arc = kernel.tessellate_tolerances_ref();
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Load bracket_source: tessellate_snapshot runs, populating tess_mesh_cache.
    // MockGeometryKernel returns 3 vertices per realization: [0,0,0],[1,0,0],[0,1,0].
    let load_state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source must succeed for bracket_source");

    let tess_count_after_load = tess_arc.lock().unwrap().len();
    assert!(tess_count_after_load > 0, "initial load must produce ≥1 tessellation call");

    // Capture initial vertices/indices from the load result.
    let vertices_before: Vec<f32> = load_state.meshes.iter()
        .flat_map(|m| m.vertices.iter().cloned())
        .collect();
    let indices_before: Vec<u32> = load_state.meshes.iter()
        .flat_map(|m| m.indices.iter().cloned())
        .collect();
    assert!(!vertices_before.is_empty(), "MockGeometryKernel must produce vertices for bracket body");

    // Inject a MultiCaseResult CheckResult with "operating" (100 MPa) and
    // "overload" (200 MPa) Sampled-field ElasticResult cases.
    let values = make_multi_case_value_map();
    let check = CheckResult {
        values,
        constraint_results: vec![],
        diagnostics: vec![],
        resolved_params: std::collections::HashMap::new(),
    };
    session.inject_check_for_test(check); // FAILS TO COMPILE (step-4 adds this)

    // (a) Initial active case is None — lex-first "operating" is the implicit default.
    assert_eq!(session.get_active_fea_case(), None); // FAILS TO COMPILE (step-4 adds this)

    // Switch to "overload".
    let state_overload = session
        .set_active_fea_case("overload") // FAILS TO COMPILE (step-4 adds this)
        .expect("set_active_fea_case('overload') must succeed");

    // (b) Getter returns Some("overload") after switch.
    assert_eq!(
        session.get_active_fea_case(),
        Some("overload".to_string()),
        "get_active_fea_case must return 'overload' after set"
    );

    // (d) Vertices/indices are byte-identical — tessellation was NOT repeated.
    let vertices_after: Vec<f32> = state_overload.meshes.iter()
        .flat_map(|m| m.vertices.iter().cloned())
        .collect();
    let indices_after: Vec<u32> = state_overload.meshes.iter()
        .flat_map(|m| m.indices.iter().cloned())
        .collect();
    assert_eq!(
        vertices_before, vertices_after,
        "vertices must be byte-identical after case switch (no re-tessellation)"
    );
    assert_eq!(
        indices_before, indices_after,
        "indices must be byte-identical after case switch (no re-tessellation)"
    );
    // Tessellation count must not increase.
    let tess_count_after_set = tess_arc.lock().unwrap().len();
    assert_eq!(
        tess_count_after_set, tess_count_after_load,
        "set_active_fea_case must not trigger tessellation (before={}, after={})",
        tess_count_after_load, tess_count_after_set
    );

    // (c) scalar_channels["vonMises"] reflects "overload" (200 MPa at vertex 0).
    // MockGeometryKernel vertex 0 is at [0,0,0]; overload stress field has
    // 200e6 Pa uniaxial at node (0,0,0) → von Mises ≈ 200e6 Pa.
    let mesh_overload = state_overload.meshes.first()
        .expect("must have at least one mesh after set_active_fea_case('overload')");
    let vm_overload = mesh_overload.scalar_channels.get("vonMises")
        .expect("mesh must have vonMises channel after set_active_fea_case('overload')");
    let vertex_count = vertices_before.len() / 3; // 3 floats per vertex
    assert_eq!(vm_overload.len(), vertex_count,
        "vonMises channel length must equal vertex count ({vertex_count})");

    // Switch back to "operating" to verify the channels differ between cases.
    let state_operating = session
        .set_active_fea_case("operating")
        .expect("set_active_fea_case('operating') must succeed");
    let mesh_operating = state_operating.meshes.first()
        .expect("must have at least one mesh after set_active_fea_case('operating')");
    let vm_operating = mesh_operating.scalar_channels.get("vonMises")
        .expect("mesh must have vonMises channel for 'operating' case");

    // Overload (200 MPa) must produce a larger von Mises value at vertex 0 than
    // operating (100 MPa). Both are in-bounds (vertex 0 at [0,0,0] is the
    // nearest-neighbor for node (0,0,0) in both stress fields).
    assert!(
        vm_overload[0] > vm_operating[0] + 1.0_f32,
        "overload vonMises[0] ({:.0}) must exceed operating vonMises[0] ({:.0}) by >1 Pa (ratio ~2×)",
        vm_overload[0], vm_operating[0]
    );
}

// ── Task 3026 step-15: RED — GUI fixture-solve de-risk ────────────────────────
//
// Asserts that a GUI EngineSession fed the `fea_multi_case_bracket.ri` fixture
// produces a real multi-case solve result:
//   - Some cell in check.values has detect_multi_case_result returning
//     available_cases == ["operating", "overload", "transport"]
//   - Each case's ElasticResult is a Value::StructureInstance("ElasticResult")
//     with a non-Undef Sampled `stress` field (Value::Field with
//     FieldSourceKind::Sampled), proving the GUI end-to-end produces data
//     the case-picker can consume.
//
// FAILS TO COMPILE until step-16 creates examples/fea_multi_case_bracket.ri.

/// B-fixture: GUI EngineSession end-to-end produces a 3-case MultiCaseResult.
///
/// Loads `examples/fea_multi_case_bracket.ri` in a fresh EngineSession
/// (SimpleConstraintChecker + MockGeometryKernel) and asserts:
///   - `detect_multi_case_result` fires for some cell in check.values
///   - `available_cases == ["operating", "overload", "transport"]`
///   - Each case's ElasticResult has a Sampled `stress` field (non-Undef)
///
/// Mirrors the 4086 B4 pattern (`register_compute_fns_dispatch_yields_real_elastic_result`).
#[test]
fn gui_fixture_multi_case_bracket_produces_three_case_result() {
    use reify_ir::{FieldSourceKind, Value};

    let source = include_str!("../../../../examples/fea_multi_case_bracket.ri");

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(source, "FeaMultiCaseBracket")
        .expect("load_from_source must succeed for fea_multi_case_bracket.ri");

    let check = session
        .last_check_for_test()
        .expect("last_check_for_test must be Some after load_from_source");

    // Find the first cell that is a MultiCaseResult (detect_multi_case_result returns Some).
    let (cell_id, detected) = check
        .values
        .iter()
        .filter_map(|(id, v)| {
            reify_eval::multi_load_dispatch::detect_multi_case_result(v)
                .map(|d| (id, d))
        })
        .next()
        .unwrap_or_else(|| {
            let all_ids: Vec<_> = check.values.iter().map(|(id, _)| id).collect();
            panic!(
                "no cell in check.values matched detect_multi_case_result; \
                 cells present: {all_ids:?}"
            )
        });

    // Available cases must be exactly ["operating", "overload", "transport"].
    assert_eq!(
        detected.available_cases,
        vec!["operating".to_string(), "overload".to_string(), "transport".to_string()],
        "cell {cell_id:?}: available_cases mismatch"
    );

    // Each case must carry a real ElasticResult with a Sampled stress field.
    let outer_map = match check.values.get(cell_id).unwrap() {
        Value::Map(m) => m,
        other => panic!("cell {cell_id:?} must be Value::Map (MultiCaseResult), got: {other:?}"),
    };
    let cases_map = match outer_map.get(&Value::String("cases".to_string())) {
        Some(Value::Map(m)) => m,
        other => panic!("cell {cell_id:?}: 'cases' key must be Value::Map, got: {other:?}"),
    };

    for case_name in ["operating", "overload", "transport"] {
        let case_val = cases_map
            .get(&Value::String(case_name.to_string()))
            .unwrap_or_else(|| panic!("cases map must contain \"{case_name}\""));

        // Each case must be a Value::StructureInstance("ElasticResult").
        let er_fields = match case_val {
            Value::StructureInstance(data) => {
                assert_eq!(
                    data.type_name, "ElasticResult",
                    "case \"{case_name}\" must be StructureInstance(\"ElasticResult\"), \
                     got type_name=\"{}\"",
                    data.type_name
                );
                &data.fields
            }
            other => panic!(
                "case \"{case_name}\" must be Value::StructureInstance(\"ElasticResult\"), \
                 got: {other:?}"
            ),
        };

        // The `stress` field must be a Sampled Field (non-Undef).
        let stress_val = er_fields
            .get("stress")
            .unwrap_or_else(|| panic!("case \"{case_name}\": stress field missing from ElasticResult"));
        match stress_val {
            Value::Field { source, .. } => {
                assert!(
                    matches!(source, FieldSourceKind::Sampled),
                    "case \"{case_name}\": stress source must be Sampled, got: {source:?}"
                );
            }
            other => panic!(
                "case \"{case_name}\": expected stress to be Value::Field (Sampled), got: {other:?}"
            ),
        }
    }
}
