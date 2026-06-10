# PRD — Geometric Joints (`joint … with` over `relate` + mount→FK offset) — joint half

**Milestone:** v0_6 · **Status:** active (foundation gated on the core relate substrate; the mount→FK seam gated on core ζ + KIN-OFFSET α) · **Approach:** B + H (contract + two-way boundary tests) · **Authored:** 2026-06-08

**Design of record:** `docs/design/geometric-relations.md` §7 (pure-mate-vs-joint abstraction), §8 (the reframed joint↔mechanism unification), §11 step 9, §14 (decisions locked, interactive 2026-06-08). This PRD is the **decomposition contract** for that design's *joint half* — the companion to the committed **core** PRD `docs/prds/v0_6/geometric-relations.md` (commit `e8ce420d3b`, tasks α–θ = 4381–4388). It does **not** re-open the locked decisions; read the design doc for the ontology (§2), vocabulary (§3), the abstraction mechanism (§7), and the §8 reframe. Durable record: `~/.claude/projects/-home-leo-src-reify/memory/project_geometric_constraint_relations_design.md` (the **ABSTRACTION** and **B-VERIFICATION** threads are load-bearing).

**Scope boundary (why "joint half"):** the **core** static assembly-mate system (design §11 steps 1–8 — `relate { }`, `at auto`, the relation vocabulary, the feature→datum bridge, the per-scope relate-solve, `self`/grounding, the DOF ledger) is **owned by the core PRD** (α–θ = 4381–4388) and **consumed here, not redone**. This PRD owns only the joint-specific surface: the `joint … with` **definition syntax** (grammar), the **self-checking law**, the **mount→`origin` handshake** (the seam that populates KIN-OFFSET-1's offset field), and the relate-defined standard joint library. It is hard-gated on **KIN-OFFSET-1** (`docs/prds/v0_6/kinematic-inter-joint-offsets.md`, task **4331**, in-progress) and co-designed with it per design §8.2. See **Out of scope**.

---

## 1. Goal & user-observable surface (G1)

Let a `.ri` author **define a joint with `relate`** — a coincidence-constraint body plus a named, typed residual DOF — and have the **existing mechanism subsystem animate it**, mounted at its real solved spatial position:

```reify
joint revolute(a: Axis, b: Axis, stop: Plane) with angle: Angle in 0deg..120deg = {
    coaxial(a, b)         // −4
    on(a.point, stop)     // axial stop, −1   → residual: 1 rotational (the declared `angle`)
}

// use site — the relate-solve fixes the MOUNT; the mechanism owns the motion:
sub arm : Arm at auto
relate { arm_joint = revolute(base.pivot.axis, arm.hub.axis, base.top.plane) }
// `arm_joint.angle` is bound/swept by the mechanism; the mount sits at the solved pivot, not the world origin.
```

The joint vocab is **re-based on the relate substrate** — joints are defined by `coaxial(a, b)`, not by hand axis-math — and a joint *cannot lie about its kinematics* (the self-checking law). The numbers and the fictional world-origin pivot are gone.

**Consumer / user-observable signals** (the chain terminates at the `.ri` author via CLI + GUI — no orphan producer):
- `reify check` on a well-formed `joint … with` definition **types it as a joint** with the declared driving DOF; on a DOF-mismatched definition it emits **`E_JOINT_DOF_MISMATCH`** with a geometric explanation — **before** any solve (the self-checking law, design §7/F).
- `reify build` / `reify eval` on a relate-defined joint places its **mount at the solved spatial position** (coaxial + axial-stop), observable as GUI mesh pose via the debug MCP or a CI `.ri` example asserting the posed transform — **not** at the world origin.
- Under `bind` / `sweep`, FK/snapshot **poses the link at the mounted position while the swept angle stays the mechanism's bind value** (the §8.1 reframe: relate places the mount, the mechanism owns the motion variable — the angle is never re-solved geometrically).
- A relate-placed mechanism with a **closed motion loop still runs the loop-closure Newton solver** at snapshot (§8.3).

**In-engine seam (overlay G1).** This introduces **no new seam**. The mount production rides `engine-integration-norm.md §3.5` (the ConstraintSolver / per-scope `Resolution` node — the same seam the core's relate-solve ζ uses); the motion rides `§3.6` (the freshness-only / FK walk) and the dynamics trampoline — exactly the seams KIN-OFFSET-1 widens. The mechanism subsystem (FK, loop-closure Newton, snapshot sweep, `joint_signatures.rs` typing family, mechanism-completion enforcement) is **reused, not replaced**.

---

## 2. Background

The mechanism subsystem is landed and rich, but two facts make the joint half a *gated* feature:

1. **`joint … with` does not parse.** Verified 2026-06-08: `tree-sitter parse --quiet` on the §1 form FAILs (`/tmp/prd-gate-fixtures/gr-05a-joint-with.ri` = **22 ERROR** nodes; the record form `gr-05b-joint-with-rec.ri` = **17 ERROR**). The core's grammar producer δ (4384) covers `relate { }` / `at auto` / `auto(…)` / `where` but **explicitly not** `joint … with` (core PRD §10). So this PRD owns a **grammar-producer task** for both the single (`with angle: T in range`) and record (`with { a: T, b: U }`) forms.

2. **Joints have no pivot/origin today** (the B-VERIFICATION SURPRISE#2). A joint is a `Value::Map { kind, axis, range }` (`reify-stdlib/src/joints.rs`) — `transform_at` rotates a revolute about the **world origin**; the `pivot: Point3` in older PRDs is a fiction `make_joint` never stores. **KIN-OFFSET-1** (task 4331) adds an optional `origin` `Value::Transform` key, pre-composed uniformly at `transform_at` (`origin ∘ motion`, outside the per-kind match) so FK / loop / dynamics all inherit it by construction. **`relate` is the natural front-end that produces the mount frames that field stores** (design §8.2) — that is the seam this PRD owns.

The reframe that makes B viable (design §8.1, B-VERIFICATION SURPRISE#1): the architecture carries **no symbolic/parametric residual** — every solve (incl. SolveSpace) returns a concrete `Value` per cell (`reify-ir/src/constraint.rs`); `auto(free)` = "skip the uniqueness check, return `unique:false`," never a free variable. The joint's motion variable was **never** a solver residual — it is supplied externally via `bind`/`sweep`/range-midpoint (`snapshot.rs`). Therefore **relate fixes the mount; the mechanism owns the motion** (§8.1). The self-checking law guarantees the geometric residual equals exactly the DOF the mechanism will drive.

(Code anchors are hints as of authoring — main moves fast; **re-locate every symbol at implementation time**, per design.)

---

## 3. Sketch of approach

Map design §11 step 9 onto a 5-task DAG (α–ε), three layers:

- **Definition layer (gated only on the core relate substrate):** the `joint … with` **grammar production** (α — the one novel-syntax prerequisite this PRD owns); the **self-checking law** (β — declared free DOF ↔ body geometric residual, by count + kind, at definition); the **standard relate-defined joint library** (γ — revolute/prismatic/cylindrical/planar/spherical/ball as `joint … with` over relation bodies; couplings-on-the-scalar-side boundary stated).
- **Seam layer (gated on core ζ + KIN-OFFSET α):** the **mount→`origin` handshake machinery** (δ — the relate-solved mount `Frame` written into the joint `Value::Map`'s `origin` field that KIN-OFFSET α adds).
- **Vertical slice (the integration-gate leaf):** the **end-to-end** relate-defined revolute mounted at a nonzero pivot that sweeps via the mechanism (ε — FK poses the link at the mount while the swept angle stays the mechanism's bind value; the §8.3 closed-loop-runs-Newton case).

### G3 — grammar reality (verified 2026-06-08)

`tree-sitter parse --quiet` over extracted fixtures (regenerate from design §7 if `/tmp` is gone):

| Fragment | Parses today? | Disposition |
|---|---|---|
| `joint NAME(d) with <name>: T in range = { body }` (single form) | ❌ `gr-05a` = 22 ERROR | **grammar-producer task α** (this PRD) |
| `joint NAME(d) with { a: T, b: U } = body` (record form) | ❌ `gr-05b` = 17 ERROR | **grammar-producer task α** (this PRD) |
| `coaxial(a, b)` / `on(a.point, stop)` (the body — relation vocab) | ✅ via core γ (4383) + the relation grammar core δ (4384) | consumed from the core, not redone |
| `relate { }` / `at auto` / `auto(…)` / `where` (the use-site context) | ❌ today, ✅ once core δ (4384) lands | **owned by core δ** — this PRD `depends_on` it |
| `range = 0deg..120deg` (dimensionally-typed) | ✅ existing `validate_range` (`joints.rs`) | reuse 1:1 |

So α is a real grammar prerequisite for the `joint … with` form only; every task that emits `joint … with` or `relate` syntax `depends_on` α (for the joint form) and core δ=4384 (for the relate context). The `joint … with` grammar is **not** core δ's responsibility (core PRD §10).

---

## 4. Resolved design decisions

These are **locked** (design §14 + the ABSTRACTION / B-VERIFICATION threads) — listed so the decomposition isn't re-litigated:

- **Joint = the one new definition syntax** `joint NAME(datums) with <named free DOF: typed [in range]> = <relation body>` (design §7). A pure mate (`fn … -> Relation`) needs **no** keyword and is owned by the **core** (γ=4383); only the `joint … with` form is joint-specific and owned here.
- **The self-checking law (design §7/F, load-bearing):** a joint's declared free DOF must **match the body's geometric residual by count + kind** (`Angle` ⇒ rotational, `Length` ⇒ translational, `Orientation` ⇒ 3 rotational), verified **at definition**; a mismatch is a compile error. Joints cannot lie about their kinematics. Post-§8 reframe: the declared DOF is the *mechanism-owned motion variable*; the self-check guarantees the geometric residual equals exactly that DOF.
- **The §8.1 reframe — relate places the MOUNT, not the motion variable.** The relate-solve determines the joint's mounting frame/axis (a concrete `SolveResult`); the joint's motion variable stays the mechanism's `bind`/`sweep`/range-midpoint value, **never an auto-param**. `revolute(a,b).angle` is a declared, mechanism-owned variable. No symbolic/parametric residual exists anywhere in the architecture.
- **§8.2 — `KIN-OFFSET-1` is a hard prerequisite, co-designed.** relate's solved mount `Frame` **populates** the optional `origin` `Value::Transform` field KIN-OFFSET α (4331) adds and threads through `transform_at` / `walk_fk` / `loop_residual_twist` / `loop_residual_jacobian_by_joint` / dynamics. **This PRD owns producing the mount frame and writing it; KIN-OFFSET-1 owns the field + its threading** — a resolved scope split, not a reciprocal seam.
- **§8.3 — solver split by topology.** The existing loop-closure Newton solver keeps motion-time closed-chain consistency (re-solves free joints per snapshot, warm-start for sweeps). The geometric (SolveSpace) relate-solve is confined to **single-shot static assembly-mate placement** — *not* closed kinematic loops with mobility. A relate-placed mechanism with a closed motion loop **still runs Newton at snapshot**.
- **§8.4 — couplings stay on the scalar side.** gear / screw / rack-and-pinion / `couple` are *algebraic ratios* between two joint variables (`v_child = ratio·v_parent + offset`), **not** geometric coincidence over datums; they belong on the scalar `constraint` side (the dimensional solver), **not** `relate`. This PRD states the boundary; it does **not** implement couplings.
- **Kind-generics & ranges:** `coincident<D: Datum>` and the relation ΔDOF inference are the core's (γ=4383, gated on generics 4235); `range` is dimensionally-typed and reuses `validate_range` verbatim (design §8.5).

---

## 5. Pre-conditions for activating

| Prerequisite | For | Status |
|---|---|---|
| Core γ (4383) — relation vocabulary + `Relation` type + ΔDOF inference | α (body parses to `Relation`), β (residual count from ΔDOF), γ (joint bodies are relation conjunctions) | **pending** |
| Core δ (4384) — `relate`/`at auto`/`auto(…)`/`where` grammar | α (the `joint … with` grammar extends the relate grammar) | **pending** |
| Core ε (4385) — feature→datum bridge (joints grip datums) | the use site (a joint over `hole.axis` / `shaft.axis` resolved features) — reached transitively via core ζ | **pending** (blocked on 4118/4119/4120) |
| Core ζ (4386) — per-scope relate-solve at the `Resolution` node | δ + ε — **produces the mount frame the joint seam consumes** | **pending** (gated on unified-DAG driver 4357–4362) |
| **KIN-OFFSET α (4331)** — the optional `origin` `Value::Transform` field + uniform `transform_at` threading | δ (the field this PRD's seam writes), ε (offset-aware FK) | **in-progress** |
| Generics 4232 (done) / 4235 (pending) | kind-generic relation bodies (consumed from core γ) | 4232 done; 4235 pending |
| Mechanism subsystem (mechanism-completion done; `joint_signatures.rs`; KCC closed-chain) | β (self-check typing family), γ (driving-vs-non-driving), ε (FK/loop/snapshot) | **done — reuse** |

**No task is buildable until the core relate substrate (γ, δ) lands**; the seam layer (δ, ε) additionally stages behind core ζ + KIN-OFFSET α via real `add_dependency` edges, so the scheduler holds the integration tasks behind the (pending/in-progress) core ζ + KIN-OFFSET α edges (blocked-vs-pending semantics).

---

## 6. Cross-PRD relationship (G4)

| Other PRD / substrate | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `v0_6/geometric-relations.md` (core, committed `e8ce420d3b`) | consumes | the relation vocab + `Relation` type (γ=4383), the `relate`/`at auto` grammar (δ=4384), the feature→datum bridge (ε=4385), and the per-scope relate-solve at the `Resolution` node (ζ=4386) are all the core's; this PRD adds only `joint … with` on top | core owns the substrate; **this PRD** owns the joint surface | core α–θ queued |
| `v0_6/kinematic-inter-joint-offsets.md` (KIN-OFFSET-1 / task **4331**) | **produces** (this PRD) / consumes (that PRD) | relate's solved **mount frame** is written into the optional `origin` `Value::Transform` field KIN-OFFSET α adds + threads through `transform_at`/FK/loop/dynamics | **`geometric-joints.md` (this PRD)** owns *producing + writing* the mount frame; **KIN-OFFSET-1** owns the *field + threading* | **resolved scope split** — 4331 in-progress; KIN-OFFSET α's `consumer_ref` already names this PRD |
| mechanism subsystem / mechanism-completion | consumes | the relate-defined joint = a mechanism-owned motion variable mounted by relate; FK, loop-closure Newton, snapshot sweep, `joint_signatures.rs`, driving-vs-non-driving enforcement | mechanism subsystem owns animation; this PRD owns definition + mount | **done — reuse, don't replace** |
| `v0_6/constraint-solver-completion.md` / undef-self-describing (4321–4327) | extends | the self-check + mount-solve diagnostics speak geometry (reuse `W_UNDERDETERMINED` + the `UndefCause` tracer extended by core θ=4388) | core θ owns the geometric-residual diagnostic surfaces; this PRD's `E_JOINT_DOF_MISMATCH` is a definition-time typed diagnostic | core θ queued |
| `geometry-transforms-frames-projection.md` (P6) | adjacent | `Frame`/`Transform` surface types for the mount/`origin` | P6 owns the Frame3 surface; KIN-OFFSET δ owns oriented authoring | wired (Frame/Transform exist) |
| scalar `constraint` side / coupling vocabulary | **boundary (not a seam)** | gear/screw/rack-and-pinion/`couple` are algebraic ratios on the scalar side — explicitly **not** `relate` | scalar `constraint` side owns couplings; this PRD only states the boundary | out of scope here |

**Reciprocity resolution (the one real risk), resolved.** relate↔KIN-OFFSET-1 — each could claim "the other threads the offset." Resolved by the clean scope split above: **KIN-OFFSET-1 owns the offset field + its threading through FK/loop/dynamics; this PRD owns *producing* the mount frame via `relate` and *writing* it into that field.** KIN-OFFSET α's `consumer_ref` already names *"the geometric-joints joint half (relate's solved mount frame populates this origin field, design §8.2)"* — so the seam is owned, not reciprocal. No new contested-ownership seam (checked against the overlay's known trio: persistent-naming/multi-kernel, imported-field/multi-kernel, topology-selectors/persistent-naming).

---

## 7. Contract section (H) — the two load-bearing seams

### 7.1 The joint-definition contract — the self-checking law

Given a definition `joint NAME(datums) with <declared free DOF> = <relation body>`, the compiler **MUST**, at definition time (before any solve):

1. **Type the body** as a conjunction of `Relation`s over the declared datum parameters (core γ=4383 supplies the relation vocab + `Relation` type; core δ=4384 supplies the `joint … with` body grammar this PRD's α extends).
2. **Compute the body's geometric residual.** The joint grips a relative pose between two datum sets — 6 DOF nominal. Subtract the body's removed DOF using the **nominal ΔDOF inference** from the core's `relation_signatures.rs` (γ=4383; `coincident(X,X)` removes `codim(X)`, `on` removes `3 − dim(host)`, metrics remove 1), independence-assumed (the same accounting as the core DOF ledger). `residual = 6 − Σ ΔDOF(body)`.
3. **Classify the residual by kind** from the relation algebra's null-space structure: each residual DOF is **rotational** or **translational** (`coaxial` leaves 1 rot + 1 trans; an axial stop removes the translational one → 1 rot; etc.).
4. **Match declared ↔ residual by count AND kind.** `Angle`-typed declared field ⇒ 1 rotational; `Length` ⇒ 1 translational; `Orientation` ⇒ 3 rotational. The multiset of declared kinds **MUST** equal the multiset of residual kinds.
5. **On mismatch, emit `E_JOINT_DOF_MISMATCH`** with a geometric explanation, e.g. *"declared 1 rotational free DOF, but the relation leaves 1 rot + 1 trans; add a constraint or declare `travel: Length`."* The error speaks geometry, never solver internals.

**Home:** the `joint_signatures.rs` typing family (`crates/reify-compiler/src/joint_signatures.rs`, the landed `is_*_typed_fn` / `DRIVING_JOINT_KINDS` pattern), reusing the ΔDOF attribute from core γ's `relation_signatures.rs` and `validate_range` (`reify-stdlib/src/joints.rs:1290`) for the dimensionally-typed `range`. **Invariant:** the self-check is exact integer count+kind arithmetic — no numeric tolerance, no error floor (G6 numeric-floor N/A).

### 7.2 The mount→`origin` handshake contract — the seam this PRD owns

After the per-scope relate-solve (core ζ=4386) yields a concrete `SolveResult::Solved { Frame }` for a joint's mounting datums, the joint half **MUST**:

1. **Write the solved mount `Frame` into the joint `Value::Map`'s `origin` key** — a `Value::Transform` (SE(3) Frame3), the field **KIN-OFFSET α (4331) adds** and threads. The mount `Frame` from `relate` matches the `origin` field's type exactly (a full 6-DOF Frame3 — design decision KIN-OFFSET §4.2 chose Frame3 *specifically* so geometric-joints consumes it without widening).
2. **Ordering (load-bearing):** *solve mount* (core ζ at the `Resolution` node) → *write `origin`* (this PRD, δ) → *FK places* (KIN-OFFSET α's `transform_at` = `origin ∘ motion`). The relate-solve runs **once** (single-shot, §8.3); FK/snapshot then pose the link at the mount each evaluation.
3. **The motion variable is NOT the origin.** `revolute(a,b).angle` stays the mechanism's `bind`/`sweep`/range-midpoint value (`snapshot.rs`); the `origin` fixes only the **mount**. FK at angle θ poses the link at `origin ∘ rot(axis, θ)` — the angle is **never re-solved geometrically** (§8.1).
4. **Back-compat (KIN-OFFSET α's invariant):** a joint with no relate-placement carries **no** `origin` ⇒ identity ⇒ byte-identical to today's behaviour. The seam is additive: it only *populates* the field for relate-defined joints.
5. **§8.3 closed-loop invariant:** a relate-placed mechanism with a **closed motion loop** still runs the loop-closure Newton solver at snapshot — the relate-solve placed the mounts once; Newton owns motion-time closed-chain consistency over the now-offset-aware link geometry (exactly the link geometry KIN-OFFSET-1's four-bar e2e exercises).

**Tolerance hierarchy (the coherence law, design §2.3 / core §7.1).** `kernel_local ≤ solver_convergence ≤ assertion/dedup` — the relate-solve's mount `Frame` satisfies `coaxial` within the assertion tolerance, which dominates the solver convergence; the written `origin` carries that solved `Frame` verbatim (no re-quantization). No fixed numeric premise is asserted by any leaf signal (G6 numeric-floor N/A).

---

## 8. Boundary-test sketch (H) — facing the compiler, the `.ri` author, and the mechanism consumer

| # | Scenario | Preconditions | Postconditions (assert) |
|---|---|---|---|
| B1 | self-check **pass** | `joint revolute(a,b,stop) with angle: Angle = { coaxial(a,b); on(a.point, stop) }` (residual 1 rot) | type-checks as a joint; declared DOF == residual (1 rotational); driving DOF = `angle` |
| B2 | self-check **fail (count)** | declares `with angle: Angle` but body = `coaxial(a,b)` only (residual 1 rot + 1 trans) | `E_JOINT_DOF_MISMATCH`: *"declared 1 rotational, relation leaves 1 rot + 1 trans; add a constraint or declare `travel: Length`"* |
| B3 | self-check **fail (kind)** | declares `with travel: Length` but body leaves a rotational residual | `E_JOINT_DOF_MISMATCH` (kind mismatch — rotational vs translational), at `reify check`, before any solve |
| B4 | record `with { }` form | `joint cylindrical(a,b) with { angle: Angle, travel: Length } = coaxial(a,b)` (residual 1 rot + 1 trans) | parses (`gr-05b` GREEN) **and** type-checks (2-DOF match: 1 rot + 1 trans) |
| B5 | **mount solved + written to `origin`** (PRODUCER side) | relate-defined revolute, `coaxial(a,b)` over datums at a nonzero pivot | relate-solve yields a mount `Frame` coaxial within tol; that `Frame` is written into the joint Map's `origin` (`Value::Transform`, **nonzero** translation) — not left identity |
| B6 | **FK poses at mount; motion stays the mechanism's** (CONSUMER side) | the B5 joint, `bind(joint, 30deg)` / sweep | `walk_fk` poses the link at the mounted pivot (`origin ∘ motion`); the swept angle **== the mechanism's bind value (30deg)**, NOT re-solved geometrically; `transform_at == origin ∘ rot(axis, 30deg)` |
| B7 | **closed motion loop still runs Newton** (§8.3) | a relate-placed planar 4-bar (mounts placed once by relate; closed loop) | the loop-closure Newton solver runs at snapshot (re-solves free joints, warm-start); the loop closes at the **offset-aware nonzero** residual the relate-placed link geometry produces |
| B8 | **couplings stay scalar-side** (boundary) | a gear ratio / `couple(parent, ratio, offset)` | routed to the scalar `constraint` side (algebraic ratio); a `relate { }` body member that is a coupling is a type error / absent from the relation vocabulary — never lowered to an SE(3) relation |
| B9 | **back-compat no-op** | every existing mechanism joint (no relate, no `origin`) | absent `origin` ⇒ identity ⇒ composed transforms **byte-identical** to pre-change (KIN-OFFSET α's no-op invariant); full mechanism suite green |

**Signal assignment:** β names **B1–B4** (the self-checking law). γ names **B8** + the standard joint library type-checks. δ names **B5** (the mount→`origin` producer write + B9 the back-compat no-op). The **integration-gate leaf ε** names **B6 + B7** (the consumer-observable e2e — a relate-defined revolute mounted at a nonzero pivot that FK-poses at the mount while the swept angle stays the mechanism's bind value, plus the closed-loop-runs-Newton case). These face both the producer (relate-solve + the `origin` write + the FK/loop walks) and the consumer (`reify build`/`eval` + the mechanism sweep).

---

## 9. Decomposition plan (α–ε; G2 signal per task)

Greek labels here; task IDs assigned at decompose time. **Definition layer** (α–γ, gated only on the core relate substrate); **seam layer** (δ, gated on core ζ + KIN-OFFSET α); **vertical slice** (ε — the integration-gate leaf).

- **α — Grammar production: the `joint … with` definition syntax** (both the single `with <name>: T in range` form and the record `with { a: T, b: U }` form) + lowering. tree-sitter rule extending the relate grammar (core δ), parser tests, lowering to a joint-definition node. Modules: `tree-sitter-reify/`, `reify-compiler` lowering. *Signal (intermediate → β, γ):* fixtures `gr-05a` (single) **and** `gr-05b` (record) parse (`tree-sitter parse --quiet` exit 0) with parser tests in `tree-sitter-reify/tests/`; the lowered node carries the declared DOF + body. *Prereqs:* core **γ (4383)** (the `Relation` type the body lowers to), core **δ (4384)** (the relate grammar this extends). **`grammar_confirmed=false` — this is the joint-grammar producer.**

- **β — The self-checking law (definition-time DOF count + kind match).** The `joint_signatures.rs`-family typing of `joint … with`: compute the body's geometric residual from the core's relation ΔDOF inference, classify by kind (rotational/translational), match against the declared `with` DOF by count + kind, emit `E_JOINT_DOF_MISMATCH` on mismatch. Modules: `reify-compiler/src/joint_signatures.rs`, the relation ΔDOF attribute (core γ), `validate_range` (`reify-stdlib/src/joints.rs:1290`). *Signal (intermediate → γ):* `reify check` on a matched joint (B1) types it; on a count-mismatch (B2) and a kind-mismatch (B3) emits `E_JOINT_DOF_MISMATCH` with the geometric explanation, **before** any solve; a CI `.ri` example exercises pass + both fail modes. *Prereqs:* α, core **γ (4383)**. `grammar_confirmed=false` (consumes α's grammar).

- **γ — The standard relate-defined joint library + the couplings-on-the-scalar-side boundary.** revolute / prismatic / cylindrical / planar / spherical / ball defined as `joint … with` over `coaxial` / `coincident` / `on` relation bodies (design §7 / §13 open-Q-1 enumeration), each passing the self-check; dimensionally-typed `range` via `validate_range`; driving-vs-non-driving via the mechanism-completion enforcement; couplings (gear/screw/rack-and-pinion/`couple`) documented + enforced as scalar-`constraint`-side, **not** `relate` (B8). Modules: a stdlib `.ri` joint library, `joint_signatures.rs`, mechanism-completion enforcement. *Signal (intermediate → δ):* a `.ri` example defines the standard joint set and `reify check` types each as a joint with the correct driving DOF (all self-checks pass); **B8** — a coupling in a `relate { }` body is rejected (type error / not in the relation vocab). *Prereqs:* α, β, core **γ (4383)**. `grammar_confirmed=false` (uses `joint … with`).

- **δ — The mount→`origin` handshake machinery (the seam this PRD owns).** After the per-scope relate-solve (core ζ) yields a concrete mount `Frame` for a joint's mounting datums, write it into the joint `Value::Map`'s `origin` `Value::Transform` key (the field KIN-OFFSET α adds); preserve the absent-`origin` ⇒ identity back-compat. Modules: the relate-solve → joint-placement path at the `Resolution` node (core ζ), `reify-stdlib/src/joints.rs` (the `origin` field KIN-OFFSET α adds). *Signal (intermediate → ε):* **B5** — a relate-defined revolute over datums at a nonzero pivot has its solved mount `Frame` (coaxial within tol) written into the joint Map's `origin` (`Value::Transform`, nonzero translation), verified by a Rust/`.ri` test reading the joint Map; **B9** — joints with no relate-placement carry no `origin` and the mechanism suite stays green (byte-identical). *Prereqs:* γ; out-of-batch core **ζ (4386)** (produces the mount frame), **KIN-OFFSET α (4331)** (the `origin` field). `grammar_confirmed=false`.

- **ε — End-to-end vertical slice: a relate-defined joint mounted at a nonzero pivot that sweeps via the mechanism.** *The integration-gate leaf / consumer signal.* A relate-defined revolute mounts at a solved nonzero pivot; under `bind`/`sweep` FK/snapshot poses the link at the mount while the swept angle stays the mechanism's bind value (§8.1); a relate-placed closed-loop mechanism still runs Newton at snapshot (§8.3). Modules: an `examples/` `.ri` mechanism (NEW), `crates/reify-eval/tests/` e2e, exercising core ζ's relate-solve + δ's `origin` write + KIN-OFFSET α's offset-aware FK + the mechanism sweep. *Signal (leaf — the consumer signal):* `reify build`/`eval` on the example poses the link at the **solved nonzero pivot** (GUI mesh pose via debug MCP / CI example asserting the posed transform), the swept angle == the bind value (**B6**), and the closed-loop variant closes via Newton at the offset-aware residual (**B7**); the companion Rust e2e passes. *Prereqs:* δ; out-of-batch core **ζ (4386)** (relate-solve, directly invoked at build), **KIN-OFFSET α (4331)** (offset-aware FK threading, directly invoked).

**DAG:** β←α · γ←{α,β} · δ←γ · ε←δ. Out-of-batch: α←{4383,4384} · β←4383 · γ←4383 · δ←{4386,4331} · ε←{4386,4331}. The scheduler holds δ + ε behind the (pending) core ζ (4386) + (in-progress) KIN-OFFSET α (4331) edges; α/β/γ stage behind the (pending) core γ/δ (4383/4384). No task dispatches until the core relate substrate lands — correct (the joint half *is* gated, design §11 step 9).

---

## 10. Out of scope for this PRD

- **The core static assembly-mate system** (design §11 steps 1–8): `relate { }`, `at auto`, the relation vocabulary + `Relation` type, the feature→datum bridge, the per-scope relate-solve, `self`/grounding, the DOF ledger → **owned by the core PRD** `geometric-relations.md` (α–θ = 4381–4388) and consumed here.
- **The `origin` field representation + its threading** through `transform_at` / `walk_fk` / `loop_residual_twist` / `loop_residual_jacobian_by_joint` / dynamics → **owned by KIN-OFFSET-1** (task 4331). This PRD only *produces + writes* the mount frame into that field.
- **Couplings** (gear / screw / rack-and-pinion / `couple`): algebraic ratios on the scalar `constraint` side — the boundary is **stated and enforced** (B8) but couplings are **not implemented** here (design §8.4).
- **Geometry-in-the-loop solving** (a relation whose datum depends on the very pose it constrains) — `E_EVAL_UNRESOLVED`; a future PRD (design §12).
- **Closed kinematic loops with mobility via the geometric (SolveSpace) solver** — owned by the existing loop-closure Newton solver (design §8.3). This PRD only makes relate *place the mounts* so Newton has real link geometry to close.
- **First-class partial application** of relations (design §7) and **cross-structure-level relations** (design §4) — both out of scope for v1 (inherited from the core).
- **Oriented Frame3 joint-origin `.ri` authoring** beyond what relate produces — that authoring surface is KIN-OFFSET δ (P6-gated); here the mount frame comes from the relate-solve, not hand-authored.

---

## 11. Open questions (tactical — surfaced, not blocking; design §13)

1. **Standard joint library enumeration.** Pin the exact set + definitions (revolute / prismatic / cylindrical / planar / spherical / ball; tangent/offset/centered/symmetric are pure mates owned by the core). *Decide during γ.*
2. **Final `joint … with` keyword spelling.** `joint … with` / `with { … }` are the working names — confirm before α's grammar lands (design §13.3). *Decide before α.*
3. **`with`-DOF kind inference for compound bodies.** The residual-kind classification (rotational vs translational) for a body mixing `coaxial` + `on` + metrics — pin the null-space-structure rule the self-check uses. *Decide during β.*
4. **Where the mount→`origin` write lives.** At the `Resolution` node immediately after the relate-solve (core ζ's path) vs in a joint-construction post-pass — must run after the solve, before FK. *Decide during δ.*
5. **Closed-loop relate-placement interplay with warm-start.** Whether a relate-placed closed loop seeds Newton's warm-start from the mounted config. *Decide during ε.*

---

## 12. Notes for decompose mode

- File α–ε with `planning_mode=True`; wire **all** deps (intra-batch per §9 DAG + out-of-batch: α←4383/4384, β←4383, γ←4383, δ←4386/4331, ε←4386/4331) while deferred; flip the whole batch to `pending` in one bulk call. The scheduler holds the entire batch behind the core relate substrate (4383/4384) and the seam layer behind core ζ (4386) + KIN-OFFSET α (4331) — correct, the joint half is gated (design §11 step 9).
- δ and ε share `joints.rs` / the relate-solve placement path touch points — ε←δ orders them so the narrow-file-lock collision (overlay G5 / G2 escape hatch) is the dependency edge, not a starvation race.
- Build the **capability manifest** beside this PRD (`geometric-joints.capability-manifest.md`): the **grammar-fixture** binding for α uses `gr-05a` (22 ERROR) + `gr-05b` (17 ERROR) — *producer-self PASS* (α **is** the named `joint … with` grammar producer; turning the fixtures GREEN is its deliverable, `grammar_confirmed=false`); the **G6 branch-3 end-to-end trace** for δ + ε to core ζ (4386) + KIN-OFFSET α (4331); **field-population N/A** (the `origin` write is an *input*-field `Value::Transform` on the production path — covered by the anti-orphan/wired-on-main binding that the producer writes a real non-`Undef` Frame, not the result-field sentinel); **numeric-floor N/A** (the DOF numbers are exact codimension integers and the self-check is exact count+kind matching — G6 branches 1/2 do not fire). Any FAIL blocks the batch.
- **Do not re-file the cross-PRD prose task** — the core's companion 4389 (in-progress) and KIN-OFFSET κ already own pointing the design + the two PRDs at the §8.2 co-design seam. Now that this PRD exists, those tasks concretize their forward-pointers (a dedup note, not a new task).
- The `user_observable_signal` / `consumer_ref` / `grammar_confirmed` metadata fields are substrate for future tracking infra — the orchestrator does not read them yet.
