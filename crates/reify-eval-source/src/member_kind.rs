//! What kind of member each written form is.

use reify_ast::MemberDecl;
use reify_core::Type;

/// The kind of a member: what the evaluator's table of member kinds maps to a
/// recipe. It carries no data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberKind {
    /// A `param` or `let` whose value is not geometry.
    Cell,
    /// A `param` or `let` whose value is geometry, which only a kernel makes.
    GeometryMember,
    Constraint,
    /// `constraint Name(..)`, applying a `constraint def`.
    AppliedConstraintDefinition,
    /// A single `sub`.
    Sub,
    /// A list or keyed `sub`.
    Collection,
    /// `minimize` or `maximize`.
    Objective,
    /// `where <guard> { .. } else { .. }`.
    GuardedGroup,
    /// A declaration-level `match`.
    MatchGroup,
    Port,
    Connect,
    Chain,
    Relate,
    Sketch,
    /// `meta { .. }`.
    Meta,
    /// `forall v in c: constraint ..`.
    ForallConstraint,
    /// `forall v in c: connect ..` or `.. : chain ..`.
    ForallConnect,
    /// A `fn` written in a definition or trait body.
    AssociatedFunction,
    /// A trait's `type` member.
    AssociatedType,
}

/// The kind of the member `decl` writes. A `param` or `let` is a [`Cell`]
/// here; whether its value is geometry is [`cell_kind`]'s to decide.
///
/// [`Cell`]: MemberKind::Cell
pub(crate) fn form_kind(decl: &MemberDecl) -> MemberKind {
    match decl {
        MemberDecl::Param(_) | MemberDecl::Let(_) => MemberKind::Cell,
        MemberDecl::Constraint(_) => MemberKind::Constraint,
        MemberDecl::ConstraintInst(_) => MemberKind::AppliedConstraintDefinition,
        MemberDecl::Sub(sub) => {
            if sub.is_collection || !sub.keyed_members.is_empty() {
                MemberKind::Collection
            } else {
                MemberKind::Sub
            }
        }
        MemberDecl::Minimize(_) | MemberDecl::Maximize(_) => MemberKind::Objective,
        MemberDecl::GuardedGroup(_) => MemberKind::GuardedGroup,
        MemberDecl::MatchArmDeclGroup(_) => MemberKind::MatchGroup,
        MemberDecl::AssociatedType(_) => MemberKind::AssociatedType,
        MemberDecl::Fn(_) => MemberKind::AssociatedFunction,
        MemberDecl::Port(_) => MemberKind::Port,
        MemberDecl::Connect(_) => MemberKind::Connect,
        MemberDecl::Chain(_) => MemberKind::Chain,
        MemberDecl::MetaBlock(_) => MemberKind::Meta,
        MemberDecl::ForallConnect(_) => MemberKind::ForallConnect,
        MemberDecl::ForallConstraint(_) => MemberKind::ForallConstraint,
        MemberDecl::Relate(_) => MemberKind::Relate,
        MemberDecl::Sketch(_) => MemberKind::Sketch,
    }
}

/// The kind of a `param` or `let` given its types: the type its table entry
/// carries and the type the existing compile gave it. It holds geometry when
/// either does, since the expression compiler types a geometry builtin's call
/// as a dimensionless placeholder and only the existing compile's
/// classification of the member says it is geometry.
pub(crate) fn cell_kind<'t>(types: impl IntoIterator<Item = &'t Type>) -> MemberKind {
    if types.into_iter().any(is_geometry_type) {
        MemberKind::GeometryMember
    } else {
        MemberKind::Cell
    }
}

/// Whether a value of type `ty` is geometry, or a collection or option of it:
/// something only a kernel produces.
fn is_geometry_type(ty: &Type) -> bool {
    match ty {
        Type::Geometry | Type::Feature | Type::Selector(_) | Type::AnySelector => true,
        Type::List(element) | Type::Set(element) | Type::Keyed(element) | Type::Option(element) => {
            is_geometry_type(element)
        }
        _ => false,
    }
}
