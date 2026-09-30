# `reify check` diagnostic truthfulness

Milestone: v0.6 (units-gating program, PRD 2 of 5). Status: active — author-mode
session, no upstream blockers. Re-verified against main HEAD `dc83d4fd60` (2026-07-28).

## Goal

`reify check <file>` is the command Reify's own docs teach first
(`docs/getting-started.md`:8) and the cheapest, no-kernel-required entry point for
"is my design still sound". Today it lies in three independent ways: it can run
without ever compiling a module's geometry operations, it can run compile-time
geometry realization and then throw the diagnostics away, and even when a
`Severity::Error` diagnostic *is* produced, only two hand-picked special cases
(`GdtIllegalModifier`, a `E_DFM_` message-prefix match) make it fail the command —
every other Error is printed to stderr and ignored for the exit code.

After this PRD: a user who runs `reify check` on a module that would produce any
`Severity::Error` diagnostic under `reify eval`/`reify build` sees the same class of
failure (non-zero exit, the diagnostic on stderr) from `check` too — without paying
`build`'s Phase-B product-serialization cost. This closes the specific,
empirically-reproduced gap: `reify check mirror_bare_origin.ri` today prints
`All constraints satisfied.` and exits 0 on a module whose `mirror(...)` call has a
bare (dimensionless) origin — a shape `examples/best_practices/symmetry_mirror.ri`
already documents as `// WRONG — bare-0 origin` — while `reify eval`/`reify build` on
the *identical* file correctly emit `error: failed to compile geometry operation:
missing or non-Length argument 'ox' for mirror` and exit 1. (Reproduced this session,
current main, see Sketch of approach.)

## Why this matters to the units-gating program

Every other PRD in this program (1/3/4/5) phrases its own leaf signals against
`reify eval`'s exit code/diagnostics *specifically because* `reify check` cannot be
trusted for the same purpose today (common brief, seam table). Once this PRD lands,
PRD 1/3's compile-layer slots and all five PRDs' eval-layer gates become
`check`-visible automatically — any of their future leaves that *want* a
check-visible signal take a real `add_dependency` edge onto this PRD's tasks. This
PRD does not gate, phrase, or depend on any of their leaf signals; it only makes the
channel honest.

## Background

`docs/legibility/design-invariants.md` already carries this exact defect as ratified,
pre-existing house doctrine — **INV-SF-2 `error-severity-exits-nonzero`** ("Any
Error-severity diagnostic ... makes the CLI command exit nonzero. No per-code
bolt-on escalation lists ... House pattern: `cmd_eval`'s and `cmd_build`'s
Severity::Error exit gates (task 4458) — check must converge on the same rule") and
**INV-SF-6 `diagnostics-carry-codes`** ("the CLI's `E_DFM_` message-prefix escalation
exists only because co-resident Error diagnostics are code-less"). This PRD is the
delivery vehicle for both. Per project convention neither invariant's text is
restated further here beyond what's quoted above — cite the slugs.

### Correcting the research doc's framing (re-verification finding)

`docs/notes/units-gating-gap-research-2026-07-28.md`'s "reify check false-green"
section and this PRD's spawn brief describe **two** check paths ("default" vs
"geometric-Conforms/DFM") and suggest running `compile_geometry_op` "cheaply, no
kernel realization" on the default path. Re-verification against current main found
both framings need correction:

1. **There are three `cmd_check` sub-paths, not two** (`crates/reify-cli/src/main.rs`
   `fn cmd_check`, :475):
   - **(a) Lightweight, no-purpose** (module has none of geometric `Conforms` /
     `RepresentationWithin` / DFM rule): `Engine::new(checker, None).check()` — no
     kernel, no `build()` call at all. (:647-651)
   - **(b) Kernel-backed, no-purpose** (module has any of the three constraint
     kinds): calls `let _ = engine.build(&compiled, ExportFormat::Step)` (:621) for
     its *side effect* of populating `realization_handles`/`achieved_repr_tol`, then
     re-runs `engine.check(&compiled)` (:646) as the diagnostics/constraint source
     of record. `build()`'s own `BuildResult` (and its diagnostics) are discarded.
   - **(c) `--purpose`** (:693-824): unconditionally `Engine::new(checker,
     None).eval(&compiled)` (:721) regardless of whether the module has geometry at
     all — never `build()`, never routed through `module_has_geometry` the way
     `cmd_eval` is (:1556-1591). This sub-path was not named in the brief or the
     research doc; it has the exact same defect as (a).
2. **`compile_geometry_op` cannot run "cheaply, without a kernel" today** — this is
   not merely unoptimized, it is **structurally kernel-gated**. In
   `crates/reify-eval/src/engine_build.rs`, `Engine::build` (:3856) delegates to
   `build_with_geometry_output` (:3881), which first calls `self.check(module)`
   internally (:3949 — i.e. `build()` ⊇ `check()`, `check()` never ⊇ `build()`), then
   gates the *entire* geometry-op compile+dispatch block on kernel presence:
   `let geometry_output = if let Some(name) = default_kernel_name.as_deref() &&
   self.geometry_kernels.contains_key(name) { … } else { None };` (:4101-4103,
   :4771-4773), with the comment at :4776 confirming "geometry_output block may be
   skipped when no kernel is registered". `compile_geometry_op` itself (defined
   `crates/reify-eval/src/geometry_ops.rs`:937) is reached from inside that block via
   `Engine::execute_realization_ops` (:4352). A kernel-independent invocation of
   `compile_geometry_op` exists **only** as a `cfg`-gated, test-only harness
   (`crates/reify-eval/src/geometry_op_characterization_probe.rs`) — it is not
   production-callable. **Getting `compile_geometry_op`'s gating diagnostics under
   `check` therefore requires an actual registered kernel and pays real
   OCCT-realization cost** — see the Resolved design decisions and the measured
   numbers below. There is no available "free" path for v1; promoting the
   characterization-probe's decoupled-compile pattern to production is named as a
   possible follow-up, out of scope here.

Both corrections are load-bearing for the decomposition below — they were verified
empirically this session, not merely re-read from source (see next section).

## Sketch of approach

**Empirically reproduced this session** (current main HEAD `dc83d4fd60`, fresh
`target/release/reify`), scratch fixtures not committed (not grammar — no new
syntax; ordinary `mirror(...)` call already exists):

```
# /tmp/…/mirror_bare_origin.ri — box + 7-arg scalar mirror(), bare origin
# (the exact "// WRONG — bare-0 origin" shape from
#  examples/best_practices/symmetry_mirror.ri:27)

$ reify check mirror_bare_origin.ri
All constraints satisfied.
$ echo $?
0

$ reify eval mirror_bare_origin.ri
…
warning: mirror: ox argument expects Length, got Int; pass a dimensioned length such as `5mm`
error: failed to compile geometry operation: missing or non-Length argument 'ox' for mirror
$ echo $?
1

$ reify build mirror_bare_origin.ri -o out.step   # same error, same exit 1
```

`reify check` is the *only* one of the three commands that is silently wrong here —
`eval`/`build` already have a working rejection mechanism (`required_length_origin3`
inside `compile_geometry_op`); `check` simply never reaches it.

**Second reproduction, real committed corpus file, no synthetic fixture needed** —
`examples/m5_geometry_flange.ri` derives `centroid`/`moi_principal` from geometry
(no GD&T/DFM/RepresentationWithin, so it takes sub-path (a) above). Currently:

```
$ reify check examples/m5_geometry_flange.ri
  OK BoltFlange#constraint[0]
  INDETERMINATE BoltFlange#constraint[1]
  OK BoltFlange#constraint[2]
  OK BoltFlange#constraint[3]
No constraints violated (1 indeterminate).
$ echo $?
0
# stderr (printed, ignored): error: `centroid` could not be resolved …
# error: `moment_of_inertia` could not be resolved …
```

`reify eval`/`reify build` on the same file correctly resolve both cells
(`centroid = point(≈0, ≈0, 0.006 m)`, `moi_principal = […]`) via the kernel-backed
path, and `constraint[1]` would be `OK`, not `INDETERMINATE`, once `check` takes the
same path. This is a real, already-committed, already-CI-run fixture whose `check`
output changes (`INDETERMINATE` → `OK`) the moment sub-path (a) is fixed — no new
example needed for this leaf's signal.

**Measured cost** (this session, same binary, `time`, 3 runs each, small/medium
fixtures — no large-assembly measurement taken, see the G6 caveat below):

| Fixture | `check` (today, sub-path a) | `build` (kernel-realized) | delta |
|---|---|---|---|
| `examples/bracket.ri` (1 box) | ~0.30s | ~0.31s | ~flat (fixed-cost dominated) |
| `examples/perforated_plate.ri` (box + pattern + boolean) | ~0.42s | ~0.47s | ~+50ms / +12% |
| `examples/m5_geometry_flange.ri` (cylinder×2 + pattern + boolean) | ~0.32s | ~0.50s | ~+180ms / +55% |

Small/simple parts: overhead is in the noise (process-startup/OCCT-init dominated).
Medium parts with booleans/patterns: overhead is real (tens to ~180ms, 10-55%) but
not prohibitive at this scale. **Not measured**: large assemblies, meshing-heavy
parts, or FEA solves (FEA stays trampoline-free under `check` regardless — see
decision D1). This is an achievability basis (G6 branch 1), not a numeric floor
assertion; if a decompose-time measurement on a larger corpus fixture shows
unacceptable overhead for common designs, the documented fallback is narrowing
sub-path (a)'s new routing condition (below) to modules that actually have a
geometry-query value cell, deferring plain-realization-only modules — filed as a
follow-up performance task, out of scope here.

## Resolved design decisions

**D1 — Route `check`'s two currently-lightweight sub-paths through the existing
kernel-backed `build()` machinery; do not build a new "cheap compile-only" capability
for v1.** Extend sub-path (a)'s routing condition from `has_geometric_conforms ||
has_representation_within || has_dfm_rule` to also include `|| module_has_geometry(&compiled)`
(reusing the existing helper at `main.rs`:2432, already used by `cmd_eval`). Extend
sub-path (c) (`--purpose`) with the same `module_has_geometry`-gated choice between
`Engine::with_registered_kernel(...).build(...)` and `Engine::new(checker,
None).eval(...)` that `cmd_eval` already uses (:1556-1591) — today it unconditionally
takes the `eval()` branch. **Rationale:** reuses proven, already-shipped machinery
(G3-clean — no fictional substrate); accepts measured, bounded kernel-realization
cost (D1 above) rather than inventing an unbuilt "compile without a kernel"
capability (the only precedent for that is test-only, per the research-doc
correction). FEA stays trampoline-free under `check` regardless (its own established
design intent, `main.rs`:448-474, unchanged by this PRD) — the register-a-kernel
change here is about geometry ops (OCCT), not solvers.

**D2 — Stop discarding `build()`'s diagnostics; merge them with the authoritative
`check()` pass, deduplicated.** Sub-path (b) today calls `build()` for its handle-
population side effect only (`let _ =` at :621), then separately calls
`engine.check(&compiled)` (:646) as the diagnostics/constraint source of record — a
correct and necessary sequence (RepresentationWithin's `achieved_repr_tol` must be
populated, via `tessellate_realizations`, *between* the two calls; see the ordering
comment at :577-589). Because `build_with_geometry_output` calls `self.check(module)`
internally as its own first step (:3949), `build()`'s discarded `BuildResult`
contains a *stale* copy of check-style diagnostics (computed before
`realization_handles`/`achieved_repr_tol` are populated) plus, appended after, the
realization-only diagnostics (`compile_geometry_op` gating, kernel errors) that
`check()` alone never produces. The fix is not "use build()'s diagnostics instead of
check()'s" (stale) nor "just concatenate both" (would double-print every genuine
eval-level diagnostic, since both passes re-run `eval()` against the same
deterministic input) — it is a **structural-equality merge**: keep `check()`'s
diagnostics as authoritative, then append only the entries from `build()`'s
diagnostics that are not already present by `(severity, code, message)` equality.
(`reify_core::Diagnostic` derives `Debug, Clone` only, not `PartialEq` — the three
comparable fields are individually `PartialEq`, so a manual tuple/helper comparison
is what's buildable; `DiagnosticCode` is `PartialEq, Eq, Hash`.) **Invariants** (the
contract an implementer must preserve, not literal code):
  - No diagnostic that would appear in `check()`'s own diagnostics today is ever
    printed twice.
  - Every diagnostic that `build()` alone would have produced (i.e. every
    `compile_geometry_op` / kernel-dispatch diagnostic) appears at least once in the
    reported set.
  - `constraint_results` come from the authoritative `check()` call only (never
    `build()`'s stale copy) — RepresentationWithin/GD&T/DFM correctness is
    unaffected.

**D3 — `check`'s exit code fails on any `Severity::Error` diagnostic, unconditionally
— not gated by `--strict`.** Replace `finish_check`'s ConstraintOutcome-only decision
with the same shape `build_is_success` already uses for `cmd_build` (:2286-2288:
`!has_error_diagnostic && !matches!(outcome, SomeViolated)`) — i.e. after computing
the merged diagnostic set (D2), `check` fails if `SomeViolated`, or
`has_error_diagnostic`, or (`SomeIndeterminate` and `--strict`). `finish_check`'s own
printed text is **unchanged** (this mirrors `cmd_build`'s existing, shipped UX: a
vacuously-`AllSatisfied` module with zero declared constraints still prints "All
constraints satisfied." *and* exits non-zero when an Error diagnostic is present —
confirmed this session against `cmd_build`'s current behavior on the reproduction
fixture). The two existing ad-hoc escalations — `GdtIllegalModifier` code-match
(:668-677/812-821) and `dfm_has_error_diagnostic` `E_DFM_` message-prefix match
(:679-690, helper defined :2580) — are **removed as redundant**: both diagnostics are
already `Severity::Error`, so the general gate subsumes them with no behavior
change (their existing regression tests get repointed at the general gate, not
deleted — same observable escalation, less special-case code, per INV-SF-2's "no
per-code bolt-on escalation lists"). **`--strict` semantics do not change** — it
still governs only `SomeIndeterminate`; per the spawn brief this is a hard
constraint (no silent overload of an existing flag), and per D3 no new flag is
needed either, since `Severity::Error` unconditionally failing is exactly `cmd_eval`
and `cmd_build`'s existing, already-shipped posture — `check` converging on it is
not a new policy, it's removing `check`'s exception to an existing one.

**D4 — Fix the one *known* case where an unconditional Error-severity gate (D3)
would be a regression, and inventory for others, before D3 ships.**
`cmd_check` deliberately never registers compute trampolines (:448-474 doc comment —
FEA stays Indeterminate under check by design, `reify build`/`reify eval` are the FEA
gate). But the diagnostic this produces — `"@optimized target {:?}: no registered
compute trampoline (falling back to body-inlining)"` is emitted at only TWO of the
four sites reporting a missing trampoline (RULING 2026-09-01 below supersedes this
paragraph's site inventory) — at all four it is `Severity::Error` and **code-less**
today. Per INV-SF-2's corollary ("a diagnostic *expected* on a healthy path is by
definition not Error-severity — demote or recode it; never exempt it from the
gate") the fix is **not** a `check`-side allowlist (that would itself be the
"per-code bolt-on escalation" INV-SF-2 forbids, just in the opposite direction) —
it is a **severity fix at the emission site**: condition severity on whether *any*
compute trampoline was ever registered on the engine at all
(`self.compute_registry.fns.is_empty()`, the same registry `register_compute_fn` /
`compute_dispatch` already read/write, `engine_admin.rs`:1160-1190) — `is_empty()` ⇒
this is `check`'s documented trampoline-free-by-design posture ⇒ `Severity::Warning`
(still visible, still informative, per INV-SF-1's "conservative degradation must
leave a trace" — not silence); not-empty (i.e. `cmd_eval`/`cmd_build` registered
trampolines but *this specific target* still isn't covered — a genuine dispatch gap)
⇒ stays `Severity::Error`. Same touch points get a real `DiagnosticCode` (new
variant, e.g. `NoRegisteredComputeTrampoline`) instead of code-less, satisfying
INV-SF-6 at zero extra site cost. **Before D3 ships**, grep-inventory every other
`Diagnostic::error`/`.with_code(...)` construction site in `reify-eval` reachable
from `Engine::eval`/`Engine::check`/`Engine::build` (excluding compile-time-only
diagnostics `cmd_check` already gates before constructing an engine, :546-552) and
classify each as expected-on-a-healthy-check-path (needs the same treatment) or a
genuine defect signal (correctly gates as-is). This is the brief's explicit
"BLAST RADIUS MUST BE MEASURED, not estimated" instruction for cause 3 — the
trampoline diagnostic is the *one* case named in the brief/research doc, not
asserted to be the only one.

### RULING 2026-09-01 (Leo) — mint-site and predicate-site are SEPARATE; the four sites are not alike

D4 above conflates *where the `DiagnosticCode` is minted* with *where the
`fns.is_empty()` severity predicate applies* ("Same touch points … at zero extra
site cost"), and its four-site list is wrong twice over: the line numbers have
drifted, and only two of the four emit the `(falling back to body-inlining)`
clause. Corrected inventory, measured on main 2026-09-01 — **resolve by symbol,
these numbers drift**:

| Site | Shape | Message |
|---|---|---|
| `engine_eval.rs`:~10417 (`evaluate_params_and_lets_unified`, gate ~:10172) | **SOFT** — push the diagnostic, fall through to body-inlining | carries `(falling back to body-inlining)` |
| `engine_eval.rs`:~11534 (`evaluate_let_bindings`, gate ~:11157) | **SOFT** — same | carries the clause |
| `engine_admin.rs`:~1674 (`dispatch_compute_node`) | **HARD** — `Err(vec![…])`, nothing falls back | clause deliberately omitted (see ~:1666-1672) |
| `engine_compute.rs`:~653 (`run_compute_dispatch`) | **HARD** — `Err(DispatchError::Failed)` | clause deliberately omitted |

**RULED (Q1 — scope).** Mint `DiagnosticCode::NoRegisteredComputeTrampoline` at
**all four** sites — INV-SF-6, and it finally delivers the "clean, named"
diagnostic `docs/prds/v0_3/compute-node-contract.md`:189 asked for and never got.
Apply the `fns.is_empty()` ⇒ `Severity::Warning` predicate at the **two SOFT
sites only**. The two HARD sites keep `Severity::Error` unconditionally, with
rustdoc recording why.

Why this is right on the merits, not merely because a test would otherwise break:

1. **At both HARD sites `fns.is_empty()` can never be true in production.**
   `dispatch_compute_node` has ZERO non-test callers workspace-wide (measured).
   `run_compute_dispatch`'s `None` arm is reached only via
   `insert_shell_extract_upstream`, which sits *inside* the
   `compute_dispatch("solver::elastic_static").is_some()` branch — so the registry
   is non-empty by construction. Applying the predicate there changes zero
   user-observable behaviour; it only flips a unit-test expectation.
2. **It would break a documented API contract.** `dispatch_compute_node`'s rustdoc
   promises its `Err` arm carries "at least one `Severity::Error` diagnostic". An
   `Err` carrying only a Warning is incoherent, and is silently swallowed by
   `build`/`eval`'s `has_error_diagnostic` exit gate.
3. **House precedent is the split, not the sweep.** `hex_wedge_mesh_diagnostic`
   (`crates/reify-core/src/diagnostics.rs`:~4354-4408) conditions severity on a
   posture flag, PRESERVES the `DiagnosticCode` across the flip, and applies the
   flip to 3 of 5 outcomes with the other two explicitly exempt.
4. **This PRD's own user-observable signal (below) is deliverable from the two
   SOFT sites alone** — they are the only sites `reify check` can reach.

**RULED (Q2 — predicate shape).** Keep `self.compute_registry.fns.is_empty()`. It
is a proxy for "this driver declines compute entirely", and it is correct for every
driver today and after #6693. An explicit `Engine` posture flag (the
`MeshContractMode` house pattern) is the honest long-term shape but needs
cross-crate plumbing into `reify-cli`, `reify-lsp` and `gui/src-tauri`, outside α's
declared file set — that belongs to the driver-contract work, not to α.

**RULED (Q3 — severity).** `Warning`, per INV-SF-1 ("conservative degradation must
leave a trace"). Three regimes already coexist for this underlying condition and
none is disturbed: `Error` at eval/dispatch, LSP `INFORMATION` for its *separate*
`fea-not-evaluated` constraint hint (`crates/reify-lsp/src/diagnostics.rs`:~445-455,
5 locks), and no diagnostic at all at the pure-expression layer
(`crates/reify-expr/src/lib.rs`:~10292).

**RULED (Q4 — the signal must be flip-proof against #6693).** Leo's 2026-08-26
driver-contract ruling 2 (`docs/notes/driver-contract-matrix-draft.md`) gives
`reify check` the FEA trampolines; #6693 delivers it, pending behind
#6653/#6689/#6692. Post-flip `check`'s registry is NON-empty, so
`examples/fea_pressure_smoke.ri` stops emitting the line at all and α's signal
becomes unmeasurable on it. **α's regression fixture must therefore declare an
`@optimized` target that is NOT in `register_production_compute_fns`'s bundle**, so
the `check` ⇒ `warning:` / `eval`+`build` ⇒ `error:` contrast stays measurable
before and after #6693. No dependency edge onto #6693 is wired: α is a root in two
PRD DAGs, and #5404's burn-down would otherwise park behind the whole
solver-parity chain. The predicate is correct in both worlds, and the LSP — whose
trampoline-free posture stays ratified (INV-FEA-1) — remains a live production
consumer of the Warning arm after the flip.

**Locks this deliberately supersedes — α changes these, and only these:**

- `crates/reify-eval/tests/compute_dispatch_registry.rs`
  `e2e_unregistered_optimized_target_emits_diagnostic_and_inlines` (~:307) — uses
  `make_simple_engine()` (empty registry) and evals through the SOFT site
  (execution-probed 2026-09-01), so it flips to `Severity::Warning`. This is an
  honest supersession, not a weakening: its `Severity::Error` assertion pins
  current behaviour, not a contract — `compute-node-contract.md`:189, the row its
  docstring cites, asks for a *named* diagnostic and `Freshness::Failed` and says
  nothing about severity (and today's soft path does not mark Failed either). α
  MUST keep asserting *diagnostic emitted / target named / body-inlined / no
  ComputeNode inserted*, AND add a twin on a NON-empty-registry engine that still
  asserts `Severity::Error`. Without that twin the update IS a silent weakening.
- `crates/reify-eval/tests/compute_dispatch_registry.rs`
  `dispatch_compute_node_unregistered_target_returns_error_diagnostic` (~:188) —
  **UNCHANGED**. It lands on the HARD `engine_admin` site (execution-probed) and
  goes on guarding the `Err`-carries-an-`Error` contract.

**Blast radius, measured 2026-09-01 — do not re-derive.** Severity is asserted in
exactly TWO places workspace-wide (both named above). About ten further tests key
on the message TEXT `"no registered compute trampoline"` and are severity-blind,
so they stay green — `no_stale_undef_invariant_gate.rs` (`TRAMPOLINE_MISSING`,
~:1106/:1219/:2309/:2324/:2337), `test_runner.rs`:~272, `cli_build_fea.rs`:~49/:93.
**Changing the wording breaks ten tests; changing the severity breaks two — keep
the message wording byte-identical.** Both severity locks run in the merge gate
(neither is in `REIFY_GATE_EXCLUDE_HEAVY`'s heavy set). The CLI side was
deliberately left unlocked against this change:
`crates/reify-cli/tests/harness_cli/cli_build_fea.rs`:~153-155 already records that
"the severity downgrade is an engine-side concern out of this CLI task's scope".

**Two loose ends α owns in the same diff:**

- `engine_eval.rs`:~11533's `// Release-hard-error is deferred to slice η` names
  `compute-node-contract.md` §9 OQ-1 ("body-inline in debug, hard error in
  release"), which NO task tracks and whose Greek-letter alias would not survive
  PTODO grammar. Delete it, or give it a live `#NNNN` cite.
- `engine_compute.rs`:~625-631's "unreachable from production code" claim is FALSE
  (point 1 above): the arm is reachable on a partially-registered engine — the
  task-5578 signature. Correct the comment.

**Downstream, unchanged by this ruling.** #5403's seeded
`CHECK_ERROR_EXIT_ALLOWLIST` entry #1
(`MessageContains("no registered compute trampoline")`, disposition `Demote`, cite
`#5311`) is exactly what this retires and #5404 burns; #5403's requirement that
`eval`/`build` keep gating on this `Error` is preserved by the non-empty arm.

*Provenance: esc-5311-3 (member esc-5311-2), task 5311; ruled by Leo 2026-09-01.*

## Contract: diagnostic collection & exit-code semantics

(G5: bare **B**, not B+H — single crate-pair, `reify-cli` + `reify-eval`, no
cross-PRD boundary; a contract section is warranted given the sequencing subtlety in
D2, a full two-sided boundary-test matrix is not.)

- **Ordering invariant (unchanged, load-bearing, do not resequence):** in sub-path
  (b), `build()` (handle population) → `tessellate_realizations()` (repr-tol
  population, RepresentationWithin only) → the authoritative `check()` call, in that
  order. D2's merge reads `build()`'s `BuildResult.diagnostics` captured at step 1
  and `check()`'s `CheckResult.diagnostics`/`.constraint_results` captured at step 3;
  it must never substitute step-1's stale constraint_results for step-3's.
- **`--purpose` branch (sub-path c):** when `module_has_geometry(&compiled)`, use
  `BuildResult` (has `.values`, `.diagnostics` — `crates/reify-eval/src/lib.rs`:1181)
  in place of today's unconditional `EvalResult` (`.values`, `.diagnostics` —
  `lib.rs`:1125); `check_constraints_with_values(&result.values)` is unaffected by
  which result type produced `.values`. Apply the same D2 merge + D3 exit-gate
  extension as sub-path (b) — this branch currently has *no* DFM-Error escalation at
  all (only `GdtIllegalModifier`, :816-821), an existing asymmetry with sub-path (b)
  that D3's general gate incidentally closes (worth noting in the leaf, not a
  separate fix).
- **Exit-code function shape** (mirrors `build_is_success`, :2286-2288 — do not
  change `check_fails`'s existing signature/tests, compose at the call site
  instead): `let has_error = merged_diagnostics.iter().any(|d| d.severity ==
  Severity::Error); ...; if has_error { ExitCode::FAILURE } else {
  finish_check(...) }` — `finish_check`'s own text-printing logic and its existing
  unit tests (:3386-3524) are untouched.
- **What must NOT change:** `reify eval`/`reify build` semantics (out of scope, per
  spawn brief); `--strict`'s Indeterminate-only scope; the FEA trampoline-free
  design intent of `check` (locked by `check_fea_violated_constraint_is_not_gated`,
  cited in the :448-474 doc comment — this PRD's D4 changes that diagnostic's
  *severity*, never its trampoline-free *cause*).
  **CORRECTION, 2026-08-26:** the trampoline-free design intent of `check` has since
  been OVERTURNED by Leo's ruling 2 in `docs/notes/driver-contract-matrix-draft.md`;
  `check` gains the FEA trampolines and the named lock is retired, delivered by
  `docs/prds/v0_6/solver-driver-parity.md` leaf δ. **This PRD's own scope is unchanged** —
  D4 still owns only the diagnostic's severity, and this bullet's other three
  "must not change" items still stand. What is no longer true is that the *cause*
  is permanent. Correction owed and landed by
  `docs/prds/v0_6/driver-contract-implementation.md`'s authoring session.

## Pre-conditions for activating

None — no upstream PRD/task dependency. Independent value even if PRD 1/3/4/5 never
land (per the research doc's Q7 recommendation). `grammar_confirmed: true` for every
leaf (no new syntax; `mirror(...)`, `centroid`, `@optimized` etc. all already parse
and compile — G3 is otherwise a substrate-reuse check, not a grammar check, and every
reused mechanism cited above (`module_has_geometry`, `build_is_success`'s shape,
`compute_registry.fns`, `BuildResult`/`CheckResult` fields, the
`geometry_op_characterization_probe` precedent, the `cli_check.rs` harness) was read
directly off current main this session, not assumed from the brief.

## Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `units-length-gate-completion` (PRD 1) | produces-for | future eval/compile-slot leaves wanting a check-visible signal | this PRD (2) | no seam today — PRD 1 wires a real `add_dependency` onto this PRD's landed tasks at ITS OWN decompose time if/when it wants one |
| `angle-units-surface-convergence` (PRD 3) | produces-for | same as above, for ANGLE compile slots | this PRD (2) | same — no seam today |
| `dimensioned-construction-strictness` (PRD 4) | produces-for | same, for ctor-slot conformance promotion | this PRD (2) | same — no seam today |
| `dimension-checked-readers` (PRD 5) | produces-for | same, for reader/solver-extraction diagnostics | this PRD (2) | same — no seam today |

Per the common brief's binding G4 ruling: `cmd_check`/`finish_check`/build-diagnostic
collection/exit codes/`--strict` are **PRD 2 ONLY** — no contested ownership, no
reciprocal ambiguity to resolve. In-flight tasks 5623/5658/5661/5662/5627 belong to
PRD 1/4 and are not referenced by any leaf below.

## Decomposition plan

Bare B (not B+H — see Contract section header). Four leaves; α and δ are
independent of each other, γ hard-depends on both, ε is a docs leaf that depends on
γ (describes the landed contract, not a moving target).

- **α — Severity/code hygiene for the compute-trampoline-fallback diagnostic + a
  measured inventory of any other expected-on-a-healthy-check-path Error site.**
  Files — **resolve every anchor by symbol; the line numbers this PRD was written
  with have all drifted, and D4's "RULING 2026-09-01" block scopes each edit**:
  `crates/reify-eval/src/engine_eval.rs` (`evaluate_params_and_lets_unified` and
  `evaluate_let_bindings` — the two SOFT sites: `DiagnosticCode` **and** the
  `is_empty()` severity predicate); `crates/reify-eval/src/engine_admin.rs`
  (`dispatch_compute_node`) and `crates/reify-eval/src/engine_compute.rs`
  (`run_compute_dispatch`) — the two HARD sites: `DiagnosticCode` **only**,
  `Severity::Error` stays unconditional, with rustdoc recording why;
  `crates/reify-core/src/diagnostics.rs` (new
  `DiagnosticCode::NoRegisteredComputeTrampoline` variant, additive —
  `#[non_exhaustive]`, no `VARIANT_COUNT` backstop, no exhaustive `match`, and no
  code table or string mapping to update anywhere in the workspace, measured
  2026-09-01); `crates/reify-eval/tests/compute_dispatch_registry.rs` (supersede
  `e2e_unregistered_optimized_target_emits_diagnostic_and_inlines` and ADD its
  non-empty-registry twin; `dispatch_compute_node_unregistered_target_returns_error_diagnostic`
  is unchanged). Condition
  severity on `self.compute_registry.fns.is_empty()` per D4. Grep-inventory other
  `Diagnostic::error` construction sites reachable via `check()`; fix any found in
  the same diff if the count is small (≤ a couple), else name a follow-up task
  explicitly (do not silently drop them). **Signal:** a regression fixture whose
  `@optimized` target is deliberately OUTSIDE `register_production_compute_fns`'s
  bundle prints `warning:` (not `error:`) for the trampoline-fallback line under
  `reify check`, and `error:` for that same line under `reify eval`/`reify build`
  — a contrast that stays measurable after #6693 gives `check` the FEA
  trampolines. **Do NOT reuse `examples/fea_pressure_smoke.ri`**: its line
  disappears entirely post-flip (see the RULING block). (Regression
  test, new — extends `crates/reify-cli/tests/harness_cli/cli_check.rs`, an
  *existing* gate-resident integration-test file, so no new nextest-partition /
  wallclock / `run-all-classification.manifest` registration is triggered — the
  drift-guard rule only fires for a *new* `tests/*.rs` file or a new
  `tests/infra/test_*.sh`, neither of which this leaf adds). No consumer-facing
  behavior change yet (foundation for γ).
- **β — Route both currently-lightweight `cmd_check` sub-paths ((a) and (c)) through
  `build()` when the module has geometry; stop discarding diagnostics.** File:
  `crates/reify-cli/src/main.rs` (`fn cmd_check`, :475-825). Implements D1 + D2:
  extend sub-path (a)'s routing condition with `|| module_has_geometry(&compiled)`;
  give sub-path (c) the same `module_has_geometry`-gated `build()`-vs-`eval()` choice
  `cmd_eval` already uses; capture (don't discard) `build()`'s `BuildResult` in
  sub-path (b) and merge its diagnostics with the final `check()` call's per D2's
  structural-equality algorithm; apply the same merge in sub-path (c). **Signal:**
  `examples/m5_geometry_flange.ri`'s `reify check` output flips
  `INDETERMINATE BoltFlange#constraint[1]` → `OK BoltFlange#constraint[1]` (new
  regression test in `cli_check.rs` pinning this exact fixture + line); a new
  committed fixture under `crates/reify-cli/tests/fixtures/` reproducing the
  `mirror(...)` bare-origin shape now shows the `error: failed to compile geometry
  operation …` line on `reify check`'s stderr (previously absent) — exit code is
  still 0 until γ lands (this leaf only fixes *collection*, not the *exit gate*;
  this is intermediate-but-independently-observable, not a fake-done leaf — the CLI
  output difference is real and user-visible even before γ).
- **γ — General `Severity::Error` exit-code gate for `check`, replacing the two
  ad-hoc escalations.** File: `crates/reify-cli/src/main.rs` (the two `finish_check`
  call sites in `cmd_check`, :660-666 and :804-810; delete the `GdtIllegalModifier`
  escalation blocks :668-677/:812-821 and `dfm_has_error_diagnostic` :2580 plus its
  dedicated unit tests, repointing their fixture-level regression coverage at the
  general gate). Implements D3. **Depends on α** (hard `add_dependency`, not PRD
  prose — landing this before α would newly fail every FEA `@optimized` check with
  no registered trampoline, a real regression) **and on β** (hard `add_dependency` —
  the canonical `mirror(...)` bare-origin fixture only reaches its Error diagnostic
  via β's routing fix; γ alone, without β, only subsumes the two ad-hoc escalations
  it deletes — a real but narrower improvement). **Signal:** the `mirror(...)`
  bare-origin fixture (committed by β) now exits 1 under `reify check` (previously
  0) — the PRD's headline reproduction, now fixed end-to-end; existing
  `GdtIllegalModifier`/DFM-Error fixture tests continue to pass unchanged
  (regression-locked: same escalation, general mechanism).
- **δ — Docs-truth leaves (all four sub-requirements in one diff).** Depends on γ.
  1. **Doc-chunk equivalent:** no `crates/reify-mcp/src/tools/chunks/*.md` chunk
     documents CLI/`reify check` semantics today (the 17 chunks are DSL
     language-surface only — grep-verified this session, zero hits for
     `reify check`/`cmd_check`/`--strict`); the brief itself names
     `docs/getting-started.md` as the applicable doc for this PRD's docs-truth gate.
     Update `getting-started.md`:8 (§1 sanity check) with a one-line note that
     `check` now surfaces build-time diagnostics too, and its troubleshooting
     section (:92-94) with a new Q&A: "`reify check` exits nonzero but I didn't
     write a `constraint`" → points at the Error-diagnostic gate + `--strict`'s
     separate, unchanged Indeterminate-only scope. Verify :46-48's existing bare-`50`
     "indeterminate" teaching example is unaffected (confirmed this session: this
     path produces no `Severity::Error`, only an `Indeterminate` `ConstraintOutcome`
     — untouched by this PRD; re-ran it against current main to confirm no
     regression).
  2. **Exemplar-corpus update:** extend
     `examples/best_practices/symmetry_mirror.ri`'s existing `// WRONG — bare-0
     origin` comment (:27) to note `reify check` now also rejects this, not only
     `eval`/`build` (no new corpus file — this PRD introduces no new authoring
     idiom, only fixes a diagnostic-truthfulness bug in an idiom the corpus already
     documents).
  3. **Cheatsheet index:** `symmetry_mirror.ri` is already indexed
     (`.claude/skills/reify-design/SKILL.md`:72) — confirmed this session, no new
     index line needed.
  4. **Discoverability:** the new getting-started.md troubleshooting entry (1.i) IS
     the discoverability leaf — a user who knows the intent ("check said fine, my
     part is wrong") finds the exit-code contract from the troubleshooting section
     they already land on today (:92-94 exists for exactly this class of question).

**G7 walk (advisory in author mode):** INV-SF-2 and INV-SF-6 are directly addressed
(see Background) — not waived, resolved. INV-SF-4 (`indeterminate-attributable-transient`)
is cited in its own evidence as relevant to `check`/`--strict` ("non-strict check
reports 'No constraints violated (N indeterminate)' and exits 0") but this PRD does
not change Indeterminate/`--strict` semantics at all (explicit brief constraint) —
acknowledged as a pre-existing condition, not newly introduced, no waiver needed.
INV-SF-1/SF-3 (undef provenance, declared-intent-consumed) are similarly pre-existing
and out of scope — `check` still never enables undef-cause capture, `relate`/DFM
silent-skip diagnostics are unchanged; neither is this PRD's charter.

**Drift-guard registration:** N/A for all four leaves — none adds a new
`crates/*/tests/*.rs` file or `tests/infra/test_*.sh` script (α and γ extend the
existing `cli_check.rs` harness file and `main.rs`'s existing `#[cfg(test)] mod
tests`; β extends both). Confirmed the rule's actual trigger condition (a *new*
gate-resident file) does not fire here, rather than asserting N/A generically.

## Out of scope for this PRD

- `reify eval`/`reify build` semantics — unchanged (explicit brief constraint).
- Any units/dimension gating itself (bare-length rejection, ANGLE convention,
  ctor-slot conformance, reader dimension-checking) — PRD 1/3/4/5's charter. This
  PRD only makes `check` truthfully surface whatever those PRDs' mechanisms already
  produce, at eval-layer and compile-slot layer alike.
- Promoting `geometry_op_characterization_probe`'s decoupled (no-kernel)
  `compile_geometry_op` invocation pattern from test-only to a production "compile
  without realizing" capability — named in D1/Background as the honest long-term
  answer to "make check cheap", explicitly deferred; D1's kernel-backed routing is
  the v1 answer, cost-measured and bounded.
- Giving DFM diagnostics (`E_DFM_OVERHANG`/`_UNDERCUT`/`_DRAFT`/etc.) real
  `DiagnosticCode`s — a separate, real INV-SF-6 gap (zero DFM `DiagnosticCode`
  variants exist today, confirmed by grep this session) that this PRD's γ leaf
  incidentally stops *depending on* (by deleting the message-prefix-match escalation
  in favor of the severity-only general gate) but does not fix. Named here so it
  isn't lost; a future INV-SF-6 remediation PRD/task is the right owner.
- The sub-path (c) / sub-path (b) DFM-escalation asymmetry noted in the Contract
  section is closed as a side effect of D3, not as an independent fix — no separate
  leaf.
- Performance follow-up if a decompose-time measurement on larger fixtures shows
  D1's kernel-backed routing is too expensive for common designs (see Sketch of
  approach's G6 caveat) — named as a possible future task, not filed here.

## Open questions (tactical, not decided in this session)

1. ~~**Exact `DiagnosticCode` variant name**~~ — **RULED 2026-09-01**:
   `NoRegisteredComputeTrampoline`, minted at all four sites. Note
   `docs/prds/v0_3/compute-node-contract.md`:189 named this diagnostic
   `UnknownComputeTarget` and it was never minted; the v0.6 name wins because it
   matches the message text the ~10 severity-blind text-matching tests key on.
   See D4's "RULING 2026-09-01" block for the full scope ruling.
2. **Whether α's inventory finds more than the one known code-less/expected-Error
   site.** If it finds a handful more, fix them in α's own diff (stated default); if
   it finds many, α should stop and file a named follow-up rather than silently
   scope-creeping — the threshold ("a couple" vs "many") is a judgment call for
   whoever implements α, not pre-decided here.
3. **Whether D1's routing condition should be narrowed** (module has a
   geometry-*query* cell, not just any geometry) if the decompose-time measurement on
   larger fixtures shows meaningful overhead for plain-realization-only modules with
   no query cells to resolve. Suggested resolution: keep `module_has_geometry` as
   written (simplest, reuses the exact existing helper) unless measurement says
   otherwise; decide at β's implementation time against a larger corpus sample than
   this session's three fixtures.
