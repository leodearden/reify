//! Pins the rendered/hidden composition that `smoke_mesh_count_parity_e2e.mjs`'s
//! `FIXTURES` floors depend on.

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::MockGeometryKernel;

use crate::commands::{engine_state_json, mesh_stats_json};
use crate::engine::EngineSession;

const AUX_ASSEMBLY_SRC: &str = include_str!("../../../test/fixtures/aux_assembly.ri");
const LARGE_ASSEMBLY_SRC: &str = include_str!("../../../test/fixtures/large_assembly.ri");

struct FixtureComposition {
    module: &'static str,
    source: &'static str,
    rendered: &'static [&'static str],
    hidden: &'static [&'static str],
}

const PARITY_SMOKE_FIXTURES: &[FixtureComposition] = &[
    FixtureComposition {
        module: "aux_assembly",
        source: AUX_ASSEMBLY_SRC,
        rendered: &[
            "AuxAssembly.drive_a",
            "AuxAssembly.drive_b",
            "AuxAssembly.spacer",
        ],
        hidden: &[
            "AuxAssembly.drive_a",
            "AuxAssembly.drive_a",
            "AuxAssembly.drive_b",
            "AuxAssembly.drive_b",
        ],
    },
    FixtureComposition {
        module: "large_assembly",
        source: LARGE_ASSEMBLY_SRC,
        rendered: &[
            "CompactAssembly.bp1",
            "CompactAssembly.bp2",
            "CompactAssembly.bp3",
            "CompactAssembly.pl1",
            "CompactAssembly.pl2",
            "CompactAssembly.tp1",
            "CompactAssembly.tp2",
        ],
        hidden: &[],
    },
];

fn session_for(src: &str, module: &str) -> EngineSession {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(
        reify_eval::SolverPosture::SolverFree,
        Box::new(checker),
        Some(Box::new(kernel)),
    );
    session
        .load_from_source(src, module)
        .unwrap_or_else(|e| panic!("{module}: load_from_source must succeed: {e:?}"));
    session
}

struct DefaultVisiblePartition {
    rendered: Vec<String>,
    hidden: Vec<String>,
}

/// Splits a debug projection's `meshes` by `default_visible` into sorted
/// entity-path prefixes (the part before `#`).
fn partition_by_default_visible(
    read: &str,
    projection: &serde_json::Value,
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
        let prefix = path.split('#').next().unwrap_or(path).to_string();
        match entry.get("default_visible") {
            Some(serde_json::Value::Bool(true)) => partition.rendered.push(prefix),
            Some(serde_json::Value::Bool(false)) => partition.hidden.push(prefix),
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
        let mut session = session_for(fixture.source, fixture.module);
        let mesh_stats = mesh_stats_json(&mut session).expect("mesh_stats_json succeeds");
        let engine_state = engine_state_json(&mut session).expect("engine_state_json succeeds");

        for (read, projection) in [("mesh_stats", &mesh_stats), ("engine_state", &engine_state)] {
            let partition = partition_by_default_visible(read, projection);
            assert_eq!(
                partition.rendered, fixture.rendered,
                "{}: {read} default_visible=true prefixes",
                fixture.module
            );
            assert_eq!(
                partition.hidden, fixture.hidden,
                "{}: {read} default_visible=false prefixes",
                fixture.module
            );
        }
    }
}
