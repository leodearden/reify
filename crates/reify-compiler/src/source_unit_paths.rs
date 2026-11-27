//! How the source-unit mode resolves a name or a path to a local reference.
//!
//! A path's root is `self` or a bare name the definition declares; its steps
//! are `.member`, `[n]` with a literal `n >= 0`, and `["key"]`. Each resolved
//! path becomes one entry of the unit's reference table. What is not such a
//! path is compiled as usual, with its receiver resolved as the longest path it
//! starts with.

use std::collections::HashSet;

use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan, Type};
use reify_ir::{CompiledExpr, MemberKey};

use crate::datum_projection::{DatumProjectionResolution, datum_projection_result_type};
use crate::expr::{
    WILDCARD_STRUCTURE_KIND, keyed_computed_index_poison, keyed_missing_key_poison,
    make_poison_literal,
};
use crate::scope::CompilationScope;
use crate::source_unit::SourceUnitRecorder;
use crate::source_unit_tables::{ReferenceRead, ReferenceRoot, SourceStep, StructuralQuery};
use crate::types::{GuardedDeclGroup, TopologyTemplate};

/// Why the source-unit mode does not compile an expression as a path.
pub(crate) enum NotAPath {
    /// Compile the receiver as usual and type the member, or the index, by the
    /// receiver's type.
    ByReceiverType,
    /// The existing compile handles the whole expression (`meta.<key>`).
    Existing,
}

/// A path's compile: a read of a reference (or a coded poison), or why it is
/// not a path.
pub(crate) type PathOutcome = Result<CompiledExpr, NotAPath>;

/// A bare name, or `self`, in the source-unit mode: the read of a local
/// reference, or `None` when the existing compile handles the name (a binder,
/// `none`, a built-in constant, an unresolved name).
pub(crate) fn compile_bare_name(
    name: &str,
    scope: &CompilationScope,
    recorder: &SourceUnitRecorder,
) -> Option<CompiledExpr> {
    let (steps, target) = root_target(name, scope).ok()?;
    let ty = target.value_type(scope)?;
    Some(recorder.reference(ReferenceRoot::Own, steps, ReferenceRead::Value, ty))
}

/// A member read or an index whose receiver is a path, in the source-unit mode.
pub(crate) fn compile_member_path(
    expr: &reify_ast::Expr,
    scope: &CompilationScope,
    recorder: &SourceUnitRecorder,
    diagnostics: &mut Vec<Diagnostic>,
) -> PathOutcome {
    let (root, hops) = path_of(expr).ok_or(NotAPath::ByReceiverType)?;
    let (mut steps, mut target) = root_target(root, scope)?;
    let last = hops.len().saturating_sub(1);
    for (position, hop) in hops.iter().enumerate() {
        match next_target(target, hop, &mut steps, scope, expr.span, diagnostics) {
            Ok(Next::Into(next)) => target = next,
            Ok(Next::Datum { name, ty }) if position == last => {
                let ty_of_prefix = Type::StructureRef(name.definition);
                let receiver = recorder.reference(
                    ReferenceRoot::Own,
                    steps,
                    ReferenceRead::Value,
                    ty_of_prefix,
                );
                return Ok(CompiledExpr::method_call(receiver, name.member, vec![], ty));
            }
            Ok(Next::Datum { .. }) => return Err(NotAPath::ByReceiverType),
            Err(Halt::Poison(poison)) => return Ok(*poison),
            Err(Halt::NotAPath(why)) => return Err(why),
        }
    }
    let ty = target.value_type(scope).ok_or(NotAPath::ByReceiverType)?;
    Ok(recorder.reference(ReferenceRoot::Own, steps, ReferenceRead::Value, ty))
}

/// One step of a path as the AST writes it.
enum Hop<'e> {
    Member(&'e str),
    Index(&'e reify_ast::Expr),
}

/// `expr` as a root name followed by its hops, root first; `None` when the
/// innermost receiver is not a name.
fn path_of(expr: &reify_ast::Expr) -> Option<(&str, Vec<Hop<'_>>)> {
    let mut hops = Vec::new();
    let mut at = expr;
    loop {
        match &at.kind {
            reify_ast::ExprKind::Ident(name) => {
                hops.reverse();
                return Some((name, hops));
            }
            reify_ast::ExprKind::MemberAccess { object, member } => {
                hops.push(Hop::Member(member));
                at = object;
            }
            reify_ast::ExprKind::IndexAccess { object, index } => {
                hops.push(Hop::Index(index));
                at = object;
            }
            _ => return None,
        }
    }
}

/// The definition whose members a hop looks up.
#[derive(Clone)]
enum Definition {
    /// The one the compile scope describes, read through the scope's maps.
    Own,
    /// Any other, read through the scope's template registry.
    Named(String),
}

/// What a path's prefix reaches.
enum Target<'s> {
    Instance(Definition),
    Collection(String),
    Keyed {
        definition: String,
        keys: DeclaredKeys<'s>,
    },
    Port {
        definition: Definition,
        port: String,
    },
    Cell(Type),
    Geometry,
    /// A match-arm cluster: one sub whose definition each arm chooses.
    Cluster(Vec<Type>),
}

impl Target<'_> {
    /// The type of a read of the thing itself; `None` for a port, which is not
    /// a value.
    fn value_type(&self, scope: &CompilationScope) -> Option<Type> {
        let instance = |definition: &str| Type::StructureRef(definition.to_string());
        Some(match self {
            Target::Instance(Definition::Own) => instance(&scope.entity_name),
            Target::Instance(Definition::Named(definition)) => instance(definition),
            Target::Collection(definition) => Type::List(Box::new(instance(definition))),
            Target::Keyed { definition, .. } => Type::Keyed(Box::new(instance(definition))),
            Target::Port { .. } => return None,
            Target::Cell(ty) => ty.clone(),
            Target::Geometry => Type::Geometry,
            Target::Cluster(arms) => Type::Union(arms.clone()),
        })
    }
}

/// The keys a keyed sub declares.
#[derive(Clone, Copy)]
enum DeclaredKeys<'s> {
    /// From the compile scope's map of the unit's own keyed subs.
    Own(&'s HashSet<String>),
    /// From the template of the definition that declares the sub.
    Template(&'s [MemberKey]),
}

impl DeclaredKeys<'_> {
    fn contains(self, key: &str) -> bool {
        match self {
            DeclaredKeys::Own(keys) => keys.contains(key),
            DeclaredKeys::Template(keys) => keys.iter().any(|k| k.0 == key),
        }
    }
}

/// An intrinsic datum of an instance (`origin`, `frame`, ...).
struct DatumName {
    definition: String,
    member: String,
}

/// Where a hop leads.
enum Next<'s> {
    Into(Target<'s>),
    /// An intrinsic datum of the prefix: a projection of the prefix's
    /// reference, so it can only end a path.
    Datum {
        name: DatumName,
        ty: Type,
    },
}

/// Why a path stops before its last hop.
enum Halt {
    /// A coded error, already reported.
    Poison(Box<CompiledExpr>),
    NotAPath(NotAPath),
}

/// The root of a path: `self`, or a member of the definition.
fn root_target<'s>(
    name: &str,
    scope: &'s CompilationScope,
) -> Result<(Vec<SourceStep>, Target<'s>), NotAPath> {
    if name == "self" && scope.is_entity_scope {
        return Ok((Vec::new(), Target::Instance(Definition::Own)));
    }
    if name == "none" || is_binder(name, scope) {
        return Err(NotAPath::ByReceiverType);
    }
    match member_of(&Definition::Own, name, scope) {
        Some(target) => Ok((vec![SourceStep::Member(name.to_string())], target)),
        None if name == "meta" => Err(NotAPath::Existing),
        None => Err(NotAPath::ByReceiverType),
    }
}

/// A binder of the unit's own expression (a lambda's, a quantifier's or a
/// match arm's), told from a member by its id: binders are minted in an
/// entity of their own, members in the definition's.
fn is_binder(name: &str, scope: &CompilationScope) -> bool {
    scope
        .resolve(name)
        .is_some_and(|(id, _)| id.entity != scope.entity_name)
}

fn next_target<'s>(
    target: Target<'s>,
    hop: &Hop,
    steps: &mut Vec<SourceStep>,
    scope: &'s CompilationScope,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Next<'s>, Halt> {
    let by_receiver_type = Err(Halt::NotAPath(NotAPath::ByReceiverType));
    match (target, hop) {
        (Target::Instance(definition), Hop::Member(member)) => {
            member_hop(&definition, member, steps, scope, span, diagnostics)
        }
        (Target::Cell(Type::StructureRef(definition)), Hop::Member(member))
            if definition != WILDCARD_STRUCTURE_KIND =>
        {
            member_hop(
                &Definition::Named(definition),
                member,
                steps,
                scope,
                span,
                diagnostics,
            )
        }
        (Target::Cell(Type::TypeParam(param)), Hop::Member(member)) => {
            let bounds = scope.type_param_bounds.get(&param);
            let ty = bounds
                .into_iter()
                .flatten()
                .find_map(|bound| trait_member_type(bound, member, scope));
            cell_hop(ty, member, steps).ok_or(Halt::NotAPath(NotAPath::ByReceiverType))
        }
        (Target::Cell(Type::TraitObject(bound)), Hop::Member(member)) => {
            let ty = trait_member_type(&bound, member, scope);
            cell_hop(ty, member, steps).ok_or(Halt::NotAPath(NotAPath::ByReceiverType))
        }
        (Target::Cluster(arms), Hop::Member(member)) => match arm_member(&arms, member, scope) {
            Some(target) => {
                steps.push(SourceStep::Member((*member).to_string()));
                Ok(Next::Into(target))
            }
            None => {
                let cluster = match steps.last() {
                    Some(SourceStep::Member(name)) => name.as_str(),
                    _ => "",
                };
                let owner = format!("match-arm sub '{cluster}'");
                Err(member_not_found(&owner, member, span, diagnostics))
            }
        },
        (Target::Collection(definition), Hop::Index(index)) => match literal_position(index) {
            Some(position) => {
                steps.push(SourceStep::Index(position));
                Ok(Next::Into(Target::Instance(Definition::Named(definition))))
            }
            None => by_receiver_type,
        },
        (Target::Keyed { definition, keys }, Hop::Index(index)) => {
            keyed_hop(definition, keys, index, steps, span, diagnostics)
        }
        (Target::Port { definition, port }, Hop::Member(member)) => {
            match port_member_type(&definition, &port, member, scope) {
                Some(ty) => {
                    steps.push(SourceStep::Member((*member).to_string()));
                    Ok(Next::Into(Target::Cell(ty)))
                }
                None => Err(member_not_found(
                    &format!("port '{port}'"),
                    member,
                    span,
                    diagnostics,
                )),
            }
        }
        _ => by_receiver_type,
    }
}

/// A `.member` hop on an instance of `definition`.
fn member_hop<'s>(
    definition: &Definition,
    member: &str,
    steps: &mut Vec<SourceStep>,
    scope: &'s CompilationScope,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Next<'s>, Halt> {
    if let Some(target) = member_of(definition, member, scope) {
        steps.push(SourceStep::Member(member.to_string()));
        return Ok(Next::Into(target));
    }
    let name = match definition {
        Definition::Own => scope.entity_name.clone(),
        Definition::Named(name) if template_of(name, scope).is_some() => name.clone(),
        Definition::Named(_) => return Err(Halt::NotAPath(NotAPath::ByReceiverType)),
    };
    if let DatumProjectionResolution::Resolved(ty) =
        datum_projection_result_type(&Type::StructureRef(name.clone()), member)
    {
        let name = DatumName {
            definition: name,
            member: member.to_string(),
        };
        return Ok(Next::Datum { name, ty });
    }
    if StructuralQuery::from_member(member).is_some() {
        return Err(Halt::NotAPath(NotAPath::ByReceiverType));
    }
    Err(member_not_found(
        &format!("structure '{name}'"),
        member,
        span,
        diagnostics,
    ))
}

/// A `.member` hop whose type a trait contract gives.
fn cell_hop<'s>(ty: Option<Type>, member: &str, steps: &mut Vec<SourceStep>) -> Option<Next<'s>> {
    let ty = ty?;
    steps.push(SourceStep::Member(member.to_string()));
    Some(Next::Into(Target::Cell(ty)))
}

/// A `["key"]` hop on a keyed sub.
fn keyed_hop<'s>(
    definition: String,
    keys: DeclaredKeys,
    index: &reify_ast::Expr,
    steps: &mut Vec<SourceStep>,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Next<'s>, Halt> {
    let sub = match steps.last() {
        Some(SourceStep::Member(sub)) => sub.clone(),
        _ => return Err(Halt::NotAPath(NotAPath::ByReceiverType)),
    };
    let element = Type::StructureRef(definition.clone());
    let reify_ast::ExprKind::StringLiteral(key) = &index.kind else {
        let poison = keyed_computed_index_poison(diagnostics, &sub, span, element);
        return Err(Halt::Poison(Box::new(poison)));
    };
    if !keys.contains(key) {
        let poison = keyed_missing_key_poison(diagnostics, &sub, key, span, element);
        return Err(Halt::Poison(Box::new(poison)));
    }
    steps.push(SourceStep::Key(key.clone()));
    Ok(Next::Into(Target::Instance(Definition::Named(definition))))
}

/// The one lookup of a member of a definition, own or not.
fn member_of<'s>(
    definition: &Definition,
    member: &str,
    scope: &'s CompilationScope,
) -> Option<Target<'s>> {
    match definition {
        Definition::Own => own_member(member, scope),
        Definition::Named(name) => template_member(template_of(name, scope)?, member),
    }
}

/// A member of the definition the scope describes, from the scope's maps.
fn own_member<'s>(member: &str, scope: &'s CompilationScope) -> Option<Target<'s>> {
    if let Some((id, ty)) = scope.resolve(member)
        && id.entity == scope.entity_name
    {
        return Some(Target::Cell(ty.clone()));
    }
    if let Some(definition) = scope.sub_component_types.get(member) {
        let definition = definition.clone();
        if scope.collection_sub_names.contains(member) {
            return Some(Target::Collection(definition));
        }
        if let Some(keys) = scope.keyed_sub_keys.get(member) {
            let keys = DeclaredKeys::Own(keys);
            return Some(Target::Keyed { definition, keys });
        }
        return Some(Target::Instance(Definition::Named(definition)));
    }
    if let Some(group) = scope.resolve_match_arm_group(member) {
        return Some(cluster(group));
    }
    if scope.geometry_realization_names.contains(member) {
        return Some(Target::Geometry);
    }
    scope.port_names.contains(member).then(|| Target::Port {
        definition: Definition::Own,
        port: member.to_string(),
    })
}

/// A member of another definition, from its template.
fn template_member<'s>(template: &'s TopologyTemplate, member: &str) -> Option<Target<'s>> {
    let guarded = template
        .guarded_groups
        .iter()
        .flat_map(|g| g.member_decls());
    if let Some(cell) = template
        .value_cells
        .iter()
        .chain(guarded)
        .find(|cell| cell.id.member == member && !member.starts_with("__"))
    {
        return Some(Target::Cell(cell.cell_type.clone()));
    }
    if let Some(group) = template.match_arm_groups.iter().find(|g| g.name == member) {
        return Some(cluster(group));
    }
    if let Some(sub) = template
        .sub_components
        .iter()
        .find(|sub| sub.name == member)
    {
        let definition = sub.element_structure_name().to_string();
        return Some(if sub.is_collection {
            Target::Collection(definition)
        } else if !sub.keyed_members.is_empty() {
            let keys = DeclaredKeys::Template(&sub.keyed_members);
            Target::Keyed { definition, keys }
        } else {
            Target::Instance(Definition::Named(definition))
        });
    }
    if template
        .realizations
        .iter()
        .any(|r| r.name.as_deref() == Some(member))
    {
        return Some(Target::Geometry);
    }
    template
        .ports
        .iter()
        .any(|port| port.name == member)
        .then(|| Target::Port {
            definition: Definition::Named(template.name.clone()),
            port: member.to_string(),
        })
}

fn cluster<'s>(group: &GuardedDeclGroup) -> Target<'s> {
    Target::Cluster(group.arms.iter().map(|arm| arm.arm_type.clone()).collect())
}

/// `member` of a match-arm cluster: what the arms that declare it agree on, or
/// a `Union` of their types. An arm that lacks it is not an error, since that
/// arm may not be the one chosen; `None` when no arm declares it.
fn arm_member<'s>(arms: &[Type], member: &str, scope: &'s CompilationScope) -> Option<Target<'s>> {
    let mut found = arms.iter().filter_map(|arm| match arm {
        Type::StructureRef(definition) => {
            member_of(&Definition::Named(definition.clone()), member, scope)
        }
        _ => None,
    });
    let first = found.next()?;
    let mut types: Vec<Type> = first.value_type(scope).into_iter().collect();
    for ty in found.filter_map(|target| target.value_type(scope)) {
        if !types.contains(&ty) {
            types.push(ty);
        }
    }
    Some(if types.len() > 1 {
        Target::Cell(Type::Union(types))
    } else {
        first
    })
}

fn template_of<'s>(name: &str, scope: &'s CompilationScope) -> Option<&'s TopologyTemplate> {
    scope.template_registry?.get(name).copied()
}

fn port_member_type(
    definition: &Definition,
    port: &str,
    member: &str,
    scope: &CompilationScope,
) -> Option<Type> {
    match definition {
        Definition::Own => scope
            .resolve(&format!("{port}.{member}"))
            .map(|(_, ty)| ty.clone()),
        Definition::Named(name) => template_of(name, scope)?
            .ports
            .iter()
            .find(|p| p.name == port)?
            .members
            .iter()
            .find(|cell| cell.id.member == member)
            .map(|cell| cell.cell_type.clone()),
    }
}

fn trait_member_type(bound: &str, member: &str, scope: &CompilationScope) -> Option<Type> {
    scope.trait_member_types.get(bound)?.get(member).cloned()
}

/// A literal index `n >= 0`.
fn literal_position(index: &reify_ast::Expr) -> Option<i64> {
    let reify_ast::ExprKind::NumberLiteral { value, is_real } = &index.kind else {
        return None;
    };
    match reify_ast::classify_number_literal(*value, *is_real) {
        reify_ast::NumberClass::Int(position) if position >= 0 => Some(position),
        _ => None,
    }
}

fn member_not_found(
    owner: &str,
    member: &str,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Halt {
    let poison = make_poison_literal(
        diagnostics,
        Diagnostic::error(format!("{owner} has no member '{member}'"))
            .with_code(DiagnosticCode::StructureMemberNotFound)
            .with_label(DiagnosticLabel::new(span, "no such member")),
    );
    Halt::Poison(Box::new(poison))
}
