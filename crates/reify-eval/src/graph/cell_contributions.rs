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
enum InsertPolicy {
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

    /// The id to write this cell at in a value-cell map, or `None` when its
    /// policy keeps the cell `is_present` reports is already there.
    /// `from_templates` folds every contribution through this.
    pub fn landing_id(&self, is_present: impl FnOnce(&ValueCellId) -> bool) -> Option<ValueCellId> {
        let id = self.graph_id();
        let keeps_existing = self.insert_policy() == InsertPolicy::KeepExisting && is_present(&id);
        (!keeps_existing).then_some(id)
    }

    fn insert_policy(&self) -> InsertPolicy {
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
    enumerate(templates, Instances::Every)
}

/// The `cell_type` of every [`contributed_cells`] cell, with each sub's
/// instances collapsed to its first. A sub-instance cell has its child
/// declaration's type whatever its slot, so this yields the same set of types
/// at a cost that does not grow with a collection's instance count.
pub fn contributed_cell_types(templates: &[TopologyTemplate]) -> impl Iterator<Item = &Type> {
    enumerate(templates, Instances::First).map(|cell| cell.cell_type())
}

/// Which of a sub's instance slots [`enumerate`] expands.
#[derive(Clone, Copy)]
enum Instances {
    Every,
    First,
}

impl Instances {
    fn limit(self) -> usize {
        match self {
            Instances::Every => usize::MAX,
            Instances::First => 1,
        }
    }
}

fn enumerate(
    templates: &[TopologyTemplate],
    instances: Instances,
) -> impl Iterator<Item = ContributedCell<'_>> {
    templates
        .iter()
        .flat_map(move |template| template_cells(templates, template, instances))
}

fn template_cells<'a>(
    templates: &'a [TopologyTemplate],
    template: &'a TopologyTemplate,
    instances: Instances,
) -> impl Iterator<Item = ContributedCell<'a>> {
    let own = template.value_cells.iter().map(declared_cell);
    let guarded = template.guarded_groups.iter().flat_map(|group| {
        std::iter::once(ContributedCell(Source::Guard(group)))
            .chain(group.members.iter().map(declared_cell))
            .chain(group.else_members.iter().map(declared_cell))
    });
    own.chain(sub_instance_cells(templates, template, instances))
        .chain(guarded)
}

fn declared_cell(decl: &ValueCellDecl) -> ContributedCell<'_> {
    ContributedCell(Source::Declared(decl))
}

fn sub_instance_cells<'a>(
    templates: &'a [TopologyTemplate],
    parent: &'a TopologyTemplate,
    instances: Instances,
) -> impl Iterator<Item = ContributedCell<'a>> {
    parent
        .sub_components
        .iter()
        .filter_map(move |sub| resolve_sub(templates, parent, sub))
        .flat_map(move |elab| {
            let SubElaboration { sub, child, .. } = elab;
            elab.slot_indices()
                .take(instances.limit())
                .flat_map(move |index| {
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
/// its instances are stamped from, and the shape of its instance slots.
#[derive(Clone, Copy)]
pub(crate) struct SubElaboration<'a> {
    parent: &'a TopologyTemplate,
    pub sub: &'a SubComponentDecl,
    pub child: &'a TopologyTemplate,
    /// The child's structure name: a keyed sub's element type, not "Keyed".
    pub element_structure: &'a str,
    pub shape: SubShape<'a>,
}

#[derive(Clone, Copy)]
pub(crate) enum SubShape<'a> {
    /// `sub xs : List<T>`; its instance count is read only when its cells
    /// are enumerated.
    Collection,
    Keyed(&'a [MemberKey]),
    Single,
}

impl<'a> SubElaboration<'a> {
    /// A collection whose count is not statically known has no slots.
    fn slot_indices(self) -> SlotIndices<'a> {
        match self.shape {
            SubShape::Collection => {
                SlotIndices::Positional(0..static_count(self.parent, self.sub).unwrap_or(0))
            }
            SubShape::Keyed(keys) => SlotIndices::Keyed(keys.iter()),
            SubShape::Single => SlotIndices::Single(std::iter::once(SlotIndex::Single)),
        }
    }
}

/// A sub's instance slots, one variant per [`SubShape`].
enum SlotIndices<'a> {
    Positional(std::ops::Range<i64>),
    Keyed(std::slice::Iter<'a, MemberKey>),
    Single(std::iter::Once<SlotIndex<'a>>),
}

impl<'a> Iterator for SlotIndices<'a> {
    type Item = SlotIndex<'a>;

    fn next(&mut self) -> Option<SlotIndex<'a>> {
        match self {
            SlotIndices::Positional(indices) => indices.next().map(SlotIndex::Positional),
            SlotIndices::Keyed(keys) => keys.next().map(SlotIndex::Keyed),
            SlotIndices::Single(single) => single.next(),
        }
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
        SubShape::Collection
    } else if !sub.keyed_members.is_empty() {
        SubShape::Keyed(&sub.keyed_members)
    } else {
        SubShape::Single
    };
    Some(SubElaboration {
        parent,
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
    use reify_compiler::CompiledModule;
    use reify_constraints::SimpleConstraintChecker;
    use reify_core::ModulePath;

    use super::{contributed_cell_types, contributed_cells};

    fn compile(src: &str) -> CompiledModule {
        let parsed = reify_compiler::parse_with_stdlib(src, ModulePath::single("test"));
        reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker)
    }

    #[test]
    fn contributed_cell_types_does_not_grow_with_instance_count() {
        let rack = |count: u32| {
            compile(&format!(
                "structure def Screw {{ param d : Real = 3.0 }}\n\
                 structure Rack {{\n    sub screws : List<Screw>\n    \
                 constraint screws.count == {count}\n}}\n"
            ))
        };
        let (few, many) = (rack(2), rack(50));
        let cell_count = |m: &CompiledModule| contributed_cells(&m.templates).count();
        let type_count = |m: &CompiledModule| contributed_cell_types(&m.templates).count();
        assert!(
            cell_count(&many) > cell_count(&few),
            "anti-vacuity: 50 screws must contribute more cells than 2 ({} vs {})",
            cell_count(&many),
            cell_count(&few),
        );
        assert_eq!(
            type_count(&many),
            type_count(&few),
            "contributed_cell_types must collapse a collection's instances, so its \
             length is the same for 50 screws (left) as for 2 (right)",
        );
    }
}
