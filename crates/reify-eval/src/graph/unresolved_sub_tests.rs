//! The evaluation graph resolves each sub's structure through the module's
//! definition environment, so a stdlib-typed sub's instance cells are part of
//! the graph like any local one's.

use reify_compiler::stdlib_loader::load_stdlib;
use reify_core::ValueCellId;

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
