//! The syntactic half of import-header derivation: every name a parsed module
//! WRITES, tagged with the position it is written in, plus the names the
//! module itself binds. No resolution happens here; imports `reify_ast` only.
//!
//! Every match is wildcard-free and every struct pattern names all its fields,
//! so a new AST variant or field is a compile error here rather than a
//! silently unwalked position (the forcing function of
//! `reify-lsp/src/references/walk.rs`).

use std::collections::HashSet;

use reify_ast::{
    Annotation, AssociatedTypeDecl, ChainDecl, ConnectDecl, ConstraintDecl, ConstraintDef,
    ConstraintInstDecl, Declaration, DefaultDecl, EnumDecl, EnumVariantDecl, Expr, FieldDef,
    FieldSource, FnBody, FnDef, FnParam, ForallConnectBody, ForallConnectDecl,
    ForallConstraintBody, ForallConstraintDecl, GuardedGroupDecl, ImportDecl, JointDef,
    JointDofField, KeyedSubMemberEntry, LetDecl, MatchArmDeclArmDecl, MatchArmDeclGroupDecl,
    MaximizeDecl, MemberDecl, MetaBlockDecl, MinimizeDecl, ModuleDecl, OccurrenceDef, ParamDecl,
    ParsedModule, PortDecl, PortRef, PurposeDef, PurposeParam, RelateDecl, SketchDecl,
    StructureDef, SubDecl, SubDerivation, SubDerivationKind, SubDisposition, SubParamOverride,
    TraitBoundRef, TraitDecl, TypeAliasDecl, TypeExpr, TypeExprKind, TypeParamDecl, UnitDecl,
    VariantPayload, WhereClause,
};

/// The position a name is written in, which decides what kinds of definition
/// it can refer to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NameRole {
    /// A type annotation: `param b : Bracket`, `List<Bracket>`.
    Type,
    /// A trait position: a refinement, a trait bound, a type-param bound.
    Trait,
    /// The head of a call or instantiation: `twice(x)`, `sub s = Bracket()`.
    Callee,
    /// A bare identifier in expression position.
    Value,
    /// The enum of an `Enum.Variant` access.
    Enum,
    /// A variant name in a match pattern or a variant construction.
    Variant,
    /// The whole unit of a quantity literal: the `mm` of `2mm`.
    BareUnit,
    /// One factor of a compound unit: the `kg` and `m` of `1kg/m^3`.
    UnitFactor,
}

/// One written name and the position it is written in.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NameUse {
    pub name: String,
    pub role: NameRole,
}

/// Everything [`collect_name_uses`] finds in one module. The binder sets are
/// module-wide: a name bound anywhere counts as bound everywhere.
#[derive(Debug, Default)]
pub(crate) struct NameUses {
    pub(crate) uses: Vec<NameUse>,
    /// Names bound as values: params, lets, fn params, sub/port names, ….
    pub(crate) value_binders: HashSet<String>,
    /// Names bound as types: type params and associated types.
    pub(crate) type_binders: HashSet<String>,
}

pub(crate) fn collect_name_uses(parsed: &ParsedModule) -> NameUses {
    let mut collector = Collector::default();
    for declaration in &parsed.declarations {
        collector.declaration(declaration);
    }
    collector.out
}

#[derive(Default)]
struct Collector {
    out: NameUses,
}

impl Collector {
    fn use_name(&mut self, name: &str, role: NameRole) {
        self.out.uses.push(NameUse {
            name: name.to_string(),
            role,
        });
    }

    fn bind_value(&mut self, name: &str) {
        self.out.value_binders.insert(name.to_string());
    }

    fn bind_type(&mut self, name: &str) {
        self.out.type_binders.insert(name.to_string());
    }

    // ─── declarations ──────────────────────────────────────────────────────

    fn declaration(&mut self, declaration: &Declaration) {
        match declaration {
            Declaration::Structure(structure) => self.structure(structure),
            Declaration::Occurrence(occurrence) => self.occurrence(occurrence),
            Declaration::Import(import) => self.import(import),
            Declaration::Enum(enum_decl) => self.enum_decl(enum_decl),
            Declaration::Function(function) => self.fn_def(function),
            Declaration::Trait(trait_decl) => self.trait_decl(trait_decl),
            Declaration::Field(field) => self.field(field),
            Declaration::Purpose(purpose) => self.purpose(purpose),
            Declaration::Constraint(constraint) => self.constraint_def(constraint),
            Declaration::Unit(unit) => self.unit(unit),
            Declaration::TypeAlias(alias) => self.type_alias(alias),
            Declaration::Default(default) => self.default_decl(default),
            Declaration::Module(ModuleDecl {
                path: _,
                span: _,
                content_hash: _,
            }) => {}
            Declaration::Joint(joint) => self.joint(joint),
        }
    }

    fn structure(&mut self, structure: &StructureDef) {
        let StructureDef {
            name: _,
            doc: _,
            is_pub: _,
            type_params,
            trait_bounds,
            members,
            span: _,
            content_hash: _,
            // Pragma arguments are compile-time constants, never resolved names.
            pragmas: _,
            annotations,
        } = structure;
        self.type_params(type_params);
        self.trait_bounds(trait_bounds);
        self.members(members);
        self.annotations(annotations);
    }

    fn occurrence(&mut self, occurrence: &OccurrenceDef) {
        let OccurrenceDef {
            name: _,
            doc: _,
            is_pub: _,
            type_params,
            trait_bounds,
            members,
            span: _,
            content_hash: _,
            pragmas: _,
            annotations,
        } = occurrence;
        self.type_params(type_params);
        self.trait_bounds(trait_bounds);
        self.members(members);
        self.annotations(annotations);
    }

    fn import(&mut self, import: &ImportDecl) {
        let ImportDecl {
            path: _,
            kind: _,
            is_pub: _,
            span: _,
            content_hash: _,
            annotations,
            cfg_predicates: _,
        } = import;
        self.annotations(annotations);
    }

    fn enum_decl(&mut self, enum_decl: &EnumDecl) {
        let EnumDecl {
            name: _,
            doc: _,
            is_pub: _,
            type_params,
            variants,
            span: _,
            content_hash: _,
            annotations,
        } = enum_decl;
        self.type_params(type_params);
        for EnumVariantDecl {
            name: _,
            payload,
            span: _,
        } in variants
        {
            match payload {
                VariantPayload::Unit => {}
                VariantPayload::Named(fields) => {
                    for (_, field_type) in fields {
                        self.type_expr(field_type);
                    }
                }
            }
        }
        self.annotations(annotations);
    }

    fn trait_decl(&mut self, trait_decl: &TraitDecl) {
        let TraitDecl {
            name: _,
            doc: _,
            is_pub: _,
            type_params,
            refinements,
            members,
            span: _,
            content_hash: _,
            pragmas: _,
            annotations,
        } = trait_decl;
        self.type_params(type_params);
        for refinement in refinements {
            self.use_name(&refinement.name, NameRole::Trait);
        }
        self.members(members);
        self.annotations(annotations);
    }

    fn field(&mut self, field: &FieldDef) {
        let FieldDef {
            name,
            is_pub: _,
            domain_type,
            codomain_type,
            source,
            span: _,
            content_hash: _,
            annotations,
        } = field;
        self.bind_value(name);
        self.type_expr(domain_type);
        self.type_expr(codomain_type);
        match source {
            FieldSource::Analytical { expr } | FieldSource::Composed { expr } => {
                self.visit_expr(expr)
            }
            FieldSource::Sampled { config } => {
                for (_, value) in config {
                    self.visit_expr(value);
                }
            }
            // String-valued keys only: a file path, a format and a grid name.
            FieldSource::Imported {
                path: _,
                format: _,
                grid: _,
            } => {}
        }
        self.annotations(annotations);
    }

    fn purpose(&mut self, purpose: &PurposeDef) {
        let PurposeDef {
            name: _,
            is_pub: _,
            type_params,
            params,
            members,
            defaults,
            structures,
            span: _,
            content_hash: _,
            pragmas: _,
            annotations,
        } = purpose;
        self.type_params(type_params);
        for PurposeParam {
            name,
            entity_kind,
            span: _,
        } in params
        {
            self.bind_value(name);
            // `subject : Bracket` types the param as a reference to that entity.
            self.use_name(entity_kind, NameRole::Type);
        }
        self.members(members);
        for default in defaults {
            self.default_decl(default);
        }
        for structure in structures {
            self.structure(structure);
        }
        self.annotations(annotations);
    }

    fn constraint_def(&mut self, constraint: &ConstraintDef) {
        let ConstraintDef {
            name: _,
            is_pub: _,
            type_params,
            params,
            predicates,
            span: _,
            content_hash: _,
            pragmas: _,
            annotations,
        } = constraint;
        self.type_params(type_params);
        for param in params {
            self.param(param);
        }
        for predicate in predicates {
            self.visit_expr(predicate);
        }
        self.annotations(annotations);
    }

    fn unit(&mut self, unit: &UnitDecl) {
        let UnitDecl {
            name: _,
            is_pub: _,
            dimension_type,
            conversion,
            offset,
            span: _,
            content_hash: _,
            annotations,
        } = unit;
        self.type_expr(dimension_type);
        for expr in conversion.iter().chain(offset) {
            self.visit_expr(expr);
        }
        self.annotations(annotations);
    }

    fn type_alias(&mut self, alias: &TypeAliasDecl) {
        let TypeAliasDecl {
            name: _,
            doc: _,
            is_pub: _,
            type_params,
            type_expr,
            span: _,
            content_hash: _,
            annotations,
        } = alias;
        self.type_params(type_params);
        self.type_expr(type_expr);
        self.annotations(annotations);
    }

    fn default_decl(&mut self, default: &DefaultDecl) {
        let DefaultDecl {
            type_expr,
            value,
            span: _,
            content_hash: _,
        } = default;
        self.type_expr(type_expr);
        self.visit_expr(value);
    }

    fn joint(&mut self, joint: &JointDef) {
        let JointDef {
            name: _,
            doc: _,
            is_pub: _,
            type_params,
            params,
            dof,
            body,
            span: _,
            content_hash: _,
            annotations,
        } = joint;
        self.type_params(type_params);
        for param in params {
            self.fn_param(param);
        }
        for JointDofField {
            name,
            type_expr,
            range,
            span: _,
        } in dof
        {
            self.bind_value(name);
            self.type_expr(type_expr);
            if let Some(range) = range {
                self.visit_expr(range);
            }
        }
        for expr in body {
            self.visit_expr(expr);
        }
        self.annotations(annotations);
    }

    fn fn_def(&mut self, function: &FnDef) {
        let FnDef {
            name,
            doc: _,
            is_pub: _,
            type_params,
            params,
            return_type,
            body,
            span: _,
            content_hash: _,
            annotations,
        } = function;
        self.bind_value(name);
        self.type_params(type_params);
        for param in params {
            self.fn_param(param);
        }
        if let Some(return_type) = return_type {
            self.type_expr(return_type);
        }
        if let Some(FnBody {
            let_bindings,
            result_expr,
        }) = body
        {
            for binding in let_bindings {
                self.let_decl(binding);
            }
            self.visit_expr(result_expr);
        }
        self.annotations(annotations);
    }

    fn fn_param(&mut self, param: &FnParam) {
        let FnParam {
            name,
            is_self,
            type_expr,
            default,
            span: _,
        } = param;
        self.bind_value(name);
        // A receiver's type is a `self` sentinel, not a written type.
        if !is_self {
            self.type_expr(type_expr);
        }
        if let Some(default) = default {
            self.visit_expr(default);
        }
    }

    // ─── members ───────────────────────────────────────────────────────────

    fn members(&mut self, members: &[MemberDecl]) {
        for member in members {
            self.member(member);
        }
    }

    fn member(&mut self, member: &MemberDecl) {
        match member {
            MemberDecl::Param(param) => self.param(param),
            MemberDecl::Let(binding) => self.let_decl(binding),
            MemberDecl::Constraint(constraint) => self.constraint_decl(constraint),
            MemberDecl::ConstraintInst(instance) => self.constraint_inst(instance),
            MemberDecl::Sub(sub) => self.sub(sub),
            MemberDecl::Minimize(MinimizeDecl {
                expr,
                where_clause,
                span: _,
                content_hash: _,
            })
            | MemberDecl::Maximize(MaximizeDecl {
                expr,
                where_clause,
                span: _,
                content_hash: _,
            }) => {
                self.visit_expr(expr);
                self.where_clause(where_clause);
            }
            MemberDecl::GuardedGroup(GuardedGroupDecl {
                condition,
                members,
                else_members,
                span: _,
                content_hash: _,
            }) => {
                self.visit_expr(condition);
                self.members(members);
                self.members(else_members);
            }
            MemberDecl::AssociatedType(AssociatedTypeDecl {
                name,
                default_type,
                span: _,
                content_hash: _,
            }) => {
                self.bind_type(name);
                if let Some(default_type) = default_type {
                    self.type_expr(default_type);
                }
            }
            MemberDecl::Fn(function) => self.fn_def(function),
            MemberDecl::Port(port) => self.port(port),
            MemberDecl::Connect(connect) => self.connect(connect),
            MemberDecl::Chain(chain) => self.chain(chain),
            // Free-form `key = "string"` metadata.
            MemberDecl::MetaBlock(MetaBlockDecl {
                entries: _,
                span: _,
                content_hash: _,
            }) => {}
            MemberDecl::ForallConnect(ForallConnectDecl {
                variable,
                collection,
                body,
                span: _,
                content_hash: _,
            }) => {
                self.bind_value(variable);
                self.visit_expr(collection);
                match body {
                    ForallConnectBody::Connect(connect) => self.connect(connect),
                    ForallConnectBody::Chain(chain) => self.chain(chain),
                }
            }
            MemberDecl::ForallConstraint(ForallConstraintDecl {
                variable,
                collection,
                body,
                span: _,
                content_hash: _,
            }) => {
                self.bind_value(variable);
                self.visit_expr(collection);
                match body {
                    ForallConstraintBody::Constraint(constraint) => {
                        self.constraint_decl(constraint)
                    }
                    ForallConstraintBody::Instantiation(instance) => self.constraint_inst(instance),
                }
            }
            MemberDecl::MatchArmDeclGroup(group) => self.match_arm_decl_group(group),
            MemberDecl::Relate(RelateDecl {
                relations,
                span: _,
                content_hash: _,
            }) => {
                for relation in relations {
                    self.visit_expr(relation);
                }
            }
            MemberDecl::Sketch(SketchDecl {
                name,
                members,
                span: _,
                content_hash: _,
            }) => {
                self.bind_value(name);
                self.members(members);
            }
        }
    }

    fn param(&mut self, param: &ParamDecl) {
        let ParamDecl {
            name,
            doc: _,
            is_priv: _,
            type_expr,
            default,
            where_clause,
            annotations,
            span: _,
            content_hash: _,
        } = param;
        self.bind_value(name);
        if let Some(type_expr) = type_expr {
            self.type_expr(type_expr);
        }
        if let Some(default) = default {
            self.visit_expr(default);
        }
        self.where_clause(where_clause);
        self.annotations(annotations);
    }

    fn let_decl(&mut self, binding: &LetDecl) {
        let LetDecl {
            name,
            doc: _,
            is_pub: _,
            is_priv: _,
            is_aux: _,
            type_expr,
            value,
            where_clause,
            annotations,
            span: _,
            content_hash: _,
        } = binding;
        self.bind_value(name);
        if let Some(type_expr) = type_expr {
            self.type_expr(type_expr);
        }
        self.visit_expr(value);
        self.where_clause(where_clause);
        self.annotations(annotations);
    }

    fn constraint_decl(&mut self, constraint: &ConstraintDecl) {
        let ConstraintDecl {
            is_priv: _,
            label: _,
            expr,
            where_clause,
            span: _,
            content_hash: _,
        } = constraint;
        self.visit_expr(expr);
        self.where_clause(where_clause);
    }

    fn constraint_inst(&mut self, instance: &ConstraintInstDecl) {
        let ConstraintInstDecl {
            name: _,
            args,
            where_clause,
            span: _,
            content_hash: _,
        } = instance;
        for (_, arg) in args {
            self.visit_expr(arg);
        }
        self.where_clause(where_clause);
    }

    fn sub(&mut self, sub: &SubDecl) {
        let SubDecl {
            name,
            structure_name: _,
            type_args,
            args,
            is_collection: _,
            where_clause,
            body,
            spec_param_overrides,
            keyed_members,
            is_aux: _,
            is_priv: _,
            pose_expr,
            index_binder,
            index_domain,
            relate_relations,
            derivation,
            span: _,
            content_hash: _,
        } = sub;
        self.bind_value(name);
        for type_arg in type_args {
            self.type_expr(type_arg);
        }
        for (_, arg) in args.iter().chain(spec_param_overrides) {
            self.visit_expr(arg);
        }
        self.where_clause(where_clause);
        if let Some(body) = body {
            self.members(body);
        }
        for KeyedSubMemberEntry {
            key: _,
            overrides,
            param_overrides,
            span: _,
        } in keyed_members
        {
            self.members(overrides);
            for (_, value) in param_overrides {
                self.visit_expr(value);
            }
        }
        if let Some(binder) = index_binder {
            self.bind_value(&binder.name);
        }
        for expr in pose_expr.iter().chain(index_domain).chain(relate_relations) {
            self.visit_expr(expr);
        }
        if let Some(derivation) = derivation {
            self.sub_derivation(derivation);
        }
    }

    fn sub_derivation(&mut self, derivation: &SubDerivation) {
        let SubDerivation {
            kind,
            // A sibling sub of the same body: bound by that sub's own name.
            prototype: _,
            param_overrides,
            param_resets: _,
            dispositions,
            members,
            span: _,
        } = derivation;
        match kind {
            SubDerivationKind::Mirror { plane } => self.visit_expr(plane),
            SubDerivationKind::Image { transform } => self.visit_expr(transform),
        }
        for SubParamOverride {
            name: _,
            value,
            guard,
        } in param_overrides
        {
            self.visit_expr(value);
            self.where_clause(guard);
        }
        for SubDisposition {
            kind: _,
            // Feature-path segments inside the prototype, never top-level names.
            path: _,
            using_plane,
            span: _,
        } in dispositions
        {
            if let Some(plane) = using_plane {
                self.visit_expr(plane);
            }
        }
        self.members(members);
    }

    fn port(&mut self, port: &PortDecl) {
        let PortDecl {
            name,
            direction: _,
            type_name,
            is_priv: _,
            members,
            frame_expr,
            span: _,
            content_hash: _,
        } = port;
        self.bind_value(name);
        // The compiler looks a port's type up in the trait registry only.
        self.use_name(type_name, NameRole::Trait);
        self.members(members);
        if let Some(frame) = frame_expr {
            self.visit_expr(frame);
        }
    }

    fn connect(&mut self, connect: &ConnectDecl) {
        let ConnectDecl {
            left: PortRef { expr: left },
            operator: _,
            right: PortRef { expr: right },
            connector_type: _,
            params,
            // Port-name pairs local to the two endpoints.
            port_mappings: _,
            span: _,
            content_hash: _,
        } = connect;
        self.visit_expr(left);
        self.visit_expr(right);
        for (_, value) in params {
            self.visit_expr(value);
        }
    }

    fn chain(&mut self, chain: &ChainDecl) {
        let ChainDecl {
            elements,
            span: _,
            content_hash: _,
        } = chain;
        for element in elements {
            self.visit_expr(element);
        }
    }

    fn match_arm_decl_group(&mut self, group: &MatchArmDeclGroupDecl) {
        let MatchArmDeclGroupDecl {
            discriminant,
            arms,
            span: _,
            content_hash: _,
        } = group;
        self.visit_expr(discriminant);
        for MatchArmDeclArmDecl {
            patterns: _,
            member,
            span: _,
        } in arms
        {
            self.member(member);
        }
    }

    fn where_clause(&mut self, where_clause: &Option<WhereClause>) {
        if let Some(WhereClause { condition, span: _ }) = where_clause {
            self.visit_expr(condition);
        }
    }

    fn annotations(&mut self, annotations: &[Annotation]) {
        for Annotation {
            name: _,
            args,
            span: _,
        } in annotations
        {
            for arg in args {
                self.visit_expr(arg);
            }
        }
    }

    // ─── types ─────────────────────────────────────────────────────────────

    fn type_params(&mut self, type_params: &[TypeParamDecl]) {
        for TypeParamDecl {
            name,
            bounds,
            default,
            span: _,
        } in type_params
        {
            self.bind_type(name);
            for bound in bounds {
                self.use_name(bound, NameRole::Trait);
            }
            if let Some(default) = default {
                self.type_expr(default);
            }
        }
    }

    fn trait_bounds(&mut self, trait_bounds: &[TraitBoundRef]) {
        for TraitBoundRef {
            name,
            type_args,
            span: _,
        } in trait_bounds
        {
            self.use_name(name, NameRole::Trait);
            for type_arg in type_args {
                self.type_expr(type_arg);
            }
        }
    }

    fn type_expr(&mut self, type_expr: &TypeExpr) {
        let TypeExpr { kind, span: _ } = type_expr;
        match kind {
            TypeExprKind::Named { name, type_args } => {
                self.use_name(name, NameRole::Type);
                for type_arg in type_args {
                    self.type_expr(type_arg);
                }
            }
            TypeExprKind::DimensionalOp { op: _, left, right } => {
                self.type_expr(left);
                self.type_expr(right);
            }
            TypeExprKind::IntegerLiteral(_) => {}
            TypeExprKind::Auto { free: _, bound } => self.use_name(bound, NameRole::Trait),
            TypeExprKind::QualifiedAssoc {
                base,
                trait_name,
                // An associated-type name inside `base`, never a top-level name.
                member: _,
            } => {
                self.type_expr(base);
                if let Some(trait_name) = trait_name {
                    self.use_name(trait_name, NameRole::Trait);
                }
            }
            TypeExprKind::Function {
                params,
                return_type,
            } => {
                for param in params {
                    self.type_expr(param);
                }
                self.type_expr(return_type);
            }
        }
    }

    // ─── expressions ───────────────────────────────────────────────────────

    /// Expression positions are not walked yet: the one accepted gap of the
    /// type-position walk, closed by stdlib-namespace ε step-4.
    fn visit_expr(&mut self, _expr: &Expr) {}
}
