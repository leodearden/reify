//! `from_templates` builds `graph.value_cells` by folding
//! [`contributed_cells`]. These tests pin that fold from outside: a corpus of
//! `.ri` inputs reaching every value-cell arm, each carrying hand-measured
//! graph cells, plus the precedence and ordering the fold must keep.

use std::collections::{HashMap, HashSet};

use reify_compiler::{CompiledModule, TopologyTemplate, ValueCellKind};
use reify_constraints::SimpleConstraintChecker;
use reify_core::{ContentHash, ModulePath, Type, ValueCellId};

use super::EvaluationGraph;
use super::cell_contributions::{ContributedCell, contributed_cell_types, contributed_cells};

/// A `.ri` input chosen to reach one `from_templates` value-cell arm.
struct CorpusEntry {
    arm: &'static str,
    src: &'static str,
    /// Cells measured by hand in `from_templates`' output, proving the input
    /// reaches its arm. Never derived from the enumeration under test.
    expected: &'static [ExpectedCell],
}

struct ExpectedCell {
    entity: &'static str,
    member: &'static str,
    kind: ValueCellKind,
    cell_type: fn() -> Type,
}

impl ExpectedCell {
    fn id(&self) -> ValueCellId {
        ValueCellId::new(self.entity, self.member)
    }
}

const PARAM: ValueCellKind = ValueCellKind::Param;
const LET: ValueCellKind = ValueCellKind::Let;
const AUTO: ValueCellKind = ValueCellKind::Auto { free: false };

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
    expected: &[
        ExpectedCell {
            entity: "Fitting",
            member: "__guard_0",
            kind: LET,
            cell_type: || Type::Bool,
        },
        ExpectedCell {
            entity: "Fitting",
            member: "ribs",
            kind: PARAM,
            cell_type: || Type::Int,
        },
        ExpectedCell {
            entity: "Fitting",
            member: "finish",
            kind: PARAM,
            cell_type: || Type::Enum("Finish".to_string()),
        },
    ],
};

const COLLECTION_LITERAL_COUNT: CorpusEntry = CorpusEntry {
    arm: "collection sub, literal count",
    src: r#"structure def Screw { param d : Real = 3.0 }
structure Rack {
    sub screws : List<Screw>
    constraint screws.count == 2
}
"#,
    expected: &[
        ExpectedCell {
            entity: "Rack.screws[0]",
            member: "d",
            kind: PARAM,
            cell_type: Type::dimensionless_scalar,
        },
        ExpectedCell {
            entity: "Rack.screws[1]",
            member: "d",
            kind: PARAM,
            cell_type: Type::dimensionless_scalar,
        },
    ],
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
    expected: &[
        ExpectedCell {
            entity: "Manifold.vents[\"intake\"]",
            member: "area",
            kind: PARAM,
            cell_type: Type::dimensionless_scalar,
        },
        ExpectedCell {
            entity: "Manifold.vents[\"exhaust\"]",
            member: "area",
            kind: PARAM,
            cell_type: Type::dimensionless_scalar,
        },
    ],
};

const PLAIN_SUB: CorpusEntry = CorpusEntry {
    arm: "plain (non-collection) sub",
    src: r#"structure def Inner { param x : Real = 1.0 }
structure Outer { sub i = Inner() }
"#,
    expected: &[ExpectedCell {
        entity: "Outer.i",
        member: "x",
        kind: PARAM,
        cell_type: Type::dimensionless_scalar,
    }],
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
    expected: &[
        ExpectedCell {
            entity: "Bearing",
            member: "seal",
            kind: PARAM,
            cell_type: || Type::StructureRef("GasketSeal".to_string()),
        },
        ExpectedCell {
            entity: "Assembly.b",
            member: "seal",
            kind: PARAM,
            cell_type: || Type::StructureRef("GasketSeal".to_string()),
        },
    ],
};

const PARENT_OVERRIDE: CorpusEntry = CorpusEntry {
    arm: "parent `auto` override of a plain sub's cell (task 3806 precedence)",
    src: r#"structure Bearing { param bore : Length = 5mm }
structure A {
    sub b : Bearing { bore = auto }
    constraint self.b.bore == 10mm
}
"#,
    expected: &[ExpectedCell {
        entity: "A.b",
        member: "bore",
        kind: AUTO,
        cell_type: Type::length,
    }],
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
    expected: &[ExpectedCell {
        entity: "Tray.screws[2]",
        member: "d",
        kind: PARAM,
        cell_type: Type::dimensionless_scalar,
    }],
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

/// The value-cell map a consumer gets by writing every contribution, in
/// iteration order, where its `landing_id` says: `from_templates`' fold
/// without the node construction.
fn replay(templates: &[TopologyTemplate]) -> HashMap<ValueCellId, ContributedCell<'_>> {
    let mut cells = HashMap::new();
    for cell in contributed_cells(templates) {
        if let Some(id) = cell.landing_id(|id| cells.contains_key(id)) {
            cells.insert(id, cell);
        }
    }
    cells
}

/// Each input's graph holds its hand-measured cells and well-formed guard
/// cells, and replaying the enumeration reproduces `graph.value_cells` id for
/// id, so a value-cell insert that bypasses the fold goes red on whichever
/// input reaches it. Every input is checked before the test fails.
#[test]
fn contributed_cells_replay_reproduces_from_templates_value_cells() {
    let failing_arms: Vec<&str> = CORPUS
        .iter()
        .filter(|entry| std::panic::catch_unwind(|| assert_fold_oracle(entry)).is_err())
        .map(|entry| entry.arm)
        .collect();
    assert!(
        failing_arms.is_empty(),
        "the value-cell fold oracle failed on {failing_arms:#?}; each failure's \
         panic message is printed above",
    );
}

fn assert_fold_oracle(entry: &CorpusEntry) {
    let compiled = compile(entry.src);
    let graph = EvaluationGraph::from_templates(&compiled.templates);
    assert_expected_cells(entry, &graph);
    assert_guard_cells_are_bool_lets_over_their_guards(entry, &compiled.templates, &graph);
    assert_replay_equals_graph(entry, &compiled.templates, &graph);
}

fn assert_expected_cells(entry: &CorpusEntry, graph: &EvaluationGraph) {
    for expected in entry.expected {
        let id = expected.id();
        let Some(node) = graph.value_cells.get(&id) else {
            let mut under_entity: Vec<String> = graph
                .value_cells
                .keys()
                .filter(|graph_id| graph_id.entity.starts_with(expected.entity))
                .map(ToString::to_string)
                .collect();
            under_entity.sort();
            panic!(
                "corpus staleness ({}): the graph has no `{id}`, so this input no \
                 longer reaches its arm. Fix the input, not the measurement. graph \
                 ids under `{}`: {under_entity:#?}",
                entry.arm, expected.entity,
            );
        };
        assert_eq!(
            (node.kind, &node.cell_type),
            (expected.kind, &(expected.cell_type)()),
            "({}) graph `{id}` (left) != hand-measured (kind, type) (right)",
            entry.arm,
        );
    }
}

/// Every guard cell `graph.guarded_groups` names is a `Bool` let whose
/// default is its template group's guard expression.
fn assert_guard_cells_are_bool_lets_over_their_guards(
    entry: &CorpusEntry,
    templates: &[TopologyTemplate],
    graph: &EvaluationGraph,
) {
    let guard_exprs: HashMap<&ValueCellId, ContentHash> = templates
        .iter()
        .flat_map(|template| &template.guarded_groups)
        .map(|group| (&group.guard_value_cell, group.guard_expr.content_hash))
        .collect();
    for group in &graph.guarded_groups {
        let guard = &group.guard_cell;
        let node = graph.value_cells.get(guard).unwrap_or_else(|| {
            panic!(
                "({}) guarded group's guard `{guard}` has no graph cell",
                entry.arm
            )
        });
        assert_eq!(
            (node.kind, &node.cell_type),
            (ValueCellKind::Let, &Type::Bool),
            "({}) guard `{guard}` must be a Bool let",
            entry.arm,
        );
        assert_eq!(
            node.default_expr.as_ref().map(|e| e.content_hash),
            guard_exprs.get(guard).copied(),
            "({}) guard `{guard}`'s default (left) must be its template group's \
             guard expression (right)",
            entry.arm,
        );
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

/// The task-3806 precedence branch is exercised, not merely tolerated: a
/// contribution for `A.b.bore` defers to a cell already present, and the
/// graph keeps the parent's `auto` override.
#[test]
fn plain_sub_child_default_keeps_parent_override() {
    let compiled = compile(PARENT_OVERRIDE.src);
    let graph = EvaluationGraph::from_templates(&compiled.templates);
    let bore = ValueCellId::new("A.b", "bore");
    assert!(
        contributed_cells(&compiled.templates)
            .any(|c| c.graph_id() == bore && c.landing_id(|_| true).is_none()),
        "({}) no contribution for `{bore}` defers to a cell already present",
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
#[test]
fn same_id_contributions_resolve_last_writer_wins() {
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

/// Over every arm, collapsing a sub's instances loses no type.
#[test]
fn contributed_cell_types_are_the_contributed_cells_type_set() {
    for entry in CORPUS {
        let compiled = compile(entry.src);
        let full: HashSet<&Type> = contributed_cells(&compiled.templates)
            .map(|c| c.cell_type())
            .collect();
        let collapsed: HashSet<&Type> = contributed_cell_types(&compiled.templates).collect();
        assert_eq!(
            collapsed, full,
            "({}) contributed_cell_types (left) must yield exactly the types \
             contributed_cells does (right)",
            entry.arm,
        );
    }
}
