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

use super::*;

// ─────────────────────────── vocabulary ───────────────────────────

/// Whether a member is reachable through an *external* `obj.member` access.
///
/// This is the resolver's D6 verdict, not a verbatim mirror of
/// `types::Visibility`: a default (non-`pub`) `let` cell carries
/// `Visibility::Private` in the IR yet is never externally nameable, so it is
/// reported `Public` here rather than as a `priv` violation. Only member kinds
/// a user can actually write `priv` on map to `Private`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberVisibility {
    Public,
    Private,
}

/// A member that no `TopologyTemplate` container declares, but which the
/// language nonetheless makes accessible on a receiver — the PRD §7 **extension
/// point**.
///
/// `Count` is the only inhabitant today (`<collection_sub>.count`, typed
/// `Type::Int`, matching the existing `"count" => Type::Int` arms in `expr.rs`).
///
/// The placement-relations belt's `.world_frame` slots in here later. Adding a
/// variant is deliberately a *compile error* at the terminal classifier (which
/// matches exhaustively with no `_` arm), so a new synthesized member cannot
/// silently fall through. Its SEMANTICS — world posing, `RealizedBodySet` —
/// remain that belt's to define; this resolver only supplies the access
/// mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SynthesizedMember {
    Count,
}

/// Which `TopologyTemplate` container (or synthesized category) claims a member.
///
/// The five declared containers this resolver scans are `value_cells`,
/// `guarded_groups[].members`, `sub_components`, `ports` and `realizations`;
/// the first two both classify as [`MemberKind::ValueCell`] because a
/// `where`-guarded `param` is the same member kind as a top-level one — it is
/// only its *activation* that is guarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberKind {
    ValueCell(ValueCellKind),
    Sub,
    Port,
    Realization,
    Synthesized(SynthesizedMember),
}

/// One resolved `.member` step of a dotted path.
///
/// `object_type` is the type the hop was taken FROM (so a chain's hops record
/// the concrete type at each position, which is what C1-iii's attribution
/// needs); `object_structure` is its structure name when it has one.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Hop {
    pub(crate) object_type: Type,
    pub(crate) object_structure: Option<String>,
    pub(crate) member: String,
    pub(crate) member_kind: MemberKind,
    pub(crate) visibility: MemberVisibility,
}

/// The outcome of resolving a single hop.
///
/// `value_cell_type` is `Some` only for a member actually present in
/// `value_cells` / `guarded_groups[].members`. It exists so the rewired caller
/// in `expr.rs` can keep feeding its existing
/// `unwrap_or(Type::dimensionless_scalar())` fallback and lower byte-identically
/// — see the D9 note at that call site for why the read-back is deliberately
/// NOT widened to sub/port/realization members here.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HopResolution {
    pub(crate) hop: Hop,
    pub(crate) next_type: Type,
    pub(crate) value_cell_type: Option<Type>,
}

/// Why a path could not be resolved *and the caller should not treat that as an
/// error*.
///
/// Today's code carefully distinguishes "the member is genuinely absent"
/// (poison + `StructureMemberNotFound`) from "we cannot know" (the permissive
/// `dimensionless_scalar()` fallback, byte-for-byte `TraitObject` preservation,
/// the `struct_name != WILDCARD_STRUCTURE_KIND` belt-and-braces guards).
/// Keeping the second class a distinct variant is what makes "fall through to
/// the caller's existing path" an explicit, greppable decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndeterminateReason {
    /// Receiver is the wildcard `"Structure"` subject kind — no static template.
    WildcardStructure,
    /// Receiver is a `Type::TraitObject`; traits are not in `template_registry`,
    /// so the concrete runtime type is not statically known.
    TraitObjectReceiver,
    /// A concrete structure name with no entry in the template registry.
    TemplateNotInRegistry,
    /// The scope carries no template registry at all (function/field scopes).
    NoTemplateRegistry,
    /// The chain's innermost node is not a form this resolver types (a call, an
    /// `IndexAccess` root such as `subs[i].member`, an unbound identifier, …).
    UnresolvableRoot,
}

/// Why a member path did not resolve.
///
/// # D9 honesty
///
/// [`MemberPathError::Indeterminate`] is the explicit "defer to the caller's
/// existing path" signal, **not** a silent drop. Every *other* variant carries
/// enough data to render a real, attributed diagnostic — see
/// [`MemberPathError::to_diagnostic`]. So a caller can never lose an access:
/// it either resolves, defers deliberately, or reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MemberPathError {
    /// The expression is not a `.`-chain at all.
    NotAMemberPath,
    /// Resolution is not statically decidable here; fall through.
    Indeterminate(IndeterminateReason),
    /// Hop `hop_index` names a member `structure` does not declare (C1-iii: the
    /// attribution is the CONCRETE structure at that hop, not the chain root).
    UnknownMember {
        hop_index: usize,
        object_type: Type,
        structure: String,
        member: String,
    },
    /// Hop `hop_index` names a `priv` member of `structure` (C1-ii: enforced at
    /// every hop, not only the terminal).
    PrivateMember {
        hop_index: usize,
        structure: String,
        member: String,
    },
}

// ─────────────────────────── single-hop resolution ───────────────────────────

/// Resolve ONE `<object_type>.<member>` step — the single member-shape
/// authority (C1-iv).
///
/// Purely static (C1-i): takes no diagnostic sink and evaluates nothing.
///
/// `hop_index` on a returned error is always `0` here; the chain walk in
/// [`resolve_member_path`] overwrites it with the real position. A direct
/// single-hop caller must therefore not read it as meaningful.
pub(crate) fn resolve_hop(
    object_type: &Type,
    member: &str,
    scope: &CompilationScope,
) -> Result<HopResolution, MemberPathError> {
    let struct_name = match object_type {
        Type::StructureRef(name) => name.as_str(),
        // Traits are absent from `template_registry`, so their members are not
        // statically knowable — preserved byte-for-byte from today's behaviour.
        Type::TraitObject(_) => {
            return Err(MemberPathError::Indeterminate(
                IndeterminateReason::TraitObjectReceiver,
            ));
        }
        _ => {
            return Err(MemberPathError::Indeterminate(
                IndeterminateReason::UnresolvableRoot,
            ));
        }
    };
    if struct_name == crate::expr::WILDCARD_STRUCTURE_KIND {
        return Err(MemberPathError::Indeterminate(
            IndeterminateReason::WildcardStructure,
        ));
    }
    let Some(registry) = scope.template_registry else {
        return Err(MemberPathError::Indeterminate(
            IndeterminateReason::NoTemplateRegistry,
        ));
    };
    let Some(template) = registry.get(struct_name) else {
        return Err(MemberPathError::Indeterminate(
            IndeterminateReason::TemplateNotInRegistry,
        ));
    };

    // Container scan. Ordered so it reproduces the existing five-container
    // union exactly: value_cells → guarded_groups[].members → sub_components →
    // ports → realizations.
    let value_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .or_else(|| {
            template
                .guarded_groups
                .iter()
                .flat_map(|g| g.members.iter())
                .find(|vc| vc.id.member == member)
        });
    if let Some(vc) = value_cell {
        return Ok(HopResolution {
            hop: Hop {
                object_type: object_type.clone(),
                object_structure: Some(struct_name.to_string()),
                member: member.to_string(),
                member_kind: MemberKind::ValueCell(vc.kind),
                visibility: MemberVisibility::Public,
            },
            next_type: vc.cell_type.clone(),
            value_cell_type: Some(vc.cell_type.clone()),
        });
    }

    Err(MemberPathError::UnknownMember {
        hop_index: 0,
        object_type: object_type.clone(),
        structure: struct_name.to_string(),
        member: member.to_string(),
    })
}

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

    fn sub_component(name: &str, structure_name: &str, visibility: Visibility) -> SubComponentDecl {
        SubComponentDecl {
            name: name.to_string(),
            structure_name: structure_name.to_string(),
            visibility,
            args: Vec::new(),
            type_args: Vec::new(),
            is_collection: false,
            count_cell: None,
            guard_state: GuardState::None,
            pose: None,
            auto_pose: None,
            is_aux: false,
            keyed_members: Vec::new(),
            keyed_member_overrides: Vec::new(),
            span: SourceSpan::new(0, 0),
            content_hash: ContentHash::of_str(name),
        }
    }

    fn port(name: &str, is_priv: bool) -> CompiledPort {
        CompiledPort {
            name: name.to_string(),
            direction: reify_core::PortDirection::Bidi,
            type_name: "SomeTrait".to_string(),
            members: Vec::new(),
            constraints: Vec::new(),
            frame_expr: None,
            is_priv,
        }
    }

    fn realization(entity: &str, index: u32, name: &str) -> RealizationDecl {
        RealizationDecl {
            id: RealizationNodeId::new(entity, index),
            name: Some(name.to_string()),
            is_aux: false,
            operations: Vec::new(),
            span: SourceSpan::new(0, 0),
        }
    }

    /// `structure def C { sub child = Child()  port p : SomeTrait { }  let body = <geom> }`
    fn template_c() -> TopologyTemplate {
        TopologyTemplate {
            sub_components: vec![sub_component("child", "Child", Visibility::Public)],
            ports: vec![port("p", false)],
            realizations: vec![realization("C", 0, "body")],
            ..empty_template("C")
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

    // ── step-3: the remaining member containers ───────────────────────

    #[test]
    fn resolve_hop_classifies_a_sub_component_and_types_the_next_hop_from_it() {
        let templates = [template_c()];
        with_scope("Test", &templates, |scope| {
            let r = resolve_hop(&Type::StructureRef("C".to_string()), "child", scope)
                .expect("'child' is a declared sub of C");
            assert_eq!(r.hop.member_kind, MemberKind::Sub);
            // `SubComponentDecl.structure_name` is what makes the NEXT hop
            // resolvable against a concrete template.
            assert_eq!(r.next_type, Type::StructureRef("Child".to_string()));
            // Not a value cell → the caller's `value_cells`-only read-back must
            // still fall back to its permissive dimensionless type (D9).
            assert_eq!(r.value_cell_type, None);
        });
    }

    #[test]
    fn resolve_hop_classifies_a_port() {
        let templates = [template_c()];
        with_scope("Test", &templates, |scope| {
            let r = resolve_hop(&Type::StructureRef("C".to_string()), "p", scope)
                .expect("'p' is a declared port of C");
            assert_eq!(r.hop.member_kind, MemberKind::Port);
            assert_eq!(r.value_cell_type, None);
        });
    }

    #[test]
    fn resolve_hop_classifies_a_named_realization_as_geometry() {
        let templates = [template_c()];
        with_scope("Test", &templates, |scope| {
            let r = resolve_hop(&Type::StructureRef("C".to_string()), "body", scope)
                .expect("'body' is a named realization of C");
            assert_eq!(r.hop.member_kind, MemberKind::Realization);
            // The same type `expr.rs` stamps on a `CrossSubGeometryRef`.
            assert_eq!(r.next_type, Type::Geometry);
            assert_eq!(r.value_cell_type, None);
        });
    }

    /// C1-iii: an unknown member names the CONCRETE structure at that hop and
    /// carries that hop's concrete `Type` — never a generic geometry sentence.
    #[test]
    fn resolve_hop_reports_unknown_member_against_the_concrete_structure() {
        let templates = [template_c()];
        with_scope("Test", &templates, |scope| {
            let err = resolve_hop(&Type::StructureRef("C".to_string()), "nope", scope)
                .expect_err("'nope' is declared by no container of C");
            assert_eq!(
                err,
                MemberPathError::UnknownMember {
                    hop_index: 0,
                    object_type: Type::StructureRef("C".to_string()),
                    structure: "C".to_string(),
                    member: "nope".to_string(),
                }
            );
        });
    }

    /// The containers must partition the name space: a name present only in
    /// `sub_components` must never report `ValueCell`, and vice versa. This is
    /// the property that makes `MemberKind` a faithful classification rather
    /// than a first-match-wins guess.
    #[test]
    fn the_member_containers_are_disjoint_in_the_classification() {
        let templates = [TopologyTemplate {
            value_cells: vec![value_cell(
                "D",
                "vc",
                ValueCellKind::Param,
                Visibility::Public,
                Type::length(),
            )],
            sub_components: vec![sub_component("sc", "Child", Visibility::Public)],
            ports: vec![port("pt", false)],
            realizations: vec![realization("D", 0, "rz")],
            ..empty_template("D")
        }];
        with_scope("Test", &templates, |scope| {
            let obj = Type::StructureRef("D".to_string());
            let kind = |m: &str| resolve_hop(&obj, m, scope).map(|r| r.hop.member_kind);
            assert_eq!(
                kind("vc"),
                Ok(MemberKind::ValueCell(ValueCellKind::Param)),
                "a value-cell name must not be claimed by another container"
            );
            assert_eq!(kind("sc"), Ok(MemberKind::Sub));
            assert_eq!(kind("pt"), Ok(MemberKind::Port));
            assert_eq!(kind("rz"), Ok(MemberKind::Realization));
        });
    }
}
