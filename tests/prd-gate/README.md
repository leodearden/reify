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

## `match` predicate semantics

All set fields must hold simultaneously (AND semantics). An empty `match: {}` means
"no specific criterion" — the observation is determined by exit code and stderr alone
(used for grammar probes).

- `exit_code`: the process exit code must equal this integer
- `stderr_contains`: this string must appear in stderr
- `stdout_contains`: this string must appear in stdout
