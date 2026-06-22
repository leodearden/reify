use super::*;
use std::collections::{BTreeMap, BTreeSet};

// --- Compilation context ---

/// Per-arm child-template member map for a match-arm cluster (task 2373).
///
/// `(structure_name, member_types)` where `member_types[m] = T` for that
/// arm's child template. Tracked as a tuple so each entry preserves both
/// the arm's structure name (for diagnostics) and its member typing.
pub(crate) type ArmMemberMap = (String, BTreeMap<String, Type>);

/// External-scope cluster entry (task 2373): the cluster definition plus
/// its per-arm member maps in arm-order. Used to typecheck
/// `<sub>.<cluster>.<inner>` access from outside a sub's structure.
pub(crate) type SubClusterEntry = (GuardedDeclGroup, Vec<ArmMemberMap>);

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
    /// Names of this structure's own geometry lets / Solid-params that lower to
    /// `RealizationDecl`s (and therefore have a `named_steps[name]` entry at eval
    /// time).
    ///
    /// Only top-level geometry lets and top-level Solid params are included — guarded-group
    /// lets and guarded Solid params are conservatively excluded because they do NOT
    /// emit `RealizationDecl`s (entity.rs `compile_entity` realization-emission loop
    /// skips guarded members) and would therefore have no `named_steps` entry at eval
    /// time. Emitting `GeomRef::Sub(name)` for such a name would produce an
    /// unresolvable sub-ref at runtime.
    ///
    /// Populated once in `compile_entity` before the geometry pass (while `scope`
    /// is still `let mut`); queried by the generic geometry-arg resolution loop in
    /// `geometry.rs` for the sibling-let pre-check. Mirrors the precedent set by
    /// `collection_sub_names` / `purpose_param_names` — a dedicated typed set for a
    /// category-specific lookup rather than overloading `names`.
    pub(crate) geometry_realization_names: HashSet<String>,
    /// Trait member index for qualified access validation: trait_name → set of member names.
    /// Populated from trait_registry in compile_entity.
    pub(crate) trait_members: HashMap<String, HashSet<String>>,
    /// Type-param → bound trait names for the entity being compiled.
    ///
    /// Populated in `compile_entity` from `structure.type_params`
    /// (`TypeParamDecl { name, bounds }`). Empty for entities that declare
    /// no type parameters.
    ///
    /// Used by the `Type::TypeParam` member-access branch in `expr.rs`
    /// (task 4596) to look up the bound trait(s) for a type-param receiver
    /// so the accessed member's static type can be resolved from the trait
    /// contract (not the candidate — the candidate is unknown at L2).
    pub(crate) type_param_bounds: HashMap<String, Vec<String>>,
    /// Trait → member name → declared type, for all traits in scope.
    ///
    /// Populated in `compile_entity` from the `trait_registry`, alongside the
    /// existing `trait_members` name-set. Carries only `RequirementKind::Param`
    /// and `RequirementKind::Let` entries (the value-bearing requirements);
    /// `Sub`, `Fn`, and `AssocType` requirements are excluded because they do
    /// not produce a scalar/dimensional value that constraint expressions can
    /// reference.
    ///
    /// Used by the `Type::TypeParam` member-access branch in `expr.rs`
    /// (task 4596) to resolve the member type from the bound trait's contract
    /// (the only statically-available type source when the receiver is still
    /// an un-resolved TypeParam).
    pub(crate) trait_member_types: HashMap<String, HashMap<String, Type>>,
    /// Trait → instance-assoc-fn name → declared return type (task 3941 ζ).
    ///
    /// Populated in `compile_entity` from the `trait_registry`, alongside
    /// `trait_members`. Carries only **instance** associated functions (those
    /// with a `self` receiver) — the dispatch targets of `obj.(Trait::fn)(…)`.
    /// Static (no-`self`) assoc fns are excluded (they dispatch via the
    /// `TraitStaticCall` path, whose fns are pre-registered in `phase_traits`).
    ///
    /// Used by the `ExprKind::TraitMethodCall` dispatch arm in `expr.rs` to type
    /// the lowered `UserFunctionCall` from the trait's declared contract. The
    /// per-conformer `CompiledFunction` is not yet in `ctx.functions` at
    /// entity-body-compile time (it is injected by the post-entity registration
    /// pass), so the call site reads the return type from the trait contract here
    /// rather than via compile-time overload resolution (PRD §4.4; design
    /// decision: resolve call-site result type from the trait, registration-order
    /// independent).
    pub(crate) trait_assoc_fn_return_types: HashMap<String, HashMap<String, Type>>,
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
    /// Named realizations (geometry-producing members lowered as `RealizationDecl`s)
    /// per sub-component: sub_name → set of realization names.
    ///
    /// Geometry-typed params (`param x : Solid = <geom>`) and geometry lets
    /// (`let x = box(...)`) are both lowered as `RealizationDecl`s and therefore
    /// never appear in `value_cells` — so `sub_member_types[sub][member]` returns
    /// `None` for them, triggering the "unknown member" fallback.
    ///
    /// This side-map lets `expr.rs` distinguish "the member genuinely does not
    /// exist" from "the member is a realization on the child template, but
    /// cross-sub geometry access is not yet supported in v0.1".  When the lookup
    /// in `sub_member_types` misses but `sub_realization_names[sub]` contains the
    /// member name, `expr.rs` emits the specific cross-sub geometry diagnostic via
    /// `make_poison_literal` rather than the generic "unknown member" fallback.
    ///
    /// `BTreeSet` (not `HashSet`) for deterministic iteration, mirroring the
    /// precedent of `sub_member_types`' inner `BTreeMap`.
    pub(crate) sub_realization_names: HashMap<String, BTreeSet<String>>,
    /// Whether the current structure has at least one geometry-producing let binding
    /// (e.g., `let shape = box(...)`). Used to gate @face/@edge selectors at compile time.
    pub(crate) has_geometry: bool,
    /// Names of purpose params registered in this scope (task-2181 β, contract C1).
    ///
    /// Isolated from `names` so that future let-bindings in purpose bodies (task δ)
    /// cannot masquerade as purpose params in the `purpose_param_root` lookup.
    /// Mirrors the precedent of `port_names`/`collection_sub_names` (dedicated typed
    /// sets for category-specific lookups rather than overloading `names`).
    ///
    /// Only populated by `compile_purpose` via `register_purpose_param`; queried
    /// by `purpose_param_root` from the purpose-subject member-ref arm in `expr.rs`.
    pub(crate) purpose_param_names: HashSet<String>,
    /// Match-arm clusters keyed by their shared logical name (task 2372).
    ///
    /// Deliberately separate from `names` so that outside-match collision
    /// diagnostics (task 2375) cannot misfire on cluster members registered
    /// here — collisions are detected at pre-pass time and suppress the cluster
    /// before it ever reaches this map.
    /// Populated by `register_match_arm_group`; queried by `resolve_match_arm_group`.
    ///
    /// `BTreeMap` (not `HashMap`) so that iteration over the collected
    /// `TopologyTemplate::match_arm_groups` is deterministic across compiles —
    /// snapshot tests, JSON serialization, and downstream union typing (task
    /// 2373) all depend on a stable order. Mirrors the precedent set by
    /// `meta_entries` hashing (entity.rs ~line 1656) which sorts keys for the
    /// same reason.
    pub(crate) match_arm_groups: BTreeMap<String, GuardedDeclGroup>,
    /// Per-arm member-type maps for match-arm clusters (task 2373).
    ///
    /// Keyed by group logical name; the inner Vec is in arm-order (matches
    /// `match_arm_groups[name].arms`). Each entry is `(structure_name,
    /// member_types)` where `member_types[m] = T` for the arm's child
    /// template.
    ///
    /// Used by `expr.rs` to type-check nested `self.<cluster>.<inner>`
    /// access on a per-arm basis: the merged `sub_member_types[group]` map
    /// only retains the last arm's members (last write wins), so per-arm
    /// differentiation requires this parallel map.
    ///
    /// Populated in `compile_match_arm_decl_group` (entity.rs) on the success
    /// path, atomically with `register_match_arm_group` — the producer-side
    /// invariant `match_arm_groups.keys() == match_arm_group_arm_member_types.keys()`
    /// is enforced by an unconditional `assert!` in `compile_entity` (task 2872).
    pub(crate) match_arm_group_arm_member_types: HashMap<String, Vec<ArmMemberMap>>,
    /// External-scope match-arm clusters declared on each sub's child
    /// structure (task 2373).
    ///
    /// Keyed by sub name (e.g. `bolt`); the value is a list of
    /// `(GuardedDeclGroup, per_arm_member_maps)` for each cluster on that
    /// sub's child template. The per-arm member maps are
    /// `(structure_name, member_types)` in arm-order, mirroring
    /// `match_arm_group_arm_member_types`.
    ///
    /// Used by `expr.rs` to type-check `<sub>.<cluster>` (synthetic Union)
    /// and `<sub>.<cluster>.<inner>` (common-field lookup, missing-arm
    /// diagnostics) from outside the sub's structure. Populated in the
    /// entity.rs Sub pre-pass.
    pub(crate) sub_match_arm_groups: HashMap<String, Vec<SubClusterEntry>>,
}

impl<'u> CompilationScope<'u> {
    pub(crate) fn new(entity_name: &str) -> Self {
        CompilationScope {
            entity_name: entity_name.to_string(),
            names: HashMap::new(),
            port_names: HashSet::new(),
            collection_sub_names: HashSet::new(),
            geometry_realization_names: HashSet::new(),
            trait_members: HashMap::new(),
            type_param_bounds: HashMap::new(),
            trait_member_types: HashMap::new(),
            trait_assoc_fn_return_types: HashMap::new(),
            sub_component_types: HashMap::new(),
            sub_structure_traits: HashMap::new(),
            meta_entries: HashMap::new(),
            has_meta_block: false,
            unit_registry: None,
            template_registry: None,
            is_entity_scope: false,
            sub_member_types: HashMap::new(),
            sub_realization_names: HashMap::new(),
            has_geometry: false,
            match_arm_groups: BTreeMap::new(),
            match_arm_group_arm_member_types: HashMap::new(),
            sub_match_arm_groups: HashMap::new(),
            purpose_param_names: HashSet::new(),
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

    /// Register an identifier as a purpose param in this scope (task-2181 β, contract C1).
    ///
    /// Call once per param from `compile_purpose`'s param loop, right after
    /// `scope.register(&param.name, Type::StructureRef(...))`. This populates
    /// `purpose_param_names` so `purpose_param_root` can distinguish purpose params
    /// from future let-bindings (task δ) without changing the general `names` map.
    pub(crate) fn register_purpose_param(&mut self, name: &str) {
        self.purpose_param_names.insert(name.to_string());
    }

    /// Return the param name if `ident` is a registered purpose param, else `None` (task-2181 β).
    ///
    /// Used by `expr.rs`'s purpose-subject member-ref arm to obtain the `param_root` for the
    /// per-param entity stamp `format!("{}::{}", purpose_name, param_root)`. Returns `None`
    /// for any identifier that was not explicitly registered via `register_purpose_param`, which
    /// forward-guards against future let-bindings in purpose bodies (task δ) accidentally
    /// triggering the per-param stamp arm.
    pub(crate) fn purpose_param_root(&self, ident: &str) -> Option<&str> {
        self.purpose_param_names.get(ident).map(|s| s.as_str())
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
    /// outside-match collision diagnostics (task 2375) cannot misfire on cluster
    /// members; the cluster-isolation invariant prevents collisions with
    /// outside-of-match decls from surfacing as cluster-internal duplicates.
    ///
    /// Callers must never register the same `name` twice. In practice,
    /// `compile_match_arm_decl_group` (entity.rs) emits a
    /// "duplicate match-arm cluster name" diagnostic and returns early before
    /// reaching this call for a second cluster with the same name. The
    /// `debug_assert!` below is a defensive backstop: it will fire loudly in
    /// debug builds if a future refactor bypasses that early return.
    pub(crate) fn register_match_arm_group(&mut self, name: &str, group: GuardedDeclGroup) {
        debug_assert!(
            !self.match_arm_groups.contains_key(name),
            "duplicate cluster registration for '{}'",
            name
        );
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

    /// Returns `true` when `self.<sub_name>.<member>` should lower to a
    /// cross-sub geometry handle on the compile side.
    ///
    /// The predicate is `has_realization || forward_declared` where:
    /// * `has_realization` — `sub_realization_names[sub_name]` contains `member`.
    /// * `forward_declared` — the sub is registered in `sub_component_types`
    ///   but its `sub_member_types` entry has not yet been populated (parent
    ///   template compiled before child template; see the forward-declared
    ///   optimism rationale in `try_resolve_cross_sub_geometry_value_ref` and
    ///   `try_resolve_cross_sub_geom_ref`).
    ///
    /// Centralizes the predicate so the two compile-side call sites cannot
    /// drift apart:
    /// * `expr.rs::try_resolve_cross_sub_geometry_value_ref` — value-ref
    ///   `CompiledExpr` for `self.<sub>.<member>` in expression position.
    /// * `geometry.rs::try_resolve_cross_sub_geom_ref` — `GeomRef::Sub` for
    ///   the same access in geometry-arg position.
    ///
    /// The eval-side handshake (`engine_build.rs` populating
    /// `named_steps["<sub>.<member>"]`) and the unresolvable-handle diagnostic
    /// (`geometry_ops.rs::resolve_geom_ref`) are unchanged; only the source
    /// of the compile-side decision is consolidated.
    pub(crate) fn sub_member_is_cross_sub_geometry_or_forward_declared(
        &self,
        sub_name: &str,
        member: &str,
    ) -> bool {
        let has_realization = self
            .sub_realization_names
            .get(sub_name)
            .is_some_and(|s| s.contains(member));
        let forward_declared = self.sub_component_types.contains_key(sub_name)
            && !self.sub_member_types.contains_key(sub_name);
        has_realization || forward_declared
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── match-arm-group registration (task 2372, step-3) ─────────────────────

    fn make_test_group(name: &str) -> GuardedDeclGroup {
        use reify_ir::Value;
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
        assert!(
            retrieved.is_some(),
            "group should be retrievable after registration"
        );
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
            !scope.names.contains_key("head"),
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
        scope.register("x", Type::dimensionless_scalar());
        let rejected = scope.register_if_absent("x", Type::length());
        assert_eq!(
            rejected,
            Some(Type::length()),
            "register_if_absent should hand back the rejected type on conflict"
        );
        let (_, ty, _) = scope.names["x"].clone();
        assert_eq!(ty, Type::dimensionless_scalar(), "existing type must not be overwritten");
    }

    /// Task 2612 step-4: registering the same cluster name twice must panic in
    /// debug builds via `debug_assert!`.
    ///
    /// The production-path early-return diagnostic in `compile_match_arm_decl_group`
    /// (entity.rs) already prevents reaching `register_match_arm_group` a second
    /// time for the same name during normal compilation. This unit test exercises
    /// the API contract directly — it documents that `register_match_arm_group` is
    /// the defensive backstop if that early-return is ever refactored away.
    ///
    /// RED before the `debug_assert!` exists; GREEN after.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "duplicate cluster registration for")]
    fn register_match_arm_group_panics_on_duplicate_in_debug_builds() {
        let mut scope = CompilationScope::new("TestEntity");
        scope.register_match_arm_group("head", make_test_group("head"));
        // Second call with same name must panic via debug_assert!.
        scope.register_match_arm_group("head", make_test_group("head"));
    }

    // ── sub_member_is_cross_sub_geometry_or_forward_declared (task 3455) ─────

    /// Returns true when the member is a named realization on the sub (e.g. a
    /// geometry-producing let binding or geometry param on the child template).
    #[test]
    fn sub_member_is_cross_sub_geometry_or_forward_declared_returns_true_when_member_is_realization() {
        let mut scope = CompilationScope::new("TestEntity");
        scope
            .sub_component_types
            .insert("bolt".to_string(), "Bolt".to_string());
        scope
            .sub_member_types
            .insert("bolt".to_string(), BTreeMap::new());
        scope.sub_realization_names.insert(
            "bolt".to_string(),
            BTreeSet::from(["body".to_string()]),
        );
        assert!(
            scope.sub_member_is_cross_sub_geometry_or_forward_declared("bolt", "body"),
            "should return true when member is a realization"
        );
    }

    /// Returns true for a forward-declared sub: `sub_component_types` has the
    /// sub but `sub_member_types` does not (parent compiled before child).
    #[test]
    fn sub_member_is_cross_sub_geometry_or_forward_declared_returns_true_for_forward_declared_sub() {
        let mut scope = CompilationScope::new("TestEntity");
        scope
            .sub_component_types
            .insert("bolt".to_string(), "Bolt".to_string());
        // Intentionally leave sub_member_types and sub_realization_names empty for "bolt".
        assert!(
            scope.sub_member_is_cross_sub_geometry_or_forward_declared("bolt", "body"),
            "should return true for forward-declared sub regardless of member name"
        );
    }

    /// Returns true for the realization member and false for a scalar member when
    /// both coexist on a fully-resolved sub.  This is the common production state:
    /// a child template with mixed scalar params (`length`) and a geometry param
    /// (`body`).  Ensures `has_realization` correctly wins for `body` and that
    /// scalar members are NOT mistaken for geometry handles.
    #[test]
    fn sub_member_is_cross_sub_geometry_or_forward_declared_mixed_sub_realization_wins_scalars_do_not() {
        let mut scope = CompilationScope::new("TestEntity");
        scope
            .sub_component_types
            .insert("bolt".to_string(), "Bolt".to_string());
        scope.sub_member_types.insert(
            "bolt".to_string(),
            BTreeMap::from([("length".to_string(), Type::length())]),
        );
        scope.sub_realization_names.insert(
            "bolt".to_string(),
            BTreeSet::from(["body".to_string()]),
        );

        // has_realization wins: "body" is in sub_realization_names.
        assert!(
            scope.sub_member_is_cross_sub_geometry_or_forward_declared("bolt", "body"),
            "should return true for the geometry realization member"
        );
        // Scalar member — present in sub_member_types but NOT in sub_realization_names.
        assert!(
            !scope.sub_member_is_cross_sub_geometry_or_forward_declared("bolt", "length"),
            "should return false for a scalar member (not a geometry realization)"
        );
        // Completely unknown member — absent from both maps.
        assert!(
            !scope.sub_member_is_cross_sub_geometry_or_forward_declared("bolt", "missing"),
            "should return false for an unknown member on a fully-populated sub"
        );
    }

    /// Returns false when the member is neither a realization nor the sub is
    /// forward-declared (sub_member_types is populated, so it's fully resolved,
    /// but the queried member is not in sub_realization_names).
    #[test]
    fn sub_member_is_cross_sub_geometry_or_forward_declared_returns_false_when_member_is_unknown_on_populated_sub(
    ) {
        let mut scope = CompilationScope::new("TestEntity");
        scope
            .sub_component_types
            .insert("bolt".to_string(), "Bolt".to_string());
        scope.sub_member_types.insert(
            "bolt".to_string(),
            BTreeMap::from([("length".to_string(), Type::length())]),
        );
        scope.sub_realization_names.insert(
            "bolt".to_string(),
            BTreeSet::from(["body".to_string()]),
        );
        // "head" is neither a realization nor the sub is forward-declared.
        assert!(
            !scope.sub_member_is_cross_sub_geometry_or_forward_declared("bolt", "head"),
            "should return false when member is unknown on a fully-populated sub"
        );
    }

    /// Returns false when the sub is not registered at all (unknown sub name).
    #[test]
    fn sub_member_is_cross_sub_geometry_or_forward_declared_returns_false_for_unknown_sub() {
        let scope = CompilationScope::new("TestEntity");
        // All three maps are empty.
        assert!(
            !scope.sub_member_is_cross_sub_geometry_or_forward_declared("missing", "anything"),
            "should return false for a completely unknown sub"
        );
    }
}
