//! Pins the rendered/hidden composition that `smoke_mesh_count_parity_e2e.mjs`'s
//! fixture floors (`gui/test/visual/meshCountParityFixtures.json`) depend on.

use std::collections::{BTreeSet, HashMap};

use crate::commands::{engine_state_json, mesh_stats_json};
use crate::engine::EngineSession;
use crate::types::EntityTreeNode;

use super::test_session_from_source;

const SMOKE_FIXTURE_FLOORS_JSON: &str =
    include_str!("../../../test/visual/meshCountParityFixtures.json");

/// A fixture's realizations, each labelled `<entity>#<binding>`: the mesh key
/// with its realization index replaced by the binding that realizes it.
struct FixtureComposition {
    file: &'static str,
    source: &'static str,
    rendered: &'static [&'static str],
    hidden: &'static [&'static str],
}

impl FixtureComposition {
    fn module(&self) -> &'static str {
        self.file
            .strip_suffix(".ri")
            .unwrap_or_else(|| panic!("{}: fixture file must end in .ri", self.file))
    }
}

macro_rules! fixture_composition {
    ($file:literal, rendered: $rendered:expr, hidden: $hidden:expr $(,)?) => {
        FixtureComposition {
            file: $file,
            source: include_str!(concat!("../../../test/fixtures/", $file)),
            rendered: $rendered,
            hidden: $hidden,
        }
    };
}

const PARITY_SMOKE_FIXTURES: &[FixtureComposition] = &[
    fixture_composition!(
        "aux_assembly.ri",
        rendered: &[
            "AuxAssembly.drive_a#geometry",
            "AuxAssembly.drive_b#geometry",
            "AuxAssembly.spacer#geometry",
        ],
        hidden: &[
            "AuxAssembly.drive_a#drum_blank",
            "AuxAssembly.drive_a#groove_cutter",
            "AuxAssembly.drive_b#drum_blank",
            "AuxAssembly.drive_b#groove_cutter",
        ],
    ),
    fixture_composition!(
        "large_assembly.ri",
        rendered: &[
            "CompactAssembly.bp1#geometry",
            "CompactAssembly.bp2#geometry",
            "CompactAssembly.bp3#geometry",
            "CompactAssembly.pl1#geometry",
            "CompactAssembly.pl2#geometry",
            "CompactAssembly.tp1#geometry",
            "CompactAssembly.tp2#geometry",
        ],
        hidden: &[],
    ),
];

/// One row of the smoke driver's fixture table.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SmokeFixtureFloors {
    file: String,
    min_rendered_bodies: usize,
    min_hidden_bodies: usize,
}

/// Every realization node's mesh key mapped to its binding name.
fn realization_bindings(session: &EngineSession) -> HashMap<String, String> {
    let mut bindings = HashMap::new();
    let mut pending = session.get_entity_tree();
    while let Some(node) = pending.pop() {
        let EntityTreeNode {
            entity_path,
            kind,
            display_name,
            children,
            ..
        } = node;
        if kind == "realization" {
            let binding = display_name
                .unwrap_or_else(|| panic!("realization {entity_path} must carry its binding name"));
            bindings.insert(entity_path, binding);
        }
        pending.extend(children);
    }
    bindings
}

struct DefaultVisiblePartition {
    rendered: Vec<String>,
    hidden: Vec<String>,
}

/// Splits a debug projection's `meshes` by `default_visible` into sorted
/// `<entity>#<binding>` labels.
fn partition_by_default_visible(
    read: &str,
    projection: &serde_json::Value,
    bindings: &HashMap<String, String>,
) -> DefaultVisiblePartition {
    let mut partition = DefaultVisiblePartition {
        rendered: Vec::new(),
        hidden: Vec::new(),
    };
    for entry in projection["meshes"]
        .as_array()
        .unwrap_or_else(|| panic!("{read}: meshes must be an array"))
    {
        let path = entry["entity_path"]
            .as_str()
            .unwrap_or_else(|| panic!("{read}: each mesh must carry an entity_path string"));
        let entity = path.split('#').next().unwrap_or(path);
        let binding = bindings.get(path).unwrap_or_else(|| {
            panic!("{read}: mesh {path} has no realization node in the entity tree")
        });
        let label = format!("{entity}#{binding}");
        match entry.get("default_visible") {
            Some(serde_json::Value::Bool(true)) => partition.rendered.push(label),
            Some(serde_json::Value::Bool(false)) => partition.hidden.push(label),
            other => panic!(
                "{read}: mesh {path} must carry a boolean default_visible (null means the \
                 entity-tree join missed); got {other:?}"
            ),
        }
    }
    partition.rendered.sort();
    partition.hidden.sort();
    partition
}

#[test]
fn parity_smoke_fixtures_realize_their_pinned_rendered_and_hidden_composition() {
    for fixture in PARITY_SMOKE_FIXTURES {
        let mut session = test_session_from_source(fixture.source, fixture.module());
        let bindings = realization_bindings(&session);
        let mesh_stats = mesh_stats_json(&mut session).expect("mesh_stats_json succeeds");
        let engine_state = engine_state_json(&mut session).expect("engine_state_json succeeds");

        for (read, projection) in [("mesh_stats", &mesh_stats), ("engine_state", &engine_state)] {
            let partition = partition_by_default_visible(read, projection, &bindings);
            assert_eq!(
                partition.rendered, fixture.rendered,
                "{}: {read} default_visible=true realizations",
                fixture.file
            );
            assert_eq!(
                partition.hidden, fixture.hidden,
                "{}: {read} default_visible=false realizations",
                fixture.file
            );
        }
    }
}

#[test]
fn every_smoke_fixture_floor_is_its_pinned_composition_count() {
    let floors: Vec<SmokeFixtureFloors> = serde_json::from_str(SMOKE_FIXTURE_FLOORS_JSON)
        .expect("meshCountParityFixtures.json must decode as the smoke's fixture table");

    let smoke_files: BTreeSet<&str> = floors.iter().map(|row| row.file.as_str()).collect();
    let pinned_files: BTreeSet<&str> = PARITY_SMOKE_FIXTURES.iter().map(|f| f.file).collect();
    assert_eq!(
        smoke_files, pinned_files,
        "the smoke's fixture table and PARITY_SMOKE_FIXTURES must name the same fixtures"
    );

    for row in &floors {
        let pinned = PARITY_SMOKE_FIXTURES
            .iter()
            .find(|f| f.file == row.file)
            .expect("file sets are equal");
        assert_eq!(
            (row.min_rendered_bodies, row.min_hidden_bodies),
            (pinned.rendered.len(), pinned.hidden.len()),
            "{}: smoke floors (rendered, hidden) must equal the pinned composition's counts",
            row.file
        );
    }
}
