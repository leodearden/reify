# Capability manifest — struct-ctor field-type conformance enforcement

PRD: `docs/prds/struct-ctor-field-type-conformance.md` (committed `af8c26ed96`; baselines amended at decompose time, same day).
Decompose session 2026-07-20; all `grep:file:line` evidence re-verified against main `56380a8f8a` (fresh `target/debug/reify` build; the stale Jun-30 `target/release/reify` was removed because the probe harness prefers it and its `include_str!`-embedded stdlib predated task 4370).
Probe evidence: D3 verification workflow run `wf_801582f3-59d` over the fixture set (`.orchestrator-scratch/prd-struct-ctor-decompose/`, transient; rebuilt as committed fixtures by α) + a direct local run of every probe recorded below.
Machine-readable twin: `struct-ctor-field-type-conformance.capability-manifest.yaml` (labels match `metadata.prd_task_label`; task ids stamped by `commit_planning`).

Baseline correction (load-bearing, discovered this session): PRD §2 matrix rows 5/8 were authored against a pre-4370 probe binary. On current main the sub `=` path routes named ctor args through `PendingBoundCheck::TraitArgConformance` (entity.rs:2596) into the walker, whose final-arm wrapper-shape check rejects ANY non-wrapper arg to an `Option<T>` param — so row 8 already errors (wrong code `TypeNotConformingToTrait`, "does not match wrapper shape") and row 5 (legitimate implicit-Some, bare selector → `Option<FaceSelector>`) **errors on main today**. α's Option-unwrap arm fixes that live hole. PRD §2/§7/§10-Q4 amended accordingly; no D1–D10 design decision changes.

## alpha — generalize the chokepoint at Warning severity

| capability | evidence | verdict |
|---|---|---|
| chokepoint entry wired on production path | grep: `check_expr_struct_ctor_args` invoked at `crates/reify-compiler/src/compile_builder/entities_phase.rs:1344` over every template root / fn / assoc-fn expr; fn at `:1575`; allowlist to flip at `:1598-1608` | PASS |
| second entry (sub path) wired | grep: `PendingBoundCheck::TraitArgConformance` pushed at `crates/reify-compiler/src/entity.rs:2596` (also `:2879`, `:2898`); shared executor `check_trait_arg_conformance` at `crates/reify-compiler/src/conformance/mod.rs:261` | PASS |
| walker + gate helper live | grep: `walk_param_against_arg_type` `crates/reify-compiler/src/conformance/mod.rs:864`; `reject_if_incompatible` `:827` (skip list `Error`/`TypeParam`/`Geometry`/`TraitObject`); silent final-arm comment `:985` ("a fully general arg-shape pass is future work" — this PRD is that pass) | PASS |
| predicate + extension pairs verified | grep: `type_compatible` `crates/reify-compiler/src/type_compat.rs:220`; unit tests pin the exact extension semantics — selector cross-kind false `:3091`, `List<Real>`←selector false `:3174`, dimensionless-Scalar←Int true `:3669` | PASS |
| rejection mechanism fires today (G6 branch 4) | rejection-check: probe6 (`FaceSelector`←`edges(...)`) exit 1 "requires selector type 'FaceSelector'"; probe9 (`FaceSelector`←`String`) exit 1 — walker→`type_compatible`→structured diagnostic live on main | PASS |
| silent-accept baselines (motivate the RED warnings) | rejection-absent-today observed as intended baseline: probes 1/2/4b/7 exit 0 + "All constraints satisfied." (value-cell `Option`-wrapped + `String`/concrete-leaf holes) | PASS |
| corrected sub-path baseline (rows 5/8) | rejection-check: probe4 (`Option<FaceSelector>`←`42`, sub `=`) exit 1 "does not match wrapper shape"; probe5 (bare selector→`Option<FaceSelector>`, sub `=`) exit 1 same arm — live implicit-Some hole α fixes; wrapper-shape arm at `conformance/mod.rs:985-1005` emits `TypeNotConformingToTrait` (Q2: α re-codes, since it touches that arm) | PASS |
| legality stays clean | probe3 (value-cell bare selector) exit 0; probe_row7 (`Int`→`Real` field) exit 0; `examples/fea_pressure_smoke.ri` exit 0 in CI | PASS |
| diagnostic infra | grep: `SelectorKindMismatch` `crates/reify-core/src/diagnostics.rs:584`; `ArgTypeMismatch` `:617`; `Diagnostic::warning` `:3836` | PASS |
| param-default extension substrate (D8) | grep: `check_param_default_conformance` `crates/reify-compiler/src/conformance/mod.rs:368` | PASS |
| grammar | no novel syntax — every fixture parses today (probes ran through `reify check`, which parses first); colon-form arg-bearing sub (`sub w : Widget2(label: 42)`) did NOT parse — §10 Q4 stays an α work item, not a grammar prerequisite | PASS |
| gate-test drift-guard | procedural: α's new gate-resident test binaries carry nextest-partition / run-all-classification registrations in the SAME diff (PRD §8; esc-4914-162 precedent) — stated in the task text, no upstream registration task needed | PASS |

## epsilon — unknown-field/over-arity tightening at Warning (severable)

| capability | evidence | verdict |
|---|---|---|
| lenient `__arg{i}` site exists | grep: `crates/reify-compiler/src/expr.rs:2700-2786` (unknown-named + over-arity append; duplicate-named-arg diagnostic precedent in the same region) | PASS |
| baselines silent today | probes 11 (unknown field `labl:`) and 12 (over-arity positional) exit 0 + "All constraints satisfied." | PASS |
| new codes mintable | All anchors in this cell verified at `09bf8a1734`. `E_CTOR_UNKNOWN_FIELD` / `E_CTOR_ARITY` mint as two fieldless unit variants appended to `DiagnosticCode`, following the `SelectorKindMismatch`/`ArgTypeMismatch` minting discipline — grep: `pub enum DiagnosticCode` `crates/reify-core/src/diagnostics.rs:156` (`#[non_exhaustive]`). No accessor to update: grep `impl DiagnosticCode` is **zero hits workspace-wide**. **No completeness gate fires when a variant is added** — `DiagnosticCode` derives no `strum` trait (zero `strum`/`EnumIter`/`EnumCount` hits in that file), so `DiagnosticCode::iter()`/`::COUNT`/`::VARIANTS` do not exist (zero hits repo-wide); `#[non_exhaustive]` forbids an out-of-crate exhaustive match, so there is no compile-time pressure either; and nothing anywhere scans declared-vs-used diagnostic codes (`DiagnosticCode` is zero hits across `scripts/`, `tests/infra/`, `hooks/`). ε therefore authors every check by hand in its own diff, per D2: the **normative** one is behavioural — compile the §7 row 11/12 fixtures and assert the emitted diagnostic's `code` and its severity at the D4 knob's configured setting, which is also §8's stated ε signal. The inline-`mod tests` variant-exists + `with_code` round-trip (exemplar `eval_cycle_and_unresolved_codes_exist_and_attach`) is optional smoke only: it proves that two unit variants are distinct and that a builder setter stores its argument, neither of which can catch a code minted but never wired to the ctor site. Two traps for any unit-level companion — `diagnostic_code_multi_kernel_variants_with_code_round_trip` loops a HARDCODED array, not an enumeration, so it is not a sweep; and the `rename_all = "PascalCase"` wire form exists ONLY under `reify-core`'s optional, non-default `serde` feature (`#[cfg_attr(feature = "serde", …)]` `crates/reify-core/src/diagnostics.rs:153-154`; `[features] serde = ["dep:serde"]` with no `default`, `crates/reify-core/Cargo.toml`), so a wire-form assertion must carry `#[cfg(feature = "serde")]` (exemplar `eval_codes_serialize_to_pascalcase_wire_strings`) and be run as `cargo test -p reify-core --features serde --lib` — under a bare `--lib` it is compiled out silently rather than failing. Symbols inside `diagnostics.rs` are cited by name, not line, because ε's own appends shift every anchor below the enum body. The repo's real completeness precedents split into two classes and `DiagnosticCode` follows NEITHER — **(i) strum-derived sweeps**: `TypeDiscriminants`, derived at `crates/reify-core/src/ty.rs:207-208` and consumed via `TypeDiscriminants::iter()` by `every_type_variant_is_claimed_by_exactly_one_member_access_handler_class` `crates/reify-compiler/src/expr.rs:9882`; and `GeometryOp` via `GeometryOpDiscriminants`, declared `#[strum_discriminants(name(GeometryOpDiscriminants), derive(strum::EnumIter, strum::EnumCount, Hash))]` at `crates/reify-ir/src/geometry.rs:568` and consumed as `GeometryOpDiscriminants::iter()` at `crates/reify-eval/src/engine_build/tests.rs:6765` — those two `strum_discriminants(name(…))` sites are the only ones in `crates/`; **(ii) hand-rolled `const ALL_*` + `VARIANT_COUNT` checks**: `compile_geometry_op_registry_completeness` `crates/reify-eval/src/geometry_ops/tests.rs:20435`, emphatically NOT strum-based (zero `strum`/`EnumIter`/`EnumCount` hits in that whole file) — it cross-checks hand-written arrays such as `const ALL_PRIMITIVE: [PrimitiveKind; 8]` against `PrimitiveKind::VARIANT_COUNT`, itself the hand-written `pub const VARIANT_COUNT: usize = Self::ALL.len();` `crates/reify-compiler/src/types.rs:1388`, never strum's `EnumCount::COUNT`. Class (ii) is the CLOSER analogue for ε: like those kinds, `DiagnosticCode` gets no derive-driven sweep, so ε must author the check by hand rather than expect one to go red. Verdict stays PASS: the capability (mintable) is true, and cheaper than this cell once claimed — only its evidence was fabricated. Corrected 2026-08-12 by task 6224; the text it replaced (a named strum guard, and a declared-vs-used scan, neither of which exists anywhere in the tree) is recoverable from that task's commits. | PASS |
| severity knob shared | the α-introduced knob governs ε's diagnostics too (producer: task alpha, upstream dep) | PASS |

## beta — corpus survey artifact (mechanized)

| capability | evidence | verdict |
|---|---|---|
| warn-emitting compiler upstream | producer: tasks alpha + epsilon (hard `add_dependency` edges, upstream) | PASS |
| corpus enumerable | `git ls-files '*.ri' | wc -l` = 493 on `56380a8f8a` — matches the PRD's stated count exactly | PASS |
| artifact path free | `docs/prds/struct-ctor-field-type-conformance.survey.md` does not exist on main | PASS |

## gamma — corpus fix-forward

| capability | evidence | verdict |
|---|---|---|
| survey upstream | producer: task beta (hard dep edge) | PASS |
| fix rules normative | PRD D9 (FEA: call-site only; non-FEA: per-case, recorded in the survey artifact) | PASS |
| verification gate exists | `--scope all --profile both` merge gate (scripts/verify.sh; the signal is zero ctor-conformance warnings corpus-wide + gate green) | PASS |

## delta — severity flip Warning→Error + negative fixtures (headline leaf)

| capability | evidence | verdict |
|---|---|---|
| severity knob upstream | producer: task alpha (single const, D4); knob name is α's Q3 decision — check is manual, not pattern-bound | PASS |
| warn-clean corpus upstream | producer: task gamma (zero warnings corpus-wide before the flip) | PASS |
| headline baselines silent today (G6 branch 4) | probes 1/2/7 (`face: frame3(...)`, `Widget(label: 42)`, `face: "x_max"`) exit 0 + "All constraints satisfied." on main — the RED premise δ turns green | PASS |
| exit-code channel live | rejection-check: probes 6/9 exit 1 today — Error-severity ctor-conformance diagnostics already drive `reify check`'s non-zero exit; δ changes severity only, not exit mechanics | PASS |
| no capability owned downstream | δ's signal needs only α/ε (mechanism) + γ (clean corpus), all upstream; consumers 4833/4371/v0.6 re-wire onto δ in their own PRDs (cross-PRD table §9) | PASS |

## zeta — language-spec + docs

| capability | evidence | verdict |
|---|---|---|
| spec file exists | `docs/reify-language-spec.md` on main (referenced by live `W_MODULE_DECL_MISSING` diagnostics as "spec §7.1") | PASS |
| semantics final upstream | producer: task delta (spec documents the landed Error-severity semantics) | PASS |
| G7 waiver recorded | no-lockstep-duplication — spec restates C1 as the living normative copy; agreement pinned by committed boundary fixtures, not hand-sync (waiver in PRD §8 ζ row + `metadata.g7_waivers`) | PASS (waived) |
