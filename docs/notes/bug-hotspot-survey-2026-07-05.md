# Bug-hotspot survey and architectural review — 2026-07-05

Method: mined the full git history (~48k commits) for fix-commit density per file/subsystem,
recency (since 2026-05-01), and cross-subsystem co-fix coupling; mined the fused-memory task
tracker (historical fix tasks + all 46 live non-terminal tasks); then a 7-agent review team
read the hotspot code directly (~1.1M subagent tokens). Findings below are code-confirmed with
file:line citations unless marked hypothesis. This file is a session artifact — not yet
tracked/committed; keep or delete as desired.

## Ranked hotspots (evidence)

| # | Hotspot | Evidence |
|---|---|---|
| 1 | reify-eval evaluation paths (`engine_eval.rs`, `engine_edit.rs`, `unfold.rs`, concurrent stack, `reify-expr`) | engine_eval.rs 151 fix commits (43% of all its commits); reify-expr/lib.rs 183 (43%); recurring "same bug fixed once per path" class (tasks 4317→4332, 4356, 2266, 2259/2267, 2195) |
| 2 | `engine_build.rs` | #1 most fix-patched file since May (155 touches); 21.4k lines; reset-invariant class (4349, 3437), version-id off-by-one (2554) |
| 3 | `geometry_ops.rs` + multi-kernel seam | 32.4k lines, 110 recent fix touches; serial substrate holes (4329→4336→4262→live 4876); warm-start silent misdispatch (2510) |
| 4 | GUI state bridge (`gui/src-tauri/src/engine.rs`, `debug_server.rs`, frontend stores) | engine.rs 53% fix ratio (highest in repo); debug_server.rs 67 of 69 fix touches since May; "computed but never surfaced" fixed 4× (1764, 3351, 3386, 4252) + 4251/4884 desync |
| 5 | Compiler expr/type layer (`expr.rs`, `entity.rs`, `type_resolution.rs`, signature files) | 43–47% fix ratios; silent-drop class (4603), mega-handler growth (4596), signature-table copies (4311/4493) |
| 6 | Caching/freshness substrate (`cache.rs`, dirty graph, warm-start, persistent cache) | cache garbage writes (2266), fast-path diagnostic swallowing (2259/2267), freshness ambiguity (2604), warm-start loss (2510) |
| 7 | FEA/compute cluster (compute_targets, solver-elastic, gmsh seam, constraints solver) | rising cluster; trampoline-omission class (esc-2962-66, 4458, 4468); live segfault 4876 |

Corrections to the raw-count narrative (from the reviews): `persistent_cache.rs`'s 183-commit
churn is a young (8-week) feature built with disciplined TDD-step commits, not a chronic-bug
module; `reify-constraints/src/solver.rs`'s 41% fix ratio is numerical-tolerance whack-a-mole
inherent to Nelder-Mead, not architectural rot; FEA raw touch counts are partially feature
velocity. The cross-subsystem signal: **compiler+eval are co-touched in 143 fix commits** —
by far the strongest coupling (next: eval+kernels 43, GUI backend+frontend 27).

---

## H1. Eval-engine multi-path parity (deepest hotspot)

**Diagnosis (confirmed).** The per-cell "evaluate-and-commit" transaction (build ctx →
eval_expr → choose determinacy → write values+snapshot → record cache → record journal) has no
primitive — it is re-inlined ~15 times across 4 live paths (eval, eval_cached, edit_param,
guarded-group/unfold) and 2 dead ones. Census: ~40 `record_evaluation` sites; 51 inline
`journal.record` literals vs 5 uses of the 2195-extracted helper; 72 `eval_ctx_with_meta`
sites of which only 4 use the 4356-extracted `cell_eval_ctx`. `EvalContext` (reify-expr
lib.rs:46–94) is an optional-capability builder where every missing capability degrades
silently (missing determinacy → silent Undef at EX:946; missing sink → warnings dropped).
The "MUST keep .with_determinacy and .with_runtime_diagnostics" invariant is enforced by a
doc comment (engine_eval.rs:4690–4707) that itself concedes a known unfixed containment gap.
**~40% of the parity surface is dead code**: `edit_source` (engine_edit.rs:2369–3820, no
production callers), the entire concurrent stack (reify-runtime concurrent.rs + reify-eval
concurrent.rs — only test callers; wave-1 adapter still carries the un-fixed 4356-class bug
at concurrent_eval.rs:399–410), and `freshness_walk`. eval()'s structural-query/self-datum
passes overwrite values without cache recording (engine_eval.rs:3454–3463) — divergence that
eval_cached can serve stale (same family as live 4963/4973 "post-walk mint" compensation).

**Proposals (ranked).**
- P1 (S+M, low risk, highest leverage): extract `commit_cell_result(node, val, DeterminacyRule,
  TraceSource, version)` doing values+snapshot+cache+journal atomically, plus a `CellEvalCtx`
  constructor with *required* determinacy/sink args (free function taking functions/meta_map as
  params, killing the borrow-scope excuse). Convert all ~15 transaction copies; unrecorded
  sites become explicit `CacheLeg::Skip` decisions.
- P2 (M, needs product decision): delete or feature-gate the dead paths (edit_source,
  concurrent stack, freshness-only walk trigger). Re-home the shared-property tests first.
- P3 (M): route post-pass overwrites through the commit primitive; add a debug/test audit
  "every snapshot value's cache entry hash matches" (cheap structural version of task 4952's
  no-stale invariant; run warn-only first to enumerate today's divergences).
- P4 (M): diagnostics as data — attach per-node diagnostics to cache entries, replay on hits;
  single post-pass detector registry shared by eval/eval_cached/edit (replaces the "must run
  before fast-path" convention at engine_eval.rs:4987–4992 and the cold-only detector gap).
- P5 (S): one ordering core (make dirty.rs sorts delegate to the fixpoint Kahn core).
- P6 (L, endgame after P1/P2/P5): collapse eval/eval_cached into one mode-parameterized walk;
  characterization-test the documented intentional divergences first.
- P7 (M, independent): engine-side strict lookups (`get_strict` vs `get_or_undef` reserved for
  the DSL boundary) — coordinate with live 4952/4963/4973; and one numeric-promotion table in
  reify-expr replacing the ×6-copied arithmetic cascades (the main reify-expr fix driver).

## H2. engine_build.rs

**Diagnosis (confirmed).** `execute_realization_ops` (~1,794 lines) contains a ~200-line
cache-hit short-circuit where three separate bugs clustered (4349 double-insert; 3437
false-hit on intermediate realizations; an unnumbered perf regression) because the
short-circuit must *remember* to emulate every side effect of the main op loop — obligations
implicit, not typed. 11 distinct task IDs are cited inside this one function. The 4349 fix is
a defensive eviction workaround; the principled fix (re-key feature/topology tables by
`KernelHandle`) is already named in code comments as open **task #4351**. Version-id handling
has three conventions: raw allocate-and-bump duplicated verbatim at 5 sites; private
`current_eval_version()` (engine_build.rs:7780) that engine_admin.rs:2211 *cannot call* and so
reimplements as `next_version_id.saturating_sub(1)` with different edge-case semantics.
The ~24-parameter signature caused 11+ "thread new arg through every call site" fix commits —
the input-side twin of the problem `RealizationOutputs` (task 3119) already solved for outputs.
Per-build reset invariants (feature_tag_table & co.) are correct today but tribal: hand-copied
3-line blocks at 4 entry points, no structural guarantee for a 5th table/entry point.

**Proposals.** (1, S) `allocate_snapshot_version()` helper + make `current_eval_version()`
pub(crate), replace all three conventions. (2, S/M) `RealizationOpsInput` struct mirroring the
3119 precedent. (3, M) extract `probe_realization_cache()` and execute #4351 (KernelHandle
re-key) inside it. (4, M) `reset_per_build_state()` with exhaustive Engine-field destructure so
a new per-build field forces a compile-time decision (exclude realization_cache/warm_pool/
morph_source — must-survive state; classification list in the review). (5, L, staged last)
decompose along the crate's existing engine_<verb>.rs convention: engine_realize / engine_tessellate /
engine_post_process / cross_sub_realization; move the 10.7k-line test blob per-module. Do in a
dedicated merge window (narrow-lock starvation applies).

## H3. geometry_ops.rs + multi-kernel seam

**Diagnosis (confirmed).** The `Mesh` interchange type (reify-ir geometry.rs:2414, doc:
"for visualization") has no invariants and no validator; the only weld/closed/orientable
enforcement lives *inside Manifold's ingest* (the 4329 fix), so it protected only one pair —
4336 (winding) was the next latent property and live 4876 (gmsh attributed producer segfault
on unwelded OCCT output) is the same defect at a consumer the fix never touched. The handoff
executor (engine_build.rs:6692–6786) validates nothing between tessellate and ingest.
Real-vs-mock divergence is architecturally induced: 59 `impl GeometryKernel` exist (5 real,
~45 bespoke test mocks), while real-output cross-kernel tests number exactly two files.
Handle identity is folklore: per-kernel id counters all start at 1; `TopologyAttributeTable`
keyed by bare `GeometryHandleId` without kernel provenance (collision eviction hack at
geometry.rs:4404–4409); `extract_faces` stability was invented independently per kernel (4262)
and Manifold `extract_edges` is still un-memoized (dormant re-instance, currently masked by
BRepOnly gating). **Latent warm-start bug**: `with_warm_state` clears the three extracted_*
caches but neither serializes nor clears `parent_handle` (occt lib.rs:4142–4144) → stale/wrong
OwnerBody answers after restore; plus a stale pre-2510 doc at lib.rs:476–478. Stub-vs-real
parity is hand-mirrored comments (the 2405 mechanism), with the OCCT stub not even using the
shared `assert_stub_kernel_errors!` macro. Name dispatch: ≥9 string→behavior maps in the file
plus parallel compiler-side registries — one selector is registered in ≥7 places across 2
crates; a missed registration degrades to silent Undef. The 32.4k-line file is ~10.1k
production + ~22.3k embedded tests and is the mandatory merge-contention point for all
geometry DSL work.

**Proposals.** (C1, M) `MeshContract`/`ValidatedMesh` + `validate()` in reify-ir — lift the
weld+winding checker already hand-rolled in the 4336 test; wire at the handoff executor
(debug/test builds), call from Manifold ingest and the gmsh attributed producer (turns the
4876 segfault into a diagnostic); structured `GeometryError` variant. (C2, M–L) kernel-pair
conformance suite over *real* tessellated fixtures for every producer×consumer, incl. handle-
stability property tests (would have caught 4262; catches extract_edges today); house where
dev-deps link kernels; gmsh arm starts `#[ignore = "blocked on #4876"]`. (C3, S–M) one shared
contract-test suite instantiated for real AND stub. (C4, S) fix parent_handle + per-kernel
"state inventory" test classifying every side table persist/clear/rebuild; fix stale doc.
(C5, M) split the file along its clean internal seams (op_compile / query_dispatch / kinematic /
selector_build / topology_selector / arg_resolve / reply_decode / ad_hoc / surfacing); one window.
(C6, M) unified builtin-name metadata table; first step = cross-registry drift test.
(C7, S–M) finish capability-gate wiring (KGQ) + KernelHandle-keyed attribute tables (pairs with #4351).

## H4. GUI state bridge

**Diagnosis (confirmed).** Four parallel sync mechanisms with no choke-point: (1) per-field
delta events cover only 5 of 13 GuiState fields (diff.rs:65–253); (2) the full-snapshot
command return — the only universal channel — is *discarded* on the two hottest paths
(App.tsx:1663 handleSetParameter, App.tsx:1269 source edit: `.catch()` only); (3) six bespoke
emitters installed one-per-feature, fired via a 5-line "emit quintet" copy-pasted at 6 sites
in engine.rs — the 6th copy (load_from_compiled, engine.rs:5291–5300) has already drifted
(dropped emit_fea_diagnostics), and the team's own comment at engine.rs:1493–1500 names the
needed extraction; (4) phase-transition pull-refetch. Coverage matrix: `tensegrity_wires`,
`tensegrity_surfaces`, `display_panes`, `display_appearance`, `fea_convergence` all go stale
after every param edit (fea_convergence is structurally identical to the already-fixed #4884);
`demand_prune_measurement` is computed backend-side and read by nothing. The historical
1764→3351→3386→4252→4884 sequence is literally fields being promoted one at a time from
"silently stale" to "has a bespoke wire". `debug_server.rs` mutation handlers share the engine
Arc but bypass `AppState.last_state`, so subsequent normal commands diff against a stale
baseline (plausible driver of its 67-fixes-since-May). Sidecar lifecycle is *fixed* (unified
SidecarState + single terminal-event choke-point — good template). Landmine: system-prompt.ts
advertises `reify_set_parameter`/`reify_update_source`; the allowlist can't reach them; yet
claude_bridge.rs carries live interception into real engine-mutating tools
(reify-mcp/tools/write.rs) with **zero** frontend sync — widening ALLOWED_TOOLS would silently
reactivate the whole class for AI-driven edits.

**Proposals.** (C2 stopgap, S/M) wire the 4 stale list fields into StateDelta (tessellation_
diagnostics is a literal template) + a fea-convergence emitter cloning the #4884 fix — kills
the next 4–5 known-class bugs now. (C3, S) extract `post_engine_call_telemetry()` (already
scoped by the team as review suggestion #4, task 3541). (C1, L) schema-derived sync: either a
generic top-level GuiState diff/patch channel, or a derive that forces every field into
{diffed | explicitly-full-reload-only} at compile time. (C4, M) route debug_server mutations
through the same compute_delta/emit_delta sequence (mind the e2e harness's synchronous
assumptions). (C6, S, after C1/C2) field-coverage lint analogous to check_event_inventory.sh
(which checks names only, not coverage). (C5, S, needs Leo's call) sidecar drift: correct the
prompt to the real tool surface, or properly wire reify-mcp write-tools as a live MCP server —
and gate/delete the dead interception until it has sync coverage.

## H5. Compiler expr/type layer

**Diagnosis (confirmed).** FunctionCall arm = a 19-family if/else-if ladder (expr.rs:2868–3328)
backed by 8 signature-family files (~5.2k lines) that are literal structural copies
(joint_signatures ← math_signatures ← units::is_geometry_query; signatures_common.rs is an
admitted half-finished dedup), with pairwise disjointness enforced by ~3.4k lines / 13
hand-written `*_are_disjoint_*` tests in units.rs — a test suite compensating for a missing
registry. MemberAccess arm (expr.rs:3350–4654) is sequential if-guards over receiver type with
no exhaustive match; `Type` has 37 variants, no strum/completeness test (unlike IR enums), so
a new variant trips nothing. **Two new latent bugs of the 4603 silent-drop class found**:
(a) generic-trait type args silently discarded — `param m : SomeGenericTrait<Foo>` resolves to
`Type::TraitObject("SomeGenericTrait")`, Foo dropped, no diagnostic, no arity check
(type_resolution.rs:635–664 fall-through; the 4603 fix at :1688–1724 covers structures only,
and check_applied_type_arg_bounds documents its structures-only scope at :3982–3989);
(b) `Mul`/`Div` on non-scalar operand pairs (Vector*Vector, Tensor*Tensor, …) silently infer
`Type::Int` (type_compat.rs:1576–1596 `_ =>` arms) — guards exist for Add/Sub/comparisons/
logical/Mod/Pow but not Mul/Div. Positive: phase pipeline ordering is explicit and documented;
ConstraintChecker dependency inversion is clean; the 17 resolve_* entry points are mostly
legitimately distinct (collapse assessed and rejected).

**Proposals.** (1, S, do first) trait-with-args arm mirroring the structure arm (route to
Type::Applied + bounds, or explicit rejection diagnostic). (2, S) Mul/Div operand-kind guards
(first write the reachability test). (3, L) unified builtin-signature registry, migrated
family-by-family, each swap validated against then deleting the corresponding disjointness
test. (4, M) reshape MemberAccess into one exhaustive `match` on receiver Type with a named
tail arm (pattern proven at type_compat.rs:429–490). (5, S) dedupe the auto_free
decl-construction blocks (entity.rs:1925/3165 + guards.rs).

## H6. Caching / freshness substrate

**Diagnosis (confirmed).** Two structurally disjoint invalidation walks: the dirty cone
(freshness-unaware) and `propagate_freshness_only` — built for async compute completions
(audit finding M-013) and **never called in production**; `run_compute_dispatch` invokes
neither on completion, so an async completion never invalidates dependents (the real, sharper
form of 2604). Cache-write and diagnostic-emission remain decoupled by API shape
(record_evaluation takes no diagnostics; NodeCache has none) — the 2259/2267 fix is
convention+tests, and a new per-cell-branch diagnostic would silently regress.
The deepest root cause is graph incompleteness: geometry `let`s lower to RealizationDecl with
no value cell (entity.rs Let arm), so `build_combined_param_let_graph` never nodes them —
proven root cause in docs/prds/v0_6/eval-uniform-dependency-handling.md:25, fix ratified but
deferred (unified DAG executor, task #4727-sequenced), interim invariant checker = live task
4952. **Note: freshness-gap and graph-incompleteness are two different root causes that both
present as "silent stale/Undef".** Freshness::default() = Final means an absent cache entry
reads as authoritative (silent-default shape). Warm-start drift: new code has a compiler-
enforced pattern (elastic_result.rs exhaustive struct literals, :617–642) — old code
(OcctKernel::warm_state) builds by manual copy, so the next field silently drops (2510's other
half); `last_warm_start_failures` is invisible in production (eprintln only). VersionId
*reading* is centralized (engine_build); *allocation* is not (see H2). persistent_cache.rs is
healthy (see corrections above). LSP: hypothesis — `compute_module_hash` hashes import paths
not content, so a changed imported file may reuse stale per-cell entries (untraced).

**Proposals.** (1, S–M) wire `propagate_freshness_only` into compute-dispatch completion (or
prove unreachable and delete); comment the invariant at the load-bearing end. (2) = H2's
reset_per_build_state. (3, S) exhaustive `let Self {…} = self` destructure in
OcctKernel::warm_state + tracing::warn for restore failures + stale-doc fix. (4) back task
4952's checker; freeze further investment in build_combined_param_let_graph per the PRD's own
guidance — the unified DAG executor is the keystone (also addresses the 143-commit
compiler+eval co-fix coupling). (5, M, prophylactic, after 1–4) cache-entry constructor that
takes an explicit (possibly empty) diagnostics vec. (6, housekeeping) single-file persistent
cache entry format (deletes the crash-orphan path).

## H7. FEA / compute cluster

**Diagnosis (confirmed).** Trampoline registration is a repeated-omission class with three
confirmed incidents (esc-2962-66 GUI, 4458 cmd_build, 4468 run_tests), each fixed via an
independent hand-assembled bundler; no canonical constructor exists and the GUI bundler has
*already* drifted (missing morph-producer registration vs CLI). The LSP creates 11 bare
engines with no registration, undocumented and untested — FEA constraint violations are
invisible in the editor (Indeterminate dropped at diagnostics.rs:190/236), same meaningless-
pass shape as the CLI bugs. The `catch_unwind` boundaries (engine_eval.rs:6658/7613) wrap only
the body-inline *fallback*; the primary compute-dispatch path has none, and the FEA trampoline
is built on ~30 panic!-based Value-shape asserts — GUI is saved by engine_lock.rs's
poison-safe wrapper; **CLI has zero catch_unwind** → process crash reachable. Task 4876 root
cause confirmed in-code: the attributed gmsh producer forbids vertex-merging repair, its only
real caller feeds raw unwelded OCCT tessellation, and the tetgen segfault is uncatchable; the
current mitigation is a scope-narrowing gate. NaN-unsafe `partial_cmp().unwrap_or(Equal)`
fixed at 1 of 4 sites (through_thickness.rs) — mesh_boundary.rs:514–531, adaptive.rs:266–278,
interpolation.rs:374–391 remain. Constraints-solver churn = inherent numerical tuning; FEA
touch counts partially feature velocity.

**Proposals.** (1, S) `Engine::new_production(...)` in reify-eval bundling all registrations;
migrate CLI/GUI/test_runner; grep-based architecture test that they delegate; fixes GUI morph
gap as a side effect. (2, S) LSP posture decision + locking test (mirror cmd_check's
documented posture). (3, S then M) catch_unwind at invoke_compute_trampoline → ComputeOutcome::
Failed (+ regression test feeding Undef per arg); then Result-based extract_* refactor for
diagnostic quality. (4, M/L) 4876: Rust-side watertightness preflight (degrade gracefully to
the plain producer) now; attribution-aware repair (merge-correspondence map) as the real fix.
(5, S) `f64::total_cmp` / finite-guard sweep at the 3 remaining sites.

---

## Latent bugs found by this review (fileable now)

1. OCCT `with_warm_state` neither persists nor clears `parent_handle` → wrong/stale OwnerBody
   after restore (occt lib.rs:517, 4142–4144); stale pre-2510 doc at lib.rs:476–478.
2. Generic-trait type args silently dropped in type resolution (type_resolution.rs:635–664).
3. `Mul`/`Div` non-scalar operands silently infer `Type::Int` (type_compat.rs:1576–1596).
4. Manifold `extract_edges` un-memoized — dormant 4262 re-instance (kernel-manifold kernel.rs:812).
5. GUI: tensegrity_wires/surfaces, display_panes/appearance, fea_convergence stale after every
   param edit; demand_prune_measurement dead on arrival (engineStore has no field).
6. GUI: 6th emit-quintet copy dropped emit_fea_diagnostics (engine.rs:5291–5300, test-only path).
7. debug_server mutations bypass the delta baseline → stale-baseline desync for subsequent commands.
8. LSP: FEA constraint violations produce no diagnostics (undocumented trampoline-free posture).
9. CLI: compute-trampoline panic path uncaught → whole-process crash on malformed/Undef inputs.
10. Concurrent wave-1 adapter still carries the 4356-class bare-context bug (dead code, but a
    trap if ever wired) (reify-runtime concurrent_eval.rs:399–410).
11. eval() structural-query/self-datum passes write values without cache recording →
    eval_cached can serve stale pre-expansion values (engine_eval.rs:3454–3463).
12. `propagate_freshness_only` never called in production; async compute completion never
    invalidates dependents (engine_admin.rs:2075; engine_compute.rs run_compute_dispatch).
13. Sidecar tool-surface drift: system prompt advertises tools the allowlist blocks; dormant
    unsynced engine-mutation path behind it (system-prompt.ts:67–83, session.ts:81,
    claude_bridge.rs:424–487, reify-mcp/tools/write.rs).

Task-tracker hygiene observed in passing: duplicated deferred tasks (4963/4973 identical;
4974/4976–4979 same title ×5) from the "auto-eval redo" batch.

## Cross-cutting themes (the systemic program)

1. **Extract-but-abandon at 3-of-N sites.** Every subsystem has a correct extraction adopted
   partially: cell_eval_ctx (4/72), record_eval_completed (5/51), signatures_common (1 helper),
   RealizationOutputs (outputs only), reset_dispatch_tallies (2 counters), emit quintet
   (named-but-not-extracted). Convention: an extraction isn't done until adoption is total and
   the old shape is unrepresentable (required args, typed transactions, exhaustive destructure).
2. **Silent defaults.** Undef puns (not-yet/failed/missing), Type::Int fallbacks,
   Freshness::default()=Final, None-capability contexts, missed name registrations degrading
   to Undef. Matches the established project norm (feedback_silent_defaults_pattern); the
   engine-boundary fix is strict Result-typed lookups + provenance.
3. **Contracts held in one consumer instead of at the seam.** Mesh contract in Manifold's
   ingest; GuiState sync in per-field bespoke wires; trampoline registration at N call sites.
   Fix shape: seam-owned contract type + validator + conformance suite over real outputs.
4. **Dead parallel implementations consume the parity budget.** edit_source, concurrent stack,
   freshness walk, reify-mcp write-tools interception. Decide: wire with tests, or delete.
5. **Registry vs match-arm sprawl.** 19-family compiler ladder, ≥9 geometry name maps, GUI
   event wiring — replace disjointness-by-test with disjointness-by-construction; first step
   everywhere is a cheap cross-registry drift test.
6. **The keystone**: the ratified-but-deferred unified DAG executor (eval-uniform-dependency
   PRD / #4727 sequencing) addresses the graph-incompleteness root cause, the eval-path
   unification endgame (H1 P6), and the 143-commit compiler+eval co-fix coupling. Sequence H1
   P1/P2 as its enablers.

## Suggested sequencing

- **Wave 1 (small, low-risk, immediate):** H7-1/2/3a/5; H6-1/3; H2-1; H3-C4; H5-1/2; H4-C2/C3;
  file the latent-bug list as tasks.
- **Wave 2 (medium):** H1-P1/P3; H2-2/3/4; H3-C1/C3/C6-step1; H4-C4/C6; H5-5.
- **Wave 3 (large, dedicated windows/PRDs):** H1-P2 decision then P6; H3-C2/C5; H5-3/4;
  H4-C1; engine_build decomposition (H2-5); unified DAG executor.
