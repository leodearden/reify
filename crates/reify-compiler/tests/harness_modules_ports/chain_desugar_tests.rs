//! `chain` default-port inference tests (spec §6.2).
//!
//! A bare chain element names an occurrence/structure sub rather than a port, and
//! each hop is desugared to that sub's unique port in the direction the hop needs.
//! Lives apart from `connect_compile_tests.rs`, which already carries the explicit
//! `connect` surface at 2368 lines.
//!
//! Inference is per-INSTANCE: an element must denote one occurrence, so a
//! collection or keyed sub named without an indexer is refused rather than
//! inferred. The INDEXED forms it refuses to cover — `vents[0]`, which `forall`
//! substitution produces, and `vents["intake"]` — must keep inferring, and
//! `forall_chain_elements_infer_default_ports` is the standing guard for that.

use reify_core::*;
use reify_test_support::{
    assert_no_diagnostic, assert_no_error_diagnostics, compile_source, errors_only,
};

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

/// A `bidi` port is usable in either role, so a sub whose only port is `bidi`
/// has a unique endpoint for both halves of a hop — `is_forward_compatible`
/// accepts `(Bidi, Bidi)`.
///
/// Rejecting it as "has no 'out' port" states something false: the port exists
/// and the hand-dotted `chain a.link -> b.link` over this same fixture compiles.
#[test]
fn chain_element_with_only_a_bidi_port_resolves_it() {
    let source = r#"
trait Air { param d : Length }
structure def Node {
    port link : bidi Air { param d : Length = 5mm }
}
structure def S {
    sub a = Node()
    sub b = Node()
    chain a -> b
}
"#;

    let module = compile_source(source);
    assert_no_error_diagnostics(&module.diagnostics, "chain desugaring");

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
    assert_eq!(endpoints, vec![("a.link", "b.link")]);
}

/// Accepting `bidi` is a FALLBACK, not a widening of the candidate set. A sub
/// with one `in`, one `out` and one `bidi` port has exactly one port in each
/// needed direction per §6.2, so inference must still pick the directional
/// pair — pooling `bidi` in with them would turn this working shape into an
/// ambiguity error.
#[test]
fn chain_element_prefers_an_exact_direction_match_over_a_bidi_port() {
    let source = r#"
trait Air { param d : Length }
structure def Vent {
    port inlet : in Air { param d : Length = 5mm }
    port outlet : out Air { param d : Length = 5mm }
    port link : bidi Air { param d : Length = 5mm }
}
structure def S {
    sub a = Vent()
    sub b = Vent()
    chain a -> b
}
"#;

    let module = compile_source(source);
    assert_no_error_diagnostics(&module.diagnostics, "chain desugaring");

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
    assert_eq!(endpoints, vec![("a.outlet", "b.inlet")]);
}

/// Several `bidi` ports and nothing directional is the §6.2 ambiguity case
/// reached through the fallback tier: both are usable, so the compiler refuses
/// to guess and names them. Each role fails separately — `a` as a source, `b`
/// as a destination — so the author sees one diagnostic per element.
#[test]
fn chain_element_with_several_bidi_ports_is_ambiguous() {
    let source = r#"
trait Air { param d : Length }
structure def Coupler {
    port linkA : bidi Air { param d : Length = 5mm }
    port linkB : bidi Air { param d : Length = 5mm }
}
structure def S {
    sub a = Coupler()
    sub b = Coupler()
    chain a -> b
}
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);
    assert_eq!(
        errors.len(),
        2,
        "expected one error per role — `a` as source, `b` as destination; got: {:?}",
        errors
    );
    for (element, dir, error) in [("'a'", "'out'", errors[0]), ("'b'", "'in'", errors[1])] {
        for expected in [element, dir, "linkA", "linkB"] {
            assert!(
                error.message.contains(expected),
                "ambiguity error should name {expected}, got: {}",
                error.message
            );
        }
    }
    assert_no_diagnostic(&module.diagnostics, Severity::Error, "undefined port");

    let s = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");
    assert!(
        s.connections.is_empty(),
        "a hop with an unresolved endpoint must not be emitted, got: {:?}",
        s.connections
    );
}

/// A collection sub names N occurrences, not one, so there is no single
/// endpoint to infer. Refusing is the whole point: inferring `vents.outlet`
/// yields a compat constraint over a node that does not exist — the real
/// instances are `vents[0].outlet` and `vents[1].outlet` — and it reports
/// "All constraints satisfied" over that phantom.
///
/// Before §6.2 inference existed this source was a hard `undefined port
/// 'vents'`, so the diagnostic is mandatory: returning no endpoint silently
/// would trade the phantom for a missing error.
#[test]
fn chain_over_a_collection_sub_without_an_indexer_is_an_error() {
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
    chain vents -> hub
}
"#;

    assert_chain_over_an_unindexed_collection_is_refused(source);
}

/// The keyed half of the same rule. Keyed subs have `is_collection == false`
/// and so are ABSENT from `collection_sub_names`, living in `keyed_sub_keys`
/// instead — a refusal that consults only the former lets this shape through
/// to the identical phantom.
#[test]
fn chain_over_a_keyed_sub_without_an_indexer_is_an_error() {
    let source = r#"
trait Air { param d : Length }
occurrence def Vent {
    param size : Length = 4mm
    port inlet : in Air { param d : Length = 5mm }
    port outlet : out Air { param d : Length = 5mm }
}
occurrence def Hub {
    port feed : in Air { param d : Length = 5mm }
    port vent : out Air { param d : Length = 5mm }
}
structure def S {
    sub vents : Keyed<Vent> { "intake" => { size = 5mm } }
    sub hub = Hub()
    chain vents -> hub
}
"#;

    assert_chain_over_an_unindexed_collection_is_refused(source);
}

/// The collection and keyed cases differ only in how `vents` is declared, so
/// both assert the same three things: one error naming the sub, a remedy that
/// prescribes the per-element `forall` form, and no connection at all —
/// emitting the hop is what manufactures the phantom endpoint.
fn assert_chain_over_an_unindexed_collection_is_refused(source: &str) {
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error — only `vents` is unindexed, `hub` is a plain sub; got: {:?}",
        errors
    );

    let message = &errors[0].message;
    for expected in ["'vents'", "forall"] {
        assert!(
            message.contains(expected),
            "refusal should name {expected}, got: {message}"
        );
    }
    assert_no_diagnostic(&module.diagnostics, Severity::Error, "undefined port");

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
        Vec::<(&str, &str)>::new(),
        "an unindexed collection element must emit no connection; \
         `vents.outlet` names no instance that exists"
    );
}
