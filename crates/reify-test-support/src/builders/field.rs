use reify_compiler::{CompiledField, CompiledFieldSource};
use reify_core::{ContentHash, Type};
use reify_ir::CompiledExpr;

// --- CompiledFieldBuilder ---

/// Builder for `CompiledField`.
pub struct CompiledFieldBuilder {
    name: String,
    is_pub: bool,
    domain_type: Type,
    codomain_type: Type,
    source: Option<CompiledFieldSource>,
    annotations: Vec<reify_ir::Annotation>,
}

impl CompiledFieldBuilder {
    pub fn new(name: impl Into<String>, domain_type: Type, codomain_type: Type) -> Self {
        Self {
            name: name.into(),
            is_pub: false,
            domain_type,
            codomain_type,
            source: None,
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

    /// Set source to `Imported` with no path/format/grid (anonymous placeholder).
    pub fn imported(mut self) -> Self {
        self.source = Some(CompiledFieldSource::Imported { path: None, format: None, grid: None });
        self
    }

    /// Set source to `Imported` with explicit path, format, and grid.
    pub fn imported_from(mut self, path: &str, format: &str, grid: &str) -> Self {
        self.source = Some(CompiledFieldSource::Imported {
            path: Some(path.to_string()),
            format: Some(format.to_string()),
            grid: Some(grid.to_string()),
        });
        self
    }

    pub fn build(self) -> CompiledField {
        let source = self
            .source
            .expect("CompiledFieldBuilder: source must be set before build()");
        // Mirror the real compiler's hashing pattern (lib.rs:5448-5464)
        let content_hash = {
            let name_hash = ContentHash::of_str(&self.name);
            let domain_hash = ContentHash::of_str(&format!("{}", self.domain_type));
            let codomain_hash = ContentHash::of_str(&format!("{}", self.codomain_type));
            let source_hash = match &source {
                CompiledFieldSource::Analytical { expr } => expr.content_hash,
                CompiledFieldSource::Sampled { config } => {
                    let hashes = config
                        .iter()
                        .map(|(k, e)| ContentHash::of_str(k).combine(e.content_hash));
                    ContentHash::combine_all(hashes)
                }
                CompiledFieldSource::Composed { expr } => expr.content_hash,
                CompiledFieldSource::Imported { path, format, grid } => {
                    let ph = path.as_deref().map(ContentHash::of_str).unwrap_or(ContentHash(0));
                    let fh = format.as_deref().map(ContentHash::of_str).unwrap_or(ContentHash(0));
                    let gh = grid.as_deref().map(ContentHash::of_str).unwrap_or(ContentHash(0));
                    ContentHash::combine_all([ph, fh, gh])
                }
            };
            ContentHash::combine_all([name_hash, domain_hash, codomain_hash, source_hash])
        };
        CompiledField {
            name: self.name,
            is_pub: self.is_pub,
            domain_type: self.domain_type,
            codomain_type: self.codomain_type,
            source,
            content_hash,
            annotations: self.annotations,
        }
    }
}

#[cfg(test)]
mod annotation_tests {
    use super::*;
    use crate::builders::{ann_str, annotation, annotation_with_args};
    use reify_core::{DEPRECATED_ANNOTATION, TEST_ANNOTATION};

    #[test]
    fn compiled_field_builder_single_annotation() {
        let field =
            CompiledFieldBuilder::new("f", reify_core::Type::Geometry, reify_core::Type::dimensionless_scalar())
                .imported()
                .annotation(annotation(DEPRECATED_ANNOTATION))
                .build();
        assert_eq!(field.annotations.len(), 1);
        assert_eq!(field.annotations[0].name, DEPRECATED_ANNOTATION);
    }

    #[test]
    fn compiled_field_builder_annotation_with_args() {
        let field =
            CompiledFieldBuilder::new("f", reify_core::Type::Geometry, reify_core::Type::dimensionless_scalar())
                .imported()
                .annotation(annotation_with_args(
                    DEPRECATED_ANNOTATION,
                    vec![ann_str("use bar")],
                ))
                .build();
        assert_eq!(field.annotations.len(), 1);
        assert_eq!(field.annotations[0].args.len(), 1);
    }

    #[test]
    fn compiled_field_builder_annotations_replace_all() {
        let field =
            CompiledFieldBuilder::new("f", reify_core::Type::Geometry, reify_core::Type::dimensionless_scalar())
                .imported()
                .annotations(vec![annotation("a"), annotation("b")])
                .build();
        assert_eq!(field.annotations.len(), 2);
    }

    #[test]
    fn compiled_field_builder_annotation_does_not_affect_content_hash() {
        let f1 =
            CompiledFieldBuilder::new("f", reify_core::Type::Geometry, reify_core::Type::dimensionless_scalar())
                .imported()
                .build();
        let f2 =
            CompiledFieldBuilder::new("f", reify_core::Type::Geometry, reify_core::Type::dimensionless_scalar())
                .imported()
                .annotation(annotation(TEST_ANNOTATION))
                .build();
        assert_eq!(f1.content_hash, f2.content_hash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builders::literal;
    use reify_ir::Value;

    #[test]
    fn compiled_field_builder_analytical_produces_field() {
        let body = literal(Value::Real(1.0));
        let field = CompiledFieldBuilder::new("temp", Type::Geometry, Type::dimensionless_scalar())
            .analytical(body)
            .build();
        assert_eq!(field.name, "temp");
        assert!(!field.is_pub);
        assert_eq!(field.domain_type, Type::Geometry);
        assert_eq!(field.codomain_type, Type::dimensionless_scalar());
        assert!(matches!(
            field.source,
            CompiledFieldSource::Analytical { .. }
        ));
        assert_ne!(field.content_hash, ContentHash(0));
    }

    #[test]
    fn compiled_field_builder_public_sampled() {
        let field = CompiledFieldBuilder::new("vel", Type::Geometry, Type::dimensionless_scalar())
            .public()
            .sampled(vec![("resolution", literal(Value::Int(32)))])
            .build();
        assert!(field.is_pub);
        assert!(matches!(field.source, CompiledFieldSource::Sampled { .. }));
        assert_ne!(field.content_hash, ContentHash(0));
    }

    #[test]
    fn compiled_field_builder_composed() {
        let body = literal(Value::Real(0.0));
        let field = CompiledFieldBuilder::new("composed_f", Type::Geometry, Type::dimensionless_scalar())
            .composed(body)
            .build();
        assert!(matches!(field.source, CompiledFieldSource::Composed { .. }));
    }

    #[test]
    fn compiled_field_builder_imported() {
        let field = CompiledFieldBuilder::new("ext", Type::Geometry, Type::dimensionless_scalar())
            .imported()
            .build();
        assert!(matches!(field.source, CompiledFieldSource::Imported { .. }));
        assert_ne!(field.content_hash, ContentHash(0));
    }

    #[test]
    fn compiled_field_hash_differs_by_domain_type() {
        let f1 = CompiledFieldBuilder::new("temp", Type::Geometry, Type::dimensionless_scalar())
            .imported()
            .build();
        let f2 = CompiledFieldBuilder::new("temp", Type::dimensionless_scalar(), Type::dimensionless_scalar())
            .imported()
            .build();
        assert_ne!(
            f1.content_hash, f2.content_hash,
            "fields with same name but different domain_type must produce different content_hash"
        );
    }

    #[test]
    fn compiled_field_hash_differs_by_codomain_type() {
        let f1 = CompiledFieldBuilder::new("temp", Type::Geometry, Type::dimensionless_scalar())
            .imported()
            .build();
        let f2 = CompiledFieldBuilder::new("temp", Type::Geometry, Type::Int)
            .imported()
            .build();
        assert_ne!(
            f1.content_hash, f2.content_hash,
            "fields with same name but different codomain_type must produce different content_hash"
        );
    }

    #[test]
    fn compiled_field_hash_differs_by_source() {
        // Use different expressions for analytical vs composed so source_hash differs.
        // (Real compiler hashes Analytical/Composed identically via expr.content_hash,
        // so same-expr would match — use distinct exprs to test source sensitivity.)
        let f_analytical = CompiledFieldBuilder::new("temp", Type::Geometry, Type::dimensionless_scalar())
            .analytical(literal(Value::Real(1.0)))
            .build();
        let f_composed = CompiledFieldBuilder::new("temp", Type::Geometry, Type::dimensionless_scalar())
            .composed(literal(Value::Real(2.0)))
            .build();
        let f_imported = CompiledFieldBuilder::new("temp", Type::Geometry, Type::dimensionless_scalar())
            .imported()
            .build();
        assert_ne!(
            f_analytical.content_hash, f_composed.content_hash,
            "analytical vs composed (different expr) must produce different content_hash"
        );
        assert_ne!(
            f_analytical.content_hash, f_imported.content_hash,
            "analytical vs imported source must produce different content_hash"
        );
        assert_ne!(
            f_composed.content_hash, f_imported.content_hash,
            "composed vs imported source must produce different content_hash"
        );
    }
}
