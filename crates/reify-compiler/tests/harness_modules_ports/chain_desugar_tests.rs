//! `chain` default-port inference tests (spec §6.2).
//!
//! A bare chain element names an occurrence/structure sub rather than a port, and
//! each hop is desugared to that sub's unique port in the direction the hop needs.
//! Lives apart from `connect_compile_tests.rs`, which already carries the explicit
//! `connect` surface at 2368 lines.

use reify_core::*;
use reify_test_support::{assert_no_diagnostic, compile_source};

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

/// §6.2 refuses to guess. An element whose sub has several ports in the
/// direction the hop needs is a compile error that names the candidates, so the
/// author can pick one by dotting the element.
///
/// The hop must be dropped rather than half-emitted: the author gets exactly
/// this one diagnostic, not this one plus `compile_connection`'s misleading
/// `undefined port 'a'` — which is the only thing this shape reports today.
#[test]
fn chain_element_with_multiple_ports_in_needed_direction_is_an_error() {
    let source = r#"
trait FluidPort { param diameter : Length }
occurrence def Splitter {
    port inlet : in FluidPort { param diameter : Length = 25mm }
    port outA : out FluidPort { param diameter : Length = 25mm }
    port outB : out FluidPort { param diameter : Length = 25mm }
}
structure def Pipeline {
    sub a = Splitter()
    sub b = Splitter()
    chain a -> b
}
"#;

    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error — only `a` is ambiguous, `b` has a unique `in` port; got: {:?}",
        errors
    );

    let message = &errors[0].message;
    for expected in ["'a'", "'out'", "outA", "outB"] {
        assert!(
            message.contains(expected),
            "ambiguity error should name {expected}, got: {message}"
        );
    }
    assert_no_diagnostic(&module.diagnostics, Severity::Error, "undefined port");

    let pipeline = module
        .templates
        .iter()
        .find(|t| t.name == "Pipeline")
        .expect("expected template Pipeline");
    assert!(
        pipeline.connections.is_empty(),
        "a hop with an unresolved endpoint must not be emitted, got: {:?}",
        pipeline.connections
    );
}

/// The other half of the §6.2 uniqueness rule: zero ports in the needed
/// direction fails just as loudly as several, and says so in its own words —
/// there is nothing to list, so the author is pointed at dotting the element.
///
/// `Sink` has no `out` port, so `a` cannot source a hop. `b` still resolves to
/// its unique `in` port, which is what keeps this to a single diagnostic.
#[test]
fn chain_element_with_no_port_in_needed_direction_is_an_error() {
    let source = r#"
trait FluidPort { param diameter : Length }
occurrence def Sink {
    port inlet : in FluidPort { param diameter : Length = 25mm }
}
structure def Pipeline {
    sub a = Sink()
    sub b = Sink()
    chain a -> b
}
"#;

    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error — only `a` lacks the port its role needs; got: {:?}",
        errors
    );

    let message = &errors[0].message;
    for expected in ["'a'", "'out'", "no "] {
        assert!(
            message.contains(expected),
            "missing-port error should name {expected}, got: {message}"
        );
    }
    assert_no_diagnostic(&module.diagnostics, Severity::Error, "undefined port");

    let pipeline = module
        .templates
        .iter()
        .find(|t| t.name == "Pipeline")
        .expect("expected template Pipeline");
    assert!(
        pipeline.connections.is_empty(),
        "a hop with an unresolved endpoint must not be emitted, got: {:?}",
        pipeline.connections
    );
}

/// §6.2 inference belongs to `chain`, not to one desugar site. A `forall` body
/// elaborates its own chain in `forall_elaborate.rs`, and a bare element there
/// must resolve exactly as it does in a plain `chain`.
///
/// The bound variable substitutes to an INDEXED element (`vents[0]`), which is
/// why resolution keys on the sub name with the indexer stripped: every element
/// of a collection shares one child template and so one set of port directions.
#[test]
fn forall_chain_elements_infer_default_ports() {
    let source = r#"
trait Air { param d : Length }
occurrence def Vent {
    port inlet : in Air { param d : Length = 5mm }
    port outlet : out Air { param d : Length = 5mm }
}
occurrence def Hub {
    port feed : in Air { param d : Length = 5mm }
    port vent : out Air { param d : Length = 5mm }
}
structure def S {
    sub vents : List<Vent>
    constraint vents.count == 2
    sub hub = Hub()
    forall v in vents: chain v -> hub
}
"#;

    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let s = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");

    let endpoints: Vec<(&str, &str)> = s
        .connections
        .iter()
        .map(|c| (c.left_port.as_str(), c.right_port.as_str()))
        .collect();
    assert_eq!(
        endpoints,
        vec![
            ("vents[0].outlet", "hub.feed"),
            ("vents[1].outlet", "hub.feed"),
        ]
    );
}
