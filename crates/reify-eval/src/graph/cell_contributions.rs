#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use reify_compiler::{CompiledModule, TopologyTemplate, ValueCellKind};
    use reify_constraints::SimpleConstraintChecker;
    use reify_core::{ModulePath, Type, ValueCellId};

    use super::{ContributedCell, InsertPolicy, contributed_cells};
    use crate::graph::EvaluationGraph;

    /// A `.ri` input chosen to reach one `from_templates` value-cell arm, and
    /// the (measured) cell ids proving it does.
    struct CorpusEntry {
        arm: &'static str,
        src: &'static str,
        witnesses: &'static [&'static str],
    }

    const GUARDED_BOTH_BRANCHES: CorpusEntry = CorpusEntry {
        arm: "guarded group, both branches (guard cell + members + else_members)",
        src: r#"enum Shape { Round, Square }
enum Finish { Raw, Coated }
structure Fitting {
    let shape = Shape.Round
    param size : Real = 10.0
    where shape == Shape.Round {
        param ribs : Int = 3
    } else {
        param finish : Finish = Finish.Raw
    }
}
"#,
        witnesses: &["Fitting.__guard_", "Fitting.ribs", "Fitting.finish"],
    };

    const COLLECTION_LITERAL_COUNT: CorpusEntry = CorpusEntry {
        arm: "collection sub, literal count",
        src: r#"structure def Screw { param d : Real = 3.0 }
structure Rack {
    sub screws : List<Screw>
    constraint screws.count == 2
}
"#,
        witnesses: &["Rack.screws[0].d"],
    };

    const KEYED: CorpusEntry = CorpusEntry {
        arm: "keyed sub",
        src: r#"structure def Vent { param area : Real = 1.0 }
structure Manifold {
    sub vents : Keyed<Vent> {
        "intake" => { area = 5.0 }
        "exhaust" => { area = 8.0 }
    }
}
"#,
        witnesses: &["Manifold.vents[\"intake\"].area"],
    };

    const PLAIN_SUB: CorpusEntry = CorpusEntry {
        arm: "plain (non-collection) sub",
        src: r#"structure def Inner { param x : Real = 1.0 }
structure Outer { sub i = Inner() }
"#,
        witnesses: &["Outer.i.x"],
    };

    const SUCCESSFUL_MONOMORPH: CorpusEntry = CorpusEntry {
        arm: "successful `auto:` monomorphisation (generic + monomorph share ids)",
        src: r#"trait Seal {}
structure def GasketSeal : Seal { param d : Real = 2.0 }
structure def Bearing<T: Seal> {
    param bore : Real = 1.0
    param seal : T
    constraint bore > 0.1
}
structure def Assembly { sub b = Bearing<auto: Seal>() }
"#,
        witnesses: &["Assembly.b.seal"],
    };

    const PARENT_OVERRIDE: CorpusEntry = CorpusEntry {
        arm: "parent `auto` override of a plain sub's cell (task 3806 precedence)",
        src: r#"structure Bearing { param bore : Length = 5mm }
structure A {
    sub b : Bearing { bore = auto }
    constraint self.b.bore == 10mm
}
"#,
        witnesses: &["A.b.bore"],
    };

    const COLLECTION_VALUE_REF_COUNT: CorpusEntry = CorpusEntry {
        arm: "collection sub, count is a ValueRef to a literal Int param",
        src: r#"structure def Screw { param d : Real = 3.0 }
structure Tray {
    param n : Int = 3
    sub screws : List<Screw>
    constraint screws.count == n
}
"#,
        witnesses: &["Tray.screws[2].d"],
    };

    const CORPUS: [&CorpusEntry; 7] = [
        &GUARDED_BOTH_BRANCHES,
        &COLLECTION_LITERAL_COUNT,
        &KEYED,
        &PLAIN_SUB,
        &SUCCESSFUL_MONOMORPH,
        &PARENT_OVERRIDE,
        &COLLECTION_VALUE_REF_COUNT,
    ];

    fn compile(src: &str) -> CompiledModule {
        let parsed = reify_compiler::parse_with_stdlib(src, ModulePath::single("test"));
        reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker)
    }

    /// The value-cell map a consumer gets by inserting every contribution in
    /// iteration order under its own [`InsertPolicy`].
    fn replay(templates: &[TopologyTemplate]) -> HashMap<ValueCellId, ContributedCell<'_>> {
        let mut cells = HashMap::new();
        for cell in contributed_cells(templates) {
            let id = cell.graph_id();
            if cell.insert_policy() == InsertPolicy::KeepExisting && cells.contains_key(&id) {
                continue;
            }
            cells.insert(id, cell);
        }
        cells
    }

    /// The enumeration IS `from_templates`' value-cell set: replaying it
    /// reproduces `graph.value_cells` id for id, so a value-cell insert that
    /// bypasses the enumeration goes red on whichever input reaches it.
    #[test]
    fn contributed_cells_replay_reproduces_from_templates_value_cells() {
        for entry in CORPUS {
            let compiled = compile(entry.src);
            let graph = EvaluationGraph::from_templates(&compiled.templates);
            assert_witnesses_reached(entry, &compiled.templates, &graph);
            assert_replay_equals_graph(entry, &compiled.templates, &graph);
            assert_guard_contributions_are_bool_lets(entry, &compiled.templates);
        }
        assert_parent_override_is_kept();
        assert_same_id_contributions_resolve_last_writer_wins();
    }

    /// Anti-vacuity: each input still reaches the arm it was chosen for.
    fn assert_witnesses_reached(
        entry: &CorpusEntry,
        templates: &[TopologyTemplate],
        graph: &EvaluationGraph,
    ) {
        let graph_ids: Vec<String> = graph.value_cells.keys().map(ToString::to_string).collect();
        let contributed_ids: Vec<String> = contributed_cells(templates)
            .map(|c| c.graph_id().to_string())
            .collect();
        for witness in entry.witnesses {
            let entity = witness.split('.').next().unwrap_or(witness);
            for (source, ids) in [("graph", &graph_ids), ("contributed", &contributed_ids)] {
                assert!(
                    ids.iter().any(|id| id.contains(witness)),
                    "corpus staleness ({}): no {source} cell id contains `{witness}`, so \
                     this input no longer reaches its arm. Fix the input, not the \
                     witness. {source} ids under `{entity}`: {:#?}",
                    entry.arm,
                    ids.iter()
                        .filter(|id| id.starts_with(entity))
                        .collect::<Vec<_>>(),
                );
            }
        }
    }

    fn assert_replay_equals_graph(
        entry: &CorpusEntry,
        templates: &[TopologyTemplate],
        graph: &EvaluationGraph,
    ) {
        let replayed = replay(templates);
        let mut only_in_graph: Vec<String> = graph
            .value_cells
            .keys()
            .filter(|id| !replayed.contains_key(*id))
            .map(ToString::to_string)
            .collect();
        let mut only_in_replay: Vec<String> = replayed
            .keys()
            .filter(|id| !graph.value_cells.contains_key(*id))
            .map(ToString::to_string)
            .collect();
        only_in_graph.sort();
        only_in_replay.sort();
        assert!(
            only_in_graph.is_empty() && only_in_replay.is_empty(),
            "({}) replaying contributed_cells does not reproduce graph.value_cells' \
             ids.\nin graph only: {only_in_graph:#?}\nin replay only: {only_in_replay:#?}",
            entry.arm,
        );

        for (id, node) in graph.value_cells.iter() {
            let cell = &replayed[id];
            assert_eq!(
                cell.cell_type(),
                &node.cell_type,
                "({}) `{id}`: contributed cell_type (left) != graph cell_type (right)",
                entry.arm,
            );
            assert_eq!(
                cell.kind(),
                node.kind,
                "({}) `{id}`: contributed kind (left) != graph kind (right)",
                entry.arm,
            );
            assert_eq!(
                cell.default_expr().map(|e| e.content_hash),
                node.default_expr.as_ref().map(|e| e.content_hash),
                "({}) `{id}`: contributed default_expr hash (left) != graph's (right)",
                entry.arm,
            );
        }
    }

    fn assert_guard_contributions_are_bool_lets(
        entry: &CorpusEntry,
        templates: &[TopologyTemplate],
    ) {
        for cell in contributed_cells(templates) {
            let id = cell.graph_id();
            if id.member.starts_with("__guard_") {
                assert_eq!(
                    (cell.cell_type(), cell.kind()),
                    (&Type::Bool, ValueCellKind::Let),
                    "({}) guard cell `{id}` must be a Bool let",
                    entry.arm,
                );
            }
        }
    }

    /// The task-3806 precedence branch is exercised, not merely tolerated: the
    /// child-default contribution for `A.b.bore` is offered as KeepExisting and
    /// loses to the parent's `auto` override.
    fn assert_parent_override_is_kept() {
        let compiled = compile(PARENT_OVERRIDE.src);
        let graph = EvaluationGraph::from_templates(&compiled.templates);
        let bore = ValueCellId::new("A.b", "bore");
        assert!(
            contributed_cells(&compiled.templates)
                .any(|c| c.insert_policy() == InsertPolicy::KeepExisting && c.graph_id() == bore),
            "({}) no KeepExisting contribution for `{bore}`",
            PARENT_OVERRIDE.arm,
        );
        let kind = graph.value_cells.get(&bore).map(|n| n.kind);
        assert!(
            kind.is_some_and(|k| k.is_auto()),
            "({}) graph `{bore}` must keep the parent's Auto override, got {kind:?}",
            PARENT_OVERRIDE.arm,
        );
    }

    /// Iteration order is insertion order: the generic template's
    /// `Bearing.seal : TypeParam("T")` comes first, and the graph holds the
    /// LAST same-id contribution — the overwrite that makes a successful
    /// monomorph safe to evaluate.
    fn assert_same_id_contributions_resolve_last_writer_wins() {
        let compiled = compile(SUCCESSFUL_MONOMORPH.src);
        let graph = EvaluationGraph::from_templates(&compiled.templates);
        let seal = ValueCellId::new("Bearing", "seal");
        let seal_types: Vec<&Type> = contributed_cells(&compiled.templates)
            .filter(|c| c.graph_id() == seal)
            .map(|c| c.cell_type())
            .collect();
        let generic = Type::TypeParam("T".to_string());
        assert_eq!(
            seal_types.first(),
            Some(&&generic),
            "({}) the generic template's `{seal}` must be contributed first; \
             contributions in order: {seal_types:#?}",
            SUCCESSFUL_MONOMORPH.arm,
        );
        assert_ne!(
            seal_types.last(),
            Some(&&generic),
            "({}) a later monomorph contribution must overwrite the generic \
             `{seal}`, or this input does not exercise ordering; contributions \
             in order: {seal_types:#?}",
            SUCCESSFUL_MONOMORPH.arm,
        );
        assert_eq!(
            graph.value_cells.get(&seal).map(|n| &n.cell_type),
            seal_types.last().copied(),
            "({}) graph `{seal}` (left) must equal the LAST same-id contribution \
             (right); contributions in order: {seal_types:#?}",
            SUCCESSFUL_MONOMORPH.arm,
        );
    }
}
