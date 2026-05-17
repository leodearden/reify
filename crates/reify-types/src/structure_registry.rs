//! Per-Engine structure-definition registry.
//!
//! Maps interned [`StructureTypeId`]s to [`StructureMeta`] (declared trait
//! bounds, `@version(N)`, source span, field layout). Backs the
//! `Value::StructureInstance` side-table per
//! `docs/prds/v0_3/structure-instance-runtime.md` (task SIR-α / 3540).
//!
//! The registry is per-Engine: ids are *not* stable across Engine restarts,
//! so cache-key composition keys off the structure *name* + `version`, never
//! the [`StructureTypeId`].

use crate::{SourceSpan, Type};
use std::collections::HashMap;

/// Stable per-Engine identifier for an interned structure definition.
///
/// Opaque `u32` handle into the [`StructureRegistry`] side-table. Not stable
/// across Engine restarts — cache-key composition uses the structure *name*,
/// not this id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructureTypeId(pub u32);

/// Side-table metadata for a structure definition.
///
/// Sourced from the compiler's `TopologyTemplate` at Engine construction:
/// declared trait bounds drive nominal-conformance checks, `version` drives
/// `@version(N)` cache invalidation, `field_layout` records declaration order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StructureMeta {
    /// Canonical structure name (e.g. `"Steel_AISI_1045"`). Also the
    /// cache-key-stable identity across Engine restarts.
    pub name: String,
    /// `@version(N)` value; defaults to `1` when the annotation is absent.
    pub version: u32,
    /// Trait names this structure declares conformance to
    /// (`structure def Foo : Bar` → `["Bar"]`).
    pub declared_trait_bounds: Vec<String>,
    /// Source span of the `structure def` declaration, if known. `None` for
    /// synthetic / prelude-interned structures without a span.
    pub source: Option<SourceSpan>,
    /// Declared parameters in declaration order: `(field_name, field_type)`.
    pub field_layout: Vec<(String, Type)>,
}

/// Per-Engine registry mapping structure names ↔ ids and ids → meta.
///
/// `intern` is idempotent on the name: re-interning an existing name keeps
/// the original [`StructureTypeId`] stable while overwriting its metadata.
#[derive(Debug, Clone, Default)]
pub struct StructureRegistry {
    by_id: Vec<StructureMeta>,
    by_name: HashMap<String, StructureTypeId>,
}

impl StructureRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern `name` with its `meta`, returning a stable id.
    ///
    /// If `name` was already interned, the existing id is returned and its
    /// metadata is overwritten with `meta` (id stays stable). Otherwise a
    /// fresh sequential id is allocated.
    pub fn intern(&mut self, name: &str, meta: StructureMeta) -> StructureTypeId {
        if let Some(&id) = self.by_name.get(name) {
            self.by_id[id.0 as usize] = meta;
            id
        } else {
            let id = StructureTypeId(self.by_id.len() as u32);
            self.by_id.push(meta);
            self.by_name.insert(name.to_string(), id);
            id
        }
    }

    /// Look up the id for a structure name, if interned.
    pub fn id_for(&self, name: &str) -> Option<StructureTypeId> {
        self.by_name.get(name).copied()
    }

    /// Look up the canonical name for an id, if it is in range.
    pub fn name_for(&self, id: StructureTypeId) -> Option<&str> {
        self.by_id.get(id.0 as usize).map(|m| m.name.as_str())
    }

    /// Borrow the full metadata for an id, if it is in range.
    pub fn meta(&self, id: StructureTypeId) -> Option<&StructureMeta> {
        self.by_id.get(id.0 as usize)
    }

    /// Borrow the declared trait bounds for an id, if it is in range.
    pub fn declared_bounds(&self, id: StructureTypeId) -> Option<&[String]> {
        self.meta(id).map(|m| m.declared_trait_bounds.as_slice())
    }

    /// Number of interned structures.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the registry has no interned structures.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Type;

    fn sample_meta(name: &str, version: u32, bounds: &[&str]) -> StructureMeta {
        StructureMeta {
            name: name.to_string(),
            version,
            declared_trait_bounds: bounds.iter().map(|s| s.to_string()).collect(),
            source: None,
            field_layout: vec![("youngs_modulus".to_string(), Type::Real)],
        }
    }

    #[test]
    fn empty_registry_returns_none_for_all_lookups() {
        let reg = StructureRegistry::new();
        assert_eq!(reg.id_for("Anything"), None);
        assert_eq!(reg.name_for(StructureTypeId(0)), None);
        assert!(reg.meta(StructureTypeId(0)).is_none());
        assert!(reg.declared_bounds(StructureTypeId(0)).is_none());
    }

    #[test]
    fn intern_returns_stable_ids_for_same_name() {
        let mut reg = StructureRegistry::new();
        let id1 = reg.intern(
            "Steel_AISI_1045",
            sample_meta("Steel_AISI_1045", 1, &["ElasticMaterial"]),
        );
        let id2 = reg.intern(
            "Steel_AISI_1045",
            sample_meta("Steel_AISI_1045", 1, &["ElasticMaterial"]),
        );
        assert_eq!(id1, id2, "re-interning the same name must yield the same id");
    }

    #[test]
    fn distinct_names_get_distinct_ids() {
        let mut reg = StructureRegistry::new();
        let a = reg.intern("A", sample_meta("A", 1, &[]));
        let b = reg.intern("B", sample_meta("B", 1, &[]));
        assert_ne!(a, b);
    }

    #[test]
    fn lookup_by_name_and_by_id_are_consistent() {
        let mut reg = StructureRegistry::new();
        let id = reg.intern("Beam", sample_meta("Beam", 3, &["Member"]));

        assert_eq!(reg.id_for("Beam"), Some(id));
        assert_eq!(reg.name_for(id), Some("Beam"));

        let m = reg.meta(id).expect("meta present after intern");
        assert_eq!(m.name, "Beam");
        assert_eq!(m.version, 3);

        assert_eq!(
            reg.declared_bounds(id).map(<[String]>::to_vec),
            Some(vec!["Member".to_string()])
        );
    }

    #[test]
    fn name_for_unknown_id_returns_none() {
        let mut reg = StructureRegistry::new();
        let id = reg.intern("Only", sample_meta("Only", 1, &[]));
        // An id one past the interned range is unknown.
        let bogus = StructureTypeId(id.0 + 1);
        assert_eq!(reg.name_for(bogus), None);
        assert!(reg.meta(bogus).is_none());
        assert!(reg.declared_bounds(bogus).is_none());
    }

    #[test]
    fn idempotent_reintern_overwrites_meta_but_keeps_id_stable() {
        let mut reg = StructureRegistry::new();
        let id1 = reg.intern("Foo", sample_meta("Foo", 1, &["Bar"]));
        let id2 = reg.intern("Foo", sample_meta("Foo", 2, &["Bar", "Baz"]));

        assert_eq!(id1, id2, "id must remain stable across re-intern");
        let m = reg.meta(id2).expect("meta present");
        assert_eq!(m.version, 2, "meta overwritten with the newer version");
        assert_eq!(
            reg.declared_bounds(id2).map(<[String]>::to_vec),
            Some(vec!["Bar".to_string(), "Baz".to_string()]),
            "declared bounds overwritten on re-intern"
        );
        // by_name must still resolve to the same stable id.
        assert_eq!(reg.id_for("Foo"), Some(id1));
    }
}
