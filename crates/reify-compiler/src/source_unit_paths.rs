//! How the source-unit mode resolves a name or a path to a local reference.
//!
//! A path's root is `self` or a bare name the definition declares; its steps
//! are `.member`, `[n]` with a literal `n >= 0`, and `["key"]`. Each resolved
//! path becomes one entry of the unit's reference table. What is not such a
//! path is compiled as usual, with its receiver resolved as the longest path it
//! starts with.

use std::collections::HashSet;

use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledFunction, MemberKey};

use crate::datum_projection::{DatumProjectionResolution, datum_projection_result_type};
use crate::expr::{
    WILDCARD_STRUCTURE_KIND, is_declared_param, keyed_computed_index_poison,
    keyed_missing_key_poison, make_poison_literal,
};
use crate::scope::CompilationScope;
use crate::source_unit::{SourceUnitNames, SourceUnitRecorder};
use crate::source_unit_tables::{ReferenceRead, ReferenceRoot, SourceStep, StructuralQuery};
use crate::types::{GuardedDeclGroup, TopologyTemplate, ValueCellDecl};

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
    let (root, steps, target) = root_target(name, scope, recorder.names()).ok()?;
    let ty = target.value_type()?;
    Some(recorder.reference(root, steps, ReferenceRead::Value, ty))
}

/// A member read or an index whose receiver is a path, in the source-unit mode.
pub(crate) fn compile_member_path(
    expr: &reify_ast::Expr,
    scope: &CompilationScope,
    recorder: &SourceUnitRecorder,
    diagnostics: &mut Vec<Diagnostic>,
) -> PathOutcome {
    let Walked { root, steps, end } = match walk_path(expr, scope, recorder.names(), diagnostics) {
        Ok(walked) => walked,
        Err(Halt::Poison(poison)) => return Ok(*poison),
        Err(Halt::NotAPath(why)) => return Err(why),
    };
    match end {
        End::Target(target) => {
            let ty = target.value_type().ok_or(NotAPath::ByReceiverType)?;
            Ok(recorder.reference(root, steps, ReferenceRead::Value, ty))
        }
        End::Query(query) => {
            let instance = Type::StructureRef(WILDCARD_STRUCTURE_KIND.to_string());
            let ty = Type::List(Box::new(instance));
            Ok(recorder.reference(root, steps, ReferenceRead::Query(query), ty))
        }
        End::Datum { name, ty } => {
            let ty_of_prefix = Type::StructureRef(name.definition);
            let receiver = recorder.reference(root, steps, ReferenceRead::Value, ty_of_prefix);
            Ok(CompiledExpr::method_call(receiver, name.member, vec![], ty))
        }
    }
}

/// `present(<path>)` in the source-unit mode: a read of whether the thing the
/// path reaches is present, which the evaluator's caller tests. `None` leaves
/// the call to the existing compile: a user's function named `present`
/// shadows the test, and an argument that is not a path is not one.
pub(crate) fn compile_presence(
    args: &[reify_ast::Expr],
    arg_names: &[Option<String>],
    scope: &CompilationScope,
    recorder: &SourceUnitRecorder,
    functions: &[CompiledFunction],
) -> Option<CompiledExpr> {
    if functions.iter().any(|function| function.name == "present") {
        return None;
    }
    let ([argument], [None]) = (args, arg_names) else {
        return None;
    };
    // An argument that does not resolve cleanly is compiled, and its errors
    // reported, by the existing compile.
    let mut unreported = Vec::new();
    let Ok(Walked {
        root,
        steps,
        end: End::Target(target),
    }) = walk_path(argument, scope, recorder.names(), &mut unreported)
    else {
        return None;
    };
    target.value_type()?;
    Some(recorder.reference(root, steps, ReferenceRead::Presence, Type::Bool))
}

/// `sub.(Trait::member)` in the source-unit mode: the sub is a path's root,
/// resolved as any root is (so in a sub's braces it may be the writer's), the
/// trait is checked against the sub's definition, and the member is one hop.
pub(crate) fn compile_qualified_member(
    sub: &str,
    trait_name: &str,
    member: &str,
    scope: &CompilationScope,
    recorder: &SourceUnitRecorder,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledExpr {
    match qualified_member(sub, trait_name, member, scope, recorder, span, diagnostics) {
        Ok(read) => read,
        Err(poison) => *poison,
    }
}

fn qualified_member(
    sub: &str,
    trait_name: &str,
    member: &str,
    scope: &CompilationScope,
    recorder: &SourceUnitRecorder,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<CompiledExpr, Box<CompiledExpr>> {
    let rooted = root_target(sub, scope, recorder.names()).ok();
    let (root, mut steps, target) = match rooted {
        Some(rooted @ (_, _, Target::Instance(Definition::Named(_)) | Target::Cluster(_))) => {
            rooted
        }
        _ => return Err(unknown_sub(sub, span, diagnostics)),
    };
    let conforms = |ty: &Type| match ty {
        Type::StructureRef(definition) => trait_bounds_of(definition, scope)
            .is_some_and(|bounds| bounds.iter().any(|bound| bound == trait_name)),
        _ => false,
    };
    let conforming = match &target {
        Target::Instance(Definition::Named(definition)) => {
            conforms(&Type::StructureRef(definition.clone()))
        }
        Target::Cluster(arms) => arms.iter().any(conforms),
        _ => false,
    };
    if !conforming {
        return Err(Box::new(make_poison_literal(
            diagnostics,
            Diagnostic::error(format!(
                "sub-component '{sub}' does not implement trait '{trait_name}'"
            ))
            .with_code(DiagnosticCode::TraitNotImplemented)
            .with_label(DiagnosticLabel::new(span, "trait not implemented")),
        )));
    }
    let declared_by_trait = scope
        .trait_members
        .get(trait_name)
        .is_none_or(|members| members.contains(member));
    let hop = Hop::Member(member);
    let next = if declared_by_trait {
        next_target(target, &hop, &mut steps, scope, span, diagnostics)
    } else {
        Err(Halt::NotAPath(NotAPath::ByReceiverType))
    };
    match next {
        Ok(Next::Into(target)) if let Some(ty) = target.value_type() => {
            Ok(recorder.reference(root, steps, ReferenceRead::Value, ty))
        }
        Err(Halt::Poison(poison)) => Err(poison),
        Ok(_) | Err(Halt::NotAPath(_)) => {
            let owner = format!("trait '{trait_name}'");
            Err(no_such_member(&owner, member, span, diagnostics))
        }
    }
}

/// The traits a definition declares conformance to.
fn trait_bounds_of<'s>(definition: &str, scope: &'s CompilationScope) -> Option<&'s [String]> {
    match template_of(definition, scope) {
        Some(template) => Some(&template.trait_bounds),
        None => scope
            .sub_structure_traits
            .get(definition)
            .map(Vec::as_slice),
    }
}

fn unknown_sub(
    sub: &str,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Box<CompiledExpr> {
    Box::new(make_poison_literal(
        diagnostics,
        Diagnostic::error(format!("unknown sub-component '{sub}'"))
            .with_code(DiagnosticCode::UnresolvedName)
            .with_label(DiagnosticLabel::new(span, "unknown sub-component")),
    ))
}

/// Where a path written in the source leads: from `root`, through `steps`,
/// to `end`.
struct Walked<'s> {
    root: ReferenceRoot,
    steps: Vec<SourceStep>,
    end: End<'s>,
}

enum End<'s> {
    /// The steps reach `target`.
    Target(Target<'s>),
    /// A structural query of the instance the steps reach.
    Query(StructuralQuery),
    /// An intrinsic datum of the instance the steps reach.
    Datum { name: DatumName, ty: Type },
}

/// Resolve `expr` as a path, hop by hop.
fn walk_path<'s>(
    expr: &reify_ast::Expr,
    scope: &'s CompilationScope<'s>,
    names: SourceUnitNames<'s>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Walked<'s>, Halt> {
    let (name, hops) = path_of(expr).ok_or(Halt::NotAPath(NotAPath::ByReceiverType))?;
    let ((root, mut steps, mut target), hops) =
        match opened_binder(name, &hops, scope, expr.span, diagnostics)? {
            Some(root) => (root, &hops[1..]),
            None => {
                let root = root_target(name, scope, names).map_err(Halt::NotAPath)?;
                (root, &hops[..])
            }
        };
    let last = hops.len().saturating_sub(1);
    for (position, hop) in hops.iter().enumerate() {
        let end = match next_target(target, hop, &mut steps, scope, expr.span, diagnostics)? {
            Next::Into(next) => {
                target = next;
                continue;
            }
            Next::Datum { .. } | Next::Query(_) if position != last => {
                return Err(Halt::NotAPath(NotAPath::ByReceiverType));
            }
            Next::Datum { name, ty } => End::Datum { name, ty },
            Next::Query(query) => End::Query(query),
        };
        return Ok(Walked { root, steps, end });
    }
    let end = End::Target(target);
    Ok(Walked { root, steps, end })
}

/// A path that opens the structure value a binder of the unit's own
/// expression holds, from its first hop `.m`: `None` when the root is not
/// such a binder. The evaluator binds a binder locally and a value carries
/// its parameters as fields (PRD §5.4), so `x.<parameter>` stays the
/// existing field projection; any other member of the value's definition (a
/// `let`, a geometry member, a sub) is reached by opening the value.
fn opened_binder<'s>(
    name: &str,
    hops: &[Hop],
    scope: &'s CompilationScope<'s>,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<Rooted<'s>>, Halt> {
    let not_a_path = Halt::NotAPath(NotAPath::ByReceiverType);
    let Some((id, ty)) = binder(name, scope) else {
        return Ok(None);
    };
    let (Type::StructureRef(definition), Some(Hop::Member(member))) = (ty, hops.first()) else {
        return Err(not_a_path);
    };
    let Some(template) = template_of(definition, scope) else {
        return Err(not_a_path);
    };
    if declared_cells(template).any(|cell| cell.id.member == *member && is_declared_param(cell)) {
        return Err(not_a_path);
    }
    let Some(target) = template_member(template, member) else {
        let intrinsic = StructuralQuery::from_member(member).is_some()
            || matches!(
                datum_projection_result_type(ty, member),
                DatumProjectionResolution::Resolved(_)
            );
        if intrinsic {
            return Err(not_a_path);
        }
        let owner = format!("structure '{definition}'");
        return Err(member_not_found(&owner, member, span, diagnostics));
    };
    let root = ReferenceRoot::Binder { id: id.clone() };
    let steps = vec![SourceStep::Member((*member).to_string())];
    Ok(Some((root, steps, target)))
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
enum Definition<'s> {
    /// The one a scope describes (the compile scope, or the writer of a sub's
    /// braces), read through that scope's maps.
    OfScope(&'s CompilationScope<'s>),
    /// Any other, read through the compile scope's template registry.
    Named(String),
}

/// What a path's prefix reaches.
enum Target<'s> {
    Instance(Definition<'s>),
    Collection(String),
    Keyed {
        definition: String,
        keys: DeclaredKeys<'s>,
    },
    Port {
        definition: Definition<'s>,
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
    fn value_type(&self) -> Option<Type> {
        let instance = |definition: &str| Type::StructureRef(definition.to_string());
        Some(match self {
            Target::Instance(Definition::OfScope(own)) => instance(&own.entity_name),
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
    /// A structural query of the prefix, which also can only end a path.
    Query(StructuralQuery),
}

/// Why a path stops before its last hop.
enum Halt {
    /// A coded error, already reported.
    Poison(Box<CompiledExpr>),
    NotAPath(NotAPath),
}

/// A path's root: where its steps start, the steps the root itself takes,
/// and what they reach.
type Rooted<'s> = (ReferenceRoot, Vec<SourceStep>, Target<'s>);

/// The root of a path, `self` or a bare name, resolved in the order `names`
/// gives: in a sub's braces a `let` of the braces, then a member of the sub's
/// definition, then a member of the writer; `self` is the writer.
fn root_target<'s>(
    name: &str,
    scope: &'s CompilationScope<'s>,
    names: SourceUnitNames<'s>,
) -> Result<Rooted<'s>, NotAPath> {
    if name == "none" || binder(name, scope).is_some() {
        return Err(NotAPath::ByReceiverType);
    }
    let (lets, declaring, (self_root, writer)) = match names {
        SourceUnitNames::Own => {
            let own = (ReferenceRoot::Own, scope);
            (None, vec![own.clone()], own)
        }
        SourceUnitNames::Braces { lets, writer } => {
            let writer = (ReferenceRoot::Writer, writer);
            let own = (ReferenceRoot::Own, scope);
            (Some(lets), vec![own, writer.clone()], writer)
        }
    };
    if name == "self" {
        if !writer.is_entity_scope {
            return Err(NotAPath::ByReceiverType);
        }
        let target = Target::Instance(Definition::OfScope(writer));
        return Ok((self_root, Vec::new(), target));
    }
    let steps = vec![SourceStep::Member(name.to_string())];
    if let Some(ty) = lets.and_then(|lets| lets.get(name)) {
        return Ok((ReferenceRoot::BracesLet, steps, Target::Cell(ty.clone())));
    }
    for (root, declarer) in declaring {
        if let Some(target) = own_member(name, declarer) {
            return Ok((root, steps, target));
        }
    }
    Err(if name == "meta" {
        NotAPath::Existing
    } else {
        NotAPath::ByReceiverType
    })
}

/// A binder of the unit's own expression (a lambda's, a quantifier's or a
/// match arm's), with its id and type. A binder is told from a member by its
/// id: binders are minted in an entity of their own, members in the
/// definition's.
fn binder<'s>(name: &str, scope: &'s CompilationScope) -> Option<(&'s ValueCellId, &'s Type)> {
    scope
        .resolve(name)
        .filter(|(id, _)| id.entity != scope.entity_name)
}

fn next_target<'s>(
    target: Target<'s>,
    hop: &Hop,
    steps: &mut Vec<SourceStep>,
    scope: &'s CompilationScope<'s>,
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
            let definition = Definition::Named(definition);
            match member_hop(&definition, member, steps, scope, span, diagnostics)? {
                // A structural query is asked of an instance, not of a value.
                Next::Query(_) => by_receiver_type,
                next => Ok(next),
            }
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
    definition: &Definition<'s>,
    member: &str,
    steps: &mut Vec<SourceStep>,
    scope: &'s CompilationScope<'s>,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Next<'s>, Halt> {
    if let Some(target) = member_of(definition, member, scope) {
        steps.push(SourceStep::Member(member.to_string()));
        return Ok(Next::Into(target));
    }
    let name = match definition {
        Definition::OfScope(own) => own.entity_name.clone(),
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
    if let Some(query) = StructuralQuery::from_member(member) {
        return Ok(Next::Query(query));
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
    definition: &Definition<'s>,
    member: &str,
    scope: &'s CompilationScope<'s>,
) -> Option<Target<'s>> {
    match definition {
        Definition::OfScope(own) => own_member(member, own),
        Definition::Named(name) => template_member(template_of(name, scope)?, member),
    }
}

/// The kinds of member a definition declares.
#[derive(Clone, Copy)]
enum MemberKind {
    Cell,
    Cluster,
    Sub,
    Geometry,
    Port,
}

/// The order a member name is looked up in. A cluster precedes a sub because
/// each arm's sub is also recorded under the cluster's name, and the cluster
/// is what the name means.
const MEMBER_LOOKUP_ORDER: [MemberKind; 5] = [
    MemberKind::Cell,
    MemberKind::Cluster,
    MemberKind::Sub,
    MemberKind::Geometry,
    MemberKind::Port,
];

/// A member of the definition the scope describes, from the scope's maps.
fn own_member<'s>(member: &str, scope: &'s CompilationScope<'s>) -> Option<Target<'s>> {
    MEMBER_LOOKUP_ORDER
        .into_iter()
        .find_map(|kind| own_member_of_kind(kind, member, scope))
}

fn own_member_of_kind<'s>(
    kind: MemberKind,
    member: &str,
    scope: &'s CompilationScope<'s>,
) -> Option<Target<'s>> {
    match kind {
        MemberKind::Cell => scope
            .resolve(member)
            .filter(|(id, _)| id.entity == scope.entity_name)
            .map(|(_, ty)| Target::Cell(ty.clone())),
        MemberKind::Cluster => scope.resolve_match_arm_group(member).map(cluster),
        MemberKind::Sub => {
            let definition = scope.sub_component_types.get(member)?.clone();
            if scope.collection_sub_names.contains(member) {
                return Some(Target::Collection(definition));
            }
            if let Some(keys) = scope.keyed_sub_keys.get(member) {
                let keys = DeclaredKeys::Own(keys);
                return Some(Target::Keyed { definition, keys });
            }
            Some(Target::Instance(Definition::Named(definition)))
        }
        MemberKind::Geometry => scope
            .geometry_realization_names
            .contains(member)
            .then_some(Target::Geometry),
        MemberKind::Port => scope.port_names.contains(member).then(|| Target::Port {
            definition: Definition::OfScope(scope),
            port: member.to_string(),
        }),
    }
}

/// A member of another definition, from its template.
fn template_member<'s>(template: &'s TopologyTemplate, member: &str) -> Option<Target<'s>> {
    MEMBER_LOOKUP_ORDER
        .into_iter()
        .find_map(|kind| template_member_of_kind(kind, template, member))
}

fn template_member_of_kind<'s>(
    kind: MemberKind,
    template: &'s TopologyTemplate,
    member: &str,
) -> Option<Target<'s>> {
    match kind {
        MemberKind::Cell => declared_cells(template)
            .find(|cell| cell.id.member == member && !member.starts_with("__"))
            .map(|cell| Target::Cell(cell.cell_type.clone())),
        MemberKind::Cluster => template
            .match_arm_groups
            .iter()
            .find(|g| g.name == member)
            .map(cluster),
        MemberKind::Sub => {
            let sub = template
                .sub_components
                .iter()
                .find(|sub| sub.name == member)?;
            let definition = sub.element_structure_name().to_string();
            Some(if sub.is_collection {
                Target::Collection(definition)
            } else if !sub.keyed_members.is_empty() {
                let keys = DeclaredKeys::Template(&sub.keyed_members);
                Target::Keyed { definition, keys }
            } else {
                Target::Instance(Definition::Named(definition))
            })
        }
        MemberKind::Geometry => template
            .realizations
            .iter()
            .any(|r| r.name.as_deref() == Some(member))
            .then_some(Target::Geometry),
        MemberKind::Port => template
            .ports
            .iter()
            .any(|port| port.name == member)
            .then(|| Target::Port {
                definition: Definition::Named(template.name.clone()),
                port: member.to_string(),
            }),
    }
}

/// A definition's value cells, guarded ones included.
fn declared_cells(template: &TopologyTemplate) -> impl Iterator<Item = &ValueCellDecl> {
    let guarded = template
        .guarded_groups
        .iter()
        .flat_map(|g| g.member_decls());
    template.value_cells.iter().chain(guarded)
}

fn cluster<'s>(group: &GuardedDeclGroup) -> Target<'s> {
    Target::Cluster(group.arms.iter().map(|arm| arm.arm_type.clone()).collect())
}

/// `member` of a match-arm cluster: what the arms that declare it agree on, or
/// a `Union` of their types. An arm that lacks it is not an error, since that
/// arm may not be the one chosen; `None` when no arm declares it.
fn arm_member<'s>(
    arms: &[Type],
    member: &str,
    scope: &'s CompilationScope<'s>,
) -> Option<Target<'s>> {
    let mut found = arms.iter().filter_map(|arm| match arm {
        Type::StructureRef(definition) => {
            member_of(&Definition::Named(definition.clone()), member, scope)
        }
        _ => None,
    });
    let first = found.next()?;
    let mut types: Vec<Type> = first.value_type().into_iter().collect();
    for ty in found.filter_map(|target| target.value_type()) {
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
        Definition::OfScope(own) => own
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
    Halt::Poison(no_such_member(owner, member, span, diagnostics))
}

fn no_such_member(
    owner: &str,
    member: &str,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Box<CompiledExpr> {
    Box::new(make_poison_literal(
        diagnostics,
        Diagnostic::error(format!("{owner} has no member '{member}'"))
            .with_code(DiagnosticCode::StructureMemberNotFound)
            .with_label(DiagnosticLabel::new(span, "no such member")),
    ))
}
