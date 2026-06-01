use reify_compiler::{
    CompiledConstraint, CompiledPurpose, CompiledPurposeParam, ResolvedSchemaQuery,
};
use reify_core::{ConstraintNodeId, ContentHash, SourceSpan, ValueCellId};
use reify_ir::CompiledExpr;

// --- CompiledPurposeBuilder ---

/// Builder for `CompiledPurpose`.
pub struct CompiledPurposeBuilder {
    name: String,
    is_pub: bool,
    params: Vec<CompiledPurposeParam>,
    constraints: Vec<CompiledConstraint>,
    objective: Option<reify_ir::ObjectiveSet>,
    resolved_queries: Vec<ResolvedSchemaQuery>,
    annotations: Vec<reify_ir::Annotation>,
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
            annotations: Vec::new(),
        }
    }

    /// Push a single annotation onto this builder.
    pub fn annotation(mut self, ann: reify_ir::Annotation) -> Self {
        self.annotations.push(ann);
        self
    }

    /// Replace all annotations with the given vec.
    pub fn annotations(mut self, anns: Vec<reify_ir::Annotation>) -> Self {
        self.annotations = anns;
        self
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
            optimized_target: None,
        });
        self
    }

    pub fn objective(mut self, obj: reify_ir::ObjectiveSet) -> Self {
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
            lets: Vec::new(),
            constraints: self.constraints,
            objective: self.objective,
            resolved_queries: self.resolved_queries,
            content_hash,
            annotations: self.annotations,
            pragmas: Vec::new(),
        }
    }
}

#[cfg(test)]
mod annotation_tests {
    use super::*;
    use crate::builders::{ann_str, annotation, annotation_with_args};
    use reify_core::{DEPRECATED_ANNOTATION, TEST_ANNOTATION};

    #[test]
    fn compiled_purpose_builder_single_annotation() {
        let p = CompiledPurposeBuilder::new("p")
            .annotation(annotation(TEST_ANNOTATION))
            .build();
        assert_eq!(p.annotations.len(), 1);
        assert_eq!(p.annotations[0].name, TEST_ANNOTATION);
    }

    #[test]
    fn compiled_purpose_builder_annotation_with_args() {
        let p = CompiledPurposeBuilder::new("p")
            .annotation(annotation_with_args(
                DEPRECATED_ANNOTATION,
                vec![ann_str("use q")],
            ))
            .build();
        assert_eq!(p.annotations.len(), 1);
        assert_eq!(p.annotations[0].args.len(), 1);
    }

    #[test]
    fn compiled_purpose_builder_annotations_replace_all() {
        let p = CompiledPurposeBuilder::new("p")
            .annotations(vec![annotation("a"), annotation("b")])
            .build();
        assert_eq!(p.annotations.len(), 2);
    }

    #[test]
    fn compiled_purpose_builder_annotation_does_not_affect_content_hash() {
        let p1 = CompiledPurposeBuilder::new("p").build();
        let p2 = CompiledPurposeBuilder::new("p")
            .annotation(annotation(TEST_ANNOTATION))
            .build();
        assert_eq!(p1.content_hash, p2.content_hash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builders::literal;
    use reify_ir::{ObjectiveSet, ObjectiveSense, Value};

    #[test]
    fn purpose_builder_basic_param_and_constraint() {
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
        let purpose: CompiledPurpose = CompiledPurposeBuilder::new("opt_ready").public().build();
        assert!(purpose.is_pub);
    }

    #[test]
    fn purpose_builder_with_objective() {
        let obj_expr = literal(Value::Real(1.0));
        let purpose: CompiledPurpose = CompiledPurposeBuilder::new("minimize_mass")
            .param("subject", "Structure")
            .objective(ObjectiveSet::single(ObjectiveSense::Minimize, obj_expr))
            .build();
        assert!(purpose.objective.is_some());
        assert_eq!(purpose.resolved_queries.len(), 0);
        assert_ne!(purpose.content_hash, ContentHash(0));
    }

    #[test]
    fn purpose_builder_with_schema_query() {
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
