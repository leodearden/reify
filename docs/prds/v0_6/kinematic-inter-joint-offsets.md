# PRD — Kinematic fixed inter-joint (link-length) offsets (KIN-OFFSET-1)

**Status:** ACTIVE · B+H · promoted from forward-design stub (was DEFERRED) · **Promoted:** 2026-06-08 (interactive `/prd` author) · **Milestone:** v0_6
**Tracker:** **KIN-OFFSET-1 = task 4331** (currently `deferred`; flip to `pending` at decompose).
**Supersedes:** the 2026-06-03 deferred stub at this path (scope/gap/hazards record). This document is the full design — re-walks G1–G6 + META and resolves the A-vs-B fork the stub deferred.

> Promotion trigger: authored while writing `docs/prds/v0_6/geometric-relations.md` (committed `e8ce420d3b`). KIN-OFFSET-1 is the **hard prerequisite for that design's joint half** (design `docs/design/geometric-relations.md` §8.2). This PRD owns the **offset representation + threading**; the companion `geometric-joints.md` owns the `relate`↔KIN-OFFSET-1 seam and **depends on this PRD** (design §8.2 co-design).

---

## §0 — Why this exists (the proven substrate gap)

The Reify kinematic FK / loop layer has **no representation for fixed inter-joint (link-length) spatial offsets**. A joint is a `Value::Map { kind, axis, range }` (`joints.rs`) — no origin / pivot / offset frame — and the per-DOF transform a joint contributes is the motion transform **alone**:

| Anchor | Behaviour today |
|---|---|
| `reify-stdlib/src/joints.rs:173` (`transform_at` dispatch) → `:1519` (`transform_at_simple_joint`) | revolute = **pure rotation about the world origin**, prismatic = pure translation, fixed = identity, planar/spherical/cylindrical likewise origin-anchored. No constant link offset. The `pivot: Point3` in older PRDs is a fiction `make_joint` never stores; coupling's `offset` is a 1-D motion-ratio scalar, not a spatial vector. |
| `reify-stdlib/src/loop_closure.rs:77` (`chain_transform`) | folds `T_0 · T_1 · … · T_{n-1}`, each `T_i = transform_at(joint_i, value_i)` via `transform_compose`. Basis of `loop_residual_twist` (`:111`), used by **both** the snapshot kinematic loop solver **and** the dynamics constraint-Jacobian `loop_residual_jacobian_by_joint` (`:808`, central-differences the same twist). |
| `reify-stdlib/src/snapshot.rs:873` (`joint_world_transform`) → `:912` | composes the same pure `transform_at` DOF transforms up the `joint_parents` chain; a child composes from its parent **joint**'s bare frame (`:890`), never the parent body's posed frame. `walk_fk` (`:700`) applies `body.world_transform = joint_world_transform ∘ pose` (`:721`) but **never** threads `pose` into child joints' parent frames. |
| `reify-stdlib/src/dynamics/eval.rs:451,537,1091` + `:1149` | the closed-chain bridge reads each body's FK `world_transform` (`X_{p→i} = from_frame3(world_transform)`) **and** `loop_residual_jacobian_by_joint` → `reduce_constraint_rank` (`:1169`). |

**Consequence (proven in `esc-4146-280`).** In a revolute-only chain every pivot is **coincident at the world origin**. A "planar revolute 4-bar" therefore has a translational loop residual that is **identically zero for all joint angles** — no link geometry, no spatial 4-bar motion, no analytic energy rate `dE/dt` to reproduce. Two full agent budgets burned on this inexpressible fixture before the root cause was proven; task **4146** landed the closed-chain bridge (`done`, commit `316c0f9229`) but re-scoped its e2e (Leo decision **B1**) onto an *expressible* vertical 2-prismatic loop whose closing joint shares the residual axis → `m_eff = 0`. That validates the energy ledger but **not** the live-constraint (`m_eff ≥ 1`, non-empty rank-reduction, λ, incidence-map) path at the Value/e2e level. **This PRD's feature is the prerequisite for that stronger e2e.**

---

## §1 — Goal & user-observable surface (G1)

**Consumer.** A mechanism designer modeling a **real spatial linkage** — links with actual lengths between pivots (a revolute **four-bar**, slider-crank, pantograph) — where the closed loop has genuine mobility (`m_eff ≥ 1`). The headline unlock is the one e2e **task 4146 could not build**:

> `reify eval examples/dynamics/closed_4bar_idyn.ri` on a planar revolute four-bar (Grashof link lengths authored as pivot offsets) at `θ_input = 45°`, `ω = 2π rad/s` prints finite input torques whose virtual-work power `Σ τ_i·q̇_i` matches an **independently-derived analytic 4-bar `dE/dt`** within an honestly-derived tolerance — the live-constraint (`m_eff ≥ 1`) closed-chain inverse-dynamics path, end to end.

**Secondary consumer.** The **joint half** of geometric-relations (`geometric-joints.md`, future): `relate` solves a joint's **mount frame/axis** (a concrete `SolveResult`), and the mount frame is **exactly** the offset field this PRD threads (design §8.2 — "relate is the natural front-end that produces the mount frames that field stores"). That PRD consumes this representation; it does not re-thread the offset.

In-engine seam (overlay G1): the change rides `engine-integration-norm.md §3.6` (the freshness-only / FK walk) and `§3.5`/the dynamics trampoline — it does **not** introduce a new seam; it widens the transform a joint contributes inside the existing FK + loop + dynamics walks.

---

## §2 — Background

- `esc-4146-280` — the proven root cause (full code-grounded analysis; `resolved`, Leo B1).
- task **4146** (`done`, `316c0f9229`) — the Value→closed_chain bridge (M assembly, incidence map, `reduce_constraint_rank`, KKT routing) is **landed and GREEN**; only its live-constraint analytic e2e was blocked on this gap. This PRD is its kinematic prerequisite, **not** a rewrite.
- `examples/dynamics/closed_2prismatic_idyn.ri` + `crates/reify-eval/tests/closed_chain_idyn_e2e.rs::closed_2prismatic_virtual_work_identity` — the landed `m_eff = 0` e2e the four-bar e2e is modeled on (same bridge, same identity, stronger constraint).
- `docs/design/geometric-relations.md` §8.2 / §8.3 / §11 step 9 — the co-design seam; the geometric (SolveSpace) solver places **mounts**, the existing loop-closure Newton solver owns **motion-time** closed-chain consistency.

---

## §3 — Sketch of approach (Design A — thread once at `transform_at`)

**The single-change-point insight.** Every consumer the offset must reach bottoms out at **one primitive — `transform_at(joint, value)`**:

```
walk_fk → joint_world_transform → transform_at        (FK world_transforms)
loop_residual_twist → chain_transform → transform_at   (kinematic loop solver)
loop_residual_jacobian_by_joint → loop_residual_twist  (dynamics constraint-Jacobian = central-difference of the twist)
dynamics/eval.rs → reads FK world_transform  +  loop_residual_jacobian_by_joint
```

So: **store a constant origin frame on the joint Map and pre-compose it inside `transform_at`** — `transform_at(joint, v) = origin ∘ motion(axis, v)` — and **all four consumers inherit the offset from the one shared primitive and stay mutually consistent by construction.** The dynamics virtual-work identity holds because the FK `world_transform`s (from `transform_at`) and the constraint Jacobian (FD of `loop_residual_twist`, also from `transform_at`) read the *same* offset-aware transform; there is no parallel-maintained second path to drift.

**Representation.** An optional `"origin"` key on the joint `Value::Map`, holding a `Value::Transform` (an SE(3) Frame3). **Absent `origin` ⇒ identity ⇒ today's behaviour byte-for-byte** — the change is *additive at the representation level*, which is what makes the "re-validate every test" checklist tractable (existing joints carry no `origin`, so their composed transforms are unchanged).

**Threading.** A single uniform pre-compose at the top of the `transform_at` match (`joints.rs:198`): compute the per-kind motion transform as today, then return `transform_compose(origin, motion)` (origin read from the Map, defaulting to identity). Uniform across **all** kinds (revolute / prismatic / fixed / planar / spherical / cylindrical) — not per-arm — so no kind can silently miss the offset. `transform_compose(A, B) = A then B` (the left-to-right convention `chain_transform`/the planar arm already use), so `origin` is applied first (the constant mount), then the motion at the local frame.

**Authoring surface (minimal, self-contained).** Revolute/prismatic constructors gain an **optional pivot argument** authored as a translation — `revolute(axis, range, pivot: point3(40mm, 0mm, 0mm))` — internally lifted to a pure-translation `Value::Transform`. `point3`/`vec3` parse today and the constructors already take varargs (the coupling path is 3-arg), so **no new grammar** (`grammar_confirmed=true`). A planar revolute four-bar needs only pivot translations (axes all ∥ z; the rotation DOFs carry orientation), so this surface is sufficient for the headline e2e. Full **oriented** Frame3 authoring (3D linkages) is the P6-gated follow-up (§9 δ).

### G3 — substrate reality (verified 2026-06-08)

| Assumed capability | Reality |
|---|---|
| N-arg joint constructor + `point3(...)`/`vec3(...)` in arg position parse | **Yes** — `prismatic(axis_z, 0mm..1000mm)`, `point3(0mm,0mm,0mm)`, `MassProperties(...)`, and the 3-arg `couple(parent, ratio, offset)` all parse today. New pivot arg = a longer arg list, not new syntax. |
| `Value::Transform` (SE(3)) as the origin payload + `transform_compose` / `transform3_identity` | **Yes** — used throughout `joints.rs` (planar arm), `loop_closure.rs`, `dynamics/spatial.rs::from_frame3`. |
| `frame3(origin, basis) -> Value::Frame` exists (for the δ oriented-authoring follow-up) | **Yes** — `geometry.rs:250`; `project(point, Frame<3>)` landed (task 4165). The δ follow-up accepts this as a joint origin once the P6 Frame3 surface matures. |
| The landed 4146 bridge (M / incidence / rank reduction / KKT) consumes the threaded transforms unchanged | **Yes** — it reads FK `world_transform` + `loop_residual_jacobian_by_joint`; both inherit the offset via `transform_at`. **No bridge change.** |

No grammar producer task is required. The only novel `.ri` an author writes (a revolute joint with a `pivot:` arg) is a function call that already parses; α ships a committed `.ri` fixture proving it.

---

## §4 — Resolved design decisions

1. **Design A (offset inside `transform_at`), not B (pose-threading).** Ratified interactively 2026-06-08. **Corrects the stub's "B is the lighter touch" premise:** B threads body `pose` into `joint_world_transform` (FK) only — but `chain_transform` (the loop solver / Jacobian basis) folds a *flat list of joint Maps with no body poses*, so under B the loop residual **stays origin-coincident and the four-bar still cannot close** without separate, harder surgery; B also rewrites FK semantics for *every existing mechanism* (not additive). A threads the one primitive both composition paths share, is backward-compatible (absent origin = identity), and is the only design where FK and the loop residual read the **same** offset-aware transform — exactly the mutual-consistency the seam requires.
2. **Representation = a general SE(3) `Frame3` (`Value::Transform`) `origin` field**, authored minimally via a **translation pivot** for now. The full Frame3 matches `relate`'s 6-DOF mount-frame output (so geometric-joints consumes it without widening) and supports 3D linkages; the planar four-bar e2e exercises the translation part. (Rejected: translation-only `Vector3<Length>` representation — would force geometric-joints to widen it later.)
3. **The offset is a structural constant, never a differentiation variable.** `loop_residual_jacobian_by_joint` perturbs *motion* values and re-evaluates the twist; the constant `origin` enters the residual (making it nonzero / config-dependent) but is held fixed under perturbation — correct, since the offset is not a DOF.
4. **Tolerance discipline (G6).** The four-bar virtual-work e2e tolerance is **derived from the loop-closure Newton convergence floor**, not guessed. Work-energy is exact to roundoff; the real floor is the position/velocity loop-solve residual the torque inherits. β derives the analytic 4-bar position+velocity loop solution closed-form, then sets the e2e tolerance a safety factor above the *measured* solver residual — never a frozen `1µW`-style literal copied from the 2-prismatic case. (Overlay G6 precedents: `esc-3821-44`, `esc-3453` — guessed bounds below the real floor froze false RED tests.)
5. **Self-contained, dispatchable now.** With pivot authoring (decision 2) the core (α/β/γ) has **no upstream dependency** — the bridge is landed, `point3`/`revolute`/`body`/`MassProperties` parse. Only the oriented-Frame3 follow-up (δ) gates on P6.

---

## §5 — Pre-conditions for activating

- **None for the core (α/β/γ/κ).** task 4146 is `done`; the authoring substrate parses today.
- **δ (oriented Frame3 authoring)** is sequenced after the **geometry-transforms-frames PRD** (`docs/prds/geometry-transforms-frames-projection.md`) matures its Frame3 surface (task 4165 done; `frame3()` exists). Deferred until then — does **not** block the headline e2e.

---

## §6 — Cross-PRD relationship (G4)

| Other PRD / substrate | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `v0_3/rigid-body-dynamics.md` / task **4146** (closed-chain bridge) | consumes | the bridge reads offset-aware FK `world_transform` + `loop_residual_jacobian_by_joint`; **no bridge change** | **4146 landed**; this PRD is its kinematic prerequisite | done — bridge GREEN |
| `v0_6/geometric-joints.md` (companion, future) ↔ KIN-OFFSET-1 | **produces** (this PRD) / consumes (that PRD) | `relate` solves the joint **mount frame**; this PRD's `origin` field **is** where that mount frame is stored + threaded | **`geometric-joints.md` owns the `relate`↔offset seam**; KIN-OFFSET-1 owns the **representation + threading** | that PRD **depends on this one** (design §8.2 co-design) |
| `v0_6/geometric-relations.md` (core, committed `e8ce420d3b`) | adjacent | its §6 seam row + design §8.2 point here; promoting this PRD resolves the relate↔KIN-OFFSET-1 reciprocity | this PRD (κ correction task updates that prose) | geometric-relations core queued (α–θ) |
| `geometry-transforms-frames-projection.md` (P6) | consumes (δ only) | a full `frame3(origin, basis)` `Value::Frame` as a joint origin (oriented 3D linkages) | P6 owns the Frame3 `.ri` surface; this PRD's δ consumes it | 4165 done; δ deferred-until-P6 |

**Reciprocity resolution (the one real risk).** relate↔KIN-OFFSET-1 — each could claim "the other threads the offset." Resolved by clean scope split: **KIN-OFFSET-1 owns the offset field + its threading through FK/loop/dynamics (this PRD); geometric-joints owns *producing* the mount frame via `relate` and handing it to that field.** This is why 4331 is promoted **first** and geometric-joints depends on it. No new contested-ownership seam (checked against the overlay's known trio: persistent-naming/multi-kernel, imported-field/multi-kernel, topology-selectors/persistent-naming).

---

## §7 — Contract section (H) — the `transform_at` threading seam

The load-bearing seam is the offset's entry into `transform_at` and its inheritance by FK, the loop solver, and dynamics. An architect implementing α must honor:

### 7.1 Representation contract
- A joint `Value::Map` **MAY** carry an `"origin"` key whose value is a `Value::Transform` (SE(3) Frame3). **Absent ⇒ semantic identity.**
- The pivot authoring surface stores `origin = Transform { rotation: identity, translation: pivot }` from a `point3`/`vec3` argument. A malformed/non-finite pivot ⇒ the constructor returns `Value::Undef` (existing constructor discipline).

### 7.2 Threading contract (the single change point)
- `transform_at(joint, v)` **MUST** return `transform_compose(origin, motion)` where `motion` is the per-kind motion transform computed exactly as today and `origin` is read from the Map (identity if absent). Applied **once, uniformly, outside the per-kind match** — every `kind` arm inherits it; none re-implements it.
- **Invariant (single-primitive inheritance):** no consumer may reconstruct a per-joint transform by any path other than `transform_at`. `chain_transform`, `joint_world_transform`, `loop_residual_twist`, `loop_residual_jacobian_by_joint`, and the dynamics `X_{p→i}` transport (via the FK `world_transform`) all route through it and are therefore offset-aware with **zero** additional edits. γ asserts this (a bypass would be the only way A could leak).
- **Order:** `origin` is the constant transform from the parent joint's frame to this joint's mount; it is applied **before** the motion (`origin ∘ motion`), so the motion (rotation/translation) happens at the mounted frame, not the world origin.
- **Differentiation consistency:** because `loop_residual_jacobian_by_joint` is literally a central difference of `loop_residual_twist`, and both call `transform_at`, the Jacobian differentiates **exactly** the offset-aware residual the twist defines — the consistency is structural, not maintained. `origin` is held constant under perturbation (it is not a DOF).

### 7.3 Tolerance hierarchy (the coherence law)
`kernel_roundoff ≤ jacobian_FD_floor (O(eps²) ≈ 1e-14 at eps=1e-7) ≤ loop_Newton_convergence ≤ e2e virtual-work tolerance`. The four-bar e2e tolerance is set from the **measured loop-Newton convergence residual** (decision 4), dominating the chain. The FD-Jacobian-vs-twist consistency test (γ) asserts to the eps² floor.

### 7.4 Backward-compatibility contract
Every existing joint/FK/snapshot/loop/dynamics test builds joints **without** `origin`; α/γ assert those composed transforms are **byte-identical** to pre-change (the no-op invariance gate). Any divergence on the absent-origin path is a bug, not an accepted semantics change.

---

## §8 — Boundary-test sketch (H) — facing the kinematic core, the `.ri` author, and the dynamics consumer

| # | Scenario | Preconditions | Postconditions (assert) |
|---|---|---|---|
| B1 | `transform_at` offset compose | revolute joint with `origin = translate(L)`, angle θ | result == `translate(L) ∘ rot(z, θ)`; translation places the local origin at L then rotates about the mounted axis |
| B2 | absent-origin no-op (back-compat) | every existing joints/snapshot test (no `origin` key) | composed transforms **byte-identical** to pre-change; full suite green |
| B3 | FK world position | 2-link revolute chain with pivot offsets, hand-computed pivot positions | `walk_fk` `world_transform` translations match the hand-computed link-tip positions within roundoff |
| B4 | loop residual is genuinely nonzero | revolute 4-bar with Grashof pivot offsets, off-closure config | `loop_residual_twist` translational components **≠ 0** and vary with joint angle (the gap §0 proves is impossible today) |
| B5 | twist ↔ Jacobian consistency | offset-bearing chain | `loop_residual_jacobian_by_joint` columns == manual central-difference of the **offset-aware** `loop_residual_twist` within the eps² floor |
| B6 | live-constraint rank | revolute 4-bar at θ=45° | `reduce_constraint_rank` yields **`m_eff ≥ 1`** (the live constraint the 2-prismatic `m_eff=0` e2e never reached) |
| B7 | virtual-work identity (the consumer signal) | revolute 4-bar, `θ_input=45°`, `ω=2π`, analytic loop analysis | `Σ τ_i·q̇_i == analytic dE/dt` within the **derived** tolerance (decision 4); `reify eval` prints finite torques |
| B8 | no-bypass invariant | the dynamics + loop modules | a grep/structural assertion that no per-joint transform is reconstructed outside `transform_at` (the single-primitive guarantee) |
| B9 | malformed pivot | `revolute(axis, range, pivot: <non-finite>)` | constructor returns `Undef` (no half-built joint) |

The integration-gate leaf **β** names **B6 + B7** as its observable signal (the live-constraint four-bar e2e); **γ** names **B2 + B5 + B8** (the consistency + back-compat hardening). These face both the producer (the threaded `transform_at` + the loop/dynamics walks) and the consumer (`reify eval` + the analytic identity).

---

## §9 — Decomposition plan (α–κ; G2 signal per task)

Greek labels here; task IDs assigned at decompose time. **Phase 1 = representation + threading** (α, the single load-bearing change). **Phase 2 = vertical slice** (β — the four-bar e2e, the consumer signal). **Phase 3 = hardening** (γ). Plus the **companion correction** (κ) and the **P6-gated follow-up** (δ).

- **α — Joint `origin` Frame3 field + uniform `transform_at` pre-compose + translation-pivot authoring.** Modules: `reify-stdlib/src/joints.rs` (`transform_at` dispatch `:173`, `make_joint`/revolute/prismatic constructors). Mechanism: optional `"origin"` `Value::Transform` on the joint Map; `transform_at` returns `transform_compose(origin, motion)` uniformly (absent = identity); revolute/prismatic gain an optional `pivot:` arg (`point3`/`vec3` → pure-translation Transform). *Signal (intermediate → β, γ, δ, κ):* a committed `.ri` fixture + Rust unit test show `reify eval` over a revolute joint with a nonzero `pivot:` prints a `transform_at` whose translation reflects the pivot, and `transform_at(joint_with_origin, v) == origin ∘ motion`; **the full existing joints/snapshot/loop/dynamics suite stays green** (B2 no-op invariance). *Prereqs:* none. `grammar_confirmed=true`.

- **β — Revolute four-bar live-constraint (`m_eff ≥ 1`) closed-chain inverse-dynamics e2e.** *The vertical slice / consumer signal — the e2e 4146 could not build.* Modules: `examples/dynamics/closed_4bar_idyn.ri` (NEW), `crates/reify-eval/tests/closed_chain_idyn_e2e.rs` (extend, sibling of the 2-prismatic case), reusing the landed 4146 bridge. Mechanism: a planar revolute 4-bar (Grashof link lengths as pivot offsets) at `θ_input=45°`, `ω=2π`; the offset-aware loop residual is genuinely nonzero/config-dependent (B4), `reduce_constraint_rank` → `m_eff ≥ 1` (B6), KKT + energy ledger reproduce the analytic virtual-work torque (B7). *Signal (leaf):* `reify eval examples/dynamics/closed_4bar_idyn.ri` prints finite torques satisfying `Σ τ·q̇ = dE/dt` within the **derived** tolerance; the companion Rust e2e passes; the loop closes at a consistent nonzero-residual config. *Prereqs:* α; out-of-batch **4146 (done)**. **G6:** derive the analytic 4-bar position+velocity loop analysis; set tolerance from the measured loop-Newton convergence floor (decision 4) — no guessed literal.

- **γ — Offset-aware consistency proof + exhaustive FK/loop/snapshot/dynamics re-validation.** Modules: tests across `reify-stdlib/src/{joints.rs, loop_closure.rs, snapshot.rs}` + `dynamics/eval.rs`. Mechanism: the twist↔Jacobian consistency test on an offset-bearing chain (B5); the no-bypass invariant assertion (B8 — no consumer reconstructs a per-joint transform outside `transform_at`); offset-bearing variants of the key `joint_world_transform`/`walk_fk`/snapshot/loop tests (B3); the absent-origin byte-identical gate (B2). *Signal (leaf):* the FD-Jacobian matches the manual offset-aware twist gradient within the eps² floor; offset-bearing FK world positions match hand-computed link tips; the full suite green. *Prereqs:* α. (Parallel to β; both depend on α.)

- **κ — Companion correction: point geometric-relations + geometric-joints prose at the resolved offset contract.** *Required by B+H (cross-PRD prose this resolution touches).* Modules (docs): `docs/design/geometric-relations.md` §8.2, `docs/prds/v0_6/geometric-relations.md` §6 seam row, and a forward-pointer stub for `geometric-joints.md`. Mechanism: update the prose from "4331 to be promoted (stub)" to cite the **concrete** offset-representation contract (the `origin` Frame3 field + §7 threading) that `relate`'s solved mount frame populates; record that KIN-OFFSET-1 owns the field+threading and geometric-joints owns producing the mount frame. *Signal (leaf):* prose-diff committed; the geometric-relations seam table cites this PRD's §7 contract by section. *Prereqs:* α (so the field/contract name is settled).

- **δ — First-class oriented Frame3 `.ri` origin authoring (3D linkages).** *The P6-gated follow-up the user requested be queued.* Modules: `reify-stdlib/src/joints.rs` constructors (accept a full `frame3(origin, basis)` `Value::Frame` as a joint origin — orientation, not just translation), a NEW oriented-linkage `.ri` example (e.g. a spatial RSSR / bevel-axis joint). Mechanism: lift a `Value::Frame` into the `origin` `Value::Transform`; the planar pivot form stays the back-compat path. *Signal (leaf):* a `.ri` example authors a joint origin with a non-identity orientation via `frame3(...)` and builds; the spatial linkage's FK matches hand-computed poses. *Prereqs:* α; **out-of-batch: geometry-transforms-frames PRD** (Frame3 surface; 4165 done). **Deferred until the P6 Frame3 surface matures** (per the user's "after that lands"). `grammar_confirmed=true` (`frame3()` parses).

**DAG:** α → {β, γ, κ, δ}; β additionally consumes γ's invariants (shares α). Out-of-batch: β ← 4146 (done); δ ← geometry-transforms-frames PRD. **No grammar producer task** (the authoring surface parses today). The core (α/β/γ/κ) is dispatchable immediately; δ flips to `pending` but the scheduler holds it on the P6 dep (blocked-vs-pending semantics).

---

## §10 — Out of scope for this PRD

- **Oriented Frame3 `.ri` authoring** beyond the translation pivot → **δ**, gated on the geometry-transforms-frames PRD (P6).
- **The `relate` front-end** that *produces* mount frames → `geometric-joints.md` (consumes this PRD's `origin` field; design §8.2). The self-checking law, `joint … with`, couplings-on-the-scalar-side all live there.
- **Geometry-in-the-loop solving** (a relation whose datum depends on the pose it constrains) — `E_EVAL_UNRESOLVED`; a future PRD (geometric-relations design §12).
- **Closed kinematic loops with mobility via the geometric (SolveSpace) solver** — owned by the existing loop-closure Newton solver (design §8.3). This PRD only makes the *offset* expressible so Newton has real link geometry to close.
- **A `pivot`/origin field on multi-DOF kinds beyond uniform threading** — the uniform pre-compose covers all kinds; bespoke per-kind offset semantics are not introduced.

---

## §11 — Open questions (tactical — surfaced, not blocking)

1. **Standard pivot-authoring spelling.** `revolute(axis, range, pivot: point3(...))` vs a positional 3rd arg vs a dedicated `mount(joint, point)` combinator. *Decide during α* (the contract is the lifted Frame3 regardless of spelling).
2. **Four-bar loading for a non-trivial ledger.** A planar 4-bar at constant `ω` already has a non-trivial `dE/dt` from inertial coupling (coupler/rocker rates vary with config); whether to *also* load gravity (as the 2-prismatic did) for a richer ledger. *Decide during β's analytic derivation.*
3. **`origin` key naming.** `"origin"` vs `"mount"` vs `"frame"` on the joint Map (must not collide with existing keys / coupling's `offset`). *Decide during α.*
4. **Whether γ's no-bypass invariant is a test or a lint.** A one-off structural test vs a reusable assertion. *Decide during γ.*

---

## §12 — Notes for decompose mode

- Flip tracker **4331** `deferred → pending` and file α/β/γ/κ/δ with `planning_mode=True`; wire intra-batch deps (α→β,γ,κ,δ) + out-of-batch (β←4146 done; δ←geometry-transforms-frames PRD, e.g. 4165) while `deferred`; flip the whole batch to `pending` in one bulk call. The scheduler runs α/β/γ/κ now and holds δ on its P6 dep.
- Map 4331 itself onto the **β** leaf (the live-constraint e2e it was filed to unlock), or keep 4331 as the umbrella and file α–κ as its decomposition — decide at decompose time (the tracker's `human_decomposed=true` metadata expects a batch).
- Build the **capability manifest** beside this PRD (`kinematic-inter-joint-offsets.capability-manifest.md`): grammar-fixture binding for α's authoring `.ri` (parses, 0 ERROR — `grammar_confirmed=true`, no grammar producer); wired-on-main binding for the threading (transform_at is in the live FK/loop/dynamics walks — anti-orphan PASS); field-population N/A (no new result-field sampling — the e2e reads existing `forces`); **numeric-floor binding for β** — the virtual-work tolerance must bind to the *measured* loop-Newton convergence floor (decision 4 / G6), not a literal; any FAIL blocks the batch.
