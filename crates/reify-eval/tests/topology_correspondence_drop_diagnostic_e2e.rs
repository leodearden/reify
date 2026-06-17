//! End-to-end tests for the topology-correspondence-drop diagnostic wiring —
//! task 4545 (W_TOPOLOGY_CORRESPONDENCE_DROPPED).
//!
//! Verifies that `Engine::build` surfaces a `Severity::Warning` with
//! `DiagnosticCode::TopologyCorrespondenceDropped` when the kernel's
//! `execute_with_history` returns a history record carrying a non-zero
//! drop counter.
//!
//! No OCCT is required: a `DropInjectingKernel` (modelled on `HistoryMockKernel`
//! in `topology_attribute_extrude_revolve_e2e.rs:59-130`) injects synthetic
//! non-zero counts directly into the returned `AttributeHistory`. The
//! synthesised `CompiledModule` pattern is also taken from that file.
//!
//! RED until step-6 wires `diagnose_topology_correspondence_drops` into
//! `Engine::execute_realization_ops`.

use reify_compiler::{BooleanOp, CompiledGeometryOp, CurveKind, GeomRef, ModifyKind, PrimitiveKind, SweepKind};
use reify_core::{DiagnosticCode, ModulePath, Severity, Type};
use reify_ir::{
    AttributeHistory, BooleanOpHistoryRecords, ExportError, ExportFormat, GeometryError,
    GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery,
    LocalFeatureOpHistoryRecords, Mesh, QueryError, SweepOpHistoryRecords, TessError, Value,
};
use reify_test_support::*;

// ─── DropInjectingKernel ─────────────────────────────────────────────────────

/// Mock `GeometryKernel` that wraps `MockGeometryKernel` and overrides
/// `execute_with_history` to inject synthetic `AttributeHistory` records
/// carrying non-zero drop counters.
///
/// - `GeometryOp::Union` / `Difference` / `Intersection` → injects
///   `AttributeHistory::Boolean(boolean_history)`.
/// - `GeometryOp::Extrude` → injects `AttributeHistory::Extrude(sweep_history)`.
/// - `GeometryOp::Fillet` / `GeometryOp::Chamfer` → injects
///   `AttributeHistory::LocalFeature(local_feature_history)` once the
///   local-feature arm is added in step-2; until then falls to
///   `AttributeHistory::None` via the `_ =>` catch-all.
/// - All other ops → `AttributeHistory::None`.
///
/// This lets the test fabricate non-zero `silent_drop_count` values without
/// a real OCCT kernel: the injected count is arbitrary and deterministic.
struct DropInjectingKernel {
    inner: MockGeometryKernel,
    boolean_history: BooleanOpHistoryRecords,
    sweep_history: SweepOpHistoryRecords,
    local_feature_history: LocalFeatureOpHistoryRecords,
}

impl DropInjectingKernel {
    fn new(
        boolean_history: BooleanOpHistoryRecords,
        sweep_history: SweepOpHistoryRecords,
    ) -> Self {
        Self {
            inner: MockGeometryKernel::new(),
            boolean_history,
            sweep_history,
            local_feature_history: LocalFeatureOpHistoryRecords::default(),
        }
    }

    /// Builder method to configure the local-feature history injected for
    /// `GeometryOp::Fillet` / `GeometryOp::Chamfer` ops (once the arm is
    /// wired in step-2). Non-breaking: existing `DropInjectingKernel::new`
    /// call sites keep their two-arg signature.
    fn with_local_feature_history(mut self, h: LocalFeatureOpHistoryRecords) -> Self {
        self.local_feature_history = h;
        self
    }
}

impl GeometryKernel for DropInjectingKernel {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        self.inner.execute(op)
    }

    fn execute_with_history(
        &mut self,
        op: &GeometryOp,
    ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
        let handle = self.inner.execute(op)?;
        let history = match op {
            GeometryOp::Union { .. }
            | GeometryOp::Difference { .. }
            | GeometryOp::Intersection { .. } => {
                AttributeHistory::Boolean(self.boolean_history.clone())
            }
            GeometryOp::Extrude { .. } => {
                AttributeHistory::Extrude(self.sweep_history.clone())
            }
            _ => AttributeHistory::None,
        };
        Ok((handle, history))
    }

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        self.inner.query(query)
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        self.inner.tessellate(handle, tolerance)
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn mm_literal(v: f64) -> reify_ir::CompiledExpr {
    reify_ir::CompiledExpr::literal(mm(v), Type::length())
}

/// Build a synthesised `CompiledModule` with three geometry steps:
///   Step 0: Box primitive (left operand)
///   Step 1: Box primitive (right operand)
///   Step 2: Union of Step(0) and Step(1)
///
/// The mock kernel treats any Union/Difference/Intersection op as a
/// boolean and injects the configured `AttributeHistory::Boolean`.
fn boolean_union_module() -> reify_compiler::CompiledModule {
    let box_op_a = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(10.0)),
            ("height".into(), mm_literal(10.0)),
            ("depth".into(), mm_literal(10.0)),
        ],
    };
    let box_op_b = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(5.0)),
            ("height".into(), mm_literal(5.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };
    let union_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };

    let template = TopologyTemplateBuilder::new("TestBooleanDrop")
        .realization("TestBooleanDrop", 0, vec![box_op_a, box_op_b, union_op])
        .build();
    CompiledModuleBuilder::new(ModulePath::single("test_topo_drop_bool"))
        .template(template)
        .build()
}

/// Build a synthesised `CompiledModule` with an Extrude op over a
/// LineSegment curve. The mock kernel injects `AttributeHistory::Extrude`
/// with a non-zero `silent_drop_count`.
fn extrude_with_sweep_drop_module() -> reify_compiler::CompiledModule {
    let line_op = CompiledGeometryOp::Curve {
        kind: CurveKind::LineSegment,
        args: vec![
            ("x1".into(), mm_literal(0.0)),
            ("y1".into(), mm_literal(0.0)),
            ("z1".into(), mm_literal(0.0)),
            ("x2".into(), mm_literal(10.0)),
            ("y2".into(), mm_literal(0.0)),
            ("z2".into(), mm_literal(0.0)),
        ],
    };
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("distance".into(), mm_literal(10.0)),
        ],
    };
    let template = TopologyTemplateBuilder::new("TestExtrudeDrop")
        .realization("TestExtrudeDrop", 0, vec![line_op, extrude_op])
        .build();
    CompiledModuleBuilder::new(ModulePath::single("test_topo_drop_extrude"))
        .template(template)
        .build()
}

// ─── tests ───────────────────────────────────────────────────────────────────

/// A boolean union op with `silent_drop_count=7` must surface as a
/// `Severity::Warning` with `DiagnosticCode::TopologyCorrespondenceDropped`
/// in `build_result.diagnostics`, and the message must contain "7".
///
/// RED until step-6 wires `diagnose_topology_correspondence_drops` into
/// `Engine::execute_realization_ops`.
#[test]
fn boolean_union_drop_produces_warning_diagnostic() {
    const DROP_COUNT: u32 = 7;

    let module = boolean_union_module();
    let kernel = DropInjectingKernel::new(
        BooleanOpHistoryRecords {
            silent_drop_count: DROP_COUNT,
            ..Default::default()
        },
        SweepOpHistoryRecords::default(),
    );
    let mut engine = reify_eval::Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result = engine.build(&module, ExportFormat::Step);

    // There must be at least one Warning with the TopologyCorrespondenceDropped code.
    let drop_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::TopologyCorrespondenceDropped)
        })
        .collect();

    assert!(
        !drop_warnings.is_empty(),
        "expected at least one TopologyCorrespondenceDropped warning; diagnostics: {:#?}",
        result.diagnostics
    );

    // The warning message must contain the counter_name=count token — not
    // just a bare digit — so the test resists incidental matches where the
    // context string or op_idx happens to contain the same digit.
    let token = format!("silent_drop_count={DROP_COUNT}");
    let has_count = drop_warnings.iter().any(|d| d.message.contains(&token));
    assert!(
        has_count,
        "warning message should contain '{token}'; warnings: {:#?}",
        drop_warnings
    );
}

/// Build a synthesised `CompiledModule` with two geometry steps:
///   Step 0: Box primitive (parent solid)
///   Step 1: Modify{ kind, target: Step(0) } (fillet or chamfer)
///
/// The mock kernel injects `AttributeHistory::LocalFeature` when the
/// local-feature arm is added in step-2; until then it falls to
/// `AttributeHistory::None`.
fn local_feature_drop_module(kind: ModifyKind) -> reify_compiler::CompiledModule {
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(10.0)),
            ("height".into(), mm_literal(10.0)),
            ("depth".into(), mm_literal(10.0)),
        ],
    };
    let modify_args = match kind {
        ModifyKind::Fillet => vec![("radius".into(), mm_literal(1.0))],
        ModifyKind::Chamfer => vec![("distance".into(), mm_literal(1.0))],
        _ => vec![],
    };
    let modify_op = CompiledGeometryOp::Modify {
        kind,
        target: GeomRef::Step(0),
        args: modify_args,
    };
    let template = TopologyTemplateBuilder::new("TestLocalFeatureDrop")
        .realization("TestLocalFeatureDrop", 0, vec![box_op, modify_op])
        .build();
    CompiledModuleBuilder::new(ModulePath::single("test_topo_drop_local_feature"))
        .template(template)
        .build()
}

/// A sweep (extrude) op with `silent_drop_count=3` must surface as a
/// `Severity::Warning` with `DiagnosticCode::TopologyCorrespondenceDropped`
/// in `build_result.diagnostics`.
///
/// Corroborates that the sweep arm of `diagnose_topology_correspondence_drops`
/// is also wired — not just the boolean arm.
///
/// RED until step-6 wires `diagnose_topology_correspondence_drops` into
/// `Engine::execute_realization_ops`.
#[test]
fn extrude_drop_produces_warning_diagnostic() {
    const DROP_COUNT: u32 = 3;

    let module = extrude_with_sweep_drop_module();
    let kernel = DropInjectingKernel::new(
        BooleanOpHistoryRecords::default(),
        SweepOpHistoryRecords {
            silent_drop_count: DROP_COUNT,
            ..Default::default()
        },
    );
    let mut engine = reify_eval::Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result = engine.build(&module, ExportFormat::Step);

    let drop_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::TopologyCorrespondenceDropped)
        })
        .collect();

    assert!(
        !drop_warnings.is_empty(),
        "expected at least one TopologyCorrespondenceDropped warning for sweep drop; diagnostics: {:#?}",
        result.diagnostics
    );

    let token = format!("silent_drop_count={DROP_COUNT}");
    let has_count = drop_warnings.iter().any(|d| d.message.contains(&token));
    assert!(
        has_count,
        "warning message should contain '{token}'; warnings: {:#?}",
        drop_warnings
    );
}

/// A fillet op with `silent_drop_count=5` must surface as a
/// `Severity::Warning` with `DiagnosticCode::TopologyCorrespondenceDropped`
/// in `build_result.diagnostics`, and the message must contain the exact
/// token `silent_drop_count=5`.
///
/// RED until step-2 extends `DropInjectingKernel` with the local-feature arm.
#[test]
fn local_feature_fillet_drop_produces_warning_diagnostic() {
    const DROP_COUNT: u32 = 5;

    let module = local_feature_drop_module(ModifyKind::Fillet);
    let kernel = DropInjectingKernel::new(
        BooleanOpHistoryRecords::default(),
        SweepOpHistoryRecords::default(),
    )
    .with_local_feature_history(LocalFeatureOpHistoryRecords {
        silent_drop_count: DROP_COUNT,
        ..Default::default()
    });
    let mut engine = reify_eval::Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result = engine.build(&module, ExportFormat::Step);

    let drop_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::TopologyCorrespondenceDropped)
        })
        .collect();

    assert!(
        !drop_warnings.is_empty(),
        "expected at least one TopologyCorrespondenceDropped warning for fillet drop; diagnostics: {:#?}",
        result.diagnostics
    );

    let token = format!("silent_drop_count={DROP_COUNT}");
    let has_count = drop_warnings.iter().any(|d| d.message.contains(&token));
    assert!(
        has_count,
        "warning message should contain '{token}' for fillet; warnings: {:#?}",
        drop_warnings
    );
}

/// A chamfer op with `silent_drop_count=5` must surface as a
/// `Severity::Warning` with `DiagnosticCode::TopologyCorrespondenceDropped`
/// in `build_result.diagnostics`, and the message must contain the exact
/// token `silent_drop_count=5`.
///
/// RED until step-2 extends `DropInjectingKernel` with the local-feature arm.
#[test]
fn local_feature_chamfer_drop_produces_warning_diagnostic() {
    const DROP_COUNT: u32 = 5;

    let module = local_feature_drop_module(ModifyKind::Chamfer);
    let kernel = DropInjectingKernel::new(
        BooleanOpHistoryRecords::default(),
        SweepOpHistoryRecords::default(),
    )
    .with_local_feature_history(LocalFeatureOpHistoryRecords {
        silent_drop_count: DROP_COUNT,
        ..Default::default()
    });
    let mut engine = reify_eval::Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result = engine.build(&module, ExportFormat::Step);

    let drop_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::TopologyCorrespondenceDropped)
        })
        .collect();

    assert!(
        !drop_warnings.is_empty(),
        "expected at least one TopologyCorrespondenceDropped warning for chamfer drop; diagnostics: {:#?}",
        result.diagnostics
    );

    let token = format!("silent_drop_count={DROP_COUNT}");
    let has_count = drop_warnings.iter().any(|d| d.message.contains(&token));
    assert!(
        has_count,
        "warning message should contain '{token}' for chamfer; warnings: {:#?}",
        drop_warnings
    );
}
