use reify_compiler::{CompiledField, CompiledFieldSource};
use reify_types::{CompiledExpr, ContentHash, Type};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builders::literal;
    use reify_types::Value;

    #[test]
    fn compiled_field_builder_analytical_produces_field() {
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
        let body = literal(Value::Real(0.0));
        let field = CompiledFieldBuilder::new("composed_f", Type::Geometry, Type::Real)
            .composed(body)
            .build();
        assert!(matches!(field.source, CompiledFieldSource::Composed { .. }));
    }

    #[test]
    fn compiled_field_builder_imported() {
        let field = CompiledFieldBuilder::new("ext", Type::Geometry, Type::Real)
            .imported()
            .build();
        assert!(matches!(field.source, CompiledFieldSource::Imported));
        assert_ne!(field.content_hash, ContentHash(0));
    }

    #[test]
    fn compiled_field_hash_differs_by_domain_type() {
        let f1 = CompiledFieldBuilder::new("temp", Type::Geometry, Type::Real)
            .imported()
            .build();
        let f2 = CompiledFieldBuilder::new("temp", Type::Real, Type::Real)
            .imported()
            .build();
        assert_ne!(
            f1.content_hash, f2.content_hash,
            "fields with same name but different domain_type must produce different content_hash"
        );
    }

    #[test]
    fn compiled_field_hash_differs_by_codomain_type() {
        let f1 = CompiledFieldBuilder::new("temp", Type::Geometry, Type::Real)
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
        let body = literal(Value::Real(1.0));
        let f_analytical = CompiledFieldBuilder::new("temp", Type::Geometry, Type::Real)
            .analytical(body.clone())
            .build();
        let f_composed = CompiledFieldBuilder::new("temp", Type::Geometry, Type::Real)
            .composed(body)
            .build();
        let f_imported = CompiledFieldBuilder::new("temp", Type::Geometry, Type::Real)
            .imported()
            .build();
        assert_ne!(
            f_analytical.content_hash, f_composed.content_hash,
            "analytical vs composed source must produce different content_hash"
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
