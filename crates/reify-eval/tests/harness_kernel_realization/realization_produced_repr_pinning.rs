//! After `Engine::eval`, every realization node must retain the
//! construction-time `ReprKind::BRep` default. Eval must not clear or
//! overwrite `produced_repr` before task ε (3436) wires the per-op
//! dispatcher.

use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
use reify_core::{ModulePath, Type};
use reify_eval::Engine;
use reify_ir::{CompiledExpr, ReprKind, Value};
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, MockGeometryKernel, TopologyTemplateBuilder,
};

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

/// Forward-guard: eval must not clear/overwrite the construction-time
/// `ReprKind::BRep` default on any realization node.
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
