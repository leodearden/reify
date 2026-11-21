# PRD — Realized-world evaluation: one resolution site for realization-dependent values

**Status:** decomposed 2026-11-17: ξ #10153, α #10154, β #10136 (adopted), ι #10155, γ #10156, δ #10157, ζ #10158, η #10159, θ #10160, π #10161, ρ #10162, σ #10163, ω #10164. v0_6. Approach **B + H** (§5 contract, §6 two-way boundary tests). Origin: escalation esc-9638-2 (direction-typing ε #9638, row B17) and the class investigation that followed. Leo ruled R1–R18 (§11). One adversarial critic pass (fable/high: 3 blockers, 11 majors, each checked in code or by probe) and one fold-verification pass (opus/high) are folded.
**Code anchors.** Substrate verified against main `dd2a97059a` (2026-11-17) by reading and probing seats. Everything is cited by symbol; counts and probe results live in the dated §10. Main moves fast: re-locate every symbol at implementation time.
**Invariants touched:** INV-SF-1 `undef-has-provenance` (a new `UndefCause`); INV-ARCH-1 `module-sccs-only-shrink` (the one module this PRD adds imports nothing in the reify-eval engine SCC); INV-BUILD-3 (cache-hit parity: leaf ξ makes the swept kind part of what a hit restores). No registry status cell is flipped by this PRD.

Leaf labels in this document are this PRD's own. A label from another PRD is always written with its task number (ε #9638, κ #9263, μ #9651). **Leaf α here is the leaf direction-typing calls ε′.**

---

## 1. Goal, consumers and user-observable surface (G1, G2)

**Goal.** A value that depends on realized geometry has one meaning wherever it is written. `circular_pattern(g, cyl.axis, 4, 90deg)` and `let a : Axis = cyl.axis` followed by `circular_pattern(g, a, 4, 90deg)` build the same solid, on every walk. The engine reaches that by evaluating each such value once, at its dependency-ordered slot, with one evaluator and one resolver, instead of through passes that each recognise one top-level expression shape.

**The guarantee (R5).** *Inline equals let-bound for every realization-dependent member whose geometry receiver is a cell.* It is delivered kind by kind; §9 names which leaf delivers which kind, and each leaf inverts the old pins for its kind on purpose.

**Consumers.**
- **direction-typing μ #9651** (row B17) consumes α: kernel-minted datums reach geometry ops, inline and let-bound.
- **`.ri` authors** consume every leaf through `reify eval` / `reify build` and the GUI: the forms in §10.1 that print `undef` or fail with "got undef" today build.
- **#10136** (silent-wrong geometry in overridden sub instances) is leaf β, which follows instantiation-value-flow β #6592 (R18).
- **#7256** and **#8737** (GUI load and selective-warm paths) consume α's tessellate-surface changes.
- **#9457** (kernel-resolved values in struct-ctor fields and fn-call args) consumes α. **#8513** (a user fn over a sub's geometry never folds) consumes γ. **material-waste-cost-minimisation** made its stock helpers trait lets because a geometry query inside a user fn never folds (`tests/prd-gate/fixtures/mwaste_fn_solid_no_fold.ri`); γ lifts that constraint.
- **#4482** (GD&T datum-anchored zones) may consume α's datum resolution; undetermined until its design says whether zones are realizations. No edge.

**Engine seam (overlay G1 sub-check).** The mechanism plugs into engine-integration-norm §3.1 (op-execute: op arguments) and §3.2 (realization-kind dispatch: the scheduled `Realize` and `HydrateCell` steps). It adds no new seam.

---

## 2. The class

**Realization-dependent values** are expressions whose value is a function of realized geometry state: the kernel shape behind a handle, or a per-realization facet (topology attribute table, swept-kind table, realized repr). The kernel-free evaluator cannot produce them, and they are defined only after a specific realization has run.

- **Leaf members** are a closed set of expression nodes whose own semantics read realized state:
  - whole-handle geometry queries (`volume`, `area`, `centroid`, `bounding_box`, `max_deviation`);
  - feature→datum projection (`.axis`, `.plane`, `.point`, `.dir` on a Geometry or Selector receiver);
  - selector→geometry coercion (`ResolveSelector`, index over a selector, `single(selector)`);
  - kernel-bearing topology helpers (`adjacent_faces`, `shared_edges`, `closest_point`, …);
  - `feature(g)`; ad-hoc selectors (`g @ face("top")`);
  - conformance predicates; kinematic queries; mass properties.
- **Derived members** are cells whose dependency cone reaches a realization and that are not themselves geometry cells or compute-node outputs (`h = volume(g) / 25mm^2`, `c2 = axis_z(c)`). α treats `Let` cells only. A `Param` whose default reads geometry stays on today's path until γ, because re-evaluating a default while a binding is in effect would revert the binding.
- **Not members:**
  - kernel-free symbolic selector constructors (`faces`, `edges_at_height`, `union`), already uniform through #7875's capability;
  - datum→datum projections (`a.dir`), which are pure and are ε #9638's decoder work;
  - restricted-field containment, which the engine answers through `ContainmentQuery` on evaluation surfaces as well (D4);
  - a member over an *inline* geometry receiver (`revolve(..).axis`, `volume(box(..))`), which needs a realization to exist. The `__geoq` hoist covers the query form; the datum form stays out of scope (§7).

**Position** (displayed cell, let consumed by an op, inline op argument, nested argument, under a lambda or user fn, derived let, constraint, guarded group, cross-sub, compute argument) and **walk** (build, tessellate cold and selective-warm, the relate nested build, an overridden instance) are not part of the definition. They are the axes along which a member must behave the same.

---

## 3. Problem (measured; §10)

Kind is not the discriminator. On main:
- a let-bound geometry query feeding an op works; the same query inline in the op argument is Undef;
- a let derived from another member's let and consumed by an op is Undef for every kind probed, although the let itself displays the right value;
- a feature datum is Undef at a consuming op both inline and let-bound (the B17 trigger);
- a member inside a user fn body is Undef; a member inside a `flat_map` lambda works only for the two kinds that hand-built a lambda walker.

**Root cause.** reify-expr is kernel-free, so a kernel-dependent node evaluates to Undef, and a *pass* that matches a cell's top-level expression shape overwrites the Undef afterwards. The unit of resolution became (pass × top-level shape) instead of (expression node). Heuristic 3 (carefully factored orthogonal dimensions of variability) names the result: four axes implemented as a product of hand-built patches.
- **Kind:** fourteen `post_process_*` passes and at least six recognizer name lists that disagree about the same class.
- **Reach:** a shape match cannot see a nested member, so each kind grew its own patch: `rewrite_geometry_queries`, the `__geoq` and `__sel` hoists, #5208's Modify-only `prehydrate_inline_curated_selector_args`, `try_eval_topology_flat_map`, `try_eval_swept_kinematic_query`, and documented gaps.
- **Timing:** #4358 copied two passes into the in-loop `hydrate_value_cell_in_loop` ladder under a prose "SYNC REQUIREMENT". Kinds added later went post-loop only.
- **Walk:** each pass is hand-called per driver, so parity is copy-paste.

The **timing half is already solved.** The dependency edges in `deps.rs` order every consumer, inline or let-bound, after the receiver's realization. Only what runs at the slot is missing.

---

## 4. Resolved design decisions

- **D1 — One evaluator reaches members at any depth (R1).** reify-expr gains one capability trait, `RealizedQueries`, on `EvalContext`, probed at member nodes. It is defined kernel-free, as `ContainmentQuery` is, and implemented in reify-eval. This reverses, for the kernel-bearing half, the Strategy B ruling of 2026-06-28 (`docs/design/symbolic-eval-nested-selector-resolution.md`). The deciding evidence is §10.2: a fold at the slot could delete neither `flat_map` walker and could never reach a user fn body.
- **D2 — One resolver, value-keyed.** `resolve_realized(kind, args, world)` is one exhaustive match. Arguments arrive already evaluated. No resolver looks a geometry argument up by name, and no argument is a `Value::Lambda`: the evaluator applies lambdas before any probe.
- **D3 — The schedule is the only clock.** A member is evaluated at its node's scheduled slot (`Realize` op arguments, `HydrateCell`), by the same evaluator.
- **D4 — Three capabilities, one axis.** `EvalContext`'s builtin capabilities differ in exactly one thing, the state an answer needs: none (`SymbolicSelectorCtorFn`, attached on every surface); the engine's retained state (`ContainmentQuery`, attached to cell contexts on evaluation surfaces too, and a required argument of S4's `cell_eval_ctx`); this walk's realized world (`RealizedQueries`, attached only by a realization walk). #7886's item (B) keeps its own capability and adopts the same value-keyed argument convention.
- **D5 — Lazy evaluation is language semantics (R9).** An unselected conditional or match branch is not evaluated: it raises no diagnostic and makes no kernel query. Scheduling is not lazy: the consumer is still ordered after every realization either branch reads.
- **D6 — The identity fold never sees a realized world.** `compute_realization_upstream_values_hash_from_ops` is evaluated without the capability on every surface. An *inline* member therefore contributes the same fixed content (Undef's) at the eval mint, at each stamp, at edit time and at the GUI hash gate. Staleness for realized inputs rides the dirty cone and entity-family eviction, as it does for `GeomRef::Sub` operands; the realization cache is keyed by entity, repr and tolerance, not by this hash.
  - **Stated limit.** A *let-bound* derived cell is realized in the walk's values and Undef at the mint, so a consumer reading one is treated as changed on every edit (a conservative re-run, as today). π removes the difference by folding against a view with derived-member cells masked.
- **D7 — The generic rung writes only realized, determined values.** At `HydrateCell` the realized evaluation writes a cell only when the result contains no Undef and no handle or selector target whose `kernel_handle` is `None`. Otherwise it writes nothing and the remaining rungs run. When it declines, the diagnostics and undef causes its evaluation produced are dropped, as prehydrate does today, so a surviving rung stays the one reporter. The exception is a feature-datum failure, which no surviving rung handles: the resolver reports it once per walk through D8's memo, and the post-loop driver consults the same memo. D7 applies to this rung only; the surviving rungs keep their own write rules.
- **D8 — Memo.** One memo per driver invocation, never an `Engine` field (the relate pre-pass's nested build must not share it). Key: the kind, each argument's content hash, and the resolved `KernelHandle` of every handle an argument embeds. It stores successes only and records which failures were already reported, so a failing member reports once per walk.
- **D9 — One value per selector cell.** A selector cell always holds its descriptor. A curated Modify argument gets the compiler's `ResolveSelector` coercion, and `ResolveSelector` is value-driven at runtime: a Selector resolves, a List passes through. Ladder rung (b) (selector cell → List when a realization reads it) is deleted.
- **D10 — No realized world means a loud Undef (R10).** A member evaluated without a world is `Undef` with `UndefCause::AwaitingRealization`. A member whose resolution failed carries the failure's cause, not that one. The override paths get no world until β gives an instance run one.
- **D11 — Tessellate stamps values only, for now (R12).** After each `Realize` step `tessellate_from_values` publishes the realized handle into the walk's `values`, as the build drivers do. On tessellate this writes no oracle entry, no cache node and dispatches no compute. S2's R3 and R6 stay closed. Trigger to revisit: #9165 (§7).
- **D12 — Selective-warm widens the must-run closure (R7).** The hash gate's producer closure follows value reads as well as `GeomRef::Sub` operands.
- **D13 — Membership has one home (end state).** The builtin registry binding (`Pure | SymbolicCtor | Realized(RealizedQueryKind)`) is stamped into the IR by the compiler; eval holds no name list (leaf η). Until then each migrated kind is recognised structurally by its IR node (`MethodCall` typed by receiver, `ResolveSelector`, `IndexAccess`), never by a new list. The datum member names have one home, `DatumMember::from_method` in reify-ir; `FEATURE_DATUM_PROJECTION_MEMBERS` is derived from it until η.
- **D14 — B + H.** Blast radius is four crates, the mechanism count is above eight, and the op-execute seam is load-bearing.
- **D15 — A cache hit restores the swept kind (R13, R17).** The realization cache entry carries the terminal's swept kind and `probe_realization_cache` records it on a hit. S2 κ #9263 later folds the field into its `CachedRealization`.
- **D16 — One reporter per failure.** A kind whose consumer already reports the failure stays silent in the resolver: a curated selector argument that does not resolve is reported by the op arm, as today, and the resolver never turns a failure into an all-edges fallback. A datum failure is reported by the resolver (`FeatureDatumAmbiguous`), once (D8). Today's diagnostic counts are the floor.

---

## 5. Contract (H)

### 5.1 Member kinds (reify-ir)

```rust
pub enum RealizedQueryKind {
    FeatureDatum(DatumMember),            // α: .axis | .plane | .point | .dir
    SelectorGeometry(SelectorCoercion),   // α: List | Single | Index(i)
    // γ: GeometryQuery(..)   δ: TopologyHelper(..), FeatureOf, AdHocSelector(..)
    // ζ: Conformance(..), Kinematic(..), MassProps(..)
}
```
A variant without a resolver arm is a compile error (heuristic 12).

### 5.2 The capability (reify-expr)

```rust
pub trait RealizedQueries {
    fn resolve(&self, kind: &RealizedQueryKind, args: &[Value]) -> RealizedAnswer;
}
pub enum RealizedAnswer { Value(Value), AwaitingRealization, Failed(UndefCause) }
```
- `EvalContext` gains `realized: Option<&'a dyn RealizedQueries>`. It is `None` in all three constructors (`new`, `simple`, `_test_at_depth`). `with_scope` and the forall literal copy it, so user-fn bodies, let blocks, lambdas, variant-bind arms and quantifiers inherit it.
- **Probe sites in α:** the `MethodCall` arm, after the receiver is evaluated and before `eval_method_call`, when the member is a feature-datum projection and the receiver is a `GeometryHandle` or `Selector`; the `ResolveSelector` arm; `IndexAccess` over a selector; `single(selector)`. Each probe is one `#[inline(never)]` helper (reify-expr's stack budget).
- **No world:** a datum probe yields `Undef` and records `AwaitingRealization`. `ResolveSelector` passes its inner value through, as today.
- **`Failed(cause)`:** the node evaluates to Undef and records that cause.
- `eval_ctx_with_meta` never attaches the capability; only §5.4's `OpEnv` and the HydrateCell slot do.

### 5.3 The world and the resolver (reify-eval, `realized_world.rs`)

```rust
pub(crate) struct RealizedWorld<'w> { /* kernel: RefCell<&'w mut dyn GeometryKernel>,
    swept_kinds, topology_attributes, realized_reprs (template-start copy),
    realized: walk-local map from realization to its kernel handle,
    memo: &'w RefCell<RealizedMemo>, diagnostics: RefCell<Vec<Diagnostic>> */ }
impl RealizedQueries for RealizedWorld<'_> { /* -> resolve_realized */ }
pub(crate) fn resolve_realized(kind: &RealizedQueryKind, args: &[Value], world: &RealizedWorld<'_>) -> RealizedAnswer;
```
- **Receiver resolution.** A `Value::GeometryHandle` whose `kernel_handle` is `Some` is used directly. One whose `kernel_handle` is `None` is looked up in the world's walk-local map, which is keyed by (realization, `upstream_values_hash`) and records each realization's outcome: its kernel handle, or the cause it failed with. A receiver whose realization failed answers `Failed(cause)`; only a realization that has not run answers `AwaitingRealization`. A `Value::Selector` resolves its target the same way.
- **Module home (R16).** `realized_world.rs` is a crate-root module that names no `Engine` and imports only modules below the engine SCC (`feature_datum`, `sweep_classifier`, `topology_selectors`, reify-core, reify-ir, reify-expr). α therefore *moves* the value-keyed bodies it needs out of `geometry_ops.rs` into it: `resolve_selector_to_list` from its parent-target step onward (taking a selector value), `eval_selector_feature_datum`, the handle arm of `try_eval_feature_datum_projection`, the `single` and index unwrap of `try_eval_resolve_selector`, and their helpers (`first_leaf_target`, `route_capability` with `CapabilityRoute`, `selector_is_attribute_role_leaf`, `selector_is_provenance_leaf`). `reconstruct_selector_value` stays in `geometry_ops.rs` with the thin expression-keyed adapters that the surviving passes and rungs call; they call downward. `python3 scripts/assert-module-scc.py` stays green with the SCC at 24.
- The world is built from fields `RealizationOpsInput` and the walk's outputs already carry (S2 §5.1's rule: no third place), including `realized_reprs`, which is kept and re-pointed from prehydrate to the world. It is scoped to one op compile or one cell evaluation; the memo belongs to the driver (D8).
- Selector coercion keeps its repr gate (`QueryNotSupportedOnRepr`). The default kernel is used, as today, until #6366 gives a handle its kernel.

### 5.4 `OpEnv` (reify-eval)

```rust
pub(crate) struct OpEnv<'a> { pub values: &'a ValueMap, pub functions: &'a [CompiledFunction],
    pub meta_map: &'a MetaMap, pub realized: Option<&'a dyn RealizedQueries> }
impl OpEnv<'_> { pub(crate) fn eval(&self, expr: &CompiledExpr) -> Value; }
```
- It replaces the `(values, functions, meta_map)` triple in the 71 functions and 7 compile-fn typedefs of `geometry_ops.rs` that carry it. `OpEnv::eval` is the only way op-argument code evaluates an expression: the direct `eval_expr` call sites in the per-op compile functions go through it.
- **One bundle across three PRDs (heuristic 11).** This is the struct S2 §5.3 calls `OpEnv` (δ #9158 adopts it rather than adding a twin). S4 τ #8872's `op_arg_eval_ctx` becomes the body of `OpEnv::eval`'s context for op arguments; the realization-hash callers use the world-free form (`realized: None`), which is D6.
- The bundling is α's first commit and is behaviour-preserving (`realized: None` everywhere; the characterization suite is byte-identical). It is produced by a recorded, re-runnable rewrite so a rebase can regenerate it.
- `compile_geometry_op` keeps a thin signature for its test callers.

### 5.5 Slots

- **Realize.** `execute_realization_ops` builds the world for each op compile, after the cache probe, and compiles the op with `realized: Some(&world)`. There is no pre-resolve step.
- **HydrateCell.** The driver computes `derived_member_cells` once per walk from the graph (§2's cell rule) and passes it through `CellHydrationInputs`, replacing `realization_read_cells` (the field and `Engine::realization_read_cells` are deleted with rung (b)). For a cell in that set the step first evaluates `default_expr` with the realized context and writes under D7; otherwise, or when D7 declines, the remaining rungs run in today's order. Compute-owned cells are never in the set, so no `@optimized` body is evaluated without a dispatcher. Members of guarded groups are excluded until θ.
- **Rungs.** α deletes rung (b). Rung (d) (`try_eval_resolve_selector`) stays until δ, because `single(<relational helper>)` resolves in-loop only through it. Rungs (a), (c), (e) stay until γ and δ.
- **After each Realize** the driver publishes the handle into `values` (D11). The build drivers already do this under the unified schedule through `hydrate_after_realize`; `tessellate_from_values` gains the values-only form (the per-handle body of `hydrate_geometry_handles_into_values`).
- **Post-loop.** `post_process_feature_datum_projections` remains until π as a driver over the same resolver for cells still Undef. It holds no second implementation.
- **Curated Modify arguments.** `compile_modify_op` wraps its five curated slots (`fillet`, `chamfer`, `chamfer_asymmetric` `edges`; `draft` `faces`; `shell_open` `open_faces`) in `ResolveSelector` unless the argument's static type is already `List<Geometry>`. The op reads the resolved list through `resolve_curated_edges_p2` as today. `prehydrate_inline_curated_selector_args`, `CURATED_SELECTOR_ARG_NAMES` and `InlineSelectorMemo` are deleted.
- **Identity fold, edit, plain eval, the override paths:** no world (D6, D10).

### 5.6 Selective-warm closure

`with_sub_producers` becomes a producer closure over two edge kinds: `GeomRef::Sub` operands, and the realizations producing any geometry cell in the realization's value-read cone. The reads come from `extract_realization_dependencies(&ops).reads`, then transitively from each read cell's `default_expr` through `CompiledExpr::collect_value_refs`, and each geometry cell maps to its producers through `geometry_cell_realization_reads(graph)`; `hash_gate_verdict` already receives the graph. A stale demanded consumer therefore pulls an unchanged producer it reaches through `cyl.axis` back into the must-run set.

### 5.7 Realization-derived cells in an instance (β; follows instantiation-value-flow β #6592)

Per-instance realization is owned by `docs/prds/v0_6/instantiation-value-flow.md` (contracts C1 "one effective valuation" and C2 "values ≡ geometry"; mechanism Shape B). Its β #6592 generalises `realize_sub_override_handles` and `realize_overridden_instance_into` to compile an instance's ops against the instance's own cells, all of them, recursively. That alone turns §7's interim loud: an instance's realization-derived cell (`c = centroid(ref_box)`) is undef at instance scope, so the op that reads it fails instead of taking the default instance's value.

β here adds the one thing that contract does not: those cells get values. Within #6592's instance realization:
- **Order and slots.** The instance's realizations run in the child template's schedule order, not declaration order, with a `HydrateCell` slot between them for the instance's derived-member cells.
- **World.** Each op compile and each slot gets a realized world over instance-local swept-kind and topology-attribute tables and an instance-local realized map. The map's key (§5.3) is the instance-scope mint identity #8864 defines: the template's realization id with an instance-folded `upstream_values_hash`. The per-Realize stamp is D11's values-only form.
- **Where the value lands.** The slot writes the instance's scoped cell (`ValueCellId("<parent>.<sub>", member)`), so C2 holds: the value readable on the instance is the one its geometry was built from.
- **No records.** An instance run stamps no `input_cone_hash`, oracle entry or node-cache entry, and neither probes nor inserts the realization cache under the child entity's key (that key would serve the default instance's terminal).
- **Everything else is #6592's.** Its overlay, recursion, dedup, trigger and `named_steps["<sub>.<member>"]` contract are β's substrate and do not change.

β is design-first: its first commit confirms these against #6592 as landed and escalates any it cannot keep, before the implementation commits.

### 5.8 Ratchets (`tests/infra/adoption-ratchet.manifest`; grammar in its header)

| Row | ERE | Path | Today | Lowered by | End |
|---|---|---|---|---|---|
| curated-prehydrate | `prehydrate_inline_curated_selector_args\|CURATED_SELECTOR_ARG_NAMES\|InlineSelectorMemo` | `crates/reify-eval/src` | present | α | 0 |
| value-resolving-pass | `^ *(pub(\(crate\))? )?fn post_process_` | `crates/reify-eval/src/engine_build.rs` | 14 | γ, δ, ζ, θ, π | ≤ 2 (`post_process_geometry_handle_cells`; `post_process_containment_samples` if π finds it still needed) |
| shape-recognizer | `^ *(pub(\(crate\))? )?fn try_eval_` | `crates/reify-eval/src/geometry_ops.rs` | 11 | γ, δ, ζ, π | 1 (`try_eval_symbolic_topology_selector`) |

α adds the rows at today's counts. In the manifest the curated-prehydrate ERE uses a bare `|`; the `\|` above is Markdown escaping. The ratchet reds only when a count exceeds its ceiling, so a leaf that moves a counted function to another file (S2 ι₃ #9163, S13 γ #8749) widens the row's path to the destination in the same diff; otherwise the row reads 0 vacuously.

Deletion owners, by function. γ: `post_process_geometry_queries`, `try_eval_geometry_query`, and the geometry-query use inside `post_process_cross_sub_value_cells`. δ: `post_process_topology_selectors`, `post_process_feature_accessor`, `post_process_ad_hoc_selectors`, `try_eval_topology_selector`, `try_eval_resolve_selector`, `try_eval_feature_accessor`, `try_eval_ad_hoc_selector`, `try_eval_topology_flat_map`. ζ: `post_process_conformance_queries`, `post_process_kinematic_queries`, `post_process_cross_sub_kinematic_queries`, `post_process_body_mass_props`, `post_process_mechanism_mass_props`, `try_eval_conformance_query`, `try_eval_kinematic_query`, `try_eval_swept_kinematic_query`. θ: `post_process_cross_sub_value_cells`. π: `post_process_derived_lets`, `post_process_feature_datum_projections`, `try_eval_feature_datum_projection`.

---

## 6. Boundary-test sketch (two-way)

Producer side is the capability and resolver; consumer side is every walk and position. Rows marked † are α's gate; the rest name their leaf. Rows that name a tessellate entry are Rust tests over `Engine::tessellate_realizations` / `tessellate_snapshot`; the CLI reaches only the build surface (`reify eval` runs `realize_for_check`).

| # | Scenario | Precondition | Postcondition |
|---|---|---|---|
| B1† | Inline datum in an op argument | `circular_pattern(g, cyl.axis, 4, 90deg)`; `mirror(b, top.plane)`; ε #9638 landed | Builds; the solid equals the let-bound form's |
| B2† | Let-bound datum consumed by an op | `let a : Axis = cyl.axis`, then the pattern | Builds; `a` displays as today |
| B3† | Nested and derived | `axis_z(cyl.axis.origin)` inline; `let o = a.origin` then `axis_z(o)` | Both build |
| B4† | Under a user fn and under a lambda | `fn ax_of(c : Solid) -> Axis { c.axis }` as a let and as an op argument; a lambda whose body projects a datum | All resolve |
| B5† | Receiver that is itself a member | `single(faces_by_normal(b, ..)).plane` as a let and as a `mirror` argument | Both resolve |
| B6† | Guarded branch | `if false then cyl.axis else axis_z(..)` | Builds; no diagnostic and no kernel query for the unselected branch |
| B7† | Curated selector arguments | `fillet(b, edges(b), r)` inline and let-bound; `fillet(b, union(e1, e2), r)`; an argument that is already a List; `let e = single(shared_edges(top, side))` then `fillet(b, [e], r)` | All build with the edges today's build selects; the three deleted symbols are absent |
| B8† | Selector cell holds one value | a selector cell read by a realization and by a composition; a let-bound curated fillet, cold and selective-warm | The cell is a descriptor in-loop and post-loop; the fillets build |
| B9† | Let derived from an in-loop-resolved member's let | `let v = volume(g)`, `let h = v / 25mm^2`, then `extrude(.., h)` | Builds (declared change: undef → value) |
| B10† | Identity: unrelated edit, selective tessellate | consumer with an inline member; edit a param the receiver does not read | Consumer is in the exempt set; its dispatch count is 0 |
| B10b† | Identity: let-bound limit (D6) | the same with the member let-bound | Consumer re-runs (conservative); result unchanged |
| B11† | Identity: receiver edit, selective tessellate | edit a receiver param | Consumer re-runs; its mesh follows; nothing stale is served |
| B12† | Twice-built | a displayed history-only datum (`let d : Direction = e.dir` over an extrude); same module built twice with a demanded tolerance | Second build equals the first; no `FeatureDatumAmbiguous` (ξ's check). The op-consumer variant needs the consumer declared before the receiver, so it runs after #10137 or in release |
| B13† | Exactly one diagnostic | an ambiguous datum read by two consumers and displayed | One `FeatureDatumAmbiguous` per walk, with the in-loop slot and the post-loop driver both live |
| B14† | Tessellate cold | B1 and B7 through `tessellate_realizations` | Meshes produced; no "got undef" |
| B15† | Tessellate selective-warm | demand set, two renders, then edit a consumer-only param (pattern count; a curated-fillet radius) | Consumer re-tessellates; the unchanged producer is in the must-run set |
| B16† | Relate nested build | a bolt-plate-style fixture whose relate reads datum cells | Unchanged; the nested build uses its own memo |
| B17† | No world is loud | an *inline* feature datum inside `sub a = Inner(size: 80mm)` consumed by an op | An error naming the instance (today's "per-instance re-realization compile error for Outer.a.body"), carrying a diagnostic code α adds |
| B18† | Kernel-less surface | the B2 source evaluated with no realization walk | `a` is Undef with `AwaitingRealization` |
| B19† | Failed receiver | a datum whose receiver realization failed | One error for the realization; the member is Undef with the failure's cause, not `AwaitingRealization` |
| B20† | Cells the generic rung must not touch | a bound `Param` whose default reads geometry; a compute-owned `@optimized` cell | The binding is kept; the compute cell is not body-evaluated |
| B21† | Regression floor | `examples/best_practices`, FEA/modal, `multi_kernel/voxel_to_mesh` under `reify eval`/`check`/`build` | Byte-identical, diagnostic counts included, except the declared rows B1–B9 |
| B22 (β) | Overridden instance, derived cell | §10.1's override repro | `Outer.moved` x-span [-0.5, 160.5] mm; B17 flips to "correct"; the let-bound datum form of §7 is correct |
| B23 (β) | The instance's cell holds the instance's value | the same repro | `Outer.a.c` displays the overridden instance's centroid (today the instance prints `c: undef`) and equals the value the pattern was built about (instantiation-value-flow C2) |
| B24 (γ) | Geometry queries anywhere | `extrude(.., volume(g) / 25mm^2)`; `fn vol_of(g : Solid) -> Volume { volume(g) }`; `constraint volume(g) > 1mm^3` | Build; `mwaste_fn_solid_no_fold.ri` prints values for `ex` and `v_fn`; the constraint verdict is unchanged; the cross-sub `total_mass` aggregate is unchanged |
| B25 (δ) | Topology helpers anywhere | `fillet(b, flat_map(adjacent_faces(b, top), \|f\| shared_edges(top, f)), r)` inline; `fillet(b, [single(shared_edges(top, side))], r)` inline; `g @ face("top")` feeding an op | Build; `try_eval_topology_flat_map` is absent |
| B26 (ζ) | Predicates and mass anywhere | `extrude(.., if is_watertight(g) then 10mm else 20mm)`; a `center_of_mass`-derived let feeding `translate` | Build; `prj/printer_v01` clearance margins unchanged |
| B27 (θ) | Remaining positions | a constraint over a datum; a parent op reading `self.bolt.shank_axis`; a compute argument; a member inside `where flag { .. }` | Each evaluates at its slot |

---

## 7. Out of scope and declared interims

- **A member over an inline geometry receiver** (`revolve(..).axis`). The guarantee is stated for cell receivers.
- **Overridden instances (R15, R18).** A *let-bound* datum or geometry query inside a param-overridden instance builds from the default instance's value with no diagnostic, because the override paths copy the default instance's resolved cells. #10136's centroid case behaves this way on main today. Leo accepted the interim. Instantiation-value-flow β #6592 turns it loud (the override paths then read the instance's own cells, where such a cell is undef); β here makes it correct (B22). Both are behind the #9739 park, so the interim lasts until that lifts.
- **Guarded groups and match-declaration arms.** A member inside `where flag { .. }` is Undef today and a guarded geometry let is not realized unless referenced (#8420; #10037 and #8450 are adjacent; #8450 owns guard-gated realization, including the hoist guard carry absorbed from #10061). α does not change them. θ owns the member position (B27).
- **Oracle writes, cache nodes and compute dispatch on tessellate.** Owned by S2: #9160 (outcome sink), #9165 (freshness, compute dispatch, and the decision whether R3's build-only oracle clause widens). **Breadcrumb:** D11's values-only cell and D12's widened closure compensate for the build-only oracle; when #9165 rules, revisit both.
- **The topology-attribute facet on a cache hit.** ξ restores the swept kind only (R17). It adds a twice-built test of a feature-provenance selector over a cache-served handle and records the result on κ #9263, which owns restoring both facets; a wrong result is filed as its own defect.
- **The pure-eval (stage-b) executor**, #4958. With the build side expressed as "evaluate each node at its slot with a capability set", that design reduces to the same executor with the symbolic capability only. It inherits one fact: the realized world is single-threaded per op compile (`RefCell`), which matches today's `&mut` kernel.
- **Multi-kernel resolution** before #6366.
- **The legacy scheduler** (R11). The let-bound and curated-selector forms already fail under `REIFY_BUILD_SCHEDULER=legacy` on main; α adds no code to that arm and no `Option<&UnifiedPassResult>` parameter. #4727 deletes it.
- **GUI-level regression pins** for #7256 and #8737. α delivers the engine behaviour and tests it on the tessellate entries; those tasks keep their GUI acceptance and gain an edge on α.

---

## 8. Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Disposition |
|---|---|---|---|---|
| direction-typing: ε #9638, μ #9651 | α consumes ε; μ consumes α | `decode_axis` / `decode_plane` read `Value::Direction`; B17 | ε owns the decoder; α owns resolution | edges α → #9638, #9651 → α |
| S2 κ #9263 | κ consumes ξ | the swept kind carried by the cache entry | ξ adds the field under a name that is not `CachedRealization` and leaves `SweptKindTable`'s key alone, so κ's delivered checks stay red until κ; κ folds the field in | edge #9263 → ξ; #9263's record amended (its B-κ1 test is green on arrival) |
| S2 η #9159, δ #9158, α #4727 | same code | the drivers α edits; `tessellate_from_values`; `OpEnv` | S2 | edges #9159 → α, #9158 → α. η carries α's per-Realize values stamp, world construction, driver-owned memo and `derived_member_cells` into `realize_templates`; S2 §5.2 and §5.3 gain dated notes with this PRD's commit |
| S2 ι₃ #9163 | same code | moves of the ladder and the passes | whichever lands second adapts | π depends on #9163, whose text gains the ratchet-path widening of §5.8. (S2 ι₁ #9161 and γ #9156 move and wrap the override functions; that seam is instantiation-value-flow's, not this PRD's) |
| instantiation-value-flow: β #6592 (with α #6586; #6598, #6610, #6926) | β consumes | per-instance realization against the instance's own cells (C1, C2, Shape B) | instantiation-value-flow owns per-instance realization; this PRD's β owns only the realized world and the scheduled slots inside it (§5.7) | edge β → #6592 (R18) |
| S2 λ #9264 | θ consumes | `BuildStep::DispatchCompute` | S2 | edge θ → #9264 |
| S2 #9165, θ #9160 | adjacent | tessellate freshness and oracle | S2 | no edge; breadcrumb in §7 and on #9165 |
| S4 cell-eval-transaction-closure: τ #8872, #8864, #8866, σ #8871 | same code | `op_arg_eval_ctx` and the closed constructor set; `CellEvalCtx`; re-homed passes | S4 owns the constructor module; this PRD owns which constructor carries a world | edge #8872 → α with its text amended (op arguments get a world, realization hashes never do); π depends on #8871. #8864: no edge; its instance-scope mint identity is the key β's per-instance map uses (§5.7) |
| S13 selector-resolution-factoring: α #8747, γ #8749, δ #8750 | same code | `selector_eval/` and `compile/` moves; #8750's text places prehydrate in `compile/` | this PRD moves the value-keyed bodies and deletes prehydrate | edges #8747 → α, #8749 → α, #8750 → α. #8750's text drops prehydrate; #8749's text drops `resolve_selector_to_list` and the feature-datum bodies, which α moves to `realized_world.rs`, and gains the ratchet-path widening of §5.8. δ (here) depends on #8749 and #8750 |
| S3 dependency-invalidation #8687 | η, γ consume | `expr_classify.rs`, the recognizer homes | S3 | edges γ → #8687, η → #8687; no leaf here imports `geometry_ops::is_*` |
| S8 builtin-registry #8906 | η consumes | `ProjectionBinding`; the registry binding column | S8 owns the table; η generalises the column to calls | edge η → #8906 |
| #7886 | δ consumes | value-keyed `SymbolicSelectorCtorFn`; hoist retirement | #7886 | edge δ → #7886 |
| #5973, #5197 | adjacent | R1a detector; undef-cause model | theirs | `AwaitingRealization` is additive; η re-points the detector's recognizer with #5973's owner. No edge from α |
| #6366, #6668 | adjacent | handle carries its kernel; attribute-carrying datums | theirs | the resolver returns whatever datum shape is current; no edge |
| #9547, #8339, #5426, #8249, #7184, #9498 | adjacent | per-instance realization and reuse | instantiation-value-flow and their own owners | no edge; β builds on whatever of them has landed |
| #9457, #8513, #9033 | consume α / γ / δ | kernel-resolved values in ctor fields and fn args; cross-sub Solid in a user fn; relational selector lists | this PRD | #9457 gains an edge on α and is closed or narrowed against α's B4/B5 result; #8513 gains an edge on γ; #9033 gains an edge on δ |
| #7256, #8737 | consume α | tessellate per-Realize stamp; selective-warm closure | α owns the engine half | both gain an edge on α and keep their GUI acceptance |
| #8420, #10037, #8450 | adjacent | guarded geometry lets and the hoist under guards (#8450 owns guard-gated realization and the hoist guard carry, absorbed from #10061) | theirs | no edge; θ's guarded-member row runs after them or pins what they leave |
| #5441, #5442 | adjacent | arity-2 and list `min_clearance` beside `try_eval_kinematic_query` | theirs | ζ adapts to whichever has landed |
| kernel-geometry-queries.md (arg-shape contract), value-eval-geometry-addressing.md (inline form parked), symbolic-eval-nested-selector-resolution.md (Strategy B) | superseded per kind | — | this PRD | dated supersession notes added with this PRD's commit |

None of the three known contested-ownership pairs is touched.

---

## 9. Decomposition plan

Every leaf declares `task_kind` and its signal. ξ, α and ι run ahead of #9739. β follows instantiation-value-flow β #6592, which is parked (R18). Every other leaf depends on #9739 (R8).

**Phase 1 — vertical slice**

- **ξ #10153 — The swept kind survives a realization-cache hit** (D15). The cache value becomes a named struct holding the terminal handle and its swept kind; `probe_realization_cache` records the kind under the cached handle on a hit; conversion intermediates carry none. The cold/warm side-effect parity test and the probe's documented invariant 3 are extended; the attribute-facet measurement of §7 is added. Modules: reify-eval. **Intermediate** → unlocks α (B12). Its own check: the same module built twice with a demanded tolerance gives `e.dir` over an extrude the same `Direction` on both builds and no `FeatureDatumAmbiguous`. Prereqs: none.
- **α #10154 — Realized-world evaluation, first slice: feature datums and selector coercion resolve wherever they occur.** §5.1–§5.6 and §5.8. One leaf and one merge (R6); in order, each commit green: (1) `OpEnv` bundle, byte-identical; (2) `RealizedQueryKind`, the capability, `realized_world.rs` with the moved bodies, the datum kind, the Realize and HydrateCell slots with `derived_member_cells`, `UndefCause::AwaitingRealization`; (3) the selector-coercion kind, the five curated slots, deletion of #5208's three symbols and rung (b); (4) the per-Realize values stamp in `tessellate_from_values` and the producer closure; (5) ratchet rows. **Stop rule:** if commit (3) cannot go green, α escalates; it does not land with prehydrate retained beside the capability. Modules: reify-ir, reify-expr, reify-compiler, reify-eval, reify-cli (`UndefCause` match arms). **Leaf.** Signal: rows B1–B21; `reify eval` builds `circular_pattern(g, cyl.axis, 4, 90deg)` and `mirror(b, top.plane)` inline and let-bound. Absorbs #7256's engine half and #8737's value-read half. Prereqs: ε #9638, ξ.
- **β #10136 — Realization-derived cells in an overridden instance** (adopted task; design-first, §5.7). On top of instantiation-value-flow β #6592's instance realization: schedule order, HydrateCell slots and a realized world for the instance, writing the instance's scoped cells. Closes §7's interim. Modules: reify-eval. **Leaf.** Signal: B22, B23; `reify build` of the override repro spans x in [-0.5, 160.5] mm. Prereqs: α, #6592.
- **ι #10155 — Docs-truth for the first slice.** Spec §9.2.4 and §9.2.5 gain the effects wording of D5; a spec paragraph states the guarantee and lists the kinds α delivers; the geometry and topology MCP chunks say a feature datum can be written directly as an op argument, each signature verified against the compiler arms; `examples/best_practices/feature_datum_as_op_arg.ri` with its `INDEX.md` line; a `.claude/skills/reify-design/SKILL.md` index line; `coerce.rs`'s nested-coercion comment and spec §4.9 corrected. **Leaf.** Signal: the example runs in `examples_smoke`; an author searching "pattern around a hole's axis" finds it from the chunk or the index. Prereqs: α.

**Phase 2 — one leaf per member family (behind #9739)**

- **γ #10156 — Geometry queries.** `GeometryQuery` kinds on the capability; the DFM harvest and the post-geometry constraint re-check (`engine_constraints.rs`) evaluate with the realized context; `rewrite_geometry_queries`, ladder rung (a) and `post_process_geometry_queries` are deleted; the `Param`-default rule of §2 is decided. Cross-sub reads keep working because β's instance values exist first. **Leaf.** Signal: B24. Prereqs: α, β, #8687.
- **δ #10157 — Topology helpers, `feature()` and ad-hoc selectors.** Rungs (c), (d) and (e), `post_process_topology_selectors`, `post_process_feature_accessor`, `post_process_ad_hoc_selectors` and `try_eval_topology_flat_map` are deleted. **Leaf.** Signal: B25. Prereqs: α, #8749, #8750, #7886.
- **ζ #10158 — Conformance, kinematic and mass properties.** The hand-called passes and `try_eval_swept_kinematic_query` are deleted. Kinematic and mass kinds read sub bodies, so β's instance values come first. **Leaf.** Signal: B26. Prereqs: α, β.
- **η #10159 — Membership in the registry.** `Realized(RealizedQueryKind)` in the builtin binding column, stamped into the IR; the recognizer lists (`is_geometry_query_call`, `is_geometry_consumer_call`, `FEATURE_DATUM_PROJECTION_MEMBERS`, the kinematic and dynamics name lists) are deleted; INV-EVAL-5 clause 8 and the R1a detector read the node or the Undef cause. **Leaf.** Signal: `reify eval` output over the corpus is byte-identical; adding a kind without a resolver arm fails to compile (a compile-fail test). Prereqs: γ, δ, ζ, #8687, #8906.
- **θ #10160 — Remaining positions.** Constraints evaluate at their slot on every Realize surface (the widened `E_EVAL_UNRESOLVED` set is declared with a fixture); cross-sub scoped copies use the instance's world; compute arguments use the realized context; members inside guarded groups and match-declaration arms get slots. **Leaf.** Signal: B27. Prereqs: β, γ, δ, ζ, #9264.
- **π #10161 — Convergence.** `hydrate_value_cell_in_loop`'s ladder, both SYNC notes, `post_process_derived_lets` and the remaining value-resolving passes are deleted; the identity fold reads a view with derived-member cells masked (D6's limit goes; B10b flips to "exempt"). Ratchet rows at their end values. **Leaf.** Signal: ratchet rows at their end values; the regression floor and every §6 row unchanged except B10b. Prereqs: γ, δ, ζ, η, θ, #9163, #8871.

**Phase 3 — docs-truth and gate (behind #9739)**

- **ρ #10162 — Docs-truth for the whole class.** The spec guarantee paragraph lists every kind; chunks for queries, selectors, kinematics and mass properties; `material-waste-cost-minimisation.md`'s "stock helpers are trait lets" note retired; the best-practices example extended. **Leaf.** Signal: `examples_smoke` green over the extended example; the chunk signatures compile. Prereqs: γ, δ, ζ, θ.
- **σ #10163 — Integration gate (H).** The §6 matrix as `tests/prd-gate/fixtures/realized_world_*.ri` plus Rust boundary tests facing the capability and every walk, with same-diff drift-guard registrations. **Leaf.** Signal: the matrix runs in the gate; every row's postcondition is a test. Prereqs: every other leaf except ω.
- **ω #10164 — PRD-close.** `SHIPPED` stamp with landed IDs, the AS-AUTHORED freeze paragraph and LIVE map (LIVE: §5, §8), matching header on the capability manifest. **Leaf.** Signal: the committed header. Prereqs: all.

Shared-file serialisation to expect: `geometry_ops.rs` and the drivers are touched by every Phase 1–2 leaf. α's `OpEnv` commit is one mechanical edit across about 78 signatures; it should land as early in α as possible.

---

## 10. Measurement record (dated: main `dd2a97059a`, 2026-11-17, debug `reify`)

### 10.1 Behaviour matrix (`reify eval` unless noted)

Baselines are committed as `tests/prd-gate/fixtures/realized_world_*.ri`.

| Form | Today |
|---|---|
| inline `circular_pattern(g, cyl.axis, 4, 90deg)`; inline `mirror(b, top.plane)` | "expected an Axis/Plane value, got undef" |
| let-bound datum, then the op | the let displays correctly; the op gets undef |
| `axis_z(cyl.axis.origin)` inline; `let o = a.origin` then `axis_z(o)` | undef at the op |
| `fn ax_of(c : Solid) -> Axis { c.axis }` | undef, as a let and as an argument |
| `single(faces_by_normal(..)).plane` | undef, as a let and as an argument |
| `if true then cyl.axis else cyl.axis` | undef |
| `if false then cyl.axis else axis_z(..)` | works; no diagnostic for the unselected branch |
| let-bound `volume`/`centroid`/`area` read inside an op argument | works |
| `extrude(.., volume(g) / 25mm^2)` inline | undef at the op |
| `let v = volume(g)`, `let h = v / 25mm^2`, then `extrude(.., h)` | `h` displays; the op gets undef. The one-step `let h = volume(g) / 25mm^2` builds, through rung (a)'s nested fold |
| `is_watertight`, `center_of_mass`, `body_mass_props` lets feeding an op | the lets display; the op gets undef |
| `g @ face("top")` feeding an op | undef; a warning that selectors resolve only on the build path |
| inline `fillet(b, edges(b), r)` | works (#5208) |
| `let e = single(shared_edges(top, side))`, then `fillet(b, [e], r)` | works, through ladder rung (d). The same `single(..)` written inline in the list fails (element is Undef); that form is δ's |
| `fillet(b, flat_map(adjacent_faces(b, top), ..), r)` inline | undef at the op |
| `top.plane` with another realization reading `top` | unchanged in three variants (no divergence reproduced) |
| `let ar = area(g)` inside `where fancy { .. }` with `fancy` true | undef; a guarded geometry let is absent from the output |
| override repro (`sub a = Inner(size: 80mm)`, centroid-derived axis), `reify build` | silent-wrong: x-span 21.0 mm where the standalone size-80 build gives 161.0 mm |
| the same with an inline feature datum in the instance | loud error, in the default instance as well (the B1 failure); after α only the overridden instance is loud |
| the same with a let-bound feature datum | the override path already receives the default instance's axis; only ε #9638's decoder gap makes it fail today |
| `mwaste_fn_solid_no_fold.ri` | `ex` and `v_fn` undef; the inline forms compute |
| history-only datum (`e.dir` over an extrude), same module built twice with a demanded tolerance (Rust test) | second build Undef with `FeatureDatumAmbiguous`; a revolve's `.axis` survives through an analytic axis |
| let-bound and inline curated fillets under `REIFY_BUILD_SCHEDULER=legacy` | fail |

### 10.2 Reach census (947 tracked `.ri`, HEAD `1f1d7c6519`)

255 member occurrences in 90 files. 232 are a let or param right-hand side or a direct argument; 18 are in constraints, relates or joints; 10 are under a binder; none is in a geometry-op argument or a conditional branch. The 10 binder cases are 8 `flat_map` lambdas (kinematic `min_clearance` and `center_of_mass` over snapshots, four of them in `prj/printer_v01/printer.ri`, and one `shared_edges`) and 2 user-fn bodies. The lambdas work today only through `try_eval_topology_flat_map` and `try_eval_swept_kinematic_query`.

### 10.3 Capability spike (throwaway worktree, not committed)

A `RealizedQueries` trait over a `RefCell<&mut dyn GeometryKernel>` world compiled clean with no `unsafe`. With it, `circular_pattern(g, cyl.axis, ..)` reached `decode_axis`, inline and through a user fn. No borrow conflict was found on the realization path: the kernel borrow at the op-compile site is statement-scoped, and the swept-kind table is written only after a realization's ops finish.

### 10.4 Substrate counts

- `geometry_ops.rs`: 71 functions and 7 fn-pointer typedefs carry the triple (`git grep -cE '_?meta_map\s*:'` = 78); 12 direct `eval_expr` sites bypass `lookup_named_arg_value`.
- `EvalContext`: 2 struct literals and 3 constructors; none outside reify-expr.
- Production callers of `compile_geometry_op`: `execute_realization_ops`, `realize_overridden_instance_into`, `realize_sub_override_handles`.
- Override paths: `seed_cross_sub_named_steps` (four drivers), `seed_cross_let_named_steps` (build only), `realize_overridden_instance_into`, `realize_sub_override_handles` (surfacing walk). An overridden sub is realized twice per tessellate. There is no instance-scoped realization id; scoped value cells exist as `ValueCellId("<parent>.<sub>", member)`. In the override repro the instance prints `c: undef` for its centroid cell while its geometry is built from the default instance's centroid.
- The realization cache is keyed by entity, repr and tolerance, is evicted per entity family, and is probed only for a template's last-declared named realization under a demanded tolerance.
- Tessellate's working values are returned in `TessellateResult.values` and are not written to the snapshot.
- `geometry_ops` is in reify-eval's 24-module SCC (`scripts/assert-module-scc.py print`).
- `UndefCause` has five variants and none for awaiting realization.
- `engine_build.rs` has 14 `post_process_*` functions; `geometry_ops.rs` has 11 `try_eval_*` functions.

---

## 11. Rulings (Leo, 2026-11-17) and open questions

**Before this session (esc-9638-2).**
- **R2.** ε #9638 is narrowed to its decoder half. **R3.** The kernel-minted op forms of B17 move to a leaf after ε and before μ #9651, covering inline and let-bound. **R4.** No second inline pre-resolve family. **R5.** The guarantee of §1 is adopted; it inverts `kernel-geometry-queries.md`'s arg-shape contract and un-parks the inline form in `value-eval-geometry-addressing.md`. The uniform treatment gets this PRD.

**This session.**
- **R1 — Layering.** A kernel-backed capability on reify-expr's `EvalContext` (D1).
- **R6 — α is one leaf**, and it absorbs #5208's inline pre-resolve.
- **R7 — GUI selective-warm:** widen the must-run closure; handles are not persisted across passes (D12).
- **R8 — Park:** only the first slice runs ahead of #9739.
- **R9 — Lazy evaluation is language semantics** and goes into the spec (D5).
- **R10 — `UndefCause::AwaitingRealization`** (D10).
- **R11 — The legacy scheduler is out of scope.**
- **R12 — Values-only stamp on tessellate, for the short term;** α absorbs #7256's engine half. Leo wants build and tessellate to be thin wrappers over one walk; S2 owns that (D11, §7).
- **R13 — Cache-hit swept kind.** The gap is real (§10.1). A small unparked prerequisite leaf (ξ) closes it and α depends on ξ; nothing is released from the #9739 park. Leo first ruled to release κ #9263 and its prerequisites, then withdrew that when κ's dependency closure was measured at 95 pending tasks, 88 of them parked.
- **R14 — Override executor.** α gives the override paths no world; a separate leaf β adopts #10136. This supersedes an earlier same-day ruling to fix them inside α, and is itself narrowed by R18.
- **R15 — The override interim is accepted** (§7): a let-bound member in an overridden instance stays silently default-valued between α and β. Leo chose this over masking those cells to Undef in α.
- **R16 — Module home:** α moves the value-keyed resolver bodies into `realized_world.rs` (§5.3); no S13 leaf is released.
- **R17 — Hit facets:** ξ restores the swept kind only and measures the topology-attribute facet (§7).
- **R18 — β follows instantiation-value-flow β #6592.** Found while wiring edges: that PRD already owns per-instance realization, and #6592 generalises the same two override functions. β does not replace them or run ahead; it narrows to §5.7 and waits on #6592. Leo chose this over releasing #6592 and #6586 from the park, and over keeping β first.

**Lead decisions (recorded).**
- **L1.** D6, D7 and D8 adopt the two critic passes' fixes; no alternative was found for any of them.
- **L2.** γ and ζ do not value-key cross-sub reads until β's instance values exist.
- **L3.** The design critic's selector-receiver divergence did not reproduce (§10.1); D9 is kept because #5208's deletion needs it.
- **L4.** `post_process_feature_datum_projections` survives until π as a second *driver* of the one resolver, so a cell the schedule does not cover is still resolved.
- **L5.** α treats `Let` cells only (§2); `Param` defaults that read geometry are γ's.
- **L6.** Restricted-field containment stays its own capability (D4).

**Open questions (tactical).**
1. **D7's test.** Whether a helper for "contains no Undef and no unrealized handle" exists on `Value` or is added. Decide in α.
2. **`eval_arg_value`'s bare `EvalContext::new`.** Whether it moves onto `OpEnv` in α's first commit or stays (it has no functions or meta today). Decide in α.
3. **Uncovered cells.** Whether any cell lacks a `HydrateCell` step once #4727 has landed. π measures it before deleting L4's driver.
4. **Whether #9457 closes against α.** α runs #9457's repro as part of B4/B5; the result decides close or narrow.
5. **β's instance cache.** §5.7 keeps instance runs out of the realization cache. Whether an instance-keyed cache is worth adding is a follow-up measured after β, with #6592's dedup in view.
