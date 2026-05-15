//! Integration test: PRD §8 task α observable-signal pin.
//!
//! Pins the contract that, after `Engine::eval`, every realization node in
//! the snapshot graph has `produced_repr == ReprKind::BRep`.
//!
//! In v0.2, `produced_repr` is initialized to `ReprKind::BRep` at
//! graph-construction time (`EvaluationGraph::from_templates`) — the BRep
//! constant matches the only output type any v0.2 kernel adapter (OCCT)
//! produces. Task ε (3436) will wire the per-op dispatcher choice at
//! execution time; if that wiring accidentally stops writing BRep for the
//! OCCT path, this test will fail, surfacing the regression before merge.
//!
//! Observable-signal definition (PRD §8 task α):
//! "after `execute_realization_ops`, every realization node's `produced_repr`
//!  matches the actual ReprKind of its stored value (verified against the
//!  kernel adapter's output type)."

use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
use reify_eval::Engine;
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, MockGeometryKernel, TopologyTemplateBuilder,
};
use reify_types::{CompiledExpr, ModulePath, ReprKind, Type, Value};

/// Build a minimal compiled module containing a single Box primitive
/// realization for the "Widget" structure. No constraints or params — the
/// fixture stays focused on the realization graph shape.
fn single_box_realization_module() -> reify_compiler::CompiledModule {
    let ops = vec![CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            (
                "width".to_string(),
                CompiledExpr::literal(Value::length(0.10), Type::length()),
            ),
            (
                "height".to_string(),
                CompiledExpr::literal(Value::length(0.05), Type::length()),
            ),
            (
                "depth".to_string(),
                CompiledExpr::literal(Value::length(0.02), Type::length()),
            ),
        ],
    }];

    let template = TopologyTemplateBuilder::new("Widget")
        .realization("Widget", 0, ops)
        .build();

    CompiledModuleBuilder::new(ModulePath::single("widget"))
        .template(template)
        .build()
}

/// PRD §8 task α observable-signal pin:
/// after `Engine::eval`, every realization in `snapshot().graph.realizations`
/// must carry `produced_repr == ReprKind::BRep`.
///
/// In v0.2, `produced_repr` is initialized at graph-construction time inside
/// `EvaluationGraph::from_templates` to the constant `ReprKind::BRep` — the
/// sole output kind produced by the OCCT kernel adapter (the only wired
/// non-stub adapter in this release). `MockGeometryKernel` mirrors this
/// contract (BRep-tagged handles only), so the assertion holds on both
/// OCCT-enabled and CI (mock-kernel) build configurations.
///
/// Future regression guard: when task ε (3436) wires the per-op dispatcher
/// choice and begins writing `produced_repr` dynamically inside
/// `execute_realization_ops`, this test will catch any accidental non-BRep
/// output on the OCCT path before it reaches the merge queue.
#[test]
fn every_realization_node_has_produced_repr_brep_after_eval() {
    let module = single_box_realization_module();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));

    // Drive eval so the snapshot is populated from the compiled module.
    // `eval` builds the EvaluationGraph (which initializes produced_repr to
    // BRep at construction time) and stores it in the engine's eval_state.
    let _ = engine.eval(&module);

    let snap = engine
        .snapshot()
        .expect("snapshot must be Some after a successful eval()");

    assert!(
        !snap.graph.realizations.is_empty(),
        "expected at least one realization node in the snapshot graph; \
         check that TopologyTemplateBuilder::realization() wired the op correctly"
    );

    for (id, node) in snap.graph.realizations.iter() {
        assert_eq!(
            node.produced_repr,
            ReprKind::BRep,
            "realization {:?}: expected produced_repr == ReprKind::BRep \
             (v0.2 OCCT-only baseline; MockGeometryKernel also emits BRep handles); \
             got {:?}. If this fires after task \u{03b5} (3436) lands, check that \
             execute_realization_ops correctly writes BRep for the OCCT path.",
            id,
            node.produced_repr
        );
    }
}
