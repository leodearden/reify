//! How the source-unit mode resolves a name or a path to a local reference.
//!
//! A path's root is `self` or a bare name the definition declares; each
//! resolved path becomes one entry of the unit's reference table. Anything
//! that is not such a path is left to the existing compile.

use reify_core::Type;
use reify_ir::CompiledExpr;

use crate::scope::CompilationScope;
use crate::source_unit::SourceUnitRecorder;
use crate::source_unit_tables::{ReferenceRead, ReferenceRoot, SourceStep};

/// A bare name, or `self`, in the source-unit mode: the read of a local
/// reference, or `None` when the existing compile handles the name (a binder,
/// `none`, a built-in constant, an unresolved name).
pub(crate) fn compile_bare_name(
    name: &str,
    scope: &CompilationScope,
    recorder: &SourceUnitRecorder,
) -> Option<CompiledExpr> {
    if name == "self" && scope.is_entity_scope {
        let ty = Type::StructureRef(scope.entity_name.clone());
        return Some(recorder.reference(ReferenceRoot::Own, Vec::new(), ReferenceRead::Value, ty));
    }
    if name == "none" || is_binder(name, scope) {
        return None;
    }
    let ty = own_member_type(name, scope)?;
    Some(recorder.reference(
        ReferenceRoot::Own,
        vec![SourceStep::Member(name.to_string())],
        ReferenceRead::Value,
        ty,
    ))
}

/// A binder of the unit's own expression (a lambda's, a quantifier's or a
/// match arm's), told from a member by its id: binders are minted in an
/// entity of their own, members in the definition's.
fn is_binder(name: &str, scope: &CompilationScope) -> bool {
    scope
        .resolve(name)
        .is_some_and(|(id, _)| id.entity != scope.entity_name)
}

/// The type of `name` if the definition the scope describes declares it.
/// Subs and collections are typed as the instances they hold.
fn own_member_type(name: &str, scope: &CompilationScope) -> Option<Type> {
    if let Some((_, ty)) = scope.resolve(name) {
        return Some(ty.clone());
    }
    if let Some(definition) = scope.sub_component_types.get(name) {
        let instance = Type::StructureRef(definition.clone());
        return Some(if scope.collection_sub_names.contains(name) {
            Type::List(Box::new(instance))
        } else if scope.keyed_sub_keys.contains_key(name) {
            Type::Keyed(Box::new(instance))
        } else {
            instance
        });
    }
    if let Some(group) = scope.resolve_match_arm_group(name) {
        return Some(Type::Union(
            group.arms.iter().map(|arm| arm.arm_type.clone()).collect(),
        ));
    }
    scope
        .geometry_realization_names
        .contains(name)
        .then_some(Type::Geometry)
}
