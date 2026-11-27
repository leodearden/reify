//! The source-unit mode of the expression compiler.
//!
//! A source unit is one expression the second output compiles on its own. In
//! this mode a reference the expression reads is local to the unit: it is
//! minted in [`LOCAL_REFERENCE_ENTITY`] and described by an entry of the
//! unit's reference table, rather than stamped with a definition's name. The
//! mode rides on the compile scope, and only [`compile_source_unit`] turns it
//! on, so every other caller of the expression compiler gets the existing
//! compile.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

use reify_core::{Diagnostic, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, CompiledFunction};

use crate::expr::compile_expr_guarded_with_expected;
use crate::scope::CompilationScope;
use crate::source_unit_tables::{
    CompiledSourceUnit, InstanceRequest, LOCAL_REFERENCE_ENTITY, LocalReference, ReferenceRead,
    ReferenceRoot, RequestArgument, SourceStep,
};

/// Which compile an expression gets.
#[derive(Clone, Copy, Default)]
pub(crate) enum ExprMode<'u> {
    #[default]
    Existing,
    SourceUnit(&'u SourceUnitRecorder<'u>),
}

/// Which scopes a unit's names resolve against.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum SourceUnitNames<'s> {
    /// The unit is written in the definition the compile scope describes.
    Own,
    /// The unit is written in a sub's braces, and the compile scope describes
    /// the sub's definition. A name is first a `let` of the same braces (one
    /// of `lets`, with its type), then a member of the sub's definition
    /// whether present or not, then a member of the definition that wrote the
    /// braces, which `writer` describes; `self` is the writer (PRD §5.4).
    Braces {
        lets: &'s BTreeMap<String, Type>,
        writer: &'s CompilationScope<'s>,
    },
}

#[doc(hidden)]
pub struct SourceUnitSpec<'s> {
    pub names: SourceUnitNames<'s>,
    pub expected_type: Option<&'s Type>,
}

/// The unit being compiled: how its names resolve, and the table of the
/// references it reads.
pub(crate) struct SourceUnitRecorder<'u> {
    names: SourceUnitNames<'u>,
    references: RefCell<Vec<LocalReference>>,
}

impl<'u> SourceUnitRecorder<'u> {
    fn new(names: SourceUnitNames<'u>) -> Self {
        SourceUnitRecorder {
            names,
            references: RefCell::default(),
        }
    }

    pub(crate) fn names(&self) -> SourceUnitNames<'u> {
        self.names
    }

    /// A read of the reference `(root, steps, read)`, typed `ty`. A reference
    /// read twice is one table entry.
    pub(crate) fn reference(
        &self,
        root: ReferenceRoot,
        steps: Vec<SourceStep>,
        read: ReferenceRead,
        ty: Type,
    ) -> CompiledExpr {
        let known = self
            .references
            .borrow()
            .iter()
            .find(|r| r.root == root && r.steps == steps && r.read == read)
            .map(|r| r.id.clone());
        let id = known.unwrap_or_else(|| self.mint(root, steps, read));
        CompiledExpr::value_ref(id, ty)
    }

    /// A read of the unknown an `auto` argument for `parameter` stands for,
    /// typed as the parameter. Never shared: each `auto` written is its own
    /// unknown.
    pub(crate) fn argument_unknown(&self, parameter: &str, ty: Type) -> CompiledExpr {
        let read = ReferenceRead::ArgumentUnknown {
            parameter: parameter.to_string(),
        };
        let id = self.mint(ReferenceRoot::Own, Vec::new(), read);
        CompiledExpr::value_ref(id, ty)
    }

    fn mint(
        &self,
        root: ReferenceRoot,
        steps: Vec<SourceStep>,
        read: ReferenceRead,
    ) -> ValueCellId {
        let mut references = self.references.borrow_mut();
        let id = ValueCellId::new(LOCAL_REFERENCE_ENTITY, references.len().to_string());
        references.push(LocalReference {
            id: id.clone(),
            root,
            steps,
            read,
        });
        id
    }
}

/// Compile `expr`, written as `spec` says, on the scope of the definition it
/// belongs to, minting references local to the unit.
#[doc(hidden)]
// G-allow: the second output's unit lowering in reify-eval-source calls this (#10643)
pub fn compile_source_unit(
    expr: &reify_ast::Expr,
    scope: &CompilationScope,
    spec: &SourceUnitSpec,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledSourceUnit {
    let recorder = SourceUnitRecorder::new(spec.names);
    let mut unit_scope = scope.clone();
    unit_scope.expr_mode = ExprMode::SourceUnit(&recorder);
    let tree = compile_expr_guarded_with_expected(
        expr,
        &unit_scope,
        enum_defs,
        functions,
        diagnostics,
        None,
        &mut 0,
        spec.expected_type,
    );
    drop(unit_scope);
    let read = ids_read(&tree);
    let references: Vec<LocalReference> = recorder
        .references
        .into_inner()
        .into_iter()
        .filter(|r| read.contains(&r.id))
        .collect();
    debug_assert_reads_are_local(&read, &references);
    let instance_requests = instance_requests(&tree, &references);
    CompiledSourceUnit {
        expr: tree,
        references,
        instance_requests,
    }
}

/// One request per constructor node of the tree, in pre-order. A request is
/// keyed by its place, the child indices from the root in
/// `CompiledExpr::for_each_child` order, because a unit's content holds no
/// source position (PRD §5.2).
fn instance_requests(tree: &CompiledExpr, references: &[LocalReference]) -> Vec<InstanceRequest> {
    fn visit(
        node: &CompiledExpr,
        place: &mut Vec<usize>,
        references: &[LocalReference],
        requests: &mut Vec<InstanceRequest>,
    ) {
        if let CompiledExprKind::StructureInstanceCtor {
            type_name,
            ordered_args,
            ..
        } = &node.kind
        {
            requests.push(InstanceRequest {
                place: place.clone(),
                definition: type_name.clone(),
                arguments: ordered_args
                    .iter()
                    .map(|(parameter, argument)| RequestArgument {
                        parameter: parameter.clone(),
                        unknown: argument_unknown_read(argument, references),
                    })
                    .collect(),
            });
        }
        let mut index = 0;
        node.for_each_child(&mut |child| {
            place.push(index);
            visit(child, place, references, requests);
            place.pop();
            index += 1;
        });
    }
    let mut requests = Vec::new();
    visit(tree, &mut Vec::new(), references, &mut requests);
    requests
}

/// The unknown an argument is, when it is the read of an `auto` argument's
/// unknown (see [`SourceUnitRecorder::argument_unknown`]).
fn argument_unknown_read(
    argument: &CompiledExpr,
    references: &[LocalReference],
) -> Option<ValueCellId> {
    let CompiledExprKind::ValueRef(id) = &argument.kind else {
        return None;
    };
    references
        .iter()
        .find(|r| &r.id == id)
        .filter(|r| matches!(r.read, ReferenceRead::ArgumentUnknown { .. }))
        .map(|r| r.id.clone())
}

/// The mode's invariant: every id the tree reads is a table entry or a
/// binder minted under the unit's stamp, so no read names a definition. A
/// form the mode's hooks miss reds here, naming the id it leaked.
fn debug_assert_reads_are_local(read: &HashSet<ValueCellId>, references: &[LocalReference]) {
    let binder_suffix = format!(".{LOCAL_REFERENCE_ENTITY}");
    let leaked: Vec<&ValueCellId> = read
        .iter()
        .filter(|id| !references.iter().any(|r| &r.id == *id))
        .filter(|id| !(id.entity.starts_with('$') && id.entity.ends_with(&binder_suffix)))
        .collect();
    debug_assert!(
        leaked.is_empty(),
        "a source unit reads ids that are neither local references nor its binders: {leaked:?}"
    );
}

/// Every cell id the tree reads, lambda bodies included. A table entry minted
/// in a subtree the compile discarded is not among them.
fn ids_read(tree: &CompiledExpr) -> HashSet<ValueCellId> {
    let mut ids = HashSet::new();
    tree.walk(&mut |node| match &node.kind {
        CompiledExprKind::ValueRef(id)
        | CompiledExprKind::DeterminacyPredicate { cell: id, .. } => {
            ids.insert(id.clone());
        }
        _ => {}
    });
    ids
}
