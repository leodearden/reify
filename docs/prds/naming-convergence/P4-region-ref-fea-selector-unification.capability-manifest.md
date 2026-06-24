# P4 capability manifest — pose-vs-set FEA-target boundary guard

Mechanizes G3 + G6 for `docs/prds/naming-convergence/P4-region-ref-fea-selector-unification.md` §5.
One leaf (**P4-π**). Evidence verified against `main` 2026-06-24. Empty-value sentinel = `Value::Undef`
/ silent-accept (overlay). Reject side cited per overlay G3 §2 (negative-assertion sentinel).

## Leaf P4-π — Pose-vs-set FEA-target boundary guard

*Signal:* a committed fixture passing a `Value::Frame` (`frame3(point3(…), orient_identity())`) to FEA
region-target fields (`PressureLoad(face:)` / `PointLoad(point:)` **and** `FixedSupport(target:)`) makes
`reify check`/`eval` emit a structured **pose-vs-set diagnostic** (a pose is not a region target), where
today it silently exits 0 / "All constraints satisfied."; the v0.6-migrated region-target examples
still check clean. *Depends on:* 4370, 4811.

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `Value::Frame` exists; `frame3(point3(…), orient_identity())` constructs a pose | capability→producer (wired) | `grep:crates/reify-ir/src/value.rs:970-973` (`Frame{origin,basis}`); `frame3` builtin used in `examples/m10_combined.ri:60`; `@point→Frame` at `crates/reify-expr/src/lib.rs:1194-1228` | **PASS** |
| FEA region-target fields are selector-typed (so a `Frame` is a non-matching, non-selector value) | capability→producer (upstream) | `producer:task-4370` (v0.6 Bmig — `String→Selector` for the 5 FEA target fields) in P4-π's **transitive dependency closure** (`depends_on 4370`); fields confirmed still `String` on main today (`fea_multi_case.ri`), so the selector-typing is delivered by the upstream prereq, not assumed present | **PASS** |
| DAG-direction: `4370` and `4811` are **upstream** of P4-π | anti-inversion | P4-π `depends_on` 4370, 4811 (both pending/in-progress, in the DAG); neither depends on P4-π | **PASS** |
| **Rejection fires:** a `Value::Frame` at an FEA region-target is rejected with a pose-vs-set diagnostic — **not** a silent accept | rejection-mechanism (branch 4) | **Today = silent accept** (rejection-absent): `reify check` on `FixedSupport(target: frame3(point3(0mm,0mm,0mm), orient_identity()))` exits **0 + "All constraints satisfied."**, no diagnostic (this session — the 4575 silent-accept class). Capability **delivered by** `producer:task-4370` (selector-typed fields → type-conformance rejects a non-selector) **+ this task's verify-or-wire** (P4-π §5/D3 owns the explicit `validate_selector_target` `Frame`-reject + diagnostic and the extent across all target fields, replacing the opaque `_ => None`). **Observation deferred to post-4370 dispatch** — it cannot be observed pre-migration (fields still `String`); the §3 probe is re-run against post-4370 `main` (OQ#2). P4-π **owns** the extent ⇒ not `producer-extent-short`. | **PASS** (rejection delivered by 4370 upstream + self-owned wiring; deferred observation, gated) |
| No silent fabrication / regression: the v0.6-migrated region-target examples (`fea_cantilever_smoke.ri`, `fea_multi_case.ri`) still `reify check` clean after the guard | field-population / no-regression | `producer:task-4370` migrates the examples to region selectors (its own signal asserts they check clean); P4-π's guard targets only the `Frame`/pose case, leaving valid region targets unaffected — asserted by the same fixture batch | **PASS** |
| Grammar reality: the fixture syntax (`frame3(…)` as a named-arg target) parses with 0 ERROR nodes | grammar-fixture (anti-mismatch) | `grammar-fixture:/tmp/p4-gate/p4-pose-vs-set-{support,pressure}.ri` — `tree-sitter parse --quiet` exit 0 this session (§3 gate). **No novel syntax.** | **PASS** |
| Numeric floor | N/A | P4-π asserts **no** numeric bound (it is a rejection/diagnostic signal; the migrated solve is parity, owned by 4370/4371 — no new accuracy claim) | **N/A** |

**No FAIL binding.** The one binding that is *currently* unobservable (the rejection firing) is
resolved by the standard G3/G6 path — the rejection capability is queued **upstream** (4370, wired as a
hard `depends_on`) and the extent is **self-owned** by P4-π (verify-or-wire), with the observation
deferred to post-4370 dispatch (it is structurally impossible to observe pre-migration). The §3 live
probe records today's **silent-accept** baseline so a dispatch-time architect (and any verifier) can
diff intent against substrate rather than re-derive it. Queueing is not blocked.
