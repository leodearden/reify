# Capability Manifest — `geometric-relations.md` (core: `at auto` + solved `relate`)

**PRD:** `docs/prds/v0_6/geometric-relations.md` (committed `e8ce420d3b`) · **Built:** 2026-06-08 · **Author:** claude-prd-decompose

Mechanizes the decompose-time **G3 + G6** check per the gates spec (`gates.md → Capability Manifest`) and the Reify overlay (`.claude/skills/prd/project.md → Capability Manifest — reify evidence forms`). One block per task; each capability the task's user-observable signal asserts is **bound to evidence**. Any binding resolving to a FAIL value (`declared-only` / `test-only` / `producer-absent` / `producer-downstream` / `fixture-ERROR` / `bound≤floor`) **blocks the whole α–θ batch** until resolved.

Greek labels = PRD §9 decomposition; task IDs assigned at file-time (see the batch summary at the foot). Code anchors are as-of-authoring hints — re-locate at implementation time (main moves fast).

## Verdict summary

| Task | Kind | Bindings | Verdict |
|---|---|---|---|
| α | intermediate (→ζ) | wired-on-main (registry) | **PASS** (α is the producer; seam exists, currently un-installed) |
| β | intermediate (→γ,ε,ζ,η,θ) | grammar-fixture · new-type-producer | **PASS** |
| γ | intermediate (→δ,ζ,η,θ) | grammar-fixture · producer-self | **PASS** |
| δ | intermediate (→ζ,η) | grammar-fixture (producer) | **PASS** (δ **is** the grammar producer; gr-01/02/03 RED by design) |
| ε | intermediate (→ζ,θ) | producer-upstream (4118/4119/4120) | **PASS** |
| ζ | **leaf** (integration-gate) | branch-3 end-to-end trace | **PASS** (every capability upstream) |
| η | **leaf** | grammar-fixture · producer-upstream | **PASS** |
| θ | **leaf** | producer-upstream · diagnostic-extension | **PASS** |

**Field-population check: N/A** — no task's signal samples/reduces a result field (`result.stress`, `mode.shape`, …). Signals are solver placement, typed diagnostics, parse-acceptance, and ledger text — none read a `Value::Field`. The empty-value sentinel (`Value::Undef`) twin does not apply.

**Numeric-floor check (G6 branches 1/2): N/A** — every number the signals assert (`removes 4`, `removes 3 (2 redundant) → residual 1`, `spent 5 · free 1`, `codim(X)`) is an **exact integer codimension** from the one DOF law `coincident(X,X) removes codim(X)` (PRD §4, design §3.4) — a count of removed degrees of freedom, **not** an absolute-accuracy bound on a numerical method. There is no method-error floor to compare against, so G6 branches 1 (numeric bound) and 2 (closed-form exactness) **do not fire**. Only branch 3 (end-to-end capability tracing) fires — applied to the ζ leaf below. (Solver *convergence* tolerance is a kernel-defaulted `Length` knob, PRD §7.1; it is not asserted as a fixed numeric premise by any leaf signal.)

---

## α — Register `SolveSpaceSolver` in the production `SolverRegistry` (CLI + GUI)

**Signal:** a `.ri` fixture with a geometric-classified constraint (`std::parallel` / `std::distance`) that produced no geometric solve now reaches `SolveSpaceSolver` and solves; SolveSpace is present in the registry the CLI/GUI engines install.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `SolveSpaceSolver` type + `impl ConstraintSolver` exist | `grep:crates/reify-constraints/src/solvespace.rs:46` (`pub struct SolveSpaceSolver`), `:874` (`impl ConstraintSolver for SolveSpaceSolver`); `grep:crates/reify-constraints/src/lib.rs:39` (`pub use solvespace::SolveSpaceSolver`) | PASS (exists) |
| A `SolverRegistry` that can hold ≥1 solver exists | `grep:crates/reify-constraints/src/registry.rs:24` (`pub struct SolverRegistry`), `:73` (`impl ConstraintSolver for SolverRegistry`) | PASS (exists) |
| Geometric constraints are classified so the registry can route them | `grep:crates/reify-constraints/src/classifier.rs:46` (`std::distance \| std::angle_between \| std::parallel \| std::tangent` → `ConstraintDomain::Geometric`, :27/:174) | PASS (wired) |
| **The production engine installs `SolveSpaceSolver`** (anti-orphan, wired-on-main) | `grep:crates/reify-cli/src/main.rs:651` installs **only** `DimensionalSolver` (`engine.with_solver(Box::new(reify_constraints::DimensionalSolver))`); GUI seam `grep:gui/src-tauri/src/engine.rs:298` (`with_solver`). **This capability is `producer-absent` on main today — and α is the task whose deliverable is precisely to wire it.** | PASS (α **is** the producer; it is not a downstream consumer claiming a not-yet-built capability) |

**Note (the wired-on-main precedent C-10 / 2962):** the *consuming* capability "production engine routes to `SolveSpaceSolver`" is genuinely absent on main — exactly the orphan shape the gate guards. It binds PASS only because α's own deliverable is the wiring (registry install in `reify-cli/src/main.rs:651` + GUI `engine.rs`), with a `reify check`/`eval` integration test as the wired-on-main proof. A downstream task asserting "SolveSpace solves my constraint" must depend on α (ζ does: α→ζ).

---

## β — First-class `Direction` type + datum-projection member access

**Signal:** `reify check` accepts `let d : Direction = axis.dir`, rejects `point.dir` with `E_DATUM_PROJECTION_UNAVAILABLE`, rejects the ambiguous bare `frame.dir`; a CI `.ri` example exercises projections.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `Direction` in a type signature parses | `grammar-fixture:/tmp/prd-gate-fixtures/gr-07-direction.ri` → `tree-sitter parse --quiet` exit 0 | PASS |
| Member-access projections (`axis.dir`, `self.xy_plane`, `frame.z`) parse | `grammar-fixture:/tmp/prd-gate-fixtures/gr-08-projections.ri` → exit 0 | PASS |
| `Direction` is a resolvable value/ty | `producer:β` — PRD §2 verifies `Direction` is **absent** from `crates/reify-types/src/ty.rs` today; β's deliverable is the new type (the one genuinely new surface type). Grammar already accepts it (gr-07), so this is a type-resolver add, not a grammar add. | PASS (β is the producer; no downstream task reads `Direction` before β: β→{γ,ε,ζ,η,θ}) |

**grammar_confirmed = true** (projections + `Direction` signatures parse on existing grammar).

---

## γ — Relation vocabulary + `Relation` type + the three policing layers

**Signal:** `reify check` types `concentric(a: Axis, b: Axis) -> Relation`; rejects B9/B10 category errors (`angle(p,p,θ)`, `angle(a,b,5mm)`) with typed diagnostics; hover shows `offset(Plane,Plane,Length) -> Relation removes 3`.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `fn … -> Relation = …` (pure mate) parses | `grammar-fixture:/tmp/prd-gate-fixtures/gr-04-fn-relation.ri` → exit 0 | PASS |
| Kind-generic `fn coincident<D: Datum>(…)` parses | `grammar-fixture:/tmp/prd-gate-fixtures/gr-06-generic.ri` → exit 0 | PASS |
| `Relation` type + `relation_signatures.rs` family | `producer:γ` — NEW `crates/reify-compiler/src/relation_signatures.rs`, mirroring the landed `math_signatures.rs` / `joint_signatures.rs` family pattern (`grep:crates/reify-compiler/src/ -l "_signatures.rs"`). Signature-family precedent is on main. | PASS (γ is the producer) |
| `β` (`Direction`) available for projection policing | `producer:β` upstream (β→γ) | PASS (upstream) |
| Kind-generic monomorphization (`codim(D)` specialization) | `producer:4235` (generics completion) upstream — wired as out-of-batch dep; named specializations (`concentric`/`flush`/…) do **not** need 4235 (PRD §5). | PASS (gated; named-mate path unblocked now) |

**grammar_confirmed = true** (`fn -> Relation` + generics parse).

---

## δ — Grammar production: `relate { }` + `at auto` / `auto(…)` + `at … where { }` + Relation-vs-Bool enforcement

**Signal:** fixtures `gr-01/02/03` parse (`tree-sitter parse --quiet` exit 0) with parser tests in `tree-sitter-reify/tests/`; `relate { }` rejects a `Bool` body member (`E_RELATE_EXPECTS_RELATION`); `constraint` rejects a `Relation`.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `at auto` + `relate { }` parses | `grammar-fixture:/tmp/prd-gate-fixtures/gr-01-at-auto-relate.ri` → **exit 1 (ERROR) today** | **producer-self → PASS** |
| `at auto where { }` parses | `grammar-fixture:/tmp/prd-gate-fixtures/gr-02-at-auto-where.ri` → **exit 1 (ERROR) today** | **producer-self → PASS** |
| `auto(seed=…)` / `auto(free)` / `auto(x=…)` parses | `grammar-fixture:/tmp/prd-gate-fixtures/gr-03-auto-param.ri` → **exit 1 (ERROR) today** | **producer-self → PASS** |
| `Relation`-vs-`Bool` enforcement (`relate` accepts only `Relation`) | `producer:δ` consuming the `Relation` type from `producer:γ` upstream (γ→δ) | PASS (upstream) |

**This is the one `fixture-ERROR` row set in the manifest, and it is the correct/expected state:** δ **is** the named grammar-producer task (PRD §3 G3 table; overlay grammar-fixture rule: "parses with 0 ERROR nodes **OR** a named grammar-producer task is upstream" — here the named producer *is this task*). gr-01/02/03 are δ's RED fixtures; its deliverable is turning them GREEN (`tree-sitter parse --quiet` exit 0) plus `tree-sitter-reify/tests/` parser tests. Every task that *emits* `relate`/`at auto` syntax (ζ, η) `depends_on` δ (δ→{ζ,η}), so no consumer asserts this grammar before δ delivers it. **The `joint … with` form (gr-05a/05b, also ERROR today) is explicitly NOT δ's responsibility** — it is the out-of-scope joint-half PRD's grammar producer (PRD §10).

**grammar_confirmed = false** — δ is the grammar producer (the only `false` in the batch).

---

## ε — Feature→datum trait bundle + dedup by geometric equivalence + `feature.axis : Axis|Axis?` refinement

**Signal:** a `.ri` example where `cylinder.axis` / `hole.axis` resolves to a concrete `Axis` over a realized feature (`reify eval` prints the datum / a relation over it type-checks against the realized solid); **B8 dedup** (revolved-rectangle cylinder → one axis) passes.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Typed topology-selector value type (`Type::Selector` / `SelectorKind`) | `producer:4116`, `producer:4117` (both **done** on main) — PRD §2 confirms landed | PASS (done) |
| Selector predicate-ctors + `resolve()` + `face()/edge()/body()` composition + gate | `producer:4118`, `producer:4119`, `producer:4120` — out-of-batch deps (pending), wired ε←{4118,4119,4120} | PASS (upstream prerequisites, wired) |
| `BRepAdaptor_*` → `GeomAbs_*` → `.Axis()` analytic classification (OCCT FFI) | `producer:ε` — the feature→datum bridge is ε's own deliverable (`crates/reify-compiler/src/geometry_traits_inference.rs`, OCCT FFI); the "real missing bridge" (PRD §3) | PASS (ε is the producer) |
| `β` (`Direction`/projection lattice) available | `producer:β` upstream (β→ε) | PASS (upstream) |

---

## ζ — Per-scope relate-solve at the `Resolution` node (integration-gate leaf)

**Signal (leaf — the consumer signal):** the §1 example builds — `reify build` places the bolt **coaxial + flush** to the plate (GUI mesh position via debug MCP / CI example asserting the solved transform); boundary tests **B1–B3 + B5** pass.

**G6 branch-3 end-to-end capability trace** — every capability ζ's signal requires is delivered by ζ itself or by a task **upstream** of ζ (never downstream):

| Capability the §1 build requires | Producer | Direction | Verdict |
|---|---|---|---|
| Production engine routes geometric constraints to a geometric solver | α (register `SolveSpaceSolver`) | α→ζ (upstream) | PASS |
| `Direction` + datum projections (`bolt.shank.axis`, `plate.top.plane`) | β | β→ζ (upstream) | PASS |
| `concentric` / `flush` mates returning `Relation` + DOF inference | γ | γ→ζ (upstream) | PASS |
| `at auto` + `relate { }` syntax parses & lowers | δ | δ→ζ (upstream) | PASS |
| `cylinder.axis` / `hole.axis` resolve to concrete `Axis` over realized features (+ dedup) | ε | ε→ζ (upstream) | PASS |
| A `Resolution`-node executor seam to host the per-scope solve | 4357–4362 (unified-DAG driver) | out-of-batch dep, wired ζ←{4357…4362} (upstream) | PASS |
| `ApplyTransform` places each sub from the solved `Frame` | 3901 (**done**) | out-of-batch dep, wired ζ←3901 (upstream) | PASS |
| Driving-set rank partition → `SolveSpaceSolver` → `SolveResult::Solved{values,unique}` | ζ (the relate-solve executor itself) | this leaf | PASS (ζ is the producer of its own integration) |

No required capability is owned by a task that **depends on** ζ — the anti-inversion (DAG-direction) check passes. ζ is the C-as-integration-gate leaf: α/β/γ/δ/ε are foundation intermediates roped into this leaf, satisfying the G2 escape hatch.

---

## η — `self` datums + grounding + construction-datum constructors (leaf)

**Signal (leaf):** a `.ri` example binds a construction datum (`let mid = midplane(...)`) and mates to it (builds); an ungrounded auto-assembly emits the **B6 global-float** diagnostic; `ground(sub)` sugar resolves to `fasten(sub.frame, self.frame)`.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Construction-datum constructors (`midplane(…)`, `offset(plane, δ)`) + `self` projections parse | `grammar-fixture:/tmp/prd-gate-fixtures/gr-09-construction.ri` → exit 0; `self.xy_plane` in `gr-08` → exit 0 | PASS |
| `at auto` / `relate` grammar (the assembly being grounded) | `producer:δ` upstream (δ→η) | PASS (upstream) |
| Relation vocabulary (`fasten`, the grounding mate) | `producer:γ` upstream (γ→η) | PASS (upstream) |
| The relate-solve executor that consumes grounding | `producer:ζ` upstream (ζ→η) | PASS (upstream) |
| `Direction`/projection lattice | `producer:β` upstream (β→η) | PASS (upstream) |

**grammar_confirmed = true** (constructors + `self` projections parse on existing grammar).

---

## θ — DOF ledger + geometric residual naming + conflict sets (leaf)

**Signal (leaf):** `reify explain` on an under-constrained `at auto` sub prints the DOF ledger (`spent 5 · free 1 → rotation about bolt.shank.axis`); a conflicting `relate` (**B3**) emits the minimal conflict set with geometric explanation; GUI DOF badge updates.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `undef-self-describing` tracer to extend with new `UndefCause` variants | `producer:θ` extends the landed tracer (PRD §6: undef-self-describing 4321–4327); θ adds `UndefCause::{SolveFailed{under/over/diverged}, DatumProjectionUnavailable}` | PASS (diagnostic-extension; substrate is the existing tracer) |
| `W_UNDERDETERMINED` to extend with geometric residual naming | `producer:θ` extends constraint-solver-completion §3.6 `W_UNDERDETERMINED` (PRD §6) | PASS (extension of existing diagnostic) |
| Solve result (`spent/free`, conflict set) to render | `producer:ζ` upstream (ζ→θ) — the relate-solve produces the DOF accounting + minimal conflict set θ renders | PASS (upstream) |
| Datum-projection availability info (for `DatumProjectionUnavailable`) | `producer:ε` upstream (ε→θ) | PASS (upstream) |
| Relation ΔDOF nominals for the ledger | `producer:γ` upstream (γ→θ) | PASS (upstream) |
| GUI DOF badge | viewport state via debug MCP (overlay signal vocabulary) | PASS (observable surface) |

The DOF numbers θ renders are exact codimension integers (see numeric-floor N/A above) — no numeric bound to floor-check.

---

## Batch summary (filled at file-time)

| Greek | Task ID | Title (abbrev) | Intra-batch prereqs | Out-of-batch prereqs |
|---|---|---|---|---|
| α | 4381 | Register SolveSpaceSolver (CLI+GUI) | — | — |
| β | 4382 | First-class `Direction` + projections | — | — |
| γ | 4383 | Relation vocab + `Relation` type + policing | β | 4235 (kind-generic only) |
| δ | 4384 | Grammar: `relate`/`at auto`/`where` | β, γ | — |
| ε | 4385 | Feature→datum bundle + dedup | β | 4118, 4119, 4120 |
| ζ | 4386 | Per-scope relate-solve @ Resolution node | α, β, γ, δ, ε | 4357, 4358, 4359, 4360, 4361, 4362, 3901 |
| η | 4387 | `self`/grounding/construction datums | β, γ, δ, ζ | — |
| θ | 4388 | DOF ledger + residual naming + conflicts | γ, ε, ζ | — |

Companion (G4 prose correction, out of the α–θ DAG): task **4389** — point `kinematic-inter-joint-offsets.md` (+ future `geometric-joints.md`) at design §8.2's relate↔KIN-OFFSET-1 co-design seam.
