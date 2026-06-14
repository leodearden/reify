# Capability manifest — prd-gate-executable-substrate-verification.md (2026-06-14)

Per-leaf capability→evidence bindings (G3+G6 mechanized), **dogfooding the tightened rules this PRD introduces**: every binding is bound to *observed evidence* (captured command output), not author promise; the negative-assertion branch (W1) is applied; producer bindings verify *deliverable extent* (W2), not name identity; grammar bindings target *composed scenarios* (W4). Verified against the working tree 2026-06-14.

**Filed task IDs:** α=#4607 (keystone), β=#4610, γ=#4608, δ=#4609 (critical). **DAG:** α (no deps) → γ → β; α → δ.

**One binding resolved to clear the gate** (the dogfood paying off — see α-IR row): the naive `ir`-dump-CLI binding resolved to **producer-absent** (a W2 name-match-PASS trap on *this* batch); resolved by **rewrite-to-existing-capability** to the eval-error-signature vector. No residual FAIL.

## α (#4607) — probe runner + contract + committed-probe-set format (KEYSTONE)

| Capability asserted | Probe / evidence (captured 2026-06-14) | Verdict |
|---|---|---|
| `reify check <file>` exists, emits diagnostics, deterministic exit codes | `reify check` in CLI (`crates/reify-cli/src/main.rs`); `reify check /tmp/.../probe.ri` → exit 0 + `All constraints satisfied.` on clean, exit 1 + `error: …` on bad | PASS wired (empirical) |
| `check` observes **arg-vs-param silent-accept** (4575 negative-assertion vector) | `reify check` on `revolute("definitely-not-an-axis", 0rad..1rad, point3(0mm,0mm,0mm))` → **exit 0**, `All constraints satisfied.`, **no rejection diag** → expected=present, observed=absent ⇒ the FAIL the golden test asserts | PASS wired (empirical — negative-assertion branch sound) |
| `check` observes **type-name resolution** (4577 vector) | `reify check` on `param t : Transform3` → **exit 1**, `error: unresolved type: Transform3` | PASS wired (empirical) |
| `tree-sitter parse --quiet` exists; exit 0/1 ⇔ 0/≥1 ERROR nodes; sub-ms | `~/.cargo/bin/tree-sitter`; good fixture → exit 0; arrow-type fixture → exit 1 + `(ERROR [1,12]-[1,32])` in **0.07 ms** | PASS wired (empirical) |
| **ir vector — naive binding:** an IR-dump CLI (`reify dev inspect-ir` / `dump-ir` / `--ir`) exposes `CompiledExprKind` | `grep` for `dev`/`inspect-ir`/`dump-ir`/`--ir` in `crates/reify-cli/src` → **none**; `reify eval` prints *values* (`EvalSmoke.b = 0.005 m`), not IR kinds | **FAIL (producer-absent)** — caught here, NOT name-match-PASS'd |
| **ir vector — resolved (rewrite-to-existing-capability):** eval-error signature observes `CrossSubGeometryRef` vs `IndexAccess` | `CrossSubGeometryRef` panics in `eval_expr` (`crates/reify-compiler/src/expr.rs:374`); leaf consumed by `entity.rs` before eval (`crates/reify-eval/src/engine_purposes.rs:981–987`) → `reify eval` on the 4358 scenario yields a characteristic error/panic signature betraying the IR shape | PASS via rewrite (vector exists; α MAY add a thin `CompiledExprKind` debug-print — tooling, PRD §12, tactical) |
| `Workflow` tool exists (D3/γ rides it; α's verdicts feed it) | the harness/`Workflow` substrate is the agent runtime (PRD §5 precondition) — γ consumes α, not α consumes Workflow | PASS (consumer-upstream of γ) |

## β (#4610) — overlay G3 broadening + D3 wire-in

| Capability asserted | Probe / evidence | Verdict |
|---|---|---|
| The overlay `project.md` is reify-tracked and editable from a reify worktree | `git ls-files .claude/skills/prd/project.md` → tracked; real file (not a symlink) | PASS wired |
| The four semantic-substrate examples are *real observed behaviors* to name | arg-vs-param (4575), type-name (4577), member-access lowering (4437), IR-shape/eval-error (4358) — all grounded in α's empirical probes (PRD §3) | PASS wired |
| The D3 workflow γ exists to reference by path (no forward-reference to unbuilt substrate) | **producer-extent:** β `depends_on` γ (#4608); β's deliverable *consumes* γ's committed script path — extent = "the overlay names a workflow that exists" ✓ | PASS producer-upstream (DAG-direction: γ upstream of β) |
| The dark-factory generic `gates.md` edit (D2a) is NOT a reify deliverable | `readlink -f /home/leo/.claude/skills/prd/references/gates.md` → `/home/leo/src/dark-factory/skills/prd/references/gates.md` (separate repo) → D2a is cross-project; β is its reify-side mirror, independent | PASS wired (no inversion — §8) |

## γ (#4608) — decompose-phase verification workflow

| Capability asserted | Probe / evidence | Verdict |
|---|---|---|
| α's probe runner exists for the Prover/Adversary to call | **producer-extent:** γ `depends_on` α (#4607); γ consumes α's full interface (3 probe kinds + 3 verdicts + captured output) — α's §9 contract delivers exactly that extent ✓ | PASS producer-upstream (α upstream of γ) |
| The `Workflow` tool supports per-leaf fan-out + deterministic synthesis | `Workflow` provides `parallel`/`pipeline` + structured-output agents (the harness substrate) | PASS wired |
| A known-false premise exists to self-test the blocking path | the §3 4575 silent-accept fixture (`reify check` exit 0 / no diag) — reused from α's golden FAIL case | PASS wired (empirical) |

## δ (#4609) — historical-false-premise regression corpus (CRITICAL integration gate)

Negative-assertion branch (W1) applied per-row: each "X is rejected/resolves/parses" premise binds the probe that observes presence/absence; the corpus asserts α returns **FAIL/UNPROVABLE** for each (PRD §10 producer-side table).

| Corpus row | probe_kind | Evidence the FAIL is real | Verdict |
|---|---|---|---|
| 4575 arg-vs-param rejection | check (negative-assertion) | empirical: `revolute("not-an-axis",…)` → exit 0, no diag (PRD §3) | PASS wired |
| 4577 `Transform3` resolves | check | empirical: `error: unresolved type: Transform3` (PRD §3) | PASS wired |
| 4437 member-access→ValueRef | check | poison-literal/diag at check time (auto-type-param-resolution-completion manifest ζ misattribution) — δ authors the fixture | PASS (corpus-authored; vector = α check) |
| 4358 IR shape=IndexAccess | ir (eval-error proxy) | `CrossSubGeometryRef` panics in `eval_expr` (expr.rs:374) → eval-error signature; **uses α's resolved ir vector**, not an IR-dump CLI | PASS (corpus-authored; vector = α eval-error) |
| 4497 purpose-nested structure | grammar (**composed scenario**, W4) | fragment `default Material=` parses, but the *composed* purpose-with-nested-structure scenario is ungrammatical → ERROR node; δ commits the COMPOSED fixture, not the fragment | PASS (corpus-authored; W4 distinction is the point) |
| 3979 arrow-type param | grammar | empirical: `param f : (Length) -> Length` → exit 1, `(ERROR [1,12]-[1,32])` (PRD §3) | PASS wired (empirical) |
| 4375 `-> Scalar` extent | check (producer-extent, W2) | δ's own completeness grep `': Scalar[^<a-zA-Z]'` is blind to `-> Scalar` codomain → probe the full extent | PASS (corpus-authored; W2 extent probe) |
| 4352 `Type::Real` exists | check/ir (W3 drift) | sibling 4373 deleted `Type::Real` (merged 06-12); FAILs at D4 dispatch re-diff — corpus pins it as the drift exemplar | PASS (corpus-authored; W3 drift case) |
| CI wiring (anti-orphan) | grep | δ wires the gate into `tests/infra/run_all.sh` (the production CI entry path run by `verify.sh`) — precedent: test-run-concurrency-semaphore ε | PASS wired |

## Summary

**Manifest verdict: CLEAR.** All bindings PASS on verified substrate after **one resolution**: the α `ir`-dump-CLI binding was caught as **producer-absent** (the exact W2 name-match-PASS failure this PRD attacks, surfaced on its own batch) and resolved by rewriting the `ir` vector to the empirically-grounded eval-error signature (`CrossSubGeometryRef` panic, expr.rs:374). No residual FAIL.

**Numeric-floor branch: N/A** — no leaf asserts a tuned numeric bound; every signal is a verdict-observation / capability-recognition assertion, not a tolerance floor. The single quantitative claim (`tree-sitter parse` "sub-ms") is empirically measured (0.07 ms), not a guessed threshold.

**Cross-project edges (NOT wired here — hand-back to Leo, PRD §8):** D2a (generic `gates.md` G6 negative-assertion branch + producer-extent tightening) and D4 (orchestrator dispatch-time re-diff hook) are dark-factory-owned. Neither blocks a reify leaf (β mirrors D2a's rule operatively; D4 consumes α's committed-probe-set format). Leo files the two `dark_factory:` tasks and wires `D4 → reify:#4607` cross-project.
