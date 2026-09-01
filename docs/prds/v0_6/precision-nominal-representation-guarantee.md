# `#precision` is nominal; `RepresentationWithin` becomes a guarantee

**Milestone:** v0.6 · **Status:** active · **Shape:** B + H (contract + two-way boundary tests) · **Authored:** 2026-08-07

Adjudication of record: escalation `esc-6060-1`. Measured basis: 388 sweep points at
`/home/leo/.claude/fleet/sessions/deb-reify-6060-2953549/data/esc-6060-1-raw-sweeps.csv`,
briefing at `.../briefing-esc-6060-1.md`. Do **not** re-open the direction decided there.

---

## 1. Goal

Two declarations exist today that a `.ri` author could reasonably read as "the mesh is within
X of the true surface". Exactly one of them can carry that meaning:

- **`#precision(d)`** is a **nominal** tessellation control. It is threaded bit-exact into OCCT's
  linear-deflection argument. The *achieved* facet-chord deviation is typically **~2.07 × d** and
  was measured as high as **2.106 × d**. It is not an upper bound, and cannot cheaply be made into
  one. This PRD documents it as nominal everywhere a user can read about it.
- **`RepresentationWithin(subject, B)`** becomes a **real guarantee**: the engine tessellates,
  **measures** the achieved deviation with the existing metric, and if it exceeds `B`, **refines the
  deflection and retries** — up to a declared cap. No constant that soundness depends on.

And a third property, which Leo raised during authoring and which outranks both:

- **Uniform meaning.** A declaration means the same thing on every surface. Where a surface cannot
  honour a declared bound, it **says so** — with the true reason — instead of implying success by
  staying quiet. For an artifact that *leaves the system* (an export), "says so" means a
  user-visible error and a non-zero exit, never a silently out-of-spec file.

### 1.1 The motivating failure, live on `main` today

`reify build -o out.stl` on a module declaring `RepresentationWithin(subject, 1mm)`:

- ignores `#precision` entirely and tessellates at a hardcoded **0.1 metres** —
  `DEFAULT_STL_TESSELLATION_TOLERANCE: f64 = 0.1` at `crates/reify-kernel-occt/src/lib.rs:4073`,
  whose own doc comment claims "0.1 **mm**"; task **6085** owns both halves of that defect (its
  description cites `:4024`, which has since moved),
- reports the `RepresentationWithin` constraint as `INDETERMINATE` with the **misattributed** reason
  `operator undefined for these operand kinds: StructureInstance` (measured, §3.2),
- and **exits 0**.

So Reify will today write an STL violating a declared 1 mm bound by a factor of 100 and report
success. This is the class of failure that delayed a real $10,000 WEDM order for six weeks when the
equivalent incapacity in another CAD system's STEP export shipped out-of-spec micron-precision
gerotors. **Closing this does not require task 6085 and does not require any measurement** — it
requires refusing to claim success. It is task **η** below and it is the highest-value leaf in this
PRD.

---

## 2. Background — the measured facts this PRD rests on

All reproduced 2026-08-07 (main `43c076ea54` era), deterministic (61/61 identical across repeats,
digit-for-digit on a second binary and machine state).

- Achieved facet-chord deviation is a **staircase** in requested `#precision` — not smooth, not a
  simple sawtooth. Treads follow an upper envelope at **~2.075 ×** the request, punctuated by narrow
  periodic **downward teeth at ~0.758 ×**, plus a third branch at **~1.49 ×** for `d/R < 2.5e-4`.
- The teeth are real and recurrent: near 0.65 mm, period ~0.022 mm, width ~8.5 µm (~39 % duty
  cycle); near 0.30 mm the period tightens to ~0.006 mm.
- Transitions are brutal: `0.5955mm → 1.223e-3 m` but `0.5960mm → 4.541e-4 m`. **A 0.08 % change in
  the request produces a 2.69 × change in the result.**
- Exactly scale-invariant in `d/R` (at R = 2000 mm every tread value and transition point is
  precisely 2 × the R = 1000 mm case) — geometry, not floating-point noise or load.
- `achieved/requested` by surface class (spot values): sphere up to **2.106**; torus 0.982 with a
  knife-edge at 1.956; cone 0.851; fillet blend 0.599; cylinder 0.489; plane ~0.00003.

### 2.1 Why measure-and-refine is sound — the metric objection is dead

The refine loop keys on `measure_mesh_deviation`, documented as "a sampled lower bound"
(`crates/reify-eval/src/tolerance_combine.rs:246`) because it samples 4 points per triangle
(centroid + 3 edge midpoints). That objection was tested and does not survive:

- 45 sphere configurations across a decade of `d/R`, plus 3 torus (saddle) configurations:
  global `true_max / reported_4point_max` = **1.0000 – 1.0046**. On the 2.07 × branch the ratio is
  **exactly 1.000000000**. Worst anywhere in a 37-point log sweep: **1.004574**.
- The reason is structural. For vertices on a sphere the identity `|P|² = R² − Q(λ)` with
  `Q = Σ_{i<j} λᵢλⱼ L²ᵢⱼ` is **exact**, so `argmax(deviation) ≡ argmax(Q)`. That quadratic form peaks
  at the **centroid** for near-equilateral triangles and migrates to the **midpoint of the longest
  edge** as a triangle elongates — precisely the four sampled points. A highly elongated,
  edge-dominated triangle is where the sample is *strongest*.
- Universal analytic bound over all triangle shapes: **sup(true / 4-point) = 16/15 = 1.0667**, tight
  at sorted edge ratio 1:2:2, unchanged for mixed-sign (saddle) curvature.

**Carry-forward caveat (load-bearing, §5 C-GLOBAL).** The metric *is* locally lossy: ~99 % of
individual triangles have their true max at an unsampled interior point (median under-measurement
~1.5 %, p99 ~6.5 %). **Only the global max is trustworthy.** The refine loop keys on the global max
and must never key on per-triangle values.

### 2.2 Cost — measured during authoring, not extrapolated

`target/release/reify`, 1 m-radius sphere, one realization, this machine:

| module / surface | wall clock |
|---|---|
| sphere + `RepresentationWithin`, `#precision(0.3mm)` — `reify check` | **10.64 s** |
| *same module* — `reify build --verbose` (realizes B-rep, no tessellate/measure) | **0.37 s** |
| sphere, **no** bound — `reify check` (no kernel path taken at all) | 0.35 s |
| bound @ `#precision` 0.6 mm / 0.3 mm / 0.15 mm — `reify check` | 4.92 s / 10.64 s / **19.96 s** |
| **two** spheres, only **one** carrying a bound, @0.3 mm — `reify check` | **19.21 s** |

Three readings, all load-bearing for the design:

1. **Cost scales as 1/deflection** (halving the request roughly doubles the pass). Refining is the
   operation that makes the metric more expensive, so the attempt cap is a **cost** bound as much as
   a convergence bound (§5 C-CAP).
2. **`capture_repr_tol` is a per-`Engine` flag, not per-subject.** The two-sphere row shows the
   *unbounded* sphere is measured too — ~8.6 s of pure waste. Task **δ** reclaims it.
3. The metric dominates: 10.64 s of measured+tessellated work against 0.37 s for the entire rest of
   the pipeline on the same module. Isolating the tessellate/measure split precisely is task **α**.

---

## 3. Verified substrate (G3)

Every anchor below was re-verified in this session at `HEAD = 9d08d3d3d8`. No leaf in §9 relies on
an unverified capability.

| Capability | Location | Verified |
|---|---|---|
| `measure_mesh_deviation` — loops every triangle, 4 samples each, returns the **global** max; takes **no tolerance argument** (structural anti-circularity) | `crates/reify-kernel-occt/cpp/occt_wrapper.cpp:6491`, decl `occt_wrapper.h:1607`, FFI `src/ffi.rs:1327`, trait `crates/reify-geometry/src/lib.rs:111` | ✅ read |
| `achieved_repr_tol` population — gated on `capture_repr_tol`, skip-on-`None`, skip-on-empty-mesh | `crates/reify-eval/src/geometry_ops.rs:10950-10955` | ✅ read |
| Assertion evaluator — three-valued, `PLANAR_FLOOR = 1e-5` for zero bounds | `crates/reify-eval/src/tolerance_combine.rs:232`, `:267-313` | ✅ read |
| Subject → key resolution (value-based, then `"{struct_name}#realization["` prefix scan, **max** over matches) | `tolerance_combine.rs:316-360` (`resolve_repr_tol_key`) | ✅ read |
| Shape matcher — `struct_name` comes from `arg0.result_type = Type::StructureRef(name)`, i.e. the **subject's** type, statically available pre-tessellation | `tolerance_combine.rs:116-196` | ✅ read |
| Routing gate `module_has_representation_within` → `with_registered_kernel` + `set_capture_repr_tol(true)` + `tessellate_realizations()` | `crates/reify-cli/src/main.rs:2462`, used at `:600-607` | ✅ read |
| Per-realization budget array `tessellation_budgets[t][r]` and the `kernel.tessellate(handle, budget)` call site | `engine_build.rs:6168` (`compute_tessellation_budgets`), consumed at `geometry_ops.rs:10935` | ✅ read |
| Pragma lowering → `module.default_tolerance`; `MAX_PRECISION_TOLERANCE_M = 1.0` | `crates/reify-compiler/src/module_pragmas.rs:321`, `:220` | ✅ read |
| Bit-exact threading (the `0.8` `SAFETY_FACTOR` does **not** fire on the empty-conversion BRep→BRep path) | `tolerance_budget.rs:34`, `dispatcher.rs:548-561` (`per_stage_tolerance_for_plan`, empty-chain pass-through) | ✅ read |
| `Diagnostic::{error,warning,info}` + `.with_code(DiagnosticCode)` | `crates/reify-core/src/diagnostics.rs:3888/3898/3908/3928`, `Severity` at `:98` | ✅ read |
| `cmd_build` already exits non-zero on any `Severity::Error` diagnostic | `crates/reify-cli/src/main.rs:1095-1107`, `:1260` | ✅ read |
| Constraint diagnostics reach the user: `push_constraint_result` folds `ConstraintResult.diagnostics.messages` into `CheckResult.diagnostics` → `report_eval_output` | `engine_constraints.rs:680`, `main.rs:2756-2766` | ✅ read |

### 3.1 Substrate facts that **changed the design** (not in the brief)

**(a) Tessellation diagnostics are discarded at the CLI.** `main.rs:625` calls
`engine.tessellate_realizations(&compiled);` and **throws the `TessellateResult` away**. A diagnostic
emitted during tessellation never reaches the user. The refine report therefore **must** ride the
constraint-result channel (`ConstraintResult.diagnostics.messages`, folded by
`push_constraint_result`), not the tessellate channel. This dictates the C-LOG contract in §5.

**(b) `reify check` has no `--verbose`.** `cmd_check` (`main.rs:475-513`) parses `--strict`,
`--purpose`, `--cfg` and **explicitly rejects any other `--` flag**. The observable signal for
refinement steps must be a **diagnostic**, not a verbosity mode. No new flag is proposed — a flag
would be a second way to ask the same question.

**(c) The circularity trap is confirmed and is a `unwrap_or`, not a fold.**
`compute_tessellation_budgets` (`engine_build.rs:6205`) computes
`req_tol = demanded_tols[t][r].unwrap_or(effective_tessellation_tolerance(module))`. A
`RepresentationWithin` bound on the **geometry-owning** template therefore **replaces** `#precision`
as the deflection outright — never min-folded. Consequence: in that layout the achieved deviation is
~2.07 × `B`, so the assertion is *tautologically violated*. §5 C-CIRC states how the refine loop
avoids deepening this and how it incidentally rescues the layout.

**(d) MCP doc chunks have zero coverage.** `grep` over `crates/reify-mcp/src/tools/chunks/*.md`
returns **no** hit for `precision` or `RepresentationWithin`. The docs-truth doc-chunk arm (task **ι**)
is an **add**, not an update.

**(e) f32 mesh quantization is a real numeric floor.** `tessellate_shape` emits vertices as
`static_cast<float>` (`occt_wrapper.cpp:6341-6343`). At coordinate magnitude `S`, f32 ULP ≈
`2⁻²³·S ≈ 1.19e-7·S`. Sample points inherit that quantization, so the *measured* deviation has a
noise floor of order `1e-7·S` metres. (The existing `PLANAR_FLOOR = 1e-5` doc independently cites
"~1e-6 m f32 quantization" at unit scale — same order.) This is the G6 floor: **a bound at or below
`~1e-7·S` is unachievable by construction.** See §5 C-FLOOR.

**(f) No existing `.ri` combines a bound with an export.** All five files carrying

`RepresentationWithin` — `examples/fea_bracket_member_access.ri`, `examples/representation_within.ri`,
`examples/tolerancing/gdt_pass_weave.ri`, `crates/reify-cli/tests/fixtures/representation_within_satisfied.ri`,
`crates/reify-cli/tests/fixtures/dfm_with_repr_within.ri` — were checked for an `Output` occurrence
or `STLOutput`/`StepOutput`: **zero matches.** Task **η** (export refusal) therefore breaks no
existing fixture or example; it fires only on the case that is currently silently wrong.

### 3.2 Measured current behaviour (verbatim, this session, `target/release/reify`)

Probe module — the canonical non-circular layout, `#precision(0.3mm)`, 1 m sphere,
`RepresentationWithin(subject, 1mm)` on a separate `SphereCheck` structure whose `subject` param
carries a default (so the subject is **not** an undefined input, ruling out that confound):

| Command | Observed |
|---|---|
| `reify check` | `OK SphereCheck#constraint[0]` · `All constraints satisfied.` · exit 0 |
| `reify build --verbose` | `INDETERMINATE SphereCheck#constraint[0]` · `warning: constraint SphereCheck#constraint[0] indeterminate: operator undefined for these operand kinds: StructureInstance` · exit 0 |

The `build` reason is **wrong**. The operands are fine; the real reason is that `build` never calls
`set_capture_repr_tol`, so `achieved_repr_tol` stays empty, the
`dispatch_constraints` fast-path guard (`engine_constraints.rs:333`) fires, and the assertion falls
through to the language-level constraint checker — which has never heard of `RepresentationWithin`.
This is the INV-SF-4 (`indeterminate-attributable-transient`) violation that task **ζ** closes, and
it is the *silent* half of the §1.1 export failure.

Also observed on the same probe: with **no** `RepresentationWithin` in the module, `reify check`
takes 0.35 s and no kernel path is entered at all — the structural basis for F's scoping.

---

## 4. Resolved design decisions

### 4.1 The direction: E + F + D (decided by Leo before authoring; recorded, not re-opened)

- **E** — `#precision` is documented as a **nominal** tessellation control.
- **F** — corrective machinery is scoped to modules that **declare a bound**. No global regression.
- **D** — the mechanism is **measure-and-refine**, never prediction.

**Rejected alternatives, with the reasons that must not be relitigated:**

- **A — blanket pre-scale on the deflection.** Rejected on two independent grounds. (i) Triangle
  count scales as 1/deflection, so the ~0.47 factor needed is a **~2.1 × triangle tax
  project-wide** — on planes, cylinders, splines and fillets that already clear their bound with 2 ×
  headroom — paid on every tessellation (GUI, FEA, export, checks). (ii) The constant cannot be sized
  safely: `sup(achieved/requested)` is regime-dependent and **not pinned**. Two careful sweeps
  disagree — 2.0835 over `d/R ∈ [4e-4, 4e-3]`, but **2.106 at d = 0.048 mm**, a finer regime outside
  that range, and *the finer regime gave the larger value*. A constant sized from any finite sweep
  fails **silently** when slightly too small — the worst possible failure mode for a safety property.
  Empirically demonstrated: a 0.48 pre-scale was tested and **5 of 31 targets still exceeded the
  bound** (at requested 0.048, 0.0576, 0.096, 0.1824, 0.288 mm), with several more at 0.999 ×.
- **B — per-surface-class factor.** OCCT takes **one** deflection per `BRepMesh_IncrementalMesh` call
  (confirmed: `occt_wrapper.cpp:6312`) and real parts have mixed faces, so you must take the min over
  classes present — any part containing a sphere degenerates to A's rate. Per-face control requires
  the advanced `IMeshTools_Context` route: much more complexity, same outcome.
- **C — closed-form correction from the two-ladder model.** Exact for spheres (predicts the branch at
  388/388 measured points) but sphere-only, and it depends on OCCT **internal** constants (`0.7` in
  `BRepMesh_SphereRangeSplitter.cxx:25-27`, `0.5` in `BRepMesh_CurveTessellator.cxx:74`) that are not
  API. An OCCT upgrade silently invalidates it — brittle in the same silent way as A.

### 4.2 Refine policy: **pure geometric bisection with an attempt cap** (resolves brief Q3)

`d_{n+1} = d_n / 2`, capped at `REFINE_ATTEMPT_CAP` attempts. The corrective step
`d' = d·(B/achieved)` is **rejected**, on three grounds:

1. **It is not monotone.** Scaling `d` changes the ladder phase and therefore the branch. From a
   tooth (`k ≈ 0.758`) a corrective step lands on a tread (`k ≈ 2.07`), giving
   `a' ≈ 2.07·B/0.758 ≈ 2.73·B` — *worse than before the step*.
2. **It has no termination proof.** `d' = B/k` followed by `a' = k'·B/k` is a fixed-point iteration
   over a non-monotone map; it can cycle without `d` shrinking.
3. **It is unbounded in cost — the decisive objection.** `examples/representation_within.ri` is a
   1 m sphere at `#precision(50mm)` with a **1 µm** bound. One corrective step there requests
   `d' ≈ 4.8e-7 m`, a ~10⁵ × finer mesh, in a single jump. At the measured 1/deflection cost scaling
   that is hours of `BRepExtrema` projections and a multi-gigabyte mesh. The process would appear to
   hang. Halving cannot do this: after `N` attempts the mesh is at most `2ᴺ ×` denser.

Bisection gives, for **any** finite regime supremum `K` with `achieved ≤ K·d`:
`a_n ≤ K·d₀·2⁻ⁿ → 0`. Termination follows without pinning `K` — which is exactly the property
alternative A could not have.

**On "no magic constant anywhere".** The `2` and the cap are **not soundness constants**. The verdict
is always the *measured* one, so the loop is correct for any refinement factor > 1 and any cap; those
constants govern only speed, cost, and how often the cap is reported. That is categorically different
from A's pre-scale, whose *correctness* depended on being sized right and which failed silently when
it was not.

**Provisional `REFINE_ATTEMPT_CAP = 4`** (≤ 16 × density increase, ≤ ~16 × the measured per-pass
cost). Task **α** confirms or retunes it against measured cost. The cap is deliberately modest: the
loop is a **safety net, not a search engine**. Its job is to guarantee you never silently ship worse
than you declared — not to find a precision you should have chosen. When the cap is hit, the
diagnostic tells the user to tighten `#precision`, which is the cheap lever because starting finer
means fewer refinements *and* the user has explicitly opted into the cost.

### 4.3 Safety margin: **compare against `B` directly** — no 16/15 shrink (resolves brief Q4)

Refine until `measured_max ≤ eff_bound`, where `eff_bound = B` (or `PLANAR_FLOOR` for a zero bound,
unchanged from today).

- The measured true/4-point gap is **≤ 1.0046** across 48 configurations, and **exactly 1.000000000**
  on the dominant 2.07 × branch.
- The reported number and the compared threshold stay **the same number**. The diagnostic already
  says "sampled facet deviation"; shrinking the target silently would make the printed value and the
  comparison disagree — a transparency regression, against §1's third property.
- `16/15` is proved for the **sphere-vertex quadratic identity** family (and shown unchanged for
  saddles). It is **not** proved for arbitrary B-rep faces. Applying it repo-wide would project a
  proof outside its derivation and manufacture a false sense of rigour — the same epistemic error
  that sank alternatives A and C.

The guarantee is therefore stated honestly as: *the **sampled** maximum facet-chord deviation is
≤ B*, with the measured 0.46 % / provable 6.7 % sampling gap documented at every doc site (task **ι**,
**λ**).

### 4.4 Failure mode: honest degradation, never a false pass (resolves brief Q7)

When the cap is reached without satisfying the bound, the verdict stays **`Violated`** with the best
achieved value, plus a diagnostic naming the cap, the attempt count, the initial and final
deflection, and the remedy. It must **never** silently report `Satisfied`.

This is reinforced structurally: because the metric is measured on the mesh actually retained
(§5 C-KEEP), *any* failure of the loop — a mesher that ignored the request, a stalled branch, a hit
cap — surfaces as an unchanged or insufficient measured value and therefore as `Violated`. **There is
no path through this design that turns a real violation into a pass.**

### 4.5 Scope of the bound: **per realized occurrence** (resolves brief Q8)

The `achieved_repr_tol` key is `"{entity}#realization[{idx}]"` and `tessellation_budgets` is already
indexed `[template_idx][realization_idx]`, so per-occurrence control exists today and the loop sits
naturally at the per-occurrence tessellate call site. Where several realizations of a bounded
structure exist, each is refined independently; `resolve_repr_tol_key`'s existing **max**-over-matches
rule then makes the reported verdict the worst of them — conservative, unchanged.

### 4.6 Surface scope: uniform **meaning** everywhere; refine on `check` now, on export when actionable

Decided with Leo during authoring, against the measured costs in §2.2.

| Surface | Refine? | What a declared bound means there |
|---|---|---|
| `reify check` | **Yes** (task γ1) | Enforced. Refines up to the cap, then reports honestly. |
| `reify build -o <file>` (export) | **Not yet** — task **θ**, hard-dep on **6085** | **Refused** today (task **η**): Error + non-zero exit. Enforced once 6085 lands. |
| `reify build` (no export) | No | Attributable `Indeterminate` naming *this surface* as the reason (task **ζ**). |
| GUI viewport | No | Same attributable `Indeterminate` (task **ζ**). Revisit if a use case contradicts. |

Rationale, in the order it was decided:

- **Export outranks check.** Being able to *check* an approximation matters less than being unable to
  *ship* an uncontrolled one. §1.1 is the failure this PRD exists to close.
- **Refining before 6085 is inert, so we don't.** The exporter discards the mesh and re-tessellates
  at its hardcoded 0.1 m; a refined mesh would be thrown away. Paying that cost buys nothing
  observable. Refusing, by contrast, buys everything.
- **GUI refine is off on cost.** The GUI rebuilds on every parameter edit; a 10–20 s measure+refine
  pass per edit is not a design session. Deferred, not rejected.
- **F's scoping is structural, not new gating code.** `capture_repr_tol` is set only by `cmd_check`
  (`main.rs:607`). GUI and `reify build` never set it, so they remain byte-identical by construction.
  A module with **no** `RepresentationWithin` never enters any of this — the required negative signal.

### 4.7 Measurement is narrowed to bounded subjects (a cost **win**, task δ)

Today every realization in a module carrying *any* bound is measured — the two-sphere row in §2.2
shows ~8.6 s spent measuring a sphere nobody asked about. After **δ**, `measure_mesh_deviation` runs
only for realizations whose entity path matches a declared bound. This is strictly less work and
strictly the same verdicts: the assertion only ever reads keys matching
`"{struct_name}#realization["` for a `struct_name` that *has* a bound.

### 4.8 The circularity trap is not deepened — and is incidentally rescued

Per §3.1(c), a bound on the geometry-owning template replaces `#precision` and is therefore
tautologically violated (`achieved ≈ 2.07·B > B`). This PRD:

- **does not touch** `extract_output_tolerance_bound` or the demanded-tolerance chain — no new
  feedback path, and the loop never writes a measured value back into a budget;
- sources its bounds from an **independent static scan** of the module's `RepresentationWithin`
  constraints (§5 C-BOUND), not from the budget;
- and, as a side effect, **rescues** the circular layout: starting at `d₀ = B`, two halvings reach
  `a ≈ 2.07·B/4 ≈ 0.52·B ≤ B`. The layout stops being a trap.

The canonical **non-circular** layout — the assertion on a separate checker structure — remains the
documented idiom (task **κ**), because it keeps `#precision` free to be chosen for cost.

### 4.9 Determinism and caching (resolves brief Q6)

- The loop is deterministic given identical input: same module, same kernel, same build ⇒ same
  attempt count and same retained mesh.
- **`RealizationCache` is not involved.** It caches `KernelHandle` (the B-rep terminal), keyed
  `(entity, repr, demanded_tol)`; the loop re-calls `kernel.tessellate` on the *same* handle and
  never re-runs geometry ops. No cache key changes, no invalidation needed.
- **OCCT staleness cannot cause a false pass.** `BRepMesh_IncrementalMesh` re-meshes when a finer
  deflection is requested and the loop only ever goes finer. Even if it declined to re-mesh, the
  measurement would return the unchanged value, the loop would exhaust the cap, and the verdict would
  be an honest `Violated` (§4.4).
- Byte-identity regime: this is **iterative**, so per `decisions_byte_identity_iterative_vs_closedform`
  byte-identity is the wrong bar for the refined result. Determinism is the right one. Modules with no
  bound remain byte-identical, and that negative is a required signal (§9 task μ).

---

## 5. Contract (H)

The seam is: *the tessellate call site* (producer of meshes and measurements) ↔ *the constraint
evaluator* (consumer that renders a verdict and a user-visible diagnostic). Both sides are specified
here so neither has to guess.

### C-BOUND — bound resolution, shared with the assertion

```rust
/// Tightest declared RepresentationWithin bound per subject structure name.
/// Built by scanning `module` constraints via `match_representation_within_shape`
/// (tolerance_combine.rs:116) and min-folding duplicates. Empty when the module
/// declares no bound — which is exactly F's scoping predicate.
pub(crate) fn compute_representation_bounds(
    module: &CompiledModule,
) -> BTreeMap<String, f64>;   // struct_name -> tightest bound, SI metres

/// True iff `entity_path` is a realization of `struct_name`.
/// EXTRACTED from resolve_repr_tol_key's type-name fallback and used by BOTH,
/// so the set the loop refines and the set the assertion reads can never drift.
pub(crate) fn realization_belongs_to(entity_path: &str, struct_name: &str) -> bool;
```

**Invariant C-BOUND-1.** The loop and `resolve_repr_tol_key` must agree. The two call the *same*
`realization_belongs_to`; a boundary test asserts the refined set equals the evaluated set (§6 BT-1).

**Invariant C-BOUND-2 (safe direction).** `resolve_repr_tol_key` tries value-based resolution
(`GeometryHandle → realization_ref`) *before* the type-name scan. If a value-based resolution ever
selects a key the static scan did not cover, that occurrence is simply **not refined** — the
assertion then evaluates an unrefined mesh and reports today's verdict. Under-refinement degrades to
current behaviour and **can never produce a false pass**. Over-refinement is impossible (a bound must
be declared for any refinement to occur).

### C-LOOP — the refine loop

Sited in the tessellate callback (`geometry_ops.rs`, the closure at `:10935`), which is the unique
place holding kernel + `placed_id` + fresh mesh + `entity_path` simultaneously.

```text
d ← tessellation_budgets[t][r]
mesh ← kernel.tessellate(placed_id, d)
if !capture_repr_tol || mesh.indices.is_empty():  push(mesh); return       # unchanged path
bound ← bounds.lookup(entity_path)                                         # C-BOUND
if bound is None:                                 push(mesh); return       # δ: no measure at all
eff ← if bound <= 0 { PLANAR_FLOOR } else { bound }
if eff <= f32_floor(mesh):                                                 # C-FLOOR
    achieved ← measure(placed_id, mesh); record(unachievable); push(mesh); return
achieved ← measure(placed_id, mesh)
attempts ← 0
while achieved > eff and attempts < REFINE_ATTEMPT_CAP:
    d ← d / 2                                                              # C-CAP
    cand ← kernel.tessellate(placed_id, d)
    a    ← measure(placed_id, cand)
    attempts ← attempts + 1
    if a < achieved:  mesh ← cand;  achieved ← a                           # C-KEEP
record(entity_path, achieved, attempts, d0, d, satisfied = achieved <= eff)
push(mesh)
```

- **C-KEEP.** The value recorded in `achieved_repr_tol` is **always** the value measured on the mesh
  pushed into `meshes`. Because the staircase is non-monotone, a finer `d` can measure *worse*; the
  loop keeps the best mesh seen while `d` continues to shrink. Verdict and artifact never disagree.
- **C-CAP.** `d` at least halves every attempt, so the mesh is at most `2^CAP ×` denser than the
  nominal request. This is simultaneously the termination bound (§4.2) and the cost bound (§2.2).
- **C-FLOOR.** `f32_floor(mesh) ≈ f32::EPSILON · max|coordinate|` (§3.1(e)). A bound at or below it is
  unachievable by construction; the loop refuses to spend a single refinement on it and reports so.
  This is the G6 `bound > floor` check, enforced at runtime rather than merely asserted in prose.
- **C-GLOBAL.** The loop compares only the **global** max returned by `measure_mesh_deviation`
  (§2.1). Per-triangle values are not trustworthy and must not be introduced.
- **C-ZERO.** Zero-bound semantics (`PLANAR_FLOOR = 1e-5`) are unchanged.

### C-LOG — telemetry crosses the seam via the constraint channel

Because tessellate diagnostics are discarded at the CLI (§3.1(a)):

```rust
pub struct RefineRecord {
    pub attempts: u32,
    pub initial_deflection: f64,   // SI metres
    pub final_deflection: f64,
    pub achieved: f64,
    pub satisfied: bool,
    pub unachievable_floor: Option<f64>,   // Some(floor) when C-FLOOR fired
}

// New Engine field, sibling to `achieved_repr_tol`, reset in the SAME
// reset_per_build_state destructure choke-point (INV-BUILD-1).
repr_refine_log: BTreeMap<String, RefineRecord>,
```

```rust
pub fn eval_representation_within(
    id: &ConstraintNodeId,
    expr: &CompiledExpr,
    values: &ValueMap,
    achieved_repr_tol: &BTreeMap<String, f64>,
    refine_log: &BTreeMap<String, RefineRecord>,   // NEW
) -> Option<(Satisfaction, Vec<Diagnostic>)>;      // Vec, was Option<Diagnostic>
```

Emission rules (each carries a `DiagnosticCode`, per INV-SF-6):

| Condition | Severity | Content |
|---|---|---|
| Satisfied, `attempts == 0` | *(none)* | Output byte-identical to today. |
| Satisfied, `attempts ≥ 1` | **Info** | attempts, initial → final deflection, achieved value. |
| Violated, cap reached | **Error** | today's message **plus** the cap, attempts, best achieved, and "tighten `#precision`". |
| Violated, C-FLOOR fired | **Error** | bound vs. the f32 representable floor at this scale; no attempts were spent. |
| Indeterminate, surface does not measure | **Info** | names *this surface* as the reason (task ζ). |

`Info` is mandatory rather than `Warning`/`Error` for the satisfied-after-refinement case: it is
expected on a healthy path, and INV-SF-2's corollary forbids Error severity there. It still reaches
the user — `report_eval_output` (`main.rs:3508`) prints every diagnostic to stderr.

### C-SURFACE — uniform meaning

1. `eval_representation_within` is consulted for any `RepresentationWithin` shape **before**
   `dispatch_constraints`' fast-path guard (`engine_constraints.rs:333`), so a surface that never
   measured returns an attributable `Indeterminate` instead of falling through to the language
   checker's misattributed `operator undefined for these operand kinds: StructureInstance`.
   The non-assertion hot path must keep its zero-allocation shape (tactical: a precomputed
   per-check boolean).
2. Two production call sites share one helper,
   `unenforced_representation_bound_diagnostic` (`tolerance_combine.rs`), and emit an
   **Error**-severity coded diagnostic when the module declares a bound the export path cannot
   demonstrate it honours: `engine_build.rs`'s `build_outputs` / `build_outputs_with_result`
   (the occurrence-driven Mode-B export path); and `cmd_build`'s Mode-A `-o` path
   (`crates/reify-cli/src/main.rs`), which calls the helper directly ahead of the write rather
   than relying only on the pre-existing `Severity::Error` gate for its exit code. A third
   export surface — the GUI (`gui/src-tauri/src/engine.rs`'s `Engine::export()` → `build()`) —
   is a **known bypass, not a third enforcing site**: `build()` never calls the helper, only
   `build_outputs`/`build_outputs_with_result` do. Task **6190** closes it (fix committed on
   branch `task/6190`, not yet landed on `main`); this line records the as-shipped state, not
   the post-6190 one.

   Mode A and Mode B are also empirically asymmetric in what they report **on success**, independent
   of the refusal wiring above: Mode A (`reify build -o <path>`) prints a `Triangles: N` line that
   Mode B (occurrence-driven, no `-o`) omits — a task δ/5002 **scope** decision, not a cost one;
   §6 has the full rationale, provenance, and the occurrence-path detail.
3. Post-**6085**, (2) narrows: the export path measures and refines like `check`, and refusal is
   reserved for genuinely unachievable bounds.

---

## 6. Boundary-test sketch (H) — faces both sides of the seam

Task **μ** owns these and names this table as its observable signal.

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| **BT-1** | Refined set ≡ evaluated set | module with two bounded structures + one unbounded, several realizations each | the key set the loop wrote to `repr_refine_log` equals the key set `resolve_repr_tol_key` resolves; the unbounded structure appears in **neither** |
| **BT-2** | Previously-violating module now satisfies | 1 m sphere, `#precision` chosen so `achieved ≈ 2.07·d > B`, bound on a separate checker | `reify check` exits 0, stdout has **no** `VIOLATED`, stderr carries the Info line with `attempts ≥ 1` |
| **BT-3** | Unbounded module is untouched | any module with **no** `RepresentationWithin` | mesh bytes, diagnostics and exit code **byte-identical** to pre-PRD `main`; `measure_mesh_deviation` is never called |
| **BT-4** | Cap is honest | `examples/representation_within.ri` (1 m sphere, `#precision(50mm)`, bound 1 µm) | exit non-zero, `VIOLATED` retained, Error diagnostic names the cap + attempts + best achieved; **wall clock stays within a small multiple of today's** (proves C-CAP bounds cost) |
| **BT-5** | Verdict matches artifact | any refined module | value in `achieved_repr_tol` == `measure_mesh_deviation` recomputed on the mesh present in `TessellateResult.meshes` (C-KEEP) |
| **BT-6** | Sub-floor bound refuses instead of grinding | 1 m part, bound `1e-9 m` | zero refinement attempts; Error diagnostic names the f32 floor at that scale (C-FLOOR) |
| **BT-7** | Export refuses rather than lying | module with a bound **and** an `STLOutput` occurrence | `reify build -o out.stl` exits **non-zero**, Error diagnostic names the unenforced bound; **no out-of-spec file is presented as success** |
| **BT-8** | Non-enforcing surface is attributable | same module, `reify build` (no `-o`) | `INDETERMINATE` retained, but the reason names *the surface*, **not** `operator undefined for these operand kinds` |
| **BT-9** | Zero-bound path unchanged | planar box, `RepresentationWithin(subject, 0mm)` | `Satisfied` via `PLANAR_FLOOR`, zero refinements, message unchanged |
| **BT-10** | Determinism | any refined module, two runs, same binary | identical attempt counts, identical achieved values, identical mesh bytes |

**Gate-cost rule for all of these.** OCCT measurement tests cost 5–20 s each (§2.2). They must land
in the **already-registered** binaries — `crates/reify-eval/tests/representation_within_assertion.rs`
and `crates/reify-cli/tests/harness_cli/cli_determinacy_gate.rs` — which already sit in the `occt`
nextest test-group. Any genuinely **new** `crates/*/tests/*.rs` or `tests/infra/test_*.sh` must carry
its drift-guard registration **in the same diff** (`.config/nextest.toml` partition entry,
`tests/infra/run-all-classification.manifest` row) — this is the A3-before-A6 failure of esc-4914-162.
**No test may assert a wall-clock upper bound**; BT-4's cost check is a task-recorded measurement, not
a gate assertion (`tests/infra/test_no_new_wallclock_upper_bounds.sh`).

**The Mode A/Mode B `Triangles: N` reporting asymmetry (§5 C-SURFACE (2)) is a task δ/5002 SCOPE
decision, not a gate-cost one — this rule does not govern it.** `reify build -o <path>` (Mode A)
prints `Wrote <path>` and a `Triangles: N` line; the occurrence-driven `reify build` (Mode B, no
`-o`) prints `Wrote <path>` with **no** `Triangles:` line and writes to each occurrence's own
declared path (design-file-relative, rebased by `--out-dir` when given) — `-o` selects Mode A and
is absent by definition here. Neither mode would need a realization to report the count: Mode A
derives it straight from `data`, the bytes it just wrote (`main.rs:1621-1624`), and Mode B could
derive it identically from each `artifact.bytes` — the count itself is cheap to extract either way:
O(1) for STL (`stl_triangle_count` reads the 4-byte header field at bytes 80..84,
`main.rs:3375-3382`) and O(bytes) for 3MF (`threemf_triangle_count` counts `<triangle ` windows,
`main.rs:3408-3411`, cheap because `write_3mf` pins every ZIP part to `CompressionMethod::Stored`).
Mode B's silence is scope, not cost: task δ/5002 scoped the count to the imperative `-o` path only
(`main.rs:1738-1745`), and each Mode-B `artifact` already carries `format` + `bytes`, so the same
two helpers are reusable directly, keyed on `artifact.format`, if Mode-B observability is ever
wanted. Confirmed during task 6170's D3 γ-verification (`wf_c00d1e3e-262`, 2026-08-10) and reviewer
finding "design-coherence" on `crates/reify-cli/src/main.rs:1021`.

**A separate, genuine asymmetry *is* governed by this rule: the refusal-path report.** A refused
Mode-A `-o` build returns before `engine.build()` (`main.rs:1500-1520`) and so prints the refusal and
nothing else — no constraint results, no "No constraints violated (N indeterminate)." summary — while
Mode B must realize anyway to enumerate `: Output` occurrences and so gets the constraint results
free from `build_outputs_with_result`. Buying Mode A the same summary on refusal would cost a full
extra realization + tessellation (5–20 s, §2.2) on a build that is about to write nothing, which is
exactly what this gate-cost rule forbids paying.

---

## 7. Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| task **6085** — STL export tessellation tolerance | this PRD **consumes** | `ExtractedExportSpec.tess_tol` → `ExportOptions` → `kernel.export()`; the hardcoded `0.1 m` at `kernel lib.rs:4024` | **6085** owns the plumbing; **this PRD** owns refine-on-export (task **θ**, hard `add_dependency` on 6085) | 6085 `pending` |
| `docs/prds/v0_6/determinacy-intrinsics-completion.md` | this PRD **extends** | `RepresentationWithin` assertion eval (its tasks β/γ = 4198/4199, both `done`) | **that PRD** shipped the assertion; **this PRD** owns making it a guarantee | landed |
| `docs/prds/v0_2/per-purpose-tolerance.md` | this PRD **does not touch** | `extract_output_tolerance_bound` → demanded-tolerance chain | **that PRD**; §4.8 records the deliberate no-change | landed |
| tasks **5976** / **5977** — repr-within fixture wall-clock hygiene | adjacent, **no file overlap** | `#precision` values + prose in the CLI repr-within fixtures | **5976/5977** own those files; task **λ** is scoped away from them | both `in-progress` |
| task **6058** — sawtooth prose correction | adjacent | mechanism prose in 5976's two files | **6058**; **λ** must not contradict it — §2's staircase description is the reconciled account | `pending` (dep 6060) |
| escalation **esc-6060-1** | provenance only | the measurement basis for §2 | **Leo** — do **not** resolve, dismiss or triage | open |

No new contested-ownership pair is introduced; the three known pairs in
`docs/architecture-audit/phase-3-breadcrumb-map.md` §3 are untouched.

**Engine-integration seam (G1 sub-check).** The refine loop plugs into
`engine-integration-norm.md` **§3.2 realization-kind dispatch** — it lives inside the existing
per-realization tessellate walk and introduces no new seam.

---

## 8. Out of scope

- **GUI viewport enforcement.** Deferred on measured cost (§4.6); revisit if a use case contradicts.
  The GUI still gets uniform *meaning* via task **ζ**.
- **Per-face deflection control** (`IMeshTools_Context`). Rejected alternative B.
- **Changing `extract_output_tolerance_bound` or the demanded-tolerance chain** (§4.8).
- **Re-tuning `#precision` in existing CLI fixtures.** Tasks 5976 / 5977 / 6058 own that.
- **Fixing the export deflection plumbing itself.** Task 6085 owns it; **θ** consumes it.
- **A `--verbose` flag for `reify check`** (§3.1(b)) — the diagnostic is the interface.
- **Promoting `measure_mesh_deviation` to a user-callable query.** Already shipped as
  `max_deviation` by GD&T task 4479 (`done`).

---

## 9. Decomposition plan

Greek labels; real task ids assigned at decompose time. **Leaf** = names a user-observable signal.
**Intermediate** = names the downstream prerequisite it unlocks.

### Phase 1 — foundation

**α — Calibrate the refine envelope and the per-iteration cost.** *(Intermediate → unlocks γ1)*
Measure `achieved/requested` for the never-measured non-analytic classes (loft, sweep, pipe, spline)
and worst-regime values for cone, torus and fillet blends (brief Q1/Q2); isolate
`measure_mesh_deviation` cost from tessellation cost (brief Q5). Probe recipe: hold `#precision` on
the shape of interest, put `RepresentationWithin(subject, 0.001mm)` on a **separate** checker
structure to force a violation, read the achieved value off the `sampled facet deviation <X> m
exceeds bound` message. Note `reify build -o *.stl` is **useless** for this — it ignores `#precision`
and tessellates at a hardcoded 0.1 m (task 6085). Deliverable: a committed note under `docs/notes/`
plus the chosen `REFINE_ATTEMPT_CAP` and an honest per-iteration cost statement.
*Unlocks:* γ1 (the cap constant and the cost sentence both land in γ1's code and docs).
*Modules:* `docs/notes/`, probe fixtures under `tests/prd-gate/fixtures/`.
*Note:* this task **cannot** falsify the design — D measures rather than predicts, so an unexpected
overshoot regime changes only the iteration count, never correctness (G6, §10).

**β — Bound pre-pass + shared subject→realization predicate.** *(Intermediate → unlocks γ1, δ, θ)*
Implement `compute_representation_bounds` and extract `realization_belongs_to` from
`resolve_repr_tol_key`'s type-name fallback so the loop and the assertion share one predicate
(C-BOUND). No behaviour change on its own.
*Unlocks:* γ1 (needs the bound table), δ (needs the predicate to narrow measurement).
*Modules:* `crates/reify-eval/src/tolerance_combine.rs`, `crates/reify-eval/src/engine_build.rs`.

### Phase 2 — the vertical slice

**γ1 — Measure-and-refine loop.** *(Leaf.* deps: α, β*)*
C-LOOP, C-KEEP, C-CAP, C-FLOOR, C-GLOBAL, C-ZERO. Gated on `capture_repr_tol` **and** a declared
bound, so unbounded modules never enter it.
*Signal:* a `.ri` declaring `RepresentationWithin(subject, B)` that today prints
`VIOLATED … sampled facet deviation X m exceeds bound B m` and exits non-zero instead prints `OK` and
exits 0 from `reify check` — **and** a module with no `RepresentationWithin` produces byte-identical
mesh bytes, diagnostics and exit code (the required negative that proves F's scoping).
*Modules:* `crates/reify-eval/src/{geometry_ops.rs, engine_build.rs, lib.rs, engine_admin.rs}`.

**γ2 — Refine telemetry as user-visible diagnostics.** *(Leaf.* deps: γ1*)*
C-LOG: the `RefineRecord` log, the `eval_representation_within` signature change, the new
`DiagnosticCode` variants, and the emission table.
*Signal:* `reify check` on a refined module prints an Info line on stderr naming the refinement
count and the initial → final deflection; on a cap-hit module it prints an Error naming the cap,
attempts, best achieved and the remedy, and still exits non-zero.
*Modules:* `crates/reify-eval/src/{tolerance_combine.rs, engine_constraints.rs}`,
`crates/reify-core/src/diagnostics.rs`.

**δ — Narrow measurement to bounded subjects.** *(Leaf.* deps: β*)*
Stop measuring realizations that carry no declared bound (§4.7).
*Signal:* `reify check` on a two-sphere module where only one sphere carries a bound drops from the
measured 19.2 s toward the ~10.6 s single-sphere cost, with **identical** constraint verdicts and
diagnostics. Regression test is **structural** — assert `achieved_repr_tol` contains exactly the
bounded subject's key — **not** a wall-clock assertion (§6 gate-cost rule).
*Modules:* `crates/reify-eval/src/geometry_ops.rs`.

### Phase 3 — uniform meaning

**ζ — Attributable `Indeterminate` on surfaces that do not measure.** *(Leaf.* deps: none*)*
C-SURFACE (1). Today `reify build` on a bounded module reports
`operator undefined for these operand kinds: StructureInstance` — a misattributed reason
(INV-SF-4 violation, measured verbatim in §3.2).
*Signal:* `reify build` / the GUI on a module declaring a bound reports `INDETERMINATE` with a reason
naming **this surface** and pointing at `reify check`; `reify check` output is unchanged.
*Modules:* `crates/reify-eval/src/engine_constraints.rs`.

**η — Export refuses rather than shipping an unenforced bound.** *(Leaf.* deps: none*)*
C-SURFACE (2) — **the §1.1 fix, and the highest-value leaf here.** No 6085 dependency, no
measurement, no refine.
*Signal:* `reify build -o out.stl` on a module declaring `RepresentationWithin(subject, 1mm)` exits
**non-zero** with an Error diagnostic naming the unenforced bound, instead of today's silent exit-0
alongside a 0.1 m-deflection STL. Compatibility is measured: **zero** existing `.ri` combines a bound
with an export (§3.1(f)), so nothing regresses.
*Modules:* three call sites (§5 C-SURFACE (2) has the full contract and the Mode-A/Mode-B reporting
asymmetry): `crates/reify-eval/src/engine_build.rs` (`build_outputs`, `build_outputs_with_result` —
Mode B); `crates/reify-cli/src/main.rs`'s `cmd_build` Mode-A `-o` path; and
`gui/src-tauri/src/engine.rs` (still a bypass as of this writing — task 6190 closes it, not yet
landed on `main`).
*Lock note:* `crates/reify-cli/src/main.rs` **is** touched — an earlier version of this note said "no
CLI file is touched," which was stale: Mode A calls `unenforced_representation_bound_diagnostic`
directly ahead of the write rather than relying only on `cmd_build`'s pre-existing
`Severity::Error` gate for its exit code.

**θ — Enforce the bound on export.** *(Leaf.* deps: γ1, β, **task 6085***)*
Once a declared tolerance actually reaches `kernel.export()`, apply C-LOOP on the export path and
narrow **η**'s refusal to genuinely unachievable bounds.
*Signal:* an exported STL from a module declaring `RepresentationWithin(subject, B)` measures within
`B`; a bound the cap cannot reach still exits non-zero rather than writing the file.
*Modules:* deferred to the architect (`[]`) — the footprint depends on 6085's landed shape.
*Ordering:* the hard `add_dependency` on 6085 turns a file-lock collision into a sequence (§7).

### Phase 4 — docs-truth (all four arms; the trigger fires — this is language surface)

**ι — Doc chunks, registry-verified.** *(Leaf.* deps: γ1, γ2, ζ, η*)*
`crates/reify-mcp/src/tools/chunks/constraints.md` gains a `RepresentationWithin` section (today:
**zero** coverage) and `syntax.md` gains `#precision` with its **nominal** semantics stated plainly:
the achieved deviation is typically ~2.07 × the request and was measured to 2.106 ×; `#precision` is
a cost/quality knob, `RepresentationWithin` is the correctness assertion.
*Signal:* every documented signature compiles as written in a smoke `.ri`; **discoverability** — an
author who knows the goal ("make sure the exported mesh is within 0.05 mm of the true surface") but
not the feature name finds `RepresentationWithin` from the chunks in intent terms.
*Modules:* `crates/reify-mcp/src/tools/chunks/{constraints.md, syntax.md}`.

**κ — Exemplar + index.** *(Leaf.* deps: γ1*)*
`examples/best_practices/representation_bound.ri` — the canonical **non-circular checker-structure**
layout, stating the anti-pattern it replaces (bound on the geometry-owning structure, which replaces
`#precision` outright per §3.1(c)). Plus its `INDEX.md` line and a one-line index entry in
`.claude/skills/reify-design/SKILL.md` pointing at the corpus file (not an inline playbook).
*Signal:* the file compiles under the corpus gate (`crates/reify-compiler/tests/examples_smoke.rs`)
and `best_practices_index_matches_corpus_directory` stays green.
*Modules:* `examples/best_practices/representation_bound.ri`, `examples/best_practices/INDEX.md`,
`.claude/skills/reify-design/SKILL.md`.

**λ — Prose sites.** *(Leaf.* deps: γ1, γ2*)*
- `docs/reify-language-spec.md:2476` — §12.2 currently calls `#precision` a "hint to toolchain about
  numeric precision" and documents **only** the `#precision(float64)` form, omitting the `Length`
  form that is the one with semantics. Correct both.
- `docs/reify-stdlib-reference.md:1683` — "Use a finer `#precision` directive to reduce the
  measurement gap" must go; replace with the nominal-vs-guarantee split and the measured sampling gap
  (≤ 1.0046 observed, 16/15 provable).
- `docs/prds/pragmas.md` §2 — a one-line pointer to this PRD for the nominal semantics. **Not** a
  rewrite: it is a shipped record.
- `examples/representation_within.ri` header — add the refinement-attempts and cap behaviour (it is
  BT-4's fixture; its documented `VIOLATED` + non-zero exit is **preserved**).
*Signal:* no doc site claims or implies that `#precision` bounds achieved deviation; the
language-spec `#precision` entry documents the `Length` form.
*Modules:* the four files above. **Scoped away from** `crates/reify-cli/tests/fixtures/*.ri` and
`cli_dfm_overhang.rs` — tasks 5976/5977/6058 own those (§7).

### Phase 5 — integration gate

**μ — Two-way boundary tests.** *(Leaf.* deps: γ1, γ2, δ, ζ, η*)*
*Signal:* the §6 table, BT-1 … BT-10, green — including BT-3 (unbounded module byte-identical) and
BT-7 (export refuses). Tests land in the already-registered OCCT binaries; any new test file carries
its drift-guard registration same-diff; no wall-clock upper-bound assertions.
*Modules:* `crates/reify-eval/tests/representation_within_assertion.rs`,
`crates/reify-cli/tests/harness_cli/cli_determinacy_gate.rs`.

### Dependency graph

```
α ─┐
   ├─> γ1 ─┬─> γ2 ─┐
β ─┘       │       ├─> ι
   └─> δ ──┼───────┤
           ├─> κ   │
           ├─> λ   │
           └─> θ   │        (θ also depends on task 6085)
ζ ─────────────────┤
η ─────────────────┴─> μ
```

---

## 10. Premise validation (G6)

| Leaf | Assertion class | Basis |
|---|---|---|
| γ1 | end-to-end capability ("a violating module now satisfies") | every capability is in γ1's own dependency set or already on `main`: the metric (verified §3), the per-realization budget array (verified §3), the bound table (β, upstream), the cap (α, upstream). Nothing is owned by a task that *depends on* γ1. |
| γ1 | numeric floor | **`bound > f32_floor ≈ 1.19e-7 · S`** (§3.1(e)) — enforced at runtime by C-FLOOR, not merely asserted. `PLANAR_FLOOR = 1e-5` handles zero bounds. |
| γ1 | termination | `a_n ≤ K·d₀·2⁻ⁿ` for **any** finite `K`; no pinned constant required (§4.2). |
| γ2 | rejection / cap-hit | the Error path is *observed*, not assumed: BT-4 runs `examples/representation_within.ri`, whose 1 µm bound on a 1 m sphere is unreachable within the cap, and asserts the Error fires. |
| δ | negative assertion ("unbounded realizations are not measured") | structural test asserts `achieved_repr_tol` key set == bounded subjects' keys; measured baseline 19.2 s → ~10.6 s (§2.2). |
| ζ | negative assertion ("today's reason is wrong") | **observed**: `reify build` on a defaulted-subject bounded module prints `operator undefined for these operand kinds: StructureInstance` (§3.2). The misattribution is measured, not inferred. |
| η | rejection mechanism | **observed absent**: no `.ri` in the repo pairs a bound with an export (§3.1(f)), and `cmd_build`'s `Severity::Error` gate is verified present (`main.rs:1095-1107`) — so the refusal has a real mechanism to ride. |
| θ | end-to-end capability | requires 6085's plumbing, which is an **upstream** hard dependency — never a downstream one. |
| ι, κ, λ | none quantitative | doc/compile-gated signals. |

**The one genuine unknown** — do non-analytic surfaces (loft/sweep/pipe/spline) overshoot, and by how
much? — does **not** threaten the design. D is self-correcting by construction: it measures rather
than predicts, so an unexpected regime changes the **iteration count**, never the correctness of the
verdict. Task α sizes it. G6 is satisfied, not blocked.

## 11. Design-invariant walk (G7 — `docs/legibility/design-invariants.md`)

| Invariant | Where this PRD engages it |
|---|---|
| `undef-has-provenance` (INV-SF-1) | no new `Undef` cell is created; the loop never writes `Undef`. |
| `error-severity-exits-nonzero` (INV-SF-2) | the satisfied-after-refinement diagnostic is **Info**, not Error, because it is expected on a healthy path (corollary). The cap-hit and export-refusal diagnostics are Error **and** exit non-zero — η rides `cmd_build`'s existing gate rather than adding a per-code bolt-on. |
| `declared-intent-consumed-or-diagnosed` (INV-SF-3) | **this is the PRD's third goal.** η and ζ exist precisely so a declared bound is never silently dropped by a surface that cannot honour it. |
| `indeterminate-attributable-transient` (INV-SF-4) | ζ replaces a misattributed reason with the true runtime one and names what clears it (`reify check`). C-FLOOR's permanently-unachievable case is reported as **Violated with a named floor**, not as a permanent Indeterminate. |
| `placeholders-owned-and-loud` (INV-SF-5) | no placeholder type or sentinel default is introduced. |
| `diagnostics-carry-codes` (INV-SF-6) | every new diagnostic carries a `DiagnosticCode` (C-LOG). γ2 additionally gives the existing code-less `RepresentationWithin` violation message a code. |
| `parse-is-value-faithful` (INV-SF-7) | no grammar change — G3 grammar gate is **N/A**; this PRD adds no novel syntax. |

No waiver required.

---

## 12. Open questions (tactical only)

1. **Exact `REFINE_ATTEMPT_CAP`.** Provisional 4. Task α retunes against measured cost. Tactical: any
   value is sound (§4.2); only reach and cost change.
2. **Preserving the non-assertion zero-allocation fast path** when the `RepresentationWithin` peel
   moves above `dispatch_constraints`' guard (C-SURFACE 1). A precomputed per-check boolean is the
   obvious shape; the architect may pick another. Decide during ζ.
3. **`Engine::achieved_repr_tol()` consumers.** Only one doc reference found
   (`crates/reify-eval/src/lib.rs:1062`) and no production caller; δ narrows what the map contains, so
   confirm no reader depends on unbounded-subject keys before landing. Decide during δ.
4. **Wording of the cap-hit remedy.** "tighten `#precision`" is the mechanically correct advice; the
   exact phrasing lands with γ2 and must match what ι/λ document.
5. **Whether `θ` also covers STEP export** or STL/3MF only — depends on how 6085 lands its
   `ExportOptions` shape. Decide when 6085 is done.
