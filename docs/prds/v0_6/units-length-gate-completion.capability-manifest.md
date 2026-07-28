# Capability manifest — `units-length-gate-completion`

PRD: `docs/prds/v0_6/units-length-gate-completion.md` (landed `54afdee50b`).
Decomposed 2026-07-28. Machine-readable twin: `units-length-gate-completion.capability-manifest.yaml`.

**All substrate re-verified this session against `main` at `638d97d8ab`** (call-site re-measurement
for 5662 at `209dc5bf24`; the only commits between the two are docs/PRD artifacts, `scripts/verify.sh`
and `tests/prd-gate/fixtures/*.ri` — no units-relevant source changed). Probe binary
`target/release/reify` built 2026-07-28 20:47, newer than every units-relevant source.

## D3 verification run (Enumerator → Prover ‖ Adversary → Synthesize)

`scripts/prd-decompose-verify.py bind` → `scripts/prd-capability-check.py --json` →
`scripts/prd-decompose-verify.py synthesize`. (The `Workflow` tool was unavailable to this
background session, so the deterministic harness was driven directly and the three roles were
played in-session. The harness is the load-bearing part per the overlay: "All load-bearing logic
lives in the tested Python harness; the `.mjs` is a thin orchestration shell.")

| Role | Probes | Result |
|---|---|---|
| Prover | 22 | 22 PASS |
| Adversary | 5 | 5 PASS |
| **Synthesize** | 27 | **`blocks: false`, `blocking: []`, exit 0** |

No `FAIL`, no `UNPROVABLE`, no `HARNESS_ERROR`. Six non-probe Adversary findings were resolved by
rewriting task text before filing (§Adversary findings below) — none blocked the batch.

### The rejection-mechanism binding for the whole PRD (G6 branch 4)

Every leaf in this PRD asserts "a bare number at a dimensioned position is rejected". By
construction that rejection does **not** fire today — delivering it is the PRD. The negative-
assertion mandate is therefore discharged by proving the **rejection MECHANISM exists and is
observed to fire on an already-gated position**, plus capturing each leaf's silent PRE-STATE:

```
$ reify eval p2_mirror_scalar_gated.ri      # mirror(b, 10, 0, 0, 1, 0, 0)
warning: mirror: ox argument expects Length, got Int; pass a dimensioned length such as `5mm`
error: failed to compile geometry operation: missing or non-Length argument 'ox' for mirror
$ echo $?
1
$ reify check p2_mirror_scalar_gated.ri
All constraints satisfied.
$ echo $?
0
```

The mechanism (`accept_arg` + `length_spec` + `ArgRejection::message` + the Error-severity exit
gate) is live; every leaf below extends it to a route where it is currently absent. The `check`
half is the D8 evidence: eval-layer gates are **check-invisible**, which is why every signal in
this batch is phrased against `reify eval`.

### Captured pre-state / control probes (all PASS)

| Fixture | Command | Exit | Observed |
|---|---|---|---|
| `box(20, 20, 10)` | `reify eval` | 0 | no diagnostic — R7 fully silent |
| `box(20, 20, 10)` | `reify check` | 0 | `All constraints satisfied.` |
| `box(20mm, 20mm, 10mm)` | `reify eval` | 0 | control |
| `mirror(b, 10, 0, 0, 1, 0, 0)` | `reify eval` | **1** | Contract C rejection **fires** |
| `mirror(b, plane_yz(10))` | `reify eval` | 0 | value form silently accepted |
| `fillet(box(10mm,10mm,10mm), 1)` | `reify eval` | 1 | `OCCT make_fillet_with_history: unexpected: BRepFilletAPI_MakeFillet failed` — no op, no field, no span |
| `linear_pattern(b,1,0,0,3,20)` | `reify check` | **1** | `linear_pattern: spacing argument expects Length, got Int` — compile slot has teeth |
| `linear_pattern_2d(b,1,0,0,3,20)` (arity 6) | `reify check` | 1 | `linear_pattern_2d() expects 11 arguments, got 6` |
| `apply_transform(b, transform3(…, vec3(5,0,0)))` | `reify eval` | 1 | generic `'transform' arg is not a valid Transform<3>` — **not** a units diagnostic |
| `affine_translate(5kg, 0kg, 0kg)` | `reify eval` | 0 | `translation=[5, 0, 0]` — MASS silently discarded |
| `plane_xy(0.0)` | `reify eval` | 0 | `plane(point(0, 0, 0), …)` — dimensionless origin |
| `plane_xy(0mm)` | `reify eval` | 0 | `plane(point(0 m, 0 m, 0 m), …)` — LENGTH origin (make_zero mirrors) |
| `param s : Length` + `linear_pattern(b,1,0,0,3,s)` | `reify eval` | 1 | `argument 'spacing' for linear is unresolved (Undef)` — λ's label is reachable |
| `cylinder_centered(5mm,20mm)` / `rounded_rect` / `rounded_box` / `revolve_full` | `reify eval` | 0 | α's four controls |

**Grammar gate:** every fixture above parsed with `tree-sitter parse --quiet`, exit 0, zero ERROR
nodes. `grammar_confirmed: true` batch-wide; **no novel grammar in this PRD** (the middle-dot
grammar change is PRD 3's).

## Per-leaf bindings

Verdict values: `PASS` = evidence found; any of `declared-only | test-only | producer-absent |
producer-extent-short | producer-downstream | fixture-ERROR | bound≤floor | rejection-absent`
would block. **No binding in this manifest resolves to a blocking value.**

### α — 5742 — dimension the compiler's synthesized geometry literals

| Capability | Binding | Verdict |
|---|---|---|
| the four synthesized-literal sites exist | `grep:crates/reify-compiler/src/geometry.rs:1503,2362,2439,2525` — `CompiledExpr::literal(Value::Real(…), Type::dimensionless_scalar())`; TAU at `:2065`. All four re-verified 2026-07-28 | PASS |
| all four builtins are live and evaluate today | probe: `cylinder_centered(5mm,20mm)`, `rounded_rect(20mm,10mm,2mm)`, `rounded_box(20mm,10mm,5mm,1mm)`, `revolve_full(…)` each `reify eval` exit 0 — retyping them is a real edit, not a fiction (Adversary probe on the newly-added `rounded_box` pair) | PASS |
| `Type::dimensionless_scalar()` has LENGTH/ANGLE counterparts | wired-on-main: the same module already emits dimensioned literals; `reify_core::Type` is the producer | PASS |
| DAG-direction | α is upstream of β/5623 and everything else — the prerequisite it must be | PASS |

### β — 5743 — R7 chokepoint: primitives + profiles + the shared DiagnosticCode

| Capability | Binding | Verdict |
|---|---|---|
| `accept_arg` / `ArgSpec` / `Acceptance` / `ArgRejection` | `grep:crates/reify-eval/src/arg_acceptance.rs:117 / :28 / :40 / :52` — wired on main; `length_spec` at `:103`, `density_spec` at `:86` | PASS |
| rejection wording template | `grep:crates/reify-eval/src/arg_acceptance.rs:69` `ArgRejection::message(builtin, arg_name)` | PASS |
| the rejection is **observed to fire** (G6 branch 4) | `rejection-check:mirror(b,10,0,0,1,0,0)` → `reify eval` exit 1 with the exact template message (captured above) | PASS |
| the 38 un-gated slots exist | `grep:crates/reify-ir/src/geometry.rs:575-1082` — 46 `Value`-typed GeometryOp fields = 41 length-semantic (38 un-gated + `spacing`/`spacing1`/`spacing2` gated) + 3 dimensionless normal components + 2 angle | PASS |
| the pre-state is silent (motivation, not a required capability) | probe: `box(20,20,10)` exits 0 on both `check` and `eval` | PASS |
| `DiagnosticCode` registry exists to extend | `grep:crates/reify-core/src/diagnostics.rs:497` `DimensionMismatch` (Add/Sub-specific per its own doc at `:604`) — β mints a NEW shared code beside it | PASS |
| the Undef diagnostic β routes into exists | `grep:crates/reify-eval/src/geometry_ops.rs:344-347` `"argument '{}' for {} is unresolved (Undef)"`; probed live via λ's fixture | PASS |
| DAG-direction | α (5742) upstream; β upstream of γ/δ/ζ/μ/λ/ι/ο | PASS |

### γ — 5744 — R7 chokepoint: modify + sweep

| Capability | Binding | Verdict |
|---|---|---|
| β's chokepoint helper | `producer:task-5743` upstream, hard edge wired; extent covers the additive raw-`Value` helper + the shared code | PASS |
| the pre-state failure is an unattributable kernel error | `rejection-check:fillet(box(10mm,10mm,10mm),1)` → exit 1, `BRepFilletAPI_MakeFillet failed`, naming no op/field/span (captured) | PASS |
| the modify/sweep slots exist | `grep:crates/reify-ir/src/geometry.rs:575-1082` (ModifyKind/SweepKind `Value` fields) | PASS |
| no arc-magnitude premise | PRD D13 — no signal built on arc sweep magnitudes; γ's signal is `fillet` radius only | PASS |

### δ — 5745 — R3: decoded value-form routes

| Capability | Binding | Verdict |
|---|---|---|
| `point3_components` and its callers | `grep:crates/reify-eval/src/geometry_ops.rs:766` (no dimension check); callers at `:821`/`:824` (decode_plane), `:861`/`:863` (decode_axis), `:1127`/`:1149` (NurbsSurface control points), `:2167` (offset_curve direction) | PASS |
| the value-form hole is real | probe: `mirror(b, plane_yz(10))` → `reify eval` exit 0, `reify check` exit 0 | PASS |
| the gate wording δ reuses already exists | probe: the scalar form of the SAME builtin exits 1 with `missing or non-Length argument 'ox' for mirror` | PASS |
| **direction callers are excluded** | Adversary binding: `:824`, `:863`, `:2167` are unit-vector/direction positions that must stay dimensionless — recorded in the task text so δ cannot over-gate | PASS |
| `sample(field, point3(…))` is NOT δ's | `grep:crates/reify-expr/src/lib.rs:293` — `sample` is a reify-expr builtin, not routed through `point3_components`. `examples/imported_field/openvdb_stress.ri:36-38` bare coords are PRD 5 territory (see Cross-PRD note) | PASS |

### ε — 5746 — R11: make_plane/make_axis producers + the flipped locking test

| Capability | Binding | Verdict |
|---|---|---|
| `make_plane` / `make_axis` / `make_zero` | `grep:crates/reify-stdlib/src/geometry.rs:1112 / :1152 / :1125-1134` | PASS |
| the locking test to flip | `grep:crates/reify-stdlib/src/geometry.rs:2928` `plane_xy_real_zero_produces_dimensionless_origin` | PASS |
| the post-gate behaviour is already proven | probe: `plane_xy(0mm)` → `plane(point(0 m, 0 m, 0 m), vec(0,0,1))` — `make_zero`'s dimension mirroring yields a LENGTH origin triple for free. The pre-state `plane_xy(0.0)` → `plane(point(0, 0, 0), …)` | PASS |
| DAG-direction | δ (5745) upstream — the consumer side lands first, then the producer side | PASS |

### ζ — 5747 — R8 + R12 transform routes

| Capability | Binding | Verdict |
|---|---|---|
| the `LENGTH \|\| DIMENSIONLESS` admission | `grep:crates/reify-eval/src/geometry_ops.rs:10318-10322` — re-read this session, admission confirmed verbatim | PASS |
| `decompose_xyz3` consistency-only contract | `grep:crates/reify-stdlib/src/geometry.rs:19-26`; `affine_translate` `:446`, `affine_map` `:460`; the linear part IS already checked dimensionless at `:468-470` | PASS |
| R12's silent discard is real | probe: `affine_translate(5kg,0kg,0kg)` → exit 0, `translation=[5, 0, 0]` — a genuine 0→1 exit-code change | PASS |
| **R8's change is the MESSAGE, not the exit code** | Adversary probe: `apply_transform(b, transform3(…, vec3(5,0,0)))` ALREADY exits 1 today with the generic `'transform' arg is not a valid Transform<3>`. Recorded in the task so no RED test asserts a false exit-code flip | PASS |

### η — 5750 — LENGTH `CheckableArg` compile slots

| Capability | Binding | Verdict |
|---|---|---|
| `CheckableArg` / `ExpectedArg` | `grep:crates/reify-compiler/src/builtin_signatures.rs:80-88` (struct) and `:92-110` (`ExpectedArg` **enum**, `Scalar{dimension,type_name}` \| `Int{type_name}`) — the PRD's drift note re-confirmed | PASS |
| `builtin_arg_slots` keyed `(name, arity)` | `grep:crates/reify-compiler/src/builtin_signatures.rs:145-297`; consumer `check_builtin_arg_types` at `:325` | PASS |
| the 7 existing LENGTH slots (pattern to follow) | `grep:builtin_signatures.rs:186,194,211,254,280,288` — `edges_at_height` h+tol, `extremal_by_*` tol, `linear_pattern` spacing @6, `linear_pattern_2d` spacing1+spacing2 @11 | PASS |
| `NON_SELECTOR_ARG_SLOT_KEYS` + its coverage invariant | `grep:builtin_signatures.rs:474` (the const) and `:727` (`arg_slot_keys_are_registered_builtin_names`), `MAX_PROBED_ARITY = 14` at `:420` | PASS |
| the compile slot **is observed to fire at `reify check`** | `rejection-check:linear_pattern(b,1,0,0,3,20)` → `reify check` exit 1 with `linear_pattern: spacing argument expects Length, got Int` | PASS |
| the two inverting assertions | `grep:crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:1729,:1783` — flipped deliberately per C6 | PASS |
| **boundary row 9b premise CORRECTED** | Adversary: `compile_api_tests.rs:216-235` is `compile_linear_pattern_2d_wrong_arity_produces_diagnostic`, a deliberate arity-error negative fixture (already annotated by task 5652), and the arity guard rejects it (probe: exit 1, `linear_pattern_2d() expects 11 arguments, got 6`). Resolution written into the task: **no arity-6 LENGTH slot**, fixture stays bare, subtract from the migration list | PASS |
| this leaf needs **no PRD 2 dependency** | probe: the compile layer is already check-visible today (D8) | PASS |

### θ — 5751 — kernel tripwire

| Capability | Binding | Verdict |
|---|---|---|
| both kernel numeric-extraction boundaries | `grep:crates/reify-kernel-occt/src/lib.rs:193` and `crates/reify-kernel-fidget/src/kernel.rs:349` — `fn extract_f64` in both | PASS |
| the pre-state is span-less | probe: `OCCT make_fillet_with_history: unexpected: BRepFilletAPI_MakeFillet failed` (captured) | PASS |
| it is a tripwire, not a gate (no bound to clear) | PRD D5 — no numeric bound asserted; the 546/242/40 legitimate `Value::Real` kernel-test inputs stay green by construction | PASS |

### ι — 5752 — closure guard (+ Contract C doc shrink)

| Capability | Binding | Verdict |
|---|---|---|
| the probe seam that makes the guard buildable | `grep:crates/reify-eval/src/geometry_op_characterization_probe.rs:39` `pub fn compile_geometry_op_probe`, `#[cfg(any(test, feature = "test-instrumentation"))]` | PASS |
| the feature + self-dev-dep are already wired | `grep:crates/reify-eval/Cargo.toml:67` (`test-instrumentation = []`) and `:80-81` (`reify-eval = { path = ".", features = ["test-instrumentation"] }`) — no Cargo work needed | PASS |
| the universe (independent of the assertion target) | `grep:crates/reify-compiler/src/units.rs:21-86`; **counted programmatically this session: 64 names** (the research's 66 was a line count). The `>= 60` floor in the guard is below the measured 64 | PASS |
| the driver precedent | `grep:crates/reify-eval/tests/compile_geometry_op_characterization.rs:200` `run(op, step_handles)` — builds from `CompiledExpr::literal`, empty `ValueMap`/`named_steps`, no parser, no kernel | PASS |
| the anti-vacuity template | `grep:crates/reify-eval/tests/version_id_discipline_gate.rs:272,303,337,357,378,408,444,492` — 7 seeded self-tests + 1 real-tree scan | PASS |
| the assertion target is observable | probe: `mirror(b,10,0,0,1,0,0)` → `failed to compile geometry operation` (captured) | PASS |
| the gates it asserts over are **upstream** | `producer:task-5744,5745,5746,5747,5750,5623,5658,5661` — all hard `add_dependency` edges; DAG-direction correct, no `producer-downstream` | PASS |
| **drift-guard registrations same-diff** (C7) | `.config/nextest.toml` and `tests/infra/run-all-classification.manifest` both exist and are the registration points; ι's own diff carries them. **Not** a downstream sibling task, **not** prose-ordered — the esc-4914-162 rule is satisfied by same-diff, the stronger of the two sanctioned forms | PASS |
| the Contract C note ownership is unambiguous | hard `add_dependency` 5752 → 5658 wired, so ι lands later and owns the final text per PRD §8 ("whichever lands later owns the final text") | PASS |
| residuals are owned and loud | INV-SF-5 / PRD D14 — every allowlist residual cites a live `#NNNN` per the PTODO grammar; blanket escapes banned. Enforced by `reify-audit --pattern PTODO` on the gate | PASS |

### κ — 5754 — VARIANT_COUNT backstops

| Capability | Binding | Verdict |
|---|---|---|
| the single existing `VARIANT_COUNT` | `grep:crates/reify-compiler/src/types.rs:1448` `pub const VARIANT_COUNT: usize = Self::ALL.len();` — workspace-wide grep confirms exactly one | PASS |
| the 8 families that lack it | `grep:crates/reify-compiler/src/types.rs:1335,1378,1469,1499,1521,1559,1588,1616` — PrimitiveKind, BooleanOp, TransformKind, PatternKind, SweepKind, CurveKind, ProfileKind, SurfaceKind | PASS |
| **two** enforcement precedents, not one | Adversary: compile-time `const _: () = assert!(CASES.len() == ModifyKind::VARIANT_COUNT, …)` at `crates/reify-compiler/src/geometry_modify.rs:1006`, **and** runtime `assert_eq!(ALL_MODIFY.len(), ModifyKind::VARIANT_COUNT, …)` at `crates/reify-eval/src/geometry_ops/tests.rs:20435`. The PRD named only the second; both recorded in the task | PASS |
| non-vacuity | κ's signal requires a **seeded** demonstration for at least one family — not merely "the constant exists" | PASS |

### λ — 5755 — diagnostic labels + `isosurface.iso`

| Capability | Binding | Verdict |
|---|---|---|
| the label sites | `grep:crates/reify-compiler/src/types.rs:1510` (`Linear => "linear"`) and `:1513` (`Linear2D => "linear_2d"`), pinned by the table test at `:1876`/`:1879` | PASS |
| **the label is user-visible** (reachability, Adversary-resolved) | The obvious route is SHADOWED: the bare-literal `linear_pattern(b,1,0,0,3,20)` never reaches the eval label because η's compile slot fires first. The reachable route was found and probed: `param s : Length` (no default) → `reify eval` exit 1, `argument 'spacing' for linear is unresolved (Undef)`. Written into the task as λ's RED fixture. Cross-check: the sibling `PatternKind::Mirror` label already surfaces correctly (`… for mirror`) | PASS |
| the `isosurface` defect site | `grep:crates/reify-eval/src/geometry_ops.rs:1374` — un-checked `v.as_f64()`; the `None => 0.0` at `:1368` is the deliberate documented default (`:1358-1363`), which stays (D12) | PASS |
| DAG-direction on the shared file | κ (5754) upstream on `types.rs`; β (5743) upstream on `geometry_ops.rs` | PASS |

### μ — 5757 — GUI parameter editor

| Capability | Binding | Verdict |
|---|---|---|
| `parse_value_string` and its **production** caller | `grep:gui/src-tauri/src/engine.rs:5932` (fn) and `:2051-2057` `EngineSession::set_parameter` → `parse_value_string(value_str)?`. Wired on the production path, not test-only | PASS |
| both `UNIT_TABLE` lookup sites | `grep:gui/src-tauri/src/engine.rs:5918-5924` (5 entries: deg/rad/mm/cm/m) and the second independent lookup at `:4870` | PASS |
| the delegation target | `grep:crates/reify-core/src/units.rs:24` `pub fn unit_symbol_to_si` — 13 symbols; the GUI lacks `in`, `kg`, `g`, `s`, `K`, `A`, `mol`, `cd` | PASS |
| the longest-suffix invariant to preserve | `grep:gui/src-tauri/src/engine.rs:5948` `debug_assert!(UNIT_TABLE.windows(2).all(…))` — sorted by descending suffix length, else `m` shadows `mm` | PASS |
| **the test surface is Rust, not vitest** | Adversary correction to PRD §8: `grep:gui/src-tauri/src/tests/engine_tests.rs:17,1737` — the `parse_value_string_*` family. `parse_value_string` is Rust in `src-tauri`; vitest cannot see it. Recorded in the task | PASS |

### ν — 5759 — doc-chunk update, registry-verified

| Capability | Binding | Verdict |
|---|---|---|
| the chunks exist | `grep:crates/reify-mcp/src/tools/chunks/units.md` (52 lines) and `geometry.md` (127 lines) | PASS |
| the registries to verify against | `grep:crates/reify-compiler/src/{geometry,geometry_curve,geometry_transform,geometry_modify,geometry_boolean}.rs` + `crates/reify-compiler/src/units.rs` name registries | PASS |
| the fence-compile gate that makes the signal mechanical | the chunk-fence compile gate (task #5479/#5480 family) — every ```reify fence compiles as written | PASS |
| DAG-direction | γ (5744) and η (5750) upstream — the chunk documents behaviour that already landed, never behaviour it hopes for | PASS |

### ξ — 5760 — exemplar corpus + cheatsheet + discoverability (LEAF)

| Capability | Binding | Verdict |
|---|---|---|
| the corpus + its compile gate | `grep:examples/best_practices/INDEX.md`, `crates/reify-compiler/tests/examples_smoke.rs` — the auto-compile + bidirectional-index invariant | PASS |
| the cheatsheet index | `grep:.claude/skills/reify-design/SKILL.md` | PASS |
| **the stale claim to correct is real** | `grep:examples/best_practices/symmetry_mirror.ri:30-35` — "TRAP: that error does NOT appear under `reify check` … A green check is not evidence that a geometry call's argument dimensions are right." True today (probed), FALSE for statically-visible positions after η | PASS |
| DAG-direction | ν (5759) upstream → transitively γ, η, β. The exemplar is written against landed behaviour | PASS |
| coordination hazard recorded | 5662 also touches `symmetry_mirror.ri` (a comment block); both tasks carry the cross-reference | PASS |

### ο — 5761 — B+H integration gate, the 20-row two-way suite (LEAF)

| Capability | Binding | Verdict |
|---|---|---|
| every capability the 20 rows assert | `producer:task-5743,5744,5745,5746,5747,5750,5751,5752,5754,5755,5757,5759,5760,5623,5658,5661,5662` — **all 17 are hard `add_dependency` edges and all are upstream**. No row's capability is owned by a task that depends on ο | PASS |
| rows 1–8, 18–19 (rejection rows) | each is backed by a captured pre-state probe in the table above plus the producing leaf's own gate | PASS |
| rows 10–12 (guard rows) | `producer:task-5752` upstream; the anti-vacuity shape is contract C5 | PASS |
| rows 13–14 (kernel rows) | `producer:task-5751` upstream | PASS |
| row 15 (registry row) | `producer:task-5754` upstream | PASS |
| rows 16–17 (GUI rows) | `producer:task-5757` upstream; `in` confirmed present in `unit_symbol_to_si` and absent from the GUI table | PASS |
| row 20 (corpus row) | `producer:task-5760` upstream (added at decompose beyond the PRD's stated dep list, so the row cannot assert corpus greenness before ξ's new exemplar exists). Corpus re-census this session found **no** `.ri` migration owed by *this PRD's* gates | PASS |
| no row depends on PRD 2 | D8 — phrased against `reify eval`; rows 9/9b additionally use `reify check`, which already works via compile slots (probed) | PASS |
| **drift-guard registrations same-diff** (C7) | as ι — `.config/nextest.toml` / `run-all-classification.manifest` / wallclock-bounds registrations land in ο's own diff, not as a downstream sibling | PASS |

### 5623 / 5658 / 5661 / 5662 — adopted in-flight leaves

| Capability | Binding | Verdict |
|---|---|---|
| 5623's `eval_named_arg_f64` consumers | `grep:crates/reify-eval/src/geometry_ops.rs:202` (the helper), `:325` `required_length_arg` (the chokepoint it routes into) | PASS |
| 5658/5661's `eval_all_args_to_f64` | `grep:crates/reify-eval/src/geometry_ops.rs:418` — the four call sites 5661 enumerates | PASS |
| 5623 gets α as a hard upstream | `producer:task-5742` — edge wired; without it, gating `transform_translate` dx/dy breaks `cylinder_centered` | PASS |
| 5662's compile-slot machinery | `producer:task-5750` upstream (edge wired) | PASS |
| **5662's stated premise was FALSE — corrected** | 5662 justified leaving the value form slot-free on "5214's eval gate covers it". Probe: `mirror(b, plane_yz(10))` exits 0. The hole is task 5745's (δ). Correction written into 5662; its arity guards are retained on their own (correct) merits | PASS |
| **5662's call-site count re-measured** | ">= 6" → **4** live sites at `209dc5bf24` (let_scope_tests.rs:722,:863; compile_api_tests.rs:141; mirror_circular_value_forms_e2e.rs:451). Three of the seven listed are now comments — including `symmetry_mirror.ri:27`, whose real call at `:68` is already `0mm` | PASS |
| 5662 must stay LIVE | `grep:crates/reify-compiler/src/builtin_signatures.rs:48` `TODO(#5662)` — cancelling it would orphan a live PTODO citation | PASS |

## Adversary findings (non-probe; all resolved into task text before filing)

1. **η boundary row 9b premise partially false.** The arity-6 `linear_pattern_2d` site is a
   deliberate arity-error negative fixture and the arity guard already rejects it (exit 1). It is
   neither a legitimate overload nor a malformed fixture. → η rewritten: no arity-6 slot, fixture
   stays bare, site subtracted from the migration list.
2. **λ row 18 reachability.** The obvious bare-literal route is shadowed by η's compile slot. Found
   and probed the reachable Undef route (`param s : Length`, no default). → λ's RED fixture pinned.
3. **μ's test surface mis-stated.** `parse_value_string` is Rust in `gui/src-tauri`; the PRD said
   "vitest". → μ repointed at `gui/src-tauri/src/tests/engine_tests.rs` + the `set_parameter`
   production caller.
4. **ζ's R8 signal is a message change, not an exit-code change** — `apply_transform` with a bare
   `vec3` already exits 1 today via the generic shape check. → ζ's task text splits the two halves
   so no RED test asserts a false exit-code flip.
5. **Today's Contract C rejection is Warning-severity**, paired with an Error from the op-compile
   failure. C1(iv) wants the eval-layer rejection itself Error-severity. → β instructed to promote
   it (or keep the paired Error) deliberately and record which.
6. **κ's precedent has two shapes**, a compile-time `const _: () = assert!` and a runtime
   `assert_eq!`. The PRD named only the runtime one. → κ instructed to mirror both.

## Cross-PRD notes (recorded, not filed as work here)

- **PRD 5 owes a corpus migration this PRD does not.** `examples/imported_field/openvdb_stress.ri`
  lines 36–38 use bare `point3(0.85, 0.0, 0.0)` inside `sample(...)`. That routes through
  reify-expr's `sample` builtin (`crates/reify-expr/src/lib.rs:293`), **not** through this PRD's
  `point3_components`, so it is out of δ's scope — but it IS CI-gated
  (`crates/reify-cli/tests/harness_cli/cli_imported_field_eval.rs`) and falls squarely inside PRD 5's
  `point3` second universe. PRD 1's "zero `.ri` corpus migrations" claim is **true for PRD 1's own
  gates**; it is not a claim about PRD 5's.
- **PRD 2** (`check-diagnostic-truthfulness`) decomposed on 2026-07-28 (`209dc5bf24`). Per D8 **no
  leaf in this batch depends on it** — verified row by row. No cross-PRD edge wired, by design.
- **PRD 3** consumes α's ANGLE-typed `revolve_full` TAU literal and extends ι's guard allowlist/
  universe. Additive only — never a harness rewrite.

## G7 walk (`docs/legibility/design-invariants.md`) — no waivers

| Invariant | Verdict |
|---|---|
| INV-SF-1 `undef-has-provenance` | PASS — D10 routes `Acceptance::Undefined` into the existing "unresolved (Undef)" diagnostic at every new chokepoint (probed live). No new silent-Undef path. |
| INV-SF-2 `error-severity-exits-nonzero` | PASS — C1(iv); rejections are Error severity and `reify eval` exits nonzero via the severity gate. **No per-code escalation list added.** Adversary finding 5 is the concrete obligation on β. |
| INV-SF-3 `declared-intent-consumed-or-diagnosed` | N/A — no declaration is dropped. |
| INV-SF-4 `indeterminate-attributable-transient` | N/A — no new Indeterminate outcome. |
| INV-SF-5 `placeholders-owned-and-loud` | PASS — D14; every guard-allowlist residual cites a live `#NNNN` per the PTODO grammar, blanket escapes banned. 5662 kept live so `builtin_signatures.rs:48`'s `TODO(#5662)` stays owned. |
| INV-SF-6 `diagnostics-carry-codes` | PASS — D9; β mints one shared `DiagnosticCode` and retrofits the code-less Contract C sites. Probe confirms today's `warning: mirror: ox argument expects Length, got Int` carries **no** code while `W_MODULE_DECL_MISSING` does. |
| INV-SF-7 `parse-is-value-faithful` | N/A — no grammar change in this PRD; all 14 probe fixtures parse with 0 ERROR nodes. |

## Gate summary

- **G1** — every mechanism M1–M10 has a named consumer (PRD §1 table); re-checked, no orphan.
- **G2** — every task carries a `user_observable_signal`. ξ and ο are the true leaves under the
  strict rule; the rest are intermediates with named downstream consumers (the C-as-integration-gate
  shape, ο being the gate). No task's signal is "a unit test passes against synthetic input" —
  where a test is the vehicle (ι, κ), the signal requires a **seeded demonstration that it fires**.
- **G3** — every anchor re-verified against `main` this session; 14 grammar fixtures, 0 ERROR nodes;
  `grammar_confirmed: true` batch-wide.
- **G4** — seam table binding; no new contested-ownership pair (checked against the overlay's three).
- **G5** — B+H; the integration gate (ο, 5761) exists and names §6's 20-row sketch as its signal.
- **G6** — D3 run: 27/27 PASS, `blocks: false`. Six non-probe Adversary findings resolved into task
  text. Two PRD premises corrected (η row 9b, 5662's value-form claim) and two counts re-measured
  (5662: 4 not ≥6; guard universe: 64 not 66).
- **G7** — walked above; no hits, no waivers.
- **Drift-guard (overlay)** — ι and ο both carry their registrations **same-diff**; no registration
  is a downstream sibling and none is ordered by prose. Batch accepted on this axis.
- **metadata.files** — `scripts/lock-charter-guard.sh check` run per leaf before filing; all ACCEPT.
  ο is `[]` (unknown extent, deferred to the architect) per the tight-or-empty rule.
