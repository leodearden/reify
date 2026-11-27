//! A definition's member table: every member it declares or inherits, in
//! order, each with its kind and declared type.

use reify_ast::{MemberDecl, SubDecl, TypeExprKind};
use reify_compiler::TopologyTemplate;
use reify_core::{
    DefinitionName, Diagnostic, DiagnosticCode, DiagnosticLabel, PathName, SourceSpan, Type,
};

use crate::member_kind::{self, MemberKind};
use crate::module_context::ModuleContext;

/// One member of a definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The member's own name; `None` for a form that names nothing.
    pub name: Option<PathName>,
    pub kind: MemberKind,
    pub declared_type: DeclaredType,
    pub origin: MemberOrigin,
    /// Written with a trailing `where <guard>`.
    pub guarded: bool,
    /// The members a guarded group holds (its then-members, then its
    /// else-members) or a match group holds (one branch per arm); empty for
    /// every other kind.
    pub branches: Vec<MemberTable>,
}

/// What a member's form says its value is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredType {
    /// The form declares no type.
    None,
    /// A cell's type.
    Value(Type),
    /// The definition a sub instantiates, or a collection's element.
    Definition(DefinitionName),
    /// A port's type or an applied constraint definition's name, as written;
    /// the leaf that lowers the member resolves it.
    Written(PathName),
}

/// Where a member is declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberOrigin {
    /// In the definition's own body.
    Own,
    /// In the body of this trait, which the definition conforms to.
    Inherited(DefinitionName),
}

/// A definition's members, in order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemberTable {
    members: Vec<Member>,
}

impl MemberTable {
    pub fn iter(&self) -> std::slice::Iter<'_, Member> {
        self.members.iter()
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

impl<'t> IntoIterator for &'t MemberTable {
    type Item = &'t Member;
    type IntoIter = std::slice::Iter<'t, Member>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// The table of the definition `definition`, whose own body is `members` and
/// whose existing-compile template is `template`.
pub(crate) fn lower_definition(
    definition: &DefinitionName,
    members: &[MemberDecl],
    template: &TopologyTemplate,
    module: &ModuleContext<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> MemberTable {
    let mut lowering = Lowering {
        definition,
        template,
        module,
        diagnostics,
    };
    lowering.table(members, &MemberOrigin::Own)
}

/// What lowering one definition's members reads and reports into.
struct Lowering<'l, 'm> {
    definition: &'l DefinitionName,
    template: &'l TopologyTemplate,
    module: &'l ModuleContext<'m>,
    diagnostics: &'l mut Vec<Diagnostic>,
}

impl Lowering<'_, '_> {
    fn table(&mut self, members: &[MemberDecl], origin: &MemberOrigin) -> MemberTable {
        MemberTable {
            members: members
                .iter()
                .map(|decl| self.member(decl, origin))
                .collect(),
        }
    }

    fn member(&mut self, decl: &MemberDecl, origin: &MemberOrigin) -> Member {
        let kind = member_kind::form_kind(decl);
        let (name, guard, declared_type, branches) = match decl {
            MemberDecl::Param(param) => (
                Some(param.name.as_str()),
                param.where_clause.as_ref(),
                self.cell_type(&param.name, param.span),
                Vec::new(),
            ),
            MemberDecl::Let(binding) => {
                let declared_type = match binding.type_expr {
                    Some(_) => self.cell_type(&binding.name, binding.span),
                    None => DeclaredType::None,
                };
                (
                    Some(binding.name.as_str()),
                    binding.where_clause.as_ref(),
                    declared_type,
                    Vec::new(),
                )
            }
            MemberDecl::Constraint(constraint) => (
                None,
                constraint.where_clause.as_ref(),
                DeclaredType::None,
                Vec::new(),
            ),
            MemberDecl::ConstraintInst(applied) => (
                None,
                applied.where_clause.as_ref(),
                DeclaredType::Written(identifier(&applied.name)),
                Vec::new(),
            ),
            MemberDecl::Sub(sub) => {
                let element = if sub.keyed_members.is_empty() {
                    sub.structure_name.as_str()
                } else {
                    keyed_element(sub).unwrap_or(sub.structure_name.as_str())
                };
                (
                    Some(sub.name.as_str()),
                    sub.where_clause.as_ref(),
                    self.sub_definition(&sub.name, element, sub.span),
                    Vec::new(),
                )
            }
            MemberDecl::Minimize(objective) => (
                None,
                objective.where_clause.as_ref(),
                DeclaredType::None,
                Vec::new(),
            ),
            MemberDecl::Maximize(objective) => (
                None,
                objective.where_clause.as_ref(),
                DeclaredType::None,
                Vec::new(),
            ),
            MemberDecl::GuardedGroup(group) => (
                None,
                None,
                DeclaredType::None,
                vec![
                    self.table(&group.members, origin),
                    self.table(&group.else_members, origin),
                ],
            ),
            MemberDecl::MatchArmDeclGroup(group) => (
                None,
                None,
                DeclaredType::None,
                group
                    .arms
                    .iter()
                    .map(|arm| self.table(std::slice::from_ref(arm.member.as_ref()), origin))
                    .collect(),
            ),
            MemberDecl::AssociatedType(associated) => (
                Some(associated.name.as_str()),
                None,
                DeclaredType::None,
                Vec::new(),
            ),
            MemberDecl::Fn(function) => (
                Some(function.name.as_str()),
                None,
                DeclaredType::None,
                Vec::new(),
            ),
            MemberDecl::Port(port) => (
                Some(port.name.as_str()),
                port.where_clause.as_ref(),
                DeclaredType::Written(identifier(&port.type_name)),
                Vec::new(),
            ),
            MemberDecl::Sketch(sketch) => (
                Some(sketch.name.as_str()),
                None,
                DeclaredType::None,
                Vec::new(),
            ),
            MemberDecl::Connect(_)
            | MemberDecl::Chain(_)
            | MemberDecl::MetaBlock(_)
            | MemberDecl::ForallConnect(_)
            | MemberDecl::ForallConstraint(_)
            | MemberDecl::Relate(_) => (None, None, DeclaredType::None, Vec::new()),
        };
        Member {
            name: name.map(identifier),
            kind,
            declared_type,
            origin: origin.clone(),
            guarded: guard.is_some(),
            branches,
        }
    }

    /// The existing compile's resolution of the written type of the cell
    /// `name`; an unresolved type is reported and kept as [`Type::Error`].
    fn cell_type(&mut self, name: &str, span: SourceSpan) -> DeclaredType {
        match template_cell_type(self.template, name) {
            Some(ty) if *ty != Type::Error => DeclaredType::Value(ty.clone()),
            _ => {
                self.report(
                    DiagnosticCode::UnresolvedType,
                    format!("the type of `{name}` does not resolve"),
                    span,
                );
                DeclaredType::Value(Type::Error)
            }
        }
    }

    /// The definition the sub `name` instantiates, written `element`.
    fn sub_definition(&mut self, name: &str, element: &str, span: SourceSpan) -> DeclaredType {
        match self.module.definition_named(element) {
            Some(definition) => DeclaredType::Definition(definition),
            None => {
                self.report(
                    DiagnosticCode::UnknownSubStructure,
                    format!("the sub `{name}` names `{element}`, which is no definition"),
                    span,
                );
                DeclaredType::None
            }
        }
    }

    fn report(&mut self, code: DiagnosticCode, message: String, span: SourceSpan) {
        let definition = self.definition.name().as_str();
        self.diagnostics.push(
            Diagnostic::error(format!("in `{definition}`: {message}"))
                .with_code(code)
                .with_label(DiagnosticLabel::new(span, "this member")),
        );
    }
}

/// The type the existing compile gave the cell `name` of `template`, from its
/// value cells, its guarded groups' members or its match-arm groups.
fn template_cell_type<'t>(template: &'t TopologyTemplate, name: &str) -> Option<&'t Type> {
    let cells = template.value_cells.iter().chain(
        template
            .guarded_groups
            .iter()
            .flat_map(|g| g.member_decls()),
    );
    cells
        .filter(|cell| cell.id.member == name)
        .map(|cell| &cell.cell_type)
        .next()
        .or_else(|| {
            template
                .match_arm_groups
                .iter()
                .find(|group| group.name == name)
                .and_then(|group| group.arms.first())
                .map(|arm| &arm.arm_type)
        })
}

/// The element a keyed sub `name : Keyed<Element>` holds.
fn keyed_element(sub: &SubDecl) -> Option<&str> {
    match &sub.type_args.first()?.kind {
        TypeExprKind::Named { name, .. } => Some(name),
        _ => None,
    }
}

/// A name the parser accepted as an identifier.
fn identifier(text: &str) -> PathName {
    PathName::new(text)
        .unwrap_or_else(|e| panic!("the parser accepts `{text}` only as an identifier: {e:?}"))
}
