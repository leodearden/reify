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
    // Every `Ok` below carries a visibility verdict, and the shared tail turns a
    // `Private` verdict into `Err(PrivateMember)`. That is what makes the priv
    // check run over the FULL container union BEFORE the `UnknownMember`
    // fallback: a priv guarded-group member is denied as private rather than
    // shadowed by not-found (task #5171 — the ordering `expr.rs` calls
    // load-bearing).
    let hop = |member_kind, visibility| Hop {
        object_type: object_type.clone(),
        object_structure: Some(struct_name.to_string()),
        member: member.to_string(),
        member_kind,
        visibility,
    };

    let resolution = if let Some(vc) = value_cell {
        Some(HopResolution {
            hop: hop(MemberKind::ValueCell(vc.kind), value_cell_visibility(vc)),
            next_type: vc.cell_type.clone(),
            value_cell_type: Some(vc.cell_type.clone()),
        })
    } else if let Some(sc) = template.sub_components.iter().find(|sc| sc.name == member) {
        Some(HopResolution {
            hop: hop(MemberKind::Sub, visibility_of(sc.visibility)),
            // `structure_name` is what makes the NEXT hop resolvable against a
            // concrete template — the whole point of typed chain traversal.
            next_type: Type::StructureRef(sc.structure_name.clone()),
            value_cell_type: None,
        })
    } else if let Some(p) = template.ports.iter().find(|p| p.name == member) {
        Some(HopResolution {
            hop: hop(
                MemberKind::Port,
                if p.is_priv {
                    MemberVisibility::Private
                } else {
                    MemberVisibility::Public
                },
            ),
            // A port is not a structure instance; there is no further typed hop
            // to take from it. Widening this is PRD task η's (its two-level
            // `<sub>.<port>.<member>` matcher owns that shape today).
            next_type: Type::dimensionless_scalar(),
            value_cell_type: None,
        })
    } else if template
        .realizations
        .iter()
        .any(|r| r.name.as_deref() == Some(member))
    {
        Some(HopResolution {
            // Realizations carry no `priv` axis of their own.
            hop: hop(MemberKind::Realization, MemberVisibility::Public),
            // The same type `expr.rs` stamps on a `CrossSubGeometryRef`.
            next_type: Type::Geometry,
            value_cell_type: None,
        })
    } else {
        None
    };

    match resolution {
        Some(r) if r.hop.visibility == MemberVisibility::Private => {
            Err(MemberPathError::PrivateMember {
                hop_index: 0,
                structure: struct_name.to_string(),
                member: member.to_string(),
            })
        }
        Some(r) => Ok(r),
        None => Err(MemberPathError::UnknownMember {
            hop_index: 0,
            object_type: object_type.clone(),
            structure: struct_name.to_string(),
            member: member.to_string(),
        }),
    }
}

/// The externally-visible verdict for a value cell.
///
/// The `kind == Param` guard is load-bearing and reproduces
/// `template_member_is_priv`'s: `value_cells` (and `guarded_groups[].members`)
/// also hold `let` bindings, and a default (non-`pub`) `let` is
/// `Visibility::Private` too — but `let`s are never externally accessible by
/// name, so reporting them as priv violations would be an out-of-scope
/// behaviour change. Only a `priv param` carries `Param` + `Private`.
fn value_cell_visibility(vc: &ValueCellDecl) -> MemberVisibility {
    if vc.kind == ValueCellKind::Param {
        visibility_of(vc.visibility)
    } else {
        MemberVisibility::Public
    }
}

fn visibility_of(v: Visibility) -> MemberVisibility {
    match v {
        Visibility::Public => MemberVisibility::Public,
        Visibility::Private => MemberVisibility::Private,
    }
}

// ─────────────────────────── chain resolution ───────────────────────────

/// What a fully-resolved member path lands ON — the four C1 terminal kinds.
///
/// The split is by **lowering shape**, not by member kind alone, because C1
/// lists `ValueCell` and `InstanceField` as distinct terminals even though both
/// can name a `value_cells` member. Which one applies decides which lowering a
/// consumer must emit, and that is the whole reason β/η can pick a lowering from
/// this verdict instead of re-matching AST shapes (C1-iv).
///
/// * [`TerminalKind::ValueCell`] — a solver-visible **scoped cell**. Reached only
///   by an all-`Sub`-hop chain from a `self`/sub root, whose entity stamp follows
///   the existing `format!("{}.{}", entity, sub)` convention (expr.rs:843).
/// * [`TerminalKind::InstanceField`] — a projection out of a runtime
///   `Value::StructureInstance`, i.e. what `CompiledExpr::index_access(obj,
///   "member")` reads. Applies once the chain crosses a value-typed
///   (`let m = Inner()`) receiver, and to `Sub`/`Port` terminals, which carry no
///   solver cell of their own (today's `expr.rs` lowers those to exactly this
///   `IndexAccess` shape under its permissive dimensionless fallback).
/// * [`TerminalKind::Realization`] — a named realization, `Type::Geometry`.
/// * [`TerminalKind::Synthesized`] — the PRD §7 extension point.
// TODO(#5425): first production consumer — task β's type-driven geometry-position
// acceptance calls `resolve_member_path`; #5430 (η) then converts the two-level
// `self.<sub>.<member>` matchers. Until one of those lands the chain walk has no
// non-`cfg(test)` caller, and `scripts/verify.sh` runs
// `cargo clippy --all-targets -- -D warnings`, which builds the lib WITHOUT
// `cfg(test)` — so the unit tests below do not keep these items alive.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalKind {
    ValueCell(ValueCellId),
    InstanceField {
        structure: String,
        member: String,
        member_kind: MemberKind,
    },
    Realization {
        structure: String,
        name: String,
    },
    Synthesized(SynthesizedMember),
}

/// A resolved dotted path: every hop it took, what it landed on, and the static
/// type of the whole expression.
// TODO(#5425): see the `TerminalKind` note above — β is the first production
// consumer, #5430 (η) the second.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Resolved {
    /// One entry per `.member` step, in source order.
    pub(crate) segments: Vec<Hop>,
    pub(crate) terminal: TerminalKind,
    /// The terminal hop's `next_type`.
    pub(crate) ty: Type,
}

/// How the chain's innermost node was typed, and whether it names a
/// solver-scoped entity.
///
/// `scoped_entity` is `Some` only for the roots that lower to scoped cells —
/// `self` (the entity being compiled) and a bare sub of it. A binding from
/// `scope.names` is a VALUE, so it is `None` and every terminal beneath it is an
/// `InstanceField` projection.
struct RootBinding {
    ty: Type,
    scoped_entity: Option<String>,
}

/// Resolve a whole `<root>.<m0>.<m1>…` chain of arbitrary depth (C1).
///
/// Purely static (C1-i): no diagnostic sink, no evaluation. Visibility is
/// enforced at EVERY hop (C1-ii) and an unknown member is attributed to the
/// concrete structure at the hop it occurred at (C1-iii).
///
/// A thin walk over [`resolve_hop`] — it re-authors no container-scan or
/// visibility logic. Its two additions are the `hop_index` rewrite (`resolve_hop`
/// stamps a placeholder `0`) and the synthesized-member table, which lives here
/// and NOT in `resolve_hop` so that declared members shadow synthesized ones and
/// `resolve_hop`'s verdict — the one production consumes — stays byte-identical.
// TODO(#5425): see the `TerminalKind` note above — β is the first production
// consumer, #5430 (η) the second.
#[allow(dead_code)]
pub(crate) fn resolve_member_path(
    expr: &reify_ast::Expr,
    scope: &CompilationScope,
) -> Result<Resolved, MemberPathError> {
    // 1. Flatten the `MemberAccess` spine into (root_expr, members-in-order).
    let mut members: Vec<&str> = Vec::new();
    let mut node = expr;
    while let reify_ast::ExprKind::MemberAccess { object, member } = &node.kind {
        members.push(member.as_str());
        node = object;
    }
    if members.is_empty() {
        return Err(MemberPathError::NotAMemberPath);
    }
    members.reverse();

    // 2. Type the root statically.
    let root = root_binding(node, scope).ok_or(MemberPathError::Indeterminate(
        IndeterminateReason::UnresolvableRoot,
    ))?;
    let mut object_type = root.ty;
    let mut scoped_entity = root.scoped_entity;
    let mut segments: Vec<Hop> = Vec::with_capacity(members.len());
    let last = members.len() - 1;

    // 3. Walk, threading `next_type` forward.
    for (hop_index, member) in members.iter().enumerate() {
        let (hop, next_type) = match resolve_hop(&object_type, member, scope) {
            Ok(r) => (r.hop, r.next_type),
            // 4. Synthesized extension point — consulted ONLY after the five
            //    declared containers have all missed, so a declared member of the
            //    same name shadows it.
            Err(MemberPathError::UnknownMember {
                object_type: unknown_on,
                structure,
                member: name,
                ..
            }) => match synthesized_member(&name) {
                Some((synthesized, ty)) => (
                    Hop {
                        object_type: object_type.clone(),
                        object_structure: Some(structure),
                        member: name,
                        member_kind: MemberKind::Synthesized(synthesized),
                        // Synthesized members carry no `priv` axis.
                        visibility: MemberVisibility::Public,
                    },
                    ty,
                ),
                None => {
                    return Err(MemberPathError::UnknownMember {
                        hop_index,
                        object_type: unknown_on,
                        structure,
                        member: name,
                    });
                }
            },
            // C1-ii/C1-iii: rewrite the placeholder index to the real position.
            // The walk stops here rather than reporting some later hop.
            Err(MemberPathError::PrivateMember {
                structure, member, ..
            }) => {
                return Err(MemberPathError::PrivateMember {
                    hop_index,
                    structure,
                    member,
                });
            }
            // `Indeterminate` propagates unchanged — the caller falls through to
            // its existing path. `NotAMemberPath` is unreachable from
            // `resolve_hop`, and is passed along rather than re-mapped.
            Err(deferred) => return Err(deferred),
        };

        // 5. Classify the terminal, or thread the scoped-entity prefix forward.
        if hop_index == last {
            let terminal = classify_terminal(&hop, scoped_entity.as_deref());
            segments.push(hop);
            return Ok(Resolved {
                segments,
                terminal,
                ty: next_type,
            });
        }
        scoped_entity = match (&hop.member_kind, &scoped_entity) {
            // Still inside the solver-scoped world: `Test` → `Test.a` → `Test.a.b`.
            (MemberKind::Sub, Some(prefix)) => Some(format!("{prefix}.{}", hop.member)),
            // Any other intermediate hop crosses into a VALUE receiver (a
            // `let m = Inner()` instance, a port, a realization), whose members
            // are `Value::StructureInstance` fields rather than scoped cells.
            _ => None,
        };
        object_type = next_type;
        segments.push(hop);
    }
    // `members` is non-empty and the `hop_index == last` arm always returns.
    unreachable!("the terminal hop returns from inside the loop")
}

/// Type the chain's innermost node — the only place this resolver looks at a
/// non-`MemberAccess` AST form.
///
/// `scope.names` is consulted BEFORE `scope.sub_component_types`: a name
/// registered as a value shadows a same-named sub, matching the lookup order the
/// existing `expr.rs` identifier arm uses.
fn root_binding(expr: &reify_ast::Expr, scope: &CompilationScope) -> Option<RootBinding> {
    let reify_ast::ExprKind::Ident(name) = &expr.kind else {
        // Calls, `IndexAccess` roots (`subs[i].member` — task η's collection
        // shape), literals, … are not typed here.
        return None;
    };
    if name == "self" {
        // `self` is not a registered name; it is valid only where the compiler
        // is inside an entity body (false for function and purpose scopes,
        // where today's code produces an "unresolved name" error).
        return scope.is_entity_scope.then(|| RootBinding {
            ty: Type::StructureRef(scope.entity_name.clone()),
            scoped_entity: Some(scope.entity_name.clone()),
        });
    }
    if let Some((_, ty, _)) = scope.names.get(name.as_str()) {
        return Some(RootBinding {
            ty: ty.clone(),
            scoped_entity: None,
        });
    }
    scope
        .sub_component_types
        .get(name.as_str())
        .map(|structure| RootBinding {
            ty: Type::StructureRef(structure.clone()),
            // `self.X` ≡ `X` (spec §8.6): a bare sub root names the same scoped
            // entity the `self.`-prefixed form does.
            scoped_entity: Some(format!("{}.{}", scope.entity_name, name)),
        })
}

/// The PRD §7 synthesized-member table — members no `TopologyTemplate`
/// container declares but the language nonetheless makes accessible.
///
/// `Type::Int` for `count` matches the existing `"count" => Type::Int` arms in
/// `expr.rs`, so a consumer that routes through here types collection counts
/// exactly as today.
fn synthesized_member(member: &str) -> Option<(SynthesizedMember, Type)> {
    match member {
        "count" => Some((SynthesizedMember::Count, Type::Int)),
        _ => None,
    }
}

fn classify_terminal(hop: &Hop, scoped_entity: Option<&str>) -> TerminalKind {
    let structure = hop.object_structure.clone().unwrap_or_default();
    match hop.member_kind {
        // Matched exhaustively with NO `_` arm ON PURPOSE: adding a variant to
        // `SynthesizedMember` (the placement-relations belt's `.world_frame`,
        // PRD §7) must be a COMPILE ERROR here — a new synthesized member needs
        // a deliberate terminal-lowering decision, not a silent fall-through.
        MemberKind::Synthesized(synthesized) => match synthesized {
            SynthesizedMember::Count => TerminalKind::Synthesized(SynthesizedMember::Count),
        },
        MemberKind::Realization => TerminalKind::Realization {
            structure,
            name: hop.member.clone(),
        },
        MemberKind::ValueCell(_) => match scoped_entity {
            Some(entity) => TerminalKind::ValueCell(ValueCellId::new(entity, &hop.member)),
            None => TerminalKind::InstanceField {
                structure,
                member: hop.member.clone(),
                member_kind: hop.member_kind,
            },
        },
        // A sub or port terminal has no solver cell of its own, so it is the
        // projection shape — which is also exactly what `expr.rs` lowers such an
        // access to today (`index_access` under the dimensionless fallback).
        MemberKind::Sub | MemberKind::Port => TerminalKind::InstanceField {
            structure,
            member: hop.member.clone(),
            member_kind: hop.member_kind,
        },
    }
}

impl MemberPathError {
    /// Render this error as the diagnostic the CALLER should push, or `None`
    /// when there is nothing to say and the caller should fall through to its
    /// existing path.
    ///
    /// The message text, label text and `DiagnosticCode`s are copied verbatim
    /// from the two producer sites in `expr.rs` this resolver replaced, because
    /// several suites assert on the literal strings (`priv_member_visibility_tests.rs`,
    /// `priv_import_boundary_tests.rs`, `cli_module_visibility_example.rs`,
    /// `examples_smoke.rs`). No new `DiagnosticCode` variant is introduced —
    /// that would ripple through reify-core and
    /// `tests/diagnostic_coverage_checkpoint.rs`.
    pub(crate) fn to_diagnostic(&self, span: reify_core::SourceSpan) -> Option<Diagnostic> {
        match self {
            // Nothing to report: the caller keeps its existing behaviour.
            MemberPathError::NotAMemberPath | MemberPathError::Indeterminate(_) => None,
            MemberPathError::PrivateMember {
                structure, member, ..
            } => Some(
                Diagnostic::error(format!(
                    "E_PRIV_MEMBER_ACCESS: member '{member}' of structure '{structure}' is private"
                ))
                .with_label(DiagnosticLabel::new(span, "private member accessed here"))
                .with_code(DiagnosticCode::PrivMemberAccess),
            ),
            MemberPathError::UnknownMember {
                structure, member, ..
            } => Some(
                Diagnostic::error(format!("structure '{structure}' has no member '{member}'"))
                    .with_label(DiagnosticLabel::new(span, "unknown member"))
                    .with_code(DiagnosticCode::StructureMemberNotFound),
            ),
        }
    }
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
            // Hand-built member-path fixtures are always product geometry; the
            // `__geoq_<N>` query-only flag is minted only by the inline
            // geometry-query hoist in entity.rs (task 5345).
            is_query_only: false,
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

    // ── step-5: the D6 visibility verdict, one per priv-able member kind ──

    /// `structure def P {
    ///     priv param secret : Length     param plain : Length
    ///     let internal : Length          priv sub hidden = Leaf()
    ///     sub open = Leaf()              priv port pp : SomeTrait { }
    ///     port op : SomeTrait { }
    ///     where on { priv param gsecret : Length  param gplain : Length }
    /// }`
    fn template_p() -> TopologyTemplate {
        TopologyTemplate {
            value_cells: vec![
                value_cell(
                    "P",
                    "secret",
                    ValueCellKind::Param,
                    Visibility::Private,
                    Type::length(),
                ),
                value_cell(
                    "P",
                    "plain",
                    ValueCellKind::Param,
                    Visibility::Public,
                    Type::length(),
                ),
                // A default (non-`pub`) `let` is `Visibility::Private` in the IR
                // but is NEVER externally nameable — the load-bearing negative.
                value_cell(
                    "P",
                    "internal",
                    ValueCellKind::Let,
                    Visibility::Private,
                    Type::length(),
                ),
            ],
            sub_components: vec![
                sub_component("hidden", "Leaf", Visibility::Private),
                sub_component("open", "Leaf", Visibility::Public),
            ],
            ports: vec![port("pp", true), port("op", false)],
            guarded_groups: vec![CompiledGuardedGroup {
                guard_expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
                guard_value_cell: ValueCellId::new("P", "__guard_on"),
                members: vec![
                    value_cell(
                        "P",
                        "gsecret",
                        ValueCellKind::Param,
                        Visibility::Private,
                        Type::length(),
                    ),
                    value_cell(
                        "P",
                        "gplain",
                        ValueCellKind::Param,
                        Visibility::Public,
                        Type::length(),
                    ),
                ],
                constraints: Vec::new(),
                else_members: Vec::new(),
                else_constraints: Vec::new(),
                parent_guard: None,
            }],
            ..empty_template("P")
        }
    }

    fn priv_member_err(member: &str) -> MemberPathError {
        MemberPathError::PrivateMember {
            hop_index: 0,
            structure: "P".to_string(),
            member: member.to_string(),
        }
    }

    #[test]
    fn resolve_hop_denies_a_priv_param() {
        let templates = [template_p()];
        with_scope("Test", &templates, |scope| {
            let obj = Type::StructureRef("P".to_string());
            assert_eq!(
                resolve_hop(&obj, "secret", scope),
                Err(priv_member_err("secret"))
            );
            assert!(resolve_hop(&obj, "plain", scope).is_ok(), "public control");
        });
    }

    #[test]
    fn resolve_hop_denies_a_priv_sub() {
        let templates = [template_p()];
        with_scope("Test", &templates, |scope| {
            let obj = Type::StructureRef("P".to_string());
            assert_eq!(
                resolve_hop(&obj, "hidden", scope),
                Err(priv_member_err("hidden"))
            );
            assert!(resolve_hop(&obj, "open", scope).is_ok(), "public control");
        });
    }

    #[test]
    fn resolve_hop_denies_a_priv_port() {
        let templates = [template_p()];
        with_scope("Test", &templates, |scope| {
            let obj = Type::StructureRef("P".to_string());
            assert_eq!(resolve_hop(&obj, "pp", scope), Err(priv_member_err("pp")));
            assert!(resolve_hop(&obj, "op", scope).is_ok(), "public control");
        });
    }

    /// Task #5171: a `priv param` nested in a block-form `where` guarded group.
    #[test]
    fn resolve_hop_denies_a_priv_param_inside_a_guarded_group() {
        let templates = [template_p()];
        with_scope("Test", &templates, |scope| {
            let obj = Type::StructureRef("P".to_string());
            assert_eq!(
                resolve_hop(&obj, "gsecret", scope),
                Err(priv_member_err("gsecret"))
            );
            assert!(resolve_hop(&obj, "gplain", scope).is_ok(), "public control");
        });
    }

    /// The load-bearing negative behind `template_member_is_priv`'s
    /// `kind == Param` guard: `value_cells` also holds `let` bindings, and a
    /// default (non-`pub`) `let` carries `Visibility::Private` — but `let`s are
    /// never externally accessible by name, so reporting one as a priv
    /// violation would be an out-of-scope behaviour change.
    #[test]
    fn resolve_hop_does_not_report_a_default_visibility_let_as_private() {
        let templates = [template_p()];
        with_scope("Test", &templates, |scope| {
            let r = resolve_hop(&Type::StructureRef("P".to_string()), "internal", scope)
                .expect("a `let` cell is not a priv violation");
            assert_eq!(r.hop.visibility, MemberVisibility::Public);
        });
    }

    /// The ordering invariant `expr.rs` calls load-bearing (task #5171): a priv
    /// param declared inside a guarded group must report `PrivateMember`, never
    /// `UnknownMember`. Asserting on the VARIANT catches a future reorder that
    /// lets not-found win.
    #[test]
    fn priv_beats_unknown_for_a_guarded_group_member() {
        let templates = [template_p()];
        with_scope("Test", &templates, |scope| {
            let err = resolve_hop(&Type::StructureRef("P".to_string()), "gsecret", scope)
                .expect_err("a priv guarded member must be denied");
            assert!(
                matches!(err, MemberPathError::PrivateMember { .. }),
                "expected PrivateMember, got {err:?} — the priv check must run \
                 over the full container union BEFORE the unknown-member fallback"
            );
        });
    }

    // ── step-5: diagnostic rendering is byte-identical to today's ─────

    #[test]
    fn to_diagnostic_renders_the_existing_priv_member_access_text() {
        let span = SourceSpan::new(3, 9);
        let d = MemberPathError::PrivateMember {
            hop_index: 1,
            structure: "Inner".to_string(),
            member: "gp".to_string(),
        }
        .to_diagnostic(span)
        .expect("PrivateMember renders a diagnostic");
        assert_eq!(
            d.message,
            "E_PRIV_MEMBER_ACCESS: member 'gp' of structure 'Inner' is private"
        );
        assert_eq!(d.code, Some(DiagnosticCode::PrivMemberAccess));
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.labels.len(), 1);
        assert_eq!(d.labels[0].message, "private member accessed here");
        assert_eq!(d.labels[0].span, span);
    }

    #[test]
    fn to_diagnostic_renders_the_existing_structure_member_not_found_text() {
        let span = SourceSpan::new(3, 9);
        let d = MemberPathError::UnknownMember {
            hop_index: 0,
            object_type: Type::StructureRef("Inner".to_string()),
            structure: "Inner".to_string(),
            member: "nope".to_string(),
        }
        .to_diagnostic(span)
        .expect("UnknownMember renders a diagnostic");
        assert_eq!(d.message, "structure 'Inner' has no member 'nope'");
        assert_eq!(d.code, Some(DiagnosticCode::StructureMemberNotFound));
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.labels.len(), 1);
        assert_eq!(d.labels[0].message, "unknown member");
        assert_eq!(d.labels[0].span, span);
    }

    /// `NotAMemberPath` / `Indeterminate` have nothing to say — the caller falls
    /// through to its existing path rather than reporting.
    #[test]
    fn to_diagnostic_is_silent_for_the_defer_variants() {
        let span = SourceSpan::new(0, 0);
        assert!(
            MemberPathError::NotAMemberPath
                .to_diagnostic(span)
                .is_none()
        );
        for reason in [
            IndeterminateReason::WildcardStructure,
            IndeterminateReason::TraitObjectReceiver,
            IndeterminateReason::TemplateNotInRegistry,
            IndeterminateReason::NoTemplateRegistry,
            IndeterminateReason::UnresolvableRoot,
        ] {
            assert!(
                MemberPathError::Indeterminate(reason)
                    .to_diagnostic(span)
                    .is_none(),
                "{reason:?} must defer, not report"
            );
        }
    }

    // ── step-7: the C1 chain walk (`resolve_member_path`) ─────────────
    //
    // C1-i (purely static) is enforced by the SIGNATURE, not by an assertion:
    // `resolve_member_path(expr, scope)` takes no `&mut Vec<Diagnostic>`, so a
    // future edit that wants to report from inside the resolver cannot do it
    // without changing every call site — which is exactly the review signal the
    // contract wants. Nothing below passes a diagnostic sink because there is
    // none to pass.

    fn ast_ident(name: &str) -> reify_ast::Expr {
        reify_ast::Expr {
            kind: reify_ast::ExprKind::Ident(name.to_string()),
            span: SourceSpan::new(0, 0),
        }
    }

    fn ast_dot(object: reify_ast::Expr, member: &str) -> reify_ast::Expr {
        reify_ast::Expr {
            kind: reify_ast::ExprKind::MemberAccess {
                object: Box::new(object),
                member: member.to_string(),
            },
            span: SourceSpan::new(0, 0),
        }
    }

    /// `root.m0.m1.…` — the dotted chain these tests resolve.
    fn ast_path(root: &str, members: &[&str]) -> reify_ast::Expr {
        members
            .iter()
            .fold(ast_ident(root), |acc, m| ast_dot(acc, m))
    }

    /// `<root_expr>.m0.m1.…` for roots that are not a bare identifier.
    fn ast_path_from(root: reify_ast::Expr, members: &[&str]) -> reify_ast::Expr {
        members.iter().fold(root, |acc, m| ast_dot(acc, m))
    }

    /// What is visible at the chain ROOT: the two maps `resolve_member_path`
    /// consults (`names`, then `sub_component_types`) plus the `self` gate.
    ///
    /// A data seed rather than a `FnOnce(&mut CompilationScope<'_>)` callback:
    /// the scope borrows the template registry for a higher-ranked lifetime, and
    /// a mutating callback over it needs an HRTB annotation at every call site.
    #[derive(Default)]
    struct RootSeed {
        /// `scope.names`: the let-instance / param / purpose-param root family.
        names: Vec<(&'static str, Type)>,
        /// `scope.sub_component_types`: sub_name → structure_name.
        subs: Vec<(&'static str, &'static str)>,
        is_entity_scope: bool,
    }

    fn with_seeded_scope<R>(
        entity: &str,
        templates: &[TopologyTemplate],
        seed: RootSeed,
        body: impl FnOnce(&CompilationScope) -> R,
    ) -> R {
        let registry: HashMap<String, &TopologyTemplate> =
            templates.iter().map(|t| (t.name.clone(), t)).collect();
        let mut scope = CompilationScope::new(entity);
        scope.set_template_registry(&registry);
        scope.is_entity_scope = seed.is_entity_scope;
        for (name, ty) in seed.names {
            scope.names.insert(
                name.to_string(),
                (ValueCellId::new(entity, name), ty, None),
            );
        }
        for (sub, structure) in seed.subs {
            scope
                .sub_component_types
                .insert(sub.to_string(), structure.to_string());
        }
        body(&scope)
    }

    /// `structure def Leaf { param w : Length  priv param secret : Length  let body = <geom> }`
    fn template_leaf() -> TopologyTemplate {
        TopologyTemplate {
            value_cells: vec![
                value_cell(
                    "Leaf",
                    "w",
                    ValueCellKind::Param,
                    Visibility::Public,
                    Type::length(),
                ),
                value_cell(
                    "Leaf",
                    "secret",
                    ValueCellKind::Param,
                    Visibility::Private,
                    Type::length(),
                ),
            ],
            realizations: vec![realization("Leaf", 0, "body")],
            ..empty_template("Leaf")
        }
    }

    /// `structure def Mid { sub leaf = Leaf()  priv sub hidden = Leaf()  param inner : Length }`
    fn template_mid() -> TopologyTemplate {
        TopologyTemplate {
            value_cells: vec![value_cell(
                "Mid",
                "inner",
                ValueCellKind::Param,
                Visibility::Public,
                Type::length(),
            )],
            sub_components: vec![
                sub_component("leaf", "Leaf", Visibility::Public),
                sub_component("hidden", "Leaf", Visibility::Private),
            ],
            ..empty_template("Mid")
        }
    }

    /// `structure def Root { sub mid = Mid()  sub direct = Leaf()  let inst = Leaf() }`
    fn template_root() -> TopologyTemplate {
        TopologyTemplate {
            value_cells: vec![value_cell(
                "Root",
                "inst",
                ValueCellKind::Let,
                Visibility::Public,
                Type::StructureRef("Leaf".to_string()),
            )],
            sub_components: vec![
                sub_component("mid", "Mid", Visibility::Public),
                sub_component("direct", "Leaf", Visibility::Public),
            ],
            ..empty_template("Root")
        }
    }

    fn chain_templates() -> [TopologyTemplate; 3] {
        [template_root(), template_mid(), template_leaf()]
    }

    /// (a) ARBITRARY DEPTH — the walk is not the two-level special case today's
    /// `self.<sub>.<member>` matcher hard-codes. Each hop records the CONCRETE
    /// type it was taken FROM, which is what C1-iii attribution reads.
    #[test]
    fn resolve_member_path_walks_a_three_hop_chain_recording_each_hops_concrete_type() {
        let templates = chain_templates();
        let seed = RootSeed {
            is_entity_scope: true,
            ..RootSeed::default()
        };
        with_seeded_scope("Root", &templates, seed, |scope| {
            let r = resolve_member_path(&ast_path("self", &["mid", "leaf", "w"]), scope)
                .expect("self.mid.leaf.w is a fully declared 3-hop chain");
            assert_eq!(r.segments.len(), 3, "one segment per `.member` step");
            assert_eq!(
                r.segments[0].object_type,
                Type::StructureRef("Root".to_string())
            );
            assert_eq!(r.segments[0].object_structure.as_deref(), Some("Root"));
            assert_eq!(r.segments[0].member, "mid");
            assert_eq!(r.segments[0].member_kind, MemberKind::Sub);

            assert_eq!(
                r.segments[1].object_type,
                Type::StructureRef("Mid".to_string())
            );
            assert_eq!(r.segments[1].object_structure.as_deref(), Some("Mid"));
            assert_eq!(r.segments[1].member, "leaf");
            assert_eq!(r.segments[1].member_kind, MemberKind::Sub);

            assert_eq!(
                r.segments[2].object_type,
                Type::StructureRef("Leaf".to_string()),
                "the terminal hop is taken FROM Leaf, not from the chain root"
            );
            assert_eq!(r.segments[2].object_structure.as_deref(), Some("Leaf"));
            assert_eq!(
                r.segments[2].member_kind,
                MemberKind::ValueCell(ValueCellKind::Param)
            );
            assert_eq!(r.ty, Type::length(), "the terminal hop's declared type");
        });
    }

    /// (b) HOP_INDEX REWRITE — `resolve_hop` stamps the placeholder `0`; the walk
    /// must overwrite it with the real position, and attribute to the concrete
    /// structure at THAT hop (C1-iii, boundary row 11). Asserted on the MIDDLE
    /// hop so neither "always 0" nor "always last" passes.
    #[test]
    fn resolve_member_path_attributes_an_unknown_member_to_the_hop_it_occurred_at() {
        let templates = chain_templates();
        let seed = RootSeed {
            is_entity_scope: true,
            ..RootSeed::default()
        };
        with_seeded_scope("Root", &templates, seed, |scope| {
            let err = resolve_member_path(&ast_path("self", &["mid", "nope", "w"]), scope)
                .expect_err("'nope' is declared by no container of Mid");
            assert_eq!(
                err,
                MemberPathError::UnknownMember {
                    hop_index: 1,
                    object_type: Type::StructureRef("Mid".to_string()),
                    structure: "Mid".to_string(),
                    member: "nope".to_string(),
                },
                "the middle hop's index and the CONCRETE structure at it — \
                 never hop 0 and never the chain root's structure"
            );
        });
    }

    /// (c) PRIV AT EVERY HOP (C1-ii) — a `priv sub` at an INTERMEDIATE hop is
    /// denied there; the walk stops rather than continuing and reporting the
    /// terminal instead.
    #[test]
    fn resolve_member_path_enforces_priv_at_an_intermediate_hop_and_stops_there() {
        let templates = chain_templates();
        let seed = RootSeed {
            is_entity_scope: true,
            ..RootSeed::default()
        };
        with_seeded_scope("Root", &templates, seed, |scope| {
            let err = resolve_member_path(&ast_path("self", &["mid", "hidden", "w"]), scope)
                .expect_err("'hidden' is a priv sub of Mid");
            assert_eq!(
                err,
                MemberPathError::PrivateMember {
                    hop_index: 1,
                    structure: "Mid".to_string(),
                    member: "hidden".to_string(),
                }
            );
            // The public sibling proves the pin is not vacuous: the same shape
            // through `leaf` resolves all the way to the terminal.
            assert!(
                resolve_member_path(&ast_path("self", &["mid", "leaf", "w"]), scope).is_ok(),
                "public control"
            );
        });
    }

    /// (d.1) TERMINAL — an all-`Sub`-hop chain from a `self`/sub root names a
    /// SOLVER-VISIBLE scoped cell, stamped with the existing
    /// `format!("{}.{}", entity, sub)` convention (expr.rs:843).
    #[test]
    fn resolve_member_path_classifies_a_sub_rooted_chain_as_a_scoped_value_cell() {
        let templates = chain_templates();
        let seed = RootSeed {
            is_entity_scope: true,
            ..RootSeed::default()
        };
        with_seeded_scope("Root", &templates, seed, |scope| {
            let r = resolve_member_path(&ast_path("self", &["mid", "leaf", "w"]), scope)
                .expect("declared chain");
            assert_eq!(
                r.terminal,
                TerminalKind::ValueCell(ValueCellId::new("Root.mid.leaf", "w")),
                "the scoped entity accumulates every sub hop"
            );
        });
        // A bare sub root (`mid.leaf.w`, no `self.` prefix) must land on the
        // SAME scoped cell — the `self.X` ≡ `X` equivalence (spec §8.6).
        let seed = RootSeed {
            subs: vec![("mid", "Mid")],
            ..RootSeed::default()
        };
        with_seeded_scope("Root", &templates, seed, |scope| {
            let r = resolve_member_path(&ast_path("mid", &["leaf", "w"]), scope)
                .expect("declared chain from a bare sub root");
            assert_eq!(
                r.terminal,
                TerminalKind::ValueCell(ValueCellId::new("Root.mid.leaf", "w"))
            );
        });
    }

    /// (d.2) TERMINAL — a chain that passes through a VALUE-typed receiver
    /// (a `let m = Leaf()` instance) lowers to the `Value::StructureInstance`
    /// projection, not to a solver cell, so it terminates in `InstanceField`.
    #[test]
    fn resolve_member_path_classifies_a_let_instance_projection_as_an_instance_field() {
        let templates = chain_templates();
        let seed = RootSeed {
            names: vec![("m", Type::StructureRef("Leaf".to_string()))],
            ..RootSeed::default()
        };
        with_seeded_scope("Test", &templates, seed, |scope| {
            let r = resolve_member_path(&ast_path("m", &["w"]), scope)
                .expect("'w' is a declared param of Leaf");
            assert_eq!(
                r.terminal,
                TerminalKind::InstanceField {
                    structure: "Leaf".to_string(),
                    member: "w".to_string(),
                    member_kind: MemberKind::ValueCell(ValueCellKind::Param),
                }
            );
            assert_eq!(r.ty, Type::length());
        });
        // The same distinction INSIDE an entity: `self.inst` is a `let`-kind
        // value cell typed `StructureRef("Leaf")`, so the hop through it is a
        // value-typed receiver even though the ROOT was `self`.
        let seed = RootSeed {
            is_entity_scope: true,
            ..RootSeed::default()
        };
        with_seeded_scope("Root", &templates, seed, |scope| {
            let r = resolve_member_path(&ast_path("self", &["inst", "w"]), scope)
                .expect("self.inst.w projects out of a let-instance");
            assert_eq!(
                r.terminal,
                TerminalKind::InstanceField {
                    structure: "Leaf".to_string(),
                    member: "w".to_string(),
                    member_kind: MemberKind::ValueCell(ValueCellKind::Param),
                },
                "a non-Sub intermediate hop breaks the scoped-cell chain"
            );
        });
    }

    /// (d.3) TERMINAL — a named realization is `Type::Geometry`, the same type
    /// `expr.rs` stamps on a `CrossSubGeometryRef`.
    #[test]
    fn resolve_member_path_classifies_a_named_realization_terminal_as_geometry() {
        let templates = chain_templates();
        let seed = RootSeed {
            names: vec![("m", Type::StructureRef("Leaf".to_string()))],
            ..RootSeed::default()
        };
        with_seeded_scope("Test", &templates, seed, |scope| {
            let r = resolve_member_path(&ast_path("m", &["body"]), scope)
                .expect("'body' is a named realization of Leaf");
            assert_eq!(
                r.terminal,
                TerminalKind::Realization {
                    structure: "Leaf".to_string(),
                    name: "body".to_string(),
                }
            );
            assert_eq!(r.ty, Type::Geometry);
        });
    }

    /// (d.4) + (e) SYNTHESIZED — `.count` is the PRD §7 extension point. It is
    /// consulted ONLY after the five declared containers all miss, and ONLY from
    /// `resolve_member_path`.
    #[test]
    fn resolve_member_path_synthesizes_count_after_the_declared_containers_miss() {
        let templates = chain_templates();
        let seed = RootSeed {
            names: vec![("m", Type::StructureRef("Leaf".to_string()))],
            ..RootSeed::default()
        };
        with_seeded_scope("Test", &templates, seed, |scope| {
            let r = resolve_member_path(&ast_path("m", &["count"]), scope)
                .expect("`.count` is synthesized on any receiver");
            assert_eq!(
                r.terminal,
                TerminalKind::Synthesized(SynthesizedMember::Count)
            );
            assert_eq!(r.ty, Type::Int, "matching expr.rs's `\"count\" => Type::Int`");
            assert_eq!(r.segments.len(), 1);
            assert_eq!(
                r.segments[0].member_kind,
                MemberKind::Synthesized(SynthesizedMember::Count)
            );
            assert_eq!(r.segments[0].visibility, MemberVisibility::Public);
        });
    }

    /// (e) SHADOWING — a DECLARED member named `count` wins over the synthesized
    /// entry, so a structure that models a real `count` param keeps its own type.
    #[test]
    fn a_declared_count_member_shadows_the_synthesized_one() {
        let templates = [TopologyTemplate {
            value_cells: vec![value_cell(
                "Counted",
                "count",
                ValueCellKind::Param,
                Visibility::Public,
                Type::length(),
            )],
            ..empty_template("Counted")
        }];
        let seed = RootSeed {
            names: vec![("c", Type::StructureRef("Counted".to_string()))],
            ..RootSeed::default()
        };
        with_seeded_scope("Test", &templates, seed, |scope| {
            let r = resolve_member_path(&ast_path("c", &["count"]), scope)
                .expect("the declared param resolves");
            assert_eq!(
                r.terminal,
                TerminalKind::InstanceField {
                    structure: "Counted".to_string(),
                    member: "count".to_string(),
                    member_kind: MemberKind::ValueCell(ValueCellKind::Param),
                },
                "a declared member must shadow the synthesized entry"
            );
            assert_eq!(r.ty, Type::length(), "NOT the synthesized Type::Int");
        });
    }

    /// (e) The other half of the shadowing contract, and the reason it is
    /// load-bearing: `resolve_hop` is the entry point step-10 wires into
    /// production. If IT learned about `count`, today's
    /// `structure 'Leaf' has no member 'count'` diagnostic would silently
    /// disappear the moment the priv delegation lands.
    #[test]
    fn resolve_hop_never_synthesizes_count() {
        let templates = chain_templates();
        with_scope("Test", &templates, |scope| {
            let err = resolve_hop(&Type::StructureRef("Leaf".to_string()), "count", scope)
                .expect_err("resolve_hop knows only DECLARED containers");
            assert!(
                matches!(err, MemberPathError::UnknownMember { .. }),
                "expected UnknownMember, got {err:?} — synthesized members live \
                 one level up, in resolve_member_path"
            );
        });
    }

    /// (f) `self` ROOT — typed from the entity scope, not from `names`.
    #[test]
    fn resolve_member_path_types_a_self_root_from_the_entity_scope() {
        let templates = chain_templates();
        let seed = RootSeed {
            is_entity_scope: true,
            ..RootSeed::default()
        };
        with_seeded_scope("Root", &templates, seed, |scope| {
            let r = resolve_member_path(&ast_path("self", &["mid", "inner"]), scope)
                .expect("self.mid.inner resolves in an entity scope");
            assert_eq!(
                r.segments[0].object_type,
                Type::StructureRef("Root".to_string()),
                "`self` types as the entity being compiled"
            );
            assert_eq!(r.ty, Type::length());
        });
    }

    /// (f) …and NOT outside one: in a function / purpose scope `self` is not a
    /// name, so the root is not statically typable and the caller keeps its
    /// existing "unresolved name" path.
    #[test]
    fn resolve_member_path_defers_on_a_self_root_outside_an_entity_scope() {
        let templates = chain_templates();
        with_seeded_scope("Root", &templates, RootSeed::default(), |scope| {
            assert_eq!(
                resolve_member_path(&ast_path("self", &["mid", "inner"]), scope),
                Err(MemberPathError::Indeterminate(
                    IndeterminateReason::UnresolvableRoot
                ))
            );
        });
    }

    /// (g) ALIAS STABILITY — the ratified frame rule's STATIC half: an alias
    /// binding and the sub it aliases must agree on member shape. Types and
    /// terminal only; frame COMPOSITION is value-level and not α's.
    #[test]
    fn an_alias_root_and_a_sub_root_agree_on_terminal_and_type() {
        let templates = chain_templates();
        let via_alias = {
            let seed = RootSeed {
                names: vec![("h", Type::StructureRef("Leaf".to_string()))],
                ..RootSeed::default()
            };
            with_seeded_scope("Root", &templates, seed, |scope| {
                resolve_member_path(&ast_path("h", &["body"]), scope).expect("h.body")
            })
        };
        let via_sub = {
            let seed = RootSeed {
                is_entity_scope: true,
                ..RootSeed::default()
            };
            with_seeded_scope("Root", &templates, seed, |scope| {
                resolve_member_path(&ast_path("self", &["direct", "body"]), scope)
                    .expect("self.direct.body")
            })
        };
        assert_eq!(
            via_alias.terminal, via_sub.terminal,
            "`h.body` and `self.direct.body` name the same member of the same \
             structure — the alias must not change the member shape"
        );
        assert_eq!(via_alias.ty, via_sub.ty);
        assert_eq!(via_alias.ty, Type::Geometry);
    }

    /// (h) DEFER — not a `.`-chain at all.
    #[test]
    fn resolve_member_path_reports_a_non_member_path_expression() {
        let templates = chain_templates();
        let seed = RootSeed {
            names: vec![("m", Type::StructureRef("Leaf".to_string()))],
            ..RootSeed::default()
        };
        with_seeded_scope("Test", &templates, seed, |scope| {
            assert_eq!(
                resolve_member_path(&ast_ident("m"), scope),
                Err(MemberPathError::NotAMemberPath)
            );
        });
    }

    /// (h) DEFER — roots this resolver does not type: a call, an `IndexAccess`
    /// (`subs[i].member` — task η's collection shape), and an unbound name.
    #[test]
    fn resolve_member_path_defers_on_an_unresolvable_root() {
        let templates = chain_templates();
        let seed = RootSeed {
            names: vec![("m", Type::StructureRef("Leaf".to_string()))],
            ..RootSeed::default()
        };
        with_seeded_scope("Test", &templates, seed, |scope| {
            let expected = Err(MemberPathError::Indeterminate(
                IndeterminateReason::UnresolvableRoot,
            ));
            let call = reify_ast::Expr {
                kind: reify_ast::ExprKind::FunctionCall {
                    name: "make_leaf".to_string(),
                    args: Vec::new(),
                    arg_names: Vec::new(),
                },
                span: SourceSpan::new(0, 0),
            };
            assert_eq!(
                resolve_member_path(&ast_path_from(call, &["w"]), scope),
                expected,
                "a call root"
            );
            let indexed = reify_ast::Expr {
                kind: reify_ast::ExprKind::IndexAccess {
                    object: Box::new(ast_ident("m")),
                    index: Box::new(reify_ast::Expr {
                        kind: reify_ast::ExprKind::NumberLiteral {
                            value: 0.0,
                            is_real: false,
                        },
                        span: SourceSpan::new(0, 0),
                    }),
                },
                span: SourceSpan::new(0, 0),
            };
            assert_eq!(
                resolve_member_path(&ast_path_from(indexed, &["w"]), scope),
                expected,
                "an IndexAccess root"
            );
            assert_eq!(
                resolve_member_path(&ast_path("nowhere", &["w"]), scope),
                expected,
                "an unbound identifier root"
            );
        });
    }

    /// (h) DEFER — traits are absent from `template_registry`, so the concrete
    /// runtime type is not statically known. Preserved byte-for-byte.
    #[test]
    fn resolve_member_path_defers_on_a_trait_object_receiver() {
        let templates = chain_templates();
        let seed = RootSeed {
            names: vec![("t", Type::TraitObject("SomeTrait".to_string()))],
            ..RootSeed::default()
        };
        with_seeded_scope("Test", &templates, seed, |scope| {
            assert_eq!(
                resolve_member_path(&ast_path("t", &["w"]), scope),
                Err(MemberPathError::Indeterminate(
                    IndeterminateReason::TraitObjectReceiver
                ))
            );
        });
    }

    /// (h) DEFER — function and field scopes carry no template registry at all.
    #[test]
    fn resolve_member_path_defers_when_the_scope_has_no_template_registry() {
        let mut scope = CompilationScope::new("Test");
        scope.names.insert(
            "m".to_string(),
            (
                ValueCellId::new("Test", "m"),
                Type::StructureRef("Leaf".to_string()),
                None,
            ),
        );
        assert!(scope.template_registry.is_none());
        assert_eq!(
            resolve_member_path(&ast_path("m", &["w"]), &scope),
            Err(MemberPathError::Indeterminate(
                IndeterminateReason::NoTemplateRegistry
            ))
        );
    }

    /// (i) C1-i — resolution mutates nothing, so resolving the same path twice
    /// against the same scope yields the identical verdict. (The stronger half —
    /// "emits no diagnostics" — is enforced by the signature: there is no sink
    /// to emit into.)
    #[test]
    fn resolve_member_path_is_purely_static_and_repeatable() {
        let templates = chain_templates();
        let seed = RootSeed {
            is_entity_scope: true,
            ..RootSeed::default()
        };
        with_seeded_scope("Root", &templates, seed, |scope| {
            let path = ast_path("self", &["mid", "leaf", "w"]);
            let first = resolve_member_path(&path, scope);
            let second = resolve_member_path(&path, scope);
            assert_eq!(first, second);
            let bad = ast_path("self", &["mid", "nope"]);
            assert_eq!(
                resolve_member_path(&bad, scope),
                resolve_member_path(&bad, scope)
            );
        });
    }
}
