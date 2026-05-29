# Proposed edits — FEA-accuracy achievability survey (2026-05-29)

Companion to `fea-accuracy-achievability-survey-2026-05-29.md`. **STATUS: APPLIED
2026-05-29** — the follow-up task was filed as **4066** (pending, gated on 3819), and every
`<MODAL-P2-FOLLOWUP>` placeholder below now corresponds to `task 4066`. Draft text retained
as the change record. Original apply order was: file the new follow-up task (A) first to get
its real ID, then substitute that ID for every `<MODAL-P2-FOLLOWUP>` placeholder in B–D.

Fix shape mirrors the two prior resolutions: relax the owning bound to the
formulation-honest figure + move the aspirational target to a more-capable follow-up
(buckling→4052 P2-tet K_g; shells→4065 ANS membrane; modal→**new**, below).

> **Honest-bound number:** the relaxed bounds below use **10%** as a concrete placeholder
> (mirroring buckling's relaxation). The implementer of 3819 should *measure* the actual
> P1-tet `f₁` error on the 200×10×2mm fixture during impl and tighten the wording to the
> measured floor (likely 8–15%). 10% is the safe initial relaxation, not a claim of the
> exact floor.

---

## A. NEW follow-up task (file first)

```
title: P2-tet modal frequencies — close the bending-lock gap to 2% on slender beams
priority: medium
dependencies: [3819]      # needs the modal_analysis pipeline; reuses P1 consistent-mass
                          # precedent 3818 (done) and P2 stiffness assembly 2915 (done)
project_root: /home/leo/src/reify

description: |
  Carry the aspirational 2% modal-frequency accuracy target that was relaxed off task
  3819 (modal-analysis.md §CI-gates / §9.1) because P1-tet bending lock cannot reach 2%
  on the slender Euler-Bernoulli validation beam (L=200mm, h=2mm → L/r≈346 about the
  weak axis; f∝√K so the ~6.8–9% P1 eigenvalue floor maps to a several-percent frequency
  floor, worse here than the buckling column because this beam is ~2.5× more slender).
  This is the modal analog of buckling task 4052 (P2-tet K_g) — same lever, quadratic
  shape functions that resolve bending curvature.

  Scope:
  - Add **tet10 (P2) consistent-mass assembly** to reify-solver-elastic, parallel to the
    tet4 consistent mass shipped by 3818 (which is P1-only). Unit test: total mass of a
    uniform-density block = ρV within 1e-12; M symmetric PSD (same invariants as 3818).
  - Plumb `element_order = P2` through the modal pipeline (`modal_analysis` → (K,M)
    assembly → shift-invert Lanczos) so the (K, M) pair is assembled at P2 when requested.
    P2 stiffness already exists (2915/2916); this wires P2 *mass* + the order switch.
  - Re-author the two modal example fixtures to solve at P2 with an example-practical
    mesh and assert the tight bound.

  User-observable signal (leaf): `examples/modal/cantilever_beam_modes.ri` and
  `simply_supported_beam_modes.ri`, run at `element_order = P2` via `reify eval`, hit
  first-mode (cantilever) / first-three-modes (simply-supported) frequencies **within 2%**
  of the analytic Euler-Bernoulli / (nπ)² values at example-practical mesh.

  Escape hatch (state in the RED test header, mirroring euler_column_pin_pin.rs): if P2 at
  example-practical runtime still cannot clear 2% on this L/r≈346 beam ("even P2
  underperforms shells by 2–5× on thin features" — structural-analysis-shells.md:18), pin
  the bound to the measured P2-honest floor (~3–5%) and bookmark a dedicated beam/frame
  element path as the only route to a true 2% — reify-solver-elastic currently has no
  lock-free 1-D element. Do NOT silently keep an ungreenable 2%.

  Consumer: the modal validation suite (modal_analysis_e2e); downstream resonance-budget
  dogfood (printer_gantry_modes.ri) benefits from the tighter frequencies.
  Crates touched: reify-solver-elastic (P2 consistent mass), reify-eval (modal_ops
  element_order plumbing), examples/modal/*.ri.

  Provenance: split out of 3819 per the 2026-05-29 FEA-accuracy achievability survey
  (docs/architecture-audit/fea-accuracy-achievability-survey-2026-05-29.md). 2% lives
  HERE; 3819 ships the P1-honest bound.
```

---

## B. Task 3819 — relax to P1-honest, move 2% to the follow-up

### B1 — `docs/prds/v0_3/modal-analysis.md` lines 79-82

```diff
-  - `examples/modal/cantilever_beam_modes.ri` — uniform steel cantilever
-    L=200mm, b=10mm, h=2mm. First-mode frequency matches the analytic
-    Euler-Bernoulli value `(1.875² / 2π) · √(EI / ρAL⁴)` within 2%
-    (mesh-density permitting).
+  - `examples/modal/cantilever_beam_modes.ri` — uniform steel cantilever
+    L=200mm, b=10mm, h=2mm. First-mode frequency matches the analytic
+    Euler-Bernoulli value `(1.875² / 2π) · √(EI / ρAL⁴)` within 10%
+    (P1-tet bending-lock floor — this beam is L/r≈346 about the h=2mm
+    weak axis and f∝√K, so it cannot reach 2% at P1 at any CI-practical
+    mesh; the original 2% is aspirational, gated on the P2-tet modal
+    follow-up <MODAL-P2-FOLLOWUP> — see §9.1 and the 2026-05-29
+    achievability survey).
```

### B2 — `docs/prds/v0_3/modal-analysis.md` lines 83-85

```diff
-  - `examples/modal/simply_supported_beam_modes.ri` — same beam, both
-    ends pinned. First three frequencies match `(nπ)² / 2π · √(EI / ρAL⁴)`
-    within 2%.
+  - `examples/modal/simply_supported_beam_modes.ri` — same beam, both
+    ends pinned. First three frequencies match `(nπ)² / 2π · √(EI / ρAL⁴)`
+    within 10% (same P1-tet bending-lock floor as the cantilever, and
+    higher modes lock harder; the original 2% is aspirational, gated on
+    the P2-tet modal follow-up <MODAL-P2-FOLLOWUP>).
```

### B3 — `docs/prds/v0_3/modal-analysis.md` line 470 (§9.1 acceptance matrix)

```diff
-| **Cantilever first-mode ground truth.** Uniform steel beam L=200mm, b=10mm, h=2mm. | δ, ε, ζ wired. | `modes[0].frequency` matches `(1.875²/2π)·√(EI/ρAL⁴)` ≈ 41.0 Hz within 2%. |
+| **Cantilever first-mode ground truth.** Uniform steel beam L=200mm, b=10mm, h=2mm (L/r≈346, weak-axis bending). | δ, ε, ζ wired. | `modes[0].frequency` matches `(1.875²/2π)·√(EI/ρAL⁴)` ≈ 41.0 Hz within 10% (P1-tet bending-lock floor; original 2% aspirational → P2-tet modal follow-up <MODAL-P2-FOLLOWUP>). |
```

### B4 — `docs/prds/v0_3/modal-analysis.md` line 471

```diff
-| **Simply-supported beam first three modes.** Same beam pinned at both ends. | δ, ε, ζ wired. | First three frequencies match `(nπ)²/2π·√(EI/ρAL⁴)` within 2%. |
+| **Simply-supported beam first three modes.** Same beam pinned at both ends. | δ, ε, ζ wired. | First three frequencies match `(nπ)²/2π·√(EI/ρAL⁴)` within 10% (P1-tet bending-lock floor; original 2% aspirational → P2-tet modal follow-up <MODAL-P2-FOLLOWUP>). |
```

### B5 (optional, recommended) — `modal-analysis.md` line 473, latent static-settle trap

The step-force row implies the transient settles to the analytic `F·L³/(3EI)`, which the
P1-tet static solve under-predicts by the same bending-lock margin. Compare to the FEA
static result, not the analytic value:

```diff
-| **Step-force impulse response.** Cantilever tip force step of 10N. | η, ι, θ wired. | Tip displacement settles to static value `F·L³/(3EI)` for t → ∞; ringing frequency matches mode 1; decay matches Rayleigh ζ_1. |
+| **Step-force impulse response.** Cantilever tip force step of 10N. | η, ι, θ wired. | Tip displacement settles to the **FEA static solution** for t → ∞ (which itself under-predicts the analytic `F·L³/(3EI)` by the P1-tet bending-lock margin on this slender beam — compare to the static FEA result, not the closed form, unless on <MODAL-P2-FOLLOWUP>); ringing frequency matches mode 1; decay matches Rayleigh ζ_1. |
```

### B6 — Task 3819 description (replace the signal sentence)

```diff
-User-observable signal: examples/modal/cantilever_beam_modes.ri and examples/modal/simply_supported_beam_modes.ri run end-to-end via `reify eval`; first-mode frequencies within 2% of the analytic Euler-Bernoulli / (nπ)² values.
+User-observable signal: examples/modal/cantilever_beam_modes.ri and examples/modal/simply_supported_beam_modes.ri run end-to-end via `reify eval`; first-mode frequencies within 10% of the analytic Euler-Bernoulli / (nπ)² values (P1-tet bending-lock floor on the L/r≈346 fixture — measure and pin the exact floor during impl; the original 2% is aspirational, moved to the P2-tet modal follow-up <MODAL-P2-FOLLOWUP> per the 2026-05-29 FEA-accuracy achievability survey).
```

---

## C. Task 2928 — pin the cantilever aspect ratio (keeps P1 5% reachable)

### C1 — `docs/prds/v0_3/structural-analysis-fea.md` line 196

```diff
-20. Validation suite against analytical references: cantilever beam (tip deflection), pressurised thick-walled cylinder (radial stress profile), simple shear (uniform stress), Boussinesq half-space point load. Tolerance comparisons per case at both P1 and P2.
+20. Validation suite against analytical references: cantilever beam (tip deflection), pressurised thick-walled cylinder (radial stress profile), simple shear (uniform stress), Boussinesq half-space point load. Tolerance comparisons per case at both P1 and P2. **Pin the cantilever to a moderate aspect ratio (L/h ≤ ~8)** so the P1 5% bound stays inside the bending-lock floor and the Timoshenko shear term stays meaningful; a slender cantilever (L/h ≳ 20) cannot reach 5% at P1 (~6.8% floor) and would need the P2-tet path — see the 2026-05-29 FEA-accuracy achievability survey.
```

### C2 — Task 2928 details, the **Cantilever beam (Timoshenko)** block

```diff
 **Cantilever beam (Timoshenko):**
-- Geometry: rectangular bar, length L, cross-section h×b
+- Geometry: rectangular bar, length L, cross-section h×b — **pin a moderate
+  aspect ratio L/h ≤ ~8** (keeps the FL/(GA·k_s) shear term meaningful AND the
+  P1 5% bound inside the bending-lock floor). A slender cantilever (L/h ≳ 20)
+  bending-locks at P1 (~6.8% floor) and cannot reach 5% — see the 2026-05-29
+  FEA-accuracy achievability survey.
 - BC: fixed at one end, point load F at free end
 - Reference: tip deflection δ = FL³/(3EI) + FL/(GA·k_s) (with shear correction)
-- Assert FEA result within 5% (P1) / 1% (P2)
+- Assert FEA result within 5% (P1) / 1% (P2) — at the pinned moderate aspect
+  ratio. If a slender fixture is needed for another reason, loosen the P1 bound
+  to ~10% (P1-tet floor) and keep 5% as P2-only.
```

> The other three 2928 cases need no change: cylinder (smooth field), simple shear (P1
> exact for constant strain), Boussinesq (10% near a known singularity) are all achievable.

---

## D. Task 3780 — keep the anisotropic-beam band P1-honest

### D1 — `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md` line 134

```diff
-| Homogeneous orthotropic solve | single `OrthotropicMaterial`, cantilever | tip deflection matches anisotropic-beam reference within band |
+| Homogeneous orthotropic solve | single `OrthotropicMaterial`, cantilever (moderate aspect ratio L/h ≤ ~8) | tip deflection matches anisotropic-beam reference within band — **keep the band P1-tet-honest: the orthotropic D is exact, but a slender orthotropic cantilever still rides the P1 bending-lock floor (~6.8%); do not tighten below the P1 floor unless on the P2-tet path. See the 2026-05-29 achievability survey.** |
```

### D2 — Task 3780 description (replace the signal sentence)

```diff
-User-observable signal: a homogeneous orthotropic solve (single `OrthotropicMaterial`, cantilever) returns a non-trivial `ElasticResult` whose tip deflection matches the anisotropic-beam reference within band.
+User-observable signal: a homogeneous orthotropic solve (single `OrthotropicMaterial`, cantilever at moderate aspect ratio L/h ≤ ~8) returns a non-trivial `ElasticResult` whose tip deflection matches the anisotropic-beam reference within a P1-tet-honest band (the orthotropic D is exact; do not tighten the band below the P1 bending-lock floor ~6.8% on a slender fixture — see the 2026-05-29 FEA-accuracy achievability survey).
```

---

## Apply checklist (for when approved)

1. File task A → record real ID `N`.
2. `sed`/Edit `<MODAL-P2-FOLLOWUP>` → `N` (or `task N` / `esc-…` per local convention) in B1–B6.
3. Apply B1–B4 (+ B5 optional) to `modal-analysis.md`; B6 to task 3819 via `update_task`.
4. Apply C1 to `structural-analysis-fea.md`; C2 to task 2928.
5. Apply D1 to `anisotropic-heterogeneous-elastostatics.md`; D2 to task 3780.
6. 3454: no edit — confirm the in-progress branch's smoke example tests against the
   already-correct 10% PRD signal (line 503).
