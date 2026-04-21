pub(super) mod checker;
use checker::*;

use super::*;


#[allow(clippy::too_many_arguments)]
pub(crate) fn check_trait_conformance(
    structure: &EntityDefRef<'_>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    trait_names: &HashSet<String>,
    scope: &mut CompilationScope,
    value_cells: &mut Vec<ValueCellDecl>,
    constraints: &mut Vec<CompiledConstraint>,
    constraint_index: &mut u32,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (structure_members, structure_constraint_labels) =
        check_phase_resolve_structure_members(structure, trait_names, enum_defs, alias_registry, diagnostics);

    let ctx =
        check_phase_collect_trait_bounds(structure, trait_registry, &structure_members, diagnostics);

    let (inferred_let_exprs, pass2_skipped) = check_phase_pre_register_default_types(
        &ctx,
        &structure_members,
        structure.name,
        scope,
        enum_defs,
        functions,
        diagnostics,
    );

    let available_defaults =
        check_phase_build_available_defaults_map(&ctx, &inferred_let_exprs, &pass2_skipped);

    check_phase_check_members_against_requirements(
        &ctx,
        structure,
        &structure_members,
        &available_defaults,
        diagnostics,
    );

    check_phase_inject_defaults(
        &ctx,
        structure,
        &structure_members,
        &structure_constraint_labels,
        inferred_let_exprs,
        &pass2_skipped,
        scope,
        value_cells,
        constraints,
        constraint_index,
        enum_defs,
        functions,
        diagnostics,
    );
}


/// Verify that a compiled arg value's type conforms to the declared param type
/// in the target structure when the declared type is `Type::TraitObject(trait_name)`.
///
/// `arg_call_name` carries the callee name when the arg expression was any
/// `FunctionCall` (e.g. `Steel()` or `Steel(density: 1.0)` → `Some("Steel")`).
/// The expression compiler can default to `Type::Real` for unknown calls; if
/// `arg_call_name` is a known structure in the template registry we promote the
/// arg type to `StructureRef(name)` for the conformance check.
///
/// Conformance strategy (step-6 verified):
/// - `Type::StructureRef` args: uses `satisfies_trait_bound` to walk the structure's declared
///   trait bounds, following refinement chains transitively (e.g. `Rigid : Physical : Material`
///   satisfies a `Material` param).
/// - `Type::TraitObject` args: uses `trait_satisfies` to check equality-or-refinement between
///   the arg trait and the required trait.
///
/// Skips silently when:
/// - The target template is not found (external/unknown structure).
/// - The arg name is not found in the target's value cells (positional arg or error).
/// - The declared param type is not `Type::TraitObject` (no call-site type-check is performed in the compiler today for non-trait params).
/// - The arg_type is `Type::Error` (anti-cascade: treat as pass-through).
///
/// Emits at most one diagnostic per call.
#[allow(clippy::too_many_arguments)]
pub(crate) fn check_trait_arg_conformance(
    target_name: &str,
    arg_name: &str,
    arg_type: &Type,
    arg_call_name: Option<&str>,
    span: SourceSpan,
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Anti-cascade: if the arg itself had a compilation error, skip.
    if matches!(arg_type, Type::Error) {
        return;
    }

    // Look up the target template — skip if not found (external/forward-ref miss).
    let Some(target) = template_registry.get(target_name) else {
        return;
    };

    // Find the declared param cell for this arg name.
    let Some(cell) = target
        .value_cells
        .iter()
        .find(|vc| vc.id.member == arg_name)
    else {
        return; // Arg name not found — skip (positional arg or existing error).
    };

    // Only act when the param's declared type is a trait object.
    // TODO(follow-up): handle Option<TraitObject> and collection-typed trait params —
    // wrapping a trait type in Option or a collection currently bypasses call-site
    // conformance silently (known gap, not forgotten).
    let Type::TraitObject(required_trait) = &cell.cell_type else {
        return; // Non-trait param — no call-site type-check is performed in the compiler today.
    };

    // When the compiled arg_type defaulted to a numeric fallback (Real or Int)
    // from a FunctionCall expression and the callee is a known structure
    // template, promote to StructureRef so the conformance check can walk the
    // structure's trait bounds. Int appears when the callee's first arg is a
    // whole-number literal (e.g. `Steel(density: 1000.0)` — the literal 1000.0
    // is canonicalized to Int by the expression compiler).
    let promoted: Option<Type> = if matches!(arg_type, Type::Real | Type::Int) {
        arg_call_name
            .filter(|call_name| template_registry.contains_key(*call_name))
            .map(|call_name| Type::StructureRef(call_name.to_string()))
    } else {
        None
    };
    let effective_arg_type = promoted.as_ref().unwrap_or(arg_type);

    // Check conformance based on effective_arg_type.
    match effective_arg_type {
        Type::StructureRef(struct_name) => {
            // Look up the arg's structure template and walk its trait bounds.
            let Some(arg_template) = template_registry.get(struct_name.as_str()) else {
                return; // Arg structure not compiled yet — skip.
            };
            if !satisfies_trait_bound(&arg_template.trait_bounds, required_trait, trait_registry) {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "type '{}' does not conform to trait '{}' required by param '{}'",
                        struct_name, required_trait, arg_name
                    ))
                    .with_label(DiagnosticLabel::new(
                        span,
                        format!(
                            "type '{}' does not conform to trait '{}'",
                            struct_name, required_trait
                        ),
                    )),
                );
            }
        }
        Type::TraitObject(arg_trait_name) => {
            // Trait-object arg: check that arg_trait refines (or equals) required_trait.
            let mut visited = HashSet::new();
            if !trait_satisfies(arg_trait_name, required_trait, trait_registry, &mut visited) {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "type '{}' does not conform to trait '{}' required by param '{}'",
                        arg_trait_name, required_trait, arg_name
                    ))
                    .with_label(DiagnosticLabel::new(
                        span,
                        format!(
                            "trait '{}' does not refine trait '{}'",
                            arg_trait_name, required_trait
                        ),
                    )),
                );
            }
        }
        _ => {
            // Anti-cascade: when arg_type is a numeric fallback (Real or Int)
            // and arg_call_name is Some but the callee was not in the template
            // registry (so promotion returned None), an "undefined function"
            // diagnostic already fired for that unknown call. Emitting
            // "type 'real'/'int' does not conform to trait 'X'" here would be
            // misleading — the numeric type is the expression compiler's
            // fallback for unresolved calls, not the author's intended type.
            // Suppress.
            if matches!(arg_type, Type::Real | Type::Int) && arg_call_name.is_some() {
                return;
            }
            // Neither StructureRef nor TraitObject — cannot conform to a trait.
            // The original arg_type is used in the message (not the effective type,
            // which equals arg_type here since promotion didn't apply).
            diagnostics.push(
                Diagnostic::error(format!(
                    "type '{}' does not conform to trait '{}' required by param '{}'",
                    arg_type, required_trait, arg_name
                ))
                .with_label(DiagnosticLabel::new(
                    span,
                    format!("expected a type conforming to trait '{}'", required_trait),
                )),
            );
        }
    }
}

#[cfg(test)]
/// # Why these tests live here (and cannot move to `tests/*.rs`)
///
/// All four tests in this module call `pub(crate) check_trait_conformance` via
/// `use super::*;`.  Rust integration-test binaries in `tests/*.rs` are separate
/// crates and can only access `pub` (not `pub(crate)`) items, so none of these
/// tests can be moved to an integration-test file without also making
/// `check_trait_conformance` (and `MergeContext`, `collect_all_requirements`,
/// `check_trait_arg_conformance`) part of the public API — a non-trivial
/// architectural change that would require its own RFC-level task.
///
/// **Tests 1–2** (`check_trait_conformance_resolves_enum_typed_param_and_let`,
/// `option_b_fix_blocks_phantom_let_entry_for_pass2_skipped_name`) hand-build
/// `RequirementKind::Let` fixtures.  `RequirementKind::Let` is **not
/// parser-reachable** from reify source today (see `trait_merge_tests.rs:282`
/// and `let_type_disambiguation_tests.rs:234`), so there is no
/// `compile_source(...)` string that produces this variant.  An integration-level
/// rewrite is therefore impossible, not just inconvenient.
///
/// **Tests 3–4** (`enum_with_type_args_emits_error_diagnostic`,
/// `unknown_named_type_with_type_args_produces_unresolved_diagnostic`) assert an
/// **exact count of 1** on diagnostic substrings.  Under full-pipeline
/// compilation the same diagnostics are also emitted from `entity.rs:329` and
/// `traits.rs:36`, so a `compile_source`-based rewrite would see 2+ emissions
/// and break the exact-count assertions.  Relaxing to `any(...)` would lose the
/// path-specificity that makes these tests load-bearing (they pin that the
/// `conformance.rs:42` emission site fires in both debug and release builds).
///
/// **Closest integration-level siblings** that cover the *parser-reachable*
/// scenarios:
/// - `phantom_let_advertisement_contract_for_future_parser_extension`
///   (`tests/trait_merge_tests.rs:1445`)
/// - `reject_unresolved_type_in_trait_conformance`
///   (`tests/boundary1_consumer.rs:280`)
///
/// For full rationale and alternative paths (structural extraction,
/// test-only feature-flag API, `src/conformance_tests.rs` sibling module)
/// see the escalate_info record for task 2033.
mod tests {
    use super::*;

    /// Run `check_trait_conformance` against the given traits and structure, returning all
    /// diagnostics emitted.
    ///
    /// Centralises the ~20-line scaffolding (scope/value_cells/constraints init, registry
    /// construction, alias_registry, the call itself) that would otherwise be repeated
    /// verbatim in every conformance unit test.  Each test only needs to build its trait
    /// and structure fixtures and then assert on the returned `Vec<Diagnostic>`.
    fn run_conformance(
        traits: &[CompiledTrait],
        structure_def: &reify_syntax::StructureDef,
        enum_defs: &[reify_types::EnumDef],
    ) -> Vec<Diagnostic> {
        let entity_ref = EntityDefRef::from(structure_def);
        let trait_registry: HashMap<String, &CompiledTrait> =
            traits.iter().map(|t| (t.name.clone(), t)).collect();
        let trait_names: HashSet<String> = trait_registry.keys().cloned().collect();
        let mut scope = CompilationScope::new(&structure_def.name);
        let mut value_cells: Vec<ValueCellDecl> = vec![];
        let mut constraints: Vec<CompiledConstraint> = vec![];
        let mut constraint_index = 0u32;
        let functions: &[CompiledFunction] = &[];
        let alias_registry = TypeAliasRegistry::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];

        check_trait_conformance(
            &entity_ref,
            &trait_registry,
            &trait_names,
            &mut scope,
            &mut value_cells,
            &mut constraints,
            &mut constraint_index,
            enum_defs,
            functions,
            &alias_registry,
            &mut diagnostics,
        );

        diagnostics
    }

    /// Unit test for the Option B fix (task 1951).
    ///
    /// This test exercises the code path the integration-level
    /// `phantom_let_advertisement_contract_for_future_parser_extension` test in
    /// `trait_merge_tests.rs` CANNOT reach: it hand-builds a `RequirementKind::Let`
    /// requirement (not parseable from reify source today — see
    /// `let_type_disambiguation_tests.rs:470-497` and esc-1951-6) and verifies that
    /// the Option B guard in `available_defaults` suppresses the phantom
    /// `(name, Let) -> Type::Real` entry for names recorded in `pass2_skipped`.
    ///
    /// ## Scenario
    ///
    /// - **TraitX**: requires `let x : Length` (hand-built `RequirementKind::Let` — not
    ///   parser-reachable today)
    /// - **TraitY**: provides `param x : Length` — Pass 1 claims the scope slot for "x"
    /// - **TraitZ**: provides `let x = 5.5` (unannotated; `cell_type: None`) — Pass 2
    ///   sees the slot already claimed and records "x" in `pass2_skipped`
    /// - **Structure S : TraitX + TraitY + TraitZ { }** — no member override
    ///
    /// ## Expected behavior (post-fix)
    ///
    /// The `pass2_skipped.contains(name)` guard in the `DefaultKind::Let` arm of
    /// `available_defaults` returns `None` before reaching the `Type::Real` fallback.
    /// The `RequirementKind::Let` lookup for "x" finds no entry → the `None` arm fires →
    /// correct "missing required member" diagnostic (not the spurious "available default
    /// has Real" phantom type-mismatch).
    ///
    /// ## Pre-fix behavior (should NOT happen after fix)
    ///
    /// Without the guard, `available_defaults` contained `("x", Let) -> Type::Real`.
    /// The lookup found it, `implicitly_converts_to(Real, Length)` was false, and a
    /// spurious "requirement expects …, available default has Real" diagnostic was emitted.
    ///
    /// Characterization test that enum-typed `param` and `let` members resolve to
    /// `Type::Enum` through `check_trait_conformance`.
    ///
    /// Serves as a tripwire for the step-4 refactor (HashSet + closure extraction):
    /// any drift in enum resolution or diagnostic messages in the filter_map is caught
    /// immediately.
    ///
    /// ## Why negative assertions?
    ///
    /// `structure_members` is a local binding inside `check_trait_conformance` and is not
    /// directly observable from outside the function.  Rather than restructuring the API,
    /// this test uses three negative-assertion sentinels as a proxy for correct
    /// `Type::Enum("Direction")` resolution:
    ///
    /// - Absence of **"unresolved type"** → both `dir` and `kind` were resolved (not fallen
    ///   back to `Type::Real`)
    /// - Absence of **"type mismatch"** → the resolved types matched the trait's
    ///   `Type::Enum("Direction")` requirements
    /// - Absence of **"missing required member"** → both members appeared in `structure_members`
    ///
    /// Together these three imply `Type::Enum("Direction")` was produced.  A regression that
    /// accidentally resolves enum params to `Type::Real` would trip "type mismatch", and one
    /// that omits a member from `structure_members` would trip "missing required member".
    #[test]
    fn check_trait_conformance_resolves_enum_typed_param_and_let() {
        // Direction enum defined in the same module
        let enum_defs = vec![reify_types::EnumDef {
            name: "Direction".to_string(),
            variants: vec!["In".to_string(), "Out".to_string()],
        }];

        // TypeExpr for `Direction` (bare named type, no type_args)
        let direction_type_expr = reify_syntax::TypeExpr {
            kind: reify_syntax::TypeExprKind::Named {
                name: "Direction".to_string(),
                type_args: vec![],
            },
            span: SourceSpan::empty(0),
        };

        // TraitDir: requires `param dir : Direction` and `let kind : Direction`
        let trait_dir = CompiledTrait {
            name: "TraitDir".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![
                TraitRequirement {
                    name: "dir".to_string(),
                    kind: RequirementKind::Param(Type::Enum("Direction".to_string())),
                    span: SourceSpan::empty(0),
                },
                TraitRequirement {
                    name: "kind".to_string(),
                    kind: RequirementKind::Let(Type::Enum("Direction".to_string())),
                    span: SourceSpan::empty(0),
                },
            ],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // Structure S : TraitDir { param dir : Direction; let kind : Direction = 0.0; }
        let structure_def = reify_syntax::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![reify_syntax::TraitBoundRef {
                name: "TraitDir".to_string(),
                type_args: vec![],
                span: SourceSpan::empty(0),
            }],
            members: vec![
                reify_syntax::MemberDecl::Param(reify_syntax::ParamDecl {
                    name: "dir".to_string(),
                    doc: None,
                    type_expr: Some(direction_type_expr.clone()),
                    default: None,
                    where_clause: None,
                    annotations: vec![],
                    span: SourceSpan::empty(0),
                    content_hash: ContentHash(0),
                }),
                reify_syntax::MemberDecl::Let(reify_syntax::LetDecl {
                    name: "kind".to_string(),
                    doc: None,
                    is_pub: false,
                    type_expr: Some(direction_type_expr),
                    value: reify_syntax::Expr {
                        kind: reify_syntax::ExprKind::NumberLiteral(0.0),
                        span: SourceSpan::empty(0),
                    },
                    where_clause: None,
                    annotations: vec![],
                    span: SourceSpan::empty(0),
                    content_hash: ContentHash(0),
                }),
            ],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let diagnostics = run_conformance(&[trait_dir], &structure_def, &enum_defs);

        // No "unresolved type" → both dir and kind resolved successfully (to Type::Enum)
        let unresolved_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("unresolved type"))
            .collect();
        assert!(
            unresolved_diags.is_empty(),
            "Expected no 'unresolved type' diagnostics; got: {:?}",
            diagnostics
        );

        // No "type mismatch" → both resolved to Type::Enum("Direction"), satisfying the trait
        let mismatch_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("type mismatch"))
            .collect();
        assert!(
            mismatch_diags.is_empty(),
            "Expected no 'type mismatch' diagnostics; got: {:?}",
            diagnostics
        );

        // No "missing required member" → both dir and kind were found in structure_members
        let missing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("missing required member"))
            .collect();
        assert!(
            missing_diags.is_empty(),
            "Expected no 'missing required member' diagnostics; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn option_b_fix_blocks_phantom_let_entry_for_pass2_skipped_name() {
        // --- Build CompiledTrait fixtures ---

        // TraitX: requires `let x : Length` (hand-built — not parser-reachable)
        let trait_x = CompiledTrait {
            name: "TraitX".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "x".to_string(),
                kind: RequirementKind::Let(Type::length()),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // TraitY: `param x : Length` — no default expression needed.
        // Pass 1 registers "x" → Type::length() in the scope.
        let trait_y = CompiledTrait {
            name: "TraitY".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Param {
                    cell_type: Type::length(),
                    default_decl: reify_syntax::ParamDecl {
                        name: "x".to_string(),
                        doc: None,
                        type_expr: None,
                        default: None, // no default expression
                        where_clause: None,
                        annotations: vec![],
                        span: SourceSpan::empty(0),
                        content_hash: ContentHash(0),
                    },
                },
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // TraitZ: `let x = 5.5` (unannotated; cell_type: None).
        // Pass 2 compiles NumberLiteral(5.5) → Type::Real, finds "x" already in scope,
        // and records "x" in pass2_skipped (no inferred_let_exprs cache entry).
        let trait_z = CompiledTrait {
            name: "TraitZ".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Let {
                    cell_type: None,
                    let_decl: reify_syntax::LetDecl {
                        name: "x".to_string(),
                        doc: None,
                        is_pub: false,
                        type_expr: None,
                        value: reify_syntax::Expr {
                            kind: reify_syntax::ExprKind::NumberLiteral(5.5),
                            span: SourceSpan::empty(0),
                        },
                        where_clause: None,
                        annotations: vec![],
                        span: SourceSpan::empty(0),
                        content_hash: ContentHash(0),
                    },
                },
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // Structure S : TraitX + TraitY + TraitZ { } — no member overrides
        let structure_def = reify_syntax::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![
                reify_syntax::TraitBoundRef {
                    name: "TraitX".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
                reify_syntax::TraitBoundRef {
                    name: "TraitY".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
                reify_syntax::TraitBoundRef {
                    name: "TraitZ".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
            ],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let diagnostics = run_conformance(&[trait_x, trait_y, trait_z], &structure_def, &[]);

        // --- Assertion 1: no phantom type-mismatch diagnostic ---
        // Pre-fix: `available_defaults` had `("x", Let) -> Real`; the
        // RequirementKind::Let lookup found it, `implicitly_converts_to(Real, Length)` was
        // false, and a spurious "requirement expects …, available default has Real"
        // diagnostic was emitted.
        // Post-fix: no phantom entry → this filter collects nothing.
        let phantom_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.message.contains("available default")
                    && d.message.contains("Real")
                    && d.message.contains('x')
            })
            .collect();
        assert!(
            phantom_diags.is_empty(),
            "Option B fix violated: phantom `(x, Let) -> Type::Real` advertisement caused \
             a spurious type-mismatch diagnostic. Expected no phantom diagnostic. Got: {:?}",
            phantom_diags
        );

        // --- Assertion 2: correct "missing required member" diagnostic IS present ---
        // With the phantom entry absent, the None arm of the available_defaults lookup
        // fires and emits the correct "missing required member" diagnostic.
        let missing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("missing required member") && d.message.contains("x"))
            .collect();
        assert_eq!(
            missing_diags.len(),
            1,
            "Expected exactly one 'missing required member' diagnostic for 'x' (Option B fix). \
             Got: {:?}",
            diagnostics
        );
    }

    /// Test that a `param` annotation with `EnumName<T>` (non-empty type_args) emits a
    /// user-facing `Diagnostic::error` with the message
    /// "enum `Direction` does not accept type arguments".
    ///
    /// Unlike a `debug_assert!`, the diagnostic is emitted in both debug and release builds,
    /// so this test validates the error is always surfaced to users regardless of build profile.
    #[test]
    fn enum_with_type_args_emits_error_diagnostic() {
        // Direction<Something> — non-empty type_args that should trigger the diagnostic
        let bogus_type_arg = reify_syntax::TypeExpr {
            kind: reify_syntax::TypeExprKind::Named {
                name: "Something".to_string(),
                type_args: vec![],
            },
            span: SourceSpan::empty(0),
        };
        let direction_with_args = reify_syntax::TypeExpr {
            kind: reify_syntax::TypeExprKind::Named {
                name: "Direction".to_string(),
                type_args: vec![bogus_type_arg],
            },
            span: SourceSpan::empty(0),
        };

        let enum_defs = vec![reify_types::EnumDef {
            name: "Direction".to_string(),
            variants: vec!["In".to_string(), "Out".to_string()],
        }];

        let structure_def = reify_syntax::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![reify_syntax::MemberDecl::Param(reify_syntax::ParamDecl {
                name: "dir".to_string(),
                doc: None,
                type_expr: Some(direction_with_args),
                default: None,
                where_clause: None,
                annotations: vec![],
                span: SourceSpan::empty(0),
                content_hash: ContentHash(0),
            })],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let diagnostics = run_conformance(&[], &structure_def, &enum_defs);

        // Expect exactly one diagnostic reporting the type-args error.
        let type_args_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("does not accept type arguments"))
            .collect();
        assert_eq!(
            type_args_errors.len(),
            1,
            "Expected exactly one 'does not accept type arguments' diagnostic; got: {:?}",
            diagnostics
        );
    }

    /// A non-enum type name with non-empty type_args (e.g. `NotAnEnum<Something>`) should
    /// produce exactly one "unresolved type" diagnostic — the same outcome as `NotAnEnum`
    /// without type_args, because enum-resolution is gated on the name matching an enum.
    ///
    /// The positive assertion (`unresolved.len() == 1`) is the load-bearing check here:
    /// it verifies that an unknown parameterized type name falls through to the
    /// "unresolved type" diagnostic rather than silently resolving to `Type::Real` or
    /// emitting a spurious "does not accept type arguments" error.
    #[test]
    fn unknown_named_type_with_type_args_produces_unresolved_diagnostic() {
        // NotAnEnum<Something> — non-empty type_args but "NotAnEnum" is not in enum_defs
        let bogus_type_arg = reify_syntax::TypeExpr {
            kind: reify_syntax::TypeExprKind::Named {
                name: "Something".to_string(),
                type_args: vec![],
            },
            span: SourceSpan::empty(0),
        };
        let non_enum_with_args = reify_syntax::TypeExpr {
            kind: reify_syntax::TypeExprKind::Named {
                name: "NotAnEnum".to_string(),
                type_args: vec![bogus_type_arg],
            },
            span: SourceSpan::empty(0),
        };

        let enum_defs = vec![reify_types::EnumDef {
            name: "Direction".to_string(),
            variants: vec!["In".to_string(), "Out".to_string()],
        }];

        let structure_def = reify_syntax::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![reify_syntax::MemberDecl::Param(reify_syntax::ParamDecl {
                name: "p".to_string(),
                doc: None,
                type_expr: Some(non_enum_with_args),
                default: None,
                where_clause: None,
                annotations: vec![],
                span: SourceSpan::empty(0),
                content_hash: ContentHash(0),
            })],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        // Should NOT panic — "NotAnEnum" is not in enum_defs, so the enum-match arm
        // (where the debug_assert lives) is never taken.
        let diagnostics = run_conformance(&[], &structure_def, &enum_defs);

        // The unknown type produces an "unresolved type" diagnostic — not a panic.
        let unresolved: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("unresolved type"))
            .collect();
        assert_eq!(
            unresolved.len(),
            1,
            "Expected exactly one 'unresolved type' diagnostic"
        );
    }

    /// Pins the `inferred_let_exprs.get(name)` fallback at conformance.rs:358-363
    /// and the `Some(default_type) if implicitly_converts_to(...)` satisfaction arm
    /// at conformance.rs:406-410.
    ///
    /// `RequirementKind::Let` is not parser-reachable from reify source today
    /// (see `let_with_type_and_no_value_parses_as_empty_trait` and
    /// `let_type_disambiguation_tests.rs:470-497`), so only hand-built fixtures
    /// reach this path.
    ///
    /// ## Scenario
    ///
    /// - **TraitA**: requires `let x : Length` (hand-built `RequirementKind::Let` — not
    ///   parser-reachable)
    /// - **TraitB**: provides unannotated `let x = 80mm` (`DefaultKind::Let { cell_type: None,
    ///   let_decl.value: QuantityLiteral { 80.0, "mm" } }`) — Pass 2 infers `Type::length()`
    ///   and caches it in `inferred_let_exprs`
    /// - **Structure S : TraitA + TraitB { }** — no member overrides
    ///
    /// ## Expected behavior
    ///
    /// The `available_defaults` builder falls back to `inferred_let_exprs.get("x")`
    /// → `Type::length()`. The `Some(default_type) if implicitly_converts_to(...)` arm
    /// finds the types compatible → requirement satisfied → no diagnostics.
    #[test]
    fn inferred_let_expr_satisfies_let_requirement() {
        // TraitA: requires `let x : Length` (hand-built — not parser-reachable)
        let trait_a = CompiledTrait {
            name: "TraitA".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "x".to_string(),
                kind: RequirementKind::Let(Type::length()),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // TraitB: `let x = 80mm` (unannotated; cell_type: None).
        // Pass 2 compiles QuantityLiteral { value: 80.0, unit: "mm" } →
        // Type::Scalar { dimension: LENGTH } = Type::length(), finds "x" vacant in scope,
        // caches in inferred_let_exprs.
        let trait_b = CompiledTrait {
            name: "TraitB".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Let {
                    cell_type: None,
                    let_decl: reify_syntax::LetDecl {
                        name: "x".to_string(),
                        doc: None,
                        is_pub: false,
                        type_expr: None,
                        value: reify_syntax::Expr {
                            kind: reify_syntax::ExprKind::QuantityLiteral {
                                value: 80.0,
                                unit: "mm".to_string(),
                            },
                            span: SourceSpan::empty(0),
                        },
                        where_clause: None,
                        annotations: vec![],
                        span: SourceSpan::empty(0),
                        content_hash: ContentHash(0),
                    },
                },
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // Structure S : TraitA + TraitB { } — no member overrides
        let structure_def = reify_syntax::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![
                reify_syntax::TraitBoundRef {
                    name: "TraitA".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
                reify_syntax::TraitBoundRef {
                    name: "TraitB".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
            ],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let diagnostics = run_conformance(&[trait_a, trait_b], &structure_def, &[]);

        // A clean satisfaction path produces zero diagnostics.  Using is_empty() rather than
        // filtered substring checks means any unrelated upstream failure (e.g. a silent
        // compile_expr error) also trips this assertion — making it load-bearing beyond just
        // the two previously-checked categories ("type mismatch" / "missing required member").
        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics: inferred Type::length() should satisfy \
             RequirementKind::Let(Length) via the `Some(default_type) if \
             implicitly_converts_to(...)` arm at conformance.rs:406-410; got: {:?}",
            diagnostics
        );
    }

    /// Pins the `Some(default_type) =>` type-mismatch branch at conformance.rs:411-423
    /// for the `RequirementKind::Let` path when the inferred-let type is incompatible.
    ///
    /// `implicitly_converts_to(Type::Real, Type::length())` is false — `Real` and
    /// `Scalar { LENGTH }` are distinct types with no implicit conversion
    /// (type_compat.rs:3-96).
    ///
    /// ## Scenario
    ///
    /// Identical to `inferred_let_expr_satisfies_let_requirement` except the let
    /// expression is `ExprKind::NumberLiteral(5.5)` (inferred `Type::Real`)
    /// instead of `QuantityLiteral { 80.0, "mm" }`.
    ///
    /// ## Expected behavior
    ///
    /// `available_defaults` advertises `("x", Let) -> Type::Real` (via the
    /// `inferred_let_exprs.get("x")` fallback). The `Some(default_type) =>` arm
    /// fires → exactly one "type mismatch" + "available default" + "x" diagnostic.
    /// No "missing required member" for "x" (the default IS present in
    /// `available_defaults`, just with an incompatible type).
    #[test]
    fn inferred_let_expr_incompatible_with_let_requirement() {
        // TraitA: requires `let x : Length` (hand-built — not parser-reachable)
        let trait_a = CompiledTrait {
            name: "TraitA".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "x".to_string(),
                kind: RequirementKind::Let(Type::length()),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // TraitB: `let x = 5.5` (unannotated; cell_type: None).
        // Pass 2 compiles NumberLiteral(5.5) → Type::Real, finds "x" vacant in scope,
        // caches in inferred_let_exprs.
        let trait_b = CompiledTrait {
            name: "TraitB".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Let {
                    cell_type: None,
                    let_decl: reify_syntax::LetDecl {
                        name: "x".to_string(),
                        doc: None,
                        is_pub: false,
                        type_expr: None,
                        value: reify_syntax::Expr {
                            kind: reify_syntax::ExprKind::NumberLiteral(5.5),
                            span: SourceSpan::empty(0),
                        },
                        where_clause: None,
                        annotations: vec![],
                        span: SourceSpan::empty(0),
                        content_hash: ContentHash(0),
                    },
                },
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // Structure S : TraitA + TraitB { } — no member overrides
        let structure_def = reify_syntax::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![
                reify_syntax::TraitBoundRef {
                    name: "TraitA".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
                reify_syntax::TraitBoundRef {
                    name: "TraitB".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
            ],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let diagnostics = run_conformance(&[trait_a, trait_b], &structure_def, &[]);

        // Assertion 1: exactly one "type mismatch" + "available default" + "'x'" diagnostic.
        // Using "'x'" (quoted member name as it appears in the diagnostic template at
        // conformance.rs:415) rather than bare 'x' avoids false matches on words like
        // "expects" that also contain the character.  This pins the `Some(default_type) =>`
        // branch at conformance.rs:411-423.
        let mismatch: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.message.contains("type mismatch")
                    && d.message.contains("available default")
                    && d.message.contains("'x'")
            })
            .collect();
        assert_eq!(
            mismatch.len(),
            1,
            "expected exactly one type-mismatch diagnostic from the `Some(default_type) =>` \
             branch; got: {:?}",
            diagnostics
        );

        // Assertion 2: no "missing required member" for "'x'" (quoted, same rationale).
        // The inferred_let_exprs fallback advertised `("x", Let)` so the None arm was
        // never reached — the default IS present in available_defaults, just with an
        // incompatible type.
        assert!(
            diagnostics
                .iter()
                .filter(|d| d.message.contains("missing required member")
                    && d.message.contains("'x'"))
                .count()
                == 0,
            "negative case should hit the Some(default_type) arm, not the None arm; \
             got: {:?}",
            diagnostics
        );
    }

    /// Phase-contract test for `check_phase_resolve_structure_members`.
    ///
    /// Verifies that the helper correctly builds both the `structure_members`
    /// HashMap and the `structure_constraint_labels` HashSet from a minimal
    /// StructureDef fixture. This test fails to compile until the helper exists
    /// (TDD compile-tripwire) and pins the helper's return type signature.
    #[test]
    fn check_phase_resolve_structure_members_builds_member_and_constraint_maps() {
        let real_type_expr = reify_syntax::TypeExpr {
            kind: reify_syntax::TypeExprKind::Named {
                name: "Real".to_string(),
                type_args: vec![],
            },
            span: SourceSpan::empty(0),
        };
        let length_type_expr = reify_syntax::TypeExpr {
            kind: reify_syntax::TypeExprKind::Named {
                name: "Length".to_string(),
                type_args: vec![],
            },
            span: SourceSpan::empty(0),
        };

        let structure_def = reify_syntax::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![
                reify_syntax::MemberDecl::Param(reify_syntax::ParamDecl {
                    name: "width".to_string(),
                    doc: None,
                    type_expr: Some(real_type_expr),
                    default: None,
                    where_clause: None,
                    annotations: vec![],
                    span: SourceSpan::empty(0),
                    content_hash: ContentHash(0),
                }),
                reify_syntax::MemberDecl::Let(reify_syntax::LetDecl {
                    name: "length".to_string(),
                    doc: None,
                    is_pub: false,
                    type_expr: Some(length_type_expr),
                    value: reify_syntax::Expr {
                        kind: reify_syntax::ExprKind::NumberLiteral(0.0),
                        span: SourceSpan::empty(0),
                    },
                    where_clause: None,
                    annotations: vec![],
                    span: SourceSpan::empty(0),
                    content_hash: ContentHash(0),
                }),
                reify_syntax::MemberDecl::Constraint(reify_syntax::ConstraintDecl {
                    label: Some("bound".to_string()),
                    expr: reify_syntax::Expr {
                        kind: reify_syntax::ExprKind::NumberLiteral(1.0),
                        span: SourceSpan::empty(0),
                    },
                    where_clause: None,
                    span: SourceSpan::empty(0),
                    content_hash: ContentHash(0),
                }),
            ],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let entity_ref = EntityDefRef::from(&structure_def);
        let trait_names: HashSet<String> = HashSet::new();
        let alias_registry = TypeAliasRegistry::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let (structure_members, structure_constraint_labels) =
            check_phase_resolve_structure_members(
                &entity_ref,
                &trait_names,
                &[],
                &alias_registry,
                &mut diagnostics,
            );

        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics; got: {:?}",
            diagnostics
        );
        assert!(
            structure_members.contains_key("width"),
            "Expected 'width' in structure_members"
        );
        assert!(
            structure_members.contains_key("length"),
            "Expected 'length' in structure_members"
        );
        assert!(
            structure_constraint_labels.contains("bound"),
            "Expected 'bound' in structure_constraint_labels"
        );
    }

    /// Phase-contract test for `check_phase_collect_trait_bounds`.
    ///
    /// Verifies that the helper populates a MergeContext with the trait requirements
    /// from the structure's trait bounds. This test fails to compile until the helper
    /// exists (TDD compile-tripwire) and pins the helper's return type signature.
    #[test]
    fn check_phase_collect_trait_bounds_populates_ctx_requirements() {
        let trait_a = CompiledTrait {
            name: "TraitA".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "w".to_string(),
                kind: RequirementKind::Param(Type::Real),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        let structure_def = reify_syntax::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![reify_syntax::TraitBoundRef {
                name: "TraitA".to_string(),
                type_args: vec![],
                span: SourceSpan::empty(0),
            }],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let entity_ref = EntityDefRef::from(&structure_def);
        let trait_registry: HashMap<String, &CompiledTrait> =
            [("TraitA".to_string(), &trait_a)].into_iter().collect();
        let structure_members: HashMap<String, Type> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let ctx = check_phase_collect_trait_bounds(
            &entity_ref,
            &trait_registry,
            &structure_members,
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics; got: {:?}",
            diagnostics
        );
        assert_eq!(ctx.requirements.len(), 1, "Expected 1 requirement");
        assert_eq!(ctx.requirements[0].name, "w", "Expected requirement name 'w'");
    }

    /// Phase-contract test for `check_phase_pre_register_default_types`.
    ///
    /// Verifies that the helper registers an annotated Param default into the scope
    /// (Pass 1) and returns empty caches (no unannotated Let defaults to process).
    /// This test fails to compile until the helper exists (TDD compile-tripwire) and
    /// pins the helper's return type signature.
    #[test]
    fn check_phase_pre_register_default_types_registers_annotated_param_into_scope() {
        let param_decl = reify_syntax::ParamDecl {
            name: "x".to_string(),
            doc: None,
            type_expr: None,
            default: None,
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![TraitDefault {
            name: Some("x".to_string()),
            kind: DefaultKind::Param {
                cell_type: Type::Real,
                default_decl: param_decl,
            },
            span: SourceSpan::empty(0),
        }];

        let structure_members: HashMap<String, Type> = HashMap::new();
        let mut scope = CompilationScope::new("S");
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let (inferred_let_exprs, pass2_skipped) = check_phase_pre_register_default_types(
            &ctx,
            &structure_members,
            "S",
            &mut scope,
            &[],
            &[],
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics; got: {:?}",
            diagnostics
        );
        assert!(
            inferred_let_exprs.is_empty(),
            "Expected no inferred_let_exprs for a param-only context"
        );
        assert!(
            pass2_skipped.is_empty(),
            "Expected no pass2_skipped for a param-only context"
        );
        // Verify "x" was registered in scope: a second register_if_absent call for "x"
        // should find it occupied (Some(..)) — no direct lookup API needed.
        let conflict = scope.register_if_absent("x", Type::Int);
        assert!(
            conflict.is_some(),
            "Expected 'x' to be registered in scope (register_if_absent should find it occupied)"
        );
    }

    /// Phase-contract test for `check_phase_build_available_defaults_map`.
    ///
    /// Verifies that the helper builds a composite-keyed HashMap from ctx.defaults,
    /// including Param defaults and excluding Constraint defaults. This test fails to
    /// compile until the helper exists (TDD compile-tripwire) and pins the helper's
    /// return type signature.
    #[test]
    fn check_phase_build_available_defaults_map_uses_composite_key() {
        let param_decl = reify_syntax::ParamDecl {
            name: "x".to_string(),
            doc: None,
            type_expr: None,
            default: None,
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };
        let constraint_decl = reify_syntax::ConstraintDecl {
            label: Some("bound".to_string()),
            expr: reify_syntax::Expr {
                kind: reify_syntax::ExprKind::BoolLiteral(true),
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![
            TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Param {
                    cell_type: Type::Real,
                    default_decl: param_decl,
                },
                span: SourceSpan::empty(0),
            },
            TraitDefault {
                name: Some("bound".to_string()),
                kind: DefaultKind::Constraint(constraint_decl),
                span: SourceSpan::empty(0),
            },
        ];

        let inferred_let_exprs: HashMap<String, CompiledExpr> = HashMap::new();
        let pass2_skipped: HashSet<String> = HashSet::new();

        let available_defaults =
            check_phase_build_available_defaults_map(&ctx, &inferred_let_exprs, &pass2_skipped);

        assert_eq!(
            available_defaults.len(),
            1,
            "Expected exactly 1 entry (Param); Constraint should be filtered. Got: {:?}",
            available_defaults.keys().collect::<Vec<_>>()
        );
        assert!(
            available_defaults
                .contains_key(&("x".to_string(), AvailableDefaultKind::Param)),
            "Expected key ('x', Param) in available_defaults"
        );
        assert_eq!(
            available_defaults[&("x".to_string(), AvailableDefaultKind::Param)],
            Type::Real,
            "Expected Type::Real for key ('x', Param)"
        );
    }

    /// Phase-contract test for `check_phase_check_members_against_requirements`.
    ///
    /// Verifies that the helper emits a "missing required member" diagnostic when a
    /// structure satisfies neither the member directly nor a same-kind default.
    /// This test fails to compile until the helper exists (TDD compile-tripwire) and
    /// pins the helper's signature.
    #[test]
    fn check_phase_check_members_against_requirements_emits_missing_member_when_unsatisfied() {
        let structure_def = reify_syntax::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![], // No members — requirement "w" is unsatisfied
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };
        let entity_ref = EntityDefRef::from(&structure_def);

        let mut ctx = MergeContext::new();
        ctx.requirements = vec![TraitRequirement {
            name: "w".to_string(),
            kind: RequirementKind::Param(Type::Real),
            span: SourceSpan::empty(0),
        }];

        let structure_members: HashMap<String, Type> = HashMap::new();
        let available_defaults: HashMap<(String, AvailableDefaultKind), Type> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];

        check_phase_check_members_against_requirements(
            &ctx,
            &entity_ref,
            &structure_members,
            &available_defaults,
            &mut diagnostics,
        );

        assert_eq!(
            diagnostics.len(),
            1,
            "Expected 1 diagnostic for missing member 'w'; got: {:?}",
            diagnostics
        );
        assert!(
            diagnostics[0].message.contains("missing required member"),
            "Expected 'missing required member' in diagnostic; got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("'w'"),
            "Expected member name 'w' in diagnostic; got: {}",
            diagnostics[0].message
        );
    }

    /// Phase-contract test for `check_phase_inject_defaults`.
    ///
    /// Verifies that the helper injects a Param value cell when the structure does not
    /// override the default. The injected cell should have kind=Param, member="x",
    /// no constraints, and no diagnostics. This test fails to compile until the helper
    /// exists (TDD compile-tripwire) and pins the helper's signature.
    #[test]
    fn check_phase_inject_defaults_injects_param_cell_for_non_overridden_default() {
        let param_decl = reify_syntax::ParamDecl {
            name: "x".to_string(),
            doc: None,
            type_expr: None,
            default: None, // No default expression
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let structure_def = reify_syntax::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };
        let entity_ref = EntityDefRef::from(&structure_def);

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![TraitDefault {
            name: Some("x".to_string()),
            kind: DefaultKind::Param {
                cell_type: Type::Real,
                default_decl: param_decl,
            },
            span: SourceSpan::empty(0),
        }];

        let structure_members: HashMap<String, Type> = HashMap::new();
        let structure_constraint_labels: HashSet<String> = HashSet::new();
        let inferred_let_exprs: HashMap<String, CompiledExpr> = HashMap::new();
        let pass2_skipped: HashSet<String> = HashSet::new();
        let mut scope = CompilationScope::new("S");
        let mut value_cells: Vec<ValueCellDecl> = vec![];
        let mut constraints: Vec<CompiledConstraint> = vec![];
        let mut constraint_index: u32 = 0;
        let mut diagnostics: Vec<Diagnostic> = vec![];

        check_phase_inject_defaults(
            &ctx,
            &entity_ref,
            &structure_members,
            &structure_constraint_labels,
            inferred_let_exprs,
            &pass2_skipped,
            &mut scope,
            &mut value_cells,
            &mut constraints,
            &mut constraint_index,
            &[],
            &[],
            &mut diagnostics,
        );

        assert_eq!(
            value_cells.len(),
            1,
            "Expected 1 value cell for injected param 'x'; got: {:?}",
            value_cells
        );
        assert_eq!(
            value_cells[0].id.member, "x",
            "Expected cell member='x'; got: {}",
            value_cells[0].id.member
        );
        assert_eq!(
            value_cells[0].kind,
            ValueCellKind::Param,
            "Expected ValueCellKind::Param"
        );
        assert!(constraints.is_empty(), "Expected no constraints; got: {:?}", constraints);
        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics; got: {:?}",
            diagnostics
        );
    }
}
