use super::*;
use std::collections::BTreeMap;

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
    /// Whether the entity declared a `meta { }` block (even if empty).
    pub(crate) has_meta_block: bool,
    /// Reference to the active unit registry.
    /// Set by compile_entity/compile_purpose. None for scopes that don't need it (functions, fields).
    pub(crate) unit_registry: Option<&'u UnitRegistry>,
    /// Reference to the template registry for purpose-body member validation.
    /// Set by compile_purpose so that concrete subject types can be validated
    /// against their declared value cells (task-2200). None for entity/function scopes.
    pub(crate) template_registry: Option<&'u HashMap<String, &'u TopologyTemplate>>,
    /// Whether this scope is an entity (structure or occurrence) scope where `self` is valid.
    /// False for function and purpose scopes, where `self` must produce an "unresolved name" error.
    pub(crate) is_entity_scope: bool,
    /// Member types for all sub-components: sub_name → { member_name → Type }.
    /// Populated for both collection and non-collection subs to resolve self.sub.member
    /// chains and instance qualified access.
    /// Inner map is BTreeMap so iteration order is lexicographic — this makes bare
    /// collection-sub identifier resolution (expr.rs: members.iter().next()) deterministic.
    pub(crate) sub_member_types: HashMap<String, BTreeMap<String, Type>>,
    /// Whether the current structure has at least one geometry-producing let binding
    /// (e.g., `let shape = box(...)`). Used to gate @face/@edge selectors at compile time.
    pub(crate) has_geometry: bool,
    /// Match-arm clusters keyed by their shared logical name (task 2372).
    ///
    /// Deliberately separate from `names` so that duplicate-name diagnostics
    /// (task 2375) cannot misfire on cluster members registered here.
    /// Populated by `register_match_arm_group`; queried by `resolve_match_arm_group`.
    ///
    /// `BTreeMap` (not `HashMap`) so that iteration over the collected
    /// `TopologyTemplate::match_arm_groups` is deterministic across compiles —
    /// snapshot tests, JSON serialization, and downstream union typing (task
    /// 2373) all depend on a stable order. Mirrors the precedent set by
    /// `meta_entries` hashing (entity.rs ~line 1656) which sorts keys for the
    /// same reason.
    pub(crate) match_arm_groups: BTreeMap<String, GuardedDeclGroup>,
}

impl<'u> CompilationScope<'u> {
    pub(crate) fn new(entity_name: &str) -> Self {
        CompilationScope {
            entity_name: entity_name.to_string(),
            names: HashMap::new(),
            port_names: HashSet::new(),
            collection_sub_names: HashSet::new(),
            trait_members: HashMap::new(),
            sub_component_types: HashMap::new(),
            sub_structure_traits: HashMap::new(),
            meta_entries: HashMap::new(),
            has_meta_block: false,
            unit_registry: None,
            template_registry: None,
            is_entity_scope: false,
            sub_member_types: HashMap::new(),
            has_geometry: false,
            match_arm_groups: BTreeMap::new(),
        }
    }

    /// Set the unit registry reference for this scope.
    pub(crate) fn set_unit_registry(&mut self, registry: &'u UnitRegistry) {
        self.unit_registry = Some(registry);
    }

    /// Set the template registry reference for purpose-body member validation (task-2200).
    /// Mirrors `set_unit_registry`; called once from `compile_purpose` after scope construction.
    pub(crate) fn set_template_registry(
        &mut self,
        registry: &'u HashMap<String, &'u TopologyTemplate>,
    ) {
        self.template_registry = Some(registry);
    }

    /// Look up a unit by name, applying factor and offset.
    /// Returns None if the unit is not in the registry.
    pub(crate) fn lookup_unit_in_registry(
        &self,
        value: f64,
        unit: &str,
    ) -> Option<(Value, DimensionVector)> {
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

    /// Insert `name → (id, ty, None)` only if `name` is not already registered.
    ///
    /// Returns `None` if the entry was inserted (vacant), or `Some(ty)` handing
    /// back the rejected type if the name was already registered (occupied) —
    /// callers can log the ignored type on conflict without paying for a clone on
    /// the hot insertion path. Unlike `register`, this method is guaranteed never
    /// to overwrite an existing registration.
    pub(crate) fn register_if_absent(&mut self, name: &str, ty: Type) -> Option<Type> {
        use std::collections::hash_map::Entry;
        match self.names.entry(name.to_string()) {
            Entry::Vacant(e) => {
                let id = ValueCellId::new(&self.entity_name, name);
                e.insert((id, ty, None));
                None
            }
            Entry::Occupied(_) => Some(ty),
        }
    }

    pub(crate) fn resolve(&self, name: &str) -> Option<(&ValueCellId, &Type)> {
        self.names.get(name).map(|(id, ty, _)| (id, ty))
    }

    /// Register a match-arm `GuardedDeclGroup` under its logical name.
    ///
    /// Stored in `match_arm_groups` — deliberately separate from `names` so that
    /// future duplicate-name diagnostics (task 2375) cannot misfire on cluster
    /// members. Overwriting a previous registration is harmless because the cluster
    /// is fully assembled before this call is made.
    pub(crate) fn register_match_arm_group(&mut self, name: &str, group: GuardedDeclGroup) {
        self.match_arm_groups.insert(name.to_string(), group);
    }

    /// Look up a match-arm cluster by its logical name.
    ///
    /// Returns `None` if no cluster has been registered under `name`. This never
    /// consults `self.names`, preserving the separation-from-`names` invariant.
    ///
    /// Currently consumed only by tests and future tasks (2373+) that union-type
    /// match-arm clusters; allowed here to avoid spurious dead-code lint.
    #[allow(dead_code)]
    pub(crate) fn resolve_match_arm_group(&self, name: &str) -> Option<&GuardedDeclGroup> {
        self.match_arm_groups.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── match-arm-group registration (task 2372, step-3) ─────────────────────

    fn make_test_group(name: &str) -> GuardedDeclGroup {
        use reify_types::Value;
        GuardedDeclGroup {
            name: name.to_string(),
            arms: vec![GuardedDeclArm {
                guard_expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
                guard_value_cell: ValueCellId::new("TestEntity", "__guard_0"),
                arm_type: Type::StructureRef("SomeHead".to_string()),
            }],
        }
    }

    #[test]
    fn register_match_arm_group_stores_in_dedicated_map() {
        let mut scope = CompilationScope::new("TestEntity");
        let group = make_test_group("head");
        scope.register_match_arm_group("head", group.clone());
        let retrieved = scope.resolve_match_arm_group("head");
        assert!(retrieved.is_some(), "group should be retrievable after registration");
        assert_eq!(retrieved.unwrap().name, "head");
        assert_eq!(retrieved.unwrap().arms.len(), 1);
    }

    #[test]
    fn register_match_arm_group_does_not_pollute_names_map() {
        let mut scope = CompilationScope::new("TestEntity");
        let group = make_test_group("head");
        scope.register_match_arm_group("head", group);
        assert!(
            scope.resolve("head").is_none(),
            "cluster registration must NOT insert into the regular names map"
        );
        assert!(
            scope.names.get("head").is_none(),
            "names map must remain empty after cluster registration"
        );
    }

    #[test]
    fn resolve_match_arm_group_returns_none_for_unknown_name() {
        let scope = CompilationScope::new("TestEntity");
        assert!(
            scope.resolve_match_arm_group("nonexistent").is_none(),
            "resolve_match_arm_group must return None for an unknown name"
        );
    }

    #[test]
    fn register_if_absent_does_not_overwrite() {
        let mut scope = CompilationScope::new("TestEntity");

        // Vacant case: register_if_absent should insert and return None.
        let inserted = scope.register_if_absent("y", Type::Bool);
        assert!(
            inserted.is_none(),
            "register_if_absent should return None for a fresh name"
        );
        let (_, ty, _) = scope.names["y"].clone();
        assert_eq!(ty, Type::Bool, "fresh insert should store the given type");

        // Occupied case: register_if_absent must NOT overwrite and must return Some(rejected_ty).
        scope.register("x", Type::Real);
        let rejected = scope.register_if_absent("x", Type::length());
        assert_eq!(
            rejected,
            Some(Type::length()),
            "register_if_absent should hand back the rejected type on conflict"
        );
        let (_, ty, _) = scope.names["x"].clone();
        assert_eq!(ty, Type::Real, "existing type must not be overwritten");
    }
}
