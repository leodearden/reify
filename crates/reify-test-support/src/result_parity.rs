#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::Type;
    use reify_ir::{StructureInstanceData, StructureMeta, StructureRegistry, Value};

    /// A registry declaring one structure, `Pair { a, b }`.
    fn pair_registry() -> StructureRegistry {
        let mut registry = StructureRegistry::new();
        registry.intern(
            "Pair",
            StructureMeta {
                name: "Pair".to_string(),
                version: 1,
                declared_trait_bounds: Vec::new(),
                source: None,
                field_layout: ["a", "b"]
                    .into_iter()
                    .map(|name| (name.to_string(), Type::dimensionless_scalar()))
                    .collect(),
            },
        );
        registry
    }

    /// A registry-free `Pair` instance writing exactly `fields`.
    fn pair_writing(fields: &[&str]) -> Value {
        Value::StructureInstance(Box::new(StructureInstanceData::registry_free(
            "Pair",
            fields
                .iter()
                .map(|name| (name.to_string(), Value::Real(0.0)))
                .collect(),
        )))
    }

    #[test]
    fn writing_exactly_the_declared_fields_is_parity() {
        assert_eq!(
            key_set_parity(&pair_registry(), &pair_writing(&["a", "b"])),
            Ok(())
        );
    }

    #[test]
    fn an_undeclared_write_is_named() {
        let mismatch = key_set_parity(&pair_registry(), &pair_writing(&["a", "b", "warm_started"]))
            .expect_err("an undeclared field must break parity");
        assert_eq!(mismatch.structure, "Pair");
        assert_eq!(mismatch.undeclared_written, ["warm_started"]);
        assert!(mismatch.declared_unwritten.is_empty(), "{mismatch:?}");
    }

    #[test]
    fn a_declared_field_left_unwritten_is_named() {
        let mismatch = key_set_parity(&pair_registry(), &pair_writing(&["a"]))
            .expect_err("an unwritten declared field must break parity");
        assert_eq!(mismatch.structure, "Pair");
        assert_eq!(mismatch.declared_unwritten, ["b"]);
        assert!(mismatch.undeclared_written.is_empty(), "{mismatch:?}");
    }
}
