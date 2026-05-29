# FEA-Accuracy Achievability Survey — 2026-05-29

**Author:** one-off achievability survey (Claude, interactive session)
**Scope:** All remaining FEA-accuracy numeric signals in the Reify project, screened
for the *formulation-vs-bound mismatch* failure mode.
**Status:** Findings only — **no tasks or PRDs were mutated.** Proposed fixes are for
Leo to approve.

---

## 1. The failure mode we are hunting

A RED-test (or PRD observable-signal) asserts a numeric accuracy/tolerance bound that
the **in-scope formulation of the owning task cannot physically reach** — because the
bound was borrowed from a more-capable formulation. The two cases that froze at L2 and
motivated this survey:

| esc | task | asserted bound | in-scope formulation | true floor | resolution |
|---|---|---|---|---|---|
| esc-3454-51 | 3454 (buckling smoke) | Euler load within **5%** | P1-tet K_g | ~6.8% asymptote / 9.2% CI-mesh | relaxed PRD line 503 → **10%**; 5% → P2-tet task **4052** (f1550d8bdc) |
| esc-3392-357 | 3392 (shells) | within **2×** of MacNeal-Harder @ 4×4 | flat/curved MITC3, **no ANS membrane** | 21–2200× under-prediction (membrane lock) | re-spec 3392 to formulation-honest signal; ~50% → new ANS task **4065**; 3513 re-gated on 4065 |

**Fix shape (the template for everything below):** relax the owning task's bound to the
formulation-honest figure, and move the aspirational target to the correct
more-capable follow-up task (creating one if it does not exist).

### Physics cheat-sheet used for every verdict

- **P1 (linear, constant-strain) tets bending-lock.** On slender members they
  *overestimate* bending stiffness → *underestimate* deflection by **30–50%** at default
  mesh (`structural-analysis-shells.md:17-18`). The error model is `a + b/n²`; for the
  buckling column (L/r≈138) the eigenvalue error floored at **~6.8% asymptote, ~9.2% at
  CI-practical mesh**, and 5% needed ~81K DOFs (`euler_column_pin_pin.rs:19-25`).
- **Eigenvalue → frequency.** Modal `f ∝ √(K)`, so a stiffness/eigenvalue error of `ε`
  shows up as roughly `ε/2` in frequency. A 6.8% eigenvalue floor ⇒ **~3.4% frequency
  floor** — already above a 2% target.
- **Locking severity scales with slenderness²** (shear-lock error ∝ (L/t)²). A beam that
  is k× more slender than a measured reference needs a far finer mesh for the same
  tolerance.
- **Flat-facet MITC3** cures transverse-shear lock but **not membrane lock**; cubic-bubble
  enrichment is provably inert on flat facets (`K_NB ≡ 0`, `shell_assembly.rs` header).
  Membrane lock scales **(R/t)²**; curing it needs **curved elements + ANS membrane**.
- **Exact / lock-free formulations** (no achievability risk): constant-strain fields
  (simple shear), pin-jointed bar/cable elements (axial trusses, tensegrity), closed-form
  PRB flexure stiffness, linear superposition, mass-normalization & rigid-body invariants,
  consistent-mass ρV integration. These are reported below for completeness but carry no
  formulation wall.

---

## 2. Headline result

| # | signal (task) | verdict | note |
|---|---|---|---|
| 1 | **Modal first-mode/first-3-modes within 2%** (task **3819**) | **UNACHIEVABLE** | P1-tet on L/r≈346 beam; same bending-lock mode as buckling, *more severe*. No follow-up task exists. |
| 2 | Cantilever tip-deflection **P1 within 5%** (task **2928**) | **UNCERTAIN (latent)** | Fixture aspect ratio unpinned; unreachable iff implementer picks a slender beam. |
| 3 | Anisotropic-beam tip deflection "within band" (task **3780**) | **UNCERTAIN (latent)** | Band unquantified; same risk only if later tightened on a slender orthotropic cantilever at P1. |

Everything else surveyed is **ACHIEVABLE** or **already resolved/correctly-gated**. The
3454 "live 5%" that a task-metadata scan suggested is a **false positive** — the PRD was
already fixed (see §5).

**Count of unachievable signals found: 1** (task 3819, covering both its beam-frequency
example bounds), **plus 2 latent UNCERTAIN risks** (2928, 3780) that should be pinned
before an implementer authors a tight P1 RED test.

---

## 3. UNACHIEVABLE — detailed finding

### 3.1 Task 3819 — Modal beam natural frequencies "within 2%"

**Owning task:** `3819` — *Modal ζ: `modal_analysis(part, opts) -> ModalResult` stdlib fn
+ eval dispatch* (status: **pending**, priority high, deps 3817 / 3818-done / 3882-done).

**Asserted bound (two example signals, both gating via `reify eval`):**

- `docs/prds/v0_3/modal-analysis.md:79-82` —
  > `examples/modal/cantilever_beam_modes.ri` — uniform steel cantilever … Euler-Bernoulli
  > value `(1.875²/2π)·√(EI/ρAL⁴)` **within 2%** (mesh-density permitting).
- `docs/prds/v0_3/modal-analysis.md:83-85` —
  > `simply_supported_beam_modes.ri` … first three frequencies … **within 2%**.
- Acceptance matrix pins the fixture — `modal-analysis.md:470`:
  > **Uniform steel beam L=200mm, b=10mm, h=2mm.** … `modes[0].frequency` matches
  > `(1.875²/2π)·√(EI/ρAL⁴)` **≈ 41.0 Hz within 2%**.
- Task 3819 description repeats: *"first-mode frequencies within 2% of the analytic
  Euler-Bernoulli / (nπ)² values."*

**In-scope formulation:** continuum FEA — consistent mass **tet4** (P1, task 3818-done) +
existing P1/P2 stiffness assembly, shift-invert Lanczos on the `(K, M)` pair. The example
`.ri` files do not set `element_order`, so the default is **P1 tet**
(`structural-analysis-fea.md:124`). reify-solver-elastic has **no beam/frame element** —
beam frequencies *must* come from a continuum tet mesh, which bending-locks.

**Why the bound is physically unreachable (by the in-scope P1-tet formulation at
verify-practical mesh):**

1. **The fixture is extremely slender.** First-mode bending is about the weak axis
   (thickness `h=2mm`): `r = h/√12 = 0.577mm`, so **L/r = 200/0.577 ≈ 346** (L/h = 100).
   This is **~2.5× more slender** than the buckling column (20×20×800mm, L/r≈138) that
   empirically floored at ~9%.
2. **Bending lock floor → frequency floor.** The directly-analogous buckling eigenvalue
   problem floored at ~6.8% asymptote / 9.2% at CI mesh and needed ~81K DOFs for 5%
   (`euler_column_pin_pin.rs:19-25`). Since `f ∝ √K`, the modal frequency floor is
   ~half that — but starting from a **worse** eigenvalue floor because the beam is more
   slender. Net practical floor on `f₁` is on the order of **3–5%+ at verify-practical
   mesh — above 2%.**
3. **Textbook coarse-mesh figure is far worse.** P1 tets underestimate thin-body
   deflection by 30–50% at default mesh (`structural-analysis-shells.md:17-18`) ⇒
   overestimate stiffness ⇒ overestimate `f₁` by ~20–40% before any refinement. The
   beam is well past the thin-body advisory threshold (aspect 100:1 ≫ 10× warning,
   `structural-analysis-fea.md:197` / task 2929).
4. **Higher modes are worse.** The simply-supported "first **three** modes within 2%"
   bound is *less* achievable than the first-mode bound: mode `n` frequency ∝ n² has more
   curvature, which P1 constant-strain tets capture even more poorly.
5. **"Mesh-density permitting" is the same escape hatch buckling had — and it failed
   there.** Modal frequencies do converge from above (Rayleigh quotient is an upper
   bound), so 2% is *asymptotically* reachable; but on an L/r≈346 beam the required mesh
   is well beyond what an `reify eval` verify gate can run, exactly as the buckling
   experience showed for a *less* slender geometry.

**Confidence:** High on physics; **not yet empirically measured** (3819 is pending, no
fixture exists). The evidence chain is the pinned fixture + the measured buckling analog +
the textbook thin-body figure + the `f∝√K` relationship. A definitive confirmation is a
single P1-tet eval of the 200×10×2mm fixture — recommended as the first step of the fix.

**Proposed fix (same shape as buckling esc-3454-51):**

1. **Empirically pin the floor first.** Build the `cantilever_beam_modes.ri` fixture, run
   it at P1 (default) and P2 at a verify-practical mesh, and record the measured `f₁`
   error. This converts "UNACHIEVABLE (predicted)" into a measured bound.
2. **Relax 3819's example bound to the P1-tet-honest figure** (expected ~10–15% at
   practical mesh; the precise number from step 1), with a comment citing the P1-tet
   bending-lock floor and this survey — mirroring `euler_column_pin_pin.rs:17-37`.
3. **Move the aspirational 2% to a follow-up task that does not yet exist.** Buckling has
   4052 (P2-tet K_g) and shells have 4065 (ANS membrane); **modal has no analog.** File a
   new follow-up — *"P2-tet (or beam-element) modal frequencies to 2% on slender beams"* —
   gated after 3819, and re-point the 2% target there. Keep 2% aspirational in the PRD
   acceptance matrix under that follow-up's citation.
4. **Edit the PRD signals** `modal-analysis.md:79-85` and the acceptance-matrix rows
   `:470-471` to the honest bound + follow-up citation, exactly as
   `buckling-eigensolver.md:503` now reads "within 10% … original 5% is aspirational,
   gated on … 4052."

> **Note on the other modal bounds (all ACHIEVABLE, no action):** decay-envelope within 5%
> (`:472`, linear modal superposition — exact), mass-normalization ΦᵀMΦ=I within 1e-12
> (`:474`, exact linear algebra), participation-mass within 1% / ≥99% (`:475`, capture
> enough modes), rigid-body ω≈0 within 1e-6 (`:476`). The bending-lock wall is *only* on
> the two continuum-beam-frequency example bounds.

---

## 4. UNCERTAIN / latent risks — pin before authoring

### 4.1 Task 2928 — cantilever tip-deflection "within 5% (P1)"

**Owning task:** `2928` — *FEA validation suite vs analytical references* (status:
**pending**, deps 2924 / 3092). Target file `tests/analytical_validation.rs` (does not
yet exist).

**Asserted bounds (from task details):**
- Cantilever (Timoshenko ref `δ = FL³/3EI + FL/(GA·k_s)`): **within 5% (P1) / 1% (P2)**.
- Thick-walled cylinder (Lamé), max von Mises: within 5% (P1) / 2% (P2).
- Simple shear, uniform stress: within 1% interior / 1% von Mises.
- Boussinesq half-space, subsurface stress: within 10% near load (singularity expected).

**Assessment:**
- **Cantilever P1 5% — UNCERTAIN (latent).** The PRD/task pin only *symbolic* geometry
  ("rectangular bar, length L, cross-section h×b"), **not an aspect ratio**. If the
  implementer picks a slender beam (L/h ≳ 20) to make the `FL³/3EI` term dominant, P1-tet
  bending lock floors above 5% — the same wall as buckling/modal. If the fixture is stubby
  (L/h ≲ 8–10, where the Timoshenko shear term is non-trivial and bending lock is mild),
  P1 5% is reachable with adequate mesh. **The bound is achievable *iff the fixture aspect
  ratio is constrained.***
- Cylinder / simple-shear / Boussinesq — **ACHIEVABLE.** Smooth axisymmetric field (no
  bending lock), constant-strain field (P1 exact), and a generous 10% near a known
  singularity, respectively.

**Proposed fix:** in the task spec, **pin a moderate-aspect-ratio cantilever (L/h ≤ ~8)**
for the P1 5% case, *or* document a P1-honest bound (~10%) if a slender beam is wanted —
and add a one-line note that the P1 cantilever bound is bending-lock-sensitive to aspect
ratio (cite this survey). No follow-up task needed if the fixture is constrained.

### 4.2 Task 3780 — anisotropic homogeneous-orthotropic "tip deflection … within band"

**Owning task:** `3780` — *Foundation δ: generalise material to ConstitutiveLaw|Field*
(status: **pending**). Bound is **unquantified** ("matches the anisotropic-beam reference
within band"). The orthotropic D-matrix itself is exact; the *only* risk is if the "band"
is later tightened to a few-percent P1 figure on a slender orthotropic cantilever — same
bending-lock wall. **Action:** when 3780's band is pinned, keep it loose for P1 (mirror
2928), or constrain the fixture aspect ratio. Low priority; currently soft.

---

## 5. False positive corrected — Task 3454

A task-metadata scan flagged 3454's CLI smoke as still asserting **5%** at
`buckling-eigensolver.md:503`. **This is stale.** Direct read of the live PRD:

```
503  matches the analytical Euler load within 10% (P1-tet bending-lock floor — see
504  §9.1; the original 5% is aspirational, gated on the P2-tet K_g follow-up
505  esc-3813-117). CLI evaluation confirms; ...
```

Line 503 already reads **10%** (commit f1550d8bdc, the esc-3454-51 fix in §1). The task's
own `dry_run` block-proposal that recommended the `5%→10%` edit predates the commit. **No
action** — this is resolved. (Worth a 30-second confirm that 3454's in-progress branch
authors `examples/buckling_column_smoke.ri` against the *10%* PRD signal, not a stale 5%
RED test, since the task is in-progress.)

---

## 6. Confirmed-correct / already-resolved (reference, no action)

| signal | task(s) | formulation | why correct |
|---|---|---|---|
| Euler column 10% / 11% / 9% (pin-pin/fixed-free/fixed-pin/fixed-guided) | 3453 (done), 3454 (in-prog) | P1-tet K_g (+MPC) | Bounds match measured P1 floor; tests `euler_column_pin_pin.rs`/`kg_p1_tet.rs` document the floor & the P2 path. |
| Euler column **5%** (aspirational) | **4052** (pending) | **P2-tet K_g** | Correct home for 5%; quadratic shape fns resolve half-sine curvature. ACHIEVABLE by formulation. |
| Shell smoke envelopes (sign/finite/order-of-mag) | 3392 (in-prog) | flat/curved MITC3, no ANS | `shell_benchmarks.rs:9-29` explicitly *not* validated benchmarks; bands bracket the 21–2200× locked output. 3392 re-spec'd to a "measurable improvement on twisted cantilever" signal; the ~50% bound was already excised. |
| MacNeal-Harder **~50% @ 4×4** (aspirational) | **4065** (pending) | curved MITC3 **+ ANS membrane** | Correct home; ANS is the textbook membrane-lock cure; ~50%/4×4 is the published reachable figure (Bathe & Lee 2014). |
| Scordelis-Lo tighten to ~50%/4×4 | **3513** (pending) | consumes 3392 **AND** 4065 | Correctly gated on *both* — "Do not attempt to GREEN the ~50% band before 4065 lands." Would be UNACHIEVABLE on 3392 alone (~21× membrane lock). |
| Legacy "real MacNeal-Harder via bubble enrichment" | 3325 (**cancelled**) | flat-facet bubble (K_NB≡0) | Correctly cancelled — premise mathematically inert; re-homed to 3392/3513/4065. |
| A-posteriori: plate-with-hole within 5% of SCF; L-shaped O(N^-1/3) vs O(N^-1/2) | 3002 (pending) | ZZ estimator + Dörfler adaptive refinement | ACHIEVABLE — adaptive h-refinement converges peak stress; rate-gap is standard. ("≥70% asymptotic optimal" gate self-flagged TBD/soft.) |
| Auto-resolve converged thickness within 5% (×2 PRDs) | a-posteriori T5, modal | design-loop tolerance | ACHIEVABLE — loop-convergence tolerance, not element accuracy. |
| Hex/wedge: agreement "within tolerance" + convergence steeper than tet | 2993 (pending) | P1 hex/wedge vs P1 tet | ACHIEVABLE — comparative/relative claim; hex genuinely out-converges tet per DOF on swept geometry. (Watch: if "within tolerance" is later pinned tight at P1 on a slender swept cantilever, inherits the 2928 caveat.) |
| Multi-load-case superposition within `Σ|w|·cg_tol·C` | 3015 (pending) | linear superposition | ACHIEVABLE — exact for linear elasticity; bound is solver-tolerance-derived. |
| Flexure k_θ within 1% (Howell) / 2% (Paros-Weisbord); mass-on-flexure modal within 2% | compliant-joints tasks (incl. 3868) | closed-form PRB + 1-DOF k/m | ACHIEVABLE — closed-form, not continuum lock. Double-parallelogram parasitic error <L/1e5 also achievable (asymptotic cancellation). |
| Tensegrity: bar/cable K_e/K_g vs analytic axial; prism equilibrium within tol | 3797 / 3794-3798 (pending) | pin-jointed bar/cable, force-density | ACHIEVABLE — bar elements nodally exact for axial; no bending lock. |
| Consistent-mass ρV within 1e-12; PSD | 3818 (done) | tet4 consistent mass | ACHIEVABLE — exact integration of ρV for affine tets. |
| Eigensolver synthetic / residual checks (1e-8) | 3882 / eigensolve tests | generic Lanczos/dense | ACHIEVABLE — synthetic closed-form, no FEA discretization. |

---

## 7. Summary & recommended actions for Leo

**Unachievable signals found: 1** — task **3819** (modal beam frequencies "within 2%"),
covering both its `cantilever_beam_modes.ri` and `simply_supported_beam_modes.ri` example
bounds. Root cause: P1-tet bending lock on an L/r≈346 beam — the *identical* failure mode
as the two L2 freezes, here on a more-slender fixture, and **modal has no P2/aspirational
follow-up task** to receive the moved target.

**Plus 2 latent UNCERTAIN risks** that are cheap to neutralize now:
- **2928** cantilever **P1 5%** — pin a moderate aspect ratio (L/h ≤ ~8) or loosen to ~10%.
- **3780** anisotropic-beam "band" — keep loose for P1 / constrain fixture when the band
  is pinned.

**Recommended next steps (require Leo's approval — nothing mutated):**
1. **3819 (highest priority, in the active modal batch):** empirically measure the P1/P2
   `f₁` error on the 200×10×2mm fixture; relax the example bound to the measured P1-honest
   figure; **file a new P2-tet/beam-element modal follow-up** to carry the aspirational 2%;
   edit `modal-analysis.md:79-85` and `:470-471` to the honest bound + follow-up citation.
2. **2928:** pin the cantilever aspect ratio (or loosen P1 bound) in the task spec before
   it dispatches.
3. **3780:** add a note to keep the anisotropic-beam band P1-honest.
4. **3454:** 30-second confirm the in-progress branch tests against the 10% PRD signal
   (PRD itself already correct — no edit needed).

The pattern across all five FEA accuracy families is now consistent and **self-documenting
in-tree** except for modal: buckling and shells already encode the floor + follow-up in
their test headers and PRDs; modal is the one family whose tight bound has not yet met its
formulation reality because its verification task (3819) has not run. Catching it here
avoids the third L2 freeze.
