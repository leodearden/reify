# Capability manifest — kinematic-inter-joint-offsets (KIN-OFFSET-1)

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/kinematic-inter-joint-offsets.md`. Each capability a task's signal asserts is bound to evidence; any binding resolving to `declared-only | test-only | producer-downstream | producer-absent | fixture-ERROR | bound≤floor` blocks the batch. Verified 2026-06-08.

## Summary

| Leaf | Binding class | Verdict |
|---|---|---|
| α | wired-on-main (transform_at is the live FK/loop/dynamics primitive) + grammar-fixture | **PASS** |
| α | back-compat (absent origin = identity = no-op) | **PASS** (existing suite is the gate) |
| β | wired-on-main (4146 bridge done) | **PASS** |
| β | **numeric-floor (virtual-work tolerance)** | **DERIVE-AND-BIND** — must bind to the *measured* loop-Newton residual; a frozen literal = FAIL |
| γ | wired-on-main (twist↔Jacobian structural) + no-bypass invariant | **PASS** |
| κ | prose (no substrate capability) | **N/A** |
| δ | producer-exists (`frame3()` landed) + grammar-fixture | **PASS** (held by 4382, not by a missing producer) |

No FAIL bindings. The single binding to enforce at dispatch is β's numeric-floor (the G6 discipline that burned task 4146's predecessors).

## α — Joint `origin` field + `transform_at` threading + pivot authoring

- **Capability:** a joint `Value::Map` carries an optional `origin` `Value::Transform`; `transform_at` returns `transform_compose(origin, motion)` uniformly across all kinds.
  - **Evidence (wired-on-main / anti-orphan):** `transform_at` is the production primitive in the live FK/loop/dynamics walks — `grep` `reify-stdlib/src/joints.rs:173` (dispatch), consumed by `loop_closure.rs:92` (`chain_transform`), `snapshot.rs:912` (`joint_world_transform`), `loop_closure.rs:863` (the Jacobian), and `dynamics/eval.rs:451/537/1091` (FK `world_transform`). The `origin` field is read on the production path, **not** a `tests/` helper. **PASS.**
- **Capability:** the pivot authoring surface parses.
  - **Evidence (grammar-fixture):** `tree-sitter parse --quiet` exit **0** (0 ERROR) on `revolute(axis, range, pivot: point3(...))`, the positional form, `origin: <frame>`, and `frame3(point3(...), orient_identity)`. `grammar_confirmed=true`; **no grammar producer task.** α commits the fixture into the example/`tree-sitter-reify/tests/`. **PASS.**
- **Capability:** absent `origin` ⇒ byte-identical to today.
  - **Evidence (back-compat):** the entire existing joints/snapshot/loop/dynamics suite builds joints without `origin`; α/γ assert no composed-transform change. The existing suite **is** the gate. **PASS** (a divergence on the absent path is a bug, not an accepted semantics change).

## β — Revolute four-bar live-constraint (`m_eff ≥ 1`) virtual-work e2e

- **Capability:** the closed-chain bridge reproduces the analytic four-bar input torque.
  - **Evidence (wired-on-main):** the Value→closed_chain bridge (M assembly, incidence map, `reduce_constraint_rank`, KKT) is landed `done` at `316c0f9229` (task 4146); it reads offset-aware FK `world_transform` + `loop_residual_jacobian_by_joint`. **No bridge change.** **PASS.**
- **Capability (G6):** `Σ τ_i·q̇_i == analytic dE/dt` within the e2e tolerance.
  - **Binding (numeric-floor — DERIVE-AND-BIND):** work-energy is exact to roundoff; the real floor is the **loop-closure Newton convergence residual** the position/velocity solve attains (the `1e-7` Jacobian eps / `1e-10` rank-reduction regime). The e2e tolerance **MUST** be set a safety factor above the *measured* solver residual after deriving the analytic 4-bar position+velocity loop solution closed-form. **A literal copied from the 2-prismatic `1µW` case is a FAIL** (overlay G6 precedents `esc-3821-44`, `esc-3453` — guessed bounds below the real floor froze false RED tests). Enforced at β dispatch.
- **Capability:** the offset-aware loop residual is genuinely nonzero (`m_eff ≥ 1`).
  - **Evidence (producer-upstream):** producer = α (the `transform_at` threading makes the residual config-dependent); β consumes via the `α→β` dep. The gap §0 proves this is impossible without α. **PASS** (wired).
- **field-population:** N/A — β reads the existing `forces` cell; no new result-field sampling.

## γ — Twist↔Jacobian consistency + re-validation

- **Capability:** `loop_residual_jacobian_by_joint` differentiates the *same* offset-aware `loop_residual_twist`.
  - **Evidence (structural):** the Jacobian **is** the central difference of the twist — `grep` `loop_closure.rs:863` (`loop_residual_twist(... va_plus ...)`); both call `transform_at`. Consistency is by construction; γ proves it to the eps² floor. **PASS.**
- **Capability:** no consumer reconstructs a per-joint transform outside `transform_at` (the single-primitive guarantee).
  - **Evidence (no-bypass):** the four consumers all route through `transform_at` (`chain_transform:92`, `joint_world_transform:912`, the Jacobian:863, dynamics FK read). γ adds the structural assertion. **PASS.**

## κ — Cross-PRD prose correction

- **N/A** — the deliverable is a committed prose diff (geometric-relations design §8.2 + PRD §6 seam row pointing at this PRD's §7 contract). No substrate capability to bind.

## δ — Oriented Frame3 `.ri` origin authoring (P6-gated follow-up)

- **Capability:** a joint origin accepts a full `frame3(origin, basis)` `Value::Frame` (orientation, not just translation).
  - **Evidence (producer-exists):** `frame3()` produces a real `Value::Frame` today — `grep` `geometry.rs:250`, task 4165 (`project(point, Frame)`) `done`. The *construction* surface is landed; δ's core is technically buildable now. **PASS.**
  - **Grammar-fixture:** `origin: frame3(...)` parses (fixture exit 0). **PASS.**
  - **Gate (not a FAIL):** δ is **held** on geometric-relations β (**task 4382**, in-progress — first-class Frame member access / `Direction` projections, the "first-class Frame3 `.ri` surface" the user asked δ to land alongside). This is a *sequencing* gate (clean oriented-authoring experience), not a missing producer. Re-anchor if Leo means a different Frame3-surface milestone.
