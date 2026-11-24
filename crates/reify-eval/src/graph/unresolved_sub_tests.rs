//! The evaluation graph resolves each sub's structure through the module's
//! definition environment, so a stdlib-typed sub's instance cells are part of
//! the graph like any local one's, and a sub the environment cannot resolve is
//! reported rather than dropped.

use reify_compiler::stdlib_loader::load_stdlib;
use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_test_support::{TopologyTemplateBuilder, module_of};

use super::EvaluationGraph;
use super::cell_contributions::fixtures::compile;
use crate::DefEnv;

#[test]
fn a_stdlib_typed_sub_registers_its_instance_cells() {
    let module = compile(
        "structure def D {\n    let part = box(10mm, 20mm, 5mm)\n\n    \
         sub o = STLOutput(subject: part, resolution: 0.2mm, path: \"o.stl\")\n}\n",
    );
    let graph = EvaluationGraph::from_env(DefEnv::of_module(&module, load_stdlib()));

    assert!(
        graph
            .value_cells
            .contains_key(&ValueCellId::new("D.o", "path")),
        "the STLOutput instance's `path` cell is in the graph"
    );
    assert_eq!(graph.sub_instance_template("D.o"), Some("STLOutput"));
}

#[test]
fn an_unresolvable_sub_yields_a_coded_diagnostic() {
    let module = module_of([
        TopologyTemplateBuilder::new("Rib").build(),
        TopologyTemplateBuilder::new("Parent")
            .sub_component("rib", "DoesNotExist", vec![])
            .sub_component("strut", "Rib", vec![])
            .build(),
    ]);
    let graph = EvaluationGraph::from_env(DefEnv::of_module(&module, &[]));

    let [diagnostic] = graph.unresolved_sub_diagnostics.as_slice() else {
        panic!(
            "exactly one unresolved-sub diagnostic, got {:?}",
            graph.unresolved_sub_diagnostics
        );
    };
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(diagnostic.code, Some(DiagnosticCode::UnknownSubStructure));
    assert!(
        diagnostic.message.contains("sub-component \"rib\"")
            && diagnostic.message.contains("\"DoesNotExist\""),
        "{}",
        diagnostic.message
    );
}
