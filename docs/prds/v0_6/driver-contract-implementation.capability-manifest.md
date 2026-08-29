# Capability manifest — Driver-contract implementation

PRD: `docs/prds/v0_6/driver-contract-implementation.md`. Machine-readable twin:
`docs/prds/v0_6/driver-contract-implementation.capability-manifest.yaml`.

Mechanizes G3 + G6 per leaf: every capability a leaf's signal asserts is bound to
evidence, so the substrate check is paid once here rather than once per task at
dispatch. Any FAIL binding blocks the batch.

**Evidence base.** Observations were made at main `9a992fc2f2` (2026-08-26) with
`target/debug/reify` built the same day at 19:51; the three commits between that build
and HEAD are docs-only, so the binary is code-current. The six committed fixtures under
`tests/prd-gate/fixtures/driver_contract_*.ri` each parse with 0 ERROR nodes under
`tree-sitter parse --quiet` and reproduce the baseline recorded in their own headers.
Code-only claims were verified by reading the cited symbol at HEAD, not by trusting the
survey's cites — four of which had drifted or were wrong (§12 of the PRD).

**Verdict summary: 25 leaves, 103 bindings, 0 FAIL.** Thirty-five carry a mechanical `delivered_check` in the sidecar; the rest are `manual` or evidence-only.

**Scope-of-evidence caveat, stated up front.** Three leaves — ξ (GUI module header),
ο (LSP module header) and ρ (GUI FEA cache) — rest on **code-read evidence only**. No
GUI or LSP runtime session was available in this session, matching the limitation the
cross-driver survey records for itself. Each such binding is marked `code-read` rather
than `observed`, and the distinction is load-bearing: a code-read binding says "the call
site is absent", not "the behaviour was witnessed".

---

## Negative-assertion sentinel — the six inverted checks

Six leaves assert a rejection or a gate. Per the sentinel rule each was authored and run
(or, where marked, read at the call site), and the asserted behaviour was observed to be
**ABSENT** — which confirms the capability does not exist and correctly makes each a
*producing* leaf rather than a preservation assertion.

| Leaf | Asserted behaviour | Observed today | Disposition |
|---|---|---|---|
| θ | `reify eval` exits non-zero on a violated constraint | exit **0**, and **no constraint output at all** | ABSENT → θ produces it |
| ι | `reify report --bom` exits non-zero on a violated design | exit **0**, prints the line item and `Total: 0.42 USD` | ABSENT → ι produces it |
| λ | an Indeterminate `@test` exits non-zero | exit **0**, `test result: ok. 0 passed; 0 failed; 1 indeterminate` | ABSENT → λ produces it |
| γ/δ | eval and report surface the `E_DFM_*` pair `check` surfaces | **zero** DFM diagnostics, exit 0, values printed as if clean | ABSENT → γ/δ produce it |
| ξ/ο | a mismatched `module` header is diagnosed in the GUI / LSP | neither calls the header check (**code-read**); all seven CLI subcommands exit 1 on the same file | ABSENT → ξ/ο produce it |
| κ | `reify explain` **warns** on check failure | warning ABSENT (explain never runs the constraint pass) — but the **exit-0 half is PRESENT** | mixed: κ produces the warning, preserves the exit code |

κ's row is deliberately the mixed shape. Its ruled role difference is that one half must
change and the other must not, so its boundary test asserts both directions; binding only
the warning would let a leaf "fix" explain into gating and still pass.

---

## Per-leaf bindings

### α — Capability profile + profile-taking construction path

| Capability | Binding | Verdict |
|---|---|---|
| `resolution-profile-is-upstream-not-here` | **DAG-direction** — `ResolutionProfile` does not exist on main (exhaustive grep over `crates/`, zero hits). It is created by solver-driver-parity's leaf α, which is **upstream** by a real edge. This leaf widens a type it does not create. | PASS |
| `trampoline-bundler-exists` | capability→producer, **wired on main** — `Engine::register_production_compute_fns` is a real method with three delegating call sites; α composes it and becomes its sole caller. | PASS |
| `bundler-panics-on-double-registration` | observed (doc + code) — the bundler's own rustdoc states it panics if called twice, because it re-runs the unconditional registrar. This is why sole ownership is a correctness requirement, not a style rule. | PASS |
| `grep-architecture-gate-exists` | capability→producer, **wired on main** — `scripts/check-compute-trampoline-registration.sh` exists and is registered in the verify-pipeline manifests. α widens it; it does not mint a second gate. | PASS |
| `seventeen-construction-sites` | observed — an exhaustive census of production (non-test-helper) engine construction found 17 sites carrying 12 distinct 7-axis fingerprints. This is the duplication α removes. | PASS |
| `inv-fea-1-row-exists` | capability→producer, **wired on main** — `docs/invariants.md` carries an `INV-FEA-1` row ("full registration has one constructor", status `proposed`, owner compute-fea-hardening). α generalises that row rather than minting a new id. | PASS |
| `cmd-check-predicates-are-not-alpha's` | **SCOPE BOUNDARY, added 2026-08-27** — check's content predicates *are* its kernel routing, which a binding G4 ruling reserves to check-diagnostic-truthfulness. As first filed, α's deliverable 3 asked for something this PRD's own §8.1 forbids. Resolved by ordering, not by exemption: check's routing is rewritten upstream, the GUI purpose PRD's seam leaf then unifies check's body behaviour-preservingly, and α adopts the result. Both are real edges on α now. | PASS |

### β — `build` gains the FEA persistent cache

| Capability | Binding | Verdict |
|---|---|---|
| `cache-installer-exists` | capability→producer, **wired on main** — `Engine::set_persistent_cache_dir` is the installer; four production sites already call it. β adds a fifth. | PASS |
| `build-lacks-the-cache` | observed — `cmd_build` builds its engine directly and deliberately bypasses the shared helper that installs the cache; no `set_persistent_cache_dir` call is reachable from it. | PASS |
| `cache-hooks-are-inert-without-a-dir` | observed (doc + code) — the setter's contract states that `None` (the default) makes the compute-dispatch write/lookup hooks inert. β's signal is therefore a real behavioural change, not a no-op flag flip. | PASS |

### γ — `eval` adopts the full profile

| Capability | Binding | Verdict |
|---|---|---|
| `measurement-seam-is-upstream` | **DAG-direction** — `Engine::run_measurement_pass` does not exist on main (zero hits). It is created by gui-on-demand-measurement's leaf α, **upstream** by a real edge, whose D1 forbids a second copy of the arm sequence. γ calls it. | PASS |
| `openvdb-attach-api-exists` | capability→producer, **wired on main** — `Engine::ensure_openvdb_kernel` is idempotent, registry-driven, and returns false when the adapter is absent. Two production callers today: `cmd_check` and `cmd_build`. | PASS |
| `eval-never-attaches-openvdb` | **observed by execution** — `reify eval` on an isosurface module emits `no openvdb kernel registered (call ensure_openvdb_kernel()); leaving the BRep/Mesh fallback`, which `reify build` does not. | PASS |
| `dfm-arm-is-check-only` | **observed by execution** — `driver_contract_dfm_measurement_arm.ri`: `check` exit 1 with `E_DFM_MIN_WALL` + `E_DFM_MIN_FEATURE`; `eval` exit 0 with neither. | PASS |
| `fixture-parses` | grammar-fixture — `tests/prd-gate/fixtures/driver_contract_dfm_measurement_arm.ri`, 0 ERROR nodes. | PASS |

### δ — `report` and `explain` adopt the full profile

| Capability | Binding | Verdict |
|---|---|---|
| `measurement-seam-is-upstream` | **DAG-direction** — as γ. | PASS |
| `report-is-kernel-free` | observed — `cmd_report` constructs a kernel-less engine and evaluates; BOM mass cells are undef in consequence (survey DV13). | PASS |
| `report-lacks-dfm` | **observed by execution** — `report --bom` on the DFM fixture: exit 0, zero DFM diagnostics. | PASS |
| `explain-is-kernel-free` | observed — `cmd_explain` constructs a kernel-less engine; corroborated independently by solver-legibility-telemetry, which records that pose autos therefore produce zero provenance on the CLI. | PASS |

### ε — `test` gains the BRep kernel

| Capability | Binding | Verdict |
|---|---|---|
| `test-engine-is-kernel-free` | observed — the test runner's engine builder passes no kernel; its rustdoc documents the solver omission, and the kernel omission is what makes a geometry `@test` structurally Indeterminate. | PASS |
| `trampoline-half-already-landed` | capability→producer, **wired on main** — the test runner already calls `register_production_compute_fns` (morph `Unavailable`) under `INV-FEA-1`. Only the BRep kernel is missing; ε must not re-do the trampoline half. | PASS |
| `geometry-test-is-indeterminate` | **observed by execution** — `driver_contract_geometry_test_indeterminate.ri`: `INDETERMINATE TestBlockVolume`, exit 0. | PASS |
| `solver-half-is-upstream` | **DAG-direction** — the solver for `test` is solver-driver-parity's leaf ζ, upstream by a real edge. ε owns the kernel only. | PASS |

### ζ — `explain` on the kernel-bearing path

| Capability | Binding | Verdict |
|---|---|---|
| `provenance-field-is-upstream` | **DAG-direction** — `CheckResult` and `BuildResult` have **no** provenance field on main (verified by reading both struct definitions). The field is added by solver-legibility-telemetry's leaf α, upstream by a real edge. | PASS |
| `provenance-map-populated-only-under-a-solver` | observed — the eval-side population is gated on an active solver; with none, the map stays empty. So ζ's engine needs a solver as well as a kernel. Recorded because a leaf that added only the kernel would produce an empty map and read as done. | PASS |
| `matrix-cite-is-wrong` | **premise correction** — the matrix's cited line is inside field elaboration at HEAD and the empty map it names lives in the warm/cached serve path, not `build()`. Bound here so no leaf is written against the cited line. | PASS |
| `explain-prints-no-provenance-today` | **observed by execution** — `reify explain` on the eval-blindness fixture prints `No objective provenance recorded (no auto parameters resolved).` | PASS |

### η — Evaluating drivers obtain constraint verdicts

| Capability | Binding | Verdict |
|---|---|---|
| `eval-result-has-no-verdict-field` | observed — the eval-path result struct carries values, diagnostics, resolved params and provenance, and **no** constraint results. This is the structural gap, not a missing call. | PASS |
| `check-result-has-verdicts` | capability→producer, **wired on main** — the check-path result carries `constraint_results`; η widens the eval path toward that shape rather than inventing a verdict type. | PASS |
| `geometry-branch-already-computes-verdicts` | observed — `cmd_eval`'s geometry branch calls `Engine::build`, which calls `check()` internally; the verdicts are computed and then dropped at the destructuring. Half of η is deletion of a discard. | PASS |

### θ — `eval` gates exit on violation

| Capability | Binding | Verdict |
|---|---|---|
| `eval-does-not-gate-today` | **negative-assertion sentinel, observed by execution** — `driver_contract_eval_blind_to_violation.ri`: `check` exit 1 `VIOLATED`, `eval` exit **0** with no constraint output. Rejection ABSENT. | PASS |
| `shared-verdict-fold-exists` | capability→producer, **wired on main** — `ConstraintOutcome`, `report_constraint_results`, `check_fails`, `build_is_success` and `finish_check` all exist and are used by check and build. θ routes through them. | PASS |
| `fixture-parses` | grammar-fixture — `tests/prd-gate/fixtures/driver_contract_eval_blind_to_violation.ri`, 0 ERROR nodes. | PASS |

### ι — `report` runs `check()` and gates

| Capability | Binding | Verdict |
|---|---|---|
| `report-never-runs-check` | observed — `cmd_report` calls `eval()` and never `check()`; no verdict exists on that path. | PASS |
| `report-does-not-gate-today` | **negative-assertion sentinel, observed by execution** — `driver_contract_report_blind_to_violation.ri`: `check` exit 1, `report --bom` exit **0** printing `Total: 0.42 USD`. Rejection ABSENT. | PASS |
| `fixture-parses` | grammar-fixture — `tests/prd-gate/fixtures/driver_contract_report_blind_to_violation.ri`, 0 ERROR nodes. | PASS |

### κ — `explain` warns and never gates

| Capability | Binding | Verdict |
|---|---|---|
| `explain-warning-absent` | **negative-assertion sentinel, observed by execution** — explain never runs the constraint pass, so no warning exists. Warning ABSENT → κ produces it. | PASS |
| `explain-exit-zero-present` | **preservation assertion, observed by execution** — `reify explain` on the violated fixture exits **0** today. This half must survive κ, which is why the boundary test asserts it in the reverse direction. | PASS |
| `role-difference-is-ruled` | producer-self — the warn-never-gate split is ruling 3(b) of the closed matrix, not an inference from code. | PASS |
| `warning-severity-is-invariant-required` | **G7 interaction, observed** — `INV-SF-2` forbids a per-command exemption from the Error-severity exit gate, and `explain` already gates on Error-severity diagnostics. An Error-severity warn-but-exit-0 would create exactly the exemption the invariant bans; `Warning` severity keeps it unnecessary. Bound because "warns" alone leaves the severity to the implementer. | PASS |

### λ — `@test` Indeterminate exits non-zero; `@allow_indeterminate`

| Capability | Binding | Verdict |
|---|---|---|
| `annotation-grammar-already-admits-it` | **grammar-fixture** — `tests/prd-gate/fixtures/driver_contract_allow_indeterminate.ri` parses with 0 ERROR nodes. The annotation production already admits `@name`, `@name(ident)` and `@name("string")`; all three candidate spellings were probed clean. **No grammar work is queued.** | PASS |
| `annotation-is-loudly-unregistered` | **observed by execution** — `check` and `test` emit `warning: unknown annotation @allow_indeterminate`. λ's completion is partly the disappearance of this warning, which makes the leaf falsifiable. | PASS |
| `schema-registry-exists` | capability→producer, **wired on main** — the annotation schema registry is a real const slice with nine entries and a validation dispatcher; λ adds an entry, it does not build a registry. | PASS |
| `test-args-are-silently-dropped` | **observed by execution** — `@test(allow_indeterminate)` and `@test("allow_indeterminate")` both compile with **zero** diagnostics. This is the measured basis for choosing the sibling spelling (PRD decision 5) and is explicitly NOT fixed here (PRD §11). | PASS |
| `indeterminate-does-not-gate-today` | **negative-assertion sentinel, observed by execution** — the geometry `@test` fixture: `test result: ok`, exit 0. Rejection ABSENT. | PASS |
| `runner-fold-exists` | capability→producer, **wired on main** — `compute_status` folds per-constraint satisfaction into a test status, and the CLI counts Indeterminate separately already. λ changes the exit decision, which is one arm. | PASS |
| `diagnostics-printing-is-upstream` | **DAG-direction** — printing the dropped `TestResult.diagnostics` is solver-driver-parity's leaf ζ, upstream by a real edge. λ owns the exit contract and the annotation only. | PASS |

### μ — GUI diagnostics carry their codes

| Capability | Binding | Verdict |
|---|---|---|
| `strip-is-one-projection-function` | observed — `diagnostics_to_info` in the GUI backend hard-codes `code: None` at a single line, with three production callers. | PASS |
| `code-is-available-upstream` | observed — the source `Diagnostic` carries `code: Option<DiagnosticCode>`, and the same projection reads severity, message and labels off that very value. A **lossy projection**, not an upstream absence. | PASS |
| `wire-field-exists` | capability→producer, **wired on main** — `reify_core::DiagnosticInfo.code: Option<String>` exists, is serde-serialized, and has a TS mirror. μ populates a field it does not add. | PASS |
| `non-lossy-precedent-exists` | capability→producer, **wired on main** — the LSP converter already projects a `DiagnosticCode` to a string through serde. μ copies it. | PASS |
| `existing-tests-lock-the-strip` | observed — GUI tests assert `code: None`, and one carries a comment anticipating exactly this change and instructing the implementer to update both the assertion and the doc comment. Bound so the leaf budgets for the re-baseline. | PASS |
| `two-synthesised-codes-exist` | observed — two GUI sites synthesise string codes downstream, one guarded by `if d.code.is_none()` which is always true today. The reverse boundary test exists because that guard's meaning changes. | PASS |

### ν — Module-header diagnostics gain a code and a span

| Capability | Binding | Verdict |
|---|---|---|
| `header-check-carries-no-code-or-label` | observed — both variants build a bare warning/error with the `W_MODULE_DECL_MISSING` / `E_MODULE_PATH_MISMATCH` mnemonic baked into the **message string**, with no `DiagnosticCode` and no `DiagnosticLabel`. | PASS |
| `codeless-renders-at-origin` | observed — the LSP converter falls back to range (0,0)–(0,0) when a diagnostic has no label, and drops the code when absent. This is why "squiggles in the editor" is unreachable before ν. | PASS |
| `diagnostic-code-enum-is-extensible` | capability→producer, **wired on main** — `DiagnosticCode` is `#[non_exhaustive]`, has 187 fieldless variants, no impl block anywhere and serde round-trips automatically, so a variant addition is a one-line change by that type's own documented contract. | PASS |
| `codeless-cannot-be-ledgered` | observed (doc + code) — the two-way divergence ledger this PRD adopts states that a diagnostic whose code is `None` can never be matched and always surfaces as an unreasoned divergence. ν is therefore a prerequisite of the parity gate, not only of the squiggle. | PASS |

### ξ — The GUI enforces the module-header rule on the edited file

| Capability | Binding | Verdict |
|---|---|---|
| `gui-entry-file-is-unchecked` | **code-read** — the GUI's two compile funnels call the compiler entry points directly and neither applies the header-check helper; the GUI's *imports* do get it, because they route through the module DAG. The rule fires for every imported module and never for the file the user is editing. | PASS |
| `gui-names-modules-by-file-stem` | **code-read** — both GUI load paths derive a single-segment module path from the file stem, which is exactly the identity the header rule compares against. The comparison input exists; only the call is missing. | PASS |
| `mechanism-may-arrive-unasserted` | **DAG-direction** — resolution-unification's GUI leaf deletes the GUI's hand-rolled compile twin and routes it to the shared entry point, which *does* attach the header diagnostic. Enforcement could therefore arrive as an unasserted side effect; ξ owns the **assertion**, and is ordered behind that leaf by a real edge. | PASS |
| `fixture-parses` | grammar-fixture — `tests/prd-gate/fixtures/driver_contract_header_mismatch.ri`, 0 ERROR nodes; all seven CLI subcommands exit 1 on it (observed by execution). | PASS |

### ο — The LSP enforces the module-header rule

| Capability | Binding | Verdict |
|---|---|---|
| `lsp-never-checks-the-header` | **code-read** — exhaustive grep over the LSP crate for the header-check helper and both mnemonics returns zero hits. | PASS |
| `lsp-names-modules-by-uri-stem` | **code-read** — the LSP derives a single-segment module path from the URI stem (and re-implements that derivation inline in its diagnostics path). Comparison input present. | PASS |
| `import-axis-is-owned-elsewhere` | **DAG-direction** — imported-file header-mismatch surfacing is a resolution-unification leaf. ο is the *driver* axis and is disjoint from it; bound so the two are not read as duplicates. | PASS |

### π — The LSP uses the real constraint checker

| Capability | Binding | Verdict |
|---|---|---|
| `lsp-compiles-with-the-stub-checker` | observed — the LSP's three compile sites call the entry point that hard-wires the compile-time stub checker, which returns Indeterminate for every constraint. The real-checker entry point has exactly two production callers: the CLI and the GUI. | PASS |
| `real-checker-is-keystroke-cheap` | observed — the real checker is a zero-sized struct whose whole body is an expression evaluation over an already-built value map; it touches no geometry kernel and no solver. The latency objection does not apply. | PASS |
| `injection-is-a-no-op-except-for-constants` | **VACUITY GUARD, observed (two in-tree pins)** — one test pins that injecting the real checker is a no-op at compile time (the compile-time value map is empty, so both checkers return Indeterminate); a second pins that a **constant** constraint diverges. π's signal and BT8 are specified on the constant case. **Without this binding π is a fake-done leaf**: a real code change with a passing test and no user-observable difference. | PASS |
| `trampoline-subtraction-must-survive` | **preservation assertion** — the LSP's trampoline-free posture is Leo-ratified, documented and locked by a named test. π changes the *checker*, not the posture; the reverse boundary direction asserts the lock still holds. | PASS |

### ρ — The GUI installs the FEA persistent cache

| Capability | Binding | Verdict |
|---|---|---|
| `gui-never-installs-the-cache` | **code-read** — `set_persistent_cache_dir` has zero occurrences under the GUI tree, so the compute-dispatch cache hooks are inert there. | PASS |
| `shared-resolver-is-already-reachable` | capability→producer, **wired on main** — the GUI already calls the shared config-crate cache resolver in its startup sweep, so it resolves the exact root the CLI installs and then discards the path. **Not** a crate-visibility refactor: the CLI's own private resolver is a different function. | PASS |
| `survey-framing-corrected` | **premise correction** — the survey calls this a "comment-vs-code contradiction" and says a comment promises the wiring. No such comment exists; it is a resolve-then-discard gap. Bound so the leaf is not written hunting for a promise. | PASS |

### σ — `--json` driver-result envelope

| Capability | Binding | Verdict |
|---|---|---|
| `no-json-surface-exists-today` | observed — exhaustive grep finds no `--json` flag in any Rust crate. σ creates the surface. | PASS |
| `structured-stdout-precedent-exists` | capability→producer, **wired on main** — `reify doc --format json` already emits serde JSON of a model to stdout, and is the only structured-stdout mode on the CLI. σ follows its convention. | PASS |
| `diagnostic-code-serializes` | capability→producer, **wired on main** — `DiagnosticCode` derives serde behind a feature and renames to PascalCase; the LSP already relies on that round-trip and pins it with a table test. That is the wire form. | PASS |
| `cli-does-not-enable-the-serde-feature-directly` | **observed, and a real hazard** — `reify-cli` does not request `reify-core`'s `serde` feature; it compiles with serde only through feature unification via its LSP and MCP dependencies. σ must enable it explicitly (PRD decision 7) or a dependency reshuffle breaks `--json` for an unrelated reason. | PASS |
| `verdicts-require-eta` | **DAG-direction** — the envelope carries constraint verdicts, which do not exist on the eval path until η lands. Real intra-batch edge. | PASS |

### τ — `reify doc` usage errors exit 1

| Capability | Binding | Verdict |
|---|---|---|
| `doc-is-the-only-exit-2-driver` | **observed by execution** — `reify doc` with no args and with an unknown flag both exit **2**; `reify check` with an unknown flag and `reify eval` with no args both exit **1**. | PASS |
| `fifteen-sites` | observed — fifteen `ExitCode::from(2u8)` sites, all inside the doc command; nothing else in the workspace asserts a reify exit code 2. | PASS |
| `twelve-tests-and-three-doc-sites-must-move` | observed — twelve tests assert the code (several encode "two" in their names), plus the harness module doc block, a format-enum doc comment, and one sentence in the reify-doc PRD. Bound so the leaf budgets for a rename-and-rebaseline, not a one-line change. | PASS |
| `no-clap-to-fight` | observed — the workspace has no `clap` dependency; every subcommand hand-rolls its flag walk, so the exit code is chosen explicitly at each site and there is no framework default to override. | PASS |

### υ — LSP cfg surface

| Capability | Binding | Verdict |
|---|---|---|
| `lsp-has-no-cfg-surface-at-all` | **premise correction, observed** — the matrix ruling says the LSP baseline is host-default. It is not: the LSP constructs no cfg set anywhere and its compile entry point takes no cfg parameter. υ *introduces* the host default. | PASS |
| `host-default-constructor-exists` | capability→producer, **wired on main** — `CfgSet::host_default()` exists and is what the GUI uses; υ adopts it. | PASS |
| `initialization-options-are-already-read` | capability→producer, **wired on main** — the LSP's initialize handler already reads one key from `initializationOptions` by inline JSON access, and stores it on server state. υ adds a second key to a live path. | PASS |
| `cfg-parameter-arrives-with-the-ru-leaf` | **DAG-direction** — the cfg-bearing compile entry point is defined by one resolution-unification leaf and reaches the LSP via another. Both are upstream by real edges. **Label correction:** the brief calls the LSP leaf "the cfg surface"; the cfg surface is a different decision in that PRD. The dependency is right, the label is not. | PASS |

### φ — `--purpose` on the other CLI drivers

| Capability | Binding | Verdict |
|---|---|---|
| `purpose-engine-api-exists` | capability→producer, **wired on main** — activate/deactivate/is-active purpose methods all exist on the engine and are exercised today by `reify check --purpose`. | PASS |
| `purpose-is-check-only-today` | observed — `--purpose` appears in exactly one usage string; no other driver parses it, and the GUI has no purpose surface at all (33 Tauri commands, none purpose-related). | PASS |
| `shared-seam-is-upstream-and-filed` | **DAG-direction — RESOLVED 2026-08-27.** `activate_purpose_session()` is owned by the GUI purpose PRD, which landed and decomposed on 2026-08-27; the seam is its leaf α. The real edge is wired and φ is `pending`. (This binding read *unfiled* at authoring, when φ was correctly parked `deferred` per PRD §8.3.) | PASS |
| `seam-is-behaviour-preserving-for-check` | **observed in the landed sibling PRD, added 2026-08-27** — that seam leaf is explicitly scoped behaviour-preserving: it changes no `cmd_check` routing, exit code, diagnostic or `--strict`, because all four are reserved elsewhere. So φ inherits a callable unified body, **not** a fixed `--purpose` arm. The same PRD records, with executed evidence, that `reify check --purpose` exits 0 on a file plain `reify check` rejects with a `RepresentationWithin` violation — a false green the seam does not close. φ must not assume it has. | PASS |

### χ — Cross-driver diagnostic-set and exit-code parity

| Capability | Binding | Verdict |
|---|---|---|
| `resolution-harness-is-upstream` | **DAG-direction** — solver-driver-parity's parity leaf already drives one fixture corpus through six surfaces; it exists as a specification, not as code, and is upstream by a real edge. χ extends its target rather than building a fourth harness. | PASS |
| `diagnostic-set-harness-covers-three-surfaces` | observed — resolution-unification's parity leaf asserts diagnostic-set equality across check, eval and the GUI load path; **it does not cover the LSP**, and it deliberately excludes exit-code equality under a recorded G7 waiver. That exclusion is the residue χ closes. | PASS |
| `neither-harness-exists-yet` | observed — the file named by the diagnostic-set harness's own task metadata does not exist, and no test in the workspace performs a cross-driver diagnostic comparison. χ is a producing leaf, not an extension of running code. | PASS |
| `no-new-standalone-binary` | **constraint, observed** — the harness-layout ratchet supersedes grandfathering a new standalone test binary in this crate; the sanctioned remedy is a `harness_<subsystem>/` compile unit. χ extends the existing consolidated CLI harness. | PASS |
| `drift-guard-registration-same-diff` | **constraint** — a new gate-resident test owes its drift-guard registrations in the same diff (classification manifest row, wall-clock bounds, nextest partitions). Bound because the overlay's worked case is exactly a parity harness landing without them and reddening main. | PASS |

### ψ — Distinguishable parity and conformance verdicts

| Capability | Binding | Verdict |
|---|---|---|
| `two-way-ledger-precedent-exists` | capability→producer, **wired on main** — the differential harness's `Divergence` enum and `assert_equivalent_or_allowed` implement a two-way gate that fails both on an unmatched divergence and on a **stale allow-entry**. ψ adopts this shape. | PASS |
| `one-way-ledger-precedents-also-exist` | observed — two one-way ledgers exist in tree (a grammar-corpus expected-clean set and a static/runtime exemption list). Both were considered; the two-way one is the only one that fails on stale cover, which is why it is chosen. | PASS |
| `verdict-split-is-ruled-but-unbuilt` | producer-self — the parity-vs-conformance distinction is ruled in the committed conformance scope-boundary note; no PRD implements it. ψ produces it. | PASS |
| `codes-must-exist-first` | **DAG-direction** — the ledger matches allow-entries by diagnostic code, and its own contract says a code-less diagnostic can never be matched. μ and ν are upstream by real edges. | PASS |

### ω — Docs-truth obligations

| Capability | Binding | Verdict |
|---|---|---|
| `language-surface-changes` | producer-self — this PRD adds `@allow_indeterminate` (language surface), `--json` and the exit-code contract (tooling surface), and widens `--purpose` reach. The overlay's docs-truth gate therefore fires and requires all four deliverables. | PASS |
| `chunk-and-corpus-substrate-exists` | capability→producer, **wired on main** — the MCP chunk directory, the auto-compile-gated best-practices corpus with its index, and the reify-design cheatsheet index all exist. ω updates them. | PASS |
| `spec-test-section-is-already-wrong` | observed — the language spec's `@test` section promises that test output "reports pass/fail with constraint diagnostics for failures". The runner does not print them. ω corrects the spec alongside the leaf that makes the promise true. | PASS |
| `purposes-chunk-is-not-omegas` | **DOCS-OWNERSHIP BOUNDARY, added 2026-08-27** — the GUI purpose PRD's docs leaf owns `chunks/purposes.md` and documents `--purpose` as it stands when it runs (check + GUI). φ then widens the flag to five more drivers, making that statement stale. ω is ordered behind that leaf by a real edge and corrects **only** that statement; it does not rewrite the chunk, and it extends the existing `reify-design` index line rather than adding a competing one. | PASS |

### Ω — PRD close

| Capability | Binding | Verdict |
|---|---|---|
| `terminal-vocabulary-is-closed` | producer-self — the overlay defines exactly three terminal status tokens and the freeze-header shape. Ω applies it to this PRD and to this manifest. | PASS |
| `cancelled-sibling-counts-as-satisfied` | producer-self — per the overlay, a cancelled sibling leaf satisfies Ω's dependency edge; if the scheduler treats it as unmet the steward removes the edge by hand rather than leaving Ω permanently blocked. | PASS |

---

## Anti-vacuity notes

Three bindings exist specifically to stop a leaf from passing while changing nothing a
user can observe:

- **π's `injection-is-a-no-op-except-for-constants`.** The single highest-risk leaf in
  the batch: the obvious signal ("the LSP uses the real checker") is pinned in-tree as a
  no-op. Only the constant-constraint case is observable.
- **ν's `codeless-cannot-be-ledgered`.** Without ν, both the squiggle leaves and the
  parity gate would ship green against diagnostics that render at (0,0) with no code.
- **ψ's `codes-must-exist-first`.** A parity gate over a code-stripped surface fails
  permanently and uninformatively, which reads as "the gate is flaky" rather than "the
  gate is unwired".

## Cross-PRD corrections owed at landing

Recorded here as well as in the PRD so the obligation is machine-visible: three committed
documents currently assert that `reify check`'s trampoline-free opt-out stands, is locked,
and must not change. Ruling 2 reverses it and another PRD's leaf delivers the inversion.
Amend those documents in this PRD's authoring commit; the reify-doc exit-code sentence
travels with leaf τ instead, because unlike the others it is not yet false.
