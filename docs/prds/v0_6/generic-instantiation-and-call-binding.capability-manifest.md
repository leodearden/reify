# Capability manifest — generic-instantiation-and-call-binding

PRD: `docs/prds/v0_6/generic-instantiation-and-call-binding.md`. Machine-readable twin:
`docs/prds/v0_6/generic-instantiation-and-call-binding.capability-manifest.yaml` (stamped by
`commit_planning`). Evidence measured on main `baff3ed38b` (2026-10-17) with a debug `reify` built 2026-10-16
14:45 (no compiler/eval/ir/expr/core source change since, except comment-only `125f161b33`); delivered-check
polarity re-measured on main `5d07b395db` (2026-10-18). Fixtures are in `tests/prd-gate/fixtures/`; every
`gicb_*` fixture and `tooling_fe_unrepresentable_generic.ri` passes the grammar gate (`tree-sitter parse
--quiet`, isolated cache, exit 0). Every delivered check below was measured to FAIL on main and to PASS once
its producer lands.

**Decompose verification.** `scripts/prd-decompose-verify.mjs` ran on 2026-10-18 (run `wf_73385a73-006`, a
scratch copy pinned to `REIFY_BIN=target/debug/reify` because `target/release/reify` dates from 2026-09-01)
over α₁, α₂, β₁, γ₁, γ₂ and δ; β₂ (builder/LSP), ε (docs) and ω (close stamp) have no CLI probe vector.
Disposition BLOCKS with 6/6 leaves probed, 0 malformed records, 2 fixture-absent controls (written as
`gicb_gamma_free_fn_arg_conformance_control.ri` and probed in scratch). The blocking items resolve as:
- **Real findings, fixed before filing:** the CLI prints no code mnemonic, so D3's message carries the
  `E_TYPE_ARG_ARITY:` prefix as its sibling `E_TYPE_ARG_ON_TRAIT:` does; probe (c)'s `= [1]` default
  cascades a second error, so the fixture omits it; the `effective_arg` ratchet and check matched
  `conformance/mod.rs`'s unrelated `effective_arg_type` (now `let effective_arg:`, baseline 2); γ₁'s
  re-resolve check was satisfiable by a rename (now `resolve_function_overload\(`); δ's generic-enum case
  needs an applied-enum arm in the shared resolver (R14: #8214 absorbed into α₁, new fixture
  `gicb_alpha_nested_generic_enum.ri`).
- **Producer-self postconditions** probed against today's tree (α₁, α₂ "after" states): fail by construction.
- **Harness binding defects, not premise falsifications:** `stderr_not_contains` is not a match key, and a
  list-valued `stderr_contains` crashes `prd-capability-check.py` (the β₁ HARNESS_ERROR); each premise holds
  when bound correctly (the lead's own probes, PRD §2).

**Critic.** A fable/high critic read the assembled draft (2026-10-18): 2 blockers, 6 majors, 7 minors, all
folded. The concreteness blocker was re-ruled by Leo (R13); the rest are corrections of fact or scope (id
carriers in the destructure, identifier-safe monomorph names, β₂'s signal, D2's ordering, the undocumented
control fixture, a bare-token check, LSP strings in ε, the header's freshness claim).

**Fold verification.** An opus/medium fold-verifier re-read the folded draft (2026-10-18): all 20 findings
addressed; it found two majors in the fold (a non-existent `GuardedDeclArm` path in the destructure table; a
`remap_entity` instruction that would leave content hashes stale) and seven minors, all applied. `signal_leaves`
names the four key-closing behaviour leaves (α₂, β₁, γ₂, δ); β₂ is a consolidation leaf and carries no goal
signal.

Verdict key: **PASS** means the binding held at authoring time. "producer-self" means the capability is the
leaf's own deliverable; "producer:task-N" an upstream filed task wired as a dependency.

## α₁ — One type-argument binder

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| annotation-default-rejected-today | `gicb_alpha_default_padding.ri`: `error: wrong number of type arguments for 'Holder': expected 2, got 1`, rc 1; the same file's `sub part = Holder<Bolt>()` is accepted | PASS | — |
| nested-alias-arity-silent-today | `gicb_alpha_nested_alias_arity.ri`: only `unresolved type: Q<Int>`; the direct `P<Int, Int>` names `P` and the counts | PASS | — |
| nongeneric-builtin-args-silent-today | `gicb_alpha_nongeneric_builtin_args.ri`: `All constraints satisfied.` rc 0 for `Int<Real>`, `Length<Int>`, `Bool<Length>`, `String<Int>`, `Orientation<2>`. Rejection absent today is the motivation; after α₁ `bind_type_args(Fixed(0), ..)` is the mechanism | PASS | — |
| nested-applied-enum-unresolved-today | `gicb_alpha_nested_generic_enum.ri`: `unresolved type` for `Option<Ge<Real>>`, `List<Ge<Real>>` and `W` (#8214, absorbed per R14) | PASS | — |
| resolution-ctx-upstream | producer: #8643 (S7 μ) | PASS | — |
| builtin-arity-table-upstream | producer: #8903 (S8 ο) | PASS | — |
| any-descendant-upstream | producer: #8638 (S7 λ) | PASS | — |
| enum-bare-arity-upstream | producer: #8013 (generic-enum α2; branch `task/8013`, 5 commits, 2026-10-18) | PASS | — |
| binder-fn | producer-self | PASS | grep `fn bind_type_args` present in `crates/reify-compiler/src` (0 now) |
| strict-applied-arity-gone | producer-self (INV-α2) | PASS | grep `args\.len\(\) != target\.type_params\.len\(\)` absent in `type_resolution.rs` (1 now) |
| builtin-guard-arms-gone | producer-self (D5) | PASS | grep `" if type_args\.len\(\) == [0-9]` absent in `type_resolution.rs` (24 now) |
| default-type-arg-equivalence-test | producer-self; ported from #9329, scoped to the fixture read (which also forces PG-DRIFT registration) instead of #9329's bare type spelling | PASS | grep `gicb_alpha_default_padding` present in `crates/reify-compiler/tests` (0 now) |

## α₂ — Instantiation and pattern sites

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| sub-arity-codeless-today | `gicb_alpha2_sub_arity_excess.ri`: code-less `too many type arguments for 'Holder': expected 2, got 3`, rc 1 | PASS | — |
| binder-upstream | producer: α₁ | PASS | — |
| variant-construct-upstream | producer: #8018 (generic-enum δ) | PASS | — |
| effective-arg-copies-gone | producer-self; the colon excludes `conformance/mod.rs`'s unrelated `effective_arg_type` | PASS | grep `let effective_arg:` absent in `crates/reify-compiler/src` (2 now) |
| hand-too-many-check-gone | producer-self | PASS | grep `type_args\.len\(\) > type_params\.len\(\)` absent in `entity.rs` (1 now) |

## β₁ — Monomorph extraction (closes fk-7bf9e6299eca)

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| declared-generic-panics-today | `tooling_fe_unrepresentable_generic.ri`: `reify check` and `reify eval` panic rc 101 on `Bearing.seal` (`TypeParam("T")`) | PASS | — |
| explicit-instantiation-panics-today | `gicb_beta_explicit_instantiation.ri`: check panics on `Holder.seal`, eval on `Top.h.seal` | PASS | — |
| monomorph-collision-today | `gicb_beta_two_monomorph_collision.ri`: eval prints `Widget.slot_t = SealB { b: 2 }` beside the two instance cells | PASS | — |
| upstream-walkers | producers: #8638, #8743, #8704 (#8704 is in #8612's closure; depending on it is allowed) | PASS | — |
| hydration-has-no-diag-channel | `EvaluationGraph::from_templates -> Self`; `Engine::eval -> EvalResult` carries diagnostics (D8) | PASS | — |
| instantiate-fn | producer-self | PASS | grep `fn instantiate_template` present in `crates/reify-compiler/src/compile_builder` (0 now) |
| instantiation-pass | producer-self | PASS | grep `fn phase_instantiate_generics` present in `crates/reify-compiler/src` (0 now) |
| representability-home-moved | producer-self (R13: one shallow predicate beside `Type`) | PASS | grep `fn has_value_representation` present in `crates/reify-core/src` (0 now) |
| representability-eval-copy-gone | producer-self | PASS | grep `fn is_representable_cell_type\(` absent in `crates/reify-eval/src` (1 now; the `(` excludes the unit-test names) |
| identifier-safe-mono-names | producer-self (a monomorph name is a valid `InstancePath` root) | PASS | grep `fn mangle_type_arg` present in `crates/reify-compiler/src` (0 now) |
| placeholders-in-derived-types-hydrate | behavioural (R13) | PASS | manual: β₁'s compile/eval test |
| concreteness-predicate | producer-self | PASS | grep `pub fn template_is_concrete` present in `crates/reify-compiler/src` (0 now) |
| clone-and-clear-gone | producer-self | PASS | grep `mono\.type_params\.clear\(\)` absent in `compile_builder` (1 now) |
| invariant-row | producer-self | PASS | grep `INV-COMP-4` present in `docs/invariants.md` (0 now) |
| fixtures-evaluate | behavioural | PASS | manual: the reify-eval e2e tests are the cover |

## β₂ — One representability definition (follow-on, G1; consolidation and deletion)

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| predicate-upstream | producer: #8761 (S27 β) | PASS | — |
| partition-upstream | producer: #8860 (S4 ν) | PASS | — |
| assert-callers-gone | producer-self | PASS | grep `assert_value_cell_types_representable\(&(snapshot\|new_snapshot)\.graph\)` absent in `crates/reify-eval/src` (2 now) |
| assert-fn-gone | producer-self | PASS | grep `fn assert_value_cell_types_representable` absent in `crates/reify-eval/src` (1 now) |

## γ₁ — Call binding recorded

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| trait-method-arg-unchecked-today | `gicb_gamma_trait_method_arg_conformance.ri`: `All constraints satisfied.` rc 0, eval `Top.bad = 1`; the control `gicb_gamma_free_fn_arg_conformance_control.ri` errors `type 'Foam' does not conform to trait 'Rigid' required by param 'other'` | PASS | — |
| padded-calls-checked-today | correction to the brief: a padded call re-resolves and its bound check fires (`type argument 'Foam' does not satisfy bound 'Rigid' …`), so padding is not γ₁'s observable | PASS | — |
| intrinsic-stamp-upstream | producer: #8897 (S8 ζ) | PASS | — |
| fnref-type | producer-self | PASS | grep `pub struct FnRef` present in `crates/reify-ir/src` (0 now) |
| variant-carries-callee | producer-self | PASS | grep `callee: FnRef` present in `crates/reify-ir/src/expr.rs` (0 now) |
| name-only-variant-gone | producer-self | PASS | grep `function_name: String,` absent in `crates/reify-ir/src/expr.rs` (2 now: variant field + constructor param) |
| binder-fn | producer-self | PASS | grep `fn bind_user_call` present in `crates/reify-compiler/src` (0 now) |
| post-pass-reresolve-gone | producer-self; not scoped to the `function_name` binding, which a rename would satisfy | PASS | grep `resolve_function_overload\(` absent in `entities_phase.rs` (1 now) |

## γ₂ — One resolver at eval (closes fk-1b3f91469997; deletion leaf)

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| overload-output-today | `gicb_gamma_overload_dispatch.ri`: eval `S.a = 1`, `S.b = 2`; must stay byte-identical | PASS | — |
| parity-test-upstream | producer: #5992 (branch `task/5992`, 14 commits) | PASS | — |
| eval-resolver-gone | producer-self | PASS | grep `fn find_matching_compiled_function` absent in `crates/reify-expr/src` (1 now) |
| second-merge-gone | producer-self | PASS | grep `fn merge_functions\(` absent in `crates/reify-eval/src` (1 now) |

## δ — One TypeExpr resolver (closes fk-7eb49b30a889)

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| alias-body-gap-today | `gicb_delta_alias_body_deferred_entity.ri`: `unresolved type: Outer<Real>`; `gicb_delta_alias_body_generic_enum.ri`: `unresolved type: W<Real>`; the direct spellings resolve | PASS | — |
| pin-test-vacuous-today | the known-gap module asserts `alias == direct ∨ alias errors`, green either way (D13) | PASS | — |
| ctx-upstream | producers: #8643, #8903, α₁ | PASS | — |
| twin-resolver-gone | producer-self | PASS | grep `fn resolve_type_alias_expr_with_subst\(` absent in `type_resolution.rs` (1 now) |
| env-type | producer-self | PASS | grep `enum TypeParamEnv` present in `crates/reify-compiler/src` (0 now) |
| known-gap-pin-retired | producer-self | PASS | grep `mod parametric_alias_deferred_alias_body_known_gap` absent in `crates/reify-compiler/tests` (1 now) |

## ε — Docs truth

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| orientation-arg-spellings-today | 63 lines match `(Orientation\|Frame\|Transform3?)<[0-9N]>`: 48 in the stdlib reference, the spec and the chunks, 15 in `reify-lsp` completion signatures, `geometry_ops.rs` runtime strings and comments, and a `geometry.rs` diagnostic; no `.ri` code | PASS | — |
| orientation-arg-spellings-gone | producer-self (R9) | PASS | grep `(Orientation\|Frame\|Transform3?)<[0-9N]>` absent in those six paths (63 now) |
| chunks-state-behaviour | behavioural | PASS | manual: `harness_doc_chunks` plus a smoke `.ri` per documented signature |

## ω — PRD close

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| terminal-header | producer-self | PASS | manual: PPRDSTATUS is the detector |
