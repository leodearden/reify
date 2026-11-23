//! Per-scope ambient-default table, its innermost-wins resolver, and the
//! `Param` defaults it synthesizes to fill a conformer's unprovided members.
//!
//! Wires semantics onto the `default <TypeName> = <expr>` grammar produced by
//! task A (4496). An ambient default declares a value that fills any unprovided
//! structure member whose type matches the named type (DD1: keyed by type
//! *name*, machinery type-generic; the v1 surface only exercises `Material`).
//!
//! Two scopes carry ambient defaults:
//!   * **file level** — top-level `default` declarations, applied file-wide; and
//!   * **purpose level** — `default` declarations nested directly in a `purpose`
//!     body, applied to the structures nested in that same purpose (task 4639).
//!
//! Resolution is innermost-wins: a lookup for a given purpose consults that
//! purpose's own map first and falls back to the file level on a miss.
//!
//! A declaration whose value the declaration site rejected still occupies its
//! slot, as [`AmbientValue::Rejected`], so a consumer's member is filled rather
//! than reported missing; it never displaces an accepted value.

use std::collections::{HashMap, HashSet};

use reify_core::{ContentHash, SourceSpan, Type};

use crate::types::{DefaultKind, RequirementKind, TraitDefault, TraitRequirement};

/// The value side of an ambient-default declaration.
#[derive(Debug, Clone)]
pub(crate) enum AmbientValue {
    /// Passed the declaration-site check; recompiled at each injection site
    /// exactly like a trait-declared param default (DD2).
    Accepted(reify_ast::Expr),
    /// The declaration site rejected the value and already reported it. It
    /// still fills the member, with no default expression, so consumers neither
    /// report the member missing nor recompile the value.
    Rejected,
}

impl AmbientValue {
    /// The default expression an injection site compiles, if any.
    pub(crate) fn default_expr(&self) -> Option<&reify_ast::Expr> {
        match self {
            AmbientValue::Accepted(expr) => Some(expr),
            AmbientValue::Rejected => None,
        }
    }
}

/// A resolved ambient-default entry.
///
/// Carries the declaration's value, the type the default was declared against
/// (resolved via `resolve_type_expr_with_aliases`), and the declaration span
/// (used to anchor the declaration-site type-mismatch and duplicate
/// diagnostics — DD4/DD5).
#[derive(Debug, Clone)]
pub(crate) struct ResolvedAmbientDefault {
    /// The value to inject (e.g. the `Material(...)` constructor call), or the
    /// marker of a value the declaration site rejected.
    pub value: AmbientValue,
    /// The type the `default` was declared against (e.g. `StructureRef("Material")`).
    pub declared_type: Type,
    /// Span of the `default ...` declaration, for diagnostics.
    pub span: SourceSpan,
}

/// Per-scope ambient-default table keyed by type name (DD1).
///
/// Holds a file-level map plus a per-purpose map. [`AmbientDefaults::resolve`]
/// walks innermost (purpose) → outermost (file).
#[derive(Debug, Clone, Default)]
pub(crate) struct AmbientDefaults {
    /// File-level (top-level) defaults: type name → entry.
    file_level: HashMap<String, ResolvedAmbientDefault>,
    /// Purpose-level defaults: purpose name → (type name → entry).
    purpose_level: HashMap<String, HashMap<String, ResolvedAmbientDefault>>,
}

impl AmbientDefaults {
    /// Resolve the ambient default for `type_name` as seen from the given
    /// scope, innermost-wins: a `Some(purpose)` lookup consults that purpose's
    /// own map first and falls back to the file level on a miss; a `None`
    /// (file-scope) lookup consults only the file level.
    ///
    /// Private: [`AmbientDefaults::synthesize_param_defaults`] owns the DD3
    /// ladder this lookup sits at the bottom of.
    fn resolve(&self, type_name: &str, purpose: Option<&str>) -> Option<&ResolvedAmbientDefault> {
        // Innermost-wins: from inside a purpose, consult that purpose's own map
        // first; on a miss (or at file scope) fall back to the file level.
        if let Some(purpose_name) = purpose
            && let Some(entry) = self
                .purpose_level
                .get(purpose_name)
                .and_then(|by_type| by_type.get(type_name))
        {
            return Some(entry);
        }
        self.file_level.get(type_name)
    }

    /// Insert a file-level (top-level) default for `type_name`.
    pub(crate) fn insert_file_level(&mut self, type_name: String, entry: ResolvedAmbientDefault) {
        insert_by_precedence(&mut self.file_level, type_name, entry);
    }

    /// Insert a purpose-level default for `type_name` under `purpose`.
    pub(crate) fn insert_purpose_level(
        &mut self,
        purpose: String,
        type_name: String,
        entry: ResolvedAmbientDefault,
    ) {
        insert_by_precedence(
            self.purpose_level.entry(purpose).or_default(),
            type_name,
            entry,
        );
    }

    /// The `Param` defaults the table supplies for a conformer's unprovided
    /// `Param(StructureRef(T))` requirements, each carrying the ambient value
    /// for `T` as its default expression. The caller appends them to the
    /// trait defaults, so an ambient default rides the trait-default rails
    /// exactly like a trait-declared param default (DD2).
    ///
    /// ## Type-keying (DD1)
    ///
    /// Any `Param(StructureRef(name))` requirement is a candidate, keyed by the
    /// referenced type *name*; there is no trait- or member-path coupling.
    ///
    /// ## Resolution ladder (DD3): explicit member > trait-declared default > ambient
    ///
    /// A requirement named in `structure_members` or already covered by an
    /// entry of `existing_defaults` gets nothing: the ambient default is never
    /// created when a higher-priority source exists.
    ///
    /// ## Scope (DD6)
    ///
    /// `purpose` selects the innermost scope for [`AmbientDefaults::resolve`]:
    /// `None` for a top-level structure, `Some(purpose)` for one nested in that
    /// purpose, whose own defaults then win over the file level's.
    ///
    /// A Rejected entry yields a `Param` default with the declared type and no
    /// default expression: the member is filled and the value never recompiled.
    pub(crate) fn synthesize_param_defaults(
        &self,
        requirements: &[TraitRequirement],
        existing_defaults: &[TraitDefault],
        structure_members: &HashMap<String, Type>,
        purpose: Option<&str>,
    ) -> Vec<TraitDefault> {
        let existing_default_names: HashSet<&str> = existing_defaults
            .iter()
            .filter_map(|d| d.name.as_deref())
            .collect();

        let mut synthesized = Vec::new();
        for req in requirements {
            let RequirementKind::Param(Type::StructureRef(type_name)) = &req.kind else {
                continue;
            };
            if structure_members.contains_key(&req.name)
                || existing_default_names.contains(req.name.as_str())
            {
                continue;
            }
            let Some(entry) = self.resolve(type_name, purpose) else {
                continue;
            };
            synthesized.push(TraitDefault {
                name: Some(req.name.clone()),
                kind: DefaultKind::Param {
                    cell_type: entry.declared_type.clone(),
                    default_decl: reify_ast::ParamDecl {
                        name: req.name.clone(),
                        doc: None,
                        is_priv: false,
                        type_expr: None,
                        default: entry.value.default_expr().cloned(),
                        where_clause: None,
                        annotations: vec![],
                        span: entry.span,
                        content_hash: ContentHash(0),
                    },
                },
                span: entry.span,
            });
        }
        synthesized
    }
}

/// A Rejected entry fills only a vacant slot; an Accepted entry takes the slot
/// from any occupant.
fn insert_by_precedence(
    slots: &mut HashMap<String, ResolvedAmbientDefault>,
    type_name: String,
    entry: ResolvedAmbientDefault,
) {
    if matches!(entry.value, AmbientValue::Rejected) && slots.contains_key(&type_name) {
        return;
    }
    slots.insert(type_name, entry);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `ResolvedAmbientDefault` whose `span.start` is `tag`, so the
    /// resolver tests can tell *which* entry (file vs. purpose) was returned
    /// purely by the returned span — the value/type are placeholders.
    fn tagged_entry(tag: u32) -> ResolvedAmbientDefault {
        ResolvedAmbientDefault {
            value: AmbientValue::Accepted(reify_ast::Expr {
                kind: reify_ast::ExprKind::Ident("placeholder".to_string()),
                span: SourceSpan::empty(tag),
            }),
            declared_type: Type::StructureRef("Material".to_string()),
            span: SourceSpan::empty(tag),
        }
    }

    /// Like [`tagged_entry`], but for a declaration whose value the
    /// declaration site rejected.
    fn tagged_rejected(tag: u32) -> ResolvedAmbientDefault {
        ResolvedAmbientDefault {
            value: AmbientValue::Rejected,
            declared_type: Type::StructureRef("Material".to_string()),
            span: SourceSpan::empty(tag),
        }
    }

    /// The tag of the entry `resolve` returns, and whether it was rejected.
    fn resolved_tag(table: &AmbientDefaults, purpose: Option<&str>) -> (u32, bool) {
        let entry = table
            .resolve("Material", purpose)
            .expect("Material should resolve");
        (
            entry.span.start,
            matches!(entry.value, AmbientValue::Rejected),
        )
    }

    #[test]
    fn rejected_entry_fills_vacant_file_slot() {
        let mut table = AmbientDefaults::default();
        table.insert_file_level("Material".to_string(), tagged_rejected(10));

        assert_eq!(resolved_tag(&table, None), (10, true));
    }

    #[test]
    fn accepted_entry_displaces_rejected_file_occupant() {
        let mut table = AmbientDefaults::default();
        table.insert_file_level("Material".to_string(), tagged_rejected(10));
        table.insert_file_level("Material".to_string(), tagged_entry(20));

        assert_eq!(resolved_tag(&table, None), (20, false));
    }

    #[test]
    fn rejected_entry_never_displaces_accepted_file_occupant() {
        let mut table = AmbientDefaults::default();
        table.insert_file_level("Material".to_string(), tagged_entry(10));
        table.insert_file_level("Material".to_string(), tagged_rejected(20));

        assert_eq!(resolved_tag(&table, None), (10, false));
    }

    #[test]
    fn first_rejected_entry_keeps_file_slot() {
        let mut table = AmbientDefaults::default();
        table.insert_file_level("Material".to_string(), tagged_rejected(10));
        table.insert_file_level("Material".to_string(), tagged_rejected(20));

        assert_eq!(resolved_tag(&table, None), (10, true));
    }

    #[test]
    fn purpose_slot_follows_same_precedence() {
        let mut table = AmbientDefaults::default();
        table.insert_purpose_level("P".to_string(), "Material".to_string(), tagged_rejected(10));
        table.insert_purpose_level("P".to_string(), "Material".to_string(), tagged_entry(20));
        table.insert_purpose_level("Q".to_string(), "Material".to_string(), tagged_entry(30));
        table.insert_purpose_level("Q".to_string(), "Material".to_string(), tagged_rejected(40));

        assert_eq!(resolved_tag(&table, Some("P")), (20, false));
        assert_eq!(resolved_tag(&table, Some("Q")), (30, false));
    }

    #[test]
    fn rejected_purpose_entry_shadows_file_entry() {
        let mut table = AmbientDefaults::default();
        table.insert_file_level("Material".to_string(), tagged_entry(10));
        table.insert_purpose_level("P".to_string(), "Material".to_string(), tagged_rejected(20));

        assert_eq!(
            resolved_tag(&table, Some("P")),
            (20, true),
            "innermost-wins applies to a rejected purpose entry too"
        );
    }

    #[test]
    fn file_level_entry_resolves_at_file_scope() {
        let mut table = AmbientDefaults::default();
        table.insert_file_level("Material".to_string(), tagged_entry(10));

        let resolved = table
            .resolve("Material", None)
            .expect("file-level Material should resolve at file scope");
        assert_eq!(resolved.span.start, 10);
    }

    #[test]
    fn purpose_entry_wins_over_file_entry_innermost() {
        let mut table = AmbientDefaults::default();
        table.insert_file_level("Material".to_string(), tagged_entry(10));
        table.insert_purpose_level("P".to_string(), "Material".to_string(), tagged_entry(20));

        // Innermost-wins: from inside purpose "P", the purpose entry shadows
        // the file entry.
        let resolved = table
            .resolve("Material", Some("P"))
            .expect("Material should resolve from inside purpose P");
        assert_eq!(
            resolved.span.start, 20,
            "expected the PURPOSE entry (tag 20), not the file entry (tag 10)"
        );
    }

    #[test]
    fn file_scope_ignores_purpose_entry() {
        let mut table = AmbientDefaults::default();
        table.insert_file_level("Material".to_string(), tagged_entry(10));
        table.insert_purpose_level("P".to_string(), "Material".to_string(), tagged_entry(20));

        // At file scope (purpose = None) the purpose-level entry is invisible.
        let resolved = table
            .resolve("Material", None)
            .expect("Material should resolve at file scope");
        assert_eq!(
            resolved.span.start, 10,
            "file scope must return the file entry (tag 10), not purpose P's (tag 20)"
        );
    }

    #[test]
    fn unknown_type_resolves_to_none() {
        let mut table = AmbientDefaults::default();
        table.insert_file_level("Material".to_string(), tagged_entry(10));

        assert!(table.resolve("Unknown", None).is_none());
    }

    #[test]
    fn purpose_without_own_entry_falls_back_to_file_level() {
        let mut table = AmbientDefaults::default();
        table.insert_file_level("Material".to_string(), tagged_entry(10));
        table.insert_purpose_level("P".to_string(), "Material".to_string(), tagged_entry(20));

        // Purpose "Q" has no own Material entry → fall back to the file level.
        let resolved = table
            .resolve("Material", Some("Q"))
            .expect("Material should fall back to the file level from purpose Q");
        assert_eq!(
            resolved.span.start, 10,
            "purpose Q has no own entry; must fall back to the file entry (tag 10)"
        );
    }

    mod synthesize_param_defaults {
        use super::*;

        fn ident_expr(marker: &str) -> reify_ast::Expr {
            reify_ast::Expr {
                kind: reify_ast::ExprKind::Ident(marker.to_string()),
                span: SourceSpan::empty(0),
            }
        }

        fn material_entry(value: AmbientValue) -> ResolvedAmbientDefault {
            ResolvedAmbientDefault {
                value,
                declared_type: Type::StructureRef("Material".to_string()),
                span: SourceSpan::empty(0),
            }
        }

        /// File-level table whose single `Material` entry carries an
        /// `Ident(marker)` value, so a synthesized default's value can be
        /// identified (see [`marker_of`]).
        fn material_ambient_table(marker: &str) -> AmbientDefaults {
            material_table_with(AmbientValue::Accepted(ident_expr(marker)))
        }

        /// File-level table whose single `Material` entry was rejected at its
        /// declaration site.
        fn rejected_material_table() -> AmbientDefaults {
            material_table_with(AmbientValue::Rejected)
        }

        fn material_table_with(value: AmbientValue) -> AmbientDefaults {
            let mut table = AmbientDefaults::default();
            table.insert_file_level("Material".to_string(), material_entry(value));
            table
        }

        /// A `param <name> : Material` requirement.
        fn material_param_req(name: &str) -> TraitRequirement {
            TraitRequirement {
                name: name.to_string(),
                kind: RequirementKind::Param(Type::StructureRef("Material".to_string())),
                span: SourceSpan::empty(0),
            }
        }

        /// A trait-declared `Param` default named `name` carrying an
        /// `Ident(marker)` value.
        fn trait_param_default(name: &str, marker: &str) -> TraitDefault {
            TraitDefault {
                name: Some(name.to_string()),
                kind: DefaultKind::Param {
                    cell_type: Type::StructureRef("Material".to_string()),
                    default_decl: reify_ast::ParamDecl {
                        name: name.to_string(),
                        doc: None,
                        is_priv: false,
                        type_expr: None,
                        default: Some(ident_expr(marker)),
                        where_clause: None,
                        annotations: vec![],
                        span: SourceSpan::empty(0),
                        content_hash: ContentHash(0),
                    },
                },
                span: SourceSpan::empty(0),
            }
        }

        /// The `Ident` marker carried by the `Param` default named `name`, or
        /// `None` if there is no such default or it carries no `Ident` value.
        fn marker_of<'a>(defaults: &'a [TraitDefault], name: &str) -> Option<&'a str> {
            defaults.iter().find_map(|d| {
                if d.name.as_deref() != Some(name) {
                    return None;
                }
                if let DefaultKind::Param { default_decl, .. } = &d.kind
                    && let Some(expr) = &default_decl.default
                    && let reify_ast::ExprKind::Ident(marker) = &expr.kind
                {
                    Some(marker.as_str())
                } else {
                    None
                }
            })
        }

        fn no_members() -> HashMap<String, Type> {
            HashMap::new()
        }

        #[test]
        fn fills_unfilled_material_param_with_ambient_value() {
            let synthesized = material_ambient_table("ambient_steel").synthesize_param_defaults(
                &[material_param_req("material")],
                &[],
                &no_members(),
                None,
            );

            let [default] = synthesized.as_slice() else {
                panic!("expected exactly one synthesized default; got {synthesized:?}");
            };
            assert_eq!(default.name.as_deref(), Some("material"));
            assert_eq!(marker_of(&synthesized, "material"), Some("ambient_steel"));
            assert!(
                matches!(
                    &default.kind,
                    DefaultKind::Param { cell_type, .. }
                        if cell_type == &Type::StructureRef("Material".to_string())
                ),
                "cell_type should be StructureRef(\"Material\"); got {:?}",
                default.kind
            );
        }

        /// DD3: an explicit structure member wins over an ambient default.
        #[test]
        fn explicit_structure_member_suppresses_synthesis() {
            let members = HashMap::from([(
                "material".to_string(),
                Type::StructureRef("Material".to_string()),
            )]);

            let synthesized = material_ambient_table("ambient_steel").synthesize_param_defaults(
                &[material_param_req("material")],
                &[],
                &members,
                None,
            );

            assert!(synthesized.is_empty(), "got {synthesized:?}");
        }

        /// DD3: a trait-declared default wins over an ambient default.
        #[test]
        fn existing_trait_default_suppresses_synthesis() {
            let synthesized = material_ambient_table("ambient_steel").synthesize_param_defaults(
                &[material_param_req("material")],
                &[trait_param_default("material", "trait_default")],
                &no_members(),
                None,
            );

            assert!(synthesized.is_empty(), "got {synthesized:?}");
        }

        #[test]
        fn fills_every_unfilled_material_param() {
            let synthesized = material_ambient_table("ambient_steel").synthesize_param_defaults(
                &[
                    material_param_req("material"),
                    material_param_req("secondary_material"),
                ],
                &[],
                &no_members(),
                None,
            );

            assert_eq!(synthesized.len(), 2, "got {synthesized:?}");
            assert_eq!(marker_of(&synthesized, "material"), Some("ambient_steel"));
            assert_eq!(
                marker_of(&synthesized, "secondary_material"),
                Some("ambient_steel")
            );
        }

        /// A rejected entry still fills the member, with the declared type and
        /// no default expression, so the value is never recompiled.
        #[test]
        fn rejected_entry_fills_param_with_declared_type_and_no_default() {
            let synthesized = rejected_material_table().synthesize_param_defaults(
                &[material_param_req("material")],
                &[],
                &no_members(),
                None,
            );

            let [default] = synthesized.as_slice() else {
                panic!("expected exactly one synthesized default; got {synthesized:?}");
            };
            assert_eq!(default.name.as_deref(), Some("material"));
            let DefaultKind::Param {
                cell_type,
                default_decl,
            } = &default.kind
            else {
                panic!("expected a Param default; got {:?}", default.kind);
            };
            assert_eq!(cell_type, &Type::StructureRef("Material".to_string()));
            assert!(
                default_decl.default.is_none(),
                "a rejected entry must carry no default expr; got {:?}",
                default_decl.default
            );
        }

        #[test]
        fn param_with_no_ambient_entry_is_left_unfilled() {
            let widget_req = TraitRequirement {
                name: "widget".to_string(),
                kind: RequirementKind::Param(Type::StructureRef("Widget".to_string())),
                span: SourceSpan::empty(0),
            };

            let synthesized = material_ambient_table("ambient_steel").synthesize_param_defaults(
                &[widget_req],
                &[],
                &no_members(),
                None,
            );

            assert!(synthesized.is_empty(), "got {synthesized:?}");
        }

        /// DD1: only a `Param` requirement naming a structure type is a
        /// candidate — not a `Let` of that type, nor a `Param` of another type.
        #[test]
        fn only_structure_typed_param_requirements_are_candidates() {
            let material_let_req = TraitRequirement {
                name: "m".to_string(),
                kind: RequirementKind::Let(Type::StructureRef("Material".to_string())),
                span: SourceSpan::empty(0),
            };
            let length_param_req = TraitRequirement {
                name: "len".to_string(),
                kind: RequirementKind::Param(Type::length()),
                span: SourceSpan::empty(0),
            };

            let synthesized = material_ambient_table("ambient_steel").synthesize_param_defaults(
                &[material_let_req, length_param_req],
                &[],
                &no_members(),
                None,
            );

            assert!(synthesized.is_empty(), "got {synthesized:?}");
        }

        /// `purpose` reaches the innermost-wins resolver.
        #[test]
        fn purpose_selects_the_resolution_scope() {
            let mut table = material_ambient_table("file_steel");
            table.insert_purpose_level(
                "P".to_string(),
                "Material".to_string(),
                material_entry(AmbientValue::Accepted(ident_expr("purpose_steel"))),
            );
            let requirements = [material_param_req("material")];

            let in_purpose =
                table.synthesize_param_defaults(&requirements, &[], &no_members(), Some("P"));
            let at_file_scope =
                table.synthesize_param_defaults(&requirements, &[], &no_members(), None);

            assert_eq!(marker_of(&in_purpose, "material"), Some("purpose_steel"));
            assert_eq!(marker_of(&at_file_scope, "material"), Some("file_steel"));
        }
    }
}
