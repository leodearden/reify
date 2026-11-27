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
use std::collections::HashSet;

use reify_core::{Diagnostic, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, CompiledFunction};

use crate::expr::compile_expr_guarded_with_expected;
use crate::scope::CompilationScope;
use crate::source_unit_tables::{
    CompiledSourceUnit, LOCAL_REFERENCE_ENTITY, LocalReference, ReferenceRead, ReferenceRoot,
    SourceStep,
};

/// Which compile an expression gets.
#[derive(Clone, Copy, Default)]
pub(crate) enum ExprMode<'u> {
    #[default]
    Existing,
    SourceUnit(&'u SourceUnitRecorder),
}

/// Which scopes a unit's names resolve against.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum SourceUnitNames {
    /// The unit is written in the definition the compile scope describes.
    Own,
}

#[doc(hidden)]
pub struct SourceUnitSpec<'s> {
    pub names: SourceUnitNames,
    pub expected_type: Option<&'s Type>,
}

/// The reference table of the unit being compiled.
#[derive(Default)]
pub(crate) struct SourceUnitRecorder {
    references: RefCell<Vec<LocalReference>>,
}

impl SourceUnitRecorder {
    /// A read of the reference `(root, steps, read)`, typed `ty`. A reference
    /// read twice is one table entry.
    pub(crate) fn reference(
        &self,
        root: ReferenceRoot,
        steps: Vec<SourceStep>,
        read: ReferenceRead,
        ty: Type,
    ) -> CompiledExpr {
        let mut references = self.references.borrow_mut();
        let known = references
            .iter()
            .find(|r| r.root == root && r.steps == steps && r.read == read)
            .map(|r| r.id.clone());
        let id = known.unwrap_or_else(|| {
            let id = ValueCellId::new(LOCAL_REFERENCE_ENTITY, references.len().to_string());
            references.push(LocalReference {
                id: id.clone(),
                root,
                steps,
                read,
            });
            id
        });
        CompiledExpr::value_ref(id, ty)
    }
}

/// Compile `expr`, written as `spec` says, on the scope of the definition it
/// belongs to, minting references local to the unit.
#[doc(hidden)]
pub fn compile_source_unit(
    expr: &reify_ast::Expr,
    scope: &CompilationScope,
    spec: &SourceUnitSpec,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledSourceUnit {
    let recorder = SourceUnitRecorder::default();
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
    let references = recorder
        .references
        .into_inner()
        .into_iter()
        .filter(|r| read.contains(&r.id))
        .collect();
    CompiledSourceUnit {
        expr: tree,
        references,
        instance_requests: Vec::new(),
    }
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
