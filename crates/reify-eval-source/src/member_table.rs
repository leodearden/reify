//! A definition's member table: every member it declares or inherits, in
//! order, each with its kind and declared type.

use reify_ast::{Expr, MemberDecl, SubDecl, TraitBoundRef, TypeExprKind};
use reify_compiler::{
    CompilationScope, CompiledTrait, DefaultKind, RequirementKind, SourceUnitNames, SourceUnitSpec,
    TopologyTemplate, compile_source_unit,
};
use reify_core::{
    DefinitionName, Diagnostic, DiagnosticCode, DiagnosticLabel, PathName, SourceSpan, Type,
};

use crate::definition_scope::DefinitionNames;
use crate::inherited_members::inherited_members;
use crate::member_kind::{self, MemberKind};
use crate::module_context::{ModuleContext, ModuleSet, identifier};

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

/// A definition as its table is lowered from it: its qualified name, the
/// traits it is written to conform to, and its own body.
pub(crate) struct DefinitionBody<'a> {
    pub(crate) name: &'a DefinitionName,
    pub(crate) bounds: &'a [TraitBoundRef],
    pub(crate) members: &'a [MemberDecl],
}

/// The table of `definition`, declared in `module`, whose existing-compile
/// template is `template`: the members it inherits from the traits it
/// conforms to, then its own.
pub(crate) fn lower_definition<'p, 'm>(
    definition: &DefinitionBody<'_>,
    template: &TopologyTemplate,
    module: &'p ModuleContext<'m>,
    modules: &ModuleSet<'p, 'm>,
    diagnostics: &mut Vec<Diagnostic>,
) -> MemberTable {
    let inherited = inherited_members(
        definition.name,
        definition.bounds,
        module,
        modules,
        diagnostics,
    );
    let names = DefinitionNames::new(template, module);
    let scope = names.scope();
    let mut lowering = Lowering {
        definition: definition.name,
        template,
        scope: &scope,
        module,
        diagnostics,
    };
    let mut members: Vec<Member> = inherited
        .iter()
        .map(|member| {
            let written = WrittenIn {
                origin: MemberOrigin::Inherited(member.from.clone()),
                names: member.declared_in,
                trait_declaration: Some(member.declared_by),
            };
            lowering.member(member.decl, &written)
        })
        .collect();
    let own = WrittenIn {
        origin: MemberOrigin::Own,
        names: module,
        trait_declaration: None,
    };
    members.extend(lowering.table(definition.members, &own).members);
    MemberTable { members }
}

/// Where the members being lowered are written.
struct WrittenIn<'l, 'm> {
    origin: MemberOrigin,
    /// The module whose bindings the members' written names resolve in.
    names: &'l ModuleContext<'m>,
    /// The trait whose body declares the members; `None` for the
    /// definition's own.
    trait_declaration: Option<&'m CompiledTrait>,
}

/// What one member's form writes: everything of its table entry but its kind
/// and origin.
struct WrittenMember<'d> {
    /// The member's own name; `None` for a form that names nothing.
    name: Option<&'d str>,
    guarded: bool,
    declared_type: DeclaredType,
    branches: Vec<MemberTable>,
}

impl<'d> WrittenMember<'d> {
    fn named(name: &'d str, guarded: bool, declared_type: DeclaredType) -> Self {
        Self {
            name: Some(name),
            guarded,
            declared_type,
            branches: Vec::new(),
        }
    }

    fn unnamed(guarded: bool, declared_type: DeclaredType) -> Self {
        Self {
            name: None,
            guarded,
            declared_type,
            branches: Vec::new(),
        }
    }

    /// A guarded or match group, which holds its members in `branches`.
    fn group(branches: Vec<MemberTable>) -> Self {
        Self {
            name: None,
            guarded: false,
            declared_type: DeclaredType::None,
            branches,
        }
    }
}

/// What lowering one definition's members reads and reports into.
struct Lowering<'l, 'm> {
    definition: &'l DefinitionName,
    template: &'l TopologyTemplate,
    /// The scope the definition's expressions compile in.
    scope: &'l CompilationScope<'l>,
    /// The module declaring the definition, whose enums and functions its
    /// expressions compile against.
    module: &'l ModuleContext<'m>,
    diagnostics: &'l mut Vec<Diagnostic>,
}

impl Lowering<'_, '_> {
    fn table(&mut self, members: &[MemberDecl], written: &WrittenIn<'_, '_>) -> MemberTable {
        MemberTable {
            members: members
                .iter()
                .map(|decl| self.member(decl, written))
                .collect(),
        }
    }

    fn member(&mut self, decl: &MemberDecl, written: &WrittenIn<'_, '_>) -> Member {
        let form = self.written_member(decl, written);
        let table_type = match &form.declared_type {
            DeclaredType::Value(ty) => Some(ty),
            _ => None,
        };
        let existing_type = form
            .name
            .and_then(|name| template_cell_type(self.template, name));
        let kind = member_kind::kind(decl, table_type.into_iter().chain(existing_type));
        Member {
            name: form.name.map(identifier),
            kind,
            declared_type: form.declared_type,
            origin: written.origin.clone(),
            guarded: form.guarded,
            branches: form.branches,
        }
    }

    /// What the form `decl` writes, lowered.
    fn written_member<'d>(
        &mut self,
        decl: &'d MemberDecl,
        written: &WrittenIn<'_, '_>,
    ) -> WrittenMember<'d> {
        match decl {
            MemberDecl::Param(param) => WrittenMember::named(
                &param.name,
                param.where_clause.is_some(),
                self.cell_type(&param.name, param.span, written),
            ),
            MemberDecl::Let(binding) => {
                let declared_type = match binding.type_expr {
                    Some(_) => self.cell_type(&binding.name, binding.span, written),
                    None => DeclaredType::Value(self.expression_type(&binding.value)),
                };
                WrittenMember::named(&binding.name, binding.where_clause.is_some(), declared_type)
            }
            MemberDecl::Constraint(constraint) => {
                WrittenMember::unnamed(constraint.where_clause.is_some(), DeclaredType::None)
            }
            MemberDecl::ConstraintInst(applied) => WrittenMember::unnamed(
                applied.where_clause.is_some(),
                DeclaredType::Written(identifier(&applied.name)),
            ),
            MemberDecl::Sub(sub) => WrittenMember::named(
                &sub.name,
                sub.where_clause.is_some(),
                self.sub_definition(sub, written.names),
            ),
            MemberDecl::Minimize(objective) => {
                WrittenMember::unnamed(objective.where_clause.is_some(), DeclaredType::None)
            }
            MemberDecl::Maximize(objective) => {
                WrittenMember::unnamed(objective.where_clause.is_some(), DeclaredType::None)
            }
            MemberDecl::GuardedGroup(group) => WrittenMember::group(vec![
                self.table(&group.members, written),
                self.table(&group.else_members, written),
            ]),
            MemberDecl::MatchArmDeclGroup(group) => WrittenMember::group(
                group
                    .arms
                    .iter()
                    .map(|arm| self.table(std::slice::from_ref(arm.member.as_ref()), written))
                    .collect(),
            ),
            MemberDecl::AssociatedType(associated) => {
                WrittenMember::named(&associated.name, false, DeclaredType::None)
            }
            MemberDecl::Fn(function) => {
                WrittenMember::named(&function.name, false, DeclaredType::None)
            }
            MemberDecl::Port(port) => WrittenMember::named(
                &port.name,
                port.where_clause.is_some(),
                DeclaredType::Written(identifier(&port.type_name)),
            ),
            MemberDecl::Sketch(sketch) => {
                WrittenMember::named(&sketch.name, false, DeclaredType::None)
            }
            MemberDecl::Connect(_)
            | MemberDecl::Chain(_)
            | MemberDecl::MetaBlock(_)
            | MemberDecl::ForallConnect(_)
            | MemberDecl::ForallConstraint(_)
            | MemberDecl::Relate(_) => WrittenMember::unnamed(false, DeclaredType::None),
        }
    }

    /// The existing compile's resolution of the written type of the cell
    /// `name`: the definition's cell of that name, or for a member a trait
    /// declares and the definition has no cell of, the trait's. An
    /// unresolved type is reported and kept as [`Type::Error`].
    fn cell_type(
        &mut self,
        name: &str,
        span: SourceSpan,
        written: &WrittenIn<'_, '_>,
    ) -> DeclaredType {
        let resolved = match template_cell_type(self.template, name) {
            Some(ty) => Some(ty),
            None => written
                .trait_declaration
                .and_then(|declared| trait_cell_type(declared, name)),
        };
        match resolved {
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

    /// The type the source-unit mode gives `expr` on the definition's scope.
    /// The unit itself is not kept here; its diagnostics are.
    fn expression_type(&mut self, expr: &Expr) -> Type {
        let spec = SourceUnitSpec {
            names: SourceUnitNames::Own,
            expected_type: None,
        };
        compile_source_unit(
            expr,
            self.scope,
            &spec,
            self.module.enums(),
            self.module.functions(),
            self.diagnostics,
        )
        .expr
        .result_type
    }

    /// The definition `sub`, written in a body of the module `names`,
    /// instantiates: a keyed sub's element, else its written target.
    fn sub_definition(&mut self, sub: &SubDecl, names: &ModuleContext<'_>) -> DeclaredType {
        let element = if sub.keyed_members.is_empty() {
            sub.structure_name.as_str()
        } else {
            keyed_element(sub).unwrap_or(sub.structure_name.as_str())
        };
        match names.sub_target(element) {
            Some(target) => DeclaredType::Definition(target.name),
            None => {
                self.report(
                    DiagnosticCode::UnknownSubStructure,
                    format!(
                        "the sub `{}` names `{element}`, which is no definition",
                        sub.name
                    ),
                    sub.span,
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

/// The type the trait `declared` gives its `param` or typed `let` `name`,
/// whether it requires the member or provides a default.
fn trait_cell_type<'t>(declared: &'t CompiledTrait, name: &str) -> Option<&'t Type> {
    let required = declared
        .required_members
        .iter()
        .filter(|requirement| requirement.name == name)
        .find_map(|requirement| match &requirement.kind {
            RequirementKind::Param(ty) | RequirementKind::Let(ty) => Some(ty),
            RequirementKind::Sub(_) | RequirementKind::Fn(_) | RequirementKind::AssocType(_) => {
                None
            }
        });
    required.or_else(|| {
        declared
            .defaults
            .iter()
            .filter(|default| default.name.as_deref() == Some(name))
            .find_map(|default| match &default.kind {
                DefaultKind::Param { cell_type, .. } => Some(cell_type),
                DefaultKind::Let { cell_type, .. } => cell_type.as_ref(),
                DefaultKind::Constraint(_) | DefaultKind::Fn(_) | DefaultKind::AssocType(_) => None,
            })
    })
}

/// The element a keyed sub `name : Keyed<Element>` holds.
fn keyed_element(sub: &SubDecl) -> Option<&str> {
    match &sub.type_args.first()?.kind {
        TypeExprKind::Named { name, .. } => Some(name),
        _ => None,
    }
}
