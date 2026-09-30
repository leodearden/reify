# Result-field vacuity closure: a declared result field is populated, degraded-with-reason, or owned

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-08-31 · **Approach:** B + H (contract + two-way boundary tests)

**Code anchors** verified against main `90e27653bb` (2026-08-31). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

**Provenance:** Part-investigation session census (`investigate-reify-574090`, 2026-08-31), every
finding re-verified by symbol in the authoring session (`prd-reify-1067854`, same day). Scope,
loudness, naming and doc-promise dispositions in §6 were ruled by Leo in the authoring session and
are recorded here, not re-opened. **Amended at decompose (same day)** from the D3 adversarial
verification (workflow `wf_0f28544c-9dc`; findings cited as `ADV-*`; fixtures
`tests/prd-gate/fixtures/adv_beta_*.ri`, carried verbatim by **#7106** until it lands them — their
own landing surfaced the pre-commit hook-env leak that task fixes): the δ↔γ reorder, D8/D9, the
§2.3 fake-value family, and the C1′/C2′ refinements.

**Mandate (Leo, 2026-08-31, verbatim):** *"Nothing should exist vacuously and unowned. Ever.
Anywhere."*

**Normative substrate:** `docs/legibility/design-invariants.md` — this PRD establishes **INV-PD-2**
(`result-fields-populated-or-owned`) and the umbrella principle **`nothing-vacuous-and-unowned`**,
both landed in this PRD's own commit. The same commit closes a measured gap: the sibling PRD
`trampoline-param-drop-closure.md` declares it establishes **INV-PD-1**
(`declared-param-reaches-kernel`) in that file, but no trampoline leaf owns landing the section —
this commit lands the INV-PD-1 entry too (docs vehicle only; the trampoline PRD stays normative for
that contract).

## 1. Goal

One user-observable guarantee, the output-side sibling of INV-PD-1's input-side guarantee:

> A `.ri` author who reads a declared result field either gets a **real value**, an **honest
> degraded value** (`Undef`, with its reason recorded), or can find the **live task** that owns
> making it real. Never a plausible well-formed fake, and never a field nobody owns.

Today the fake and the orphan are both shipping. `ModalResult.part` is declared as the analysed-part
handle and populated by `placeholder_part()` — a well-formed, zero-field `Part` instance a reader
cannot distinguish from real data — while the "will be grown in a later task" promise on the
declaration cites **#4578**, which is `done`: the growth has no owner. `ModalResult.topology` is
written by the engine without being declared in the `.ri` at all, and is always `Value::Undef` on
the dims path.

After this PRD:

- **The contract (INV-PD-2).** Every field a Rust producer writes into a `.ri`-declared result
  structure is *populated* (a real, sampleable value on the production path), *declared-degraded*
  (the honest `Undef` form plus a recorded reason), or *allowlisted* with a **live owning task**.
  The three sets are disjoint and their union **equals** the `structure_def`'s declared field set —
  and a written-but-undeclared field is itself a finding. There is no fourth state.
- **The honesty.** A degraded field holds the workspace's honest degraded form — `Undef`, with an
  `UndefCause` where that channel exists (INV-SF-1) — never a plausible well-formed fake.
- **The gate.** A new `reify-audit --pattern PVAC` cross-checks each producer's *declared*
  field-disposition set against the `.ri` `structure_def` fields, and reds on any field in no
  bucket, any written-but-undeclared field, any allowlist entry whose owner is absent/`done`/
  `cancelled`, and any bucket overlap. New vacancies cannot appear silently, and an allowlist entry
  cannot quietly become disowned.
- **The umbrella.** `docs/legibility/design-invariants.md` records the general principle with a
  **census table** of which declarable surfaces are detector-gated and which are convention-only —
  the residue is visible, not presumed covered.

## 2. Background — the measured vacancy census (2026-08-31)

### 2.1 The `Part` vacancy (empty AND inert AND disowned)

`ModalResult.part`, `ForcingTimeHistory.part`, `DisplacementTimeHistory.part : Part`
(`crates/reify-compiler/stdlib/modal_analysis.ri`). `Part` is a deliberately minimal opaque
`structure def Part { }` (task #4578, done) whose declaration comment promises "the actual
geometry+material+topology fields will be grown in a later task … cite task 4578" — a
ratified-then-orphaned owner. Runtime value: `placeholder_part()`
(`crates/reify-eval/src/modal_ops.rs`), sentinel `StructureTypeId(u32::MAX)`, zero fields, five
producer sites. A four-agent census found **zero readers** across the `.ri` corpus, Rust
production, GUI, and LSP. Only the `run_transient_response` echo site discards identity actually in
scope (the input result's `.part` is not re-echoed). The `Value::String("")` sites in the same file
are `#[cfg(test)]` fixtures, not a missed migration.

**Ratified disposition (Leo, 2026-08-31, Part-investigation session):** part identity is a **join
key, not a payload** — `.part` converges on the `CarriedTopology.part : GeometryHandleRef` identity
channel (`compute_targets/result_topology.rs`) when modal-on-real-geometry lands; until then it is
an **allowlist entry with a live owner**, never a parallel geometry+material+topology payload.

### 2.2 The undeclared engine-attached field

`ModalResult.topology`: the `.ri` `structure_def` declares six fields; the engine writes seven.
This is legal only because `StructureTypeId(u32::MAX)` construction bypasses field validation. The
value is always `Value::Undef` on the dims path (`build_modal_topology_value`). The populated form
exists — `CarriedTopology`, R3a task #4654 — but the modal path never reaches it. This is the
finding class the contract's written-but-undeclared arm (§4 C1′) exists for.

### 2.3 The mechanism-modal fake-value family (allowlisted to #7012)

`mechanism_modal` emits every `Mode` with `shape = []` and `participation_mass = 0`
unconditionally — and (adversary-measured at decompose, fixture
`tests/prd-gate/fixtures/adv_beta_v7_degraded_arith.ri`) also writes a **hard-coded**
`boundary_conditions = []` (not even an echo of the caller's) and `mass_matrix_norm` /
`stiffness_matrix_norm` `= 0.0`, with a source comment naming them known-unpopulated. These are
C2′ **fakes** (empty-list-masquerading-as-computed, zero-masquerading-as-measured), not the honest
`Undef` form — so they enter the gate as **allowlist entries owned by #7012 (pending)**, whose
description covers this extent. Whether each becomes *degraded-with-reason* (honest `Undef` + a
recorded lumped-model reason) or *populated* is #7012's ruling; the lumped model genuinely has no
3D mode shape and no mesh to project participation against, so degraded-with-reason is the likely
end state. (The damping half was fixed by #6875.) The *degraded* bucket's charter members are
instead the conventions that are already honest — §2.4.

### 2.4 The honest convention that already exists

Buckling's `pre_stress` and `degenerate_modal_result`'s `damping` use `Value::Undef` for
unpopulated fields — `degenerate_modal_result`'s own doc names it "the tet-result convention for
unpopulated fields". The value-honesty rule (§4 C2′) is this convention promoted to contract;
`placeholder_part()` is its charter violation (a well-formed fake where the convention says
`Undef`).

### 2.5 The enumeration handle

Rust-side result-structure producers construct with the registry-free sentinel
`StructureTypeId(u32::MAX)`. Measured 2026-08-31: **14 files in `crates/reify-eval/src`** (buckling,
elastic_static, modal_ops, dynamics_ops, membrane_load, multi_case, detectors, appearance,
trajectory_ops, form_find, tensegrity_load, as_printed_material, compute_persist, result_topology)
**plus 8 in `crates/reify-stdlib/src`** (analysis, dynamics/eval, dynamics/trampoline, fea,
snapshot, trajectory/input_shape, trajectory/tots, trajectory/trampoline). ~22 producer files. The
grep `StructureTypeId(u32::MAX)` is the census handle; leaf δ performs the measured sweep.

### 2.6 Why this is the same disease as INV-PD-1, one seam over

The input side (params discarded by trampolines) and the output side (result fields declared but
never populated) are the two halves of the same declared-surface honesty contract. The recurrence
mechanism is identical and measured: scope ships half-done, the task closes `done`, the declaration
keeps promising (#2911/#2987 on the input side; #4578's "then grow" on the output side; #2998's
ratified-forward-hook-with-terminal-owner on both). A census finds today's vacancies; only an
enumerating gate finds tomorrow's. Unenforced invariants decay in this repo — measured twice at the
infra layer (the rerere re-armer; the hooksPath clobber).

### 2.7 The doc-truth rot riding on the vacancy

`modal_analysis.ri`'s `Part` block and `ModalResult`/`ForcingTimeHistory` field-semantics comments
cite done #4578 as a future owner, and the `ForcingTimeHistory` comment claims "Consumer tasks θ/ι
locate the DOF assembly via this Part at runtime" — false; the trampolines enumerate their reads
and `part` is not among them. `placeholder_part`'s rustdoc cites #4578 the same way. These prose
promises escape both PTODO (no TODO-family marker) and PDOCCOVER (chunks↔registry only) — see D6.

## 3. Sketch of approach

Three layers, mirroring INV-PD-1's shape deliberately (§6 D1–D4 record where the mirror was
ratified rather than re-derived).

**Layer 1 — declare the disposition set.** Each producer gains an explicit in-code declaration of
its per-field dispositions for each result structure it constructs: `populated` /
`degraded(reason)` / `allowlisted(owner)`. The declaration is the source of truth the gate reads;
it is not inferred (the D3 rationale from INV-PD-1 carries over unchanged — inference cannot see
indirection, the esc-6739-1 blind spot). The declaration syntax follows whatever the PDROP
declaration mechanism (#7079) lands, so the two gates read one convention.

**Layer 2 — value honesty.** A `degraded` field holds `Value::Undef` (with an `UndefCause` where
the recording channel exists, per INV-SF-1) — never a plausible well-formed fake. No runtime
diagnostic ships in v1 (§6 D2): the gate plus the honest value form carry the loudness. Any future
runtime diagnostic carries a `DiagnosticCode` per INV-SF-6.

**Layer 3 — gate the class.** `reify-audit --pattern PVAC` reads the Layer-1 declarations and the
stdlib `structure_def` fields and reds on the §4 C4′ failure modes. Owner liveness resolves through
the audit crate's existing `fused_memory_client` (the PTODO liveness-lane precedent, same as
PDROP). PVAC **extends, never copies,** PDROP's `.ri` structure-reading and allowlist machinery —
extract-and-share, the way PTYPE shares PTODO's liveness lane.

## 4. Contract (H)

### C1′ — the field-disposition declaration

Each Rust producer declares, per constructed result structure, three disjoint sets:

- **populated** — the producer writes the real value whenever one exists on the path; an
  input-conditional absence writes the honest `Undef` form, never a fake (the mechanism-modal
  `damping` case: real when the caller supplies a descriptor, honest `Undef` otherwise).
- **degraded** — the producer writes the honest `Undef` form; carries a one-line reason (which may
  cite a task that owns a future upgrade).
- **allowlisted** — declared, not yet real; carries a live owning task id. The value written today
  must still be the honest `Undef` form for **new** entries; a pre-existing plausible-fake value
  (`placeholder_part()`, the `shape = []` / `participation_mass = 0` / `norms = 0.0` family) is
  tolerated only while its allowlist owner is live, and the flip to honest form belongs to that
  owner.

Invariants:

1. The three sets are disjoint and their union **equals** the `structure_def`'s declared field set
   — equality, not subset, so the gate is a universal quantifier.
2. Every field the producer *writes* appears in the declared field set — the written-but-undeclared
   arm (catches `ModalResult.topology`). An undeclared write is a **separate registry** from the
   field dispositions (it is keyed to writes, not to declared fields, so it can never satisfy
   invariant 1's equality); it is resolved by declaring the field in the `.ri`, deleting the write,
   or carrying an undeclared-write allowlist entry with a live owner.
3. Every `degraded`-cited upgrade task and every `allowlisted` owner is live (not absent, `done`,
   or `cancelled`) at gate time.
4. A structure constructed by two producers carries two independent declarations (the
   `degenerate_modal_result` vs `run_modal_analysis` pair is the worked case — the degenerate
   builder legitimately degrades fields the full path populates).

### C2′ — value honesty

A `degraded` or newly-`allowlisted` field holds `Value::Undef`, never a well-formed fake instance,
empty-list-masquerading-as-computed, or zero-masquerading-as-measured. Where the `UndefCause`
channel reaches the site, the cause is recorded (INV-SF-1); where it does not, the declaration's
reason string is the record. This is the existing tet-result convention (`pre_stress`, degenerate
`damping`) promoted to contract.

### C3′ — declared, proven by boundary test

Population is claimed by declaration and proven by the §5 boundary tests per slice — never inferred
by the gate from `.fields` writes. (The output-side analog of INV-PD-1's C3 default-comparison rule
does not exist: there is no user intent to compare on the read side, so presence/absence of the
honest form is the whole observable.)

### C4′ — the gate's failure modes

PVAC reds on, and distinguishes in its output: (a) a declared `.ri` field in no set; (b) a declared
set naming a field that no longer exists on the `structure_def`; (b′) a field the producer writes
that the `structure_def` does not declare; (c) a `degraded`/`allowlisted` entry whose cited task is
dead or absent; (d) sets not disjoint; (e) **vacuous scan** — discovery returning zero producers or
zero declarations on the real tree is itself RED (positive-control floor: the §2.5 family is known
non-empty, so an empty scan is a broken scanner, not a clean tree — the `armed-but-vacuous` /
`empty-scan-needs-a-control` hazard). (b), (b′) and (c) are the drift modes — why the gate runs
continuously rather than as a one-time census.

## 5. Boundary-test sketch (H)

Facing both sides of the producer seam.

| # | Scenario | Preconditions | Asserts |
|---|---|---|---|
| V1 | Gate catches an undisposed field | fixture producer + `structure_def` with one field in no set | PVAC reds, failure mode (a) |
| V2 | Gate catches a removed field | fixture declares a disposition for a field deleted from the `structure_def` | PVAC reds, failure mode (b) |
| V3 | Gate catches an undeclared write | fixture producer writes a field the `structure_def` lacks | PVAC reds, failure mode (b′) |
| V4 | Gate catches a dead owner | flip a fixture allowlist entry's owner to `cancelled` | PVAC reds, failure mode (c) |
| V5 | Gate catches overlap | one field in two sets | PVAC reds, failure mode (d) |
| V6 | Real tree is green with the debt visible | main, full family declared (δ precedes γ — see §10) | `reify-audit --pattern PVAC` exits 0 **and its stdout names each allowlist entry with its owner task id** — exit code alone cannot distinguish a working detector from a silent one (adversary finding ADV-γ-6) |
| V7 | Degraded fields are honest on the production path | construct a degenerate/degraded result via the production entry (Rust-side test, **never release-gated** — the modal e2e suite is `#[ignore]`d in debug lanes, so a release-gated V7 would never run in a task lane) | every `degraded`-declared field is `Value::Undef`, not a well-formed fake |
| V8 | Populated means the solve's own value | the modal happy path (Rust-side, never release-gated) | every `populated`-declared field holds a value the solve computed (e.g. `modes` non-empty with finite frequencies) — **not merely non-`Undef`/non-sentinel**, since `[]` and `0.0` satisfy that predicate while being exactly the fakes C2′ forbids (adversary fixtures `adv_beta_v7_degraded_arith.ri`, `adv_beta_v7_pm_is_real_zero.ri`) |
| V9 | An empty scan cannot green | point discovery at a producer-free tree (hermetic fixture) | PVAC reds, failure mode (e) — never exit 0 on zero discovered producers |

**G6 note.** No boundary test asserts a numeric bound, an exactness, or a rejection diagnostic — V1–V6
assert behaviour of a detector this PRD builds, V7/V8 assert value forms of existing production
paths (verified achievable: `degenerate_modal_result` already writes `Undef` damping on main;
`run_modal_analysis` already populates `modes`). No false-premise hazard of the esc-3453 class.

**Seam (G1).** The producers are ComputeNode-dispatch consumers
(`docs/prds/v0_3/engine-integration-norm.md` §3.4) plus the `eval_builtin` trampolines in
`reify-stdlib`; the declarations attach beside the producers at those existing seams. PVAC's
consumer is the `/audit` sweep and the `reify-audit` gate; the honesty consumer is every `.ri`
author reading result fields. No new seam.

## 6. Resolved design decisions

**D1 — Scope: the full sentinel family, day one (Leo, 2026-08-31).** PVAC's v1 surface is every
Rust-side result-structure producer enumerable via the `StructureTypeId(u32::MAX)` construction
idiom — ~22 files across `reify-eval` and `reify-stdlib` (§2.5). A modal-only v1 was rejected: it
leaves the residue invisible until a follow-up lands, the exact decay pattern the umbrella exists
to prevent. The decomposition still lands one vertical slice first (β, modal) before the sweep (δ).

**D2 — Gate-only v1 plus the value-honesty rule (Leo, 2026-08-31).** No runtime diagnostic ships:
loudness comes from the gate plus C2′'s honest `Undef` form, which a reader *can* distinguish from
data (unlike `placeholder_part()`'s fake). A read-time `W_`/`E_` was considered and deferred — it
requires read-tracking in generic `StructureInstance` member access and is v2 territory; if built
it carries a `DiagnosticCode` (INV-SF-6). Dispositions-only (no value-form rule) was rejected: it
leaves the silent half alive.

**D3 — Declared disposition set, not inferred.** Inherited from INV-PD-1 D3 with the same
rationale, not re-argued: inference over field writes cannot see indirection (the esc-6739-1
aliasing blind spot). The declaration costs source churn once and buys a universal quantifier.

**D4 — Every entry has a live owner; the gate enforces it.** Inherited from INV-PD-1 D4: owner
liveness is a gate failure mode (C4′c), not a convention. The measured orphans — `.part`'s growth
promise (#4578, done) — get live owners via §7's dispositions.

**D5 — Naming (Leo, 2026-08-31).** Invariant **INV-PD-2**, slug `result-fields-populated-or-owned`
— paired with INV-PD-1 as one declared-surface family (input and output sides of the same
contract). Detector pattern **PVAC** (vacuous) — general enough to absorb future vacancy classes;
PECHO (echo-producer framing) rejected as too narrow. The umbrella section is named from Leo's
sentence: `nothing-vacuous-and-unowned`.

**D6 — Doc-promises: clean up here, detection bookmark to PTODO's grammar (Leo, 2026-08-31).**
The §2.7 prose-promise class (task cites in doc comments outside TODO markers) is fixed by leaf ε
for the measured sites, and a **bookmark task** is filed (leaf ζ) for extending PTODO's grammar to
prose task-cites, so the class has an owner, not just a census row. Folding it into PVAC was
rejected — cite-grammar is PTODO's precedent; PVAC gates value dispositions.

**D7 — The umbrella and both invariant entries land in this PRD's own commit.** The umbrella
principle plus the INV-PD-2 entry are this PRD's deliverable; the INV-PD-1 entry rides along to
close the measured gap (§header). Landing them as docs rather than as a leaf makes the principle
citable by every PRD authored from today, including the in-flight siblings.

**D8 — The allowlist IS the baseline (pinned at decompose, 2026-08-31).** No fingerprint baseline
file, no baseline-generator bin, no baseline-freshness guard — the PTODO baseline apparatus
(`ptodo-baseline.txt` + generator + freshness test) is NOT mirrored, because a second ratchet file
would be a second source of truth over the same debt. Pinned here rather than left as an open
question because the choice determines γ's own same-diff registration set (adversary finding
ADV-γ-9): with D8, γ registers exactly one infra runner row and no baseline-artifact guards.

**D9 — The vacuity floor requires new CLI plumbing; the "additive pattern" shape does not cover
it (adversary finding ADV-γ-8, measured).** Every existing pattern dispatches
`check(ctx) -> Vec<Finding>` from the `reify-audit` bin, which is findings-only: PTODO's own §6.6
scan-stats floor lives in `check_with_stats` and is consumed **only by its test harness — the CLI
never sees it** (measured: zero `check_with_stats` references in the bin/lib dispatch). C4′(e) as
a *CLI-observable* failure mode therefore charters new work in γ: a stats-carrying dispatch for
PVAC (and the floor asserts in both the CLI exit path and the infra runner). The §8 substrate
claim is honest only with this carve-out.

## 7. Disposition table

Every measured finding, with its exit. No finding is unassigned (D4).

| Finding | Disposition | Owner |
|---|---|---|
| `ModalResult.part` / `ForcingTimeHistory.part` / `DisplacementTimeHistory.part` | **allowlist** — join-key convergence on `GeometryHandleRef` when modal-on-real-geometry lands (§2.1 ratified direction) | whole-printer-modal decomposition (in flight); leaf ζ files the owner if absent at decompose time |
| `ModalResult.topology` (written-but-undeclared, `Undef`) | **undeclared-write registry entry** (C1′-2's own arm — it is not a declared field, so it can never sit in the field allowlist; adversary finding ADV-β-3) — populated form is `CarriedTopology` (R3b twin) | same owner as `.part` (one convergence, one owner) |
| `run_transient_response` `.part` re-echo identity discard | **allowlist** — rides the `.part` entry; noted for its owner | same owner |
| `mechanism_modal` `Mode.shape` / `participation_mass` | **allowlist** — the values are C2′ fakes (`[]`, `0`), so they cannot sit in `degraded` until #7012 rules and flips the form (§2.3) | **#7012** (pending; description covers this extent) |
| `mechanism_modal` hard-coded `boundary_conditions = []` + `mass/stiffness_matrix_norm = 0.0` | **allowlist** — same fake-value family, adversary-measured (§2.3) | **#7012** (pending; description covers this extent) |
| `degenerate_modal_result` / degenerate-builder fields (`modes=[]`, `bcs=[]`, `norms=0.0`) | **degraded** — error-path builder accompanied by its diagnostic; β flips remaining fakes to honest `Undef` **on the degenerate builders only** (error paths, minimal observable change) and records reasons | this PRD, leaf β |
| buckling `pre_stress` / degenerate `damping` `Undef` convention | **degraded** — already honest; declarations land in δ | this PRD, leaf δ |
| `placeholder_part()` plausible-fake form | tolerated under `.part`'s allowlist entry per C1′; the honest-form flip belongs to the owner | `.part`'s owner |
| stale prose promises (§2.7) | **fix** — rewrite to cite live owners; delete false consumer claims | this PRD, leaf ε |
| prose-task-promise class (detection) | **bookmark** — PTODO grammar extension | leaf ζ files it |
| remaining ~20 producer files' dispositions | **sweep + declare** | this PRD, leaf δ |

## 8. Pre-conditions

None blocking. `reify-audit` exposes the live per-pattern substrate (`Pattern` enum;
`p1_producer_orphan.rs`, `ptodo.rs`, `puntested.rs`, `pdoccover.rs`, `pdssentinel.rs`, …), the
`fused_memory_client` liveness lane is shipped (PTODO lane β precedent), and the baseline/ratchet
precedents are live — **with the D9 carve-out**: the findings-only `check(ctx)` dispatch shape
covers failure modes (a)–(d), but the (e) vacuity floor needs new stats plumbing into the bin
dispatch, chartered in γ. No novel grammar — G3 N/A beyond the substrate named here (verified
against the trampoline PRD §8's identical check). Two soft sequencing edges, wired as real
dependencies at decompose: PVAC's declaration syntax follows **#7079** (PDROP α); PVAC's detector
extends **#7085**'s (PDROP η) structure-reading + allowlist machinery.

## 9. Cross-PRD relationship

| PRD / task | Direction | Mechanism | Owner |
|---|---|---|---|
| `v0_6/trampoline-param-drop-closure.md` | sibling, shared machinery | INV-PD-1 (input side) ↔ INV-PD-2 (output side); PVAC follows #7079's declaration syntax and extends #7085's `.ri`-reading + allowlist machinery — **extract-and-share, never copy** | that PRD owns PDROP + the shared substrate it lands first; this PRD owns PVAC + the extraction refactor if one is needed; this PRD's commit lands *both* invariants' `design-invariants.md` entries (docs vehicle only) |
| whole-printer-modal design (session `design-reify-3830-1055491`, in flight) | this PRD **defers to** | its decomposition is the expected live owner of the `.part`/`.topology`/re-echo allowlist entries (§7) | that design owns the modal capability; this PRD owns the invariant + detector. **Do not double-own.** If its tasks are absent at decompose time, leaf ζ files placeholder owners (the INV-PD-1 leaf-ι mirror) and that design adopts them |
| `v0_6/placeholder-type-eradication-ratchet.md` (PTYPE) | sibling, no seam | PTYPE gates the *type* axis (placeholder-typed signatures); PVAC gates the *value* axis (result-field dispositions); both cite the umbrella | no shared code beyond the PTODO liveness lane both already consume |
| `v0_6/eradicate-silent-undef.md` | this PRD **consumes** | INV-SF-1 `UndefCause` machinery cited by C2′; INV-SF-6 applies to any future runtime diagnostic | that PRD owns the machinery and rules |
| **#7012** (mechanism_modal degraded fields) | this PRD **defers to** | owns the shape/participation upgrade-or-advisory decision; PVAC's `degraded` declaration cites it | #7012 |
| **#6346** (PPRDSTATUS) | census row only | named in the umbrella census table as chartered | #6346 |

No new contested-ownership pair is introduced (`phase-3-breadcrumb-map.md` §3 lists three; this
adds none).

## 10. Decomposition plan

Phase 1 — foundation. Phase 2 — vertical slice proving the contract on the modal family. Phase 3 —
full-family declarations, **then** the detector (δ precedes γ: adversary finding ADV-γ-7 falsified
the reverse order — with only the modal family declared, PVAC's real-tree run would red mode (a)
on the ~21 undeclared files, or the gate would have to be vacuous; the sweep is mechanical once α
exists, so it lands first and γ's real-tree green is honest). Phase 4 — docs-truth + close. (The
umbrella + invariant entries are **not** leaves — they land with this PRD's commit, D7.)

| Label | Task | Modules | Observable signal | Prereqs |
|---|---|---|---|---|
| α #7099 | Field-disposition declaration mechanism (C1′), following PDROP's syntax convention | `reify-eval`, `reify-stdlib` | declaration compiles on the modal producers and is consumable by a reader fn; unlocks β/δ/γ | **#7079** |
| β #7100 | Vertical slice: modal family fully declared + V7/V8 boundary tests (Rust-side, never release-gated) + honest-`Undef` flips on the **degenerate builders only** | `reify-eval` (modal_ops) | V7 + V8 pass; the modal declarations carry the §7 dispositions (`.part` allowlisted to #7097, `.topology` in the undeclared-write registry, the mechanism-modal fake family allowlisted to #7012) | α |
| δ #7101 | Full-family sweep: dispositions declared for every §2.5 producer file (intermediate — unlocks γ's honest real-tree green) | `reify-eval`, `reify-stdlib` | declarations compile across the full ~22-file family; the reader fn enumerates them; consumer: γ | α, β |
| γ #7102 | `reify-audit --pattern PVAC` + allowlist + liveness + stats-carrying dispatch (D9) + infra ratchet runner, **drift-guard registrations in γ's own diff** (run-all-classification manifest row; wallclock registration if any elapsed-time assertion is added — the esc-4914-162 lesson, same-diff not prose-ordered; per D8 no baseline-artifact guards exist to register) | `reify-audit`, `tests/infra/` | V1–V6 + V9 pass; `reify-audit --pattern PVAC` exits 0 on the real tree with stdout naming each allowlist entry + owner id; `/audit` routes the pattern. Unknown-pattern discrimination: `--pattern PVAC` before γ exits 125 ("unknown pattern"), after γ it is an accepted value — tests must distinguish exit 1 (findings) from exit 125 (no such pattern) | α, δ, **#7085** |
| ε #7103 | Doc-truth cleanup: rewrite the §2.7 stale promises to cite live owners; delete the false θ/ι consumer claim; fix `placeholder_part` rustdoc | `reify-compiler` stdlib, `reify-eval` | no doc comment in `modal_analysis.ri`/`modal_ops.rs` cites a terminal task as a future owner; the ζ-filed owner #7097 is cited instead | ζ |
| ζ — discharged at decompose: **#7097** (`.part`/`.topology` owner, filed **deferred** as a trigger-gated bookmark; the whole-printer-modal decomposition adopts or rescopes it), **#7098** (PTODO-prose-grammar bookmark, D6), **#7012 rewritten** to cover the §2.3 extent | File the owners + bookmarks | — (task-filing) | every §7 allowlist entry names a live task whose text covers the entry's extent; the bookmark exists | — |
| η #7105 | PRD-close: terminal stamp + census-status refresh | this PRD + manifest + `design-invariants.md` census column | committed `SHIPPED` header with landed leaf ids + AS-AUTHORED freeze + LIVE/AS-AUTHORED map; the umbrella census table's PVAC row flipped chartered→shipped | all above |

## 11. Out of scope

- **Populating `.part`, `.topology`, or the re-echo** — owned by the whole-printer-modal
  decomposition (§9). This PRD makes their vacancy declared, honest, and owned; it does not wire
  them.
- **The #7012 upgrade-or-advisory decision** for `mechanism_modal` shape/participation — that task
  rules; PVAC only records the degraded disposition citing it.
- **Runtime read diagnostics** — v2, per D2.
- **Sweeping the stdlib for gratuitous fields** — whether a declared field *should exist* is not
  audited here (the INV-PD-1 §11 posture, mirrored).
- **Other vacancy axes** — params (PDROP), placeholder types (PTYPE), TODO owners (PTODO), doc
  chunks (PDOCCOVER): each keeps its own gate; the umbrella census table binds them together
  without merging them.
- **Retro-migrating other `Undef` conventions** to recorded causes — INV-SF-1's opportunistic
  posture governs.

## 12. Open questions (tactical)

1. **Declaration syntax.** Follows #7079's resolution (const table vs macro). Adopt whichever PDROP
   lands; decide the PVAC-side reader in α.
2. ~~Allowlist-as-baseline~~ — **pinned as D8 at decompose** (the allowlist is the baseline; no
   second ratchet file). Removed from the open set because it determines γ's same-diff
   registration obligations (ADV-γ-9).
3. **Whether `topology` becomes a declared `.ri` field** when its populated form arrives, or stays
   an engine-attached extra. The undeclared-write owner's call; C1′-2 accepts either exit.
4. **Discovery convention for producers not using the `u32::MAX` idiom** (registry-backed
   construction of `.ri`-declared result structures, if any exist). **Suggested resolution:** δ's
   sweep measures; if found, either declare them too or pin the idiom as mandatory for result
   producers. Decide in δ.
