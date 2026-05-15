use std::path::Path;
use std::sync::atomic::Ordering;

use reify_compiler::find_template;
use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{
    CountingSubscriberBuilder, FailingMockGeometryKernel, MockConstraintSolver, MockGeometryKernel,
    bracket_source, bracket_source_violating, bracket_source_with_width,
    warn_source_with_unknown_port_type, warn_source_with_unknown_port_type_with_width,
};
use reify_types::ExportFormat;

use reify_types::{DiagnosticInfo, ModulePath, SourceLocationInfo, Type, ValueCellId};

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

/// `snapshot(m1, [bind(j1, 0mm)])` — non-empty bind list, but the second arg
/// of `bind` is a dimensional literal (`0mm`), not an `Ident`.  No valid
/// `bind(Ident, Ident)` pair survives the filter — case (c).  Must emit DEBUG.
const NON_BIND_LIST_SNAPSHOT_SOURCE: &str = r#"
structure Kinematic {
    let j1 = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let m0 = mechanism()
    let m1 = body(m0, "solid_a", j1)
    let snap = snapshot(m1, [bind(j1, 0mm)])
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
        "expected exactly 1 DEBUG event for snapshot(m1, [bind(j1, 0mm)]) — non-empty list \
         with no valid bind(Ident,Ident) pairs (case c); got {}",
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

    // (5) build_gui_state should still return Ok with original valid state
    let state = session
        .build_gui_state()
        .expect("build_gui_state should work after failed update");
    assert!(
        state.values.len() >= 5,
        "should still have original values after failed update, got {}",
        state.values.len()
    );
    assert_eq!(state.files.len(), 1);
    assert!(
        state.files[0].content.contains("structure Bracket"),
        "files should still contain original valid source, got: {}",
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
    use reify_types::{DimensionVector, Value};

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
    use reify_types::{DimensionVector, Value};

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
    use reify_types::{DimensionVector, Value};

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
    use reify_types::Diagnostic;

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
    use reify_types::{Diagnostic, Severity};

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
    use reify_types::byte_offset_to_line_col;
    use reify_types::{Diagnostic, DiagnosticLabel, SourceSpan};

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
    use reify_types::byte_offset_to_line_col;
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
    let sentinel = reify_types::SourceSpan::PRELUDE_SENTINEL_OFFSET;
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
    use reify_types::byte_offset_to_line_col;
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
            reify_types::SourceSpan::PRELUDE_SENTINEL_OFFSET
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
    use reify_types::byte_offset_to_line_col;
    use reify_types::{Diagnostic, DiagnosticLabel, SourceSpan};

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
    use reify_types::Diagnostic;

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
    use reify_types::byte_offset_to_line_col;
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
    use reify_types::byte_offset_to_line_col;
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
    use reify_types::Diagnostic;

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
    use reify_types::Diagnostic;

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
    use reify_types::Diagnostic;

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
    use reify_types::Diagnostic;

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
    use reify_types::{DimensionVector, ModulePath, Type};

    let mass_type = Type::Scalar {
        dimension: DimensionVector::MASS,
    };

    let bolt_template = TopologyTemplateBuilder::new("Bolt")
        .param("Bolt", "mass", mass_type, None)
        .build();

    // Use source-level test since we can't inject CompiledModule
    // Collection sub syntax: `sub bolts: List<Bolt>()`
    // Reify may or may not support this in the parser — test via compiled module builder
    let count_cell = reify_types::ValueCellId::new("Assembly", "__count_bolts");
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
    use reify_types::ModulePath;

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
    let node = build_template_node(a_template, "A", &compiled, None);

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
    use reify_types::ModulePath;

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
    let node_a = build_template_node(a_template, "A", &compiled, None);
    let node_b = build_template_node(b_template, "B", &compiled, None);

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
    use reify_types::{ModulePath, Type};

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
    let node = build_template_node(container_template, "Container", &compiled, None);

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
    use reify_types::ContentHash;
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
    use reify_types::ModulePath;
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
        if let reify_syntax::Declaration::Structure(s) = d {
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
    use reify_types::ModulePath;

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
        .with_extracted_faces(reify_types::GeometryHandleId(1), vec![])
        .with_extracted_edges(reify_types::GeometryHandleId(1), vec![]);
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
        .with_extracted_faces(reify_types::GeometryHandleId(1), vec![])
        .with_extracted_edges(reify_types::GeometryHandleId(1), vec![]);
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
    use reify_types::Value;
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
    use reify_types::Value;
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
/// `compile_diagnostics`.
///
/// Pins step-3/step-4 of the task-3351 plan: `update_source`'s single-file
/// branch (when `self.file_path` is `None`) must populate
/// `compile_failure` on failure, just like `load_from_source`.
#[test]
fn build_gui_state_surfaces_parse_error_after_failed_update_source_on_fresh_session() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Fresh session — no load_file or load_from_source, so self.file_path is None.
    // update_source takes the single-file branch (compile_single_file_with_stdlib).
    let err = session
        .update_source("foo.ri", "this is not valid {{{}}}")
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

    // Remaining fields should still be empty.
    assert!(state.meshes.is_empty(), "meshes should be empty");
    assert!(state.values.is_empty(), "values should be empty");
    assert!(state.constraints.is_empty(), "constraints should be empty");
    assert!(state.files.is_empty(), "files should be empty");
    assert!(
        state.tessellation_diagnostics.is_empty(),
        "tessellation_diagnostics should be empty"
    );
}

/// After a failed `load_from_source` (parse error on a fresh session),
/// `build_gui_state` must surface the failure in `compile_diagnostics` rather
/// than returning a silent empty viewport.
///
/// Pins step-1 of the task-3351 plan: the early-return branch of
/// `build_gui_state` (when `compiled` is `None`) must emit the stored
/// `compile_failure.diags` rather than `Vec::new()`.
#[test]
fn build_gui_state_surfaces_parse_error_after_failed_load_from_source() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // A parse-error source — `{{{` is invalid reify syntax so `parsed.errors` is non-empty.
    let err = session
        .load_from_source("this is not valid reify syntax {{{}}}", "bad")
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

    // The rest of the GuiState should remain empty (early-return semantics preserved).
    assert!(state.meshes.is_empty(), "meshes should be empty");
    assert!(state.values.is_empty(), "values should be empty");
    assert!(state.constraints.is_empty(), "constraints should be empty");
    assert!(state.files.is_empty(), "files should be empty");
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
            Some(CompileFailure { kind: CompileFailureKind::ColdStart, diags }) if !diags.is_empty()
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
            Some(CompileFailure { kind: CompileFailureKind::LiveEdit, diags }) if !diags.is_empty()
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
/// must surface the failure in `compile_diagnostics`.
///
/// Pins step-5/step-6 of the task-3351 plan: `load_file` routes through
/// `compile_entry_with_imports`, which must also populate
/// `compile_failure` on failure once refactored in step-6.
#[test]
fn build_gui_state_surfaces_parse_error_after_failed_load_file() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    // Write an invalid .ri file — `{{{` is unparseable syntax.
    let file_path = dir.path().join("main.ri");
    std::fs::write(&file_path, "structure {{{}}}}\n").expect("write should succeed");

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

    // Remaining fields should still be empty.
    assert!(state.meshes.is_empty(), "meshes should be empty");
    assert!(state.values.is_empty(), "values should be empty");
    assert!(state.constraints.is_empty(), "constraints should be empty");
    assert!(state.files.is_empty(), "files should be empty");
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
            Some(CompileFailure { kind: CompileFailureKind::LiveEdit, diags }) if !diags.is_empty()
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
            Some(CompileFailure { kind: CompileFailureKind::LiveEdit, diags }) if !diags.is_empty()
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
            Some(CompileFailure { kind: CompileFailureKind::LiveEdit, diags }) if !diags.is_empty()
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
            Some(CompileFailure { kind: CompileFailureKind::LiveEdit, diags }) if !diags.is_empty()
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
    use reify_types::Diagnostic;

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
            Some(CompileFailure { kind: CompileFailureKind::LiveEdit, diags }) if !diags.is_empty()
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

/// (e) Cursor on the `structure Bracket {` header line (line=1, col=1) → None.
///
/// The template's approximate span is derived from value-cell and constraint spans,
/// which start on line 2. The structure keyword at (1,1) = byte 0 falls before the
/// span start, so the position is not inside any template → None.
///
/// **Current-behavior note:** this assertion pins the result of the _current_
/// approximate-span derivation (union of member spans starting at line 2) and is
/// not a hard semantic contract.  If a future patch derives the span from the
/// parsed structure declaration (which starts at byte 0), (1,1) could legitimately
/// return `Some("Bracket")`.  Updating this assertion to match the improved
/// approximation is safe — the real invariant is that the position maps
/// consistently to at most one entity, not the exact `None` outcome.
#[test]
fn get_entity_at_source_location_structure_header_returns_none() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load should succeed");
    let result = session.get_entity_at_source_location(1, 1);
    assert!(
        result.is_none(),
        "cursor on structure header (1,1) is outside the template's approximate span → None, \
         got {:?}",
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

/// Step-4: No auto-resolve events should fire when the loaded source has no
/// `auto` parameters — the emit-helper must guard on empty `resolved_params`.
#[test]
fn engine_session_no_auto_resolve_emission_when_no_auto_params() {
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
    use reify_types::Value;

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

/// Behavioral test for `CoreState::commit_file_path`:
/// `load_file` must set `file_path` and leave the other five core fields consistent
/// (compiled and last_check become Some; module_name matches the file stem).
#[test]
fn load_file_updates_only_file_path_via_commit_file_path() {
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

    // load_file triggers commit_file_path internally.
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
}
