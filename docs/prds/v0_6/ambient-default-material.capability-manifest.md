# Capability manifest — ambient-default-material.md (2026-06-10)

| Leaf | Capability asserted by signal | Evidence | Verdict |
|---|---|---|---|
| A | `default Material = …` does NOT parse today (A is the producer) | grammar-fixtures `/tmp/prd-gate-fixtures/ambient-default-{1,2,3}.ri` → `tree-sitter parse` ERROR nodes 2026-06-10 | PASS producer-self (grammar-producer convention; `grammar_confirmed=false`) |
| B | trait-default injection machinery exists to host the ambient rung | grep: `conformance/checker.rs:1561-1844` (inject-if-absent), `:501-554` (param-default registration before let compilation) | PASS wired |
| B | grammar | producer: task A upstream (dep wired) | PASS producer-upstream |
| C | `Material(...)` ctor evaluates at runtime (default's value side) | probe-verified `RigidPost.mass = 23.55 kg` through `Material(name: …, density: 7850kg/m^3, …)` (`structural_traits_dimensioned.ri:19`) | PASS wired |
| C | ladder + water rung sites | grep: `resolve_body_density` `dynamics_ops.rs:91-110`, `W_DynamicsDefaultDensity` `:99-108` | PASS wired |
| C | cleaned B-side contract | producer: type-hygiene δ upstream (cross-PRD dep wired) | PASS producer-upstream |
| D | all capabilities | producer-upstream within batch (A/B/C) | PASS producer-upstream |

Numeric-floor branch: N/A. No FAIL bindings.
