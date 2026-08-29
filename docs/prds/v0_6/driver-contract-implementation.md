# Driver-contract implementation — one engine posture, one verdict contract

**Milestone:** v0_6 · **Status:** active — contract PRD · **Approach:** B + H

Authored 2026-08-26 in an interactive `/prd` session (Leo + Claude; groundwork by a
seven-agent team). Implements the **RULINGS (Leo, 2026-08-26 — matrix CLOSED)** section
of `docs/notes/driver-contract-matrix-draft.md`. Every cell that document rules is
decided; this PRD does not relitigate any of them. It is **Ring 2** of the
spec-conformance program (`docs/notes/conformance-scope-boundary-draft.md`): the
observables that must be constant across *this implementation's own drivers*, which a
different conforming implementation would legitimately ship differently.

**Code anchors** verified against main `9a992fc2f2` (2026-08-26), with runtime probes
run on `target/debug/reify` built the same day. Main moves fast — cite-by-symbol;
re-locate lines at implementation time.

---

## §1 — Goal

One engine posture, one verdict contract, one diagnostic egress — across every driver
that reads a `.ri` file.

Today reify has **17 hand-assembled engine-construction sites in production code**
carrying **12 distinct capability fingerprints**, and the choice of which capabilities a
driver gets is made independently at each site. The result is not a set of considered
role differences; it is an accretion in which `reify check` measures a DFM rule that
`reify eval` cannot see, `reify eval` prints a design's values without noticing that its
constraints are violated, and `reify report` emits a costed bill of materials for a part
that does not satisfy its own design intent.

After this PRD, capability reach is a **declared profile**, not a per-call-site
accident; every evaluating driver runs the constraint pass and maps its verdict to an
exit code by one shared rule; and a committed cross-driver gate proves that the same
file produces the same diagnostics through check, eval, build, test, report, explain,
the GUI and the LSP — with a **parity failure and a conformance failure reported as
distinguishable verdicts**.

### User-observable on landing

Each of these is **measured RED today** (§1.1 carries the command output):

- `reify eval` on a design with a violated non-geometry constraint exits **1** naming
  the constraint. Today: exit 0 with no constraint output whatsoever.
- `reify report --bom` on a violated design exits **1** instead of printing
  `Total: 0.42 USD` and a procurement-actionable line item. Today: exit 0.
- `reify eval` and `reify report` surface the two `E_DFM_*` errors that `reify check`
  already surfaces on the same file. Today: zero DFM diagnostics, exit 0.
- `reify test` on a geometry-dependent `@test` reports a real verdict, and an
  `Indeterminate` verdict exits **non-zero** unless the test carries
  `@allow_indeterminate`. Today: `test result: ok. 0 passed; 0 failed; 1 indeterminate`,
  exit 0.
- A file whose `module` header does not match its filename **squiggles in the GUI
  editor and in the LSP**, with a `DiagnosticCode` and a real span. Today: silent in
  both; every CLI subcommand exits 1 on the same file.
- The GUI diagnostics panel shows diagnostic **codes**. Today `code` is hard-coded
  `None` at one projection function and populated nowhere.
- `reify doc` returns exit **1** on a usage error, like every other subcommand. Today: 2.
- `reify <driver> --json` emits a machine-readable driver-result envelope — diagnostics
  with codes, spans and phase, plus constraint verdicts and the exit reason. Today no
  reify surface emits structured diagnostics on the CLI at all.
- The same fixture driven through all eight surfaces reports the same diagnostic set,
  asserted by a committed gate whose failure output says whether the divergence is a
  Ring-2 parity break or a Ring-1 conformance break.

### §1.1 — Committed evidence fixtures

Six fixtures land with this PRD under `tests/prd-gate/fixtures/`. Each parses with
**0 ERROR nodes** under `tree-sitter parse --quiet` and was executed at HEAD; the
baseline recorded in each file's header is the observed output, not a prediction.

| Fixture (all `tests/prd-gate/fixtures/`) | Pins | Measured baseline at `9a992fc2f2` |
|---|---|---|
| `driver_contract_eval_blind_to_violation.ri` | eval's violation-blindness (survey DV11/D2) | `check` exit 1 `VIOLATED Beam#constraint[0]`; `build` exit 1; **`eval` exit 0, no constraint output**; `explain` exit 0 |
| `driver_contract_report_blind_to_violation.ri` | report's violation-blindness | `check` exit 1 `VIOLATED Bolt#constraint[0]`; **`report --bom` exit 0**, prints the line item and `Total: 0.42 USD` |
| `driver_contract_dfm_measurement_arm.ri` | the measurement-arm gap (survey D3/DV7) | **`check` exit 1** with `E_DFM_MIN_WALL` + `E_DFM_MIN_FEATURE`; **`eval` exit 0, zero DFM diagnostics**; **`report --bom` exit 0, zero DFM diagnostics** |
| `driver_contract_geometry_test_indeterminate.ri` | `@test` Indeterminate ⇒ pass (survey D2) | `test` exit 0, `INDETERMINATE TestBlockVolume` / `test result: ok. 0 passed; 0 failed; 1 indeterminate` |
| `driver_contract_header_mismatch.ri` | module-header reach (survey D5) | all seven CLI subcommands exit 1 with `E_MODULE_PATH_MISMATCH`; GUI and LSP silent (code-read) |
| `driver_contract_allow_indeterminate.ri` | the ruled annotation spelling | `tree-sitter parse --quiet` exit 0; `check` and `test` emit `warning: unknown annotation @allow_indeterminate` — **the annotation parses and is loudly unregistered** |

None of the six is read by a compiled test target yet. When a leaf's test starts reading
one, that leaf must add the basename to `_RUST_COUPLED_RI_FIXTURES` (`scripts/verify.sh`)
in the same diff — `tests/infra/test_verify_scope.sh`'s PG-DRIFT scenario re-derives the
coupled set from `git grep` and turns RED until it does.

---

## §2 — Background

### §2.1 — What the matrix ruled

`docs/notes/driver-contract-matrix-draft.md` closed nine open cells on 2026-08-26. The
rulings this PRD implements, in the matrix's own numbering:

- **Ruling 2 — all-get-all.** One shared engine construction path, with *named ratified
  subtractions only*: the LSP keystroke-latency posture, and `doc` compile-only.
- **Ruling 3 — the constraint-verdict contract.** Every evaluating driver runs
  `check()`; `eval` and `report` gate exit on violation; **`explain` warns on check
  failure but never gates** (a ruled role difference); `@test` Indeterminate ≠ pass.
- **Ruling 5 — the module-header rule is enforced everywhere**, GUI and LSP included.
- **Ruling 7 — LSP cfg**: host-default by default, with an optional `cfg` map honoured
  in `initializationOptions`; specified now, wired when resolution-unification's LSP
  leaf lands.
- The **mechanical-alignment list**: the GUI `code: None` strip, the LSP real checker,
  GUI FEA-cache use, eval's OpenVDB wiring, and a CLI `--json` diagnostics mode.
- **RQ-9** (`conformance-scope-boundary-draft.md`) — normalize `reify doc` usage errors
  from exit 2 to exit 1.

Rulings 1, 4 and 6, and the solver halves of ruling 2, are owned elsewhere; §8 is the
full ownership table.

### §2.2 — The census: 17 sites, 12 fingerprints

Capability axes: **(a)** BRep kernel · **(b)** OpenVDB · **(c)** solver ·
**(d)** compute trampolines · **(e)** FEA persistent cache · **(f)** capture flags
(repr tolerance / tessellation) · **(g)** purpose activation.

| Driver / site | a | b | c | d | e | f | g |
|---|---|---|---|---|---|---|---|
| `cmd_check` kernel arm | ✓ | cond. `has_thickness_dfm` | — | — | — | cond. `has_representation_within` | — |
| `cmd_check` lightweight arm | — | — | — | — | — | — | — |
| `cmd_check --purpose` arm | — | — | — | — | — | — | ✓ |
| `cmd_test` (via `build_test_engine`) | — | — | — | ✓ (morph `Unavailable`) | — | — | — |
| `cmd_build` | ✓ | cond. `module_has_isosurface` | — | ✓ | — | — | — |
| `cmd_report` | — | — | ✓ | ✓ | ✓ | — | — |
| `cmd_eval` geometry branch | ✓ | — | ✓ | ✓ | ✓ | — | — |
| `cmd_eval` plain branch | — | — | ✓ | ✓ | ✓ | — | — |
| `cmd_explain` | — | — | ✓ | ✓ | ✓ | — | — |
| `cmd_doc` | no engine constructed at all |
| GUI production boot | ✓ | — | ✓ | ✓ | **—** (sweeps the cache, never installs it) | — | — |
| GUI def-preview engine | — | — | — | — | — | — | — |
| LSP × 4 sites | — | — | — | — | — | — | — |
| CLI `mcp-server` | — | — | — | — | ✓ | — | — |

Three observations drive the design.

**First, four of the seven axes are gated on module *content*, not on driver identity.**
`cmd_check` attaches a kernel when the module carries a `Conforms`, `RepresentationWithin`
or DFM rule; `cmd_build` attaches OpenVDB when the module carries an isosurface;
`cmd_eval` attaches a kernel when `module_has_geometry`. Those predicates are hand-rolled
at seven call sites. **That, not the capability list, is the real duplication** — and it
is why the divergences are content-dependent and therefore invisible until a user writes
the file that trips one.

**Second, two canonical mechanisms already exist and must be composed, not replaced.**
`Engine::register_production_compute_fns` landed under `INV-FEA-1` (compute-fea-hardening
task A1) and is guarded by a grep architecture test; it **panics on double registration**,
which makes single-ownership a correctness requirement rather than a style preference.
`Engine::run_measurement_pass` is chartered by `gui-on-demand-measurement`'s leaf α, which
extracts `cmd_check`'s kernel/capture/OpenVDB arm sequence and forbids a second copy.

**Third, `cmd_check`'s own posture rustdoc contains an internal contradiction of record.**
It states that "check attaches no kernel by design" a few lines above a branch that calls
`Engine::with_registered_kernel`. A posture that is documented in prose and contradicted
by the adjacent code is exactly what a declared profile makes impossible.

### §2.3 — The verdict contract is a return-type gap, not a missing call

`reify eval`'s violation-blindness is usually described as "eval doesn't run check". That
is not what the code does. On the geometry branch `cmd_eval` calls `engine.build()`, which
calls `self.check()` internally — so the verdicts *are computed* and then dropped, because
`cmd_eval` destructures only values, diagnostics and the engine. On the plain branch it
calls `engine.eval()`, and **`EvalResult` has no `constraint_results` field at all**, so
no verdict exists to drop. `cmd_report` and `cmd_explain` are on that same `eval()` path.

The same shape recurs one layer down for provenance: `Engine::check()` computes
`objective_provenance` and discards it at the `CheckResult` construction, because
`CheckResult` and `BuildResult` have no such field. That half is owned by
solver-legibility-telemetry's leaf α; this PRD depends on it and does the verdict half.

Uniform gating is therefore a **struct-shape change plus a routing change**, in both
cases — not the addition of a missing call. Any leaf specified as "make eval call
check()" would be specified against a false premise.

### §2.4 — What "silent" costs, measured

The four survey rows this PRD closes are each a case where a driver answers a question it
cannot actually answer. `reify report --bom` is the sharpest: it produces a costed line
item and a total for a design whose constraints `reify check` rejects, and exits 0. A
procurement workflow driven by `report` has no channel through which the violation could
reach it.

---

## §3 — Consumers (G1)

| Consumer | What it consumes | Status |
|---|---|---|
| **Every reify user** — the CLI/GUI/LSP consistency guarantee | The verdict contract and the capability profile: the answer to "is my design OK?" stops depending on which command asked. The GUI is the primary use case. | live |
| **The spec-conformance suite** | This contract *is* its Ring-2 substrate (`conformance-scope-boundary-draft.md` §"Ring 2"), and the `--json` envelope (leaf σ) is its named prerequisite. Ring-1 clauses are tested *through* Ring-2 drivers, so the parity/conformance verdict split (leaf ψ) is what makes its results interpretable. | chartered, not yet authored *(landed 2026-08-26: `docs/prds/v0_6/spec-conformance-suite.md`, `a35b11740a`; decomposed #6758–#6791)* |
| **`reify report` / BOM consumers** | A bill of materials that cannot be emitted for a design that fails its own constraints. | live |
| **The GUI diagnostics panel and the debug-MCP observation channel** | Diagnostic codes (leaf μ), so panel filtering and the parity gate can key on codes instead of message substrings. The wire contract is `reify_core::DiagnosticInfo`, owned by the landed gui-diagnostics-panel PRD. | live |
| **`gui-on-demand-measurement`'s parity gate** | Its BT-1 asserts GUI measurement ≡ `reify check`. That oracle is only meaningful once "what `check` measures" is a declared profile rather than a content-gated accident. | chartered (#6740–#6744) |
| **In-engine seam** (`engine-integration-norm.md` §3) | The profile plugs in at **§3.4 ComputeNode dispatch** (trampolines) and **§3.3 multi-kernel dispatch** (kernel + OpenVDB selection). It introduces no new seam. | — |

---

## §4 — Contract (B + H)

Blast radius: `reify-eval`, `reify-cli`, `reify-compiler`, `reify-core`, `reify-lsp`,
`gui/src-tauri`, plus verify infra — six crates, well over eight mechanisms, and the
FEA/ComputeNode and grammar-adjacent seams are on the G5 load-bearing list. B + H it is.

### §4.1 — I1: capability reach is declared, never inferred at a call site

Every engine that drives a `.ri` file is constructed by naming a **profile**. The profile
is a value, not a convention: a driver cannot acquire or lose a capability by editing a
construction expression, and no construction path yields a capability set that no profile
names.

This **composes** solver-driver-parity's `ResolutionProfile`; it neither widens it nor
parallels it. The distinction is load-bearing and was corrected on 2026-08-27 — the
original text here said "widen", which would have broken the very invariant it cited.

`ResolutionProfile` is deliberately **two axes — iteration budget and staleness — with no
stage-decline field**, and its totality is doing real work: I5 says *"a profile may not
decline a stage"*, and that is what keeps `INV-SF-4`'s doctrine ("a solve that never ran"
is an unexpected cause plain `reify check` must fail on) applicable **without amendment**.
Widening that type to carry kernels and trampolines would have to add exactly the
stage-decline field it forbids — because §4.3's two ratified subtractions *are* declines —
and would cost that reasoning.

So the layering is: a **capability profile** carries the six non-solve axes and the two
ratified subtractions, and **has a `ResolutionProfile`** for the solve axis. `I5` stays
literally true and total at the resolution layer; capability subtractions live one layer
up, where they are Leo-ratified and locked by tests. This is still one construct and one
architecture gate — the thing §6 decision 1 actually cares about — with no second
solver-posture type anywhere.

The two ratified subtractions are consistent with I5 read at its own layer: the LSP
declines **compute trampolines**, not a solve stage (solver-driver-parity's own LSP leaf
*gives* the LSP a budgeted solver), and `doc` constructs no engine at all, so it has no
resolution profile to decline anything with.

**Consequence for that leaf's open question.** solver-driver-parity's §13 asks whether
`ResolutionProfile` lives in `reify-ir` or `reify-eval`, and suggests `reify-ir` so that
`reify-lsp` and `gui/src-tauri` can name a profile without depending on engine internals.
Under composition that suggestion is simply **right and unaffected** — `ResolutionProfile`
stays where that leaf puts it. **This PRD's answer: keep `reify-ir`, and keep the type
capability-*declarative*** — it names axes and settings, never engine types — so the
capability profile can sit beside it in `reify-ir` and `reify-eval` owns the
interpretation (which kernel object, which registrar). This must reach that leaf before
it dispatches (§7).

### §4.2 — I2: the shared constructor composes; it does not re-implement

The profile-taking construction path calls, and is the **only** caller of:

- `Engine::register_production_compute_fns` — the landed `INV-FEA-1` bundler. Single
  ownership is a correctness requirement: the bundler panics on double registration.
- `Engine::run_measurement_pass` — the arm sequence extracted by gui-on-demand-measurement's
  leaf α (kind detection → capture flag → handle-populating build → tessellation →
  OpenVDB). The measurement arms are **not** re-extracted here; that PRD's D1 forbids a
  second copy and this PRD honours it.
- The kernel selection already shared by every kernel-using driver.

The seven hand-rolled content predicates (`module_has_geometry`, `module_has_isosurface`,
`module_has_thickness_dfm_rule`, `module_has_representation_within`, and the
`Conforms || ReprWithin || DFM` disjunction) move **out** of the driver call sites and
**into** the construction path, which sees the compiled module. This is where "all get
all" actually bites: a driver stops choosing which content-gated capability it wants.

**`cmd_check` is the exception, and the exception is not optional** (corrected
2026-08-27). Check's content predicates *are* its kernel routing, and check's kernel
routing is reserved to check-diagnostic-truthfulness by a binding G4 ruling (§8.1) — so
"move the predicates" cannot mean check's without this PRD breaking its own seam table.
What actually happens to check is a three-step hand-off this PRD is the last step of:
its kernel routing is rewritten by that PRD's in-flight leaf; the GUI purpose PRD's
`activate_purpose_session()` leaf then makes **one** body serve both check arms,
behaviour-preservingly; and only then does leaf α make check *construct from a profile*
like every other driver, adopting the already-unified arm sequence rather than
re-deriving it. Leaf α carries real dependency edges on both upstreams. The predicate
unification α owns outright is the other six drivers'.

Enforcement is the existing grep architecture test
(`scripts/check-compute-trampoline-registration.sh`, wired into the verify manifests),
widened from the trampolines axis to the construction path — not a new gate.

### §4.3 — I3: exactly two subtractions, each named, documented and locked

The profile admits precisely two non-full constants, each carrying a doc comment citing
the 2026-08-26 ruling and each pinned by a locking test:

1. **LSP keystroke posture.** State the subtraction at the precision the code actually
   holds: it is the **compute-trampoline** subtraction — Leo-ratified under
   compute-fea-hardening ("keystroke-time FEA solves in the LSP — rejected outright, not
   merely deferred"), already documented and already locked by
   `fea_bearing_constraint_produces_no_false_violation_or_false_pass`. It is **not** a
   solver subtraction: solver-driver-parity's leaf η is *adding* a budgeted solver to the
   LSP. "LSP keystroke-latency posture" as a blanket phrase over-claims; the profile
   names the axes individually.
2. **`doc` compile-only.** `cmd_doc` constructs no engine at all and evaluates nothing.
   Ratification, not mechanism.

A third subtraction may not be added by a code change alone: it requires a profile
constant, a doc comment naming the ruling, and a locking test — the same three artifacts.

Both subtractions live at the **capability** layer, never inside `ResolutionProfile`
(§4.1). Neither declines a resolution stage, so solver-driver-parity's I5 remains total.

### §4.4 — I4: staleness is a declared axis, not an attached default

solver-driver-parity's I2 deliberately keeps the persistent cache out of default
construction, calling it *"a staleness axis a driver must opt into deliberately"*. The
matrix ruling lists the FEA persistent cache among the shared constructor's capabilities.
These are reconciled, not in conflict: **the profile carries a staleness field; the FULL
constant enables it; every driver's profile names its value explicitly.** Both "all get
all" and "declared axis" hold, and no driver acquires cache-served staleness silently.

A served-stale result stays marked as such, per that PRD's I5. This PRD sets the per-driver
values (build, report, explain and the GUI gain the cache); it does not redefine the axis.

### §4.5 — I5: every evaluating driver produces a verdict, and one rule maps it to an exit

An **evaluating driver** is one that constructs an engine and evaluates a module: check,
eval, test, build, report, explain. (`doc` is excluded by I3.)

Each obtains constraint verdicts — which requires the return-type work of §2.3 — and folds
them through the **existing shared machinery** (`ConstraintOutcome`,
`report_constraint_results`, `check_fails` / `build_is_success`, `finish_check`,
`report_eval_output`). No driver grows a private verdict fold.

The ruled per-driver mapping:

| Driver | runs `check()` | Violated | Indeterminate |
|---|---|---|---|
| check | yes | exit 1 | exit 0; exit 1 under `--strict` |
| eval | yes | **exit 1** (today: 0) | exit 0 |
| build | yes | exit 1 | exit 0 |
| report | **yes** (today: never) | **exit 1** (today: 0) | exit 0 |
| explain | **yes** (today: never) | **warn, exit 0** — ruled role difference | warn, exit 0 |
| test | yes | exit 1 | **exit 1** unless `@allow_indeterminate` (today: 0) |

`explain`'s exemption is a ruled role difference, not an oversight: `explain` answers
"why did the solver choose this?", a question that remains meaningful — arguably *more*
meaningful — for a design that does not satisfy its constraints. Because it is the single
exception to an otherwise total rule, it gets its own two-way boundary test (§5 BT6): one
direction asserting the warning is emitted, the other asserting the exit code stays 0.

**`explain`'s diagnostic is `Warning`-severity, and that is load-bearing, not cosmetic.**
`INV-SF-2` (`error-severity-exits-nonzero`) says any Error-severity diagnostic on any
channel makes the command exit non-zero, with the corollary that *"a diagnostic expected
on a healthy path is by definition not Error-severity — demote or recode it; never exempt
it from the gate."* A ruled warn-but-never-gate role is exactly that corollary's case: had
leaf κ emitted an Error and returned 0, it would have created the per-command exemption
the invariant forbids, and `explain` already gates on Error-severity diagnostics today.
Warning-severity keeps the exemption unnecessary. Recorded here because "warns" alone
would have left an implementer free to pick the severity that breaks the invariant.

**Orthogonality, stated so a reader does not have to derive it.** This invariant governs
gating on **constraint violation**. It is independent of the open question of which
**Error-severity diagnostics** escalate `reify check`'s exit code — a live collision
between two tasks with opposing designs, already escalated on one of them, and declined by
both solver-driver-parity and solver-legibility-telemetry. *(Resolved 2026-08-26, ruled by
Leo: #5403 delivers the gate — its `CHECK_ERROR_EXIT_ALLOWLIST` is a bounded migration
ratchet burned to zero by #5404 — and #6608 now depends on #5403 (edge wired), contributing
only its new UndefCause variant + coded diagnostics. Mirror records in both tasks' details;
see solver-driver-parity §8.)* This PRD depends on neither and
does not resolve it. Once both land, `reify eval` on a violated design exits 1 for the
violation, whichever way that question goes.

### §4.6 — I6: a diagnostic's code survives every egress

Every surface that renders a diagnostic to a consumer carries its `DiagnosticCode`. The
GUI's projection function hard-codes `code: None` while reading severity, message and
labels off the very same value — a lossy projection, not an upstream absence. The LSP's
converter is the non-lossy precedent to copy.

This is `INV-SF-6` (`diagnostics-carry-codes`) applied at the egress rather than the
construction site, and it is load-bearing for the parity gate for a specific reason: the
in-tree two-way divergence ledger this PRD adopts (§4.7) matches allow-entries **by
diagnostic code**, and its own contract says a code-less diagnostic *"can never be
matched … it always surfaces as an unreasoned divergence. That is intended."* A parity
gate over a code-stripped surface would fail permanently and uninformatively.

The module-header diagnostics are the same class from the other end: they carry neither a
code nor a span, with their `E_`/`W_` mnemonics baked into message text. Leaf ν fixes
that before leaves ξ and ο route them to editors, because a squiggle needs a span.

### §4.7 — I7: divergence is either absent or ledgered, and the two failures are distinguishable

The cross-driver gate asserts **diagnostic-set equality** across all eight surfaces and
**exit-code equality** within the classes I5 defines. Reify has three in-tree precedents
for machine-checked sanctioned divergence; this PRD adopts the strongest — the two-way
ledger in `crates/reify-eval/tests/common/differential.rs`, whose contract fails both on
an *unmatched divergence* (an unreasoned difference) and on a *stale allow-entry*
(cover for something that no longer diverges). A one-way allowlist rots; a two-way one
cannot.

The gate reports two distinguishable verdicts:

- **Ring-2 parity failure** — the drivers disagree with each other. The contract broken is
  this document.
- **Ring-1 conformance failure** — the drivers agree, and agree on the wrong answer. The
  contract broken is the language spec.

The distinction is mechanical, not editorial: parity is a cross-surface comparison,
conformance is a comparison against an expectation. A single gate that reported only
"failed" would send every Ring-1 defect to a Ring-2 owner.

---

## §5 — Boundary-test sketch (B + H, two-way)

Each row faces both directions: the *forward* assertion is the capability, the *reverse*
assertion is the thing that must not silently regress.

| ID | Boundary | Fixture | Forward | Reverse |
|---|---|---|---|---|
| BT1 | eval ↔ verdict contract | `driver_contract_eval_blind_to_violation.ri` | `reify eval` exits 1 naming `Beam#constraint[0]` | an Indeterminate-only design still exits 0 under eval (Indeterminate is not promoted by this PRD outside `test`) |
| BT2 | report ↔ verdict contract | `driver_contract_report_blind_to_violation.ri` | `reify report --bom` exits 1 and emits no procurement-actionable BOM | a *satisfied* BOM design still prints the identical table and total — the report itself is unchanged |
| BT3 | measurement arm ↔ driver reach | `driver_contract_dfm_measurement_arm.ri` | eval and report emit the same `E_DFM_MIN_WALL` + `E_DFM_MIN_FEATURE` pair check emits | a module with no DFM rule attaches no OpenVDB kernel — the content gate still gates, it merely lives in one place |
| BT4 | test ↔ Indeterminate posture | `driver_contract_geometry_test_indeterminate.ri`, `driver_contract_allow_indeterminate.ri` | a geometry `@test` returns a real verdict; an Indeterminate one exits non-zero | `@allow_indeterminate` exits 0 **and** the annotation is registered, so the `unknown annotation` warning is gone — both halves, or the leaf has only renamed the failure |
| BT5 | module header ↔ editors | `driver_contract_header_mismatch.ri` | GUI and LSP both show a coded, spanned diagnostic on the `module` line | a *correct* header produces no diagnostic in either, and imported-module header checking (owned elsewhere) is unchanged |
| BT6 | explain ↔ the ruled exemption | `driver_contract_eval_blind_to_violation.ri` | `reify explain` prints a warning naming the violated constraint | `reify explain` still exits **0** — the exemption is asserted, not assumed |
| BT7 | GUI diagnostics ↔ codes | any compile-error fixture | the GUI panel and the GUI MCP context both carry the code | the two hand-synthesised codes (`unresolved-source`, `hot-reload-error`) still appear and are not overwritten by the newly-populated field |
| BT8 | LSP ↔ real checker | a constant-constraint fixture | the LSP agrees with `reify check` on a constraint the stub checker calls ambiguous | the LSP still registers no compute trampolines — the ratified subtraction survives the checker change |
| BT9 | `--json` ↔ the parity gate | the whole fixture set | every driver's envelope round-trips and carries codes, spans, phase, verdicts and exit reason | human stdout is byte-unchanged when `--json` is absent |
| BT10 | parity gate ↔ verdict split | a deliberately-divergent probe | an injected cross-surface divergence reports **Ring-2 parity**; an injected wrong-but-agreed answer reports **Ring-1 conformance** | a stale ledger entry fails the gate — cover for a divergence that no longer exists is itself a failure |

BT8's forward direction is the one place a naive signal would be vacuous, and §12 records
why: injecting the real checker is a **no-op at compile time except for constant
constraints**, pinned in-tree from both sides. The fixture must be a constant constraint
or the leaf is fake-done.

BT10's reverse direction is the anti-vacuity control for the gate itself.

---

## §6 — Resolved design decisions

1. **Build the capability profile that composes (has-a) the resolution profile; do not mint
   a parallel construct.** *(Corrected 2026-08-27: was "extend" — per §4.1 as repaired by
   `271e76a577`, the capability profile composes `ResolutionProfile`; it neither widens nor
   replaces it.)* Three independent
   architecture gates for one property is the failure mode, not the fix. The profile type
   stays capability-declarative and lives where solver-driver-parity's own open question
   suggests (`reify-ir`); `reify-eval` interprets it. (§4.1)

2. **The bundler stays a method on `&mut Engine`.** compute-fea-hardening rejected
   `Engine::new_production() -> Self` after analysis — CLI and GUI build the underlying
   engine differently before wiring compute fns, and a fresh top-level constructor cannot
   express that variance without duplicating each kernel-construction path. This PRD does
   not reopen that. The "shared constructor" is a profile-taking *path* that calls the
   method; it is not a replacement for it.

3. **Content predicates move into the construction path, not into the profile.** A profile
   says "this driver measures"; the construction path decides, from the compiled module,
   which kernel that actually requires. Putting `module_has_isosurface` in the profile
   would make the profile per-file rather than per-driver.

4. **Staleness is a declared axis with a FULL default** (§4.4) — the reconciliation of the
   matrix ruling with solver-driver-parity's I2, not a choice between them.

5. **`@allow_indeterminate` as a sibling annotation, not `@test(allow_indeterminate)`.**
   Ruled by Leo, 2026-08-26, on measured evidence: `@test(...)` arguments are silently
   discarded today — the schema entry declares no args and no `arg_check`, and the
   extra-args policy is dead code — so `@test(alow_indeterminate)` would silently select
   the *opposite* posture. The sibling spelling already emits `unknown annotation` for a
   typo, so it is loud on day one. **No grammar work is required for any spelling**
   (all three probed clean); the work is a schema registry entry and a runner consumer.
   The silent-`@test`-argument hole is real but is **not** closed here — §11 names it.

6. **`--json` carries a full driver-result envelope**, not diagnostics alone: codes, spans
   and phase, plus constraint verdicts and the exit reason, on every `.ri` driver. Ruled
   by Leo, 2026-08-26. This makes the parity gate an object comparison rather than text
   scraping, and gives the conformance suite one schema spanning the Ring-1 rejection
   surface and the Ring-2 exit contract. `reify doc --format json` is the in-tree
   precedent for structured stdout; `reify_core::DiagnosticCode` already round-trips
   through serde in PascalCase, which is the wire form.

7. **`reify-cli` enables `reify-core`'s `serde` feature explicitly.** It compiles with
   serde today only through feature unification via its `reify-lsp` / `reify-mcp`
   dependencies. Leaf σ must not rest on that: a dependency reshuffle would break the
   `--json` build for reasons unrelated to `--json`.

8. **Adopt the two-way divergence ledger, not a one-way allowlist** (§4.7). Reify has
   three precedents; the two-way one is the only one that fails on stale cover.

9. **Extend the existing cross-driver harness; do not build a fourth.**
   solver-driver-parity's leaf φ already drives one fixture corpus through six surfaces —
   it has the surfaces and the wrong object (resolution equality). resolution-unification's
   leaf η has the right object (diagnostic-set equality) across three surfaces and
   deliberately excludes exit codes under a recorded G7 waiver. The residue is the union:
   the diagnostic-set object across the full driver set, the LSP surface (which neither
   covers for diagnostics), exit-code equality, and the verdict split.

10. **A new standalone test binary in `reify-cli` is forbidden.** The harness-layout
    ratchet supersedes grandfathering (Leo, 2026-07-22); the sanctioned remedy is a
    `harness_<subsystem>/` compile unit. Leaves adding CLI tests extend
    `crates/reify-cli/tests/harness_cli/`.

11. **This PRD does no work on `reify mcp-server`.** Its deletion is ratified. Its
    code-stripping, no-prelude compile and hardcoded constraint status are excluded
    wholesale, matching every sibling PRD.

---

## §7 — Pre-conditions for activating

Two are hard, one is a coordination obligation with a deadline.

1. **solver-driver-parity's leaf α must land before leaf α here** (real edge). It defines
   the profile type this PRD's capability profile composes (has-a) *(corrected 2026-08-27:
   was "widens" — §4.1 / `271e76a577`)*. Landing them in the other order means building the
   capability profile around a type
   that five driver leaves have already been written against.

2. **§4.1's placement answer must reach that leaf before it dispatches.** Its §13 open
   question 1 asks where `ResolutionProfile` lives and says "decide during α". If α
   decides in ignorance of this PRD's composition over it *(corrected 2026-08-27: was
   "widening" — §4.1 / `271e76a577`)*, the decision is made on a stale premise.
   This is a **message, not a dependency** — it costs one task-description amendment and
   must happen at this PRD's decompose, not at leaf α's dispatch.

3. **solver-legibility-telemetry's leaf α must land before leaf ζ here** (real edge). It
   adds the provenance field to `CheckResult` / `BuildResult`; without it `explain` on a
   kernel-bearing path still reads an empty map. §12 records why the matrix's own cite for
   this gap is wrong.

4. **gui-on-demand-measurement's leaf α must land before leaves γ and δ** (real edge). It
   extracts `Engine::run_measurement_pass`; those leaves call it rather than re-extracting
   the arm sequence, which that PRD's D1 forbids.

**Promoted from soft to hard, 2026-08-27.** check-diagnostic-truthfulness's
kernel-widening leaf is **in-progress** *(status note 2026-08-27: #5748 is
pending/unclaimed at merge-phase; statuses in PRD prose rot — see the task store)* and
edits `crates/reify-cli/src/main.rs` — the
same file several leaves here touch, and its declared file set also includes the test
file holding the FEA lock that solver-driver-parity's leaf δ inverts. **Four** PRDs now
converge on that file: add the GUI purpose PRD, whose seam leaf rewrites `cmd_check`'s
body. For leaf α that convergence is not merely contention but a correctness ordering —
α adopts the unified body those two leaves produce — so both are now real edges, not
prose. The remaining leaves that touch `main.rs` are ordered behind it rather than
racing it.

---

## §8 — Cross-PRD relationships (G4)

### §8.1 — Owned elsewhere: depend, do not duplicate

| Capability | Owner | Edge |
|---|---|---|
| `check` gains the solver **and** the FEA trampolines; the `cmd_build` posture comment and `cmd_check`'s posture rustdoc are deleted; **`check_fea_violated_constraint_is_not_gated` is inverted** | solver-driver-parity δ | hard — several leaves here observe the post-flip behaviour |
| `build` regains the solver | solver-driver-parity δ | hard |
| `test` gains the solver and **prints the dropped `TestResult.diagnostics`** | solver-driver-parity ζ | hard for leaves ε and λ |
| LSP gains a budgeted solver with marked staleness | solver-driver-parity η | reference — leaf α must not describe the LSP as solver-free |
| the GUI's two internal solver-free evaluators | solver-driver-parity ι | reference |
| the cross-driver **resolution**-equality harness | solver-driver-parity φ | hard for leaf χ, which extends its target |
| `objective_provenance` on `CheckResult` / `BuildResult` | solver-legibility-telemetry α | hard for leaf ζ |
| `Engine::run_measurement_pass` (the arm-sequence extraction) | gui-on-demand-measurement α | hard for leaves γ and δ |
| GUI OpenVDB kernel registration | its own baseline task | hard for the GUI half only; this PRD owns the eval/report/explain half |
| GUI η export refusal · GUI viewport at-auto poses · delete `reify mcp-server` | their own tasks | reference |
| `check`'s kernel-arm widening, `finish_check`, check's exit codes, `--strict` | check-diagnostic-truthfulness β, and its exit-gate leaf | **binding G4 ruling reserves them** — leaves here must not touch them. **This binds leaf α too** (§4.2): α makes check construct from a profile, it does not rewrite check's routing, and it is ordered behind that leaf by a real edge. |
| the single `cmd_check` body — one arm sequence serving both the plain and `--purpose` arms | GUI purpose PRD's `activate_purpose_session()` leaf | hard for leaf α, wired 2026-08-27. Deliberately **behaviour-preserving**: it unifies the body without changing routing, exit codes, diagnostics or `--strict`. α adopts its result. |
| multi-file / cfg plumbing, `compile_program`, the GUI twin deletion, LSP multi-file diagnostics | resolution-unification β/γ/δ/ε/ζ | hard for leaves ξ, ο, υ |
| imported-file module-header mismatch surfacing | resolution-unification ξ | reference — the *import* axis, disjoint from this PRD's *driver* axis |
| the shared `activate_purpose_session()` seam | the GUI purpose PRD's seam leaf | hard for leaves φ **and α**; see §8.3 |
| `crates/reify-mcp/src/tools/chunks/purposes.md` — the purposes chunk | the GUI purpose PRD's docs leaf | hard for leaf ω, wired 2026-08-27. That leaf rewrites the chunk and documents `--purpose` **as it stands when it runs** — check and the GUI. φ then widens the flag to five more drivers, which makes that one statement stale; ω corrects exactly that statement on top of their rewrite and touches nothing else in the file. Same for the `reify-design` index line: extend theirs, never add a competing one. |

### §8.2 — Corrections this PRD owes to committed documents

Landed in this PRD's own authoring commit, following the precedent of the P3 cross-PRD
correction commit:

1. **compute-fea-hardening**, §"Sketch of approach" 1 — asserts that `cmd_check`'s
   trampoline-free opt-out *"needs **no change** — it is already documented and already
   locked."* Ruling 2 reverses it. Amend in place, dated, naming the delivering leaf.
2. **check-diagnostic-truthfulness**, §"What must NOT change" — asserts the FEA
   trampoline-free design intent of `check` is preserved. Same reversal, same treatment.
3. **`driver-contract-matrix-draft.md`** ruling 2's parenthetical provenance cite, and
   ruling 7's claim that the LSP cfg baseline is host-default. Both are wrong; §12 has the
   measurements.

The `reify doc` exit-code convention is stated in the reify-doc PRD; that correction lands
**with leaf τ**, same-diff, because unlike the three above it is not yet true.

Together these close a live three-way contradiction: three committed PRDs currently hold
three positions on the same locked test.

### §8.3 — The `--purpose` seam

Matrix ruling 4 charters the GUI purpose surface and states that the other CLI drivers
gain `--purpose` "in the flag-unification wave" — this PRD is that wave. The shared
`activate_purpose_session()` seam is owned by the GUI purpose PRD, authored in a parallel
session on the same day. At this PRD's decompose: if that session has filed its seam leaf,
leaf φ wires a real dependency edge to it; if not, φ is filed **deferred** with the seam
named in its text, and whichever session lands second wires the edge. Leo, 2026-08-26.

**DISCHARGED 2026-08-27.** That PRD landed and decomposed; its seam leaf exists. φ's real
edge is wired and φ is `pending`. Two consequences a reader should not have to derive:
the seam is **behaviour-preserving for `cmd_check`**, so φ inherits a callable unified
body and **not** a fixed `--purpose` arm; and that PRD's own investigation recorded, with
executed evidence, that `reify check --purpose` currently exits 0 on a file plain
`reify check` rejects with a `RepresentationWithin` violation. That false green is
**not** closed by the seam.

**Ownership of that false green, traced 2026-08-27 and assigned to leaf α.** The first
reading — capability half to α, routing half to check-diagnostic-truthfulness — was too
generous to both. Tracing what the fix actually touches once the seams land collapses it:
every arm involved (kind detection, `set_capture_repr_tol`, the handle-populating
`build()`, `tessellate_realizations`, `ensure_openvdb_kernel`) lives **inside**
gui-on-demand-measurement's `run_measurement_pass`, which kind-gates internally — so there
is no per-arm work at all. And the exit half needs nothing either: once measurement runs
the constraint is `Violated` rather than `Indeterminate`, so check's existing violation
gate fires unaided, and that PRD's non-strict Indeterminate-is-pass policy never has to
move. What remains is **one call-site condition** — whether the unified check body invokes
the measurement seam on the purpose path.

That is not a `cmd_check` edit: after the seam extraction it is inside `reify-eval`, so the
binding G4 reservation (which covers `cmd_check`, `finish_check`, exit codes and
`--strict` — all CLI-side) does not reach it. It **is** a §4.3 violation: a body that
declines the measurement capability on a purpose flag is an unnamed third subtraction,
carrying none of the three artifacts §4.3 requires. So it is leaf α's, as an explicit
acceptance clause rather than a new task — which also gives α a falsifiable behavioural
signal where it previously had only a structural one. The gui-purpose PRD's docs leaf has
been redirected accordingly and files nothing unless the defect survives all three
upstreams. φ must still not assume it has landed.

### §8.4 — A message this PRD owes a sibling leaf

solver-legibility-telemetry's leaf ξ is about to decide against adding a machine-readable
CLI mode, on the stated premise that no non-GUI agent consumer exists. **This PRD is that
consumer** (decision 6, and the conformance suite behind it). The premise must be
corrected in that leaf's text at decompose time, or a live task will decide correctly
against a fact that has changed.

---

## §9 — Sketch of approach

Four phases, ordered by what unblocks what.

**Phase 1 — the profile (leaf α).** Build the full capability profile that composes (has-a)
the resolution profile *(corrected 2026-08-27: was "widen … into" — §4.1 / `271e76a577`)*;
move the seven content predicates into the construction path; compose the two
existing canonical mechanisms; widen the existing grep architecture test; update the
invariant registry row same-diff. Nothing else can start until the profile exists.

**Phase 2 — reach (leaves β–ζ).** Each driver adopts its profile: `build` gains the cache;
`eval`, `report` and `explain` gain kernel routing and the measurement arm; `test` gains
the BRep kernel; `explain` moves to the kernel-bearing path once the provenance field
exists. These are independent of one another and fan out.

**Phase 3 — the verdict contract (leaves η–λ) and the surfaces (leaves μ–φ).** The verdict
chain is sequential at its head — the return-type work (η) gates the three gating leaves —
then fans out. The surface leaves are almost entirely independent of each other and of
Phase 2; the one internal ordering is ν (code + span on the module-header diagnostics)
before ξ and ο (routing them to the GUI and the LSP).

**Phase 4 — the gates and closure (leaves χ–Ω).** The parity gate consumes `--json` and
the whole of Phases 2 and 3; it is the integration gate for this PRD. Then the docs-truth
obligations and the terminal stamp.

The critical path is α → (Phase 2 fan-out ∥ η → gating leaves) → χ → ψ → Ω. Everything
else is parallel.

---

## §10 — Decomposition plan

Twenty-five leaves, decomposed 2026-08-26; task IDs stamped below and in the manifest
sidecar. Rows carry `#NNNN` and deliberately say nothing about task *status* — the id is
immutable and queryable, a status word rots the moment the task moves.

**φ (#6804) was filed `deferred`** because the GUI purpose PRD had not yet filed its
`activate_purpose_session()` seam leaf. **Resolved 2026-08-27**: that leaf is #6803, the
real edge is wired, and φ is `pending`. Ω (#6808) is no longer blocked on a park.

### Phase 1 — the profile

- **α (#6773) — Capability profile + profile-taking construction path.** Modules: `reify-ir`
  (the profile type), `reify-eval` (interpretation), plus every construction site.
  Builds the capability profile that composes (has-a) solver-driver-parity's
  `ResolutionProfile`, carrying the seven axes *(corrected 2026-08-27: was "widens … to the
  seven axes" — §4.1 / `271e76a577`)*; adds the `FULL`,
  `LSP_KEYSTROKE` and `DOC_COMPILE_ONLY` constants with doc comments citing ruling 2;
  moves the content predicates into the construction path; composes
  `register_production_compute_fns` (sole caller) and the kernel selection; widens the
  grep architecture test; updates the `INV-FEA-1` registry row from the trampolines axis
  to the construction path, **same diff**. *Intermediate.* Signal: the workspace compiles
  with no engine-construction path that names no profile, and the widened architecture
  test fails on an injected undelegated site. Depends: solver-driver-parity α;
  check-diagnostic-truthfulness's kernel-widening leaf and the GUI purpose PRD's seam
  leaf — both added 2026-08-27, because α adopts `cmd_check`'s unified body rather than
  re-deriving its routing (§4.2).

### Phase 2 — per-driver reach

- **β (#6775) — `build` gains the FEA persistent cache.** Modules: `reify-cli`. Survey DV8: build
  re-solves what eval has cached. *Leaf.* Signal: a second `reify build` of an
  FEA-bearing design reports a persistent-cache hit where it reports a miss today.
  Depends: α.
- **γ (#6777) — `eval` adopts the full profile.** Modules: `reify-cli`, calling
  `Engine::run_measurement_pass`. Closes survey DV7 (eval never attaches OpenVDB) and the
  kernel-routing half. *Leaf.* Signal: **BT3 forward** — `reify eval
  tests/prd-gate/fixtures/driver_contract_dfm_measurement_arm.ri` emits
  `E_DFM_MIN_WALL` and `E_DFM_MIN_FEATURE`; today it emits neither and exits 0. Depends:
  α, gui-on-demand-measurement α.
- **δ (#6779) — `report` and `explain` adopt the full profile.** Modules: `reify-cli`. Kernel
  routing, the measurement arm, and the FEA cache for both. *Leaf.* Signal: **BT3
  forward** for `reify report --bom` on the same fixture. Depends: α,
  gui-on-demand-measurement α.
- **ε (#6781) — `test` gains the BRep kernel.** Modules: `reify-eval` (`test_runner.rs`).
  Per-test **module** isolation is preserved; capability starvation is not isolation
  (ruling 2). Note the FEA-trampoline half of `test` already landed under `INV-FEA-1`;
  only the kernel is missing. *Leaf.* Signal: **BT4 forward, first half** — the geometry
  `@test` fixture returns a real verdict instead of `INDETERMINATE`. Depends: α,
  solver-driver-parity ζ.
- **ζ (#6784) — `explain` on the kernel-bearing path.** Modules: `reify-cli`. Consumes the
  provenance field. *Leaf.* Signal: `reify explain` on a design whose `at auto` poses are
  solved reports non-empty provenance; today it reports `No objective provenance
  recorded` because the path is kernel-less. Depends: δ, solver-legibility-telemetry α.

### Phase 3a — the verdict contract

- **η (#6786) — Evaluating drivers obtain constraint verdicts.** Modules: `reify-eval`
  (`lib.rs`, `engine_eval.rs`, `engine_constraints.rs`), `reify-cli`. The return-type
  work of §2.3: the eval-path result carries verdicts, and `cmd_eval`'s geometry branch
  stops discarding the ones `build()` already computed. *Intermediate* — unlocks θ, ι, κ.
  Signal: unlocks θ; pinned by a Rust test asserting an eval-path result on a violated
  non-geometry module carries a `Violated` entry (today: structurally impossible).
  Depends: α.
- **θ (#6788) — `eval` gates exit on violation.** Modules: `reify-cli`. Routes through the
  shared fold; adds no private verdict logic. *Leaf.* Signal: **BT1**, both directions.
  Depends: η.
- **ι (#6790) — `report` runs `check()` and gates.** Modules: `reify-cli`. *Leaf.* Signal:
  **BT2**, both directions. Depends: η.
- **κ (#6792) — `explain` warns on check failure and never gates.** Modules: `reify-cli`.
  *Leaf.* Signal: **BT6**, both directions — the exit-0 half is asserted, not assumed,
  because this is the single exception to I5. Depends: η.
- **λ (#6793) — `@test` Indeterminate exits non-zero; `@allow_indeterminate`.** Modules:
  `reify-compiler` (`annotations/schema.rs`), `reify-eval` (`test_runner.rs`),
  `reify-cli`. Adds the schema registry entry and the runner consumer; re-baselines the
  pin that currently asserts an all-Indeterminate run exits 0. No grammar work (§12).
  *Leaf.* Signal: **BT4**, both directions — including that the `unknown annotation`
  warning is gone. Depends: ε, solver-driver-parity ζ.

### Phase 3b — surface alignment

- **μ (#6794) — GUI diagnostics carry their codes.** Modules: `gui/src-tauri`. One projection
  function; updates the pinned wire fixtures that currently assert `code: None` — their
  own comment anticipates this change. *Leaf.* Signal: **BT7**, both directions.
- **ν (#6795) — Module-header diagnostics gain a code and a span.** Modules: `reify-compiler`
  (`compile_builder/pre_pass.rs`), `reify-core` (two `DiagnosticCode` variants). Today
  both variants carry the mnemonic in the message string and no label, so they render at
  range (0,0) with no code. *Intermediate* — unlocks ξ, ο. Signal: `reify check` on the
  header-mismatch fixture emits the same text with a code attached and a label on the
  `module` declaration.
- **ξ (#6796) — The GUI enforces the module-header rule on the edited file.** Modules:
  `gui/src-tauri`. The GUI's *imports* already get the check via the module DAG; the
  entry file the user is editing never does. Note the mechanism may arrive as an
  unasserted side effect of resolution-unification's GUI leaf — this leaf owns the
  **assertion**. *Leaf.* Signal: **BT5** in the GUI, both directions, observed through
  the debug MCP. Depends: ν, resolution-unification ε.
- **ο (#6797) — The LSP enforces the module-header rule.** Modules: `reify-lsp`. Same shape.
  *Leaf.* Signal: **BT5** in the LSP, both directions. Depends: ν,
  resolution-unification ζ.
- **π (#6798) — The LSP uses the real constraint checker.** Modules: `reify-lsp`. Aligns with the
  checker-injection task's own boundary intent, which wired the CLI and GUI and left the
  LSP on the stub with no recorded decision. *Leaf.* Signal: **BT8**, both directions —
  the forward half **must** use a constant-constraint fixture (§12), and the reverse half
  asserts the ratified trampoline subtraction survives.
- **ρ (#6799) — The GUI installs the FEA persistent cache.** Modules: `gui/src-tauri`. The GUI
  already resolves the same cache root the CLI installs, sweeps it, and discards the
  path. *Leaf.* Signal: a second GUI evaluation of an FEA-bearing design serves from the
  persistent cache; today the cache hooks are inert in the GUI. Depends: α.
- **σ (#6800) — `--json` driver-result envelope.** Modules: `reify-cli`, `reify-core` (the
  envelope type). Diagnostics with codes, spans and phase; constraint verdicts; exit
  reason. Every `.ri` driver. Enables `reify-core`'s `serde` feature explicitly in
  `reify-cli` (decision 7). *Leaf.* Signal: **BT9**, both directions. Depends: η.
- **τ (#6801) — `reify doc` usage errors exit 1.** Modules: `reify-cli`. Fifteen sites; twelve
  tests assert the old code and several encode it in their names; the harness module doc
  block and a `Format` doc comment both state the convention. Carries the reify-doc PRD
  correction same-diff (§8.2). *Leaf.* Signal: every `reify doc` usage error exits 1;
  a re-baselined suite pins it.
- **υ (#6802) — LSP cfg surface.** Modules: `reify-lsp`. Host-default by default; an optional
  `cfg` map honoured in `initializationOptions` alongside the one option read today.
  *Leaf.* Signal: a cfg-gated import is inert under LSP diagnostics with the host default
  and live when the client supplies the cfg. Depends: resolution-unification β and ζ.
- **φ (#6804) — `--purpose` on the other CLI drivers.** Modules: `reify-cli`. Consumes the shared
  purpose seam. *Leaf.* Signal: a purpose-injected constraint is checked under `reify
  eval` and `reify build`, not only under `reify check`. Depends: the GUI purpose PRD's
  seam leaf (§8.3).

### Phase 4 — gates and closure

- **χ (#6805) — Cross-driver diagnostic-set and exit-code parity, including the LSP.** Modules:
  the existing cross-driver harness target plus `tests/prd-gate/fixtures/`. Extends
  solver-driver-parity's harness with the diagnostic-set object, exit-code equality within
  I5's classes, and the LSP surface that neither existing harness covers for diagnostics.
  Consumes `--json`. Extends `harness_cli/`, never a new standalone binary (decision 10);
  drift-guard registrations same-diff. *Leaf.* Signal: the committed gate is green across
  all eight surfaces and red on an injected divergence. Depends: σ, θ, ι, κ, λ, γ, δ, ξ,
  ο, π, μ, solver-driver-parity φ, resolution-unification η.
- **ψ (#6806) — Distinguishable parity and conformance verdicts.** Modules: the harness. Adopts
  the two-way divergence ledger; the gate names Ring-2 parity vs Ring-1 conformance.
  *Leaf.* Signal: **BT10**, both directions — including that a stale ledger entry fails.
  Depends: χ.
- **ω (#6807) — Docs-truth obligations.** Modules: `crates/reify-mcp/src/tools/chunks/`,
  `examples/best_practices/` + its `INDEX.md`, `.claude/skills/reify-design/SKILL.md`,
  CLI help strings, the language spec. Covers the new language surface
  (`@allow_indeterminate`), the new CLI surface (`--json`, the exit-code contract,
  `--purpose` reach), and the module-header reach. Includes the discoverability
  acceptance: an author who knows the goal ("make my tests fail when they can't be
  decided") finds the mechanism from the chunks or the corpus index. **Also corrects the
  spec's `@test` section**, which today promises constraint diagnostics for failures that
  the runner does not print. **Does not own the purposes chunk** — the GUI purpose PRD's
  docs leaf does, and ω is ordered behind it (§8.1); ω's only edit there is the
  flag-availability statement that φ makes stale. *Leaf.* Signal: each documented
  signature compiles as written in a smoke `.ri`; the corpus example is auto-compile-gated.
  Depends: λ, σ, τ, φ, and the GUI purpose PRD's docs leaf.
- **Ω (#6808) — PRD close.** Docs-only. Backfills real task IDs into this section, sets the
  terminal status marker naming the landed leaves, adds the AS-AUTHORED freeze paragraph
  and the LIVE/AS-AUTHORED map, and applies the matching header to the capability
  manifest. *Leaf.* Signal: the committed header. Depends: every other leaf.

### Dependency summary

Hard intra-batch edges, as wired: `α → {β, γ, δ, ε, η, ρ}`; `η → {θ, ι, κ, σ}`;
`δ → ζ`; `ε → λ`; `ν → {ξ, ο}`; `{σ, θ, ι, κ, λ, γ, δ, ξ, ο, π, μ} → χ`; `χ → ψ`;
`{λ, σ, τ, φ} → ω`; all twenty-four → `Ω`.
*(Corrected 2026-08-28, seam-integrity F3 ruling: π restored to χ's row — the §10 leaf
table always listed it while this summary and the store omitted it; the two committed
lists disagreed and Leo ruled the leaf table authoritative. Edge `#6805 ← #6798` wired.)*

Out-of-batch hard edges, as wired: `{#6689, #5748, #6803} → α` · `#6694 → {ε, λ}` ·
`#6721 → ζ` · `#6740 → {γ, δ}` · `#6724 → μ` · `#5519 → ξ` · `#5520 → {ο, υ}` ·
`#5516 → υ` · `{#6700, #5521} → χ` · `#6803 → φ` · `#6837 → ω`. The last of α's, the φ
edge and the ω edge were added 2026-08-27, once the GUI purpose PRD decomposed (§8.3).
*(Additions 2026-08-28: `#6693 → α` wired, seam-integrity F12 ruling — α and P1 δ
restructure the same cmd_check/cmd_build region. E3 note: #6803 was retired to deferred
by the E3 recombination experiment — both #6803 edges above now point at its replacement
#6904, per that task's own inbound-edge note.)*

Outbound, into other PRDs: the spec-conformance suite's #6769 depends on σ (#6800), and
its #6787 depends on ψ (#6806) — its CLI-observable tier and cross-driver tier, whose own
task text asked for these edges to be wired when this PRD decomposed. *(Reworded
2026-08-27: previously written `#6769 → σ`, which read backwards under this section's
arrow convention.)*

Note π (#6798) and ν (#6795) and τ (#6801) carry no intra-batch upstream: they are
independent of the profile and of the verdict chain, and can start immediately.
*(2026-08-27: π #6798 was temporarily blocked on an administrative escalation, since
resolved.)*

---

## §11 — Out of scope (named successors)

- **The solver axis, in its entirety** — which drivers resolve `auto`, the LSP's solver
  budget, the GUI's solver-free evaluators, the `cmd_build` posture inversion, and the
  retirement of the FEA lock. All solver-driver-parity's. This PRD depends on that work
  and does none of it.
- **`reify check`'s Error-severity exit gate**, the `CHECK_ERROR_EXIT_ALLOWLIST`, and the
  collision between the two tasks that own it with opposing designs. Orthogonal to this
  PRD's violation-gating axis (§4.5); already escalated on one of those tasks. *(Resolved
  2026-08-26, ruled by Leo: #5403 delivers the gate; #6608 depends on #5403 — edge wired,
  mirror records in both tasks' details; see solver-driver-parity §8.)*
- **`check`'s kernel-arm widening, `finish_check`, check's exit codes and `--strict`** —
  reserved to check-diagnostic-truthfulness by a binding G4 ruling.
- **The silent-`@test`-argument hole.** `@test(anything)` compiles today with zero
  diagnostics because the schema declares no args, has no `arg_check`, and the extra-args
  policy is dead code. Decision 5 routes around it by choosing the sibling spelling; it
  does not fix it. Successor: the annotation-args PRD, which specifies the `@allow(...)`
  flag-set host and the arg-checking machinery this would need.
- **`reify mcp-server`**, entirely — deletion is ratified and owned.
- **GUI OpenVDB kernel registration** and the GUI's missing `reify-kernel-openvdb`
  dependency; **GUI export η refusal**; **GUI viewport at-auto poses**. Each has an owner.
- **The GUI purpose surface itself** (selector, bindings form, staleness UX, `set_purpose`
  debug tool) — the parallel GUI purpose PRD. This PRD spreads the *flag* to the other CLI
  drivers only.
- **The warm/edit-path provenance and verdict emptiness.** The warm serve paths emit empty
  provenance by declared contract; changing that is a design decision about whether a warm
  serve re-runs the scope solver, and it belongs with the warm-path leaf that owns it.
- **Diagnostic message text quality** — Ring 3 by ruling.
- **The conformance suite itself.** This PRD builds its Ring-2 substrate and the `--json`
  prerequisite; the suite is a separate program.
- **`reify doc` gaining an engine.** Ratified compile-only (§4.3). Only its exit-code
  convention changes here.

---

## §12 — G6 premise-validity notes

Every capability claim in §1, §5 and §10 was measured at HEAD or read from code at HEAD.
The probe log is §1.1; four premise corrections are recorded here because the brief and
the matrix state them incorrectly, and a leaf written against any of them would be written
against a false premise.

1. **The provenance cite is stale *and* names the wrong function.** The matrix says
   provenance must survive `build()` and cites an empty-map site in `engine_eval.rs`. At
   HEAD that line is inside field elaboration, unrelated. The cited empty map moved, and
   lives in the **warm/cached serve path**, not `build()` — which never constructs an
   eval result at all. The real gap: `Engine::check()` computes provenance via `eval()`
   and discards it at the `CheckResult` construction, because `CheckResult` and
   `BuildResult` have **no provenance field**. It is a struct-shape gap, and it is owned
   by solver-legibility-telemetry's leaf α. Side-condition: provenance is only recorded
   when a solver is active, so routing `explain` through a kernel-bearing path requires a
   solver on that path too.

2. **The LSP has no cfg surface at all** — not the host-default the matrix ruling assumes.
   It constructs no cfg set anywhere; its compile entry point takes no cfg parameter. Leaf
   υ therefore *introduces* the host default rather than making an existing one
   configurable, and the resolution-unification leaf it depends on is what brings the cfg
   parameter within reach. (The brief also labels that leaf "the cfg surface"; the cfg
   surface is a different decision in that PRD. The dependency is right, the label is not.)

3. **The "LSP real checker" alignment is vacuous unless the fixture is a constant
   constraint.** Injecting the real checker into the checked compile entry point is a
   **no-op at compile time** — pinned in-tree by a test whose name says so — because the
   compile-time value map is empty, so every cell is undef and the real checker also
   returns Indeterminate. Divergence exists *only* for constant constraints, pinned by a
   second in-tree test from the other side. BT8 and leaf π are specified on that case.
   Without this, π is a textbook fake-done leaf: a real code change, a passing test, and
   no user-observable difference.

4. **"A module-header mismatch squiggles in the GUI editor" is not reachable today even in
   principle.** The header check emits diagnostics carrying neither a `DiagnosticCode` nor
   a `DiagnosticLabel` — the `E_`/`W_` mnemonics are baked into the message string. Routed
   to an editor as-is they arrive with no code and range (0,0)–(0,0): not a squiggle, and
   not matchable by the parity gate's code-keyed ledger. Leaf ν exists because of this and
   gates leaves ξ and ο.

Two further measured facts that shaped decisions rather than corrected premises:

5. **No grammar work is required for the indeterminate-tolerant annotation.** All three
   candidate spellings parse with 0 ERROR nodes; the annotation production already admits
   `@name`, `@name(ident)` and `@name("string")`. What does **not** exist is any checking
   of `@test`'s arguments (decision 5, §11).

6. **The GUI FEA-cache gap is a resolve-then-discard, not the comment-vs-code
   contradiction the survey describes.** No comment in the GUI promises the wiring. The
   GUI resolves the cache root through the shared resolver, sweeps the directory, and
   throws the path away without installing it. Leaf ρ is a few-line wiring change, not a
   crate-visibility refactor — the shared resolver is already reachable.

**Anchor-freshness caveat.** The survey and matrix cites are from an earlier main and
several have drifted materially (the GUI code-strip projection function among them). Every
line-anchored claim in this document was re-verified at `9a992fc2f2`; leaves must
re-locate by symbol at implementation time regardless.

---

## §13 — Open questions (tactical, deferred to implementation)

1. **The `--json` envelope's exact schema** — field names, whether phase is an enum or a
   string, whether diagnostics nest under a `diagnostics` key or stream. Suggested: mirror
   the LSP converter's existing serde projection for the diagnostic half so the two wire
   forms agree, and follow `reify doc --format json`'s stdout convention for the envelope.
   Decide during σ.
2. **Where the shared verdict fold lives** once `report` and `explain` use it — it is
   currently private to the CLI crate. Suggested: keep it in `reify-cli` and export it
   within the crate; the GUI and LSP do not map to exit codes. Decide during η.
3. **`@allow_indeterminate`'s valid contexts** — structure and constraint-def, matching
   `@test`'s own context list, or narrower. Suggested: exactly `@test`'s contexts, so an
   annotation that cannot apply is a context error rather than a silent no-op. Decide
   during λ.
4. **Whether the parity gate runs per-fixture or per-surface-pair**, and how it names a
   divergence in its failure output. Bears on runtime: eight surfaces × the corpus. Decide
   during χ.
5. **Whether `ρ` should also give the GUI a cache-size budget** distinct from the CLI's,
   given a long-lived session. Suggested: no — the shared resolver's budget applies, and a
   GUI-specific budget is a perf concern, Ring 3. Decide during ρ.
6. **`υ`'s cfg-map shape in `initializationOptions`** — flat string map versus mirroring
   the CLI's `--cfg key=value|flag` grammar. Suggested: mirror the CLI grammar so a
   workspace config and a command line say the same thing. Decide during υ.
