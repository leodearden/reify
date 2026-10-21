# Capability manifest — flexible-assembly-modal

**PRD:** `docs/prds/v0_6/flexible-assembly-modal.md` · **Decomposed:** 2026-09-01 ·
**As-of:** main `4818478dff` (2026-09-01; the PRD pair's own landing commit — anchors by symbol,
re-locate lines at implementation time).

Machine-readable twin: `flexible-assembly-modal.capability-manifest.yaml` (stamped with real task
ids by `commit_planning`; **the sidecar is normative for `delivered_check` descriptors** — this file
is the human-readable mirror). Correcting a landed check is THREE writes: the `.yaml` descriptor,
this `.md` row, and the producer task's `metadata.delivered_checks` (resubmit the COMPLETE list —
the metadata merge is shallow last-write-wins).

Every substrate binding below was confirmed **by measurement, not assertion**, at decompose time
(three probe agents plus four hand-run `reify check` probes, all 2026-09-01). Every
deliverable-shaped `delivered_check` was **executed against the tree and confirmed RED today** — all
12 of them — so each flips green only on delivery.

## What the gates changed before this batch was filed

Four things were corrected at decompose rather than filed as authored:

1. **τ's scope (esc-4914-162 rule).** PRD §9's τ row read *"Boundary-gate + drift-guard
   registrations same-diff"* with prereq *"phase leaves"* — which makes τ the **owner** of
   registrations for tests landed by **upstream** leaves. That is exactly the ordering that turned
   `main` RED when task 4914 landed a gate-resident smoke binary whose registration leaf was ordered
   after it by PRD prose only. The overlay's decompose rule rejects that shape, so **registration
   ownership was pushed up into every test-adding leaf** (A1, A2, A3, B1, B2, B3, B4, C1, C2, C3
   each carry a same-diff registration clause), and τ's registration role reduced to *verification*.
2. **A3 adopts #7097 rather than duplicating it.** `docs/prds/v0_6/result-field-vacuity-closure.md`
   filed #7097 as the live PVAC owner for `.part`/`.topology`, explicitly inviting adoption by "the
   whole-printer-modal decomposition — coordinate there, do not double-own the modal capability".
   Filing a fresh A3 would have left #7097 deferred forever as a phantom owner while the gate read a
   dead id. #7097 is therefore rescoped in place as leaf A3.
3. **υ discharged at decompose, not filed as a leaf.** §9's υ row ("Bookmark filings") is a
   **task-state write**, and reify has no task-write path — its only fused-memory client
   (`crates/reify-audit/src/fused_memory_client.rs`) exposes `get_task`/`get_tasks` only, and the
   sandboxed write-set never grants `.taskmaster/`. A leaf asking a reify agent to file tasks is
   structurally undeliverable. Both bookmarks were filed by this session instead: **#3830** rescoped
   to CMS-under-this-surface, and a new CFD/Reynolds-calibrated-surrogate bookmark. Both stay
   `deferred`, excluded from the `commit_planning` flip, with external triggers naming π's and ρ's
   committed artifacts.
4. **C7 had no leaf.** The result extensions in contract C7 were unassigned in §9. The
   `ModalOptions` full-shape opt-in flag plus its storage format (Open Q 5) went to **A1**; per-member
   energy-share attribution inside merged groups went to **B2**.

## The four hand-run probes (G3 semantic vector, `reify check` on `target/release/reify`)

Fixtures were written to `/tmp/prd-gate-fixtures/` (ephemeral tier per `tests/prd-gate/README.md`;
re-derive as committed fixtures under `tests/prd-gate/fixtures/` if a leaf needs them as evidence).

| Fixture | Result |
|---|---|
| `param region : Geometry = 5mm` inside a **`port` block** | `All constraints satisfied.` · exit 0 · **zero diagnostics** |
| the same mismatch on a **plain** structure param | `warning: param 'region' has type 'Geometry' but its default expression has non-geometry type 'Scalar[m]'` · exit 0 |
| `param region : Geometry = face(plate,"top")` inside a **`port` block** | `All constraints satisfied.` · exit 0 · **zero diagnostics** |
| the same on a **plain** param | `warning: … non-geometry type 'FaceSelector'` · exit 0 |

Two conclusions, both load-bearing for B1: `face(...)` really does type as a **selector**, not
`Geometry` (corroborating `topology_selector_result_type` in `crates/reify-compiler/src/units.rs`,
which maps `"face" -> Type::Selector(SelectorKind::Face)`), so `RegionPort.region : Geometry`
(`crates/reify-compiler/stdlib/ports.ri`) cannot hold one type-correctly; and the port-block path is
**silent**, so it "works" today only through that silence. **Recorded discrepancy for the
implementer:** a static read of the call graph says `check_param_default_type`
(`crates/reify-compiler/src/entity.rs`) *is* invoked from a site labelled the port-member
declared-vs-initializer check — yet it demonstrably does not fire for port members. Drive B1 from the
probes, not the call graph. Note also that even on the plain path the mismatch is only a **warning**,
never an error, so closing the port hole buys warning parity, not rejection.

**The hole is separately owned.** The PRD-1 decompose filed it as **#7174** ("Port-block param
defaults skip type checking entirely: `param region : Geometry = 5mm` inside a `port` block passes
`reify check` with zero diagnostics") alongside its sibling **#7175** (connect direction checking
skipped for every dotted sub-port connection). B1 fixes the region's **type** so the region path is
type-correct; it does **not** absorb the general conformance fix, and it is deliberately **not**
dep-gated on #7174 — the region typing fix stands on its own and gating it would serialise two
independent repairs.

## Leaf table

| Label | Task | Capability bindings |
|---|---|---|
| A1 | (stamped in yaml) | body-arg overload precedent; N-ary realization channel; modal volume-mesh demand registration (measured absent); P2 promotion; `VolumeMesh.boundary`; pose seam upstream; kernel-absent rejection; C7 flag + storage format |
| A2 | (stamped in yaml) | selector→node-set seam upstream (measured: both halves exist, no middle); BC fidelity inherited, no accuracy premise asserted |
| A3 | (stamped in yaml) | join-key channel exists; `.part`/`.topology` are sentinels today; real handle only reachable after A1; allowlist retires after both paths honest; adoption-not-duplication |
| A4 | (stamped in yaml) | examples-smoke auto-compile gate; bidirectional INDEX guard; producers upstream |
| M-A | (stamped in yaml) | dep-gated milestone shape (#6626 convention) |
| π | (stamped in yaml) | **no numeric bound asserted by construction**; `docs/measurements/` is the tracked convention; sparse path measurable today |
| B1 | (stamped in yaml) | face-selector-is-not-Geometry (measured); port-param default type-check is a silent hole (measured); 6-DOF spring + congruence reduction upstream (measured absent); rigid-limit claim is relative not absolute; active-patch derivation avoids the #6583/#6592 hazard |
| B2 | (stamped in yaml) | heterogeneous field substrate upstream & test-only today (measured); piecewise-field shape has an in-corpus precedent; graph enumerable but joint semantics upstream; homogeneous byte-identity pin binds; #6887 answered; per-member attribution |
| B3 | (stamped in yaml) | eigensolver mesh-agnostic + matrix-free seam; sparse mandatory (scale datum); knob honoring upstream |
| B4 | (stamped in yaml) | corpus gate + INDEX guard (distinct pattern); reader constraints upstream |
| M-B | (stamped in yaml) | dep-gated milestone shape |
| ρ | (stamped in yaml) | **no numeric bound asserted by construction**; conditioning observable on existing substrate |
| C1 | (stamped in yaml) | anisotropic law shape exists; validity range cited from ρ, never guessed; missing-substrate refuses loudly; **constructor spelling deliberately unpinned** (anti-false-red); surrogate honesty is a shipped obligation |
| C2 | (stamped in yaml) | corpus gate + INDEX guard (distinct pattern); band derived from ρ; emergent k requires C1 |
| C3 | (stamped in yaml) | MSE machinery upstream (#6883), never re-landed; damping band inherited, not newly asserted |
| M-C | (stamped in yaml) | dep-gated milestone shape |
| σ | (stamped in yaml) | **zero modal chunk coverage today** (measured across all 17 chunks); cheatsheet index surface exists; signatures verified against declarations; one modal section, not two |
| τ | (stamped in yaml) | registration ownership pushed up at decompose; drift-guard registries exist and self-check; B12 band derived from π, never byte-identity; B19 rejection observed, not inferred |
| φ | (stamped in yaml) | terminal vocabulary closed at three values; header exemplars exist; cancelled-dependency disposition; milestones excluded from the close set |
| υ-bookmark-cfd-surrogate | 7161 | discharge record: bookmark filed at decompose because a task-state write is structurally unavailable to a reify agent |

## Bindings (mirror of the yaml; the yaml is normative)

The full `binding` prose, `verdict` and `delivered_check` for every capability above lives in
`flexible-assembly-modal.capability-manifest.yaml`. The rows below record only what a reader needs
without opening it.

### Verdicts

**No binding resolves to a FAIL value** (`declared-only` · `test-only` · `producer-absent` ·
`producer-extent-short` · `producer-downstream` · `fixture-ERROR` · `bound≤floor` ·
`rejection-absent`), so the batch queues clean. Two bindings would have been FAILs without their
wired edges, and are called out because they are the ones a later reader is most likely to
misjudge:

- **B2 / `element_stiffness_p2_with_field`** is `test-only` on main — its sole caller is
  `crates/reify-solver-elastic/tests/assembly_anisotropic.rs`, with no production call site — and
  `assemble_modal_km` (`crates/reify-eval/src/modal_ops.rs`) takes a single scalar `density: f64`.
  Both are **#6882's** deliverables. The `test-only` FAIL is retired by the real
  `add_dependency` edge B2 → #6882, not by any claim in this manifest.
- **A2 / the selector→node-set seam** is `producer-absent` in the middle: `FixedSupport.target` and
  `PinnedSupport.target` are `String` placeholders (`crates/reify-compiler/stdlib/fea_multi_case.ri`),
  the consume half (`target_node_set`, via `loads_supports_to_bc_node_sets`) expects a
  `Value::List` of `Value::GeometryHandle`, and the produce half (`build_face_anchors` /
  `resolve_selector_faces` in `bc_resolve.rs`) does not bridge them — but the two halves of that
  produce pair differ, and conflating them misreads the verdict. `resolve_selector_faces` is
  genuinely **test-only** (its only callers are the `#[cfg(test)]` tests in its own module).
  `build_face_anchors` is **production-wired**: `engine_build.rs:9345`, the Task-4092
  boundary-attribution branch of `execute_realization_ops`, gated on `demanded_boundary` — but
  that path is fed by *realization* boundary demand, never by an `.ri`-declared support `target`,
  so it is not the missing bridge either. What is absent is the `target : String` → `Selector`
  flip itself. Retired by the edges A2 → #5312 and A2 → #5313.

### Numeric premises (G6 branches 1–2)

Every quantitative claim in this PRD is either **derived from a measurement leaf** or **inherited
from an upstream PRD's own band**. Nothing is guessed:

| Claim | Basis |
|---|---|
| B12 dims-vs-body-arg agreement band | **derived from π's measured mesh deltas.** Byte-identity is explicitly *not* claimed — the two paths use different meshers |
| B14 rigid-limit tolerance | relative self-consistency against PRD 1's own rigid answer, not an absolute accuracy — no method error floor exposed |
| B16/B17 surrogate bands | **inside ρ's measured conditioning envelope** |
| B18 cross-level consistency band | **derived from ρ's measurements** |
| B22 damping composition band | **inherited from #6883's own established band** — C3 sets no new bound |
| π, ρ | assert **no** bound by construction (PRD decision 5) |

This is the esc-3453 prevention applied prospectively: a guessed 5% buckling bound met a 9–10%
bending-lock floor, and the fixture comment claiming "Tuned" was aspirational. The domain floors
that would have bitten here — P1-tet bending lock (9–11% on slender members, hence P2 mandatory on
the modal path), pointwise-Dirichlet BC realization (`k≈0.67–0.70`), eigensolver conditioning at
extreme stiffness ratios — are named in the leaves that touch them rather than papered over.

### Rejection premises (G6 branch 4 — the negative-assertion mandate)

Three signals assert a rejection. **None** is bound to a grep: a grep for a diagnostic's name goes
green on the string literal that declares it while the arm stays unreachable — the task-4575
silent-accept shape. Each is `kind: manual` with a named observing test.

| Assertion | Observing test | Producer |
|---|---|---|
| kernel-absent modal solve → Error, never hollow `Converged` | **B19** (wording coordinated with #6660's own acceptance) | #6660, upstream |
| surrogate with missing blocked substrate → loud Error naming the gate, never silent isotropic fallback | **C1's own tests** | C1 |
| a non-selector support target is rejected, not silently accepted | A2's signal + **#4371 BT4** (the migration's own two-way gate) | #5312/#5313 |

### Grammar (G3 vector 1)

**Not triggered.** This PRD introduces no novel *syntax* — every new surface is a function
signature, a constructor call or a port-member param on existing grammar. The G3 work here was
entirely the **semantic/behavioral** vector (`reify check`), recorded in the probe table above. No
`.ri` fixture is bound as grammar evidence, and none is needed.

### Accepted gaps in the mechanical checks

Recorded deliberately, per the overlay's `expect:`-scoping rule:

- **C1 has no mechanical check.** PRD C6 gives the constructor name as an *example*
  ("e.g. `bearing_film(k_per_area_normal, slip_axes, frame)`"), so the spelling is genuinely
  undecided. Pinning a guessed identifier is the **esc-6739-1** false-red shape — a critical
  `DEP_CAPABILITY_NOT_DELIVERED` escalation that blocks a dependent task even though the capability
  *was* delivered. Behavioural cover: B16 + B17, gate-resident via τ.
- **B2 has no mechanical check** for the same reason: the bonded-collapse symbol does not exist yet,
  and #6882's production wiring is a *sibling's* diff, so binding it here would attribute another
  task's delivery to B2. Behavioural cover: B15.
- **A4 / B4 / C2 use three *different* INDEX.md patterns** (`modal`, `[Mm]ixed.fidelity`,
  `calibrat`). A shared pattern would have gone GREEN for B4 and C2 the moment A4 landed — a
  silently vacuous check, which is worse than a missing one. The cost is that a differently-worded
  INDEX line false-reds; correct it via the three writes.
- **A3's `GeometryHandleRef` check** is a positive construct check scoped to
  `crates/reify-eval/src/modal_ops.rs` (0 occurrences today). It could in principle go green off a
  comment. It is *not* scoped to `CarriedTopology`, whose three occurrences in that file are **all
  prose comments** — exactly the esc-6739-1 collision. Behavioural cover: B20.

## D3 adversarial verification — ran, adjudicated, one real amendment

Workflow `wf_1fdef346-917` (`scripts/prd-decompose-verify.mjs`, copied to the session scratchpad and
patched to sonnet/haiku throughout — the committed script pins its Adversary at opus/xhigh, and the
2026-08-27 precedent that a sonnet adversary still finds real defects is what justified the
downgrade). 6 leaves × 4 roles = **24 agents, 0 errors, ~1.9M subagent tokens**.

**Raw verdict: 4 of 6 leaves `blocks: true` — and 21 of 21 blocking lines were the SAME harness
artifact.** Every one bottomed out in `No such file or directory`: the Prover agents bound probe
paths under `tests/prd-gate/fixtures/` and never wrote the files. Not one blocking line reflects a
falsified premise. The dispositions, leaf by leaf:

| Leaf | Raw | Disposition |
|---|---|---|
| A1 | blocks | **harness-scope.** All 7 lines probe the body-arg overload — A1's *own* deliverable. A decompose-time probe of a not-yet-implemented capability is correctly UNPROVABLE; that is the same property as the manifest's deliverable-shaped checks being RED today, not a false premise. |
| A3 | clean | — |
| B1 | blocks | **already discharged by hand.** All 4 lines are the region-typing probes, which this session **ran and captured** (the four-row table above). The Prover pointed at files it never wrote; the session's own runs are the evidence. |
| B2 | clean | Prover confirmed PASS on both live premises: a `Fixed` connector edge **does** spell and check today, and a union-of-solids **does** type-check as a `Solid` body arg. |
| C1 | blocks | **one line was a real finding — see below.** The other two are the deliberately-unpinned constructor name. |
| τ | blocks | **harness-scope, and the report confirms the premise.** Its own text: *"Confirms the claim is correctly negative: the fixture's constraint form must be a tolerance-band comparison, not an equality/byte-identity check."* B12's premise is that byte-identity is **not** achievable — the adversary agreed. |

### The one real amendment — C1's rejection premise (G6 branch 4)

The C1 Enumerator asserted the ctor-arg rejection posture does not hold; its Prover could not
demonstrate it (missing fixture). **This session ran the probe directly** (2026-09-01,
`target/release/reify`):

| Fixture | Result |
|---|---|
| `AnisotropicMaterial()` — both required params omitted, neither has a default | `All constraints satisfied.` · exit 0 · **zero diagnostics** |
| `AnisotropicMaterial(law: 5mm, frame: MaterialFrame())` — a `Length` into a `ConstitutiveLaw` param | `All constraints satisfied.` · exit 0 · **zero diagnostics** |

**The negative-assertion sentinel fired.** There is no ctor-arg nominal rejection for this shape
today — not even a Warning. C1's signal asserts a surrogate with missing/blocked substrate produces
a loud Error, so that rejection capability had **no producer**: left as authored this was a
`rejection-absent` FAIL, the exact task-4575 shape.

**Resolution (G6 (c) — change the asserted configuration):** C1 now owns its own loudness. Its
description was amended to require validating its inputs in the constructor's own trampoline /
graph-build path and emitting its own **coded** diagnostic there, explicitly **not** inheriting
rejection from the compiler and explicitly **not** waiting on the general repair (#5306 family, and
#7174 for the port-block variant). This mirrors PRD 1's own posture — its joint-edge loudness
validates at graph-build time with its own diagnostics rather than routing through the conformance
holes. C1's user-observable signal now names the observed coded Error and non-zero exit. Recorded on
the task as `metadata.d3_amendment`.

### Two adversary findings adopted without a scope change

- **`adv-tau-b12-ownership-gap`.** B12 was the only row in the §7 table with **no owning leaf** in
  §9 — every other row is named verbatim in exactly one leaf's signal (B13+B19→A1, B20→A3, B21→A4,
  B14+B23→B1, B15→B2, B16+B17→C1, B18→C2, B22→C3). Correct, and it is why τ's description enumerates
  the full B12–B23 table and why **τ depends on π**: B12's band cannot exist before π measures it.
  Ownership is hereby τ's, recorded rather than left implicit.
- **`C1-ADV-1`.** The draft probe bound C1's rejection to a `stderr_must_name` **substring** match,
  which contradicts C1's own INV-SF-6 obligation to assert **code identity**. The substring binding
  is rejected; C1's description now says so explicitly.

### One finding recorded, deliberately not acted on

The A3 adversary observed that the PVAC allowlist A3's signal says it *retires* does not exist in
the tree yet — it is built by `result-field-vacuity-closure.md`'s own gate leaf (#7102, pending).
No edge was wired: A3's deliverable is **populating** `.part`/`.topology`, and the allowlist
deletion is a rider that is simply a no-op if the allowlist has not landed. Wiring A3 → the gate
would serialise them for no benefit and couple A3 to a task whose contract requires A3 to stay
*live*. In practice #7098–#7106 land long before A3, which sits behind A1 → #6660 → PRD 1 β.

### Harness limitation worth carrying forward

The A1 adversary flagged that `assertion_kind: "ir"` binds to a `probe_kind: "ir"` whose `observe()`
logic in `scripts/prd-capability-check.py` is hard-coded and asymmetric (documented there as an
"eval-error proxy"). Combined with the Provers' habit of binding paths they never write, the α
runner's usable decompose-time reach is narrower than the workflow's verdict surface suggests. That
is a harness observation, not a finding against this PRD — but it is why every blocking line here
was adjudicated by hand rather than taken at face value.

## G7 — design-invariant walk (`docs/legibility/design-invariants.md`)

Every task in the batch was walked against reify's own invariant family (INV-SF-1..7, INV-AD-1..4,
INV-PD-1..2). **No waivers.** Four leaves carried genuine hits; each was resolved by writing the
obligation into the task, and the resolutions are stamped on the tasks as `metadata.g7_resolutions`.

| Leaf | Invariant | Resolution written into the task |
|---|---|---|
| A1 | `diagnostics-carry-codes` | the kernel-absent diagnostic carries a `DiagnosticCode`; **B19 asserts code identity, not a message substring** |
| A1 | `error-severity-exits-nonzero` | that Error exits non-zero — an exit-0 run carrying an Error is itself the defect |
| A1 | `declared-param-reaches-kernel` | the new `ModalOptions` full-shape flag and every new overload param declare a PDROP bucket (honored / declared-ignored with a live owner / not_applicable) |
| A1 | `result-fields-populated-or-owned` + `undef-has-provenance` | flag-off full-shape field holds honest `Undef` with an `UndefCause`, never a well-formed empty fake |
| B1 | `diagnostics-carry-codes` | wrong-kind region / empty node set / unformable frame all carry codes; tests assert code identity |
| B1 | `declared-intent-consumed-or-diagnosed` | a `RegionPort.region` resolving to **no nodes** is diagnosed, never silently reduced to a degenerate frame |
| B1 | `declared-param-reaches-kernel` | the region param moves from *declared-but-unconsumed* into `honored`; new tolerance/reduction knobs declare their bucket |
| B3 | `declared-param-reaches-kernel` | every mesh-budget knob Open Q 3 picks declares a bucket — a set-but-ignored mesh size must not yield an unchanged spectrum at exit 0 |
| B3 | `diagnostics-carry-codes` / `error-severity-exits-nonzero` | assembly and solve failures carry codes and exit non-zero |
| C1 | `diagnostics-carry-codes` | the "loud Error naming the gate" carries a code — a code-less error naming a gate **in prose** is the exact message-substring dependency this invariant exists to stop |
| C1 | `error-severity-exits-nonzero` | a surrogate with missing substrate never produces a number at exit 0 |
| C1 | `declared-param-reaches-kernel` | every constructor param declares a bucket; a near-massless default is a **honored** default, not a licence to drop the mass param — B16's monotone claim is meaningless if the stiffness knobs do not reach the assembly |
| C1 | `placeholders-owned-and-loud` | any placeholder shipped in this diff is loud and carries a live owning task |

Walked with no hit: A2, A3 (A3 *is* the INV-PD-2 response — see `result-field-vacuity-closure.md`,
whose own evidence section names `ModalResult.part`'s `placeholder_part()` fake and the undeclared
`ModalResult.topology` write as the census exemplars), A4, B2, B4, ρ, π, C2, C3, σ, τ, φ and the
three milestones. B2 inherits A1's and B3's diagnostic-code obligation for anything it emits during
bonded collapse; it introduces no new declared param or result field of its own beyond the
per-member attribution recorded above.

The **angle family** (INV-AD-1..4) does not fire: this PRD introduces no new angle-valued surface.
B1's active-patch projection and the joint-local surrogate frame both consume frames PRD 1 already
defines (`Frame3`, `MaterialFrame`), so no new boundary declares an angle convention.

## Cross-PRD edges wired as real `add_dependency` edges

Per `preferences_cross_prd_deps_real_edges` — the scheduler does not read metadata.

| Edge | Why |
|---|---|
| A1 → **#6660** | gmsh linked into the author binary + the loud kernel-absent refusal posture |
| A2 → **#5312**, **#5313** | the selector→node-set seam; A2 twin-wires it and never re-lands it |
| B2 → **#6879**, **#6880**, **#6882** | Field classification, declarative constructors, heterogeneous P2 modal assembly + per-element density |
| C3 → **#6883** | MSE post-process → `Mode.damping_ratio` |
| A1 → PRD 1 **β** · A3 → PRD 1 **ζ** · B1 → PRD 1 **δ**, **ε** · B2 → PRD 1 **α** · B3 → PRD 1 **ε** · B4 → PRD 1 **θ** | the pair dependency; PRD 1 owns the surface and result contracts, this PRD extends value sources into them |
| **#6887** → B2 | the parked milestone's decision is answered by B2; **#6626 removed** from its dep set per PRD §11 |

**#4371** (the selector migration's own BT1–BT7 gate) is deliberately *not* an edge: it is a
sibling gate proving the seam is consumed, not a producer of anything A2 needs.
