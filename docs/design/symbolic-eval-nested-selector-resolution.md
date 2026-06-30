# Design — Symbolic-eval nested-selector resolution: eval-side mint (A) vs compiler hoist (B) vs alternatives

**Status:** **RATIFIED 2026-06-28 — Strategy B** (human ratification); task #4370 re-scoped to implement B. ⚠️ Implementing Strategy A surfaced a second, orthogonal **TIMING/ORDERING** axis that the reach-only A-vs-B framing below does **not** address — see the **§9 post-ratification addendum**, which supersedes §8's "no eval-time changes required" claim. (Original status: design review / upstream ratification gate for task #4370.)
**Provenance:** task #4882, sourced from `/unblock 4370`, 2026-06-28.
**Code anchors as of HEAD `937eb80de2`** — re-locate every symbol at implementation time, cite-by-symbol; line numbers are hints only.
**Owners / consumers:** task #4370 (nested selector resolution, blocked — pending ratification), FEA String→typed-selector migration, downstream users of `PressureLoad`/`BodyForce`/`Torque` structural types whose field args are topology-selector constructors.

---

## 1. Problem — nested selector constructors yield `Value::Undef` on the symbolic eval path

The symbolic (kernel-less) eval path is the path taken by `reify check`, the LSP, and GUI incremental evaluation — any code path that calls `Engine::eval()`, `Engine::eval_cached()`, or the edit-path equivalent in `engine_edit.rs` without a geometry kernel. It is intentionally designed to be registry-free and kernel-free so it can run cheaply on every keystroke.

**The gap.** Topology-selector constructors nested inside structure-instance field arguments — e.g.:

```ri
PressureLoad(face: face(b, "x_max"), magnitude: 1e6 Pa)
BodyForce(body: solid_body(b), force: 9.81 m/s²)
Torque(edge: edges_by_length(b, 0..10 mm), magnitude: 50 N·m)
```

must resolve to `Value::Selector` on the symbolic eval path so that:
1. The LSP can type-check and surface selector-typed fields correctly.
2. The GUI can display live topology overlays without triggering a build.
3. Downstream constraint evaluators that run on the symbolic path see a resolved selector, not `Value::Undef`.

**Root cause.** `eval_structure_instance_ctor` (`crates/reify-expr/src/lib.rs:1031`) evaluates each field via `eval_expr` in an `EvalContext` that has **no kernel and no registry**. The `eval_expr` function dispatches `FunctionCall` nodes through `reify-stdlib`'s builtin chain, which knows about arithmetic, list ops, and geometry constructors for scalars — but does **not** handle topology-selector constructors (`face`, `edges`, `solid_body`, etc.), because those are handled post-eval by a separate geometry-ops pass. So when a field expression is `face(b, "x_max")`, `eval_expr` returns `Value::Undef`.

The existing **`mint_symbolic_topology_selectors_into_values`** pass (`crates/reify-eval/src/geometry_ops.rs:4924`) fixes exactly this for **top-level value cells**: it walks every `template.value_cells` and for each cell whose `default_expr` is a recognised kernel-free leaf selector constructor and whose value is `Undef`, it mints a `Value::Selector` via `try_eval_symbolic_topology_selector` (`geometry_ops.rs:4851`) → `try_build_kernel_free_leaf_selector` (`geometry_ops.rs:4575`). But it walks **only top-level value cells** — it does **not** descend into `Value::StructureInstance` fields.

The result: a top-level `let force_faces = faces(body)` cell gets properly minted into `Value::Selector`. But `PressureLoad(face: face(b, "x_max"))` produces a `Value::StructureInstance` whose `face` field is `Value::Undef`, because the nested ctor call was evaluated inside `eval_structure_instance_ctor` with no knowledge of selector dispatch.

**Named-leaf constructors.** The gap is sharpest for the named-leaf constructors `face(body, "name")`, `edge(body, "name")`, `vertex(body, "name")`, and `solid_body(body)`, which are intentionally **not** in the kernel-free leaf set handled by `try_eval_symbolic_topology_selector` (these require either kernel resolution or a symbolic stand-in). Task #4370 proposes Strategy A to patch this downstream. The core question is: which strategy is the right architectural foundation for #4370 to land?

---

## 2. Current in-tree state and premise correction

**Premise correction — Strategy A is NOT on main.** The task description for #4882 states that Strategy A "was shipped by task #4370" — that `mint_symbolic_topology_selectors_into_values` "was extended to do a parallel (default_expr, value) walk into structure-instance fields." That extension is **NOT on main at HEAD `937eb80de2`**. Verified:

1. `git merge-base --is-ancestor f8c58bc788 HEAD` → false; the latest #4370 ref `f8c58bc788` is **not** an ancestor of HEAD.
2. There is no `Merge task/4370` commit in the git history.
3. The latest #4370 ref `f8c58bc788` is a "wip … partial" commit whose own message states the nested structure-instance case is **"NOT yet fixed … remaining work."**
4. HEAD's `mint_symbolic_topology_selectors_into_values` walks `template.value_cells` default exprs **only** (call-sites `engine_eval.rs:3536`, `engine_eval.rs:4367`, `engine_edit.rs:3504`) — no structure-instance parallel walk.

So Strategy A as described is an **in-flight/proposed approach** on the #4370 branch — the review conducted here is the UPSTREAM gate that should decide HOW nested resolution is done **before** #4370 blesses A by default. Blocking #4882 on #4370 landing would invert the intended dependency (land A → then review → maybe rip out); the premise nuance is recorded here for the human ratifier instead.

**What IS on main (the substrate all strategies build on):**

| Symbol | File | Approx. line | Role |
|--------|------|-------------|------|
| `eval_structure_instance_ctor` | `crates/reify-expr/src/lib.rs` | 1031 | Pure registry/kernel-free struct ctor evaluator; each field via `eval_expr` → `Value::Undef` for nested selector ctors |
| `materialize_template_lets` | `crates/reify-expr/src/lib.rs` | 1076 | Template let-synthesis called from the ctor above; Strategy B must interact with value-cell ordering here |
| `mint_symbolic_topology_selectors_into_values` | `crates/reify-eval/src/geometry_ops.rs` | 4924 | The existing kernel-free mint pass; walks top-level value cells only |
| `try_eval_symbolic_topology_selector` | `crates/reify-eval/src/geometry_ops.rs` | 4851 | Dispatch for kernel-free leaf selector ctors; returns `None` for named-leaf ctors |
| `try_build_kernel_free_leaf_selector` | `crates/reify-eval/src/geometry_ops.rs` | 4575 | Shared kernel-free builder; covers 9+ leaf ctors |
| `try_eval_resolve_selector` / `resolve_selector_to_list` | `crates/reify-eval/src/geometry_ops.rs` | 4974 | Kernel-bearing selector resolution (build path) |
| `post_process_topology_selectors` | `crates/reify-eval/src/engine_build.rs` | 8549 | Build-path analogue; walks top-level value cells with a live kernel |
| Mint call-site: `eval()` | `crates/reify-eval/src/engine_eval.rs` | 3536 | First call-site for the selector-mint pass |
| Mint call-site: `eval_cached()` | `crates/reify-eval/src/engine_eval.rs` | 4367 | Second call-site (LSP/GUI incremental) |
| Mint call-site: `engine_edit` | `crates/reify-eval/src/engine_edit.rs` | 3504 | Third call-site (edit path) |

---

## 3. Strategy A — eval-side post-pass nested mint (the #4370 approach)

**Core idea.** After the existing top-level `mint_symbolic_topology_selectors_into_values` pass has run, add a second post-pass that recursively walks the **already-built `Value::StructureInstance` / `Value::List` values** in the value map in parallel with the corresponding **default_expr** sub-tree, and mints `Value::Selector` into any nested field/element whose value is `Value::Undef` and whose expression is a recognised selector constructor.

**Where it runs.** As a new function (e.g., `mint_symbolic_topology_selectors_into_struct_fields`) called at the **same three eval-path sites** as the existing mint pass — `engine_eval.rs:3536`, `engine_eval.rs:4367`, `engine_edit.rs:3504` — immediately after the existing top-level pass.

**Dispatch.** For each field `(name, value)` in a `StructureInstance`, find the corresponding field expression from the template's `default_expr` (by walking `ordered_args` + `defaults`), then call `try_eval_symbolic_topology_selector` (which routes through `try_build_kernel_free_leaf_selector`). For named-leaf constructors (`face`, `solid_body`, etc.) that `try_eval_symbolic_topology_selector` returns `None` for today, Strategy A's extended branch adds a named-leaf symbolic stand-in: build a `Value::Selector` with a symbolic (unresolved) target that the build path will later realize.

**Build-path analogue.** The build-path already has `post_process_topology_selectors` (`engine_build.rs:8549`) which walks top-level cells with a live kernel. Strategy A's nested walk either needs a parallel extension there, or it relies on the build path's `execute_realization_ops` + `post_process_*` pipeline reaching the struct fields through a different mechanism (e.g., existing deep-value patching logic). This seam needs explicit verification.

### Concern A-1 — Structural-correspondence drift

The parallel `(expr, value)` walk is fragile when the `default_expr` tree shape can diverge from the `Value` shape. Concrete problem cases:

- **`if`/`else` branching.** A field expression `if cond { face(b, "x_max") } else { face(b, "x_min") }` compiles to a `CompiledExprKind::If`; the corresponding value is the evaluated branch result. The parallel walk must take the same branch the evaluator did — but the evaluator consumed the condition value; the walk must replicate that branch decision independently.
- **`let`-bound intermediates.** A nested let that shadows a selector alias: `let f = face(b, "x_max"); PressureLoad(face: f)`. The value cell for `f` is a top-level cell (handled by the existing pass), but the struct field's value may be a reference to it, not a nested ctor call. The walk must handle `ValueRef` indirection correctly.
- **List fields.** `loads: [PressureLoad(face: face(b, "x_max")), BodyForce(body: solid_body(c))]` — a list of structure instances. The parallel walk must zip `CompiledExprKind::List` (or `FunctionCall` for a list constructor) against `Value::List` elements correctly; list lengths can differ if a list-producing call returns a different element count at different eval paths.
- **Nested structures.** A `SimConfig(loads: [PressureLoad(...), BodyForce(...)])` — the walk must recurse through arbitrarily deep nesting, accumulating the (expr, value) path correctly at each level.
- **Conditional / optional fields.** A field not present in the struct if its default is `Undef` introduces a shape mismatch between the compiler's `defaults` slice and the runtime `fields` map.

Each of these cases requires a specific branch in the parallel-walk dispatch. Missing any branch silently leaves fields as `Undef` (the walk returns `None`, which is indistinguishable from "not a selector ctor"). The surface contract is hard to test exhaustively without integration fixtures for every nested form.

### Concern A-2 — Layering

Strategy A patches `Value::StructureInstance` fields **downstream** of the pure `reify-expr` evaluator (`eval_structure_instance_ctor`), which is intentionally registry/kernel-free (see `lib.rs:1021` doc comment "No registry lookup — reify-expr stays registry-free"). The post-pass in `reify-eval/src/engine_eval.rs` reaches into already-built values to retroactively fix what the pure evaluator left as `Undef`. This is the same pattern as the existing top-level mint pass — a deliberate seam — but extending it into struct fields deepens the layering concern:

- The pure evaluator's output (a `Value::StructureInstance` with `Undef` selector fields) is now a *provisional* representation that is expected to be mutated before consumers see it. The invariant "a `StructureInstance` from `eval_expr` has settled field values" no longer holds unless the caller runs the post-pass first.
- Any future consumer of `eval_structure_instance_ctor` that does not go through the engine's eval path (e.g., a standalone test or a future expression interpreter) will observe `Undef` selector fields unless it also runs the post-pass.

---

## 4. Strategy B — compiler ANF/let-hoist normalization

**Core idea.** A compiler-side normalization pass (analogous to A-Normal Form rewriting) that lifts **nested selector constructor calls** out of structure-instance field arguments and into synthetic top-level `value_cells`. The struct field's compiled expression is rewritten from `face(b, "x_max")` to a `ValueRef(synthetic_cell_id)` pointing at the new cell. The existing top-level `mint_symbolic_topology_selectors_into_values` pass then reaches the nested selector uniformly — no parallel (expr, value) walk, no struct-field post-pass.

**Where it runs.** As a new compiler pass in `reify-compiler`, applied after the template is fully compiled and before `CompiledModule` is emitted. It walks every structure-instance field expression in every template's `ordered_args` and `defaults`, detects selector-constructor call shapes, mints a synthetic `ValueCellId` for each, appends the cell to `template.value_cells`, and rewrites the field expression to a `ValueRef`.

**Why the existing mint pass then suffices.** After the hoist, the nested `face(b, "x_max")` ctor lives in a top-level value cell. `mint_symbolic_topology_selectors_into_values` already iterates all value cells and calls `try_eval_symbolic_topology_selector` on each — so no new eval-time code is needed. The existing three call-sites cover it automatically. The build path's `post_process_topology_selectors` (`engine_build.rs:8549`) also iterates value cells, so the nested selector is realized correctly at build time too.

### Concern B-1 — New compiler phase

Introducing an ANF hoisting pass adds a new phase to the compiler pipeline. Concerns:
- **Phase ordering.** The pass must run after the expression is fully compiled (so all selector ctor call shapes are present in `CompiledExprKind::FunctionCall`) but before `ValueCellId` allocation is finalized (so synthetic cells can be appended without re-indexing). The exact insertion point in the compiler pipeline needs verification.
- **`ValueCellId` minting.** Synthetic cells need unique, stable `ValueCellId`s. The current allocator (`entity.rs`) mints IDs during compilation; the new pass needs either access to the same allocator or a reserved namespace for synthetic IDs.
- **Round-trip stability.** If the module is serialized/deserialized (e.g., for caching), the synthetic cells must survive faithfully. The existing serialization round-trips all `value_cells`; synthetic cells are indistinguishable from user-authored ones, so this should be safe — but needs a test.

### Concern B-2 — Blast radius

The normalization pass touches `CompiledModule` structure, which is consumed by every downstream pass: the eval path, the build path, the constraint path, the LSP, and the incremental edit path. An incorrect hoist (e.g., incorrectly hoisting a non-selector call, or creating a cycle in `ValueRef` dependencies) would break all of them simultaneously. The parallel-walk Strategy A is localized to the eval post-pass and affects only the selector-mint window.

### Concern B-3 — Interaction with `materialize_template_lets`

`materialize_template_lets` (`reify-expr/src/lib.rs:1076`) builds a child `ValueMap` from the already-populated `fields` and evaluates let exprs in that child scope. After hoisting, a field that was `face(b, "x_max")` is now `ValueRef(synthetic_cell_id)`. The let evaluator evaluates `ValueRef` against the child map — but the synthetic cell's `ValueCellId` belongs to the template, not the struct type's namespace, so it may not be present in the child scope. This interaction needs careful analysis to ensure lets that reference struct fields still resolve correctly.

### Concern B-4 — Named-leaf constructors with string arguments

`face(b, "x_max")` takes a string literal naming a specific face. At compile time, the face name is a string literal; the selector's resolution (which face on which body has that name) requires either the kernel or a symbolic placeholder. Strategy B hoists the call into a value cell and mints a `Value::Selector(NamedLeaf { ... })` symbolic stand-in — but `try_build_kernel_free_leaf_selector` currently returns `None` for named-leaf ctors (see `geometry_ops.rs:4575` match arms). Strategy B thus requires **either** extending `try_build_kernel_free_leaf_selector` to produce a symbolic named-leaf stand-in, **or** a new `try_eval_symbolic_named_leaf_selector` sibling. This is the same requirement as Strategy A.

---

## 5. Alternatives

### 5a — Unify the kernel-free and kernel-bearing selector eval paths

**Sketch.** Today there are two selector-resolution entry points:
- Kernel-free: `try_eval_symbolic_topology_selector` (`geometry_ops.rs:4851`) → `try_build_kernel_free_leaf_selector` (`geometry_ops.rs:4575`) — returns `Value::Selector` without a live kernel.
- Kernel-bearing: `try_eval_resolve_selector` / `resolve_selector_to_list` (`geometry_ops.rs:4974`) — resolves a `Value::Selector` to actual geometry handles using a live kernel.

A unified entry point would accept an `Option<&mut dyn GeometryKernel>` and dispatch: `None` → symbolic mint, `Some(kernel)` → full resolution. The `eval_structure_instance_ctor` evaluator could call this unified entry point for field expressions that are selector ctors, parameterized by whether a kernel is available.

**Pros:**
- Eliminates the two-path divergence at the source; no post-pass required; the struct ctor evaluator itself handles nested selectors.
- Any future change to selector semantics is made once.

**Cons:**
- `eval_structure_instance_ctor` lives in `reify-expr`, which is intentionally registry/kernel-free (see "No registry lookup" doc comment). Passing an `Option<kernel>` into it breaks that design boundary. If `reify-expr` must not know about geometry kernels, this requires a callback/seam or a re-architecture of the crate boundary.
- The kernel-free case still needs symbolic stand-ins for named-leaf ctors (same as A and B).
- The unified function's `None`-kernel path is essentially Strategy A's dispatch, just moved inside the evaluator rather than as a post-pass.

### 5b — Thin seam / registry in `reify-expr` for geometry-ctor dispatch

**Sketch.** Extend `reify-expr`'s `eval_expr` `FunctionCall` arm with a pluggable selector-ctor dispatch table (a `&dyn SelectorCtorRegistry` or a `HashMap<&str, Box<dyn Fn(args) -> Value>>`). At the engine layer, register the kernel-free leaf ctors before calling `eval_expr`. When `eval_structure_instance_ctor` evaluates a field and hits a selector-ctor `FunctionCall`, the registry dispatch fires and returns a `Value::Selector` inline.

**Pros:**
- The struct ctor evaluator handles nested selectors without post-passes or compiler hoisting.
- The registry is narrow (selector ctors only) and doesn't require a full kernel reference.
- Named-leaf ctors can be registered with a symbolic-stand-in factory.

**Cons:**
- Widens the deliberately registry-free boundary of `reify-expr`. The doc comment at `lib.rs:1021` explicitly calls this out as a design decision. Opening a registry seam changes the crate's contract for all future users.
- The registry must be threaded through `EvalContext` or passed separately to all `eval_expr` call-sites. The `EvalContext` type is used across many crates; adding a selector field requires a cascade of updates.
- Testing becomes context-sensitive (the same `eval_expr` call produces different results depending on what is registered).

### 5c — Make selectors first-class values evaluated eagerly regardless of nesting

**Sketch.** Extend `eval_expr` to recognize selector-ctor call shapes directly in the `FunctionCall` arm (without a registry) by matching on a hard-coded set of known selector function names, and build a `Value::Selector` there — exactly as `try_build_kernel_free_leaf_selector` does, but inline. This eliminates both the post-pass and the registry.

**Pros:**
- No post-pass, no compiler hoisting, no registry; handles nesting by construction since `eval_expr` recurses.
- Simplest possible control flow; the selector is minted at the same point any other value is computed.

**Cons:**
- Couples `reify-expr` to topology-selector semantics (function name → selector kind mapping), which is currently owned by `reify-eval/src/geometry_ops.rs` (the `TopologySelectorHelper` dispatch table). Duplicating or moving this table into `reify-expr` creates a maintenance boundary problem.
- Like 5b, named-leaf ctors still need a symbolic stand-in that requires knowing what a "symbolic handle" means — knowledge that today lives in `reify-eval`, not `reify-expr`.
- Any future change to the selector name set requires updating `reify-expr`, a low-level crate with wide blast radius.

### 5d — Eliminate the kernel-less mint pass in favour of later-stage realization

**Sketch.** Remove the symbolic selector-mint pass entirely. Selectors in struct fields remain `Value::Undef` on the symbolic eval path. Downstream consumers (LSP hover types, GUI overlays, constraint evaluators) are made tolerant of `Undef` selector fields by treating them as "selector not yet resolved" rather than a type error.

**Pros:**
- No complexity: no parallel walk, no compiler hoisting, no registry extension.
- The "selector not yet resolved" state is honest about what the kernel-less path can know.

**Cons:**
- LSP and GUI consumers already rely on `Value::Selector` being present for hover types and topology overlays. Making them `Undef`-tolerant requires widespread changes to diagnostic and rendering logic.
- Constraint evaluators that consume selector-typed fields on the symbolic path (e.g., for parametric optimization) cannot proceed with `Undef`.
- Regresses the gains from the existing `mint_symbolic_topology_selectors_into_values` pass (task #4653 R2b), which deliberately made top-level selectors available on the symbolic path. Extending that to nested selectors is the natural continuation, not a regression.
- Does not compose with the FEA String→typed-selector migration, whose goal is to have typed selectors available end-to-end.

---

## 6. Evaluation matrix

| Criterion | A (eval post-pass nested mint) | B (compiler ANF hoist) | 5a (unified entry point) | 5b (registry in reify-expr) | 5c (inline in eval_expr) | 5d (eliminate mint pass) |
|-----------|-------------------------------|------------------------|--------------------------|-----------------------------|--------------------------|--------------------------| 
| **Correctness / drift risk** | Medium — parallel (expr, value) walk is fragile under if/let/list shape divergence; each branch needs explicit handling; silent `Undef` on missing cases | Low — hoist is a structural rewrite at compile time; no runtime shape mismatch; the existing mint pass is provably correct over flat value cells | Low — if expr shape is canonical, inline dispatch is correct by construction | Low — same as 5c, registry replaces hard-coded dispatch | Low — inline match has no shape divergence | High — `Undef` selector fields throughout; existing consumers break |
| **Layering purity** | Low — post-pass mutates `StructureInstance` fields produced by the pure evaluator; `reify-expr`'s "settled value" contract is weakened | High — compiler normalizes IR before eval; eval sees flat value cells; no post-mutation | Low — passes `Option<kernel>` into `reify-expr`; breaks registry-free boundary | Low — opens registry seam in `reify-expr` | Low — couples `reify-expr` to selector semantics | High — no layering violation; but only because the feature is removed |
| **Blast radius** | Low — localized to the selector-mint post-pass and its 3 call-sites; doesn't touch the compiler or `reify-expr` | Medium — touches `CompiledModule` structure; all downstream consumers see normalized IR; incorrect hoist breaks all paths simultaneously | Medium — touches `EvalContext` and `reify-expr` crate boundary | Medium-high — `EvalContext` cascade + all `eval_expr` call-sites | Medium — `reify-expr` crate change with wide downstream footprint | Medium — regresses top-level selector mint; widespread consumer changes |
| **Migration cost** | Low-Medium — new `mint_symbolic_topology_selectors_into_struct_fields` function + named-leaf symbolic stand-in; no compiler changes; 3 call-site additions; need parallel-walk branches for each container/branch shape | Medium-High — new compiler pass + `ValueCellId` allocation extension + `materialize_template_lets` interaction analysis + round-trip test + named-leaf stand-in; no eval-time changes | High — `reify-expr` crate boundary change; `EvalContext` threading; named-leaf stand-in; all existing `eval_expr` call-sites audited | High — registry design + threading through `EvalContext` + named-leaf stand-in + backward-compatibility | Medium — `reify-expr` change + named-leaf stand-in + topology-selector name table duplication | High (regressive) — widespread downstream consumer changes + regression of task #4653 gains |
| **Runtime / compile perf** | Negligible — one extra post-pass over the (typically small) value map | Negligible — compile-time normalization is a one-time cost per module; eval sees fewer dynamic branches | Negligible | Negligible | Negligible | Negligible |
| **Testability** | Medium — parallel-walk correctness needs fixtures per container/branch shape; silent `Undef` on missing cases makes test gaps invisible | High — compile-time transformation is unit-testable at the IR level; easy to snapshot before/after hoist | Medium | Medium | Medium | N/A |
| **Interaction with build path** | Medium — must verify `post_process_topology_selectors` reaches nested struct fields, or add a parallel nested walk there too | Low — the hoist makes the build path work automatically via existing `post_process_topology_selectors` | Requires analysis | Requires analysis | Requires analysis | N/A |

---

## 7. Recommendation

**Recommended: Strategy B (compiler ANF/let-hoist normalization).**

**Rationale:**

1. **Eliminates the parallel-walk fragility.** Strategy A's core risk is structural-correspondence drift: the (expr, value) walk must handle every container/branch shape correctly, and failures are silent `Undef`s rather than errors. Strategy B eliminates this by making the normalization a compile-time structural rewrite — the IR the eval path sees is flat value cells, which the existing mint pass handles provably correctly.

2. **Better layering.** Strategy B preserves the `reify-expr` registry-free boundary (the pure evaluator never needs to know about selectors). The compiler is the right place for ANF normalization — it already owns the `ValueCellId` allocation and the `value_cells` list. The eval path remains a clean consumer of a normalized IR.

3. **Build-path correctness comes for free.** After hoisting, the build path's `post_process_topology_selectors` (`engine_build.rs:8549`) reaches the synthetic cells automatically. With Strategy A, the build-path parallel nested walk would also need to be extended — or the build path would have an analogous gap for struct fields.

4. **Single concern B-1 (new compiler phase) is tractable.** The compiler pipeline already has several normalization passes; the `ValueCellId` allocator is accessible at the pass insertion point; and the let-synthesis interaction (Concern B-3) has a clear resolution (the hoisted `ValueRef` cell ID belongs to the template namespace, not the struct type namespace, so it resolves against the module value map rather than the struct child scope).

5. **The alternative 5a/5b/5c options all require widening the `reify-expr` crate boundary**, which is a higher-risk, wider-blast-radius change than either A or B. Alternative 5d is a regression.

**Strategy A is a valid fallback** if Concern B-3 (`materialize_template_lets` interaction) turns out to be intractable. In that case, A's parallel walk should be implemented with explicit branch coverage (if/let/list/nested-struct) and a test fixture for each shape, and the `reify-expr` "settled value" contract must be documented as provisional-until-post-pass.

**The decision between B and A-as-fallback should be the subject of the human ratification requested by this task.**

---

## 8. Migration cost, sequencing, and proposed follow-up

### If B is ratified

1. **Compiler normalization pass** — new function (e.g., `hoist_nested_selector_ctors`) in `reify-compiler`, called after template compilation and before `CompiledModule` emission. Walks `ordered_args` + `defaults` of every structure-definition's compiled template; for each field expression that is a selector-ctor `FunctionCall` (any name in `try_eval_symbolic_topology_selector`'s dispatch set, **plus** named-leaf ctors), mints a synthetic `ValueCellId` under the template entity name, appends a new `value_cells` entry, and rewrites the field expression to `ValueRef(synthetic_id)`. ~200–300 LOC.

2. **Named-leaf symbolic stand-in** — extend `try_build_kernel_free_leaf_selector` (or add a sibling) to produce a `Value::Selector` with a symbolic target for named-leaf ctors (`face(body, "name")` → `Value::Selector(NamedLeaf { target: Symbolic(handle_id), name: "x_max" })`). The build path realizes the name against the kernel; the symbolic path carries the stand-in through. ~100 LOC.

3. **`materialize_template_lets` interaction** — audit whether any struct-def let expr references a field that was hoisted. The child scope in `materialize_template_lets` is keyed by `ValueCellId::new(type_name, member)` (the struct member name); hoisted fields are still present in `fields` as `ValueRef(synthetic)` expressions, so let exprs referencing them by member name still resolve correctly. Confirm with a test fixture.

4. **Round-trip test** — add an integration test that compiles a `.ri` file containing nested selector ctors, serializes and deserializes the `CompiledModule`, and checks that the synthetic cells survive round-trip and the eval path produces `Value::Selector` (not `Value::Undef`) for the struct field.

5. **No eval-time changes required** — ⚠️ **SUPERSEDED — see §9.** The original review claimed the hoist is transparent to all eval-path/build-path consumers (the three `mint_symbolic_topology_selectors_into_values` call-sites + `post_process_topology_selectors` unchanged). Implementing Strategy A disproved this: REACH (the hoist) is *necessary but not sufficient*. The mint pass runs **post-eval**, while `@optimized` compute consumers read their inputs during the **earlier cell pass**, so the hoisted cell is still `Undef` at solve-read time. B **does** require an eval-time change — moving the kernel-less selector/handle resolution into the dependency-ordered cell pass (the TIMING axis).

**Estimated effort:** 1–2 engineer-days.

### If A is ratified as fallback

1. **Nested-walk function** — new `mint_symbolic_topology_selectors_into_struct_fields` in `geometry_ops.rs`. Iterates `values` collecting `Value::StructureInstance` entries; for each, walks the struct's `ordered_args` + `defaults` exprs (from the `CompiledModule` template) in parallel with the field values; for selector-ctor expr shapes calls `try_eval_symbolic_topology_selector` (plus the named-leaf stand-in) and writes back `Undef` → `Value::Selector`. Must explicitly handle: `if`/`else` (replicate branch decision), `ValueRef` indirection, `List` containers (zip expr elements with value elements), and nested struct recursion. ~300–400 LOC including branch handlers.

2. **Named-leaf symbolic stand-in** — same as B step 2.

3. **Build-path extension** — either (a) extend `post_process_topology_selectors` with a nested-walk that mirrors the eval-path walk, or (b) verify that the build path reaches nested struct fields through a different mechanism (e.g., `resolve_selector_to_list` already recurses into `StructureInstance` fields). Needs verification.

4. **Test fixtures per shape** — dedicated fixtures for each container/branch shape to catch silent `Undef` regressions.

**Estimated effort:** 2–4 engineer-days (higher than B due to per-shape branch coverage and build-path audit).

### Proposed follow-up task (to be filed AFTER human ratification)

> **Title:** Implement nested-selector resolution on the symbolic eval path (ratified approach)
> **Scope:** Implement the strategy ratified by task #4882's human review. If Strategy B: add `hoist_nested_selector_ctors` compiler pass, named-leaf symbolic stand-in, `materialize_template_lets` interaction test, round-trip test. If Strategy A: add `mint_symbolic_topology_selectors_into_struct_fields` post-pass, named-leaf stand-in, build-path audit, per-shape test fixtures. This task unblocks task #4370, which should be re-scoped to land the ratified approach rather than A-by-default. No code change in task #4882 itself.

**Task #4882 itself ships no code change.** The deliverable is this design note and the human-ratification escalation below.

---

## 9. Post-ratification addendum — the TIMING/ORDERING axis (implementation finding, 2026-06-28)

**Ratification:** a human ratified **Strategy B** on 2026-06-28. Task #4370 is re-scoped to implement B with *both* axes below.

**Why this addendum exists.** Sections 1–8 framed the problem on a single axis — **REACH**: how to make a selector constructor nested inside a structure-instance field resolve to `Value::Selector` instead of `Value::Undef`. Implementing Strategy A (during `/unblock 4370`) surfaced a **second, orthogonal axis — TIMING/ORDERING** — that neither A nor B as framed above addresses. Both axes must be solved together, or the implementation re-blocks identically.

### The TIMING gap

The kernel-less symbolic resolution of both the geometry handle (`b`) and the selectors runs as a **post-eval pass** — the `mint_symbolic_topology_selectors_into_values` call-sites (`engine_eval.rs:3536` / `engine_eval.rs:4367` / `engine_edit.rs:3504`), which fire *after* the value/cell map is built. But an `@optimized` FEA solve (the consumer of `PressureLoad.face`, etc.) reads its load/support inputs **during the earlier let/cell evaluation pass**. So even with REACH solved, the selector cell is still `Value::Undef` at the moment the solve reads it → the load is `Undef` → `max_von_mises == 0`, and the optimization is a structural no-op.

### Implication for Strategy B (corrects §8 point 5)

Hoisting nested ctors into synthetic top-level `value_cells` (REACH) makes the existing mint pass *able* to reach them — but that mint pass still runs post-eval, so the hoisted `__selN` cell is `Undef` when the `@optimized` consumer reads it: **identical failure**. Therefore §8 "If B is ratified" point 5 (*"No eval-time changes required"*) is **wrong** and is superseded. Strategy B must **also**:

> Resolve the hoisted selector cells (and the geometry handle) **kernel-less, in dependency order, during the cell/let-evaluation pass — before any `@optimized` compute consumer reads its inputs** — i.e. move the kernel-less selector+handle resolution off the post-eval mint pass into the dependency-ordered cell pass.

This **TIMING/ORDERING requirement is non-optional**: B-without-it re-blocks on exactly the wall the `/unblock 4370` session hit when it tried REACH alone. That session's working Strategy-A fix succeeded *only* because it did **both** — the nested walk (REACH) **and** moving the mints before compute dispatch (TIMING).

### Net: Strategy B = REACH + TIMING

1. **REACH** — compiler ANF/let-hoist (§4, §8): lift nested selector ctors into synthetic top-level `value_cells` + `ValueRef` rewrite.
2. **TIMING** — relocate the kernel-less selector/handle resolution into the dependency-ordered cell pass (off the post-eval pass), so the hoisted cells resolve **before** `@optimized` consumers read them.

The same TIMING requirement applies to the Strategy-A fallback. Task #4370's spec now carries both axes. **User-observable signal:** the FEA fixture with `face(...)` nested in a `PressureLoad` field yields a real load with `max_von_mises > 0` — the selector resolved at solve-read time, not `Undef`.

**TIMING axis ownership (updated 2026-06-30):** the TIMING/ORDERING axis is owned by the shared leaf **R3d (task #4900)** in the value-eval-geometry-addressing PRD (`docs/prds/v0_6/value-eval-geometry-addressing.md §8`). Task #4900 implements the dependency-ordered in-walk mint at all three eval entry points (`Engine::eval`, `eval_cached`, `engine_edit`) for the value-eval consumer class (kernel-free, no FEA fixture). Task **#4370 reuses R3d's relocation** for the `@optimized` FEA AXIS-2 witness (`max_von_mises > 0`) without re-implementing it — #4370's remaining scope is the REACH axis (compiler ANF/let-hoist or equivalent) plus the FEA integration witness that confirms the combined fix.
