# MITC3+ Shell Formulation Review — Curved-Element (A) vs Flat-Facet Shear-Bubble (B)

**Date:** 2026-05-29
**Author:** FE-domain expert-review session (analysis only — no task/PRD/escalation mutation)
**Decision owner:** escalation-watcher (esc-3392-360 + 4065 reconciliation)
**Subject:** Task 3392 "Curved-element shell formulation" — which formulation delivers the corrected bending/shear-locking improvement at least cost/risk?

---

## RECOMMENDATION (read this first)

**B — redirect 3392 to the genuine flat-facet Bathe & Lee 2014 MITC3+ (cubic rotation bubble + the MITC3+ *interior*-tying assumed-transverse-shear field + static condensation).**

The premise that justified the CURVED direction — *"MITC3+ requires curved elements because the bubble is inert on flat facets (K_NB ≡ 0)"* — is a **misdiagnosis**. Genuine MITC3+ is a **flat-facet** triangle. Its bubble is *not* inert on flat facets: it couples through the **transverse-shear** field via tying points placed **inside** the element (where the bubble is non-zero), not through the bending block (where it provably is zero). The corrected 3392 deliverable — *a measurable transverse-shear/bending-locking improvement vs bare flat-facet MITC3* — is **exactly** what flat-facet MITC3+ delivers, at a fraction of the cost and risk of building a non-standard curved-geometry element. The curved-element work does not vanish under B; it **relocates to task 4065**, where it is actually required (membrane locking is a curvature phenomenon and does not exist on a flat facet).

The recommendation letter is restated crisply at the end.

---

## 1. Nomenclature / attribution — which is the genuine Bathe & Lee 2014 MITC3+?

### 1.1 The genuine MITC3+ (Lee, Y., Lee, P.-S. & Bathe, K.-J., 2014, *Comput. Struct.* 138:12–23)

MITC3+ is built **on the flat 3-node MITC3 triangle** (Lee & Bathe 2004). The "+" adds three coupled ingredients:

1. **Cubic-bubble enrichment of the *rotation* field.** The two section rotations are enriched by a cubic bubble `f_b = ξ·η·(1−ξ−η)` (∝ the area-coordinate product `L₁L₂L₃`) tied to an **internal node at the centroid**, adding **2 internal rotational DOFs** (Δθ). The bubble vanishes on all three edges, so it adds *no* nodal/edge DOFs.

2. **A new assumed *transverse-shear* strain field with a new tying scheme.** This is the load-bearing part. The covariant transverse-shear strains are re-interpolated from samples taken at **tying points placed *inside* the element** (the literature: *"a new assumed natural shear strain field in terms of the values at six tying points inside the element"*). This is **not** the MITC3 edge-midpoint scheme.

3. **Static condensation** of the 2 internal bubble DOFs at the element level, yielding an 18×18 element matrix identical in size to bare MITC3.

The element **passes the patch, zero-energy-mode and isotropy tests** and shows excellent convergence **on flat triangular meshes, including highly distorted ones**. It relieves **transverse-shear locking** (and improves distorted/warped-mesh bending). It is **not** a curved-geometry element and does **not** introduce a varying in-plane Jacobian. Curvature of a real shell is captured the standard way — by faceting plus per-node directors — not by a varying mid-surface Jacobian inside one element.

> **Why the bubble is live on a *flat* facet.** In Reissner–Mindlin kinematics the rotation enters the **transverse shear** as a *value* (`γ_α ≈ w,_α − θ_α`) and the **bending curvature** as a *gradient* (`κ ≈ ∂θ/∂x`). The bubble's shear contribution is therefore `−f_b·Δθ`, which is non-zero **only where f_b ≠ 0**. MITC3+ samples shear at **interior** tying points where `f_b ≠ 0`, so the bubble survives into the assumed-shear stiffness and the nodal↔bubble shear coupling `K_NB^shear ≠ 0` even on a flat facet. After condensation, `K_NN` is genuinely modified (not bit-identical to bare MITC3), and shear locking is relieved.

### 1.2 What Reify actually ships today

- `crates/reify-solver-elastic/src/shell_assembly.rs` assembles **bare MITC3** (Bathe & Dvorkin 1985 / Lee & Bathe 2004): three **edge-midpoint** tying points `A=(½,0), B=(0,½), C=(½,½)`, a **single element normal** (no per-node directors), and a **constant** in-plane Jacobian (`det2`, `jac2_inv_t` computed once per element).
- `crates/reify-solver-elastic/src/elements/mitc3_plus.rs` is **misnamed**. Its header correctly describes MITC3+ ("the rotation field is enriched by a deviatoric cubic bubble … that eliminates spurious **transverse-shear** locking"), and it defines `bubble_at`/`bubble_grad_at` — **but those bubble functions are never called by the assembly**, and its `interpolate_assumed_shear` is the **MITC3** edge-midpoint scheme, not the MITC3+ interior-tying scheme. The file is MITC3 wearing a MITC3+ name, with a vestigial unused bubble.

### 1.3 The two header comments disagree — and `shell_assembly.rs` is the wrong one

| Claim | `mitc3_plus.rs` header | `shell_assembly.rs` header (lines 25–43) | Literature verdict |
|---|---|---|---|
| Bubble enriches… | **rotation** field | bending block | **rotation** ✔ (`mitc3_plus.rs`) |
| Bubble relieves… | **transverse-shear** locking | **membrane** locking | **transverse-shear** ✔ (`mitc3_plus.rs`) |
| Bubble routed via… | (rotation → shear) | bending cross-coupling `K_NB` | shear, **interior** tying ✔ |

`shell_assembly.rs`'s "Why this is MITC3, not MITC3+" note makes **two attribution errors**: it says the bubble is a **membrane**-locking cure (it is a **transverse-shear** cure), and it evaluates it via the **bending** cross-coupling block. Both errors trace to task 3349.

### 1.4 The K_NB ≡ 0 proof is correct but narrow — it kills the *wrong* bubble

The 3349 proof (`shell_assembly.rs:25–43`) is mathematically valid **for the construction it tested**: a *bending* bubble. Because linear shape functions give a **constant** nodal bending B-matrix on a flat, constant-Jacobian facet, it pulls out of the area integral, leaving `∫_T ∇f_b dA = ∮_∂T f_b·n ds = 0` (divergence theorem, f_b = 0 on edges). So a **bending** bubble's coupling is identically zero on flat facets. **True.**

But task 3349's own action plan (fused-memory get_task 3349) shows what it actually wired:

> *"write non-zero columns 18 and 19 of `b_cov` from `bubble_grad_at(ξ_tying, η_tying)` … the gradient is non-zero at the tying points even though `f_b` is zero there."* (tying points = the **edge midpoints** `A,B,C`.)

That is a category error compounded by the wrong tying scheme:

1. It injected **∇f_b** — a *bending* quantity — into the **shear** sampling columns. The shear field needs the bubble **value** `f_b`, not its gradient.
2. It kept the **MITC3 edge-midpoint** tying points, where **`f_b = 0`** — precisely the points at which the bubble's genuine shear contribution vanishes.

So 3349 measured the inertness of a **bending** bubble (K_NB ≡ 0, divergence theorem) and then **over-generalised** it to "MITC3+ is impossible on flat facets → curved elements required." The genuine MITC3+ avoids both pitfalls by moving the tying points **inside** the element and letting the bubble enter shear by its **value**. The prior-agent 2026-05-29 memory is **correct**: the proof mis-attributes *why* 3349's bubble was inert, and the real MITC3+ is a flat-facet element.

**Verdict (item 1): Candidate B is the genuine / closest Bathe & Lee 2014 MITC3+. Candidate A ("varying-Jacobian curved element") is a different, non-standard formulation that the literature does not call MITC3+.**

---

## 2. Which reaches the corrected twisted-cantilever deliverable at least cost/risk?

**Corrected deliverable (esc-3392-357, not re-litigated):** a *measurable* transverse-shear/bending-locking improvement vs bare flat-facet MITC3 on a bending/shear-dominated benchmark (e.g. twisted cantilever) at a fixed mesh, asserted in `shell_benchmarks.rs`. Membrane locking explicitly **out of scope** (→ 4065).

### Candidate A — CURVED (varying-Jacobian), the currently-planned direction

- **Mechanism:** per-node normals → Weingarten map → curvature tensor b_αβ → quadratic mid-surface correction → position-dependent in-plane J(ξ,η) → position-dependent bending B → now `∫_T ∇f_b dA ≠ 0` in physical space (nodal B no longer constant) → `K_NB ≠ 0` → non-trivial static condensation.
- **Would it move the twisted-cantilever number?** Yes — a varying J makes even the *bending* bubble live, so the result changes measurably.
- **Cost:** **High.** Per the 3392 record itself: *"Substantial mesh-layer plumbing (neighbour-finding or curvature estimation) — not contained to shell_assembly.rs … cross-cutting changes to mesh data flow … Requires FE-domain expert review of the chosen curvature representation."* New curvature-reconstruction pass, new mesh-layer data flow, varying-J B-matrices, plus condensation.
- **Correctness risk:** **High.** A bespoke curved-geometry triangle must be re-proven to pass the **patch test**, **isotropy**, and **no-spurious-zero-energy-modes**, and to reduce **bit-identically to flat MITC3** when flat. None of this is off-the-shelf; there is no citable reference element to validate against. The curvature reconstruction (bilinear-patch vs principal-curvature input) is itself a modelling choice with accuracy/consistency consequences.
- **Review burden:** **High** (explicitly called out in the task).
- **Naming:** perpetuates the "MITC3+" misnomer on a formulation that is not MITC3+.

### Candidate B — FLAT-FACET genuine MITC3+ (interior shear-tying + rotation bubble + condensation)

- **Mechanism:** keep flat facets and the constant Jacobian. (a) Wire the cubic bubble into the **rotation** field (2 internal DOFs); (b) replace the MITC3 edge-midpoint `interpolate_assumed_shear` with the **MITC3+ interior-tying assumed-shear field**; (c) statically condense the 2 internal DOFs. The bubble enters shear by **value** at interior tying points (f_b ≠ 0) → `K_NB^shear ≠ 0` on flat facets → condensation modifies `K_NN` → shear locking relieved.
- **Would it move the twisted-cantilever number?** Yes. MITC3+ is *designed and benchmarked* as a transverse-shear-locking cure on flat triangular meshes and is documented to out-perform MITC3 specifically on **distorted/warped** meshes — which is exactly the twisted-beam geometry. Bare MITC3 currently under-predicts the twisted beam ~1.7× (3349 notes); MITC3+ should measurably close that.
- **Cost:** **Low–moderate, element-local.** No mesh-layer plumbing, no neighbour-finding, no curvature pass. The bubble functions already exist (`mitc3_plus.rs:108–138`). The work is: the MITC3+ tying scheme + bubble-value wiring + a 2×2 condensation — all inside `shell_assembly.rs`/`mitc3_plus.rs`.
- **Correctness risk:** **Low.** It is a **published, citable** element with **known reference results** and **known tests it passes** (patch / zero-energy / isotropy). Validation is "match the paper," not "invent and prove a new element."
- **Review burden:** **Low–moderate**, and the review can check against literature.
- **Bonus:** resolves the standing `mitc3_plus.rs`-is-misnamed tech-debt honestly — the file finally *is* MITC3+.
- **One caveat to call out:** B delivers shear-locking relief with the **existing single element normal**; it does **not** require per-node directors. (Per-node directors would further help the *curved-shell* warping benchmarks but are not needed for the corrected shear deliverable — they belong in a re-scoped 4065 or their own task.)

### Benchmark-choice note

The twisted cantilever (MacNeal–Harder twisted beam) is warping-sensitive as well as shear-sensitive; it is a fine "measurable improvement" target and both candidates will move it. If an *unambiguous* transverse-shear-locking demonstration is wanted, a thin clamped/SS plate under transverse load across an L/t sweep isolates shear locking most cleanly. The corrected deliverable's "e.g. twisted cantilever" wording already permits this.

**Verdict (item 2): B reaches the corrected deliverable at decisively lower cost, lower correctness risk, and lower review burden — and is the literature-standard instrument for the exact locking class in scope.**

---

## 3. Effect on task 4065 (ANS membrane) — does ANS membrane require the curved/varying-J element?

**Yes — ANS membrane genuinely requires a curved substrate, and this is the strongest point in the whole analysis.**

Membrane locking is the inability of a **curved** element to represent inextensional (pure-bending) deformation without parasitic membrane strain; the membrane strain measure couples to bending **through curvature**. **On a perfectly flat facet, membrane and bending decouple — there is no membrane locking within the element, so ANS-membrane has nothing to correct.** This is standard FE theory.

Consequences for the two candidates:

- **Under A (3392 = curved):** 4065 layers ANS-membrane on 3392's varying-J element, as currently written. But note this is partly **circular**: 3392's curved element is what *introduces* the sub-element curvature (and hence the membrane-locking problem) that 4065 then cures. The cost of the curved element is paid in 3392, ostensibly to deliver a **shear** improvement — even though a far cheaper flat-facet element delivers that shear improvement.

- **Under B (3392 = flat-facet MITC3+):** 4065's stated premise — *"build ANS membrane on the varying-Jacobian element of 3392"* — is **invalidated** and must be re-scoped, because there is no varying-J element. **But the curved-element work does not disappear; it relocates to 4065, which is where it actually belongs**, since ANS-membrane is meaningless without a curved substrate. So under B the factoring becomes clean:
  - **3392** = transverse-shear cure on the substrate that suffices for it (flat facet).
  - **4065** = the curved-element substrate **plus** ANS-membrane, on the substrate the membrane cure actually requires.

  The program-level curved-element cost is **conserved**, not added — it is simply attributed to the task whose deliverable requires it instead of being front-loaded onto the shear task.

### A premise flag for 4065 (for the watcher — not a re-litigation of the corrected deliverables)

Independently of A vs B, 4065's working assumption that **ANS-membrane is the cure for Reify's curved MacNeal–Harder gap** deserves scrutiny before it is built:

- Reify meshes shells as **flat triangular facets**. Flat triangular facets do **not** exhibit classical element-level membrane locking; that pathology is primarily a **curved quad / higher-order** element problem.
- On flat-facet triangular meshes the coarse-mesh curved-shell under-prediction (hemisphere ~2200×, Scordelis-Lo ~21×, pinched cylinder ~76×) is dominated by **faceting (geometric) error + transverse-shear locking**, with **per-node directors + mesh refinement** as the conventional levers — not ANS-membrane on a triangle.
- So 4065 may be solving a problem that its substrate doesn't have unless 3392(A)'s curved element first *manufactures* it. This is worth a G6-style premise check during 4065 reconciliation; a re-scope toward **per-node directors** (proper degenerated-shell directors) may be the higher-value lever for the curved benchmarks than ANS-on-a-flat-triangle.

**Verdict (item 3): ANS membrane cannot sit on a flat-facet element; it needs curvature. Under B, that curved-element work moves into 4065 (correctly), and 4065's own premise should additionally be re-examined.**

---

## 4. Recommendation, with concrete costs

### Recommend: **B — redirect 3392 to genuine flat-facet MITC3+.**

**Reasoning, in one line:** the only thing that ever required curved elements was *membrane* locking; membrane is now explicitly **out of 3392's scope**; what remains (transverse-shear locking) is cured on **flat facets** by the genuine, citable, lower-risk MITC3+ — which is also what `mitc3_plus.rs` already claims to be.

### Concrete cost to the corrected 3392 spec (means change only; deliverable unchanged)

- **Unchanged:** the observable RED-test signal ("measurable improvement vs bare flat-facet MITC3 on a bending/shear-dominated benchmark at a fixed mesh, in `shell_benchmarks.rs`"); the smoke envelopes on the curved membrane benchmarks (those stay open pending 4065); the "do not chase ~50% MacNeal-Harder here" guardrail.
- **Changed (title + Scope "means"):** replace *"implement curved elements / per-element curvature / varying J(ξ,η) / mesh-layer neighbour-finding or curvature estimation"* with *"implement the genuine Bathe & Lee 2014 MITC3+ on the existing flat facet: cubic rotation-bubble enrichment (2 internal DOFs) + the MITC3+ interior-tying assumed-transverse-shear field + static condensation, element-local in `shell_assembly.rs`/`mitc3_plus.rs`."*
- **Net effort:** **drops** (removes all mesh-layer plumbing; reuses existing bubble functions; validates against a published element).
- **Side benefit:** fix the two attribution errors in the `shell_assembly.rs` header and the `mitc3_plus.rs` misnomer in the same change.

### Concrete cost to 4065 (the load-bearing constraint)

- 4065's dependency/premise line *"ANS membrane builds on the varying-Jacobian element [of 3392]"* must be rewritten. Two viable re-scopes (watcher's call during esc-3392-360 / 4065 reconciliation):
  - **(b-i)** 4065 **absorbs** the curved-element substrate it needs: "build the curved/per-element-curvature element **and** layer ANS-membrane on it." This is the smallest conceptual change — the curved-element work from 3392(A) moves here intact, where it is actually required.
  - **(b-ii)** Re-examine 4065's premise first (see §3 flag): if per-node directors + refinement are the real lever for the curved-shell gap on a flat-facet triangular code, re-scope 4065 toward directors and treat ANS-membrane-on-curved-triangles as a separate, later, research-gated item. *(My lean: do the premise check; it is cheap and may save building an ANS-membrane cure for a pathology a flat-facet triangle doesn't have.)*
- **What does NOT change:** 4065 keeps the published ~50%/4×4 MacNeal-Harder target and remains the home of the membrane-locking work. The only edit is *what substrate it builds on*, and that substrate is no longer a free input from 3392.

### Why not A, and why not D

- **Not A:** A pays the full price of a bespoke, non-standard, high-review-burden curved element to deliver a *shear* improvement that flat-facet MITC3+ delivers more cheaply and more safely; its main forward justification (feeding 4065) rests on a premise that may itself be wrong, and it cements the MITC3+ misnomer.
- **Not D (hybrid):** there is no genuine hybrid of A and B for 3392's shear deliverable — the clean answer is "B for 3392, relocate the curved-element substrate to 4065." That is already captured as B + the 4065 re-scope above, so a separate "D" adds nothing.

---

## Evidence trail

- `crates/reify-solver-elastic/src/shell_assembly.rs:1–43` — bare-MITC3 assembly; edge-midpoint tying; single element normal; constant Jacobian; the "Why this is MITC3, not MITC3+" note with the (narrow, bending-only) K_NB ≡ 0 proof and the two attribution errors.
- `crates/reify-solver-elastic/src/elements/mitc3_plus.rs:1–20, 108–200` — correct MITC3+ attribution in the header (rotation bubble → transverse-shear); `bubble_at`/`bubble_grad_at` defined but **unused**; `interpolate_assumed_shear` is the **MITC3 edge-midpoint** scheme, not MITC3+.
- fused-memory get_task **3349** — action plan wiring `bubble_grad_at` (∇f_b) into the **shear** `b_cov` columns at **edge-midpoint** tying points; the over-generalisation "True MITC3+ requires curved-element formulation"; the membrane-locking misattribution in the description.
- fused-memory get_task **3392** — corrected (esc-3392-357) deliverable: measurable improvement on a bending/shear benchmark; membrane moved to 4065; the "substantial mesh-layer plumbing / FE-domain review required" cost flags for the curved direction.
- fused-memory get_task **4065** — depends on 3392; "ANS membrane builds on the varying-Jacobian element"; carries the ~50%/4×4 target.
- `docs/prds/v0_4/structural-analysis-shells.md`, `docs/prds/v0_4/shell-stress-channel-surfacing.md`, `docs/architecture-audit/findings/structural-analysis-shells.md` (M-005) — the v0.4 bare-MITC3 contract and the MITC3/MITC3+ drift record.
- Literature: Lee, Lee & Bathe (2014) "The MITC3+ shell element and its performance," *Comput. Struct.* 138:12–23 — cubic rotation bubble, **new assumed transverse-shear field with tying points inside the element** (six interior points), static condensation, flat triangle, passes patch/zero-energy/isotropy, excellent on distorted meshes.
  Sources:
  - https://www.sciencedirect.com/science/article/abs/pii/S0045794914000595
  - http://web.mit.edu/kjb/www/Principal_Publications/The_modal_behavior_of_the_MITC3_plus_triangular_shell_element.pdf
  - http://web.mit.edu/kjb/www/Principal_Publications/Development_of_MITC_Isotropic_Triangular_Shell_Finite_Elements.pdf

---

## FINAL RECOMMENDATION

**B** — redirect task 3392 from the curved/varying-Jacobian element to the **genuine flat-facet Bathe & Lee 2014 MITC3+** (cubic rotation-bubble + MITC3+ interior-tying assumed-transverse-shear field + static condensation). It delivers the corrected twisted-cantilever shear/bending improvement at the lowest cost and risk, is the literature-standard element, and corrects the `mitc3_plus.rs` misnomer. Re-scope task 4065 so it owns the curved-element substrate it actually needs for ANS-membrane (and sanity-check 4065's own premise that ANS-membrane is the right lever for a flat-facet triangular mesh).

**B**
