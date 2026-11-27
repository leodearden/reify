//! C4's runtime written-equals-declared key-set check for Rust result builders
//! (`docs/prds/v0_6/trampoline-decode-and-result-build.md` §4 C4).

use std::collections::BTreeSet;
use std::fmt;

use reify_ir::{MATERIALIZED_ANNOTATIONS_KEY, SOURCE_SPAN_KEY, StructureRegistry, Value};

/// The engine's overlay keys: attached to an evaluated instance's fields map,
/// never declared by its structure.
const OVERLAY_KEYS: [&str; 2] = [SOURCE_SPAN_KEY, MATERIALIZED_ANNOTATIONS_KEY];

/// How the fields a minted instance writes differ from the fields its structure
/// declares; both lists sorted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySetMismatch {
    pub structure: String,
    pub undeclared_written: Vec<String>,
    pub declared_unwritten: Vec<String>,
}

impl fmt::Display for KeySetMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: written but undeclared {:?}, declared but unwritten {:?}",
            self.structure, self.undeclared_written, self.declared_unwritten
        )
    }
}

/// `Ok` when `value` writes exactly the fields its structure declares in
/// `registry`, resolved by `type_name`. The engine's overlay keys
/// (`@@source_span`, `@@materialized_annotations`) are not written fields, so an
/// engine-evaluated instance compares like the builder output it came from.
///
/// # Panics
///
/// When `value` is not a `Value::StructureInstance`, or its structure is not
/// registered: either is a broken test precondition, not a mismatch.
pub fn key_set_parity(registry: &StructureRegistry, value: &Value) -> Result<(), KeySetMismatch> {
    let Value::StructureInstance(data) = value else {
        panic!("key_set_parity needs a StructureInstance, got {value:?}");
    };
    let meta = registry
        .meta_for(data)
        .unwrap_or_else(|| panic!("structure {:?} is not registered", data.type_name));
    let declared: BTreeSet<&str> = meta
        .field_layout
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let written: BTreeSet<&str> = data
        .fields
        .keys()
        .map(String::as_str)
        .filter(|name| !OVERLAY_KEYS.contains(name))
        .collect();
    let undeclared_written: Vec<String> = written
        .difference(&declared)
        .map(|name| name.to_string())
        .collect();
    let declared_unwritten: Vec<String> = declared
        .difference(&written)
        .map(|name| name.to_string())
        .collect();
    if undeclared_written.is_empty() && declared_unwritten.is_empty() {
        Ok(())
    } else {
        Err(KeySetMismatch {
            structure: data.type_name.clone(),
            undeclared_written,
            declared_unwritten,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{SourceSpan, Type};
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
    fn engine_overlays_are_not_written_fields() {
        let Value::StructureInstance(data) = pair_writing(&["a", "b"]) else {
            unreachable!("pair_writing mints a StructureInstance")
        };
        let mut data = data.with_source_span(Some(SourceSpan::new(3, 9)));
        data.fields.insert(
            MATERIALIZED_ANNOTATIONS_KEY.to_string(),
            Value::Map(Default::default()),
        );
        assert_eq!(
            key_set_parity(&pair_registry(), &Value::StructureInstance(Box::new(data))),
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
