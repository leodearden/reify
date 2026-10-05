//! The value cells [`EvaluationGraph::from_templates`] puts in
//! `graph.value_cells`, enumerated once so callers can ask about them without
//! building the graph.
//!
//! `from_templates` populates its value cells by folding [`contributed_cells`],
//! so this iterator IS that set, not a description of it. A template's
//! `ports.members` never reaches the graph and is deliberately not enumerated.
//!
//! [`EvaluationGraph::from_templates`]: super::EvaluationGraph::from_templates

use reify_compiler::{
    CompiledGuardedGroup, SubComponentDecl, TopologyTemplate, ValueCellDecl, ValueCellKind,
    find_template,
};
use reify_core::{Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, MemberKey, Value};

/// Every guarded group's guard cell is a `Bool` let over its guard expression.
static GUARD_CELL_TYPE: Type = Type::Bool;

/// How a contribution lands in a value-cell map that may already hold its id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertPolicy {
    /// Replace whatever is there: the last contribution for an id wins.
    Overwrite,
    /// Insert only if the id is absent, so a parent's override of a plain
    /// sub's cell (`sub b : Bearing { bore = auto }`, task 3806) survives the
    /// child's default.
    KeepExisting,
}

/// One value cell `from_templates` inserts, in insertion order.
#[derive(Clone, Copy, Debug)]
pub struct ContributedCell<'a>(Source<'a>);

#[derive(Clone, Copy, Debug)]
enum Source<'a> {
    Declared(&'a ValueCellDecl),
    SubInstance {
        slot: InstanceSlot<'a>,
        decl: &'a ValueCellDecl,
    },
    Guard(&'a CompiledGuardedGroup),
}

#[derive(Clone, Copy, Debug)]
struct InstanceSlot<'a> {
    parent: &'a str,
    sub_name: &'a str,
    index: SlotIndex<'a>,
}

#[derive(Clone, Copy, Debug)]
enum SlotIndex<'a> {
    Single,
    Positional(i64),
    Keyed(&'a MemberKey),
}

impl InstanceSlot<'_> {
    fn scoped_entity(&self) -> String {
        let InstanceSlot {
            parent, sub_name, ..
        } = self;
        match self.index {
            SlotIndex::Single => format!("{parent}.{sub_name}"),
            SlotIndex::Positional(i) => format!("{parent}.{sub_name}[{i}]"),
            SlotIndex::Keyed(key) => format!("{parent}.{}", key.path_segment(sub_name)),
        }
    }
}

impl<'a> ContributedCell<'a> {
    pub fn cell_type(&self) -> &'a Type {
        match self.0 {
            Source::Declared(decl) | Source::SubInstance { decl, .. } => &decl.cell_type,
            Source::Guard(_) => &GUARD_CELL_TYPE,
        }
    }

    pub fn kind(&self) -> ValueCellKind {
        match self.0 {
            Source::Declared(decl) | Source::SubInstance { decl, .. } => decl.kind,
            Source::Guard(_) => ValueCellKind::Let,
        }
    }

    pub fn default_expr(&self) -> Option<&'a CompiledExpr> {
        match self.0 {
            Source::Declared(decl) | Source::SubInstance { decl, .. } => decl.default_expr.as_ref(),
            Source::Guard(group) => Some(&group.guard_expr),
        }
    }

    /// The id this cell has in `graph.value_cells`. Allocates for a
    /// sub-instance cell, whose scoped entity is not stored anywhere.
    pub fn graph_id(&self) -> ValueCellId {
        match self.0 {
            Source::Declared(decl) => decl.id.clone(),
            Source::SubInstance { slot, decl } => {
                ValueCellId::new(slot.scoped_entity(), decl.id.member.clone())
            }
            Source::Guard(group) => group.guard_value_cell.clone(),
        }
    }

    pub fn insert_policy(&self) -> InsertPolicy {
        match self.0 {
            Source::SubInstance {
                slot:
                    InstanceSlot {
                        index: SlotIndex::Single,
                        ..
                    },
                ..
            } => InsertPolicy::KeepExisting,
            _ => InsertPolicy::Overwrite,
        }
    }
}

/// Every value cell `from_templates(templates)` inserts, in insertion order.
/// Per template: its own cells, then each resolvable sub's instance cells,
/// then each guarded group's guard cell, members and else-members.
pub fn contributed_cells(
    templates: &[TopologyTemplate],
) -> impl Iterator<Item = ContributedCell<'_>> {
    templates
        .iter()
        .flat_map(move |template| template_cells(templates, template))
}

fn template_cells<'a>(
    templates: &'a [TopologyTemplate],
    template: &'a TopologyTemplate,
) -> impl Iterator<Item = ContributedCell<'a>> {
    let own = template.value_cells.iter().map(declared_cell);
    let guarded = template.guarded_groups.iter().flat_map(|group| {
        std::iter::once(ContributedCell(Source::Guard(group)))
            .chain(group.members.iter().map(declared_cell))
            .chain(group.else_members.iter().map(declared_cell))
    });
    own.chain(sub_instance_cells(templates, template))
        .chain(guarded)
}

fn declared_cell(decl: &ValueCellDecl) -> ContributedCell<'_> {
    ContributedCell(Source::Declared(decl))
}

fn sub_instance_cells<'a>(
    templates: &'a [TopologyTemplate],
    parent: &'a TopologyTemplate,
) -> impl Iterator<Item = ContributedCell<'a>> {
    parent
        .sub_components
        .iter()
        .filter_map(move |sub| resolve_sub(templates, parent, sub))
        .flat_map(move |elab| {
            let SubElaboration {
                sub, child, shape, ..
            } = elab;
            shape.slot_indices().flat_map(move |index| {
                let slot = InstanceSlot {
                    parent: &parent.name,
                    sub_name: &sub.name,
                    index,
                };
                child
                    .value_cells
                    .iter()
                    .map(move |decl| ContributedCell(Source::SubInstance { slot, decl }))
            })
        })
}

/// A sub-component resolved against the template slice: the child template
/// its instances are stamped from, and how many instances there are.
pub(crate) struct SubElaboration<'a> {
    pub sub: &'a SubComponentDecl,
    pub child: &'a TopologyTemplate,
    /// The child's structure name: a keyed sub's element type, not "Keyed".
    pub element_structure: &'a str,
    pub shape: SubShape<'a>,
}

#[derive(Clone, Copy)]
pub(crate) enum SubShape<'a> {
    /// `sub xs : List<T>`; `count` is `None` unless statically known.
    Collection {
        count: Option<i64>,
    },
    Keyed(&'a [MemberKey]),
    Single,
}

impl<'a> SubShape<'a> {
    fn slot_indices(self) -> impl Iterator<Item = SlotIndex<'a>> {
        let (positional, keyed, single) = match self {
            SubShape::Collection { count } => {
                (count.map(|n| (0..n).map(SlotIndex::Positional)), None, None)
            }
            SubShape::Keyed(keys) => (None, Some(keys.iter().map(SlotIndex::Keyed)), None),
            SubShape::Single => (None, None, Some(SlotIndex::Single)),
        };
        positional
            .into_iter()
            .flatten()
            .chain(keyed.into_iter().flatten())
            .chain(single)
    }
}

/// `None` when the sub's structure is not among `templates`; such a sub
/// contributes no cells and is not registered.
pub(crate) fn resolve_sub<'a>(
    templates: &'a [TopologyTemplate],
    parent: &'a TopologyTemplate,
    sub: &'a SubComponentDecl,
) -> Option<SubElaboration<'a>> {
    let element_structure = element_structure_name(sub);
    let child = find_template(templates, element_structure)?;
    let shape = if sub.is_collection {
        SubShape::Collection {
            count: static_count(parent, sub),
        }
    } else if !sub.keyed_members.is_empty() {
        SubShape::Keyed(&sub.keyed_members)
    } else {
        SubShape::Single
    };
    Some(SubElaboration {
        sub,
        child,
        element_structure,
        shape,
    })
}

fn element_structure_name(sub: &SubComponentDecl) -> &str {
    if sub.keyed_members.is_empty() {
        return &sub.structure_name;
    }
    match sub.type_args.first() {
        Some(Type::StructureRef(name)) => name,
        _ => &sub.structure_name,
    }
}

/// The count cell's default when it is an Int literal, or a reference to a
/// parent cell whose default is one.
fn static_count(parent: &TopologyTemplate, sub: &SubComponentDecl) -> Option<i64> {
    let count_expr = cell_default(parent, sub.count_cell.as_ref()?)?;
    match &count_expr.kind {
        CompiledExprKind::ValueRef(referenced) => literal_int(cell_default(parent, referenced)?),
        _ => literal_int(count_expr),
    }
}

fn cell_default<'a>(template: &'a TopologyTemplate, id: &ValueCellId) -> Option<&'a CompiledExpr> {
    template
        .value_cells
        .iter()
        .find(|cell| cell.id == *id)?
        .default_expr
        .as_ref()
}

fn literal_int(expr: &CompiledExpr) -> Option<i64> {
    match expr.kind {
        CompiledExprKind::Literal(Value::Int(n)) => Some(n),
        _ => None,
    }
}

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
            let under_entity = |ids: &[String]| -> Vec<String> {
                ids.iter()
                    .filter(|id| id.starts_with(entity))
                    .cloned()
                    .collect()
            };
            assert!(
                graph_ids.iter().any(|id| id.contains(witness)),
                "corpus staleness ({}): no graph cell id contains `{witness}`, so this \
                 input no longer reaches its arm. Fix the input, not the witness. \
                 graph ids under `{entity}`: {:#?}",
                entry.arm,
                under_entity(&graph_ids),
            );
            assert!(
                contributed_ids.iter().any(|id| id.contains(witness)),
                "({}) from_templates inserts `{witness}` but contributed_cells never \
                 yields it: the enumeration is missing an arm. contributed ids under \
                 `{entity}`: {:#?}",
                entry.arm,
                under_entity(&contributed_ids),
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
