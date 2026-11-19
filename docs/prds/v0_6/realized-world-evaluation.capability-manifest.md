# Capability manifest — realized-world-evaluation

**PRD:** `docs/prds/v0_6/realized-world-evaluation.md` · **Decomposed:** 2026-11-17 · **As-of:** main `dd2a97059a` (2026-11-17; anchors by symbol).

Machine-readable twin: `realized-world-evaluation.capability-manifest.yaml`, stamped with real task ids by `commit_planning`. The sidecar is normative for `delivered_check` descriptors; this file is generated from it.

Every grep check was executed against main at decompose: each `present` pattern matches nothing today and each `absent` pattern matches today. Absent-checks are bound to the function definition (`fn name\(`), not the bare name, so prose that names a retired mechanism does not keep a check red; where another stream may move the function first, the path is the crate source directory.

**Filed:** 13 leaves. Twelve are new; β adopts the existing task #10136. ξ, α and ι run ahead of #9739. β follows instantiation-value-flow β #6592, which is parked (ruling R18, made after the first commit: β's mechanical check was withdrawn and its scope narrowed). The rest depend on #9739 (R8).

**Baselines.** Fourteen fixtures, `tests/prd-gate/fixtures/realized_world_*.ri`, each parsed with `tree-sitter parse --quiet` (0 ERROR nodes) and run with `target/debug/reify eval` on 2026-11-17; each file's header comment records what it printed. The PRD introduces no new syntax, so the grammar gate has nothing further to bind.

**G7 (design invariants).** Walked against INV-SF-1..7 and INV-ARCH-1. INV-SF-1: a member evaluated without a realized world carries `UndefCause::AwaitingRealization`, and a failed member carries the failure's cause. INV-ARCH-1: the new module sits below the engine SCC (R16). One waiver, on α: a let-bound member in an overridden instance takes the default instance's value with no diagnostic (PRD §7, rulings R15 and R18); instantiation-value-flow β #6592 turns it loud and β makes it correct.

**D3 verification.** `scripts/prd-decompose-verify.mjs` with `reify_bin` = the debug binary, run `wf_660138cd-ef2`, over the two unparked leaves `reify eval` can observe (α, β): `disposition: BLOCKS`, 2 of 2 probed, 28 records all with executed-probe evidence, 0 malformed, 0 fixture-absent, 4 blocking records.
- 2 are α's own future deliverable probed on today's binary (`reify check` exits 1 on the let-bound and user-fn datum fixtures because the consuming op fails). They confirm the baseline. Not findings.
- 2 are real and folded: the inline spelling `fillet(b, [single(shared_edges(top, side))], r)` fails today, so row B7 and §10.1 now name the let-bound form (the inline form is δ's); and an inline datum fails in the default instance too, so row B17's loudness is asserted after α, not as a baseline property of the override path.
ξ and ι were not submitted (a Rust-only check and a docs leaf). The parked leaves were not submitted: their premises are the committed baselines, re-probed when the park lifts.

**Critic.** One fable/high adversarial pass over the assembled draft (3 blockers, 11 majors, all folded or ruled; digest in the authoring session's record) and one opus/high fold-verification pass.

## ξ #10153: the swept kind survives a realization-cache hit

Intermediate; unlocks α (row B12). Measured gap: seat test at HEAD 0611fc55a1, `e.dir` over an extrude is Undef with FeatureDatumAmbiguous on the second build.

| Capability | Verdict | Evidence | Delivered check |
|---|---|---|---|
| swept-kind-restored-on-hit | PASS | built here: `Engine::probe_realization_cache` records the cached terminal's swept kind into the swept-kind table on a hit. Today the hit path returns before the only `swept_kind_table.record` call. Accepted gap: bound to the name of the twice-built regression test this leaf adds; the test itself (same module built twice under a demanded tolerance, `e.dir` equal on both builds) is the behavioural cover. | `present` grep `fn cache_hit_restores_swept_kind` in `crates/reify-eval` |
| kappa-checks-stay-red | PASS | built here: the new cache value is a named struct that is not `CachedRealization`, and `SweptKindTable` is not re-keyed, so κ #9263's present-checks (`struct CachedRealization`, a KernelHandle-keyed swept-kind map) stay red until κ lands. | manual: A negative on another task's checks; verified by running #9263's two delivered checks after this leaf and seeing both still fail. |
| attribute-facet-measured | PASS | built here: a twice-built test of a feature-provenance selector over a cache-served handle, its result recorded on #9263 (PRD §7, R17). | manual: A measurement whose outcome is not known in advance; the record on #9263 is the evidence. |

## α #10154: feature datums and selector coercion resolve wherever they occur

Signal leaf of the first slice. Baselines: tests/prd-gate/fixtures/realized_world_datum_*.ri, realized_world_guarded_branch.ri, realized_world_derived_let.ri, realized_world_single_relational_edge.ri, realized_world_override_instance_datum.ri. The B17 forms also need ε #9638 (upstream).

| Capability | Verdict | Evidence | Delivered check |
|---|---|---|---|
| capability-trait | PASS | built here: `pub trait RealizedQueries` in crates/reify-expr/src/lib.rs with an `EvalContext` field that is None in all three constructors. | `present` grep `pub trait RealizedQueries` in `crates/reify-expr/src` |
| member-kind-enum | PASS | built here: `enum RealizedQueryKind` in reify-ir with the FeatureDatum and SelectorGeometry kinds. | `present` grep `enum RealizedQueryKind` in `crates/reify-ir/src` |
| world-module-below-the-scc | PASS | built here: crates/reify-eval/src/realized_world.rs holds the world, the memo and `resolve_realized`, importing nothing in the engine SCC (PRD §5.3, R16). `scripts/assert-module-scc.py` is the behavioural cover for the import rule. | `present` path `crates/reify-eval/src/realized_world.rs` |
| op-env-bundle | PASS | built here: `OpEnv` replaces the (values, functions, meta_map) triple in geometry_ops.rs. Accepted gap: S2 δ #9158 plans a struct of the same name; the edge #9158 → α keeps this check from going green early. | `present` grep `pub\(crate\) struct OpEnv` in `crates/reify-eval/src` |
| awaiting-realization-cause | PASS | built here: `UndefCause::AwaitingRealization` in crates/reify-ir/src/value.rs, recorded when a member is evaluated with no realized world. | `present` grep `AwaitingRealization` in `crates/reify-ir/src` |
| prehydrate-deleted | PASS | `grep:crates/reify-eval/src/geometry_ops.rs` (today defines `prehydrate_inline_curated_selector_args`; this leaf deletes it with `CURATED_SELECTOR_ARG_NAMES` and `InlineSelectorMemo`). Accepted gap: bound to the function definition, so prose that names the retired mechanism does not keep the check red; ratchet row curated-prehydrate covers the other two symbols. | `absent` grep `fn prehydrate_inline_curated_selector_args\(` in `crates/reify-eval/src` |
| selector-cell-one-value | PASS | `grep:crates/reify-eval/src/engine_build.rs` (today defines `Engine::realization_read_cells`, the gate of ladder rung (b)). This leaf deletes rung (b) and that function; a selector cell keeps its descriptor (D9). | `absent` grep `fn realization_read_cells\(` in `crates/reify-eval/src` |
| datum-reaches-op-inline-and-let-bound | PASS | built here, with `producer:task-9638` upstream for the decoder. Baseline observed with target/debug/reify eval: both fixtures fail "expected an Axis value, got undef". | manual: Behavioural: rows B1–B5 of PRD §6, asserted through the CLI harness on the committed fixtures. |
| tessellate-stamp-and-producer-closure | PASS | built here: the per-Realize values stamp in `tessellate_from_values` and the value-read producer closure in realization_staleness.rs (PRD §5.5, §5.6). | manual: Behavioural: rows B14 and B15, Rust tests over the tessellate entries. S2 η #9159 deletes `tessellate_from_values`, so no grep on it is sound. |
| override-paths-stay-loud-inline | PASS | rejection-check: realized_world_override_instance_datum.ri run with target/debug/reify eval prints `per-instance re-realization compile error for Outer.a.body` today; this leaf must keep an inline datum in an overridden instance a coded error (B17). | manual: Behavioural rejection, pinned by a CLI harness test on the fixture. |

## β #10136: realization-derived cells in an overridden instance (adopts #10136; follows instantiation-value-flow β #6592)

Adopted task #10136. Design-first (PRD §5.7). Narrowed 2026-11-17 by ruling R18: per-instance realization is instantiation-value-flow β #6592's (upstream); this leaf adds the scheduled slots and the realized world inside it. Baseline: tests/prd-gate/fixtures/realized_world_override_instance.ri builds silently wrong today (x-span 21.0 mm where a standalone size-80 build gives 161.0 mm) and prints `c: undef` for the instance's centroid cell.

| Capability | Verdict | Evidence | Delivered check |
|---|---|---|---|
| instance-overlay-of-all-cells | PASS | `producer:task-6592` upstream: the override paths compile an instance's ops against the instance's own cells (instantiation-value-flow C1, C2). Today `instance_value_overlay` overlays params only. | manual: Upstream capability; its delivery is #6592's own delivered checks. |
| instance-derived-cells-correct | PASS | built here, with α upstream for the realized world and #6592 upstream for the instance realization. | manual: Behavioural: rows B22 and B23; the override fixture's Outer.moved spans x in [-0.5, 160.5] mm and Outer.a.c displays the overridden centroid. No grep is sound here: the functions involved are #6592's to reshape. |

## ι #10155: docs-truth for the first slice (spec lazy-branch effects, guarantee, chunks, exemplar)

| Capability | Verdict | Evidence | Delivered check |
|---|---|---|---|
| exemplar-in-corpus | PASS | built here: examples/best_practices/feature_datum_as_op_arg.ri with its INDEX.md line; compile-gated by examples_smoke. | `present` path `examples/best_practices/feature_datum_as_op_arg.ri` |
| spec-and-chunks | PASS | `grep:docs/reify-language-spec.md` (§9.2.4 and §9.2.5 state value-level branch semantics today) and `grep:crates/reify-mcp/src/tools/chunks` (the geometry and topology chunks); this leaf adds the effects wording and the guarantee, with `producer:α` upstream for the behaviour documented. | manual: Prose; each documented signature is verified by a smoke .ri in the same diff. |

## γ #10156: geometry queries resolve wherever they occur

Parked behind #9739. Baselines: realized_world_query_inline.ri, mwaste_fn_solid_no_fold.ri.

| Capability | Verdict | Evidence | Delivered check |
|---|---|---|---|
| nested-fold-deleted | PASS | `grep:crates/reify-eval/src/geometry_ops.rs` (today defines `rewrite_geometry_queries`); this leaf moves the DFM harvest and the constraint re-check onto the realized context and deletes it. | `absent` grep `fn rewrite_geometry_queries\(` in `crates/reify-eval/src` |
| query-inline-and-in-user-fn | PASS | built here, with α and β upstream. | manual: Behavioural: row B24. |

## δ #10157: topology helpers, feature() and ad-hoc selectors resolve wherever they occur

Parked behind #9739. Baseline: realized_world_flat_map_inline.ri.

| Capability | Verdict | Evidence | Delivered check |
|---|---|---|---|
| flat-map-walker-deleted | PASS | `grep:crates/reify-eval/src/geometry_ops.rs` (today defines `try_eval_topology_flat_map`); the capability reaches members under a lambda, so the walker goes. | `absent` grep `fn try_eval_topology_flat_map\(` in `crates/reify-eval/src` |
| helpers-inline | PASS | built here, with α, #8749, #8750 and #7886 upstream. | manual: Behavioural: row B25. |

## ζ #10158: conformance, kinematic and mass-property members resolve wherever they occur

Parked behind #9739. Baseline: realized_world_conformance_branch.ri.

| Capability | Verdict | Evidence | Delivered check |
|---|---|---|---|
| swept-kinematic-walker-deleted | PASS | `grep:crates/reify-eval/src/geometry_ops.rs` (today defines `try_eval_swept_kinematic_query`). | `absent` grep `fn try_eval_swept_kinematic_query\(` in `crates/reify-eval/src` |
| predicates-and-mass-feed-ops | PASS | built here, with α and β upstream. | manual: Behavioural: row B26. |

## η #10159: membership lives in the builtin registry binding

Parked behind #9739.

| Capability | Verdict | Evidence | Delivered check |
|---|---|---|---|
| registry-binding-carries-the-kind | PASS | built here, generalising S8 #8906's binding column (upstream) to calls. | `present` grep `Realized\(RealizedQueryKind\)` in `crates` |
| recognizer-list-deleted | PASS | `grep:crates/reify-eval/src/geometry_ops.rs` (today defines `is_geometry_consumer_call`). The path is the crate source directory because S3 #8687 moves the predicate to expr_classify.rs first. | `absent` grep `fn is_geometry_consumer_call\(` in `crates/reify-eval/src` |

## θ #10160: constraints, cross-sub copies, compute arguments and guarded members evaluate at their slot

Parked behind #9739.

| Capability | Verdict | Evidence | Delivered check |
|---|---|---|---|
| remaining-positions | PASS | built here, with β, γ, δ, ζ and S2 λ #9264 upstream. | manual: Behavioural: row B27, one fixture per position. |

## π #10161: convergence — the ladder and the value-resolving passes are deleted

Parked behind #9739.

| Capability | Verdict | Evidence | Delivered check |
|---|---|---|---|
| ladder-deleted | PASS | `grep:crates/reify-eval/src/engine_build.rs` (today defines `hydrate_value_cell_in_loop`). The path is the crate source directory because S2 ι₃ #9163 moves it first. | `absent` grep `fn hydrate_value_cell_in_loop\(` in `crates/reify-eval/src` |
| derived-lets-pass-deleted | PASS | `grep:crates/reify-eval/src/engine_build.rs` (today defines `post_process_derived_lets`). | `absent` grep `fn post_process_derived_lets\(` in `crates/reify-eval/src` |

## ρ #10162: docs-truth for the whole class

Parked behind #9739.

| Capability | Verdict | Evidence | Delivered check |
|---|---|---|---|
| class-documented | PASS | built here, with γ, δ, ζ and θ upstream. | manual: Prose; each documented signature is verified by a smoke .ri in the same diff. |

## σ #10163: integration gate — the kind × position × walk matrix runs in the gate

Parked behind #9739. Integration-gate leaf (H); its signal is PRD §6.

| Capability | Verdict | Evidence | Delivered check |
|---|---|---|---|
| matrix-in-the-gate | PASS | built here: every §6 row is a test over a committed tests/prd-gate/fixtures/realized_world_*.ri fixture, with same-diff drift-guard registrations. | manual: The fixtures directory already holds this PRD's baselines, so a path or name grep cannot distinguish the gate from the baseline. |

## ω #10164: PRD-close — stamp SHIPPED with landed IDs

Parked behind #9739.

| Capability | Verdict | Evidence | Delivered check |
|---|---|---|---|
| terminal-status-stamped | PASS | built here: the Status line of docs/prds/v0_6/realized-world-evaluation.md, which reads "authored" at decompose. | `present` grep `^\*\*Status:\*\*? \*?\*?SHIPPED` in `docs/prds/v0_6/realized-world-evaluation.md` |
