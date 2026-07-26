//! The single compile-time **member-shape authority** for dotted member paths.
//!
//! Introduced by task 5424 (PRD `docs/prds/v0_6/uniform-member-access.md` task
//! α, §5 contract C1). One resolver answers, for any `<receiver>.<member>`
//! chain of arbitrary depth: *is this a member?*, *of which concrete structure?*,
//! *what kind of member?*, *is it visible from here?*, and *what is its static
//! type?*
//!
//! # Contract
//!
//! * **C1-i — purely static.** Resolution performs no evaluation and emits no
//!   diagnostics. Neither entry point takes a `&mut Vec<Diagnostic>`, so this
//!   is enforced by the signature rather than by convention: callers render
//!   diagnostics themselves from the returned typed error via
//!   `MemberPathError::to_diagnostic(span)`. This lets speculative consumers
//!   (PRD task β's type-driven geometry acceptance) ask "is this a
//!   Geometry-typed member path?" without side effects.
//! * **C1-ii — `priv` at every hop.** Visibility is enforced at each hop of the
//!   chain, not only at the terminal.
//! * **C1-iii — concrete attribution.** An unknown member at hop *k* names the
//!   concrete structure at hop *k*, never a generic sentence.
//! * **C1-iv — no lockstep duplication (INV-5).** No other site may re-match
//!   member AST shapes to decide membership or visibility. Sites that need that
//!   verdict call in here.
//!
//! # Visibility of this module's items
//!
//! `pub(crate)` throughout. The known future consumers — PRD task β (geometry
//! position acceptance) and task η (sub-matcher retirement) — are same-crate
//! callers, so no `pub` export is warranted yet.

#[allow(unused_imports)]
use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// An otherwise-empty `TopologyTemplate` named `name`, for tests to fill in
    /// via functional-update syntax (`TopologyTemplate { value_cells: …,
    /// ..empty_template("S") }`).
    ///
    /// Deliberately hand-built rather than built with
    /// `reify_test_support::builders::topology::TopologyTemplateBuilder`: that
    /// helper links against reify-compiler through reify-test-support's own
    /// dependency edge — a *different* compilation of this crate than the one
    /// these unit tests run inside — so its `TopologyTemplate` is an E0308
    /// mismatch against this build's ("multiple different versions of crate
    /// `reify_compiler`"). Same rationale, spelled out in full, at
    /// `guards.rs`'s `guarded_param_default_tests::compile`. The builder also
    /// cannot express `let`-kind cells, per-member `priv`, or ports, all of
    /// which this resolver must classify.
    fn empty_template(name: &str) -> TopologyTemplate {
        TopologyTemplate {
            name: name.to_string(),
            doc: None,
            entity_kind: EntityKind::Structure,
            visibility: Visibility::Public,
            type_params: Vec::new(),
            trait_bounds: Vec::new(),
            value_cells: Vec::new(),
            constraints: Vec::new(),
            realizations: Vec::new(),
            sub_components: Vec::new(),
            relations: Vec::new(),
            ports: Vec::new(),
            connections: Vec::new(),
            guarded_groups: Vec::new(),
            structure_controlling: HashSet::new(),
            objective: None,
            meta: HashMap::new(),
            content_hash: ContentHash::of_str(name),
            is_recursive: false,
            annotations: Vec::new(),
            pragmas: Vec::new(),
            match_arm_groups: Vec::new(),
            forall_templates: Vec::new(),
            assoc_fns: Vec::new(),
            assoc_types: Vec::new(),
        }
    }

    /// `structure def S { param w : Length  let d : Length  where on { param g : Length } }`
    fn template_s() -> TopologyTemplate {
        TopologyTemplate {
            value_cells: vec![
                value_cell(
                    "S",
                    "w",
                    ValueCellKind::Param,
                    Visibility::Public,
                    Type::length(),
                ),
                value_cell(
                    "S",
                    "d",
                    ValueCellKind::Let,
                    Visibility::Private,
                    Type::length(),
                ),
            ],
            guarded_groups: vec![CompiledGuardedGroup {
                guard_expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
                guard_value_cell: ValueCellId::new("S", "__guard_on"),
                members: vec![value_cell(
                    "S",
                    "g",
                    ValueCellKind::Param,
                    Visibility::Public,
                    Type::length(),
                )],
                constraints: Vec::new(),
                else_members: Vec::new(),
                else_constraints: Vec::new(),
                parent_guard: None,
            }],
            ..empty_template("S")
        }
    }

    fn value_cell(
        entity: &str,
        member: &str,
        kind: ValueCellKind,
        visibility: Visibility,
        cell_type: Type,
    ) -> ValueCellDecl {
        ValueCellDecl {
            id: ValueCellId::new(entity, member),
            kind,
            visibility,
            is_aux: false,
            cell_type,
            default_expr: None,
            solver_hints: Vec::new(),
            span: SourceSpan::new(0, 0),
        }
    }

    /// Run `body` with a scope whose template registry holds `templates`.
    ///
    /// The registry borrows the templates, so it cannot escape the closure —
    /// hence the callback shape rather than a returned `CompilationScope`.
    fn with_scope<R>(
        entity: &str,
        templates: &[TopologyTemplate],
        body: impl FnOnce(&CompilationScope) -> R,
    ) -> R {
        let registry: HashMap<String, &TopologyTemplate> =
            templates.iter().map(|t| (t.name.clone(), t)).collect();
        let mut scope = CompilationScope::new(entity);
        scope.set_template_registry(&registry);
        body(&scope)
    }

    #[test]
    fn resolve_hop_classifies_a_top_level_param_value_cell() {
        let templates = [template_s()];
        with_scope("Test", &templates, |scope| {
            let r = resolve_hop(&Type::StructureRef("S".to_string()), "w", scope)
                .expect("'w' is a declared param of S");
            assert_eq!(r.hop.member, "w");
            assert_eq!(r.hop.object_structure.as_deref(), Some("S"));
            assert_eq!(
                r.hop.member_kind,
                MemberKind::ValueCell(ValueCellKind::Param)
            );
            assert_eq!(r.hop.visibility, MemberVisibility::Public);
            // The DECLARED type, not the permissive `dimensionless_scalar()`
            // fallback the caller applies when the type is unknown.
            assert_eq!(r.next_type, Type::length());
            assert_eq!(r.value_cell_type, Some(Type::length()));
        });
    }

    #[test]
    fn resolve_hop_classifies_a_let_value_cell_by_its_cell_kind() {
        let templates = [template_s()];
        with_scope("Test", &templates, |scope| {
            let r = resolve_hop(&Type::StructureRef("S".to_string()), "d", scope)
                .expect("'d' is a declared let cell of S");
            assert_eq!(r.hop.member_kind, MemberKind::ValueCell(ValueCellKind::Let));
            assert_eq!(r.next_type, Type::length());
            assert_eq!(r.value_cell_type, Some(Type::length()));
        });
    }

    /// The α RED. `template_has_member` (expr.rs) scans
    /// `value_cells ∪ ports ∪ sub_components` and does NOT scan
    /// `guarded_groups[].members`, while its sibling `template_member_is_priv`
    /// DOES — so a `param` declared inside a `where { }` block is denied
    /// membership while its `priv` twin is recognised. That divergence is the
    /// C1-iv lockstep hazard; this pins that the one authority scans guarded
    /// groups too.
    #[test]
    fn resolve_hop_scans_guarded_group_members_not_just_value_cells() {
        let templates = [template_s()];
        with_scope("Test", &templates, |scope| {
            let r = resolve_hop(&Type::StructureRef("S".to_string()), "g", scope)
                .expect("'g' is a param declared inside a `where` guarded group of S");
            assert_eq!(
                r.hop.member_kind,
                MemberKind::ValueCell(ValueCellKind::Param)
            );
            assert_eq!(r.hop.visibility, MemberVisibility::Public);
            assert_eq!(r.value_cell_type, Some(Type::length()));
        });
    }
}
