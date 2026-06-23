# tests/prd-gate — Committed probe set and fixtures

Executable capability probes for PRD §4 D1 (`scripts/prd-capability-check.py`).
Consumed by γ/D3 (decompose-phase verification workflow) and dark-factory D4
(dispatch-time substrate re-diff).

## Committed-probe-set format (JSON)

```json
{
    "probes": [
        {
            "capability": "<human name for the capability being probed>",
            "probe_kind": "grammar" | "check" | "ir",
            "fixture": "<repo-relative path to the .ri fixture file>",
            "expected": {
                "observation": "present" | "absent",
                "match": {
                    "exit_code": <int>,          // optional
                    "stderr_contains": "<str>",  // optional
                    "stdout_contains": "<str>"   // optional
                }
            }
        }
    ]
}
```

**All `match` fields are optional.** An empty `match: {}` object means "no
specific criterion" (used for grammar probes, where observation is determined
by exit code alone).

## Probe kinds

| `probe_kind` | Command | Observation model |
|---|---|---|
| `grammar` | `tree-sitter parse --quiet <fixture>` (CWD = `tree-sitter-reify/`) | exit 0 → **PRESENT** (no parse errors); exit 1 with `(ERROR` in output → **ABSENT**; "Failed to load language" in stderr → **HARNESS ERROR** |
| `check` | `reify check <fixture>` | `match` predicate satisfied → **PRESENT**; predicate not satisfied → **ABSENT** |
| `ir` | `reify eval <fixture>` (eval-error proxy) | exit 0 → **ABSENT** (sound by determinism); exit ≠ 0 with asserted `stderr_contains` signature → **PRESENT**; exit ≠ 0 without signature → **INDETERMINATE** → UNPROVABLE |

## Verdicts

| Observation | Expected | Verdict |
|---|---|---|
| PRESENT | present | **PASS** |
| ABSENT | absent | **PASS** |
| PRESENT | absent | **FAIL** |
| ABSENT | present | **FAIL** |
| INDETERMINATE | (any) | **UNPROVABLE** |

## Harness exit codes

| Code | Meaning |
|---|---|
| 0 | All probes PASS |
| 1 | ≥1 probe FAIL |
| 2 | ≥1 probe UNPROVABLE, 0 FAIL |
| 64 | Usage / argument error (`EX_USAGE`) |
| 70 | Tool / runtime error (`EX_SOFTWARE`) — missing binary, grammar load failure |

## Running the probe set

```bash
python3 scripts/prd-capability-check.py tests/prd-gate/example-probe-set.json
python3 scripts/prd-capability-check.py --json tests/prd-gate/example-probe-set.json
```

## Fixtures

| File | Probe kind | What it tests |
|---|---|---|
| `fixtures/arrow_type.ri` | grammar | No arrow-type grammar production (3979 class) — `param f : (Length) -> Length` → tree-sitter exits 1 |
| `fixtures/revolute_silent_accept.ri` | check | §3 4575 silent-accept — `revolute("not-an-axis", …)` → `reify check` exits 0, no rejection diagnostic |
| `fixtures/ir_clean_eval.ri` | ir | Clean eval baseline — `reify eval` exits 0 with no error |
| `fixtures/transform3_unresolved.ri` | check | 4577 — `param t : Transform3` → was exit 1, "unresolved type: Transform3"; **removed from corpus** (task 4577 landed Transform3 resolver — probe flipped PASS) |
| `fixtures/typeparam_member_access.ri` | check | 4437 — `constraint item.length > 5mm` (type-param bounded) → exit 1, "member access not yet supported: .length" |
| `fixtures/purpose_nested_structure.ri` | grammar | 4497 — nested `structure` inside `purpose {}` → was tree-sitter exit 1 (MISSING "}"); **removed from corpus** (grammar production landed — probe flipped PASS) |
| `fixtures/cross_sub_geometry_ref.ri` | check | 4358 — `let copy = self.inner.body` (cross-sub ref) → exit 0 with panic in stderr |
| `fixtures/scalar_codomain_mismatch.ri` | check | 4375 — `field def f : Length -> Scalar` → exit 1, "codomain mismatch" |

## Committed probe sets

| File | Description |
|---|---|
| `example-probe-set.json` | Example showing all three probe kinds (used in README and docs) |
| `corpus-probe-set.json` | δ historical-false-premise regression corpus — 4 rows, all FAIL |

## `match` predicate semantics

All set fields must hold simultaneously (AND semantics). An empty `match: {}` means
"no specific criterion" — the observation is determined by exit code and stderr alone
(used for grammar probes).

- `exit_code`: the process exit code must equal this integer
- `stderr_contains`: this string must appear in stderr
- `stdout_contains`: this string must appear in stdout

## Historical-false-premise regression corpus (δ)

`corpus-probe-set.json` is the δ committed probe-set (task 4609, PRD §10 producer-side
table / §11). It encodes 4 historical false premises as probe records so that
`scripts/prd-capability-check.py` can assert **all rows FAIL or UNPROVABLE**.

A row flipping to **PASS** means either:
- the substrate changed (premise now satisfied → **update corpus**), or
- the checker regressed (→ **gate fires**).

The gate runs automatically via `tests/infra/test_prd_gate_corpus.sh`
(auto-discovered by `run_all.sh`, skip-guarded on toolchain presence).

### Corpus rows

| Case | Fixture | probe_kind | expected.observation | match | Observed → Verdict |
|---|---|---|---|---|---|
| **3979** arrow-type grammar | `arrow_type.ri` | grammar | present | `{}` | ABSENT → **FAIL** |
| **4575** revolute silent-accept | `revolute_silent_accept.ri` | check | present | `exit_code:1` | ABSENT → **FAIL** |
| **4358** CrossSubGeometryRef panic | `cross_sub_geometry_ref.ri` | check | absent | `stderr_contains:"CrossSubGeometryRef should be consumed by entity.rs"` | PRESENT → **FAIL** |
| **4375** Scalar codomain mismatch | `scalar_codomain_mismatch.ri` | check | absent | `stderr_contains:"codomain mismatch"` | PRESENT → **FAIL** |

### Polarity and flip semantics

**Bug rows** (4358, 4375) use `observation=absent` + `stderr_contains` pinning
the bug's diagnostic signature.  While the bug exists the signature is PRESENT → FAIL.
When the substrate is fixed the signature disappears → ABSENT → verdict PASS → gate fires
("update corpus").  (4577 Transform3 unresolved and 4437 typeparam member access were
bug rows — both **removed from corpus** when their substrates landed and the probes
flipped PASS: 4577 via the Transform3 resolver here, 4437 on main via commit 9552cd760b.)

**The 4575 silent-accept row** uses `observation=present` + `exit_code:1` (PRD §9
negative-assertion): "revolute with invalid args should be rejected" — observed exit 0
(no rejection) → ABSENT → FAIL.

**Grammar row** (3979) uses `observation=present` + empty `match:{}`: "this syntax
should parse" — tree-sitter exits 1 (ERROR) → ABSENT → FAIL.

### Per-row encoding notes

**4358 — `probe_kind=check` (NOT `ir`):**
`reify check` (and `reify eval`) on the cross-sub-geometry-ref fixture emit the
`CrossSubGeometryRef` unreachable-panic to stderr, but the process **exits 0** (the panic
is swallowed per-realization).  α's `ir` vector gates on exit code first (`exit 0 → ABSENT`
unconditionally), so the eval-error proxy would return ABSENT → PASS — a silent false
negative.  α's `check` vector evaluates `match.stderr_contains` regardless of exit code,
so it cleanly observes the panic signature.

**4375 — FIELD codomain, not function codomain:**
Function `-> Scalar` codomains check lax/clean (exit 0) — they would be a silent false
negative like 4352.  A **field** whose declared `Scalar` codomain resolves to `Scalar[m]`
(bare Scalar → `Type::length()`, `type_resolution.rs:562`) and whose lambda body produces
dimensionless `Real` triggers exit 1 with a real diagnostic:
`"field 'f' codomain mismatch: declared codomain Scalar[m], lambda body produces Real"`.
This is the faithful observable of the unmigrated `-> Scalar` codomain
(W2 producer-extent; E_BARE_SCALAR not yet landed).

### Dropped case: 4352

Task 4352 (`Type::Real` deleted enum variant) was **dropped** from the static corpus per
Leo's ratification of esc-4609-216 (option A).  `Real` is a surface alias
(`type_resolution.rs:596 → dimensionless_scalar()`) that resolves clean (exit 0);
a deleted Rust enum variant is NOT statically observable at the surface via
grammar/check/IR probes.  PRD §10/§8 uniquely assign 4352 to `"FAIL at dispatch re-diff"`
(D4, dark-factory-owned), not a static probe.

### Running the corpus gate

```bash
# Run alpha directly (harness exits 1 = all FAIL, as expected):
python3 scripts/prd-capability-check.py tests/prd-gate/corpus-probe-set.json

# Run the full CI gate (includes skip-guard and all assertions):
bash tests/infra/test_prd_gate_corpus.sh
```

The gate is auto-discovered by `tests/infra/run_all.sh` and wired into CI via
`verify.sh:983` under a 20-minute timeout.  `reify check` is compile-only (no OCCT
realization, no `LD_LIBRARY_PATH` needed); the whole gate runs in < 10 s.
