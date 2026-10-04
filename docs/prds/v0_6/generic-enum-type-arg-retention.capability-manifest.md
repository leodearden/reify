# Capability manifest — `generic-enum-type-arg-retention`

**PRD:** `docs/prds/v0_6/generic-enum-type-arg-retention.md` (authored 2026-09-29, esc-6020-11 session; revised after the critic pass; forks R1–R5 RULED by Leo 2026-09-29; decomposed 2026-09-29)
**Substrate HEAD:** main `300910857b` (2026-09-29) · **Probe binaries:** `target/debug/reify` 2026-09-25 and `target/release/reify` 2026-09-01 — see the PRD header for the nine files that changed after the debug build (judged inert by reading, not re-measured) and for the two binaries' known disagreements (#5306 conformance severity; one eval path).
**Batch:** tasks **8012–8023** (12 leaves: α 8012 · α2 8013 · β 8014 · γ 8015 · ε2 8016 · θ 8017 · δ 8018 · ε 8019 · ζ 8020 · η 8021 · ι 8022 · κ 8023) · 26 intra-batch dependency edges, 0 cross-PRD edges (#7456/#6020 are ordering seams recorded in PRD §8, stamped on those tasks by the lead, not `add_dependency` edges)
**Machine-readable twin:** `docs/prds/v0_6/generic-enum-type-arg-retention.capability-manifest.yaml` (task ids stamped by `commit_planning`)

Mechanizes G3 + G6 per leaf. Substrate posture: **no novel `.ri` syntax** — all 15 fixtures parse with 0 Lezer error nodes and reach the type checker. The D3 workflow's role is **behavioural** (overlay vectors 2 and 4): each leaf's signal is a before-image fixture whose exit code, diagnostic substring and/or printed value flip. Numeric-floor and field-population sub-checks N/A.

**Probe sets are split per leaf** (`tests/prd-gate/generic-enum-type-arg-retention-{alpha2,beta,gamma,epsilon2,delta,epsilon}-probe-set.json`) so a leaf's dispatch-time check never depends on a downstream leaf's rows (the harness exits 1 if any row fails). Measured 2026-09-29 under both binaries: alpha2 1 FAIL; beta 2 FAIL; gamma 1 FAIL; epsilon2 1 FAIL + 1 PASS (the PASS is the `let`-bound regression guard); delta 3 FAIL + 1 UNPROVABLE (the U-1 `value` row reads nothing while the fixture exits 1); epsilon (debug binary only) 1 FAIL + 2 UNPROVABLE. All rows carry a `stderr_contains`/`stdout_contains`/`stdout_value` naming the mechanism, not exit code alone. **POST-state runs need a rebuilt binary**: the harness prefers `target/release/reify` and honours `REIFY_BIN`; the epsilon set is meaningful ONLY under `REIFY_BIN=target/debug/reify` (release already passes it today, vacuously), so its yaml check is `manual`. The sets and their `tests/prd-gate/README.md` rows land with leaf α: `tests/prd-gate/*.json` is the C5 fail-wide catch-all in `decide_scope` (`RUN_RUST=1`, all crates; `test_verify_scope.sh` PG-3), so they cannot ride the direct docs commit. Six sets: alpha2, beta, gamma, epsilon2, delta, epsilon.

Fixtures (`tests/prd-gate/fixtures/`, headers record both binaries): `getar_fallback_type_mismatch_silent.ri` (S-1), `getar_result_arg_mismatch_assign_silent.ri` (S-2), `getar_tree_force_leaf_length_silent.ri` (S-3), `getar_wildcard_headed_arg_silent.ri` + `getar_wildcard_placeholder_binder_silent.ri` (S-4), `getar_recursive_conflict_silent.ri` (4031), `getar_fn_param_rejects_construct.ri` (U-1), `getar_match_over_let_construct_panics.ri` + `getar_match_over_annotated_let_panics.ri` (P-1, closed by δ), `getar_match_err_first_arm_panics.ri` + `getar_match_err_subject_ok_first_panics.ri` (P-1, need R5/ε), `getar_bare_result_annotation_match_panics.ri` (R2), `getar_decl_match_applied_param_rejected.ri` + `getar_decl_match_let_construct_ok.ri` (ε2), `getar_let_annotated_result_arg_mismatch_silent.ri` (§10 out of scope). None is registered in `_RUST_COUPLED_RI_FIXTURES` / `_GUI_COUPLED_RI_FIXTURES` at authoring; a leaf that `include_str!`s one registers it in the same diff.

Leaves: **α** (docs root, also lands the probe sets), **α2** (real leaf), **β, γ, ε2, θ** (independent after α), **δ** (after β, γ, ε2), **ε, ζ, η, ι, κ**. No leaf is an intermediate: β has its own S-4 signal.

---

## α — docs root (PRD forks recorded; superseded-PRD LIVE pointer) — `docs/prds/` only

Signal: the committed PRD header with R1–R5 resolved; the superseded PRD's LIVE header points here. (The spec/chunk static-type sentence is **δ's** deliverable: it is false until δ lands, and `enums.md` is under `crates/`, so it cannot be a docs fast-path commit.)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| Superseded PRD is frozen; edits go to its LIVE header only | policy | `generic-data-carrying-enums.md` header "SHIPPED … AS-AUTHORED design record" | PASS |
| α touches nothing under `crates/` | policy | leaf definition | PASS (obligation) |

## α2 — bare generic-enum annotation rejected (R2(b) RULED — real leaf)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| Bare annotation is silent today | value probe | `getar_bare_result_annotation_match_panics.ri`: debug exit 101 panic, release exit 0 `W.v = 0.005 m` | PASS |
| Arity diagnostic mechanism exists | rejection-mechanism | `resolve_enum_type_with_args` arity-mismatch path already diagnoses `Result<Length>`; zero-arg path is the exception | PASS |
| Zero corpus churn | measured | `git grep -E 'param [a-z_]+ *: *(Result\|Tree\|Pair\|Either\|Maybe) *(=\|$)' -- '*.ri'` → 0 | PASS |

## β — tier-3 narrowing (LEAF)

Signal: `getar_wildcard_headed_arg_silent.ri` check 1 "no matching overload for fl("; probe set `…-beta-probe-set.json`.

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| S-4 is live today | value probe | `getar_wildcard_headed_arg_silent.ri`: check 0 both binaries; eval `W.v = Result::Ok` (debug) / `undef` (release). `getar_wildcard_placeholder_binder_silent.ri`: check 0, `W.v = false` | PASS |
| The hole is the arg-side tier-3 disjunct | wired-on-main | `slot_matches_wildcard_tier` in `crates/reify-core/src/overload.rs`: `… \|\| param_ty == arg_ty \|\| type_carries_type_param(arg_ty)`; tier 2 narrows only the param-side disjunct | PASS |
| One predicate reaches all three consumers | wired-on-main | callers: `resolve_function_overload` (`type_compat.rs`), `find_matching_compiled_function` (`reify-expr/src/lib.rs`), the `TraitMethodCall` arm in `expr.rs` (`use reify_core::overload::slot_matches_wildcard_tier`) | PASS |
| D4 bare-arg behaviour preserved | boundary | `overload_bare_type_param_arg_still_resolves` (`type_compat.rs` tests) and `bare_type_param_arg_resolves_a_non_generic_concrete_candidate` (`find_matching_compiled_function_tests.rs`) stay green | PASS (obligation) |
| Placeholder predicate available | producer | R1(c) reserved prefix; precedent `AUTO_TYPE_PARAM_PLACEHOLDER_PREFIX` in `entity.rs` | PASS (gated on R1) |

## γ — args-aware enum-compat oracle + `unify` placeholder rule + conformance (LEAF)

Signal: `getar_result_arg_mismatch_assign_silent.ri` check 1 "argument 'b' has type" (B3); B14 (`or_else(Ok{1mm}, Ok{2mm})`) stays clean; probe set `…-gamma-probe-set.json`; `examples_smoke` green after the sweep.

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| Name-only oracle is the chokepoint | wired-on-main | `base_enum_name` + `enum_payload_compatible` (`type_compat.rs`); conformance enum branch calls `base_enum_name` | PASS |
| S-2 silent today | value probe | fixture: check 0, `W.n = 0.005 m` under a `Force` binder, both binaries | PASS |
| Rejection mechanism | rejection-mechanism | ctor-conformance `ArgTypeMismatch` at Error severity (#5306, landed 2026-09-29 — a POST run needs a binary built after it) | PASS |
| `unify` must change | wired-on-main | `unify` TypeParam arm hard-conflicts two distinct `TypeParam` bindings (`unify_two_distinct_erased_params_still_conflict`); the placeholder no-conflict rule is γ's, pinned by B14 | PASS (obligation) |
| Let form stays silent (scoped out) | boundary | `getar_let_annotated_result_arg_mismatch_silent.ri`; `check_let_annotation_type` early return; follow-up vs #4705 filed at decompose | PASS (out of scope, owned) |

## ε2 — decl-form match accepts `Applied` (LEAF, before δ)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| Pre-existing rejection | value probe | `getar_decl_match_applied_param_rejected.ri`: check 1 "expected an enum", both binaries | PASS |
| Regression guard | value probe | `getar_decl_match_let_construct_ok.ri`: check 0 today; would regress after δ without ε2 | PASS |
| Shared resolver with #6020 | seam | #6020 plan step-10 `match_discriminant_enum`; ε2 consumes it if landed | PASS (obligation) |

## θ — param-override kind check admits `Applied` (LEAF, before η)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| Rejects `Applied` today | wired-on-main (read, not run) | `value_type_kind_matches` in `crates/reify-eval/src/lib.rs`: `Value::Enum { .. } => matches!(ty, Type::Enum(_))`; callers: engine-admin override and `registry_parity_tests.rs::classify` | PASS |
| Test home is an existing standalone binary | landed | `crates/reify-eval/tests/generic_enum_erasure_e2e.rs`, baselined in `tests/infra/harness-layout-baseline.manifest` | PASS |

## δ — construction emits `Applied` + docs-truth (LEAF)

Signal: B2/B4/B5/B6 fixture-backed (probe set `…-delta-probe-set.json`); B7 (`getar_match_over_*_panics.ri`) under a debug binary; spec §3.9.2 + `enums.md` sentence in the same diff.

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| Inferred args exist at the success exit | wired-on-main | `final_subst` in `compile_variant_construct` | PASS |
| `Applied` is a legal enum carrier | wired-on-main | `resolve_enum_type_with_args` returns it for annotations; `binder_subst` in the `expr.rs` Match branch reads it | PASS |
| Conflict code exists | rejection-mechanism | `EnumTypeArgConflict` ("type parameter … bound to both …") in `variant_construct.rs`; B5 reaches it via `unify`'s Applied/Applied arm | PASS |
| Pinned-arg payload check | rejection-mechanism | "field … of variant … expects type …" in `variant_construct.rs`; today defeated by `enum_payload_compatible`'s name-only child match (S-3) | PASS |
| Non-generic goldens unaffected | boundary | `Applied` only for non-empty `type_params`; `crates/reify-compiler/tests/harness_patterns/variant_construction_check_tests.rs` | PASS (obligation) |
| Runtime unchanged | boundary | `generic_enum_erasure_tests.rs` reads the runtime `Value` only | PASS |
| Compile-time pins flip | expected-red | `result_prelude_enum_tests.rs` two `Type::Enum("Result")` assertions | PASS (obligation) |
| #7456 seam | seam | D-8: lead stamps #7456 now; WORK item 1 says `Type::Enum(name)` | PASS (obligation) |
| Docs-truth same diff | docs-truth | `enums.md` "Type arguments are inferred from the payload at construction" sentence; spec §3.9.2 keeps "erased before evaluation" and gains the static-type sentence | PASS (obligation) |

## ε — match result-type rule (R5) (LEAF)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| R5 cases survive δ | value probe | `getar_match_err_first_arm_panics.ri` / `getar_match_err_subject_ok_first_panics.ri`: debug 101 panic, release 0 (`"ok"` / `0.001 m`); after δ the first arm's binder is placeholder-typed | PASS |
| Harness-expressible, debug only | vacuity | `…-epsilon-probe-set.json` under `REIFY_BIN=target/debug/reify` (rebuilt); vacuous under release; yaml check `manual` | PASS |
| Producer | wired-on-main | `expr.rs` Match branch result type = first arm; `is_representable_cell_type` (`engine_eval.rs`, `ASSERT_MSG_PREFIX`) rejects `TypeParam` | PASS |

## ζ — EnumAccess expected-type threading (OPTIONAL, gated on R4)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| Arm ignores `type_params` and expected type | wired-on-main | `expr.rs` EnumAccess arm (`E.V` spelling) | PASS |
| No generic unit variant compiled in tree | measured | `Maybe<T>{Nothing, Just}` only in tree-sitter/lowering fixtures | PASS (leaf declares fixture) |

## η — `parse_length_r` registered as `Result<Length, String>` (LEAF, after θ)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| Headless today, and why | wired-on-main | `registry.rs` `ParseLengthR { … result: Const(Type::Enum("Result")) }` with the comment "so the eval-time value_type_kind_matches guard passes" — hence the θ dependency | PASS |
| Pins that flip | expected-red | `registry_seed_result_types.rs`, `parse_length_signatures.rs`, the in-crate test in `registry.rs`; `registry_parity_tests.rs` is an Undef sweep, not a pin | PASS (obligation) |

## ι — accept-side exemplar + discoverability (LEAF)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| Example carries only accept-side rows | design | B6/B7 shapes as constraints; rejections (B2, B5) stay in fixtures — an `examples_smoke` file must exit 0 | PASS |
| Corpus + index surfaces exist | wired-on-main | `examples/best_practices/INDEX.md`; `.claude/skills/reify-design/SKILL.md` reference index | PASS |

## κ — PRD-close (LEAF)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| Terminal vocabulary + freeze shape | policy | overlay "PRD terminal status"; exemplar `generic-data-carrying-enums.md` | PASS |
