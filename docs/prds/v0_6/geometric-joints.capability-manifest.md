# Capability Manifest — `geometric-joints.md` (joint half: `joint … with` + mount→FK offset)

**PRD:** `docs/prds/v0_6/geometric-joints.md` (committed `1d0a6e9153`) · **Built:** 2026-06-08 · **Author:** claude-prd-geometric-joints

Mechanizes the decompose-time **G3 + G6** check per the gates spec (`gates.md → Capability Manifest`) and the Reify overlay (`.claude/skills/prd/project.md → Capability Manifest — reify evidence forms`). One block per task; each capability the task's user-observable signal asserts is **bound to evidence**. Any binding resolving to a FAIL value (`declared-only` / `test-only` / `producer-absent` / `producer-downstream` / `fixture-ERROR` / `bound≤floor`) **blocks the whole α–ε batch** until resolved.

Greek labels = PRD §9 decomposition; task IDs assigned at file-time (see the batch summary at the foot). Code anchors are as-of-authoring hints — re-locate at implementation time (main moves fast).

## Verdict summary

| Task | ID | Kind | Bindings | Verdict |
|---|---|---|---|---|
| α | 4395 | intermediate (→β,γ) | grammar-fixture (producer) | **PASS** (α **is** the `joint…with` grammar producer; gr-05a/05b RED by design) |
| β | 4396 | intermediate (→γ) | producer-self · producer-upstream | **PASS** |
| γ | 4397 | intermediate (→δ) | producer-self · producer-upstream | **PASS** |
| δ | 4398 | intermediate (→ε) | wired-on-main (origin write) · branch-3 trace | **PASS** (every capability upstream; producer writes a real `Value::Transform`) |
| ε | 4399 | **leaf** (integration-gate) | branch-3 end-to-end trace | **PASS** (every capability upstream) |

**Field-population check: N/A** — no task's signal samples/reduces a *result* field (`result.stress`, `mode.shape`, …). The closest concern is δ's `origin` write: that is an **input**-field `Value::Transform` written on the production path (the relate-solved mount `Frame`, never `Undef`) — covered by δ's **wired-on-main** binding (the producer writes a real non-`Undef` value, KIN-OFFSET α's `transform_at` reads it), not the result-field empty-value sentinel. The `Value::Undef` twin does not apply.

**Numeric-floor check (G6 branches 1/2): N/A** — every number the signals assert is an **exact integer**: the DOF residual counts (`6 − Σ ΔDOF`, `1 rot + 1 trans`) are exact codimensions from the core's one DOF law `coincident(X,X) removes codim(X)` (design §3.4), and the self-checking law (β) is **exact integer count + kind matching** — there is no numerical method, no absolute-accuracy bound, hence no error floor to compare against. G6 branches 1 (numeric bound) and 2 (closed-form exactness) **do not fire**. Only branch 3 (end-to-end capability tracing) fires — applied to the δ + ε seam tasks below. (The solve/assertion tolerance that places the mount `Frame` is the kernel-defaulted coherence-law knob inherited from the core, PRD §7.2; no leaf signal asserts it as a fixed numeric premise.)

---

## α — Grammar production: `joint … with` definition syntax (single + record) — task 4395

**Signal:** fixtures `gr-05a` (single `joint…with…in range`) AND `gr-05b` (record `with { a: T, b: U }`) parse (`tree-sitter parse --quiet` exit 0) with parser tests in `tree-sitter-reify/tests/`; the lowered joint-definition node carries the declared DOF + body.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `joint NAME(d) with <name>: T in range = { body }` (single form) parses | `grammar-fixture:/tmp/prd-gate-fixtures/gr-05a-joint-with.ri` → `tree-sitter parse --quiet` **exit 1, 22 ERROR today** | **producer-self → PASS** |
| `joint NAME(d) with { a: T, b: U } = body` (record form) parses | `grammar-fixture:/tmp/prd-gate-fixtures/gr-05b-joint-with-rec.ri` → **exit 1, 17 ERROR today** | **producer-self → PASS** |
| The body lowers to a `Relation` conjunction | `producer:γ`-core (task 4383) upstream — the `Relation` type + relation vocab; wired α←4383 | PASS (upstream) |
| The `joint…with` grammar extends the `relate` grammar | `producer:δ`-core (task 4384) upstream — the `relate`/`at auto` grammar; wired α←4384 | PASS (upstream) |
| `range = 0deg..120deg` (dimensionally-typed) | `grep:crates/reify-stdlib/src/joints.rs:1290` (`fn validate_range`) — reused 1:1 | PASS (exists) |

**This is the batch's one `fixture-ERROR` row set, and it is the correct/expected state:** α **is** the named `joint…with` grammar-producer task (PRD §3 G3 table; overlay grammar-fixture rule: "parses with 0 ERROR nodes **OR** a named grammar-producer task is upstream" — here the named producer *is this task*). gr-05a/05b are α's RED fixtures; its deliverable is turning them GREEN (`tree-sitter parse --quiet` exit 0) plus `tree-sitter-reify/tests/` parser tests. Every task that *emits* `joint…with` syntax (β, γ, δ, ε) `depends_on` α, so no consumer asserts this grammar before α delivers it. The core's δ (4384) explicitly does **not** cover `joint…with` (core PRD §10) — that form is owned here.

**grammar_confirmed = false** — α is the joint-grammar producer (the only `false` in the batch).

---

## β — The self-checking law: definition-time DOF count + kind match — task 4396

**Signal:** `reify check` types a matched joint (B1); emits `E_JOINT_DOF_MISMATCH` with a geometric explanation on a count-mismatch (B2) and a kind-mismatch (B3), **before** any solve; a CI `.ri` example exercises pass + both fail modes.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| A `joint_signatures.rs` typing-family home exists | `grep:crates/reify-compiler/src/joint_signatures.rs` (exists); the landed `DRIVING_JOINT_KINDS` / `is_driving_joint` typing-family pattern `grep:crates/reify-stdlib/src/joints.rs:1385` | PASS (the family pattern is on main) |
| Relation **ΔDOF inference** to compute the body residual | `producer:γ`-core (task 4383) upstream — `relation_signatures.rs` publishes the nominal ΔDOF; wired β←4383 | PASS (upstream) |
| `joint…with` definitions to type-check | `producer:α` (4395) upstream — the grammar this consumes; wired β←α | PASS (upstream) |
| Dimensionally-typed `range` validation | `grep:crates/reify-stdlib/src/joints.rs:1290` (`validate_range`) | PASS (exists) |
| The self-check arithmetic (count + kind match → `E_JOINT_DOF_MISMATCH`) | `producer:β` — β's own deliverable: residual `= 6 − Σ ΔDOF`, kind-classify, multiset-match, emit the typed diagnostic | PASS (β is the producer; exact integer arithmetic, no numeric floor) |

**grammar_confirmed = true** (consumes α's joint grammar via the wired β←α edge).

---

## γ — Standard relate-defined joint library + couplings-on-the-scalar-side boundary — task 4397

**Signal:** a `.ri` example defines the standard joint set (revolute/prismatic/cylindrical/planar/spherical/ball) and `reify check` types each as a joint with the correct driving DOF (all self-checks pass); **B8** — a coupling in a `relate { }` body is rejected (type error / not in the relation vocab).

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `joint…with` grammar to author the library | `producer:α` (4395) upstream; wired γ←α | PASS (upstream) |
| The self-checking law to validate each library joint | `producer:β` (4396) upstream; wired γ←β | PASS (upstream) |
| Relation vocabulary (`coaxial`/`coincident`/`on`) for the bodies | `producer:γ`-core (task 4383) upstream; wired γ←4383 | PASS (upstream) |
| Driving-vs-non-driving enforcement (mechanism-completion) | `grep:crates/reify-stdlib/src/joints.rs:1385` (`DRIVING_JOINT_KINDS`) + `:1395` (`is_driving_joint`) — landed mechanism-completion enforcement | PASS (on main; reuse) |
| Couplings boundary — gear/screw/rack-and-pinion/`couple` are scalar-side, **not** `relate` | `producer:γ` — γ's deliverable is to **state + enforce** the boundary (a coupling in a `relate{}` body is a type error / absent from the relation vocab); couplings are **not implemented** here (design §8.4, PRD §10) | PASS (boundary, not a producer of couplings) |

**grammar_confirmed = true** (uses α's joint grammar, wired).

---

## δ — Mount→`origin` handshake machinery (the seam this PRD owns) — task 4398

**Signal:** **B5** — a relate-defined revolute over datums at a nonzero pivot has its solved mount `Frame` (coaxial within tol) written into the joint Map's `origin` (`Value::Transform`, nonzero translation), verified by a test reading the joint Map; **B9** — joints with no relate-placement carry no `origin` and the mechanism suite stays green (byte-identical).

**G6 branch-3 capability trace** — every capability δ's signal requires is delivered by δ itself or by a task **upstream** of δ (never downstream):

| Capability the mount→origin write requires | Producer | Direction | Verdict |
|---|---|---|---|
| A concrete relate-solved mount `Frame` (`SolveResult::Solved`) at the `Resolution` node | core ζ (task 4386) | δ←4386 (upstream) | PASS |
| The optional `origin` `Value::Transform` field on the joint `Value::Map` + `transform_at` threading | KIN-OFFSET α (task 4331) | δ←4331 (upstream) | PASS |
| A defined joint to mount (the library) | γ (task 4397) | δ←γ (upstream) | PASS |
| The `origin` field type matches the relate mount `Frame` (Frame3, no widening) | KIN-OFFSET decision §4.2 chose Frame3 **specifically** so geometric-joints consumes it without widening | design contract | PASS |
| **Producer writes a real `Value::Transform`** (wired-on-main / anti-orphan) | `producer:δ` — δ's deliverable is the write on the production path (relate-solve → joint-placement at the Resolution node → `joints.rs` `origin` key); KIN-OFFSET α's `transform_at` reads it. The mount `Frame` is the solved value, never `Undef`. | PASS (δ is the producer; the consumer — KIN-OFFSET α's FK threading — is upstream, so this is **not** an orphan-producer shape) |

No required capability is owned by a task that **depends on** δ — the anti-inversion (DAG-direction) check passes. The relate↔KIN-OFFSET reciprocal-ownership risk is resolved: KIN-OFFSET α (4331) **adds + threads** the field, δ **produces + writes** the mount frame into it — KIN-OFFSET α's `consumer_ref` already names this PRD's joint half (design §8.2).

**grammar_confirmed = true.**

---

## ε — End-to-end vertical slice: relate-defined joint mounted at a nonzero pivot sweeps via the mechanism (integration-gate leaf) — task 4399

**Signal (leaf — the consumer signal):** `reify build`/`eval` poses the link at the solved **nonzero** pivot (GUI mesh pose via debug MCP / CI example asserting the posed transform); the swept angle **== the mechanism's bind value, NOT re-solved geometrically** (B6); the closed-loop variant closes via Newton at the offset-aware residual (B7); the companion Rust e2e passes.

**G6 branch-3 end-to-end capability trace** — every capability ε's signal requires is delivered by ε itself or by a task **upstream** of ε (never downstream):

| Capability the e2e build requires | Producer | Direction | Verdict |
|---|---|---|---|
| A relate-defined joint (revolute) to mount | γ (4397) → δ (4398) | ε←δ→γ (upstream) | PASS |
| The mount `Frame` solved + written to the joint's `origin` | δ (4398) | ε←δ (upstream) | PASS |
| The per-scope relate-solve at the `Resolution` node (directly invoked at build) | core ζ (4386) | ε←4386 (upstream) | PASS |
| Offset-aware FK — `transform_at = origin ∘ motion` poses the link at the mount | KIN-OFFSET α (4331) | ε←4331 (upstream) | PASS |
| The motion variable stays the mechanism's `bind`/`sweep` value (not a solver residual) | mechanism subsystem (`snapshot.rs` `bind`/sweep) — **landed**; `auto(free)` returns a concrete value, never a free variable (design §8.1) | on main | PASS |
| The loop-closure Newton solver runs at snapshot for a closed motion loop (B7) | mechanism subsystem (loop-closure Newton / KCC) — **landed** (mechanism-completion + KCC done) | on main | PASS |
| The e2e example + boundary tests (B6, B7) | `producer:ε` — ε's own deliverable (the `examples/` `.ri` + `reify-eval/tests/` e2e) | this leaf | PASS (ε is the producer of its own integration) |

No required capability is owned by a task that **depends on** ε — the anti-inversion (DAG-direction) check passes. ε is the C-as-integration-gate leaf: α/β/γ/δ + the out-of-batch core ζ (4386) + KIN-OFFSET α (4331) are foundation/seam tasks roped into this leaf, satisfying the G2 escape hatch (the leaf's signal faces both producer and consumer).

**grammar_confirmed = true.**

---

## Batch summary (filled at file-time)

| Greek | Task ID | Title (abbrev) | Intra-batch prereqs | Out-of-batch prereqs |
|---|---|---|---|---|
| α | 4395 | Grammar: `joint…with` (single + record) | — | 4383 (core γ), 4384 (core δ) |
| β | 4396 | Self-checking law (DOF count + kind) | α | 4383 (core γ) |
| γ | 4397 | Standard joint library + couplings boundary | α, β | 4383 (core γ) |
| δ | 4398 | Mount→`origin` handshake (the seam) | γ | 4386 (core ζ), 4331 (KIN-OFFSET α) |
| ε | 4399 | E2E slice @ mechanism (integration-gate leaf) | δ | 4386 (core ζ), 4331 (KIN-OFFSET α) |

**13 `add_dependency` edges wired** (5 intra-batch per the §9 DAG — β←α, γ←α, γ←β, δ←γ, ε←δ — + 8 out-of-batch: α←{4383,4384}, β←4383, γ←4383, δ←{4386,4331}, ε←{4386,4331}). All bindings **PASS**; key convention = **"producer-self PASS"** (α's gr-05a/05b grammar-ERROR is NOT an orphan FAIL because α IS the named `joint…with` grammar producer; δ's `origin` write is the producer of the value KIN-OFFSET α's *upstream* `transform_at` consumes). Field-population N/A, numeric-floor N/A (DOF numbers are exact codim integers + exact count/kind matching → G6 branches 1/2 don't fire; only branch 3, applied to δ + ε). Out-of-batch dep statuses at decompose: 4383/4384/4386 **pending** (core γ/δ/ζ), 4331 **in-progress** (KIN-OFFSET α). The scheduler holds the whole batch behind the core relate substrate (4383/4384) and the seam layer (δ, ε) behind core ζ (4386) + KIN-OFFSET α (4331) — correct: the joint half is gated (design §11 step 9).

**Cross-PRD prose (dedup — NOT re-filed):** the core's companion 4389 (in-progress) and KIN-OFFSET κ already own pointing `docs/design/geometric-relations.md` §8.2 + the two PRDs at the relate↔KIN-OFFSET co-design seam. Now that this PRD exists, those tasks concretize their forward-pointers; no new prose task is filed here.
