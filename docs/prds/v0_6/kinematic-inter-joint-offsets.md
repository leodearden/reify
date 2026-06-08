# PRD (forward stub / DEFERRED): kinematic fixed inter-joint (link-length) offsets

**Status:** DEFERRED forward-design stub · **Author session:** 2026-06-03 · **Milestone:** v0_6+
**Relationship:** the kinematic-substrate prerequisite that the closed-chain inverse-dynamics bridge
(`task 4146`, descoped from RBD-η `3836`) discovered it needs but could not build. The 4146 bridge
itself (M assembly, constraint-Jacobian, rank reduction, KKT routing) is **landed and GREEN**; only
its *live-constraint* analytic e2e (a real spatial linkage) is blocked on the gap below. Root cause
proven in **esc-4146-280**. Held by deferred tracker task **KIN-OFFSET-1** (filed `deferred`, not
activated).

> This is a **stub**, not a ready-to-decompose PRD. It records scope, the substrate gap, and the
> known hazards so the excluded work is tracked rather than dropped. It must be promoted to a full
> B+H PRD (re-walk all gates, verify each kinematic change against the FK + loop-solver + dynamics
> consumers, set numeric tolerances) before the tracker is activated. Do **not** decompose it as-is.
>
> Tracker: **KIN-OFFSET-1 = task 4331** (filed `deferred`, `source: unblock-triage`).

---

## §0 — Why this is deferred (the substrate gap, code-grounded)

The Reify kinematic FK / loop layer has **no representation for fixed inter-joint (link-length)
spatial offsets**. A joint stores only an `axis` + a motion `range`; there is no origin / offset /
frame field, and the per-DOF transform a joint contributes is the motion transform *alone*:

| Anchor | Behavior |
|---|---|
| `crates/reify-stdlib/src/joints.rs:1519` (`transform_at_simple_joint`, revolute arm) | **pure rotation, zero translation**. Prismatic arm = pure translation; fixed = identity. No constant link offset. |
| `crates/reify-stdlib/src/loop_closure.rs:77` (`chain_transform`) | folds a chain as `T_0 · T_1 · … · T_{n-1}` where each `T_i = transform_at(joint_i, value_i)`. Basis of `loop_residual_twist` (`:111`), used by **both** the snapshot kinematic loop solver **and** the dynamics bridge's constraint-Jacobian (`loop_residual_jacobian_by_joint`). |
| `crates/reify-stdlib/src/snapshot.rs:866` (`joint_world_transform`) | composes the same pure `transform_at` DOF transforms up the `joint_parents` chain. A child composes from its parent **joint**'s frame (`:890`), **not** the parent body's frame. |
| `crates/reify-stdlib/src/snapshot.rs:692,714` (`walk_fk`) | body `pose` is applied **only** as `body.world_transform = joint_world_transform ∘ pose` (`:714`). It is **never threaded into child joints' parent frames** — so a body's `pose` does not displace the joints mounted on it. |

**Consequence.** In a revolute-only chain every pivot is **coincident at the world origin**. A
"planar revolute 4-bar" therefore has a translational loop residual that is **identically zero for
all joint angles** — there is no link geometry, no spatial 4-bar motion, and thus no analytic
mechanical-energy rate `dE/dt` to reproduce. The orientation-only closure may numerically
"converge" (matching rotations) but is physically meaningless, and would be **inconsistent** with
the dynamics path, which reads body `world_transform`s that *do* include pose-derived COM positions
— so a virtual-work identity `Σ τ·q̇ = dE/dt` could not hold even if forced. Two full agent budgets
were burned on this inexpressible fixture before the root cause was proven (esc-4146-280).

---

## §1 — Consumer (G1, provisional)

A mechanism designer modeling a **real spatial linkage** — links with actual lengths between
pivots: a revolute **four-bar**, a slider-crank, a pantograph — where the closed loop has genuine
mobility (`m_eff ≥ 1`). Concretely this unlocks the one e2e **task 4146 could not build**:

- a **live-constraint** (`m_eff ≥ 1`) closed-chain inverse-dynamics e2e — a planar revolute 4-bar at
  `θ_input = 45°`, `ω = 2π rad/s`, reproducing the analytic 4-bar virtual-work torque end-to-end.
  4146's e2e was re-scoped (Leo decision **B1**) onto an *expressible* vertical 2-prismatic loop
  whose closing joint shares the residual axis → `m_eff = 0`, so it validates the bridge's gravity
  energy ledger but **not** the nonzero-constraint (λ, incidence-map, non-empty rank-reduction)
  path at the Value/e2e level. This PRD's feature is the prerequisite for that stronger e2e.

---

## §2 — Sketch (provisional, to be hardened on promotion)

Two candidate designs (pick one at promotion; **B is the lighter touch**):

1. **(A) Add an `origin` / offset frame to joints.** Give each joint a constant `Frame3`
   (translation + orientation) applied *before* its motion transform, so
   `transform_at(joint, v) = origin ∘ motion(axis, v)`. Threads through `chain_transform`,
   `joint_world_transform`, and `loop_residual_twist` uniformly. Requires a surface syntax for the
   per-joint offset (`Frame3` has no `.ri` surface type yet — see `MassProperties.origin` TODO and
   the geometry-transforms/frames PRD `reference_prd_geometry_transforms_frames_projection`).
2. **(B) Thread body `pose` into child-joint parent frames.** Make `joint_world_transform` compose a
   child joint from its parent **body**'s posed frame (`joint_world ∘ pose`) rather than the parent
   **joint**'s bare frame. Reuses the existing `pose` field (no new joint field / surface syntax);
   the link length is expressed as the mounting body's `pose` translation. Smaller surface delta but
   changes FK semantics — must re-validate every FK / loop-solver / snapshot test.

Either way the change is **load-bearing across three consumers** that must stay consistent:
FK (`walk_fk` / `joint_world_transform`), the kinematic loop solver (`loop_residual_twist`), and the
dynamics bridge's constraint-Jacobian (`loop_residual_jacobian_by_joint`, which central-differences
the same residual). A divergence between them silently produces wrong torques.

---

## §3 — Cross-PRD seam ownership (G4)

| Seam | Owner |
|---|---|
| `Value→closed_chain` dynamics bridge (M / A / rank reduction / KKT routing) | `task 4146` (**landed** — this PRD is its kinematic prerequisite, not a rewrite) |
| `Frame3` surface type / `.ri` syntax for a per-joint offset (design A) | geometry-transforms-frames PRD (`reference_prd_geometry_transforms_frames_projection`) — verify ownership at promotion |
| FK / loop-solver / residual-Jacobian consistency | **this PRD** (on promotion) |
| `relate`→mount frame/axis production: `relate` (docs/prds/v0_6/geometric-relations.md §6/§10, design §8.2) solves the mount frame/axis stored by the offset field (§2 design A origin frame / design B body pose); KIN-OFFSET-1 threads that frame through FK (`walk_fk` / `joint_world_transform`), the loop-residual, and the dynamics constraint-Jacobian | **`geometric-joints.md`** (companion; **not yet authored**) owns the relate↔offset co-design seam, **co-designed** with this PRD per design §8.2; hard-gated on KIN-OFFSET-1 (4331, `deferred`) — author `geometric-joints.md` after 4331 is promoted (stub → full B+H PRD) |

> **Forward-reference (reciprocal-risk seam):** `geometric-joints.md` does **not yet exist** — it is the designated future companion PRD for the joint half. The seam owner is unambiguous: `geometric-joints.md` owns the relate↔offset co-design, resolving the reciprocal-risk ("each side could claim 'the other threads the offset'") by requiring the two PRDs to be co-designed per design §8.2. Author `geometric-joints.md` after task 4331 is promoted from stub to a full B+H PRD. See also: docs/prds/v0_6/geometric-relations.md §6/§10 for the reciprocal record of this seam.

---

## §4 — Deferred tracker task

- **KIN-OFFSET-1** (task **4331**, `deferred`) — Kinematic fixed inter-joint (link-length) offsets: add a per-joint origin frame
  (design A) **or** thread body pose into child-joint parent frames (design B), keeping FK, the
  kinematic loop solver, and the dynamics constraint-Jacobian mutually consistent; then build the
  true revolute-4-bar live-constraint (`m_eff ≥ 1`) closed-chain inverse-dynamics e2e that 4146
  could not. `deferred`; B+H; references this PRD + esc-4146-280.

## §5 — Promotion checklist (before activating KIN-OFFSET-1)

- [ ] Choose design A (joint origin frame) vs B (thread body pose); prototype a revolute 4-bar and
      confirm a **nonzero** translational loop residual that closes at a consistent configuration.
- [ ] Re-validate **every** FK / `walk_fk` / `joint_world_transform` / loop-solver / snapshot test
      against the new frame semantics (the change is not additive — it alters composed transforms).
- [ ] Confirm `loop_residual_jacobian_by_joint` (dynamics bridge) and `loop_residual_twist`
      (kinematic solver) differentiate the **same** offset-aware residual — a divergence yields
      silently-wrong torques.
- [ ] Derive the analytic 4-bar position + velocity loop analysis and set an **honest** tolerance on
      the virtual-work identity (work-energy is exact to roundoff; the convergence tolerance is the
      real floor) — G6.
- [ ] Re-walk G1–G6 + META; write the capability manifest.
