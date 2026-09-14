//! `chain` default-port inference tests (spec §6.2).
//!
//! A bare chain element names an occurrence/structure sub rather than a port, and
//! each hop is desugared to that sub's unique port in the direction the hop needs.
//! Lives apart from `connect_compile_tests.rs`, which already carries the explicit
//! `connect` surface at 2368 lines.

use reify_core::*;
use reify_test_support::compile_source;

/// The spec §6.2 worked shape: three single-in/single-out occurrences chained
/// by bare name. Each hop must resolve the source element to its unique `out`
/// port and the destination element to its unique `in` port.
///
/// The middle element is the load-bearing part: `p2` has to mean `p2.inlet` as
/// a destination and `p2.outlet` as a source. Reading it one way in both roles
/// is what makes any chain longer than two elements direction-invalid.
#[test]
fn chain_bare_occurrence_elements_infer_default_ports() {
    let source = r#"
trait FluidPort { param diameter : Length }
occurrence def Pipe {
    port inlet : in FluidPort { param diameter : Length = 25mm }
    port outlet : out FluidPort { param diameter : Length = 25mm }
}
structure def Pipeline {
    sub p1 = Pipe()
    sub p2 = Pipe()
    sub p3 = Pipe()
    chain p1 -> p2 -> p3
}
"#;

    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let pipeline = module
        .templates
        .iter()
        .find(|t| t.name == "Pipeline")
        .expect("expected template Pipeline");

    assert_eq!(
        pipeline.connections.len(),
        2,
        "expected 2 connections, got {}",
        pipeline.connections.len()
    );

    assert_eq!(pipeline.connections[0].left_port, "p1.outlet");
    assert_eq!(pipeline.connections[0].right_port, "p2.inlet");
    assert_eq!(
        pipeline.connections[0].operator,
        reify_ast::ConnectOp::Forward
    );

    assert_eq!(pipeline.connections[1].left_port, "p2.outlet");
    assert_eq!(pipeline.connections[1].right_port, "p3.inlet");
    assert_eq!(
        pipeline.connections[1].operator,
        reify_ast::ConnectOp::Forward
    );
}
