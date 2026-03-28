use reify_types::{ContentHash, SourceSpan, Type, TypeParam};

use reify_compiler::{
    CompiledTrait, DefaultKind, RequirementKind, TraitDefault, TraitRequirement,
};

/// Returns a static tag string for a `DefaultKind` variant.
fn default_kind_tag(kind: &DefaultKind) -> &'static str {
    match kind {
        DefaultKind::Param { .. } => "Param",
        DefaultKind::Let(_) => "Let",
        DefaultKind::Constraint(_) => "Constraint",
    }
}

/// Returns a hash-friendly string representation for a `RequirementKind`.
fn requirement_kind_str(kind: &RequirementKind) -> String {
    match kind {
        RequirementKind::Param(ty) => format!("Param:{}", ty),
        RequirementKind::Let(ty) => format!("Let:{}", ty),
        RequirementKind::Sub(s) => format!("Sub:{}", s),
    }
}

/// Computes the content hash for a trait definition from its components.
///
/// This is the shared implementation used by both `TraitDefBuilder` and
/// `CompiledTraitBuilder` to ensure they produce identical hashes for
/// identical inputs.
fn compute_trait_content_hash(
    name: &str,
    required_members: &[TraitRequirement],
    refinements: &[String],
    type_params: &[TypeParam],
    defaults: &[TraitDefault],
) -> ContentHash {
    let name_hash = ContentHash::of_str(name);
    let req_hashes = required_members.iter().map(|r| {
        let kind_str = requirement_kind_str(&r.kind);
        ContentHash::of_str(&format!("{}:{}", r.name, kind_str))
    });
    let ref_hashes = refinements.iter().map(|r| ContentHash::of_str(r));
    let type_param_hashes = type_params.iter().map(|p| ContentHash::of_str(&p.name));
    let default_hashes = defaults.iter().map(|d| {
        let kind_tag = default_kind_tag(&d.kind);
        ContentHash::of_str(&format!(
            "{}:{}",
            d.name.as_deref().unwrap_or(""),
            kind_tag
        ))
    });
    let all_hashes = std::iter::once(name_hash)
        .chain(req_hashes)
        .chain(ref_hashes)
        .chain(type_param_hashes)
        .chain(default_hashes);
    ContentHash::combine_all(all_hashes)
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
        let content_hash = compute_trait_content_hash(
            &self.name,
            &self.required_members,
            &self.refinements,
            &self.type_params,
            &self.defaults,
        );

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

    pub fn type_param(mut self, param: TypeParam) -> Self {
        self.type_params.push(param);
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
        // Comprehensive hashing aligned with TraitDefBuilder's approach
        let content_hash = {
            let name_hash = ContentHash::of_str(&self.name);
            let req_hashes = self.required_members.iter().map(|r| {
                let kind_str = match &r.kind {
                    RequirementKind::Param(ty) => format!("Param:{}", ty),
                    RequirementKind::Let(ty) => format!("Let:{}", ty),
                    RequirementKind::Sub(s) => format!("Sub:{}", s),
                };
                ContentHash::of_str(&format!("{}:{}", r.name, kind_str))
            });
            let ref_hashes = self.refinements.iter().map(|r| ContentHash::of_str(r));
            let type_param_hashes = self
                .type_params
                .iter()
                .map(|p| ContentHash::of_str(&p.name));
            let default_hashes = self.defaults.iter().map(|d| {
                let kind_tag = match &d.kind {
                    DefaultKind::Param { .. } => "Param",
                    DefaultKind::Let(_) => "Let",
                    DefaultKind::Constraint(_) => "Constraint",
                };
                ContentHash::of_str(&format!(
                    "{}:{}",
                    d.name.as_deref().unwrap_or(""),
                    kind_tag
                ))
            });
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

// Tests remaining in mod.rs pending extraction to their target submodules:
#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::DimensionVector;

    #[test]
    fn compute_trait_content_hash_minimal() {
        let hash = compute_trait_content_hash("X", &[], &[], &[], &[]);
        assert_ne!(hash, ContentHash(0), "name-only hash must be non-zero");
    }

    #[test]
    fn compute_trait_content_hash_differs_by_name() {
        let h1 = compute_trait_content_hash("A", &[], &[], &[], &[]);
        let h2 = compute_trait_content_hash("B", &[], &[], &[], &[]);
        assert_ne!(h1, h2, "different names must produce different hashes");
    }

    #[test]
    fn compute_trait_content_hash_matches_builder() {
        let from_builder = TraitDefBuilder::new("X").build().content_hash;
        let from_fn = compute_trait_content_hash("X", &[], &[], &[], &[]);
        assert_eq!(
            from_builder, from_fn,
            "extracted function must match TraitDefBuilder output"
        );
    }

    #[test]
    fn default_kind_tag_covers_all_variants() {
        let param_tag = default_kind_tag(&DefaultKind::Param {
            cell_type: Type::Real,
            default_decl: reify_syntax::ParamDecl {
                name: "x".to_string(),
                doc: None,
                type_expr: None,
                default: None,
                where_clause: None,
                span: SourceSpan::new(0, 0),
                content_hash: ContentHash::of_str("x"),
            },
        });
        assert_eq!(param_tag, "Param");

        let let_tag = default_kind_tag(&DefaultKind::Let(reify_syntax::LetDecl {
            name: "y".to_string(),
            is_pub: false,
            doc: None,
            type_expr: None,
            value: reify_syntax::Expr {
                kind: reify_syntax::ExprKind::BoolLiteral(true),
                span: SourceSpan::new(0, 0),
            },
            where_clause: None,
            span: SourceSpan::new(0, 0),
            content_hash: ContentHash::of_str("y"),
        }));
        assert_eq!(let_tag, "Let");

        let constraint_tag = default_kind_tag(&DefaultKind::Constraint(reify_syntax::ConstraintDecl {
            label: Some("c".to_string()),
            expr: reify_syntax::Expr {
                kind: reify_syntax::ExprKind::BoolLiteral(true),
                span: SourceSpan::new(0, 0),
            },
            where_clause: None,
            span: SourceSpan::new(0, 0),
            content_hash: ContentHash::of_str("c"),
        }));
        assert_eq!(constraint_tag, "Constraint");
    }

    #[test]
    fn requirement_kind_str_covers_all_variants() {
        let param_str = requirement_kind_str(&RequirementKind::Param(Type::Real));
        assert_eq!(param_str, "Param:Real");

        let let_str = requirement_kind_str(&RequirementKind::Let(Type::Int));
        assert_eq!(let_str, "Let:Int");

        let sub_str = requirement_kind_str(&RequirementKind::Sub("MountPoint".to_string()));
        assert_eq!(sub_str, "Sub:MountPoint");
    }

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
    fn trait_def_content_hash_differs_by_requirement_inner_type() {
        let ct1 = TraitDefBuilder::new("Rigid")
            .requirement("val", RequirementKind::Param(Type::Real))
            .build();
        let ct2 = TraitDefBuilder::new("Rigid")
            .requirement("val", RequirementKind::Param(Type::Int))
            .build();
        assert_ne!(
            ct1.content_hash, ct2.content_hash,
            "same Param variant but different inner types (Real vs Int) must produce different content_hash"
        );
    }

    #[test]
    fn trait_def_content_hash_differs_by_default_kind() {
        let ct1 = TraitDefBuilder::new("Rigid")
            .add_default(
                Some("d"),
                DefaultKind::Param {
                    cell_type: Type::Real,
                    default_decl: reify_syntax::ParamDecl {
                        name: "d".to_string(),
                        doc: None,
                        type_expr: None,
                        default: None,
                        where_clause: None,
                        span: SourceSpan::new(0, 0),
                        content_hash: ContentHash::of_str("d"),
                    },
                },
            )
            .build();
        let ct2 = TraitDefBuilder::new("Rigid")
            .add_default(
                Some("d"),
                DefaultKind::Constraint(reify_syntax::ConstraintDecl {
                    label: Some("d".to_string()),
                    expr: reify_syntax::Expr {
                        kind: reify_syntax::ExprKind::BoolLiteral(true),
                        span: SourceSpan::new(0, 0),
                    },
                    where_clause: None,
                    span: SourceSpan::new(0, 0),
                    content_hash: ContentHash::of_str("d"),
                }),
            )
            .build();
        assert_ne!(
            ct1.content_hash, ct2.content_hash,
            "same default name but different DefaultKind variant must produce different content_hash"
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

    #[test]
    fn compiled_trait_builder_hash_differs_by_requirement_kind() {
        let t1: CompiledTrait = CompiledTraitBuilder::new("Rigid")
            .require_param("val", Type::Real)
            .build();
        let t2: CompiledTrait = CompiledTraitBuilder::new("Rigid")
            .require_let("val", Type::Real)
            .build();
        assert_ne!(
            t1.content_hash, t2.content_hash,
            "Param vs Let with same name and type must produce different content_hash"
        );
    }

    #[test]
    fn compiled_trait_builder_hash_differs_by_requirement_type() {
        let t1: CompiledTrait = CompiledTraitBuilder::new("Rigid")
            .require_param("val", Type::Real)
            .build();
        let t2: CompiledTrait = CompiledTraitBuilder::new("Rigid")
            .require_param("val", Type::Int)
            .build();
        assert_ne!(
            t1.content_hash, t2.content_hash,
            "same Param variant but different types (Real vs Int) must produce different content_hash"
        );
    }

    #[test]
    fn compiled_trait_builder_hash_differs_by_refinement() {
        let t1: CompiledTrait = CompiledTraitBuilder::new("Rigid").build();
        let t2: CompiledTrait = CompiledTraitBuilder::new("Rigid")
            .refinement("Base")
            .build();
        assert_ne!(
            t1.content_hash, t2.content_hash,
            "with vs without refinement must produce different content_hash"
        );
    }

    /// Both builders must produce identical content_hash for identical inputs.
    /// This is the core invariant the refactoring must preserve.
    #[test]
    fn cross_builder_hash_equivalence() {
        // Minimal case: name only
        let from_def = TraitDefBuilder::new("Rigid").build();
        let from_compiled = CompiledTraitBuilder::new("Rigid").build();
        assert_eq!(
            from_def.content_hash, from_compiled.content_hash,
            "name-only: both builders must produce same hash"
        );

        // With a Param requirement
        let from_def = TraitDefBuilder::new("Rigid")
            .requirement("val", RequirementKind::Param(Type::Real))
            .build();
        let from_compiled = CompiledTraitBuilder::new("Rigid")
            .require_param("val", Type::Real)
            .build();
        assert_eq!(
            from_def.content_hash, from_compiled.content_hash,
            "with Param requirement: both builders must produce same hash"
        );

        // With a refinement
        let from_def = TraitDefBuilder::new("Rigid")
            .refinement("Base")
            .build();
        let from_compiled = CompiledTraitBuilder::new("Rigid")
            .refinement("Base")
            .build();
        assert_eq!(
            from_def.content_hash, from_compiled.content_hash,
            "with refinement: both builders must produce same hash"
        );

        // With a type param
        use reify_types::{TraitBound, TraitRef};
        let param = || TypeParam {
            name: "T".to_string(),
            bounds: vec![TraitBound {
                trait_ref: TraitRef {
                    name: "Rigid".to_string(),
                    type_args: vec![],
                },
            }],
            default: None,
        };
        let from_def = TraitDefBuilder::new("Container")
            .type_param(param())
            .build();
        let from_compiled = CompiledTraitBuilder::new("Container")
            .type_param(param())
            .build();
        assert_eq!(
            from_def.content_hash, from_compiled.content_hash,
            "with type_param: both builders must produce same hash"
        );
    }

    #[test]
    fn compiled_trait_builder_hash_differs_by_type_param() {
        use reify_types::{TraitBound, TraitRef, TypeParam};
        let t1: CompiledTrait = CompiledTraitBuilder::new("Container").build();
        let t2: CompiledTrait = CompiledTraitBuilder::new("Container")
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
            t1.content_hash, t2.content_hash,
            "with vs without type_param must produce different content_hash"
        );
    }
}
