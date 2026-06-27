//! Keyed identity stability across re-elaboration (task 3932 δ).
//!
//! Step-1 (RED): pins connect desugaring to a keyed member port and the
//! key-addressed identity-stability integration guard.
//!
//! Step-5 (RED): pins that `from_templates` registers keyed subs in
//! `graph.keyed_subs` with the correct member_keys.
//!
//! User-observable signal:
//!   cargo test -p reify-eval --test keyed_identity_reelaboration

use reify_core::{Severity, ValueCellId};
use reify_eval::graph::EvaluationGraph;
use reify_ir::MemberKey;
use reify_test_support::{compile_source, parse_and_compile_with_stdlib};

// ── Sources ───────────────────────────────────────────────────────────────────

/// Full keyed-vents source with trait Flow, port inlet on Vent, port src on
/// Manifold, and a connect src -> vents["intake"].inlet.
///
/// RED today (step-1): resolve_port_name returns None for the StringLiteral
/// index, so compile_connection emits "invalid port reference" and pushes NO
/// CompiledConnection for the keyed port ref.
const KEYED_CONNECT_SRC: &str = r#"
trait Flow {}

structure def Vent {
    param area : Length = 1mm
    port inlet : in Flow {}
}

structure def Manifold {
    sub vents : Keyed<Vent> {
        "intake" => { area = 5mm }
        "exhaust" => { area = 8mm }
    }
    port src : out Flow {}
    let a = vents["intake"].area
    let b = vents["exhaust"].area
    connect src -> vents["intake"].inlet
}
"#;

/// M1 source: two keyed members (intake, exhaust) with a connect to
/// vents["intake"].inlet. Key-addressed identity: `Manifold.vents["intake"]`
/// is stable regardless of member count.
const M1_SRC: &str = r#"
trait Flow {}

structure def Vent {
    param area : Length = 1mm
    port inlet : in Flow {}
}

structure def Manifold {
    sub vents : Keyed<Vent> {
        "intake" => { area = 5mm }
        "exhaust" => { area = 8mm }
    }
    port src : out Flow {}
    let a = vents["intake"].area
    connect src -> vents["intake"].inlet
}
"#;

/// M2 source: same as M1 but with "bypass" declared FIRST (insertion-order
/// index 0). A positional vents[0] would now resolve to "bypass" (shifted),
/// while vents["intake"] is key-addressed and remains stable.
const M2_SRC: &str = r#"
trait Flow {}

structure def Vent {
    param area : Length = 1mm
    port inlet : in Flow {}
}

structure def Manifold {
    sub vents : Keyed<Vent> {
        "bypass" => { area = 3mm }
        "intake" => { area = 5mm }
        "exhaust" => { area = 8mm }
    }
    port src : out Flow {}
    let a = vents["intake"].area
    connect src -> vents["intake"].inlet
}
"#;

/// Clean source for keyed sub registration test (step-5): no connect/port,
/// no count — tests only that from_templates populates keyed_subs.
const KEYED_CLEAN_SRC: &str = r#"
structure def Vent {
    param area : Length = 1mm
}

structure def Manifold {
    sub vents : Keyed<Vent> {
        "intake" => { area = 5mm }
        "exhaust" => { area = 8mm }
    }
    let a = vents["intake"].area
    let b = vents["exhaust"].area
}
"#;

// ── Tests ─────────────────────────────────────────────────────────────────────

/// (a) Connect desugaring to a keyed member port (step-1 RED).
///
/// GREEN criterion (step-2): resolve_port_name handles StringLiteral with {:?}
/// formatting → right_port == `vents["intake"].inlet` (byte-identical to
/// MemberKey::path_segment's output dotted with the port member name).
///
/// RED today: resolve_port_name returns None for StringLiteral → no
/// CompiledConnection is produced → both assertions fail:
///   - no connection with right_port == `vents["intake"].inlet`
///   - "invalid port reference" Error diagnostic IS present
#[test]
fn keyed_connect_desugars_to_key_addressed_port() {
    // compile_source: does NOT panic on Error diagnostics (the RED-phase
    // "invalid port reference" Error must not abort the test).
    let module = compile_source(KEYED_CONNECT_SRC);

    // Find the Manifold template.
    let manifold = module
        .templates
        .iter()
        .find(|t| t.name == "Manifold")
        .expect("Manifold template must exist");

    // The connect target `vents["intake"].inlet` must desugar to a
    // CompiledConnection whose right_port is the key-addressed port path
    // `vents["intake"].inlet` — byte-identical to what MemberKey::path_segment
    // builds for the "intake" key dotted with the "inlet" port member.
    let expected_right_port = r#"vents["intake"].inlet"#;
    let conn = manifold.connections.iter().find(|c| c.right_port == expected_right_port);
    assert!(
        conn.is_some(),
        "expected a CompiledConnection with right_port == {:?}, but connections are: {:?}",
        expected_right_port,
        manifold.connections.iter().map(|c| &c.right_port).collect::<Vec<_>>(),
    );

    // No Error diagnostic about "invalid port reference" must exist — the
    // connect statement compiled cleanly.
    let invalid_port_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("invalid port reference"))
        .collect();
    assert!(
        invalid_port_errors.is_empty(),
        "unexpected 'invalid port reference' Error diagnostic(s): {:?}",
        invalid_port_errors,
    );
}

/// (b) Key-addressed cell stable across sibling-add (step-1 RED).
///
/// GREEN criterion (step-2): connect desugars → connect right_port present and
/// identical across M1/M2; cell presence already holds from step-8 (γ).
///
/// RED today: no CompiledConnection for the keyed port ref in either M1 or M2
/// → the connection right_port assertions fail (cell assertions already pass).
///
/// Design note: a positional vents[0] would now resolve to "bypass"
/// (insertion-order-first shifted) while vents["intake"] is key-addressed and
/// stable. This test demonstrates the headline of the PRD's G2 gate.
#[test]
fn keyed_member_identity_stable_across_sibling_add() {
    // compile_source: does NOT panic on Error (keyed connect is RED Error).
    let m1 = compile_source(M1_SRC);
    let m2 = compile_source(M2_SRC);

    // Build evaluation graphs from the compiled templates.
    let g1 = EvaluationGraph::from_templates(&m1.templates);
    let g2 = EvaluationGraph::from_templates(&m2.templates);

    // Cell identity: Manifold.vents["intake"].area must be present in BOTH graphs
    // with the same 5mm override (scoped entity id is the key-addressed path).
    let intake_area_id = ValueCellId::new(r#"Manifold.vents["intake"]"#, "area");
    assert!(
        g1.value_cells.contains_key(&intake_area_id),
        "M1 graph must contain Manifold.vents[\"intake\"].area",
    );
    assert!(
        g2.value_cells.contains_key(&intake_area_id),
        "M2 graph must contain Manifold.vents[\"intake\"].area (key-addressed, stable)",
    );

    // M2 also has "bypass" — present only in g2, not g1.
    let bypass_area_id = ValueCellId::new(r#"Manifold.vents["bypass"]"#, "area");
    assert!(
        !g1.value_cells.contains_key(&bypass_area_id),
        "M1 graph must NOT contain Manifold.vents[\"bypass\"].area (not declared)",
    );
    assert!(
        g2.value_cells.contains_key(&bypass_area_id),
        "M2 graph must contain Manifold.vents[\"bypass\"].area (declared first)",
    );

    // Connect right_port: vents["intake"].inlet must be present and byte-identical
    // in both M1 and M2 (key-addressed, stable under sibling insertion).
    //
    // A positional `vents[0]` would differ between M1 (= "intake") and
    // M2 (= "bypass"), demonstrating the stability advantage of keyed addressing.
    let expected_right_port = r#"vents["intake"].inlet"#;

    let m1_manifold = m1
        .templates
        .iter()
        .find(|t| t.name == "Manifold")
        .expect("M1 Manifold template must exist");
    let m2_manifold = m2
        .templates
        .iter()
        .find(|t| t.name == "Manifold")
        .expect("M2 Manifold template must exist");

    let m1_conn = m1_manifold
        .connections
        .iter()
        .any(|c| c.right_port == expected_right_port);
    let m2_conn = m2_manifold
        .connections
        .iter()
        .any(|c| c.right_port == expected_right_port);

    assert!(
        m1_conn,
        "M1: expected CompiledConnection with right_port == {:?}, but connections are: {:?}",
        expected_right_port,
        m1_manifold.connections.iter().map(|c| &c.right_port).collect::<Vec<_>>(),
    );
    assert!(
        m2_conn,
        "M2: expected CompiledConnection with right_port == {:?} (key stable, not 'vents[\"bypass\"].inlet'), \
         but connections are: {:?}",
        expected_right_port,
        m2_manifold.connections.iter().map(|c| &c.right_port).collect::<Vec<_>>(),
    );
}

/// (step-5 RED) `from_templates` registers keyed subs in `graph.keyed_subs`.
///
/// GREEN criterion (step-6): populate keyed_subs in the keyed branch of
/// from_templates. RED today: keyed_subs stays empty (population not yet
/// implemented).
///
/// Uses `parse_and_compile_with_stdlib` (clean source, no connect, no missing key).
#[test]
fn from_templates_registers_keyed_sub_with_member_keys() {
    let module = parse_and_compile_with_stdlib(KEYED_CLEAN_SRC);
    let graph = EvaluationGraph::from_templates(&module.templates);

    // There must be exactly one keyed_subs entry for the Manifold.vents sub.
    let info = graph
        .keyed_subs
        .iter()
        .find(|s| s.parent_entity == "Manifold" && s.sub_name == "vents");
    assert!(
        info.is_some(),
        "expected a KeyedSubInfo for Manifold.vents in keyed_subs, got: {:?}",
        graph.keyed_subs,
    );
    let info = info.unwrap();

    // Structure name: the resolved element type (NOT the "Keyed" wrapper).
    assert_eq!(
        info.structure_name, "Vent",
        "structure_name must be the element type 'Vent', not the Keyed wrapper",
    );

    // Member keys in declaration order.
    assert_eq!(
        info.member_keys,
        vec![MemberKey::new("intake"), MemberKey::new("exhaust")],
        "member_keys must match the author-assigned keys in declaration order",
    );
}
