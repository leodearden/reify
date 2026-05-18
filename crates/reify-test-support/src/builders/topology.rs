use std::collections::HashSet;

use reify_compiler::{
    CompiledConnection, CompiledConstraint, CompiledForallTemplate, CompiledGeometryOp,
    CompiledGuardedGroup, EntityKind, RealizationDecl, SolverHint, SubComponentDecl,
    TopologyTemplate, ValueCellDecl, ValueCellKind,
};
use reify_syntax;
use reify_types::{
    CompiledExpr, ConstraintNodeId, ContentHash, RealizationNodeId, SourceSpan, Type, TypeParam,
    ValueCellId,
};

/// Builder for `TopologyTemplate`.
pub struct TopologyTemplateBuilder {
    name: String,
    doc: Option<String>,
    entity_kind: EntityKind,
    visibility: reify_compiler::Visibility,
    type_params: Vec<TypeParam>,
    trait_bounds: Vec<String>,
    value_cells: Vec<ValueCellDecl>,
    constraints: Vec<CompiledConstraint>,
    realizations: Vec<RealizationDecl>,
    sub_components: Vec<SubComponentDecl>,
    guarded_groups: Vec<CompiledGuardedGroup>,
    structure_controlling: HashSet<ValueCellId>,
    objective: Option<reify_types::OptimizationObjective>,
    meta: std::collections::HashMap<String, String>,
    is_recursive: bool,
    annotations: Vec<reify_types::Annotation>,
    pragmas: Vec<reify_syntax::Pragma>,
    forall_templates: Vec<CompiledForallTemplate>,
    connections: Vec<CompiledConnection>,
}

impl TopologyTemplateBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            doc: None,
            entity_kind: EntityKind::Structure,
            visibility: reify_compiler::Visibility::Private,
            type_params: Vec::new(),
            trait_bounds: Vec::new(),
            value_cells: Vec::new(),
            constraints: Vec::new(),
            realizations: Vec::new(),
            sub_components: Vec::new(),
            guarded_groups: Vec::new(),
            structure_controlling: HashSet::new(),
            objective: None,
            meta: std::collections::HashMap::new(),
            is_recursive: false,
            annotations: Vec::new(),
            pragmas: Vec::new(),
            forall_templates: Vec::new(),
            connections: Vec::new(),
        }
    }

    /// Set the `doc` string for this template (mirrors `TopologyTemplate::doc`).
    pub fn doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = Some(doc.into());
        self
    }

    /// Add a captured `CompiledForallTemplate` to the builder (task 2629).
    ///
    /// Used by hand-built fixtures exercising the runtime forall re-elaboration
    /// path. Production builds populate `forall_templates` via
    /// `forall_elaborate.rs` automatically.
    pub fn forall_template(mut self, ft: CompiledForallTemplate) -> Self {
        self.forall_templates.push(ft);
        self
    }

    /// Add a `CompiledConnection` to the builder (task 2690).
    ///
    /// Used by hand-built fixtures exercising the runtime
    /// `EvaluationGraph::connections` carrier. Production builds populate
    /// `connections` via `compile_connection` in `connect.rs` and
    /// `forall_elaborate.rs` automatically.
    pub fn connection(mut self, conn: CompiledConnection) -> Self {
        self.connections.push(conn);
        self
    }

    /// Push a single annotation onto this builder.
    pub fn annotation(mut self, ann: reify_types::Annotation) -> Self {
        self.annotations.push(ann);
        self
    }

    /// Replace all annotations with the given vec.
    pub fn annotations(mut self, anns: Vec<reify_types::Annotation>) -> Self {
        self.annotations = anns;
        self
    }

    /// Set meta entries for this template.
    pub fn meta(mut self, entries: std::collections::HashMap<String, String>) -> Self {
        self.meta = entries;
        self
    }

    /// Declare a trait bound this structure conforms to.
    pub fn trait_bound(mut self, name: impl Into<String>) -> Self {
        self.trait_bounds.push(name.into());
        self
    }

    /// Add a type parameter to this structure.
    pub fn type_param(mut self, param: TypeParam) -> Self {
        self.type_params.push(param);
        self
    }

    pub fn visibility(mut self, vis: reify_compiler::Visibility) -> Self {
        self.visibility = vis;
        self
    }

    pub fn param(
        mut self,
        entity: &str,
        member: &str,
        cell_type: Type,
        default: Option<CompiledExpr>,
    ) -> Self {
        self.value_cells.push(ValueCellDecl {
            id: ValueCellId::new(entity, member),
            kind: ValueCellKind::Param,
            visibility: reify_compiler::Visibility::Public,
            cell_type,
            default_expr: default,
            solver_hints: Vec::new(),
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn auto_param(mut self, entity: &str, member: &str, cell_type: Type) -> Self {
        self.value_cells.push(ValueCellDecl {
            id: ValueCellId::new(entity, member),
            kind: ValueCellKind::Auto { free: false },
            visibility: reify_compiler::Visibility::Public,
            cell_type,
            default_expr: None,
            solver_hints: Vec::new(),
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn auto_param_free(mut self, entity: &str, member: &str, cell_type: Type) -> Self {
        self.value_cells.push(ValueCellDecl {
            id: ValueCellId::new(entity, member),
            kind: ValueCellKind::Auto { free: true },
            visibility: reify_compiler::Visibility::Public,
            cell_type,
            default_expr: None,
            solver_hints: Vec::new(),
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn param_with_solver_hints(
        mut self,
        entity: &str,
        member: &str,
        cell_type: Type,
        default: Option<CompiledExpr>,
        solver_hints: Vec<SolverHint>,
    ) -> Self {
        self.value_cells.push(ValueCellDecl {
            id: ValueCellId::new(entity, member),
            kind: ValueCellKind::Param,
            visibility: reify_compiler::Visibility::Public,
            cell_type,
            default_expr: default,
            solver_hints,
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn let_binding(
        mut self,
        entity: &str,
        member: &str,
        cell_type: Type,
        expr: CompiledExpr,
    ) -> Self {
        self.value_cells.push(ValueCellDecl {
            id: ValueCellId::new(entity, member),
            kind: ValueCellKind::Let,
            visibility: reify_compiler::Visibility::Private,
            cell_type,
            default_expr: Some(expr),
            solver_hints: Vec::new(),
            span: SourceSpan::new(0, 0),
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
            optimized_target: None,
        });
        self
    }

    pub fn realization(
        mut self,
        entity: &str,
        index: u32,
        operations: Vec<CompiledGeometryOp>,
    ) -> Self {
        let span = SourceSpan::new(0, 0);
        let feature_tags = reify_compiler::derive_feature_tags(&operations, span);
        self.realizations.push(RealizationDecl {
            id: RealizationNodeId::new(entity, index),
            name: None,
            feature_tags,
            operations,
            // Sentinel (0, 0): builder-constructed RealizationDecls have no originating
            // source span.  Callers that exercise span-aware diagnostics must construct
            // the RealizationDecl directly rather than via this builder.
            span,
        });
        self
    }

    /// Like `realization` but sets the user-facing `name` field, which lets
    /// the engine build a name→handle map for `GeomRef::Sub` resolution.
    pub fn realization_named(
        mut self,
        entity: &str,
        index: u32,
        name: impl Into<String>,
        operations: Vec<CompiledGeometryOp>,
    ) -> Self {
        let span = SourceSpan::new(0, 0);
        let feature_tags = reify_compiler::derive_feature_tags(&operations, span);
        self.realizations.push(RealizationDecl {
            id: RealizationNodeId::new(entity, index),
            name: Some(name.into()),
            feature_tags,
            operations,
            // Sentinel (0, 0): builder-constructed RealizationDecls have no originating
            // source span.  Callers that exercise span-aware diagnostics must construct
            // the RealizationDecl directly rather than via this builder.
            span,
        });
        self
    }

    pub fn objective(mut self, obj: reify_types::OptimizationObjective) -> Self {
        self.objective = Some(obj);
        self
    }

    pub fn sub_component(
        mut self,
        name: impl Into<String>,
        structure_name: impl Into<String>,
        args: Vec<(String, CompiledExpr)>,
    ) -> Self {
        let name = name.into();
        let structure_name = structure_name.into();
        self.sub_components.push(SubComponentDecl {
            content_hash: ContentHash::of_str(&format!("sub {} = {}", name, structure_name)),
            name,
            structure_name,
            visibility: reify_compiler::Visibility::Public,
            args,
            type_args: Vec::new(),
            is_collection: false,
            count_cell: None,
            guard_state: reify_compiler::GuardState::None,
            span: SourceSpan::new(0, 0),
        });
        self
    }

    /// Mark this template as recursive (used by tests to simulate Tarjan SCC detection).
    pub fn is_recursive(mut self, recursive: bool) -> Self {
        self.is_recursive = recursive;
        self
    }

    /// Add a sub-component with a guard expression (for recursive subs that need termination guards).
    pub fn sub_component_with_guard(
        mut self,
        name: impl Into<String>,
        structure_name: impl Into<String>,
        args: Vec<(String, CompiledExpr)>,
        guard_expr: CompiledExpr,
    ) -> Self {
        let name = name.into();
        let structure_name = structure_name.into();
        self.sub_components.push(SubComponentDecl {
            content_hash: ContentHash::of_str(&format!(
                "sub {} = {} where ...",
                name, structure_name
            )),
            name,
            structure_name,
            visibility: reify_compiler::Visibility::Public,
            args,
            type_args: Vec::new(),
            is_collection: false,
            count_cell: None,
            guard_state: reify_compiler::GuardState::Compiled(Box::new(guard_expr)),
            span: SourceSpan::new(0, 0),
        });
        self
    }

    /// Add a collection sub-component (`sub name : List<T>`) with a count cell.
    pub fn collection_sub_component(
        mut self,
        name: impl Into<String>,
        structure_name: impl Into<String>,
        count_cell: ValueCellId,
    ) -> Self {
        let name = name.into();
        let structure_name = structure_name.into();
        self.sub_components.push(SubComponentDecl {
            content_hash: ContentHash::of_str(&format!("sub {} : List<{}>", name, structure_name)),
            name,
            structure_name,
            visibility: reify_compiler::Visibility::Public,
            args: Vec::new(),
            type_args: Vec::new(),
            is_collection: true,
            count_cell: Some(count_cell),
            guard_state: reify_compiler::GuardState::None,
            span: SourceSpan::new(0, 0),
        });
        self
    }

    /// Add a ValueCellId to the structure_controlling set.
    pub fn structure_controlling_cell(mut self, id: ValueCellId) -> Self {
        self.structure_controlling.insert(id);
        self
    }

    pub fn guarded_group(
        mut self,
        guard_expr: CompiledExpr,
        guard_value_cell: ValueCellId,
        members: Vec<ValueCellDecl>,
        constraints: Vec<CompiledConstraint>,
        else_members: Vec<ValueCellDecl>,
        else_constraints: Vec<CompiledConstraint>,
    ) -> Self {
        self.structure_controlling.insert(guard_value_cell.clone());
        self.guarded_groups.push(CompiledGuardedGroup {
            guard_expr,
            guard_value_cell,
            members,
            constraints,
            else_members,
            else_constraints,
            parent_guard: None,
        });
        self
    }

    pub fn build(self) -> TopologyTemplate {
        // Build a content-sensitive hash matching compile_structure() logic.
        let content_hash = {
            let name_hash = ContentHash::of_str(&self.name);

            let vc_hashes = self.value_cells.iter().map(|vc| {
                vc.default_expr
                    .as_ref()
                    .map(|e| e.content_hash)
                    .unwrap_or(ContentHash(0))
            });

            let constraint_hashes = self.constraints.iter().map(|c| c.expr.content_hash);

            let sub_hashes = self.sub_components.iter().map(|s| s.content_hash);

            let guard_hashes = self.guarded_groups.iter().flat_map(|g| {
                std::iter::once(g.guard_expr.content_hash)
                    .chain(g.members.iter().map(|m| {
                        m.default_expr
                            .as_ref()
                            .map(|e| e.content_hash)
                            .unwrap_or(ContentHash(0))
                    }))
                    .chain(g.constraints.iter().map(|c| c.expr.content_hash))
                    .chain(g.else_members.iter().map(|m| {
                        m.default_expr
                            .as_ref()
                            .map(|e| e.content_hash)
                            .unwrap_or(ContentHash(0))
                    }))
                    .chain(g.else_constraints.iter().map(|c| c.expr.content_hash))
            });

            let all_hashes = std::iter::once(name_hash)
                .chain(vc_hashes)
                .chain(constraint_hashes)
                .chain(sub_hashes)
                .chain(guard_hashes);

            ContentHash::combine_all(all_hashes)
        };

        TopologyTemplate {
            name: self.name,
            doc: self.doc,
            entity_kind: self.entity_kind,
            visibility: self.visibility,
            type_params: self.type_params,
            trait_bounds: self.trait_bounds,
            value_cells: self.value_cells,
            constraints: self.constraints,
            realizations: self.realizations,
            sub_components: self.sub_components,
            ports: Vec::new(),
            connections: self.connections,
            guarded_groups: self.guarded_groups,
            structure_controlling: self.structure_controlling,
            objective: self.objective,
            meta: self.meta,
            content_hash,
            is_recursive: self.is_recursive,
            annotations: self.annotations,
            pragmas: self.pragmas,
            match_arm_groups: vec![],
            // task 2629: builder-side forall_templates aren't mixed into
            // content_hash above — mirroring the production-side intentional
            // omission so cache keys are stable across the runtime-only field.
            forall_templates: self.forall_templates,
        }
    }
}

#[cfg(test)]
mod annotation_tests {
    use super::*;
    use crate::builders::{ann_str, annotation, annotation_with_args};
    use reify_types::{DEPRECATED_ANNOTATION, OPTIMIZED_ANNOTATION};

    #[test]
    fn topology_builder_single_annotation() {
        let t = TopologyTemplateBuilder::new("T")
            .annotation(annotation(OPTIMIZED_ANNOTATION))
            .build();
        assert_eq!(t.annotations.len(), 1);
        assert_eq!(t.annotations[0].name, OPTIMIZED_ANNOTATION);
    }

    #[test]
    fn topology_builder_annotation_with_args() {
        let t = TopologyTemplateBuilder::new("T")
            .annotation(annotation_with_args(
                DEPRECATED_ANNOTATION,
                vec![ann_str("use Q")],
            ))
            .build();
        assert_eq!(t.annotations.len(), 1);
        assert_eq!(t.annotations[0].args.len(), 1);
    }

    #[test]
    fn topology_builder_annotations_replace_all() {
        let t = TopologyTemplateBuilder::new("T")
            .annotations(vec![annotation("a"), annotation("b")])
            .build();
        assert_eq!(t.annotations.len(), 2);
    }

    #[test]
    fn topology_builder_annotation_does_not_affect_content_hash() {
        let t1 = TopologyTemplateBuilder::new("T").build();
        let t2 = TopologyTemplateBuilder::new("T")
            .annotation(annotation(OPTIMIZED_ANNOTATION))
            .build();
        assert_eq!(t1.content_hash, t2.content_hash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_param_builder() {
        let template = TopologyTemplateBuilder::new("T")
            .auto_param("T", "x", Type::length())
            .build();

        assert_eq!(template.value_cells.len(), 1);
        let cell = &template.value_cells[0];
        assert_eq!(cell.id, ValueCellId::new("T", "x"));
        assert!(cell.kind.is_auto());
        assert!(cell.default_expr.is_none());
        assert_eq!(cell.cell_type, Type::length());
    }

    #[test]
    fn topology_with_trait_bound() {
        let template = TopologyTemplateBuilder::new("Bolt")
            .trait_bound("Rigid")
            .build();
        assert_eq!(template.trait_bounds.len(), 1);
        assert_eq!(template.trait_bounds[0], "Rigid");
    }

    #[test]
    fn topology_with_multiple_trait_bounds() {
        let template = TopologyTemplateBuilder::new("Bolt")
            .trait_bound("Rigid")
            .trait_bound("Fastener")
            .build();
        assert_eq!(template.trait_bounds.len(), 2);
        assert!(template.trait_bounds.contains(&"Rigid".to_string()));
        assert!(template.trait_bounds.contains(&"Fastener".to_string()));
    }

    #[test]
    fn topology_with_type_param() {
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
        let template = TopologyTemplateBuilder::new("Container")
            .type_param(param)
            .build();
        assert_eq!(template.type_params.len(), 1);
        assert_eq!(template.type_params[0].name, "T");
        assert_eq!(template.type_params[0].bounds[0].trait_ref.name, "Rigid");
    }
}
