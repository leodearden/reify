use super::*;

// --- Compilation context ---

/// Name scope: maps identifier names to (ValueCellId, Type, Option<guard_cell_id>)
/// within a structure. The guard cell ID tracks which guard (if any) protects this name.
#[derive(Clone)]
pub(crate) struct CompilationScope<'u> {
    pub(crate) entity_name: String,
    pub(crate) names: HashMap<String, (ValueCellId, Type, Option<ValueCellId>)>,
    /// Names of ports declared in this structure, for member access disambiguation.
    pub(crate) port_names: HashSet<String>,
    /// Names of collection sub-components (sub name : List<T>), for count expression handling.
    pub(crate) collection_sub_names: HashSet<String>,
    /// Member types for collection sub-components: collection_name → { member_name → Type }.
    /// Populated from already-compiled child templates to resolve correct types for
    /// indexed member access (e.g., bolts[0].diameter → Type::length()).
    pub(crate) collection_sub_member_types: HashMap<String, HashMap<String, Type>>,
    /// Trait member index for qualified access validation: trait_name → set of member names.
    /// Populated from trait_registry in compile_entity.
    pub(crate) trait_members: HashMap<String, HashSet<String>>,
    /// Sub-component type map: sub_name → structure_name.
    /// Used to resolve instance qualified access (sub.(Trait::member)).
    pub(crate) sub_component_types: HashMap<String, String>,
    /// Trait bounds per structure name: structure_name → [trait_names].
    /// Used to verify a sub-component implements a given trait.
    pub(crate) sub_structure_traits: HashMap<String, Vec<String>>,
    /// Meta block entries for the current entity: key → value.
    pub(crate) meta_entries: HashMap<String, String>,
    /// Reference to the active unit registry.
    /// Set by compile_entity/compile_purpose. None for scopes that don't need it (functions, fields).
    pub(crate) unit_registry: Option<&'u UnitRegistry>,
    /// Whether this scope is an entity (structure/purpose) scope where `self` is valid.
    /// False for function scopes, where `self` must produce an "unresolved name" error.
    pub(crate) is_entity_scope: bool,
    /// Member types for non-collection sub-components: sub_name → { member_name → Type }.
    /// Used to resolve self.sub.member chains for non-collection subs.
    pub(crate) sub_member_types: HashMap<String, HashMap<String, Type>>,
}

impl<'u> CompilationScope<'u> {
    pub(crate) fn new(entity_name: &str) -> Self {
        CompilationScope {
            entity_name: entity_name.to_string(),
            names: HashMap::new(),
            port_names: HashSet::new(),
            collection_sub_names: HashSet::new(),
            collection_sub_member_types: HashMap::new(),
            trait_members: HashMap::new(),
            sub_component_types: HashMap::new(),
            sub_structure_traits: HashMap::new(),
            meta_entries: HashMap::new(),
            unit_registry: None,
            is_entity_scope: false,
            sub_member_types: HashMap::new(),
        }
    }

    /// Set the unit registry reference for this scope.
    pub(crate) fn set_unit_registry(&mut self, registry: &'u UnitRegistry) {
        self.unit_registry = Some(registry);
    }

    /// Look up a unit by name, applying factor and offset.
    /// Returns None if the unit is not in the registry.
    pub(crate) fn lookup_unit_in_registry(&self, value: f64, unit: &str) -> Option<(Value, DimensionVector)> {
        self.unit_registry?.lookup(unit).map(|entry| {
            let si_value = value * entry.factor + entry.offset.unwrap_or(0.0);
            (
                Value::Scalar {
                    si_value,
                    dimension: entry.dimension,
                },
                entry.dimension,
            )
        })
    }

    pub(crate) fn register(&mut self, name: &str, ty: Type) {
        let id = ValueCellId::new(&self.entity_name, name);
        self.names.insert(name.to_string(), (id, ty, None));
    }

    pub(crate) fn register_guarded(&mut self, name: &str, ty: Type, guard: ValueCellId) {
        let id = ValueCellId::new(&self.entity_name, name);
        self.names.insert(name.to_string(), (id, ty, Some(guard)));
    }

    pub(crate) fn resolve(&self, name: &str) -> Option<(&ValueCellId, &Type)> {
        self.names.get(name).map(|(id, ty, _)| (id, ty))
    }
}
