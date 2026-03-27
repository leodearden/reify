pub mod expr;
pub mod constraint;
pub mod topology;
pub use expr::*;
pub use constraint::*;
pub use topology::*;

use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, ContentHash, DimensionVector, SourceSpan, Type, Value,
    ValueCellId,
};

use reify_compiler::{
    CompiledConstraint, CompiledField, CompiledFieldSource, CompiledImport, CompiledModule,
    CompiledPurpose, CompiledPurposeParam, CompiledTrait, DefaultKind, RequirementKind,
    ResolvedSchemaQuery, TopologyTemplate, TraitDefault, TraitRequirement, ValueCellKind,
};
use reify_types::{ConstraintNodeId, TypeParam};

#[cfg(test)]
mod reexport_contract_tests {
    //! Guard tests: verify that core builder functions remain accessible
    //! via the `crate::builders` module path after submodule extraction.
    use crate::builders::{
        binop, conditional_expr, eq, equality_constraint, fn_call, ge, gt, lambda_expr, le,
        list_expr, literal, lt, map_expr, method_call_expr, ne, neg, not, range_constraint,
        sample_call, set_expr, user_fn_call, value_ref, value_ref_typed,
    };
    use reify_types::{BinOp, Type, Value};

    #[test]
    fn expr_builders_accessible_via_module_path() {
        // Compilation-only: if this compiles, the re-export contract holds.
        let _ = literal(Value::Int(1));
        let _ = value_ref("E", "m");
        let _ = value_ref_typed("E", "m", Type::Real);
        let a = literal(Value::Int(1));
        let b = literal(Value::Int(2));
        let _ = binop(BinOp::Add, a.clone(), b.clone());
        let _ = gt(a.clone(), b.clone());
        let _ = lt(a.clone(), b.clone());
        let _ = ge(a.clone(), b.clone());
        let _ = le(a.clone(), b.clone());
        let _ = eq(a.clone(), b.clone());
        let _ = ne(a.clone(), b.clone());
        let _ = not(literal(Value::Bool(true)));
        let _ = neg(literal(Value::Int(1)));
        let _ = list_expr(vec![literal(Value::Int(1))]);
        let _ = set_expr(vec![literal(Value::Int(1))]);
        let _ = map_expr(vec![(literal(Value::String("k".into())), literal(Value::Int(1)))]);
        let _ = conditional_expr(literal(Value::Bool(true)), literal(Value::Int(1)), literal(Value::Int(2)));
        let _ = fn_call("f", "q::f", vec![], Type::Real);
        let _ = user_fn_call("f", vec![], Type::Real);
        let _ = method_call_expr(literal(Value::Int(1)), "m", vec![], Type::Int);
        let _ = sample_call(literal(Value::Real(0.0)), literal(Value::Real(1.0)), Type::Real);
        let _ = lambda_expr(vec![("x", Type::Real)], literal(Value::Real(1.0)));
    }

    #[test]
    fn constraint_builders_accessible_via_module_path() {
        let exprs = range_constraint(
            "E",
            "m",
            Type::Real,
            literal(Value::Real(0.0)),
            literal(Value::Real(1.0)),
        );
        assert_eq!(exprs.len(), 2);
        assert_eq!(exprs[0].result_type, Type::Bool);
        assert_eq!(exprs[1].result_type, Type::Bool);

        let eq_exprs = equality_constraint("E", "m", Type::Real, literal(Value::Real(1.0)));
        assert_eq!(eq_exprs.len(), 1);
        assert_eq!(eq_exprs[0].result_type, Type::Bool);
    }
}

// Tests remaining in mod.rs pending extraction to their target submodules:
#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::Value;

    // step-1: failing test for TraitDefBuilder minimal
    #[test]
    fn trait_def_builder_minimal() {
        let ct = TraitDefBuilder::new("Rigid").build();
        assert_eq!(ct.name, "Rigid");
        assert!(!ct.is_pub);
        assert!(ct.required_members.is_empty());
        assert!(ct.defaults.is_empty());
        assert!(ct.refinements.is_empty());
        assert!(ct.type_params.is_empty());
        // content_hash should be non-zero (derived from name)
        assert_ne!(ct.content_hash, reify_types::ContentHash(0));
    }

    // step-3: failing tests for TraitDefBuilder members
    #[test]
    fn trait_def_builder_with_requirement() {
        let ct = TraitDefBuilder::new("Rigid")
            .requirement(
                "mass",
                RequirementKind::Param(Type::Scalar {
                    dimension: DimensionVector::LENGTH, // reuse LENGTH for test simplicity
                }),
            )
            .build();
        assert_eq!(ct.required_members.len(), 1);
        assert_eq!(ct.required_members[0].name, "mass");
        assert!(matches!(
            &ct.required_members[0].kind,
            RequirementKind::Param(_)
        ));
    }

    #[test]
    fn trait_def_builder_with_refinement() {
        let ct = TraitDefBuilder::new("StronglyRigid")
            .refinement("Rigid")
            .build();
        assert_eq!(ct.refinements.len(), 1);
        assert_eq!(ct.refinements[0], "Rigid");
    }

    #[test]
    fn trait_def_builder_with_type_param() {
        use reify_types::{TraitBound, TraitRef};
        let param = TypeParam {
            name: "T".to_string(),
            bounds: vec![TraitBound {
                trait_ref: TraitRef {
                    name: "Rigid".to_string(),
                    type_args: vec![],
                },
            }],
            default: None,
        };
        let ct = TraitDefBuilder::new("Container").type_param(param).build();
        assert_eq!(ct.type_params.len(), 1);
        assert_eq!(ct.type_params[0].name, "T");
        assert_eq!(ct.type_params[0].bounds.len(), 1);
        assert_eq!(ct.type_params[0].bounds[0].trait_ref.name, "Rigid");
    }

    #[test]
    fn trait_def_builder_is_pub() {
        let ct = TraitDefBuilder::new("Rigid").is_pub().build();
        assert!(ct.is_pub);
    }

    #[test]
    fn trait_def_builder_content_hash_differs_by_name() {
        let ct1 = TraitDefBuilder::new("Rigid").build();
        let ct2 = TraitDefBuilder::new("Flexible").build();
        assert_ne!(ct1.content_hash, ct2.content_hash);
    }

    #[test]
    fn trait_def_builder_with_default() {
        let ct = TraitDefBuilder::new("Rigid")
            .add_default(
                Some("mass_positive"),
                DefaultKind::Constraint(reify_syntax::ConstraintDecl {
                    label: Some("mass_positive".to_string()),
                    expr: reify_syntax::Expr {
                        kind: reify_syntax::ExprKind::BoolLiteral(true),
                        span: SourceSpan::new(0, 0),
                    },
                    where_clause: None,
                    span: SourceSpan::new(0, 0),
                    content_hash: ContentHash::of_str("true"),
                }),
            )
            .build();
        assert_eq!(ct.defaults.len(), 1);
        assert_eq!(ct.defaults[0].name.as_deref(), Some("mass_positive"));
    }

    #[test]
    fn trait_def_content_hash_differs_by_type_param() {
        use reify_types::{TraitBound, TraitRef};
        let ct1 = TraitDefBuilder::new("Container").build();
        let ct2 = TraitDefBuilder::new("Container")
            .type_param(TypeParam {
                name: "T".to_string(),
                bounds: vec![TraitBound {
                    trait_ref: TraitRef {
                        name: "Rigid".to_string(),
                        type_args: vec![],
                    },
                }],
                default: None,
            })
            .build();
        assert_ne!(
            ct1.content_hash, ct2.content_hash,
            "traits differing only in type_params must produce distinct content_hashes"
        );
    }

    #[test]
    fn trait_def_content_hash_differs_by_default() {
        let ct1 = TraitDefBuilder::new("Rigid").build();
        let ct2 = TraitDefBuilder::new("Rigid")
            .add_default(
                Some("mass_positive"),
                DefaultKind::Constraint(reify_syntax::ConstraintDecl {
                    label: Some("mass_positive".to_string()),
                    expr: reify_syntax::Expr {
                        kind: reify_syntax::ExprKind::BoolLiteral(true),
                        span: SourceSpan::new(0, 0),
                    },
                    where_clause: None,
                    span: SourceSpan::new(0, 0),
                    content_hash: ContentHash::of_str("true"),
                }),
            )
            .build();
        assert_ne!(
            ct1.content_hash, ct2.content_hash,
            "traits differing only in defaults must produce distinct content_hashes"
        );
    }

    // --- CompiledFieldBuilder tests (step-13) ---

    #[test]
    fn compiled_field_builder_analytical_produces_field() {
        use reify_compiler::CompiledFieldSource;
        let body = literal(Value::Real(1.0));
        let field = CompiledFieldBuilder::new("temp", Type::Geometry, Type::Real)
            .analytical(body)
            .build();
        assert_eq!(field.name, "temp");
        assert!(!field.is_pub);
        assert_eq!(field.domain_type, Type::Geometry);
        assert_eq!(field.codomain_type, Type::Real);
        assert!(matches!(
            field.source,
            CompiledFieldSource::Analytical { .. }
        ));
        assert_ne!(field.content_hash, ContentHash(0));
    }

    #[test]
    fn compiled_field_builder_public_sampled() {
        use reify_compiler::CompiledFieldSource;
        let field = CompiledFieldBuilder::new("vel", Type::Geometry, Type::Real)
            .public()
            .sampled(vec![("resolution", literal(Value::Int(32)))])
            .build();
        assert!(field.is_pub);
        assert!(matches!(field.source, CompiledFieldSource::Sampled { .. }));
        assert_ne!(field.content_hash, ContentHash(0));
    }

    #[test]
    fn compiled_field_builder_composed() {
        use reify_compiler::CompiledFieldSource;
        let body = literal(Value::Real(0.0));
        let field = CompiledFieldBuilder::new("composed_f", Type::Geometry, Type::Real)
            .composed(body)
            .build();
        assert!(matches!(field.source, CompiledFieldSource::Composed { .. }));
    }

    #[test]
    fn compiled_field_builder_imported() {
        use reify_compiler::CompiledFieldSource;
        let field = CompiledFieldBuilder::new("ext", Type::Geometry, Type::Real)
            .imported()
            .build();
        assert!(matches!(field.source, CompiledFieldSource::Imported));
        assert_ne!(field.content_hash, ContentHash(0));
    }
}

/// Builder for `CompiledTrait`.
///
/// Follows the same fluent pattern as `TopologyTemplateBuilder`.
pub struct TraitDefBuilder {
    name: String,
    is_pub: bool,
    type_params: Vec<TypeParam>,
    refinements: Vec<String>,
    required_members: Vec<TraitRequirement>,
    defaults: Vec<TraitDefault>,
}

impl TraitDefBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_pub: false,
            type_params: Vec::new(),
            refinements: Vec::new(),
            required_members: Vec::new(),
            defaults: Vec::new(),
        }
    }

    pub fn is_pub(mut self) -> Self {
        self.is_pub = true;
        self
    }

    pub fn refinement(mut self, trait_name: impl Into<String>) -> Self {
        self.refinements.push(trait_name.into());
        self
    }

    pub fn type_param(mut self, param: TypeParam) -> Self {
        self.type_params.push(param);
        self
    }

    pub fn requirement(mut self, name: impl Into<String>, kind: RequirementKind) -> Self {
        self.required_members.push(TraitRequirement {
            name: name.into(),
            kind,
            span: reify_types::SourceSpan::new(0, 0),
        });
        self
    }

    pub fn add_default(mut self, name: Option<impl Into<String>>, kind: DefaultKind) -> Self {
        self.defaults.push(TraitDefault {
            name: name.map(|n| n.into()),
            kind,
            span: reify_types::SourceSpan::new(0, 0),
        });
        self
    }

    pub fn build(self) -> CompiledTrait {
        let content_hash = {
            let name_hash = ContentHash::of_str(&self.name);
            let req_hashes = self.required_members.iter().map(|r| {
                ContentHash::of_str(&format!("{}:{:?}", r.name, std::mem::discriminant(&r.kind)))
            });
            let ref_hashes = self.refinements.iter().map(|r| ContentHash::of_str(r));
            let type_param_hashes = self
                .type_params
                .iter()
                .map(|p| ContentHash::of_str(&p.name));
            let default_hashes = self
                .defaults
                .iter()
                .map(|d| ContentHash::of_str(d.name.as_deref().unwrap_or("")));
            let all_hashes = std::iter::once(name_hash)
                .chain(req_hashes)
                .chain(ref_hashes)
                .chain(type_param_hashes)
                .chain(default_hashes);
            ContentHash::combine_all(all_hashes)
        };

        CompiledTrait {
            name: self.name,
            is_pub: self.is_pub,
            type_params: self.type_params,
            refinements: self.refinements,
            required_members: self.required_members,
            defaults: self.defaults,
            content_hash,
        }
    }
}

/// Builder for `CompiledModule`.
pub struct CompiledModuleBuilder {
    path: reify_types::ModulePath,
    imports: Vec<CompiledImport>,
    functions: Vec<reify_types::CompiledFunction>,
    trait_defs: Vec<CompiledTrait>,
    templates: Vec<TopologyTemplate>,
    diagnostics: Vec<reify_types::Diagnostic>,
    fields: Vec<CompiledField>,
    enum_defs: Vec<reify_types::EnumDef>,
    compiled_purposes: Vec<CompiledPurpose>,
}

impl CompiledModuleBuilder {
    pub fn new(path: reify_types::ModulePath) -> Self {
        Self {
            path,
            imports: Vec::new(),
            functions: Vec::new(),
            trait_defs: Vec::new(),
            templates: Vec::new(),
            diagnostics: Vec::new(),
            fields: Vec::new(),
            enum_defs: Vec::new(),
            compiled_purposes: Vec::new(),
        }
    }

    pub fn trait_def(mut self, t: CompiledTrait) -> Self {
        self.trait_defs.push(t);
        self
    }

    pub fn function(mut self, f: reify_types::CompiledFunction) -> Self {
        self.functions.push(f);
        self
    }

    pub fn import(mut self, path: impl Into<String>) -> Self {
        self.imports.push(CompiledImport {
            path: path.into(),
            kind: reify_syntax::ImportKind::Module,
            is_pub: false,
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn import_with(
        mut self,
        path: impl Into<String>,
        kind: reify_syntax::ImportKind,
        is_pub: bool,
    ) -> Self {
        self.imports.push(CompiledImport {
            path: path.into(),
            kind,
            is_pub,
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn template(mut self, template: TopologyTemplate) -> Self {
        self.templates.push(template);
        self
    }

    pub fn diagnostic(mut self, diag: reify_types::Diagnostic) -> Self {
        self.diagnostics.push(diag);
        self
    }

    pub fn field(mut self, f: CompiledField) -> Self {
        self.fields.push(f);
        self
    }

    pub fn enum_def(mut self, e: reify_types::EnumDef) -> Self {
        self.enum_defs.push(e);
        self
    }

    pub fn compiled_purpose(mut self, p: CompiledPurpose) -> Self {
        self.compiled_purposes.push(p);
        self
    }

    pub fn build(self) -> CompiledModule {
        // Build a content-sensitive hash matching compile() logic.
        let content_hash = {
            let path_hash = ContentHash::of_str(&format!("{}", self.path));

            let template_hashes = self.templates.iter().map(|t| t.content_hash);

            let import_hashes = self.imports.iter().map(|i| ContentHash::of_str(&i.path));

            let function_hashes = self.functions.iter().map(|f| f.content_hash);

            let trait_def_hashes = self.trait_defs.iter().map(|t| t.content_hash);

            let field_hashes = self.fields.iter().map(|f| f.content_hash);

            let purpose_hashes = self.compiled_purposes.iter().map(|p| p.content_hash);

            let enum_hashes = self.enum_defs.iter().map(|e| ContentHash::of_str(&e.name));

            let all_hashes = std::iter::once(path_hash)
                .chain(template_hashes)
                .chain(import_hashes)
                .chain(function_hashes)
                .chain(trait_def_hashes)
                .chain(field_hashes)
                .chain(purpose_hashes)
                .chain(enum_hashes);

            ContentHash::combine_all(all_hashes)
        };

        CompiledModule {
            path: self.path,
            imports: self.imports,
            enum_defs: self.enum_defs,
            functions: self.functions,
            trait_defs: self.trait_defs,
            fields: self.fields,
            compiled_purposes: self.compiled_purposes,
            templates: self.templates,
            diagnostics: self.diagnostics,
            content_hash,
        }
    }
}

// --- CompiledFieldBuilder ---

/// Builder for `CompiledField`.
pub struct CompiledFieldBuilder {
    name: String,
    is_pub: bool,
    domain_type: Type,
    codomain_type: Type,
    source: Option<CompiledFieldSource>,
}

impl CompiledFieldBuilder {
    pub fn new(name: impl Into<String>, domain_type: Type, codomain_type: Type) -> Self {
        Self {
            name: name.into(),
            is_pub: false,
            domain_type,
            codomain_type,
            source: None,
        }
    }

    pub fn public(mut self) -> Self {
        self.is_pub = true;
        self
    }

    /// Set source to `Analytical { expr }`.
    pub fn analytical(mut self, expr: CompiledExpr) -> Self {
        self.source = Some(CompiledFieldSource::Analytical { expr });
        self
    }

    /// Set source to `Sampled { config }`.
    pub fn sampled(mut self, config: Vec<(&str, CompiledExpr)>) -> Self {
        let config = config
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        self.source = Some(CompiledFieldSource::Sampled { config });
        self
    }

    /// Set source to `Composed { expr }`.
    pub fn composed(mut self, expr: CompiledExpr) -> Self {
        self.source = Some(CompiledFieldSource::Composed { expr });
        self
    }

    /// Set source to `Imported`.
    pub fn imported(mut self) -> Self {
        self.source = Some(CompiledFieldSource::Imported);
        self
    }

    pub fn build(self) -> CompiledField {
        let source = self
            .source
            .expect("CompiledFieldBuilder: source must be set before build()");
        let content_hash = ContentHash::of_str(&self.name).combine(ContentHash::of(&[99])); // distinguish from zero
        CompiledField {
            name: self.name,
            is_pub: self.is_pub,
            domain_type: self.domain_type,
            codomain_type: self.codomain_type,
            source,
            content_hash,
        }
    }
}

// --- CompiledPurposeBuilder (step-16) ---

/// Builder for `CompiledPurpose`.
pub struct CompiledPurposeBuilder {
    name: String,
    is_pub: bool,
    params: Vec<CompiledPurposeParam>,
    constraints: Vec<CompiledConstraint>,
    objective: Option<reify_types::OptimizationObjective>,
    resolved_queries: Vec<ResolvedSchemaQuery>,
}

impl CompiledPurposeBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_pub: false,
            params: Vec::new(),
            constraints: Vec::new(),
            objective: None,
            resolved_queries: Vec::new(),
        }
    }

    pub fn public(mut self) -> Self {
        self.is_pub = true;
        self
    }

    pub fn param(mut self, name: impl Into<String>, entity_kind: impl Into<String>) -> Self {
        self.params.push(CompiledPurposeParam {
            name: name.into(),
            entity_kind: entity_kind.into(),
        });
        self
    }

    pub fn constraint(
        mut self,
        entity: &str,
        index: u32,
        label: Option<&str>,
        expr: CompiledExpr,
    ) -> Self {
        self.constraints.push(CompiledConstraint {
            id: ConstraintNodeId::new(entity, index),
            label: label.map(String::from),
            expr,
            span: SourceSpan::new(0, 0),
            domain: None,
        });
        self
    }

    pub fn objective(mut self, obj: reify_types::OptimizationObjective) -> Self {
        self.objective = Some(obj);
        self
    }

    pub fn schema_query(
        mut self,
        param_name: impl Into<String>,
        query_kind: impl Into<String>,
        resolved_ids: Vec<ValueCellId>,
    ) -> Self {
        self.resolved_queries.push(ResolvedSchemaQuery {
            param_name: param_name.into(),
            query_kind: query_kind.into(),
            resolved_ids,
        });
        self
    }

    pub fn build(self) -> CompiledPurpose {
        let name_hash = ContentHash::of_str(&self.name);
        let constraint_hashes = self.constraints.iter().map(|c| c.expr.content_hash);
        let query_hashes = self
            .resolved_queries
            .iter()
            .map(|q| ContentHash::of_str(&format!("{}.{}", q.param_name, q.query_kind)));
        let content_hash = std::iter::once(name_hash)
            .chain(constraint_hashes)
            .chain(query_hashes)
            .fold(ContentHash::of(&[0x50]), |acc, h| acc.combine(h));

        CompiledPurpose {
            name: self.name,
            is_pub: self.is_pub,
            params: self.params,
            constraints: self.constraints,
            objective: self.objective,
            resolved_queries: self.resolved_queries,
            content_hash,
        }
    }
}

// --- CompiledTraitBuilder (step-18) ---

/// Builder for `CompiledTrait`.
pub struct CompiledTraitBuilder {
    name: String,
    is_pub: bool,
    type_params: Vec<reify_types::TypeParam>,
    refinements: Vec<String>,
    required_members: Vec<TraitRequirement>,
    defaults: Vec<reify_compiler::TraitDefault>,
}

impl CompiledTraitBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_pub: false,
            type_params: Vec::new(),
            refinements: Vec::new(),
            required_members: Vec::new(),
            defaults: Vec::new(),
        }
    }

    pub fn public(mut self) -> Self {
        self.is_pub = true;
        self
    }

    pub fn refinement(mut self, name: impl Into<String>) -> Self {
        self.refinements.push(name.into());
        self
    }

    pub fn require_param(mut self, name: impl Into<String>, ty: Type) -> Self {
        self.required_members.push(TraitRequirement {
            name: name.into(),
            kind: RequirementKind::Param(ty),
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn require_let(mut self, name: impl Into<String>, ty: Type) -> Self {
        self.required_members.push(TraitRequirement {
            name: name.into(),
            kind: RequirementKind::Let(ty),
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn require_sub(mut self, name: impl Into<String>, structure: impl Into<String>) -> Self {
        self.required_members.push(TraitRequirement {
            name: name.into(),
            kind: RequirementKind::Sub(structure.into()),
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn build(self) -> CompiledTrait {
        let name_hash = ContentHash::of_str(&self.name);
        let member_hashes = self
            .required_members
            .iter()
            .map(|m| ContentHash::of_str(&m.name));
        let content_hash = std::iter::once(name_hash)
            .chain(member_hashes)
            .fold(ContentHash::of(&[0x54]), |acc, h| acc.combine(h));

        CompiledTrait {
            name: self.name,
            is_pub: self.is_pub,
            type_params: self.type_params,
            refinements: self.refinements,
            required_members: self.required_members,
            defaults: self.defaults,
            content_hash,
        }
    }
}

// --- Tests for CompiledPurposeBuilder (step-15) ---

#[cfg(test)]
mod purpose_builder_tests {
    use super::*;
    use reify_types::OptimizationObjective;

    #[test]
    fn purpose_builder_basic_param_and_constraint() {
        use reify_compiler::CompiledPurpose;
        let constraint_expr = literal(Value::Bool(true));
        let purpose: CompiledPurpose = CompiledPurposeBuilder::new("mfg_ready")
            .param("subject", "Structure")
            .constraint("subject", 0, Some("thick_enough"), constraint_expr)
            .build();
        assert_eq!(purpose.name, "mfg_ready");
        assert!(!purpose.is_pub);
        assert_eq!(purpose.params.len(), 1);
        assert_eq!(purpose.params[0].name, "subject");
        assert_eq!(purpose.params[0].entity_kind, "Structure");
        assert_eq!(purpose.constraints.len(), 1);
        assert_eq!(
            purpose.constraints[0].label.as_deref(),
            Some("thick_enough")
        );
        assert_ne!(purpose.content_hash, ContentHash(0));
    }

    #[test]
    fn purpose_builder_public() {
        use reify_compiler::CompiledPurpose;
        let purpose: CompiledPurpose = CompiledPurposeBuilder::new("opt_ready").public().build();
        assert!(purpose.is_pub);
    }

    #[test]
    fn purpose_builder_with_objective() {
        use reify_compiler::CompiledPurpose;
        let obj_expr = literal(Value::Real(1.0));
        let purpose: CompiledPurpose = CompiledPurposeBuilder::new("minimize_mass")
            .param("subject", "Structure")
            .objective(OptimizationObjective::Minimize(obj_expr))
            .build();
        assert!(purpose.objective.is_some());
        assert_eq!(purpose.resolved_queries.len(), 0);
        assert_ne!(purpose.content_hash, ContentHash(0));
    }

    #[test]
    fn purpose_builder_with_schema_query() {
        use reify_compiler::CompiledPurpose;
        let vcid = ValueCellId::new("subject", "thickness");
        let purpose: CompiledPurpose = CompiledPurposeBuilder::new("mfg_ready")
            .param("subject", "Structure")
            .schema_query("subject", "params", vec![vcid.clone()])
            .build();
        assert_eq!(purpose.resolved_queries.len(), 1);
        assert_eq!(purpose.resolved_queries[0].param_name, "subject");
        assert_eq!(purpose.resolved_queries[0].query_kind, "params");
        assert_eq!(purpose.resolved_queries[0].resolved_ids.len(), 1);
        assert_eq!(purpose.resolved_queries[0].resolved_ids[0], vcid);
    }
}

// --- Tests for CompiledTraitBuilder (step-17) ---

#[cfg(test)]
mod trait_builder_tests {
    use super::*;
    use reify_compiler::{CompiledTrait, RequirementKind};

    #[test]
    fn trait_builder_require_param_produces_required_member() {
        let t: CompiledTrait = CompiledTraitBuilder::new("Rigid")
            .require_param("thickness", Type::length())
            .build();
        assert_eq!(t.name, "Rigid");
        assert!(!t.is_pub);
        assert_eq!(t.required_members.len(), 1);
        assert_eq!(t.required_members[0].name, "thickness");
        if let RequirementKind::Param(ty) = &t.required_members[0].kind {
            assert_eq!(*ty, Type::length());
        } else {
            panic!("expected RequirementKind::Param");
        }
        assert_ne!(t.content_hash, ContentHash(0));
    }

    #[test]
    fn trait_builder_public() {
        let t: CompiledTrait = CompiledTraitBuilder::new("Rigid").public().build();
        assert!(t.is_pub);
    }

    #[test]
    fn trait_builder_refinement_and_multiple_requirements() {
        let t: CompiledTrait = CompiledTraitBuilder::new("RigidMount")
            .refinement("Rigid")
            .require_let("vol", Type::Real)
            .require_sub("mount", "MountPoint")
            .build();
        assert_eq!(t.refinements.len(), 1);
        assert_eq!(t.refinements[0], "Rigid");
        assert_eq!(t.required_members.len(), 2);
        assert!(matches!(
            &t.required_members[0].kind,
            RequirementKind::Let(_)
        ));
        assert!(
            matches!(&t.required_members[1].kind, RequirementKind::Sub(s) if s == "MountPoint")
        );
        assert_ne!(t.content_hash, ContentHash(0));
    }

    #[test]
    fn trait_builder_defaults_initially_empty() {
        let t: CompiledTrait = CompiledTraitBuilder::new("Bounded").build();
        assert_eq!(t.defaults.len(), 0);
        assert_eq!(t.type_params.len(), 0);
    }
}

// --- Tests for extended CompiledModuleBuilder (step-19) ---

#[cfg(test)]
mod module_builder_extension_tests {
    use super::*;
    use reify_types::{EnumDef, ModulePath};

    fn module_path() -> ModulePath {
        ModulePath::new(vec!["test".to_string()])
    }

    #[test]
    fn module_builder_with_trait_def() {
        let t = CompiledTraitBuilder::new("Rigid")
            .require_param("thickness", Type::length())
            .build();
        let module = CompiledModuleBuilder::new(module_path())
            .trait_def(t)
            .build();
        assert_eq!(module.trait_defs.len(), 1);
        assert_eq!(module.trait_defs[0].name, "Rigid");
    }

    // step-26: failing test — content_hash must differ when trait_defs differ
    #[test]
    fn module_builder_trait_defs_affect_content_hash() {
        let module_no_traits = CompiledModuleBuilder::new(module_path()).build();
        let ct = TraitDefBuilder::new("Rigid").build();
        let module_with_trait = CompiledModuleBuilder::new(module_path())
            .trait_def(ct)
            .build();
        assert_ne!(
            module_no_traits.content_hash, module_with_trait.content_hash,
            "modules differing only in trait_defs must produce distinct content_hashes"
        );
    }

    #[test]
    fn module_builder_with_field() {
        let body = literal(Value::Real(1.0));
        let f = CompiledFieldBuilder::new("temp", Type::Geometry, Type::Real)
            .analytical(body)
            .build();
        let module = CompiledModuleBuilder::new(module_path()).field(f).build();
        assert_eq!(module.fields.len(), 1);
        assert_eq!(module.fields[0].name, "temp");
    }

    #[test]
    fn module_builder_with_enum_def() {
        let e = EnumDef {
            name: "Color".to_string(),
            variants: vec!["Red".to_string(), "Blue".to_string()],
        };
        let module = CompiledModuleBuilder::new(module_path())
            .enum_def(e)
            .build();
        assert_eq!(module.enum_defs.len(), 1);
        assert_eq!(module.enum_defs[0].name, "Color");
        assert_eq!(module.enum_defs[0].variants.len(), 2);
    }

    #[test]
    fn module_builder_with_compiled_purpose() {
        let p = CompiledPurposeBuilder::new("mfg_ready")
            .param("subject", "Structure")
            .build();
        let module = CompiledModuleBuilder::new(module_path())
            .compiled_purpose(p)
            .build();
        assert_eq!(module.compiled_purposes.len(), 1);
        assert_eq!(module.compiled_purposes[0].name, "mfg_ready");
    }

    #[test]
    fn module_builder_hash_changes_with_new_fields() {
        let empty_module = CompiledModuleBuilder::new(module_path()).build();
        let t = CompiledTraitBuilder::new("Rigid").build();
        let with_trait = CompiledModuleBuilder::new(module_path())
            .trait_def(t)
            .build();
        assert_ne!(empty_module.content_hash, with_trait.content_hash);
    }
}
