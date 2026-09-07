# Trampoline param-drop closure: a declared `.ri` param must reach the kernel or fail loudly

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-08-31 · **Approach:** B + H (contract + two-way boundary tests)

**Code anchors** verified against main `a31dc6a055` (2026-08-31). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

**Provenance:** four-agent trampoline sweep, 2026-08-31, run from the task-6097 discussion session
(`~/.claude/fleet/sessions/discuss-reify-6097-4077648/`). Every finding below was measured and then
independently re-verified at the cited anchor before landing here. Scope, severity and gate-shape
decisions in §6 were made by Leo in that session and are recorded here, not re-opened.

**Normative substrate:** `docs/legibility/design-invariants.md` — this PRD establishes **INV-PD-1**
(`declared-param-reaches-kernel`) and consumes **INV-SF-6** (`diagnostics-carry-codes`, owned by
`docs/prds/v0_6/eradicate-silent-undef.md`).

## 1. Goal

One user-observable guarantee:

> A `.ri` author who sets a declared `param` either gets its effect, or gets an **Error** naming the
> param and the task that owns making it work. Never a plausible-but-wrong number.

Today the third outcome is the common one. `solve_buckling(..., supports, ...)` accepts a required
`supports : List<Support>` argument and throws it away, hardcoding pin-pin boundary conditions; a
fixed-free column silently receives a critical load computed for the wrong end conditions, and
`reify check` exits 0.

After this PRD:

- **The contract (INV-PD-1).** Every `param` declared on a structure a trampoline consumes is either
  *honored* (its value reaches the kernel and changes the result) or *declared-ignored* — and a
  declared-ignored param is an enumerated, ratcheted allowlist entry naming a **live owning task**.
  There is no third state.
- **The loudness.** Setting a declared-ignored param to a non-default value is an **Error**
  (`E_PARAM_NOT_HONORED`) carrying the owning task id. A param that is structurally inapplicable on a
  given solver path gets a distinct `W_PARAM_NOT_APPLICABLE`.
- **The gate.** A new `reify-audit --pattern PDROP` cross-checks each trampoline's *declared* honored
  set against the `.ri` `structure_def` params it consumes, and reds on any param that is neither
  honored nor allowlisted — and on any allowlist entry whose owning task is absent, `done` or
  `cancelled`. New drops cannot reappear silently, and an allowlist entry cannot quietly become
  disowned.

## 2. Background — the measured drop census (2026-08-31)

Task **#4149** (done) fixed this defect class for exactly one struct: `BucklingOptions`, via
`buckling_unsupported_option_diagnostics` in `crates/reify-eval/src/compute_targets/buckling.rs`,
emitting the workspace's **only** code of this kind, `DiagnosticCode::BucklingOptionUnsupported`. The
sweep asked whether that fix generalized. It did not.

Within `BucklingOptions` itself the #4149 warning is **complete as a warning** — it covers exactly the
three dead knobs (`mode`, `sigma`, `auto_dense`), and `n_modes`/`tol`/`max_iters`/`element_order`
genuinely reach the kernel. The remaining §2.1 findings are all *outside* that struct.

**CORRECTION (2026-09-01, esc-7077-1).** The paragraph above originally ended "Every finding below is
*outside* that struct", and on that basis those three knobs were given no §7 row and no owner. That
conflates census-completeness with ownership. "Covered by a warning" is not one of C1's three states:
the three knobs are declared-but-not-honored, they are not meaningless on the buckling path, so C1
classes them `ignored` — and C1 invariant 1 (union **equals** the param set) plus invariant 2 (live
owner) therefore both bind. Their only historical owner, #4149, is `done`, which is exactly the D4
defect this PRD names for `mesh_size` / `element_order` / `target_quantity_of_interest`. They now carry
§7 rows and live owners; see D6.

### 2.1 Drops that change the computed answer

| # | Site | Measured behaviour |
|---|---|---|
| A1 | `solve_buckling` `supports` | `buckling.rs:149` — `let _ = &value_inputs[5];`. BCs hardcoded pin-pin. Comment: "Support-driven BC selection is deferred (no live task)". `supports : List<Support>` is a **required** parameter of `solve_buckling` (`solver_buckling_fns.ri`) with no caveat in its doc. |
| A2 | buckling `PointLoad.point` / `.direction` | Never read; load always applied at −Z (`buckling.rs:296,339`). Task **#4245** (done) fixed precisely this for `elastic_static`; buckling never received the same fix. |
| A3 | buckling `PressureLoad`/`TractionLoad`/`BodyForce`/`Gravity` | `extract_total_load` matches only the field name `"force"`; all other load types contribute zero and fall to a `1.0` N sentinel (`buckling.rs:714-750`). |
| A4 | `LoadCase.options` | Honored in `multi_case.rs`; silently dropped in `buckling_multi_case.rs`. |
| A5 | `TOTSShaper` / `RevoluteTOTSShaper` `actuator_limits` | Declared `trajectory.ri:702`; never read. `crates/reify-stdlib/src/trajectory/trampoline.rs:927` instead reads `field_f64(shaper_data, "force_limit", 1000.0)` — and **`force_limit` is a field on no `.ri` structure in the repo**, so the default always wins and every joint's limit is silently uniform. On the live registered dispatch path. |
| A6 | `ElasticOptions.max_iter` | Declared `= 1000` (`solver_elastic.ri`); `elastic_static.rs` hardcodes `CgSolverOptions { max_iter: SOLVER_MAX_ITER }` with `SOLVER_MAX_ITER = 2000`. Ignored **and** contradicting its own declared default. |
| A7 | `ElasticOptions.cg_tolerance` | Declared `= 0.000001`; hardcoded `tolerance: 1e-6` at the same site. Numerically equal today, so only a user-supplied *different* value is lost. |
| A8 | `TractionLoad` / `BodyForce` (elastic) | Zero mentions of either type anywhere in `crates/reify-eval/src/`. Both are `structure def … : Load`, so they typecheck into `List<Load>` and contribute nothing. **Whole types, not fields.** |
| A9 | `mechanism_modal` `tol` / `max_iters` | `modal_ops.rs:1779` — `let (requested_n_modes, _, _, _) = extract_eigen_knobs(options);`, then `modal_ops.rs:1658` builds `EigenSolverOptions { n_modes: padded_size, ..Default::default() }`. The user's values are replaced by library defaults (`tol` 1e-8, `max_iters` 1000). |
| A10 | `FDMSliceOptions.target_fidelity`, `AsPrintedOptions.target_fidelity` | Both inert. `AsPrintedOptions`' only consumer, `select_rungs` (`crates/reify-fdm/src/as_printed.rs`), has **zero production callers**. |

### 2.2 Drops that cannot change the answer

`mechanism_modal` never reads `element_order` — and unlike its non-use of `boundary_conditions` and
`reference_direction` (both explicitly documented as meaningless in the lumped generalized-coordinate
model), that non-use is documented **nowhere**. Element order has no meaning in a lumped model, so
honoring it is not the fix; refusing it is. This is the sole member of the `NOT_APPLICABLE` class.

### 2.3 Why this is unfulfilled scope, not drift

`ElasticOptions` was specified with working knobs and shipped without them. Task **#2911** (done) reads:
"Add `ElasticOptions` (element_order=P1 default with P2 override, **mesh_size override, max_iter,
cg_tolerance**, threads=…)". The declaration landed; the wiring did not; the task closed `done`. Task
**#2998** (done) separately ratified `target_quantity_of_interest` as "accepted-but-ignored in v0.4" —
a deliberate forward hook, but one whose owner is now terminal.

This is the audit's **C-07 fake-done leaf** pattern (`docs/architecture-audit/phase-3-files-synthesis.md`)
reaching production surface, and it is why the gate in §4 enumerates rather than samples: a census
finds today's drops, only an enumerating gate finds tomorrow's.

## 3. Sketch of approach

Three layers, in dependency order.

**Layer 1 — declare the honored set.** Each trampoline gains an explicit, in-code declaration of which
params of each structure it consumes it honors, and which it declared-ignores with an owning task. The
declaration is the source of truth the gate reads; it is not inferred. Inference over
`.fields.get("…")` was rejected — it cannot see indirection, which is the same aliasing blind spot that
produced `esc-6739-1`'s false-green manifest check.

**Layer 2 — emit on violation.** At extraction time, a declared-ignored param whose value differs from
its declared default raises `E_PARAM_NOT_HONORED` naming the struct, the param, the value and the
owning task. A `NOT_APPLICABLE` param raises `W_PARAM_NOT_APPLICABLE`. Both codes are real
`DiagnosticCode` variants per **INV-SF-6**.

**Layer 3 — gate the class.** `reify-audit --pattern PDROP` reads the Layer-1 declarations and the
stdlib `structure_def` params and reds on: a param in neither set; an allowlist entry whose owning task
is absent/`done`/`cancelled`; a declaration naming a param that no longer exists. Liveness is resolved
through the existing `fused_memory_client` the audit crate already carries (`crates/reify-audit/src/fused_memory_client.rs`),
the same mechanism PTODO's liveness lane β uses.

**Error, not warning — and why that is safe.** §6 D2 records the analysis: the "Error if it changes the
answer" rule classifies nearly every drop as Error, collapsing any two-tier severity split into a single
case that is better modelled as a distinct *kind* (`NOT_APPLICABLE`). Uniform Error is made
non-breaking by the ratcheted burn-down allowlist — the mechanism `eradicate-silent-undef` already
ratified for INV-SF-2 ("a hard gate NOW plus an enumerated, ratcheted burn-down allowlist, target:
zero entries"). Every entry carries an owning task, so the allowlist is a debt register, not an
amnesty.

## 4. Contract (H)

### C1 — the honored-set declaration

Each trampoline declares, per consumed structure, three disjoint sets:

- **honored** — the param reaches the kernel and changes the result.
- **ignored** — declared, not yet honored; carries an owning task id. Setting it to a non-default
  value is `E_PARAM_NOT_HONORED`.
- **not_applicable** — the param is meaningless on this code path; carries a one-line reason. Setting
  it is `W_PARAM_NOT_APPLICABLE`.

Invariants:

1. The three sets are disjoint, and their union must **equal** the `structure_def`'s param set. Not a
   subset — equality is what makes the gate a universal quantifier rather than an existence check.
2. Every `ignored` entry names a task that is live (not absent, `done` or `cancelled`) at gate time.
3. A structure consumed by two trampolines carries two independent declarations. `ModalOptions` is
   the worked case: `free_vibration` honors `element_order`, `mechanism_modal` declares it
   `not_applicable`.

### C2 — diagnostic shape

`E_PARAM_NOT_HONORED` names struct, param, the offending value, and the owning task id, so the
message is actionable without reading source. Precedent for the wording is #4149's
`"BucklingOptions.{param} = {value} is declared but not yet honored by …"`; this contract adds the
owning-task suffix. `W_PARAM_NOT_APPLICABLE` names struct, param, path, and the reason string from C1.

### C3 — default-comparison rule

The diagnostic fires on a value **differing from the declared default**, not on presence. A caller
spelling a param at its default is expressing no intent and must stay silent — otherwise every
existing call site reds on knobs that never worked. Comparison uses the `structure_def`'s declared
default; a defaultless declared-ignored param fires on presence.

### C4 — the gate's failure modes

PDROP reds on, and distinguishes in its output: (a) param in no set; (b) declared param that no longer
exists on the struct; (c) `ignored` entry with a dead or absent owner; (d) sets not disjoint. (b) and
(c) are the drift modes — they are why the gate must run continuously rather than being a one-time census.

## 5. Boundary-test sketch (H)

Facing both sides of the trampoline seam.

| # | Scenario | Preconditions | Asserts |
|---|---|---|---|
| B1 | Honored param reaches the kernel | `ElasticOptions(max_iter: 50)` on a solve that needs >50 iterations | result reports non-convergence; proves the value is not the hardcoded 2000 |
| B2 | Declared-ignored param at a non-default value errors | a param on the allowlist, set to a non-default | `reify check` exits nonzero, `E_PARAM_NOT_HONORED` names the param **and the owning task id** |
| B3 | Declared-ignored param at its default is silent | same param, spelled at its declared default | exit 0, no diagnostic (C3) |
| B4 | Not-applicable param warns, does not error | `mechanism_modal` with `element_order: ElementOrder.P2` | exit 0, `W_PARAM_NOT_APPLICABLE` fires naming the lumped-model reason |
| B5 | Gate catches a newly-added param | add a `param` to a consumed `structure_def`, declare nothing | `reify-audit --pattern PDROP` reds with failure mode (a) |
| B6 | Gate catches a dead owner | flip an allowlist entry's owning task to `cancelled` | PDROP reds with failure mode (c) |
| B7 | Gate catches a removed param | delete a declared-honored param from the `.ri` | PDROP reds with failure mode (b) |
| B8 | Buckling honors supports end-to-end | a fixed-free column via `solve_buckling` | P_cr **moves off** the pin-pin result toward the k=2 case — assert the ratio to the current pin-pin number, **not** agreement with the textbook k=2 value. See the G6 note below |
| B9 | Per-joint actuator limits are distinct | `TOTSShaper` with two joints at different limits | the shaped trajectory differs per joint; proves `1000.0` is no longer uniform |

**G6 note on B8 — do not assert a textbook buckling constant.** Two measured domain hazards make an
"agrees with the k=2 analytic value" signal a false premise of exactly the `esc-3453-5/6` class: P1-tet
**bending lock** floors slender-column accuracy at ~6.8–10% regardless of mesh density (the finding
behind #4052/#4066), and pointwise Dirichlet BCs realize an effective **fixed-pin k≈0.67–0.70**, not the
textbook fixed-fixed k=0.5. B8's assertion is therefore *relational* — the honored-supports result must
differ from the discarded-supports pin-pin result in the predicted direction and rough magnitude — and
any absolute bound must be justified against the bending-lock floor at the mesh density actually used,
not assumed. `crates/reify-solver-elastic/tests/euler_column_pin_pin.rs` is the calibration reference.

**Seam (G1).** The trampolines this PRD touches are **ComputeNode dispatch** consumers —
`docs/prds/v0_3/engine-integration-norm.md` §3.4, per `compute-node-contract.md`. The honored-set
declaration and its diagnostics attach at that seam; PDROP's consumer is the `/audit` sweep and the
`reify-audit` gate, not a new seam.

## 6. Resolved design decisions

**D1 — Scope boundary (Leo, 2026-08-31).** This PRD delivers the contract, the codes, the gate, the
owning-task allowlist, and the honors that port a mechanism that already exists elsewhere in the tree.
`TractionLoad`/`BodyForce` are **split out** — they are load types that were never implemented, in
both elastic and buckling, and implementing them is solver feature work rather than defect closure.
`ElasticOptions.mesh_size`, the elastic-path `element_order`, `target_quantity_of_interest`, and
`force_tet`/`require_hex_wedge` go on the allowlist with live owners rather than into this PRD.

**D2 — Uniform Error, not a two-tier severity split (Leo, 2026-08-31).** A two-tier rule was drafted
and rejected on analysis. The only non-hand-waving formulation — "Error if honoring would change the
computed result for some legal input" — puts `target_fidelity`, `cg_tolerance`, `max_iter` and the QoI
hook all in the Error tier alongside the load/BC drops, leaving the Warning tier with a single member
whose distinguishing property is not lesser severity but *inapplicability*. That is a difference of
kind, so it becomes `W_PARAM_NOT_APPLICABLE`, and severity is otherwise uniform. Uniform Error is made
landable by the C3 default-comparison rule plus the ratcheted allowlist.

**D3 — Declared honored-set, not inferred (Leo, 2026-08-31).** The gate reads an in-code declaration
rather than statically inferring reads. Inference is fragile against indirection (aliasing a field
before the read), the precise blind spot behind `esc-6739-1`. The declaration costs source churn once
and buys a gate that is a universal quantifier over params.

**D4 — Every allowlist entry has a live owner; the gate enforces it (Leo, 2026-08-31).** "No gap left
unowned." Owner liveness is a gate failure mode (C4c), not a convention. Entries whose historical
owner is terminal — `mesh_size` and elastic `element_order` (both from **#2911**, done) and
`target_quantity_of_interest` (**#2998**, done) — get new live owners filed by this PRD's own
decomposition.

**D5 — `target_fidelity` is a ruling, not an implementation.** `AsPrintedOptions.target_fidelity`'s
only consumer `select_rungs` has zero production callers. The disposition — wire it or delete the
param — is a decision task, not a build task, and is filed as such.

**D6 — No permanent ratification; a parked owner is not an owner (Leo, 2026-09-01, resolving
esc-7076-1 / esc-7077-1).** Both escalations offered "ratify permanently" as an exit, and a fourth C1
disposition (`ratified`, carrying a rationale and a PRD cite instead of a task) was drafted for it.
Both are **rejected**, and not on balance — the category is ruled out of existence. Leo, verbatim:
"PDROP should never express such a param. The whole point of PDROP is that there aren't any such params
allowed. The principle is: nothing is vacuous without being owned by something that is going to fix
that vacuousness. Permanently vacuous things are a bad smell and a source of bugs. And things that are
'going to get fixed someday without being owned' are effectively permanent: they don't get fixed until
they get owned, and that usually happens after they've caused some kind of expensive, painful mess."

Three consequences bind on the leaves:

1. **C1 keeps exactly three sets.** Do not add a fourth, and do not press `not_applicable` into service
   for a param that is merely unimplemented. C1's discriminator is *semantic meaninglessness on this
   code path* — the worked case is `element_order` on `mechanism_modal`, where element order has no
   meaning in a lumped model. A param the kernel *could* honor but nobody has yet is `ignored`.
2. **A `do_not_complete` owner fails C4c.** PDROP's liveness predicate as drafted in C1 invariant 2 is
   status-only ("not absent, `done` or `cancelled`"), which is strictly weaker than the PTODO predicate
   §3 claims to reuse (`ptodo.rs:1257` — "genuinely-live = present ∧ non-terminal ∧ ¬do_not_complete").
   η implements the **four-condition** predicate, and a parked owner is failure mode (c), reported
   distinguishably from a terminal one. PTODO's `parked-on-anchor` is not a blessing: #4644 built it as
   the recurrence *guard* after #4643 dismantled the never-completable-anchor pattern, on the grounds
   that such an anchor "is neither specific … nor ever-completable". An entry owned by a task that will
   never complete is the amnesty §3 forbids.
3. **Every owner must be genuinely completable.** The owners filed under this decision each close on a
   definite event: #7177 when the DWR PRD is authored (dep-gated on #4909, which owns replacing the
   uniform-refinement fallback that discards its own Dörfler marking); #7178 when the shift-invert PRD
   is authored; #7179 when `mode`/`auto_dense` are honored and leave the allowlist entirely.

**D6 note — a limit of the gate, recorded so it is not rediscovered.** C1 declares sets over the
*params* of a consumed structure, so an entire **unconsumed load type** is outside PDROP's reach by
construction: `loads: [TractionLoad(...)]` reaching `solve_buckling` is not a param drop and no
allowlist entry can express it. That class is owned by the dimension-checked-readers chain
(#5791 → #6922 → #5802) and by #5800, not here.

## 7. Disposition table

Every measured finding, with its exit. No finding is unassigned; that is the D4 obligation made
checkable.

| Finding | Disposition | Owner |
|---|---|---|
| A1 buckling `supports` | **honor** — port modal's `build_dirichlet_bcs` / `support_targets` | this PRD, leaf γ |
| A2 buckling `PointLoad.point`/`.direction` | **honor** — port `elastic_static`'s `target_node_set` + #4245's direction handling | this PRD, leaf γ |
| A3 buckling non-`PointLoad` load types | **split** (`TractionLoad`/`BodyForce`) + **honor** (`PressureLoad`/`Gravity`, already implemented elastic-side) | **#5800** / leaf γ |
| A4 `LoadCase.options` in buckling_multi_case | **honor** — `multi_case.rs` already does it | this PRD, leaf δ |
| A5 `actuator_limits` + `force_limit` phantom read | **honor** | this PRD, leaf ε |
| A6 `ElasticOptions.max_iter` | **honor** — also removes the 1000-vs-2000 contradiction | this PRD, leaf β |
| A7 `ElasticOptions.cg_tolerance` | **honor** | this PRD, leaf β |
| A8 `TractionLoad` / `BodyForce` | **split** to a load-types PRD | **#5800** (§9; #7078 was a duplicate, cancelled) |
| A9 `mechanism_modal` `tol`/`max_iters` | **honor** | this PRD, leaf ζ |
| A10 `target_fidelity` ×2 | **rule** — wire or delete | this PRD, leaf θ (decision) |
| `mechanism_modal.element_order` | **not_applicable** | this PRD, leaf ζ |
| `BucklingOptions.mode` | **honor** — dispatch dense vs shift-invert; both entry points already public | **#7179** (pending) |
| `BucklingOptions.auto_dense` | **honor** — `false` becomes a coded error below the Lanczos floor, not a faer panic | **#7179** (pending) |
| `BucklingOptions.sigma` | **allowlist**, owner live — σ≠0 needs an indefinite factorization, not plumbing | **#7178** (pending) |
| `ModalOptions.sigma` → `shift_frequency` | **allowlist**, owner live — retype #6097, numerics #7178 | **#6097** + **#7178** (both pending) |
| `force_tet` / `require_hex_wedge` | **allowlist**, owner live | **#4746** (pending) |
| `ElasticOptions.mesh_size` | **allowlist**, owner live | **#7074** (pending) |
| `ElasticOptions.element_order` (elastic path) | **allowlist**, owner live | **#7075** (pending) |
| `target_quantity_of_interest` | **allowlist**, owner live (DWR) | **#7177** (pending, dep-gated on #4909) |

## 8. Pre-conditions

None blocking. `reify-audit` already exposes a per-pattern module family
(`p1_producer_orphan.rs`, `pdead_dead_code.rs`, `ptodo.rs`, `puntested.rs`, `player.rs`, `pdssentinel.rs`,
`pdoccover.rs`) and a `Pattern` enum, so `PDROP` is an extension of live substrate, not new
infrastructure. PTODO supplies both the baseline-ratchet and the task-liveness precedents.

## 9. Cross-PRD relationship

| PRD / task | Direction | Mechanism | Owner |
|---|---|---|---|
| `v0_6/eradicate-silent-undef.md` | this PRD **consumes** | INV-SF-6: new diagnostics must carry a `DiagnosticCode`; its `PDIAG` pattern polices that | *that* PRD owns the rule; this PRD owns compliance for `E_PARAM_NOT_HONORED` / `W_PARAM_NOT_APPLICABLE` |
| `compute-fea-hardening.md` | sibling, no seam | Both edit `compute_targets/*`. Its INV-FEA-1 canonical-registration work and this PRD's param extraction are independent within those files. **File-lock contention only** — sequence, do not merge scope | that PRD keeps registration; this PRD keeps extraction |
| **#6097** (`ModalOptions.sigma` → `shift_frequency`) | this PRD **defers to** | 6097 already carries the modal not-yet-honored warning for `shift_frequency` as its own item 4 | **#6097.** This PRD must **not** double-land it; leaf ζ adopts whatever 6097 lands and adds only the remaining modal params |
| **#4746** (hex/wedge Phase A) | this PRD **defers to** | Owns honoring `force_tet`/`require_hex_wedge` | #4746 (pending; note it sits behind a phantom-done chain via #2987 — allowlisted, not adopted) |
| New: `traction-and-body-force-loads.md` | this PRD **splits to** | `TractionLoad` + `BodyForce` implementation, elastic and buckling | the new PRD; leaf ι files its bookmark |
| **#6484** (stale POSITIONAL comments) | adjacent | Also edits `buckling_column_p2.ri` | #6484 — independent, no seam |

No new contested-ownership pair is introduced (`phase-3-breadcrumb-map.md` §3 lists three; this adds none).

## 10. Decomposition plan

Phase 1 — foundation. Phase 2 — vertical slice proving the contract end-to-end on one struct.
Phase 3 — remaining honors. Phase 4 — docs-truth + close.

| Label | Task | Modules | Observable signal | Prereqs |
|---|---|---|---|---|
| α #7079 | Diagnostic codes + honored-set declaration mechanism | `reify-core` (diagnostics), `reify-eval` | `E_PARAM_NOT_HONORED` / `W_PARAM_NOT_APPLICABLE` exist as `DiagnosticCode` variants and round-trip; declaration macro compiles on one trampoline | — |
| β #7080 | Vertical slice: `ElasticOptions` fully declared + `max_iter`/`cg_tolerance` honored | `reify-eval` | B1 + B2 + B3 pass against `ElasticOptions`; `max_iter: 50` observably fails to converge | α |
| γ #7081 | Buckling: honor `supports`, `PointLoad.point`/`.direction`, `PressureLoad`/`Gravity` | `reify-eval`, `examples/` | B8 passes **under its G6 note** (relational, not a textbook constant); `buckling_column_smoke.ri` **and its line-42 comment** corrected in the same change — that comment says "pin-pin — fixed supports at both ends" while the file passes a single `FixedSupport(target: "base")`, an inconsistency the discard currently hides | α, β |
| δ #7082 | `LoadCase.options` in `buckling_multi_case` | `reify-eval` | a `LoadCase` carrying options changes the buckling multi-case result | α |
| ε #7083 | `actuator_limits` honored; delete the `force_limit` phantom read | `reify-stdlib` | B9 passes | α |
| ζ #7084 | Modal: honor `mechanism_modal` `tol`/`max_iters`; declare `element_order` not_applicable | `reify-eval` | B4 passes; user `tol` observably reaches the eigensolver | α, **#6097** |
| η #7085 | `reify-audit --pattern PDROP` + baseline ratchet + liveness lane | `reify-audit` | B5 + B6 + B7 pass; `/audit --pattern PDROP` reports the live allowlist | α, β |
| η′ #7086 | PDROP drift-guard registration | `tests/infra/` | `run-all-classification.manifest` row lands **in η′'s own diff**, wired as a hard dep of η — never prose-ordered (the esc-4914-162 failure) | η |
| θ #7087 | Ruling: wire or delete `target_fidelity` ×2 | `reify-compiler` stdlib, `reify-fdm` | a decision recorded; the param either reaches `select_rungs` or is gone | α |
| ι — discharged at decompose: #7074 #7075 #7076 #7077 #7078 | File the allowlist owners + the split PRD bookmark | — (task-filing; no task of its own) | `mesh_size`, elastic `element_order`, QoI each have a live owning task; `traction-and-body-force-loads.md` bookmarked | — |
| κ #7088 | Doc-chunk + exemplar + cheatsheet + discoverability | `reify-mcp` chunks, `examples/best_practices/`, `.claude/skills/reify-design/SKILL.md` | an author who knows the goal ("make the solver use my iteration budget") finds the knob from the chunks/index; every documented signature compiles | β, γ, ε, ζ |
| λ #7089 | PRD-close: terminal stamp | this PRD + manifest | committed `SHIPPED` header with landed leaf ids + AS-AUTHORED freeze + LIVE/AS-AUTHORED map | all above |

## 11. Out of scope

- **`TractionLoad` / `BodyForce` implementation** — split PRD (§9). This PRD only makes their current
  silence loud.
- **Honoring `mesh_size`, the elastic `element_order` path, `target_quantity_of_interest`,
  `force_tet`/`require_hex_wedge`** — allowlisted with live owners (§7).
- **Retro-migrating existing diagnostics** to the new codes. INV-SF-6's opportunistic-migration posture
  governs; this PRD adds codes, it does not sweep. **One scoped exception (D6, 2026-09-01):** leaving
  `BucklingOptions` un-migrated would permanently exempt the one struct `docs/legibility/design-invariants.md`
  holds up as "the house pattern" from the pattern it exemplifies. Its `mode` and `auto_dense` arms are
  retired by #7179 when it honors them, and its `sigma` arm gains a live `#NNNN` cite (#7178) where it
  cites no task today. That is opportunistic migration at the site, not a sweep.
- **The `sub p = Ctor(...)` silent-accept family** (#6946, #6869, #6191) — ctor *binding* diagnostics,
  a different seam from trampoline *extraction*.
- **Whether a param should exist at all.** θ rules on `target_fidelity` only because its consumer is
  provably unreachable; this PRD does not audit the stdlib for gratuitous knobs.

## 12. Open questions (tactical)

1. **Declaration syntax.** Macro (`honored_params!{…}`) versus a plain `const` table per trampoline.
   Macro reads better at the site; a const table is trivially readable by the gate without expansion.
   **Suggested resolution:** const table if the gate would otherwise need macro expansion to see the
   sets — decide in α, from whichever the PDROP reader can consume most directly.
2. **Where the declaration lives for multi-crate consumers.** `input_shape` extraction sits in
   `reify-stdlib`, not `reify-eval`. **Suggested resolution:** declaration lives beside the extraction
   code, gate discovers by convention rather than by a central registry. Decide in α.
3. **PDROP baseline seeding.** PTODO seeds a fingerprint baseline file. Whether PDROP needs one, or
   whether the allowlist *is* the baseline, is an implementation call. **Suggested resolution:** the
   allowlist is the baseline — a second ratchet file would be a second source of truth. Decide in η.
4. **`PressureLoad` on the buckling path.** Elastic implements it; whether buckling's geometric-stiffness
   formulation can consume a pressure load without new kernel work was not measured. **Suggested
   resolution:** measure in γ; if it needs kernel work, move it to the split PRD and allowlist it.
