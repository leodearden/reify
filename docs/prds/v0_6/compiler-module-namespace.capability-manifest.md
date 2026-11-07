# Capability manifest — `compiler-module-namespace.md`

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/compiler-module-namespace.md` (program
`review-all-reify-20261010`, stream S7). Drafted in author mode (pass 1), 2026-10-12.
Machine-readable twin: `docs/prds/v0_6/compiler-module-namespace.capability-manifest.yaml`
(task ids `null` until `commit_planning` stamps them).

> **Code anchors** verified against main `8e6ffcac9d` (2026-10-12); every S7 anchor path is
> byte-identical to the program anchor `a030ee7528`. Cite-by-symbol throughout; every
> `delivered_check` is pattern-anchored and was confirmed to be **red today** (an `absent`
> pattern that matches at main, or a `present` pattern / `path` that does not), so none is
> green on the day it is written.

## Batch-wide bindings

| Check | Evidence | Verdict |
|---|---|---|
| No novel grammar | No syntax introduced; `grammar_confirmed=true` batch-wide. The two new fixtures `tests/prd-gate/fixtures/s7_resolution_alias_cycle.ri` and `tests/prd-gate/fixtures/s7_resolution_enum_in_option.ri` parse with `tree-sitter parse --quiet` from `tree-sitter-reify/`, exit 0, 0 ERROR nodes (2026-10-12) | **PASS** |
| No language surface change | No builtin, diagnostic, CLI or GUI behaviour added or altered → docs-truth gate does not fire | **PASS** |
| Behaviour floor exists | reify-compiler's integration harnesses, `examples_smoke`, the prd-gate corpus and the workspace build are already gate members; every leaf's done-criteria forbid editing an expectation (PRD §5 row 1) | **PASS** |
| Fixture outputs recorded from real runs | `target/debug/reify check` (binary built 2026-10-11 19:33, after the last reify-compiler source commit `3bfae4d2e0`, 2026-10-10): `s7_resolution_alias_cycle.ri` → rc 1, `error: circular type alias 'C1'`, `error: unresolved type: C1`; `s7_resolution_enum_in_option.ri` → rc 0, `All constraints satisfied.`; `docs/prds/v0_6/fixtures/member_enum_ctor.ri` → rc 0, `OK Test#constraint[0]` (+ `W_MODULE_DECL_MISSING`); `docs/prds/v0_6/fixtures/shadow_payload_binder.ri` → rc 0 | **PASS** |
| New fixtures are inert | Both new fixtures are new basenames in `tests/prd-gate/fixtures/`, in neither `_RUST_COUPLED_RI_FIXTURES` nor `_GUI_COUPLED_RI_FIXTURES`; adding a fixture does not escalate pre-commit | **PASS** |
| Gate-test drift-guard | α and λ add `tests/infra/test_*.sh` wrappers; each leaf carries its own `run-all-classification.manifest` row in the same diff (overlay same-diff rule); no wall-clock bound; Python stdlib `unittest` only | **PASS** (same-diff obligation recorded on α, λ) |
| G7 (reify INV-SF/INV-AD/INV-PD) | Advisory in author mode: no new `Undef`, diagnostic, placeholder, parser or angle surface. μ removes an install-by-omission hazard (INV-SF-3 shape) | **PASS**, no waiver |

## Per-leaf bindings

### α — Namespace ratchet member
| Capability | Evidence | Verdict |
|---|---|---|
| infra discovery reaches a `.py` only through a `.sh` wrapper + manifest row | reify `CLAUDE.md` §Infra tests; `tests/infra/test_flake_density_report.{py,sh}` is the landed pair | PASS |
| counts are real today: 28 visibility-qualified globs, 29 unqualified column-0 globs | `git grep -cE '^pub\(crate\) use [a-z_]+::\*;' crates/reify-compiler/src/lib.rs` = 28; 25 top-level `^use super::\*;` + `conformance/mod.rs` (`use super::*;`, `use checker::*;`) + `conformance/sub_component_validation.rs` + `conformance/checker.rs` (`use crate::*;`) = 29 | PASS |
| delivered | `path` exists: `tests/infra/test_compiler_namespace_ratchet.sh` (absent today) | — |

### β — guards/traits/forall_elaborate/geometry_list; entity↔guards cycle
| Capability | Evidence | Verdict |
|---|---|---|
| the cycle is real | `entity.rs` → guards fns: 6 call sites; `guards.rs` → `build_param_value_cell_decl`, `build_auto_let_value_cell_decl`, `resolve_auto_let_cell_type` ×2, `fixup_option_none_for_let`, `fixup_option_none_for_param`, **[Amended 2026-10-20, retro-critic R8 M4]** `priv_flag_to_visibility` (the sixth) | PASS |
| the helpers can move down without reaching back | their callees: `annotations::{validate_annotations, lower_annotations, extract_solver_hints, validate_solver_hint_collections}`, `annotations/display.rs::validate_display_dimension`, `crate::scope` (`&CompilationScope`, R8 M4), the resolver, and `entity.rs::reject_keyed_value_position` (moves with them) | PASS |
| β separates entity and guards in the SCC graph | **[Amended 2026-10-20, retro-critic R8 B1 / R10]** no: β removes the direct edge only; guards still reaches entity via type_resolution, geometry → conformance, geometry_list and compile_builder::sketch_unsupported. "Apart" is delivered by σ #9419 and measured (R10) | superseded by σ |
| delivered | `^use super::\*;` absent in `guards.rs`; `mod member_cell_decls;` present in `lib.rs` | — |

### γ — eight-file slice; termination helpers private
| Capability | Evidence | Verdict |
|---|---|---|
| the 8 files glob-import | `git grep -l '^use super::\*;'` lists coerce, datum_projection, diagnostics, geometry_boolean, geometry_curve, geometry_transform, ice, termination | PASS |
| 3 termination helpers have no outside caller | `termination_args_contain_undef` 0, `termination_collect_refs` 0, `termination_is_modifying` 0; `check_recursive_termination` 1 (`post_passes.rs`) | PASS |
| delivered | `^use super::\*;` absent across the 8 paths | — |

### δ — type_compat split; facade deleted
| Capability | Evidence | Verdict |
|---|---|---|
| facade consumers are exactly 3 modules | `expr.rs`, `compile_builder/entities_phase.rs`, `variant_construct.rs` (+ `type_compat.rs` itself) | PASS |
| partition passes h13 | h14 record (seat measurement): binop group calls none of the other three groups; overload group and unify group import `reify_core` downward | PASS |
| facade check anchored on the facade, not the private import | **[Amended 2026-10-20, retro-critic R8 M3]** the retained `constraint_arg_type_conforms` needs a private `use reify_core::overload::{type_carries_trait_object, type_carries_type_param};`, which the old pattern `use reify_core::overload::\{type_carries_trait_object` would match; the per-consumer criteria (each of `expr.rs`, `compile_builder/entities_phase.rs`, `variant_construct.rs` imports `type_carries_*` from `reify_core::overload`; no `type_carries` inside a `use crate::type_compat::{…}` list) replace the vacuous `git grep 'type_compat::type_carries'` | PASS |
| delivered | `^pub\(crate\) use reify_core::overload::` absent in `type_compat.rs`; `^use super::\*;` absent in `type_compat.rs`; `path` `crates/reify-compiler/src/overload_resolution.rs` exists | — |

### ε — builtin_families.rs
| Capability | Evidence | Verdict |
|---|---|---|
| catalogue/units boundary | `units.rs`: catalogue 1–1,695, `unit_to_scalar` at 1,696, `UnitEntry` 1,711, `UnitRegistry` 1,748, `UnitResolveError` 1,799, `resolve_unit_expr` 1,864, tests 1,911 (main `8e6ffcac9d`) | PASS |
| public paths survive | `lib.rs` `pub use units::{…}` names 12 catalogue items (10 `*_NAMES` tables, `geometry_query_result_type`, `topology_selector_result_type`) + 4 unit items; retargeting the 12 keeps `reify_compiler::<NAME>` | PASS |
| delivered | `path` `crates/reify-compiler/src/builtin_families.rs` exists; `const GEOMETRY_FUNCTION_NAMES` absent in `units.rs` | — |

### ζ — remaining shared modules and conformance/
| delivered | `^use (super|crate)::\*;|^use checker::\*;` absent across the 11 paths | — |
|---|---|---|

### η — type_resolution.rs imports
| delivered | `^use super::\*;` absent in `type_resolution.rs` | — |
|---|---|---|

### θ — expr.rs and entity.rs imports
| Capability | Evidence | Verdict |
|---|---|---|
| family predicates live in units.rs today | `is_geometry_query`, `is_dynamics_query`, `is_field_op`, `is_fea_envelope_query` defined in `units.rs` (skeptic of fk-6460b80f5442) | PASS |
| delivered | `^use super::\*;` absent in `expr.rs` and `entity.rs` | — |

### ι — expr.rs partition (5 of 6)
| Capability | Evidence | Verdict |
|---|---|---|
| member-access helpers do not recurse | `compile_expr*` calls inside `member_access_on_*`, `resolve_datum_projection_or_poison`, `try_*cross_sub_geometry*`, `resolve_cluster_inner_member`: 0; the MemberAccess arm body itself has 2 (receiver, index) and stays in `expr.rs` | PASS |
| partition symbols present | poison helpers (`make_poison_literal` …), reserved names (`DETERMINACY_PREDICATE_NAMES` …), engagement (`Engagement` … `push_down_expected_for_empty_coll`), tests from the first top-level `#[cfg(test)]` and `member_access_completeness_canary` | PASS |
| delivered | `path` `crates/reify-compiler/src/member_access.rs` exists; `fn make_poison_literal\(` absent in `expr.rs` | — |

### κ — delete the root globs; clippy deny
| Capability | Evidence | Verdict |
|---|---|---|
| clippy runs `-D warnings` in the gate | `scripts/verify.sh` lint plan: `cargo clippy … --all-targets -- -D warnings` | PASS |
| `wildcard_imports` exempts `use super::*` in `#[cfg(test)]` modules by default | clippy lint documentation; κ re-verifies on the real tree and allows only `pub use types::*` if the lint reports it | OPEN (κ verifies) |
| root-path consumers beyond globs are few | `use crate::{CompiledFunction, compile_expr}` in `annotations/materialize.rs`; other `crate::<item>` uses resolve to `lib.rs`-defined fns or `types::*` | PASS |
| internal crate-root re-export paths (third C7 count) | **[Amended 2026-10-20, retro-critic R8 M2]** 122 `crate::<root pub use or types glob item>` paths in 25 files at main; κ drives the count to 0 | PASS (κ carries it) |
| delivered | `^pub\(crate\) use [a-z_]+::\*;` absent in `lib.rs`; `deny\(([^)]*,\s*)?clippy::wildcard_imports` present in `lib.rs` (anchored on the deny attribute, R8 polarity) | — |

### λ — Type::children / any_descendant; adoption ratchet
| Capability | Evidence | Verdict |
|---|---|---|
| no shared traversal exists | no `fn children`/`fn any_descendant`/`fn walk` in `reify-core/src/ty.rs` | PASS |
| the three predicates share one classification | `type_carries_type_param`, `type_carries_dim_param`, `type_mentions_conflicted_param` list identical arm groups, differing only in the leaf arm | PASS |
| completeness test substrate | `TypeDiscriminants` derives `strum::EnumIter`, `strum::EnumCount` (reify-core `ty.rs`) | PASS |
| ratchet count real today | `Type::Projection { base, .. } => <fn>(base` = 4 in production (overload.rs ×3, type_compat.rs ×1) | PASS |
| delivered | `fn any_descendant` present in `reify-core/src/ty.rs`; `=> type_carries_type_param\(base\)` absent in `overload.rs`; `path` `tests/infra/test_type_walker_adoption_ratchet.sh` exists | — |

### μ — ResolutionCtx
| Capability | Evidence | Verdict |
|---|---|---|
| three thread-locals, three guards | `type_resolution.rs` `thread_local!` blocks; `EnumNameScope`, `LocalEnumShadowScope`, `AliasDeferScope` | PASS |
| install sites | `EnumNameScope::new` ×5 production (`defs_phase`, `entity.rs`, `functions.rs` ×2, `validate_pub_parametric_alias_def_site`); `LocalEnumShadowScope::new` ×1 (`lib.rs`) + 7 test sites; `AliasDeferScope::new` ×1 | PASS |
| behaviour pins observed today | the four fixture rows in the batch-wide table; `harness_langcore/enum_ctor_param_binding_tests.rs` | PASS |
| behaviour pins byte-identical | **[Amended 2026-10-20, retro-critic R8 M5]** bound: the two s7 fixtures plus the M1 guarded-let and trait-conformance `Option<E>` cases become harness tests asserting exit code and diagnostics, landed before μ's rewrite (PRD §5 rows 6–7, 11–12); nothing in the gate reads `tests/prd-gate/fixtures` | OPEN |
| install-site context flows through its dynamic extent | **[Amended 2026-10-20, retro-critic R8 M1]** `compile_entity` → guards, member_cell_decls, conformance, forall elaboration; `phase_constraint_defs` → `compile_constraint_def`; the alias def-site check; modules gain `conformance/mod.rs`, `member_cell_decls.rs`, `compile_builder/aliases_phase.rs`, `compile_builder/reserved_name_lint.rs` | PASS |
| upstream tasks | #6955 (pending; adds a shadow-set reader); #5493 (pending; owns the install-site fold comment); **[Amended 2026-10-20, retro-critic R8 M6]** #8013 (fallback table shape, lazy `defs_phase` install) — real dependency edges | PASS (edges wired) |
| delivered | `thread_local!` absent in `type_resolution.rs`; `struct ResolutionCtx` present under `crates/reify-compiler/src` | — |

### ν — SubFacts
| Capability | Evidence | Verdict |
|---|---|---|
| 8 `sub_*` fields, two keyings, two pre-pass sites | `scope.rs` struct body; `entity.rs` plain Sub pre-pass and MatchArmDeclGroup pre-pass (cluster fold for port directions / declared member names) | PASS |
| forward-declared state is inferred today | `sub_member_is_cross_sub_geometry_or_forward_declared`: `sub_component_types.contains_key && !sub_member_types.contains_key` | PASS |
| delivered | `^\s+pub\(crate\) sub_[a-z_]+:` absent in `scope.rs` | — |

### σ — trait bounds, AST substitution and EntityDefRef below entity; sketch_unsupported lifted (#9419, R10)
| Capability | Evidence | Verdict |
|---|---|---|
| β + σ separate entity and guards | **[Amended 2026-10-20, retro-critic R10 — new leaf]** sitting memo `s7-b1-memo.md`, SCC tool's own lexer on a scratch tree: after β + σ, `entity` in {compile_builder, entity, forall_elaborate}, `guards` in a separate 10-module SCC (strict and loose resolution); every strict subset leaves them together. Textual, not compiled | PASS (measured) |
| the moved items reach no module above them | `trait_bounds` → `types` only; substitution trio and `EntityDefRef` → no crate module; lifted `diagnostic` → `reify_core` only | PASS |
| delivered | `^pub\(crate\) fn satisfies_trait_bound\b\|^pub fn satisfies_trait_bound\b` absent in `entity.rs`; `^(pub )?mod trait_bounds;` present in `lib.rs` | — |

### ξ — INV-ARCH-1 reify-compiler rows
| Capability | Evidence | Verdict |
|---|---|---|
| SCC tool exists | **[Amended 2026-10-20, retro-critic R8 polarity]** S3 α #8673 landed `scripts/assert-module-scc.py` and `scripts/module-scc-baseline.json` with reify-compiler in its `excluded` map | PASS |
| entity and guards apart | **[Amended 2026-10-20, retro-critic R8 / R10]** after σ, as measured; ξ records the residual SCCs either way; fallback (R8 (a)): a back-edge from a module outside the residual SCC reds | PASS (depends on σ) |
| delivered | `"reify-compiler": "glob imports hide edges` absent in `scripts/module-scc-baseline.json` | — |

### π — EnumCount on the geometry-kind enums (fk-bb1ba38a6a46, lead ruling 2)
| Capability | Evidence | Verdict |
|---|---|---|
| 9 ALL arrays used only for `.len()` | `const ALL: [Self;` ×9 in `types.rs`; `Self::ALL` appears only in `VARIANT_COUNT` | PASS |
| strum in reify-compiler | `crates/reify-compiler/Cargo.toml` `strum.workspace = true` | PASS |
| delivered | `const ALL: \[Self;` absent in `types.rs` | — |

### ω — PRD close
| delivered | `^Status: \*\*SHIPPED` present in the PRD | — |
|---|---|---|

## Decompose obligation: `metadata.files` covers every `absent` check

`shared.delivered_check_polarity.lint_delivered_checks` was run over every mechanical check in
the sidecar against `main` (2026-10-12). With each leaf's `files` set to the paths its
`expect: absent` checks name, all 16 blocks lint clean (σ's block, added 2026-10-20 by the
retro-critic sitting R10, names `entity.rs` in its `files`); with `files=None` every `absent` check
warns `absent_overbroad`. So each leaf's `metadata.files` must include those paths (file
paths only — ζ names the three `conformance/` files, not the directory).

## Adopted tasks (not in this batch's sidecar)

#8552 (fk-39797230491b), #8551 (fk-6432e37b9f24), #8560 (fk-6875d651562b) were filed by the
review-all mechanical filing and keep their own descriptions and checks; ω depends on them.
Subsumed tasks to cancel-as-superseded or fold at decompose: #7338 (δ), #8321 (ι), #8006 and
#7986 (ν).
