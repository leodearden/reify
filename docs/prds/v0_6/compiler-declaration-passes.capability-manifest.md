# Capability manifest — compiler declaration passes (stream S9)

> **PRD:** `docs/prds/v0_6/compiler-declaration-passes.md`. **Status:** active — decomposed 2026-10-12 (pass 2), task ids stamped (α #8730 … π #8746; λ #8741 released 2026-10-13 with dependency #8830). Amended 2026-10-20 by the retro-critic sitting (R4, R6; `plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting`): ζ's check path widened, λ's two new checks, bindings for β, ζ, κ, λ and σ updated. The machine-readable twin is `compiler-declaration-passes.capability-manifest.yaml` (same bindings). Evidence was gathered on main `8e6ffcac9d` (2026-10-12) with the debug `reify` built 2026-10-11 (compiler source unchanged since `a030ee7528`) and a session-isolated tree-sitter cache. Code anchors cite by symbol; main moves fast — re-locate at implementation time.

**Probe fixtures (committed PRD evidence, `tests/prd-gate/fixtures/`):**

| fixture | tree-sitter parse | `reify check` on main |
|---|---|---|
| `decl_passes_forward_geometry_alias.ri` | rc 0 | rc 0 — `No constraints violated (1 indeterminate)` |
| `decl_passes_backward_geometry_alias.ri` | rc 0 | rc 1 — ``comparison `>` left operand must be a comparable kind, got `Geometry` `` |
| `decl_passes_guarded_port_dropped.ri` | rc 0 | rc 0 — `All constraints satisfied.` (port and meta dropped silently) |
| `decl_passes_purpose_body_sub_swallowed.ri` | rc 1 (ERROR at the `sub` line) | rc 0 — `All constraints satisfied.` (ERROR child swallowed) |

**No OPEN rows.** λ `relation-kind-enum` is bound to S8-ψ #9418 and S6 γ #8830 (retro-critic R6); σ `lowering-populates-spans` is bound to σ itself (syntax-lowering-contract R4). The ξ/ο S7 rows were wired at decompose (#8615, #8627, #8623).



## α — #8730

Construct-ratchet member for the declaration passes (R1-R3 at today's baselines)

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `infra-member-discovered-via-wrapper` | existing-substrate — tests/infra/run_all.sh discovers test_*.sh only and each wrapper takes a run-all-classification.manifest row (tests/infra/README.md "Auto-discovery"; reify CLAUDE.md "Infra tests"). The .py sibling is invisible without the .sh wrapper, so both land in one diff. | **PASS** | grep `test_compiler_decl_pass_ratchets\.sh` present in `tests/infra/run-all-classification.manifest` |
| `baselines-measured` | measured on main 8e6ffcac9d (2026-10-12) — R1 = 17 non-comment call lines of is_geometry_let( / classify_geometry_list_let( / is_selector_expr( in crates/reify-compiler/src/{entity.rs,guards.rs}; R2 = 4 non-comment "__connector_ literals in crates/*/src production code (connect.rs mint, engine_eval.rs x2, resolve_order.rs; its #[cfg(test)] use excluded); R3 = 10 `args.len() == 3` in relation_signatures.rs above its #[cfg(test)] module. | **PASS** | manual: the member's own hermetic flag/clear fixtures and its committed baselines are the evidence; a grep cannot assert a count. |

## β — #8731

Classify each let once — LetKind / MemberKinds, order-free

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `forward-alias-divergence-on-main` | probe (debug reify built 2026-10-11; compiler source unchanged since a030ee7528) — reify check tests/prd-gate/fixtures/decl_passes_forward_geometry_alias.ri rc 0 "No constraints violated (1 indeterminate)"; decl_passes_backward_geometry_alias.ri rc 1 "comparison `>` left operand must be a comparable kind, got `Geometry`". Cause read in compile_entity: pass 1 registers a not-yet-classified alias at Type::dimensionless_scalar(); passes 2/3 re-run is_geometry_let against the completed set. | **PASS** | manual: B1 asserts both fixtures give rc 1 and the identical error line after β. |
| `stratified-classifier-is-sound` | existing-substrate — is_selector_expr(expr, functions, known_selector_lets) takes no geometry set (geometry.rs), so selectors close first; is_geometry_let returns true for an Ident only via known_geometry_lets.contains and is otherwise independent of the geometry set, so it is monotone once the selector set is fixed; classify_geometry_list_let (geometry_list.rs) reads both final sets. A least fixpoint therefore exists and reproduces the cyclic case pinned by let_scope_tests::cyclic_ident_alias_does_not_crash. | **PASS** | — |
| `classifier-module` | producer-self (§4 D1) — new let_kind.rs with classify_members -> MemberKinds; absent on main. | **PASS** | grep `fn classify_members\b` present in `crates/reify-compiler/src` |
| `prologue-sets-gone` | producer-self — compile_entity's `let mut known_geometry_lets: HashSet<&str>` (and its three siblings) are replaced by one MemberKinds value. Retro-critic R4 (2026-10-20): the pattern has 2 hits today (compile_entity and build_structure_def_skeleton), so the check goes green only once β also converts the skeleton or #5494 has deleted it — the intended binding (D1). | **PASS** | grep `let mut known_geometry_lets:\s*HashSet` absent in `crates/reify-compiler/src/entity.rs` |
| `post-5425-predicates` | producer:task-5425 upstream (uniform-member-access β, pending) — reworks "geometry.rs geometry-let allowlist + wildcard"; β builds on its result. Wired as a real dependency. | **PASS** | — |

## γ — #8732

One member-support table; no silent member drops; purpose-body parse errors reported

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `guarded-silent-drop-on-main` | probe — reify check tests/prd-gate/fixtures/decl_passes_guarded_port_dropped.ri rc 0 "All constraints satisfied."; tree-sitter parse rc 0 (grammar commonMembers admits port/meta inside where{}); guards.rs::compile_guarded_members no-op arm covers Port/Connect/Chain/AssociatedType/Fn/MetaBlock/ConstraintInst. | **PASS** | manual: B3 asserts rc 1 with one Error per dropped member. |
| `purpose-body-error-swallowed-on-main` | probe — tests/prd-gate/fixtures/decl_passes_purpose_body_sub_swallowed.ri: tree-sitter parse rc 1 (ERROR on the sub line), reify check rc 0 "All constraints satisfied.". ts_parser.rs::lower_purpose_members iterates only purpose_member children; lower_constraint_def has the "ERROR" => push_fault_error_with_excerpt arm the purpose body lacks. | **PASS** | manual: B4 asserts rc 1 with a parse error at the sub line. |
| `corpus-unaffected` | measured 2026-10-12 — scan of every tracked .ri for port/meta/connect/chain/constraint-instantiation inside a where block: 0 hits (10 files contain where blocks). Rejecting those drops breaks no corpus file. | **PASS** | — |
| `member-support-module` | producer-self (§4 D2) — member_support.rs with member_support(kind, ctx) and one diagnostic function; absent on main. | **PASS** | grep `fn member_support\b` present in `crates/reify-compiler/src` |
| `duplicate-purpose-strings-gone` | producer-self — unsupported_purpose_member_info and its 11 duplicated literals are deleted. | **PASS** | grep `fn unsupported_purpose_member_info\b` absent in `crates/reify-compiler/src` |

## δ — #8733

ReflectiveQueryKind end to end

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `string-kind-on-main` | existing-substrate — ResolvedSchemaQuery.query_kind: String (reify-compiler types.rs), CompiledExprKind::PurposeReflectiveAggregation.query_kind: String (reify-ir expr.rs); engine_purposes.rs fallback else-arm returns Vec::new() for an unknown kind. | **PASS** | — |
| `matcher-home` | existing-substrate, shape correction — ValueCellKind, is_geometric_param_type and is_material_param_type live in reify-compiler types.rs (reify-eval imports them), so the cell matcher sits there while the enum sits in reify-ir (§4 D3). | **PASS** | — |
| `enum-present` | producer-self — enum ReflectiveQueryKind in reify-ir. | **PASS** | grep `enum ReflectiveQueryKind\b` present in `crates/reify-ir/src` |
| `string-field-gone` | producer-self — no String-typed query_kind field remains. | **PASS** | grep `query_kind:\s*String` absent in `crates/reify-ir/src`, `crates/reify-compiler/src` |
| `parity-baseline` | captured on main 8e6ffcac9d — reify check: stdlib_sim_ready_geom_undef.ri rc 0 sha256 0f21fa69f791…, stdlib_sim_ready_material_active.ri rc 0 c1489f4636e6…, stdlib_sim_ready_material_ok.ri rc 0 2b91836a8e7a…, examples/determinacy_intrinsics.ri rc 0 fafa4b689743…, examples/m5_purpose.ri rc 0 "All constraints satisfied." | **PASS** | manual: B5 asserts byte-identical output against this capture. |

## ε — #8734

Typed PortPath for deferred forall-connect endpoints

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `literal-index-rewrite-on-main` | existing-substrate (read, not probed — the deferred path is reachable only through Engine::edit_param) — engine_edit.rs::rewrite_port_placeholder doc "Substring-rewrite divergence": a user-written vents[0].foo becomes vents[i].foo for i > 0; types.rs CompiledForallBody::Connect doc restates it. | **PASS** | manual: B7 is written RED first in crates/reify-eval/tests/forall_runtime_re_elaboration.rs (its existing Connect fixture is the shape to extend). |
| `rewriter-gone` | producer-self — rewrite_port_placeholder deleted; PortPath::at_element is the only materialiser. | **PASS** | grep `fn rewrite_port_placeholder\b` absent in `crates/reify-eval/src` |
| `template-strings-gone` | producer-self — CompiledForallBody::Connect carries PortPath, not String templates. | **PASS** | grep `left_port_template:\s*String` absent in `crates/reify-compiler/src` |
| `done-task-citations` | fused-memory get_statuses 2026-10-12 — #2629, #2690, #2717 all done; forall_elaborate.rs Diagnostic::info strings cite them as future scope. | **PASS** | — |

## ζ — #8735

SubOrigin on SubComponentDecl

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `prefix-recognisers-on-main` | existing-substrate — one mint (connect.rs format!("__connector_{}")) and three starts_with recognisers (engine_eval.rs connector_pin_if_determined and is_strict_connector_instance_auto; resolve_order.rs connector-first loop); SubComponentDecl has no origin field. | **PASS** | — |
| `user-named-connector-accepted` | probe — `sub __connector_0 = Gain()` compiles and evaluates rc 0 with no reserved-name diagnostic; a parent-constrains-child-auto probe gave identical output for __connector_0 and plain_0, so B9 is asserted structurally (origin == Declared). Corrected by retro-critic R4 M2 (2026-10-20): beside a connector connect the user sub collides with the minted name and silently changes the connector's solved value (10mm -> 3mm); ζ adds a reserved-prefix Error (or a mint that skips taken names) and boundary row B9b pins it. | **PASS** | — |
| `origin-field` | producer-self — SubOrigin enum and SubComponentDecl.origin. | **PASS** | grep `enum SubOrigin\b` present in `crates/reify-compiler/src` |
| `recognisers-gone` | producer-self — no production starts_with("__connector_") remains (R2 = 1). Path widened to crates/reify-eval/src by retro-critic R4 (the third recogniser is in resolve_order.rs). | **PASS** | grep `starts_with\("__connector_"\)` absent in `crates/reify-eval/src` |

## η — #8736

Conformance tests through the conformance entry, not the phases

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `phase-pinned-tests-on-main` | measured — conformance/mod.rs test region: 79 #[test], 19 direct check_phase_*( calls, 19 check_fn_arg_conformance( calls; module doc says "All four tests" and points at an escalate_info record. A run_conformance harness exists in the module. | **PASS** | — |
| `module-doc-stale-claim-gone` | producer-self — the stale module doc is replaced by a pointer. | **PASS** | grep `All four tests in this module` absent in `crates/reify-compiler/src/conformance` |

## θ — #8738

DefaultDisposition replaces the four skip sets

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `skip-sets-on-main` | existing-substrate — PreRegisterOutput { pass1_skipped, pass1_param_skipped, pass2_skipped, pass2_compile_errors } threaded as four &HashSet<String> params into both check_phase_build_available_defaults_map and check_phase_inject_defaults. | **PASS** | — |
| `sets-gone` | producer-self — one DefaultDisposition per (name, kind). | **PASS** | grep `pub pass1_param_skipped:` absent in `crates/reify-compiler/src/conformance` |
| `behaviour-pinned-first` | producer:η upstream (in-batch) — B10's source-driven cases exist and are green before θ restructures. | **PASS** | — |

## ι — #8739

conformance_accepts beside type_compatible

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `divergence-on-main` | existing-substrate — walk_param_against_arg_type Vector arm has `Type::Tensor { rank: 1, .. } => true` while type_compat.rs type_compatible Rule 1a requires vn == tn; 4 arms defer via reject_if_incompatible, 5 encode own rules. | **PASS** | — |
| `relation-module` | producer-self — type_acceptance.rs with conformance_accepts. | **PASS** | grep `fn conformance_accepts\b` present in `crates/reify-compiler/src` |

## κ — #8740

One gated DOF entry point

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `gate-outside-module-on-main` | existing-substrate — compile_builder/entities_phase.rs §(4) composes dof_kind_of, the Type::Error skip, E_ARG_TYPE_MISMATCH, body_has_undecidable_kind_split, residual_kinds, declared_kinds and check_joint_dof; the module's docs assign gating to "the caller" twice. | **PASS** | — |
| `single-entry` | producer-self — joint_dof_diagnostics is the module's entry point and entities_phase calls only it. Retro-critic R4 polarity (2026-10-20): the pattern also matches a comment (entities_phase.rs `// … (see joint_self_check::body_has_undecidable_kind_split)`); κ deletes or rewords that comment in the same diff, or the check stays red after a correct implementation (κ's record carries the note). | **PASS** | grep `joint_self_check::(residual_kinds\|declared_kinds\|dof_kind_of\|body_has_undecidable_kind_split\|check_joint_dof)\b` absent in `crates/reify-compiler/src/compile_builder` |

## λ — #8741

RelationSig table keyed by RelationKind

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `tables-on-main` | measured — RELATION_FN_NAMES doc "**9 names**" over a 10-entry slice; 10 `args.len() == 3` guards above #[cfg(test)]; the ΔDOF-sum test iterates its own curated list. | **PASS** | — |
| `relation-kind-enum` | producer:#9418 (S8-ψ: RelationKind is reify-builtins' generated Relation binding-kind sub-enum, minted by registry! from the 12 relation rows) bound by #8830 (S6 γ: reify-constraints and reify-eval take reify-builtins as a normal dependency; RelationInstance.kind: reify_builtins::RelationKind set at reify-eval relate_solve::relation_instance via lookup(name, argc), per S6 ruling R4; no from_name). Retro-critic R6 (2026-10-20) supersedes the reify-ir RelationKind. Wired as real dependencies of #8741 (#8830 since 2026-10-13; #9418 since 2026-10-20). | **PASS** | — |
| `table-present` | producer-self — RelationSig { operands, metric_slot } returned by the total fn relation_sig(RelationKind) (retro-critic R6). | **PASS** | grep `struct RelationSig\b` present in `crates/reify-compiler/src` |
| `relation-names-derive-from-rows` | producer-self (retro-critic R6) — RELATION_FN_NAMES is gone; relation names derive from the reify-builtins rows (lookup(name, argc) is the one parser). | **PASS** | grep `RELATION_FN_NAMES: &\[&str\] = &\[` absent in `crates/reify-compiler/src/relation_signatures.rs` |
| `relation-sig-reads-the-registry` | producer-self (retro-critic R6) — arity and result type are read from the reify-builtins row (row_for(BuiltinId::from(kind))), never re-declared. | **PASS** | grep `reify_builtins::` present in `crates/reify-compiler/src/relation_signatures.rs` |

## μ — #8742

ParseErrorKind on ParseError

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `textual-discriminator-on-main` | existing-substrate — reify-ast ParseError { message, span }; member_continuation.rs::is_member_continuation_message = message.contains(MESSAGE_HEAD); 4 consumer test files (reify-cli, reify-compiler x2, reify-syntax); module_dag.rs maps errors via Diagnostic::error(e.message.clone()) (span dropped). | **PASS** | — |
| `kind-field` | producer-self — ParseErrorKind enum on ParseError. | **PASS** | grep `enum ParseErrorKind\b` present in `crates/reify-ast/src` |
| `substring-predicate-gone` | producer-self. | **PASS** | grep `fn is_member_continuation_message\b` absent in `crates/reify-syntax/src` |

## ν — #8743

TopologyTemplate::all_value_cells() (G4 seam for S27)

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `no-accessor-on-main` | existing-substrate — no fn all_value_cells in crates (git grep 2026-10-12, only test names); CompiledGuardedGroup { members, else_members, parent_guard } stored flat on TopologyTemplate.guarded_groups. | **PASS** | — |
| `accessor-present` | producer-self. | **PASS** | grep `fn all_value_cells\b` present in `crates/reify-compiler/src/types.rs` |

## ξ — #8744

h14 partition of entity.rs

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `partition-measured` | record h14_measurements entity.rs — stays_whole false, 7 named partitions; the match-arm group's symptom is the `use super::*` reach-back that S7 removes. | **PASS** | — |
| `s7-explicit-imports` | producer:task-8615 (S7 β, member_cell_decls.rs) and producer:task-8627 (S7 θ, entity.rs explicit imports), docs/prds/v0_6/compiler-module-namespace.md; wired as real dependencies of ξ at decompose 2026-10-12. | **PASS** | — |

## ο — #8745

h14 partition of conformance/mod.rs

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `partition-measured` | record h14_measurements conformance/mod.rs — stays_whole false, 6 partitions; walker <-> geometry-leaf cycle broken by moving WalkCtx/diag_at to a lower module. | **PASS** | — |
| `s7-explicit-imports` | producer:task-8623 (S7 ζ, conformance/ explicit imports), docs/prds/v0_6/compiler-module-namespace.md; wired as a real dependency of ο at decompose 2026-10-12. | **PASS** | — |

## π — #8746

PRD close — SHIPPED header, landed ids, freeze map

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `terminal-stamp` | overlay "PRD terminal status" — closed vocabulary; the close leaf depends on every sibling. | **PASS** | grep `^\*\*Status:\*\* \*\*SHIPPED` present in `docs/prds/v0_6/compiler-declaration-passes.md` |

## ρ — #9321 (gap-fill 2026-10-16, K2-D2)

reify-ast child enumeration: Expr::for_each_child(_mut), MemberDecl::for_each_direct_expr, MemberDecl::child_scopes; walk_members over child_scopes; ratchet row

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `hand-rolled-enumerations-on-main` | existing-substrate — at main 04be9302ea reify-ast has walk_members over MemberRecursionSet but no Expr child enumeration; eight independent child enumerations in reify-lsp (references/walk.rs ×3, references/local.rs ×2) and reify-compiler (entity.rs for_each_child/_mut, dot_chain_lint.rs and shadow_lint.rs walk_expr_depth); walk.rs binds `derivation: _`. | **PASS** | — |
| `expr-child-enumeration` | producer-self — Expr::for_each_child (+ _mut), wildcard-free over ExprKind. | **PASS** | grep `fn for_each_child\b` present in `crates/reify-ast/src` |
| `member-child-scopes` | producer-self — MemberDecl::child_scopes (incl. derivation.members) and MemberDecl::for_each_direct_expr (incl. derivation operands, override values/guards, using_plane); walk_members over child_scopes. | **PASS** | grep `fn child_scopes\b` present in `crates/reify-ast/src` |
| `hand-rolled-match-ratchet` | producer-self — ratchet row counting hand-rolled ExprKind/MemberDecl child matches outside reify-ast (INV candidate 5). | **PASS** | manual: a grep cannot assert a count; the ratchet member's hermetic fixtures are the evidence. |

## σ — #9322 (gap-fill 2026-10-16, K2-D2)

SpannedIdent for declared and referenced names, field by field; name_token_span and the empty-span conventions deleted

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `names-rescanned-on-main` | existing-substrate — SubDecl.name / structure_name are String; SpannedIdent exists in reify-core (SubDerivation, index_binder); decl_oracle.rs::name_token_span rescans source (49 references in 7 reify-lsp files); FunctionCall callee and TypeParamDecl.bounds carry no span. | **PASS** | — |
| `lowering-populates-spans` | producer-self — syntax-lowering-contract ruling R4 (2026-10-18, docs/prds/v0_6/syntax-lowering-contract.md §4: "No leaf here") gives σ the fields, their population at the ts_parser.rs lowering sites and the tests; there is no σ ← lowering-leaf edge to wait for. G4 row owner: S9 σ. (Was OPEN on the not-yet-filed stream at 2026-10-16; corrected per retro-critic R4 drift.) | **PASS** | — |
| `name-rescan-deleted` | producer-self — name_token_span and the three empty-span conventions deleted once every declared and referenced name carries its span. | **PASS** | grep `fn name_token_span\b` absent in `crates/reify-lsp/src` |

## ε₂ — #9323 (gap-fill 2026-10-16, K9-D1)

compile_connection keeps the typed PortPath; own vs sub-member endpoint classified from PortRoot

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `string-reclassification-on-main` | existing-substrate — at main aabecfe241 connect.rs::resolve_port_name flattens the parsed PortPath to a String; own_port_name and sub_member_endpoint re-classify it with strip_prefix/contains/rsplit_once/split_once; ε #8734 typed only the deferred forall-connect path. | **PASS** | — |
| `own-port-string-classifier-gone` | producer-self. | **PASS** | grep `fn own_port_name\b` absent in `crates/reify-compiler/src/connect.rs` |
| `sub-member-string-classifier-gone` | producer-self. | **PASS** | grep `fn sub_member_endpoint\b` absent in `crates/reify-compiler/src/connect.rs` |
| `unindexed-collection-endpoint-diagnosed` | ported from #7659 (subsumed) — `connect vents.outlet -> hub.feed` with `sub vents : List<Vent>` compiles clean today with a phantom Satisfied compat constraint; after ε₂ it is diagnosed with the per-element form suggested. | **PASS** | manual: behavioural; connect_compile_tests.rs and the PRD's gap-fill boundary rows are the cover. |
