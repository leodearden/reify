# Geometry LENGTH gate completion — close the silent bare-number surface, by construction

**Status:** authored 2026-07-28 (PRD 1 of 5 in the units-gating program; fan-out session, Leo's
ratified decisions of 2026-07-28).
**Milestone:** v0_6.
**Approach:** **B + H** (contract + two-way boundary tests). Blast radius ≥ 5 crates
(`reify-eval`, `reify-compiler`, `reify-stdlib`, `reify-kernel-occt`, `reify-kernel-fidget`,
plus `gui/src-tauri`); ≥ 8 mechanisms; ≥ 3 cross-PRD consumers (PRDs 3, 5, and the
`reify check` PRD 2).

---

## 0. Provenance & the program this belongs to

Canonical evidence: **`docs/notes/units-gating-gap-research-2026-07-28.md`** (tracked; 10-agent
verification session, phase-1 facts at HEAD `d2651bce16`, phase-2 at `1195020471`). This PRD
does not restate its tables — it cites them. **Every anchor below was re-verified against
`main` at `dc83d4fd60` on 2026-07-28**; §2 records where the research drifted.

The program is five PRDs closing Reify's silent bare/wrong-dimension acceptance:

| # | PRD | Owns |
|---|---|---|
| **1** | **this** | geometry LENGTH gate completion + closure guard + kernel tripwire + GUI editor |
| 2 | `check-diagnostic-truthfulness` | `reify check` stops false-greening |
| 3 | `angle-units-surface-convergence` | ANGLE bare-rejection, `Nm`, unit-display round-trip |
| 4 | `dimensioned-construction-strictness` | ctor-slot strictness (task 5627 candidate 4), D5 reinstatement |
| 5 | `dimension-checked-readers` | native readers, solver load extraction, `.ri` load-struct retyping |

**Leo's ratified decisions (2026-07-28 — not open questions):** (1) hard-REJECT bare numbers at
dimensioned positions, bare `0` included, diagnostics carrying migration hints, migrate-corpus-
first landing shape; (2) strict `DimensionVector` equality everywhere; (3) eval layer =
soundness, compile-layer `CheckableArg` slots = **first-class UX** (not "complement");
(4) kernel tripwire = `cfg(debug_assertions)` assertion naming op kind + field, **not** a hard
gate; (5) closure guard = source-text-driven behavioural probe over the compiler's own
`GEOMETRY_FUNCTION_NAMES`; (6) `Nm` torque symbol (PRD 3); (7) unit-display round-trip (PRD 3).

## 1. Goal — consumers + user-observable surface (G1)

Today `box(20, 20, 10)` builds a **20 × 20 × 10 metre** solid and both `reify check` and
`reify eval` exit **0** with zero diagnostics (re-probed 2026-07-28 against
`target/release/reify` built 20:47, newer than every units-relevant source). After this PRD:

```
$ reify eval bare.ri
error: box: width argument expects Length, got Int; pass a dimensioned length such as `5mm`
$ echo $?
1
```

| Consumer | What it consumes | Surface |
|---|---|---|
| **The `.ri` design author** | a hard rejection with a migration hint at every length-semantic geometry argument | `reify eval` exit 1 + diagnostic; `reify check` exit 1 for the statically-visible subset (compile slots) |
| **The closure guard (this PRD, task ι)** | the gates themselves are its assertion target: it proves the enumeration is closed | a gate-resident cargo test on the merge gate |
| **PRD 3 `angle-units-surface-convergence`** | the guard harness (extends allowlist + universe for ANGLE), the shared `ArgSpec`/wording template, and task α's dimensioned `revolve_full` TAU literal | their ANGLE leaves |
| **PRD 5 `dimension-checked-readers`** | the guard harness's second universe hook; the shared rejection-wording template | their reader leaves |
| **PRD 2 `check-diagnostic-truthfulness`** | this PRD's Error-severity diagnostics become check-visible once PRD 2 lands | `reify check` exit code |
| **GUI parameter editor** | input-time rejection of a bare number typed into a dimensioned cell | the GUI param panel |
| **In-flight tasks 5623 / 5658 / 5661 / 5662** | the same chokepoint pattern, already chartered; this PRD adopts them as its first leaves | as filed |

**Engine-integration sub-check** (`docs/prds/v0_3/engine-integration-norm.md` §3): every gate
lives at the existing **op-execute** seam (§3.1) — `compile_geometry_op` is the sole IR-build
funnel and already has three production callers in `engine_build.rs` (:765, :1051, :7223). No
new in-engine seam is introduced. The kernel tripwire sits at the **multi-kernel dispatch**
seam (§3.3) boundary — `reify-kernel-occt/src/lib.rs` `extract_f64` and
`reify-kernel-fidget/src/kernel.rs`.

## 2. Substrate evidence (G3) — re-verified at `dc83d4fd60`, with research drift called out

**No novel grammar.** Every probe fixture is existing syntax; all four author-session probes
reached *semantic* behaviour, never a parse error. `grammar_confirmed = true` batch-wide.

**Live behaviour probed this session** (release binary 2026-07-28 20:47 ≫ newest units source
2026-07-28 05:39, so the binary embeds current sources):

| Probe | `reify check` | `reify eval` | Meaning |
|---|---|---|---|
| `box(20, 20, 10)` | 0, "All constraints satisfied." | **0**, no diagnostic | R7 fully silent |
| `cylinder(5,20)` / `sphere(7)` / `extrude(circle(4mm),12)` | 0 | 0 | R7 breadth confirmed |
| `fillet(box(10mm,10mm,10mm), 1)` | 0 | 1, `OCCT make_fillet_with_history: unexpected: BRepFilletAPI_MakeFillet failed` | failure is an **unattributable kernel error** — no op, no field, no span. This is exactly what task θ fixes |
| `mirror(b, 10, 0, 0, 1, 0, 0)` | **0** | **1**, `warning: mirror: ox argument expects Length, got Int; pass a dimensioned length such as \`5mm\`` + `error: failed to compile geometry operation: missing or non-Length argument 'ox' for mirror` | shipped Contract C gate; **eval-visible, check-invisible** |
| `mirror(b, plane_yz(10))` | 0 | **0** | R3 value-form bypass of the same builtin's gate |
| `linear_pattern(b,1,0,0,3,20)` | **1**, `error: linear_pattern: spacing argument expects Length, got Int` | 1, same | compile slot — the ONLY layer with teeth at `check` today (red-team finding 3 confirmed) |
| `apply_transform(box, transform3(…, vec3(5,0,0)))` | 0 | 1 (`'transform' arg is not a valid Transform<3>`) | R8's translation triple |
| `apply_transform(box, transform3(…, vec3(5kg,0kg,0kg)))` | 0 | 1 (same message) | wrong-dimension rejected, but **by the Transform<3> shape check, not a units check** |
| `affine_translate(5kg, 0kg, 0kg)` | 0 | 0 — evaluates to `translation=[5, 0, 0]` | R12: the MASS dimension is **silently discarded**; the SI magnitude becomes a length |

Two consequences are load-bearing for G2/G6: **`reify eval`'s exit code is a usable leaf signal
for eval-layer gates today** (the `mirror` probe exits 1), and **`reify check` is not** — every
leaf signal in §8 is therefore phrased against `reify eval` (decision D8; red-team finding 14
decided explicitly).

**Anchors verified (drift noted):**

| Anchor | Verified location | Drift vs research |
|---|---|---|
| `cylinder_centered` dx/dy zeros | `reify-compiler/src/geometry.rs:1502-1503` (def), `:1520-1521` (use) — `CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar())` | none |
| `rounded_rect` dz | `geometry.rs:2524-2525`, same shape | none |
| `revolve_full` TAU | `geometry.rs:2063-2067`, bound at `:2079` — dimensionless `Value::Real(TAU)` into an **angle** slot | none |
| *(newly found, same shape)* `rounded_box` dz | `geometry.rs:2362` and `:2439` — `Value::Real(-0.5)` dimensionless literals | **new** — add to task α |
| `accept_arg` / `ArgSpec` / `Acceptance` / `ArgRejection` | `reify-eval/src/arg_acceptance.rs:117 / :28 / :40 / :52`; `pub(crate)` via `lib.rs:93` | none |
| `*_spec` constructors that exist | **exactly two**: `density_spec` (:86), `length_spec` (:103) | none. **No `angle_spec`** — ANGLE builds an anonymous `ArgSpec` with **no migration hint** at `geometry_ops.rs:8767-8771` (PRD 3's to fix) |
| rejection wording template | `arg_acceptance.rs:71-79` — `"{builtin}: {arg_name} argument expects {expected}, got {got}"`, `"; {hint}"` appended | none |
| `required_length_value` / `required_length_origin3` / `resolve_length_scalar_arg` / `resolve_point3_length_arg` / `resolve_bare_angle` | `geometry_ops.rs:359 / :399 / :8817 / :8478 / :880` | none (research cited no lines) |
| `point3_components` | `geometry_ops.rs:766` — **no** dimension check; its own doc says dimensionless `Vector` and LENGTH `Point` both "pass through correctly" | none |
| `decode_plane` / `decode_axis` (eval) | `geometry_ops.rs:814 / :854` — variant + `point3_components` + unit-vector magnitude only | none |
| `decompose_transform_to_arrays` | `geometry_ops.rs:10292`; the check at **:10318-10322** admits `LENGTH \|\| DIMENSIONLESS` | none |
| `decompose_xyz3` | `reify-stdlib/src/geometry.rs:19-26` — **consistency only**, any shared dimension passes | none |
| `affine_translate` / `affine_map` | `reify-stdlib/src/geometry.rs:446 / :460` — translation dimension discarded into `_dim` / `_t_dim`; `affine_map`'s **linear** part *is* checked dimensionless (:468-470) | sharper than research |
| `make_plane` / `make_axis` | `reify-stdlib/src/geometry.rs:1112 / :1152`. `make_plane`'s zero filler **mirrors** the offset's dimension (`make_zero`, :1125-1134), so a LENGTH offset already yields a LENGTH origin triple for free | sharper than research |
| locking regression test | `reify-stdlib/src/geometry.rs:2928` `plane_xy_real_zero_produces_dimensionless_origin` — asserts all three origin components are `Value::Real(0.0)` | none |
| GeometryOp `Value`-typed fields | `reify-ir/src/geometry.rs:575-1082` — **46** | none on 46. **Restated precisely:** 46 = 41 length-semantic (38 un-gated + 3 already gated: `spacing`, `spacing1`, `spacing2`) + 3 dimensionless normal components (`nx/ny/nz`, :635-637) + 2 angle (:784, :944). The research's "38" is the *un-gated* subset |
| `isosurface.iso` | `geometry_ops.rs:1367-1383`. **Two paths**: `None => 0.0` at :1368 is the *documented deliberate* absent-arg default (:1358-1363); the defect is `v.as_f64()` at :1374 with **no LENGTH check** | **corrected** — the research framed the absent-arg default as the antipattern; it is not |
| `"linear"` kind label | `reify-compiler/src/types.rs:1510` (`Linear => "linear"`); `Linear2D => "linear_2d"` at :1513; both pinned by a table test at `:1876`/`:1879` | none; the `linear_2d` twin is **new** |
| `GEOMETRY_FUNCTION_NAMES` | `reify-compiler/src/units.rs:21-86` | line range ✓; **count is 64, not 66** (the 66 was the line count; 2 lines are comments) |
| `BUILTIN_NAME_FAMILIES` | **not** in `units.rs` — `builtin_signatures.rs:425-437`, inside `#[cfg(test)] mod tests`, aggregating 11 `pub` name slices | **drift** |
| `CheckableArg` | `builtin_signatures.rs:80-88`; `expected` is an **`ExpectedArg` enum** (`:92-110`, `Scalar{dimension,type_name}` \| `Int{type_name}`), **not** a raw `DimensionVector` | **drift** |
| `builtin_arg_slots` | `builtin_signatures.rs:145-297`, keyed `(name, arity)`; consumer `check_builtin_arg_types` :325 | none |
| existing LENGTH compile slots | **7, not 2**: `edges_at_height` h+tol (:178), `extremal_by_bbox`/`extremal_by_centroid` tol (:207), `linear_pattern` spacing @arity 6 (:250), `linear_pattern_2d` spacing1+spacing2 @arity 11 (:277) | **drift** — research said "only linear_pattern spacing" |
| `arg_slot_keys_are_registered_builtin_names` | `builtin_signatures.rs:727`; universe = `BUILTIN_NAME_FAMILIES` ∪ non-family keys ∪ deliberate typos; arity `0..=MAX_PROBED_ARITY` where `MAX_PROBED_ARITY = 14` (:420) | none |
| `version_id_discipline_gate.rs` | `reify-eval/tests/version_id_discipline_gate.rs` — 8 tests: **7 seeded self-tests (a)-(g)** at :272/:303/:337/:357/:378/:408/:444 + 1 real-tree scan at :492 | none |
| `corpus_no_bare_scalar.rs` | **moved** → `reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs`, wired from `harness_cli.rs:166-167` | **drift (path)** |
| `compile_geometry_op_characterization.rs` | `reify-eval/tests/…` (1992 ln); builds `CompiledGeometryOp` struct literals from `CompiledExpr::literal`, empty `ValueMap`/`named_steps`, **no parser, no kernel**; driver `run(op, step_handles)` :200 | none |
| the cross-crate probe seam | `reify-eval/src/geometry_op_characterization_probe.rs:39` `pub fn compile_geometry_op_probe` — `#[cfg(any(test, feature = "test-instrumentation"))]`, 1:1 delegate. **This is what makes task ι buildable** | none |
| `compile_geometry_op` | `geometry_ops.rs:937-945`; **4** non-test callers (3 × `engine_build.rs`, 1 × the probe seam) | **drift** — "engine_build.rs only" is false |
| `VARIANT_COUNT` backstops | exactly ONE in the workspace: `ModifyKind::VARIANT_COUNT`, `reify-compiler/src/types.rs:1448`; asserted at `geometry_ops/tests.rs:20435` | **drift** — **8 of 9** kind families lack it (`PrimitiveKind`, `BooleanOp`, `TransformKind`, `PatternKind`, `SweepKind`, `CurveKind`, `ProfileKind`, `SurfaceKind`), not 5 of 6 |
| GUI `UNIT_TABLE` | `gui/src-tauri/src/engine.rs:5918-5924` — exactly 5 entries: `deg`, `rad`, `mm`, `cm`, `m` | none |
| GUI `parse_value_string` | `engine.rs:5932-5974`. `"10"` → `Value::Int(10)`; `"10.0"` → `Value::Real`; `"10mm"` → `Value::Scalar{LENGTH}`. Caller `engine.rs:2057`; a **second** independent `UNIT_TABLE` lookup at `engine.rs:4870` | none |
| DSL unit registry | `reify-core/src/units.rs:24-51` `unit_symbol_to_si` — 13 symbols. GUI **lacks 8**: `in`, `kg`, `g`, `s`, `K`, `A`, `mol`, `cd` — and cannot see per-module user-declared units at all | sharper |
| spec mandate | `docs/reify-language-spec.md:125` — *"There is no \"default unit system.\""*; compat disclaimed :2575; breaking-change obligation §14.5 :2597-2601 | none |
| landing-shape precedent | `docs/prds/v0_6/real-dimensionless-unification.md:64` — migrate corpus first so "the workspace never has a window where the hard error has live violators" | none |

**Measured blast radius (re-measured this session; supersedes red-team finding 4):**
- **`.ri` corpus: ZERO.** 258 `examples/**/*.ri` plus `crates/**`, `gui/**`, `prj/**` — 3 regex
  hits, all false positives (2 comments, 1 `@shell(...)` *annotation*). Independently confirmed
  twice.
- **Eval-side Rust fixtures: the research's ~24 does NOT reproduce.** `topology_selectors.rs`
  and both `shell_solve.rs` files contain **no DSL fixtures at all** (pure Rust over kernel-level
  SI-metre `f64` APIs) → 0 each. `unified_dag_geometry_executors.rs` yields **2**
  (`:1176` `translate(b, 1, 0, 0)`, `:1177` `rotate(a, 0, 0, 1, 90)`), and both are in one
  cycle-detection fixture. A first-arg-only regex over the whole workspace finds ~60 `.rs`
  sites; the true all-positions count is larger and **must be re-measured per leaf** — no leaf
  signal in §8 asserts a total.
- **Compile-side (LENGTH slots): 29, not 31.** `let_scope_tests.rs` **15** code sites (:745, 882,
  944, 961, 999, 1018, 1041, 1065, 1094, 1206, 1229, 1297, 1480, 1734, 1791 — 20 regex hits
  minus 5 in comments); `compile_api_tests.rs` **13** (:220, 544, 578, 1848×2, 1892×3, 1957×2,
  2004×3); `fn_arg_trait_conformance_tests.rs:579` **1** (path drift: now
  `tests/harness_traits/fn_arg_trait_conformance_tests.rs`).
- **TWO inverting assertions, not one.** `translate_non_geometry_target_uses_fallback`
  (`let_scope_tests.rs:1729`, fixture `translate(42, 1, 0, 0)`, asserts `errors.is_empty()`) and
  — newly found — `extrude_non_geometry_target_uses_fallback` (`:1783`, `extrude(42, 10)`, arg 1
  is a LENGTH distance). Both invert under their respective compile slots.
- **An existing compile-slot false negative:** `compile_api_tests.rs:220` calls
  `linear_pattern_2d(w, 1, 0, 0, 3, 20)` at **arity 6**, so today's `arg_count == 11` guard
  misses it entirely. Task η fixes the arity coverage.

## 3. Resolved design decisions

- **D1 — Hard REJECT; no warn-and-convert; bare `0` included.** (Ratified 1.) A default unit
  would silently *reinterpret* existing metre-intent calls — a behaviour change with no error.
  Spec-mandated (`reify-language-spec.md:125`); breaking-change obligation (§14.5) discharged by
  the migration hint already carried in `length_spec`.
- **D2 — Two layers, both first-class.** (Ratified 3.) The **eval-layer chokepoint is the
  soundness layer** (it sees dynamically-typed and value-form arguments); the **compile-layer
  `CheckableArg` slots are the UX layer** and are *first-class deliverables*, not a complement —
  because they are the only layer with teeth at `reify check` today (§2 probe table). Gradual
  types (`plane_yz(...)` etc. type as `Type::Error`/`TypeParam`) still fall through to eval, by
  design; task 5662 records why.
- **D3 — R7 gating point: option A (eval-layer gate at Value insertion) + option C as a
  tripwire.** Typed-IR-by-construction (option B — a dimensioned newtype over the 41
  length-semantic `Value` fields) is recorded as the **endgame**, explicitly NOT built here: it
  touches 46 IR fields, both kernels and a 20k-line test module, and is best folded into the
  `Real → Scalar{DIMENSIONLESS}` unification. Breadcrumb the rejected variant at the chokepoint
  impl site (`feedback_breadcrumb_design_alternatives_at_impl_site`).
- **D4 — Gate the value-form hole at BOTH ends.** Consumers (`point3_components`' length-position
  callers, `decode_plane`/`decode_axis` origins, NurbsSurface control points) get the gate — that
  covers every producer including user-built `point3`. Producers (`make_plane`/`make_axis`) are
  *also* fixed to require a LENGTH offset, and the locking regression test
  `plane_xy_real_zero_produces_dimensionless_origin` is **flipped** — it pins an accidental gap
  (git archaeology: predates the units doctrine by 4 months), not a design. `make_plane`'s
  dimension-mirroring `make_zero` means the origin triple then becomes LENGTH for free.
- **D5 — Kernel layer is a TRIPWIRE, not a gate.** (Ratified 4.) A hard kernel gate would break
  hundreds of legitimate kernel-side tests that feed `Value::Real` op inputs (occt 546, ir 242,
  fidget 40) and kernel errors carry no span or argument name. Instead: a
  `cfg(debug_assertions)` assertion naming **op kind + field**, plus release-mode kernel
  diagnostics naming op + field (today the failure is the span-less
  `BRepFilletAPI_MakeFillet failed` of §2). Second, independent detection layer behind the
  closure guard.
- **D6 — Closure by mechanical guard, not prose.** (Ratified 5.) Contract C's completeness claim
  is prose in a doc comment today, asserted by **nothing** — no test, no `reify-audit` pattern,
  no allowlist, and invisible to the PTODO detector. Five successive hand audits missed R7
  precisely because it leaves no `as_f64` fingerprint in `reify-eval`. The guard is a
  **source-text-driven behavioural probe** whose universe is the compiler's own
  `GEOMETRY_FUNCTION_NAMES` (64 names) — never a hand list (the
  `arg_slot_keys_are_registered_builtin_names` vacuity lesson: the probe universe must be
  independent of the assertion target). Contract C's module doc shrinks to a pointer at the guard.
- **D7 — Migrate-first landing shape, per leaf.** (`real-dimensionless-unification.md:64`
  precedent.) There is **no** standalone migration task: each gating leaf migrates, **in its own
  diff**, exactly the call sites its own gate newly rejects. The workspace is green at every
  commit; there is never a window where the hard error has live violators. Deliberate negative
  fixtures stay bare and become tests *of* the new gates (updating expected messages where the
  wording changes).
- **D8 — Every leaf signal is phrased against `reify eval`.** (Red-team finding 14, decided.)
  Probed: eval-layer gates are observable via `reify eval` exit 1 today; `reify check` exits 0 on
  the same input. Leaves whose mechanism is a **compile slot** (η, and task 5662) may
  additionally note `reify check` exit 1 — that already works — but no leaf *depends* on PRD 2.
  Any signal that would need PRD 2 takes a real `add_dependency` edge onto PRD 2's task instead.
- **D9 — One rejection-wording template, one diagnostic code.** All rejections route through
  `ArgRejection::message` (`arg_acceptance.rs:71-79`), so wording stays byte-identical across
  PRDs 1/3/5. Two obligations follow: (i) every `ArgSpec` constructor **must** carry a
  `migration_hint` (the spec §14.5 breaking-change duty; the ANGLE inline spec at
  `geometry_ops.rs:8767-8771` violates this today — PRD 3's to fix); (ii) **INV-SF-6**: every
  diagnostic this PRD emits carries a `DiagnosticCode` — the existing Contract C rejections carry
  none (probe: `warning: mirror: ox argument expects Length, got Int` has no code, unlike
  `W_MODULE_DECL_MISSING`). Task β introduces the code and retrofits the existing sites; every
  later leaf and PRD 3/5 reuse it. Also in scope for D9: the **compile-slot** message
  (`error: linear_pattern: spacing argument expects Length, got Int`) currently lacks the
  migration hint the eval-layer message carries — task η reconciles them onto the same template.
- **D10 — `Undef` degrades quietly at `accept_arg`, loudly at the chokepoint.**
  `Acceptance::Undefined` stays a quiet three-state variant (it is the *value-layer* answer), but
  every new length chokepoint routes `Undefined` into the existing distinct diagnostic
  `"argument '{name}' for {kind} is unresolved (Undef)"` (`geometry_ops.rs:344-347`) rather than
  silently continuing. This keeps INV-SF-1 (`undef-has-provenance`) satisfied at the new seams
  and resolves the research's open tension "is quiet degrade right for R7?" — no.
- **D11 — R8 stops admitting `DIMENSIONLESS`.** `decompose_transform_to_arrays`
  (`geometry_ops.rs:10318-10322`) currently accepts `LENGTH || DIMENSIONLESS` for translation
  components. Under ratified decision 2 (strict `DimensionVector` equality) that admission is
  dropped: translation components require LENGTH; the **linear/rotation** part stays
  dimensionless-required (`affine_map` already enforces that at `reify-stdlib/src/geometry.rs:468-470`).
  This is *forward*-compatible with `Real → Scalar{DIMENSIONLESS}` unification: once `Real`
  becomes `Scalar{DIMENSIONLESS}`, "admits DIMENSIONLESS" would mean "admits bare" — i.e. the
  hole re-opens. Closing it now is a prerequisite for that unification, not a conflict with it.
- **D12 — `isosurface.iso`: dimension-check the value, keep the absent-arg default.** The
  `None => 0.0` branch (`geometry_ops.rs:1368`) is deliberate and documented ("absence is the
  normal, expected shape"); it stays. The defect is the un-checked `v.as_f64()` at :1374 — that
  gets the LENGTH gate. (Honest correction of the research's "silent-default antipattern"
  framing.)
- **D13 — No leaf signal is built on `arc` sweep magnitudes.** The research flagged, unconfirmed,
  that `arc` sweep may not consume its radius. Task 5658/5661 territory triages it per-arg; no
  §8 signal depends on an arc magnitude.
- **D14 — Allowlist entries are individually justified, and residuals cite a live task.**
  Every dimensionless position in the guard's allowlist carries a one-line justification
  (unit-vector component / count / weight / knot / degree / factor / index) and its expected
  `DimensionVector`. Any position deliberately left un-gated is a residual and **must** cite a
  live non-terminal task per the PTODO grammar (INV-SF-5 `placeholders-owned-and-loud`); a
  blanket "awaiting a future units PRD" escape is banned.

## 4. Sketch of approach — mechanisms

- **M1 — Dimensioned synthesized literals (reify-compiler).** The compiler's own desugarings
  emit `CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar())` into slots this
  PRD is about to gate. Retype them to LENGTH (`cylinder_centered` dx/dy, `rounded_rect` dz,
  `rounded_box` dz ×2) and ANGLE (`revolve_full` TAU). **Prerequisite to every gate.**
- **M2 — R7 eval chokepoint.** A raw-`Value` counterpart to `required_length_arg` that runs
  `accept_arg(value, &length_spec())` before the `Value` is placed into a `GeometryOp` field,
  applied to the 38 un-gated length-semantic slots. Additive helper only —
  `arg_acceptance.rs`'s core is FROZEN per the program seam table.
- **M3 — Value-form/decoded-route gates.** `point3_components`' length-position callers,
  `decode_plane`/`decode_axis` origins, NurbsSurface control points (R3); `make_plane`/`make_axis`
  producers + flipped locking test (R11); `decompose_transform_to_arrays` (R8);
  `decompose_xyz3`-fed `affine_translate`/`affine_map` translation (R12).
- **M4 — LENGTH `CheckableArg` slots (reify-compiler `builtin_signatures.rs`).** Arity-keyed per
  the 5652 pattern, using the existing `ExpectedArg::Scalar { dimension: LENGTH, .. }`; extend
  `NON_SELECTOR_ARG_SLOT_KEYS` for each new key; fix the `linear_pattern_2d` arity-6 gap; carry
  the migration hint (D9). Origin triples arrive via rescoped task 5662.
- **M5 — Kernel tripwire (both kernels).** `cfg(debug_assertions)` assertion naming op kind +
  field at `reify-kernel-occt/src/lib.rs` `extract_f64` and `reify-kernel-fidget/src/kernel.rs`;
  release-mode diagnostics naming op + field.
- **M6 — Closure guard.** For each of the 64 `GEOMETRY_FUNCTION_NAMES` × probed arity `0..=14`,
  synthesize `structure S { let x = <name>(<bare args>) }`, compile it, and feed each resulting
  `CompiledGeometryOp` through `compile_geometry_op_probe`, asserting **either** a Contract C
  rejection **or** an allowlist entry keyed by expected `DimensionVector` per position. Seeded
  anti-vacuity self-tests mirroring `version_id_discipline_gate.rs`'s seven.
- **M7 — Registry backstops.** `VARIANT_COUNT` for the 8 kind families that lack it, mirroring
  `ModifyKind::VARIANT_COUNT` (`types.rs:1448`) and its assertion (`geometry_ops/tests.rs:20435`),
  so a new enum variant cannot silently dodge the registry-completeness test.
- **M8 — Diagnostic-label + iso fixes.** `PatternKind::Linear` → `"linear_pattern"`,
  `Linear2D` → `"linear_pattern_2d"` (+ the pinning table test at `types.rs:1876`/`:1879`);
  `isosurface.iso` LENGTH check (D12).
- **M9 — GUI parameter editor.** `parse_value_string` rejects a bare number typed into a cell
  whose parameter is dimensioned (today it silently yields `Value::Int`/`Value::Real`);
  `UNIT_TABLE` reconciles with `reify-core::units::unit_symbol_to_si` — ideally by *delegating*
  to it rather than duplicating (INV-SF-6 sibling: `no-lockstep-duplication`), covering the two
  lookup sites (`engine.rs:5932`, `:4870`).
- **M10 — Docs-truth.** Doc-chunk updates (`units.md`, `geometry.md`) verified against the
  compiler registries; a best-practices exemplar + `INDEX.md` row + `reify-design` cheatsheet
  index line + discoverability acceptance. `examples/best_practices/symmetry_mirror.ri` already
  carries the sentence *"the 7-arg scalar form needs a dimensioned origin, and `reify check` will
  not tell you when it doesn't"* — it must be updated when the gate lands.

## 5. Contract (B+H)

**C1 — Chokepoint API (additive; `arg_acceptance.rs` core FROZEN).**
Every length-semantic argument, on every route, resolves through exactly one call of
`accept_arg(value, &length_spec())`. Callers map the three states:

| `Acceptance` | Caller obligation |
|---|---|
| `Accepted(si)` | proceed with the SI f64 / re-wrapped `Value::length(si)` |
| `Undefined` | emit the existing distinct *unresolved (Undef)* diagnostic and fail the op (D10) — never silently continue |
| `Rejected(r)` | emit `r.message(builtin, arg_name)` **with a `DiagnosticCode`** (D9) and fail the op |

Invariants: (i) wording is produced only by `ArgRejection::message` — no hand-rolled rejection
strings; (ii) every `ArgSpec` constructor carries a `migration_hint`; (iii) new spec constructors
are additive functions beside `length_spec`/`density_spec` — `accept_arg`/`ArgSpec`/`Acceptance`
semantics are not modified; (iv) the eval-layer rejection is Error severity, so `reify eval`
exits nonzero (INV-SF-2) without any per-code escalation list.

**C2 — Gate placement.** A length-semantic value is gated **before** it is stored into a
`GeometryOp` field, not at the kernel. `compile_geometry_op` is the sole funnel: all four callers
(three in `engine_build.rs`, one probe seam) therefore inherit the gate. Corollary: a route that
constructs a `GeometryOp` outside `compile_geometry_op` is out of contract — none exists on main
(no serde path; the latent `handle.rs:887` construction is not live), and the tripwire (C4) is
the runtime detector if one appears.

**C3 — Compile-slot contract.** For every statically-visible length position: a
`CheckableArg { index, name, expected: ExpectedArg::Scalar { dimension: LENGTH, type_name } }`
guarded by the arity that makes the position semantically honest; the key is registered in
`NON_SELECTOR_ARG_SLOT_KEYS`; the emitted message uses the C1 template **including the migration
hint**. A compile slot **never replaces** the eval gate for the same position — gradual/dynamic
types are statically invisible.

**C4 — Kernel-tripwire contract.** At each kernel's numeric-extraction boundary: under
`cfg(debug_assertions)`, receiving a non-LENGTH value for a length-semantic field **asserts**,
and the panic message names the op kind and the field. Under release, the same condition emits a
kernel diagnostic naming op kind and field (never a bare `expected numeric value`). The tripwire
**never** changes accept/reject behaviour in release builds — it is a detector, not a gate.

**C5 — Closure-guard contract.**
- *Universe:* `reify_compiler::units::GEOMETRY_FUNCTION_NAMES` (64) read at test time — never a
  literal list in the test. Arity range `0..=14` (matching `MAX_PROBED_ARITY`).
- *Probe:* compile `structure S { let x = <name>(<bare numeric args>) }`; for ops needing a
  geometry target, `<name>(box(1mm,1mm,1mm), <bare args>)`; feed each `CompiledGeometryOp` from
  the compiled realizations through `compile_geometry_op_probe` with empty `ValueMap`/`functions`/
  `meta_map`/`named_steps` and a synthetic `step_handles` vec (the
  `compile_geometry_op_characterization.rs` `run()` precedent, :200). **IR-build only — no
  kernel, fast.**
- *Assertion:* for every numeric position reached, the outcome is **either** a Contract C
  rejection **or** an allowlist entry `(builtin, arity, index) → expected DimensionVector +
  justification` (D14).
- *Anti-vacuity (seeded self-tests, `version_id_discipline_gate.rs`'s 7-test shape):* a
  shrunken allowlist makes the guard fire; a known-gated position fires when its gate is stubbed
  out in the seed; an escape-hatch suppression is observed to suppress; a comment-only mention
  does not match; the probe universe is non-empty and its size is asserted `>= 60` (a floor, not
  an equality — new builtins must not break the guard).
- *Extension points (additive only, never a harness rewrite):* PRD 3 adds ANGLE-dimension
  allowlist entries; PRD 5 adds a **second universe** (`plane_*`/`axis_*`/`point3`, `prb_*`,
  joints/solver builtins).

**C6 — Migration contract (D7).** A leaf that adds a gate lands, in the same diff, every call-site
migration its gate newly rejects, and leaves deliberate negative fixtures bare. No leaf asserts a
workspace-wide count; each re-measures its own footprint. The two inverting assertions
(`let_scope_tests.rs:1729`, `:1783`) are **flipped deliberately** by the leaf whose slot inverts
them, with a comment naming this PRD — flipping them is the fix, not a regression to suppress.

**C7 — Drift-guard contract.** Any gate-resident test this PRD adds (the closure guard, the
boundary suite) carries its drift-guard registrations **in the same diff**: nextest heavy/smoke
partition entries in `.config/nextest.toml` where applicable, a bucket row in
`tests/infra/run-all-classification.manifest` for any new `tests/infra/test_*.sh`, and the
wallclock-bounds registration for any elapsed-time assertion. (esc-4914-162 is the failure this
prevents.) No new wall-clock upper bound should be introduced at all.

## 6. Boundary-test sketch (two-way; task ο's observable signal)

Facing the **producer** side (the gates) and the **consumer** side (the guard, the tripwire, the
GUI, PRDs 3/5).

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| 1 | `box(20, 20, 10)` | β | `reify eval` exits 1; diagnostic `box: width argument expects Length, got Int; pass a dimensioned length such as \`5mm\``, carrying a `DiagnosticCode`. **Negative assertion — observed to fire** |
| 2 | `box(20mm, 20mm, 10mm)` (control) | β | exits 0; realization volume unchanged vs the pre-gate baseline |
| 3 | `box(0, 0, 0)` — bare zero | β | rejected (D1: bare `0` is not special-cased) |
| 4 | `fillet(box(10mm,10mm,10mm), 1)` | γ | exits 1 with the **units** diagnostic naming `radius`, replacing today's span-less `BRepFilletAPI_MakeFillet failed` |
| 5 | `mirror(b, plane_yz(10))` — value form | δ, ε | exits 1 naming the plane origin; the scalar form `mirror(b, 10, 0, ...)` still exits 1 with its existing wording (no regression) |
| 6 | `mirror(b, plane_yz(10mm))` (control) | δ, ε | exits 0; result geometrically identical to today's `plane_yz(0.01)` |
| 7 | `apply_transform(box, transform3(…, vec3(5,0,0)))` and the `Scalar{DIMENSIONLESS}` form | ζ | both exit 1 with a **units** diagnostic (D11), not the generic `not a valid Transform<3>` |
| 8 | `affine_translate(5kg, 0kg, 0kg)` | ζ | exits 1 — the MASS dimension is no longer silently discarded (§2 probe) |
| 9 | a bare primitive dimension, e.g. `box(20, 20, 10)` | η | `reify check` **and** `reify eval` exit 1 — the compile layer is the only one `check` sees today (§2 probe table) |
| 9b | `linear_pattern_2d(w, 1, 0, 0, 3, 20)` (`compile_api_tests.rs:220`, arity 6) | η | resolved either way: if arity 6 is a legitimate overload it gains a slot and exits 1; if it is a malformed fixture it is corrected. **Not left silently un-slotted** |
| 10 | closure guard over all 64 builtins × arity 0..=14 | ι | green; every reached numeric position is rejected or allowlisted-with-justification |
| 11 | guard anti-vacuity: shrink the allowlist by one entry | ι | the guard **fails** (seeded self-test) |
| 12 | guard anti-vacuity: stub out one shipped gate | ι | the guard **fails** naming that position |
| 13 | debug-build kernel receives a bare length for a gated field (test-only injection) | θ | assertion fires naming op kind **and** field name |
| 14 | release-build same injection | θ | kernel diagnostic names op kind and field; behaviour otherwise unchanged |
| 15 | add a new `SweepKind` variant without registering it | κ | the registry-completeness test fails via `VARIANT_COUNT` |
| 16 | GUI: type `20` into a dimensioned param cell | μ | rejected at input time with a message naming the expected dimension; `20mm` accepted |
| 17 | GUI: type `3in` into a LENGTH cell | μ | accepted (`in` is in the DSL registry; the GUI table lacked it) |
| 18 | `linear_pattern` diagnostic text | λ | names `linear_pattern`, not `linear` (and `linear_pattern_2d`, not `linear_2d`) |
| 19 | `isosurface(grid, iso: 5)` vs `isosurface(grid)` | λ | the first exits 1 with a LENGTH rejection; the second still defaults to 0.0 with no diagnostic (D12) |
| 20 | full `examples/**/*.ri` corpus + `examples/best_practices/` | all | `reify check` and the corpus eval gates stay green throughout — zero migrations were needed (§2) |

## 7. Cross-PRD relationship (G4)

Ownership is **binding** per the program's seam table; this PRD files no work owned elsewhere.

**Sibling-PRD paths below are the program's reserved slugs.** PRDs 2–5 are being authored
concurrently in the same 2026-07-28 fan-out and were not yet on disk when this file was written
— a missing sibling path is *expected*, not drift. At decompose time, resolve each named
cross-PRD edge to the sibling's real task ids and wire a **real `add_dependency` edge**
(`preferences_cross_prd_deps_real_edges`); if a sibling PRD is still absent, record the edge as
pending rather than inventing a task.

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_6/check-diagnostic-truthfulness.md` (PRD 2) | produces→them | `cmd_check`/`finish_check`, build-diagnostic collection, exit codes, `--strict`, the `let _ = engine.build(...)` discard at `reify-cli/src/main.rs:621` | **PRD 2 only** | this PRD's diagnostics become check-visible when they land; **no leaf here depends on it** (D8) |
| `docs/prds/v0_6/angle-units-surface-convergence.md` (PRD 3) | produces→them | closure-guard allowlist/universe extension; `angle_spec` + its missing migration hint (`geometry_ops.rs:8767-8771`); ALL angle gating incl. `draft.angle` and `circular_pattern`'s `resolve_bare_angle` retirement; `Nm`; middle-dot/label round-trip | **PRD 3** | this PRD gates **no** angle position. Task α's `revolve_full` TAU → ANGLE literal is the one angle-adjacent item, owned **here** by seam-table decree (single leaf, all three desugarings), consumed there |
| `docs/prds/v0_6/dimensioned-construction-strictness.md` (PRD 4) | adjacent | ctor conformance, `type_compat.rs`, `conformance/mod.rs`, `entity.rs` defaults gates, task **5627**'s ruling, D5 reinstatement record, trajectory placeholder migration (`tots_optimal_ptp.ri`, `printer_print_envelope.ri`) | **PRD 4 only** | this PRD touches none of it |
| `docs/prds/v0_6/dimension-checked-readers.md` (PRD 5) | produces→them | guard harness **second universe**; shared wording template; stdlib/native readers, solver load extraction, `.ri` load-struct retyping | **PRD 5** | additive guard entries only, never a harness rewrite |
| `docs/prds/v0_6/real-dimensionless-unification.md` | constrains | `Real → Scalar{DIMENSIONLESS}`; D11 removes R8's `DIMENSIONLESS` admission so that unification does not re-open the hole | theirs (the unification), ours (the admission removal) | forward-compatible by construction |
| `docs/prds/v0_6/eradicate-silent-undef.md` | adjacent | INV-SF-2 error-severity exit gate; INV-SF-1 undef provenance at the new seams (D10) | theirs (the general rule), ours (compliance at our seams) | no per-code escalation added here |
| **task 5623** (pending) | consumes-as-leaf | R1 `eval_named_arg_f64` sweep — 22 length positions | **this PRD** | first leaf; decompose wires deps and cites the PRD in its description. Do **not** refile |
| **task 5658** (pending, dep 5623) | consumes-as-leaf | R2 variadic curve coords (`eval_all_args_to_f64`) | **this PRD** | ditto. Its charter already says to **sharpen** (not drop) the Contract C residual note; task ι later shrinks that note to a pointer at the guard |
| **task 5661** (pending, dep 5658) | consumes-as-leaf | R2 `profile_polygon` 2D pairs | **this PRD** | ditto |
| **task 5662** (pending) | consumes-as-leaf, **rescope** | compile-layer LENGTH origin slots for `mirror`/`circular_pattern` | **this PRD** | its stated premise — that 5214's eval gate covers the value forms — is **FALSE** (§2 probe: `mirror(b, plane_yz(10))` exits 0). Its "≥6 breaking call sites" is ~3. Rescope at decompose; the value-form hole is task δ's, not 5662's |
| **task 5627** | referenced | dimensioned-`Scalar` ctor-slot ruling | **PRD 4** | never cancel; PRD 4 binds it |

No new contested-ownership pair is introduced (checked against the overlay's three known pairs:
persistent-naming ↔ multi-kernel, imported-field-source ↔ multi-kernel, topology-selectors ↔
persistent-naming — none touched).

## 8. Decomposition plan (G2 signals inline; Greek labels → task ids at decompose)

`metadata.files` is tight-or-empty per the overlay. Every signal below is phrased against
`reify eval` (D8). Every gating leaf carries its own migration same-diff (D7/C6).

**Adopted in-flight leaves** — do not refile; decompose wires deps and minimally updates
descriptions to cite this PRD (read back after `update_task`):
- **5623** — R1 `eval_named_arg_f64` sweep (22 length positions). Dep: **α**.
- **5658** — R2 variadic curve coordinates. Dep: 5623 (already wired).
- **5661** — R2 `profile_polygon`. Dep: 5658 (already wired).
- **5662** — compile-layer origin triples, **rescoped**: correct the false premise, correct the
  call-site count (~3), and record that the value-form hole belongs to δ. Dep: **η**.

New leaves:

- **α — Dimension the compiler's synthesized geometry literals.** `reify-compiler/src/geometry.rs`:
  `cylinder_centered` dx/dy (:1502-1521) and `rounded_rect` dz (:2525) and `rounded_box` dz
  (:2362, :2439) → LENGTH; `revolve_full` TAU (:2063-2079) → ANGLE. **INTERMEDIATE — unlocks
  β, γ, δ, ζ, ι and 5623.** Downstream prerequisite it unlocks: every eval gate (without it,
  `cylinder_centered(5mm, 20mm)` starts failing the moment translate dx/dy is gated). Its own
  pin: a compile-IR characterization assertion that those slots' `CompiledExpr` result types are
  LENGTH/ANGLE, driven from real source text. **MUST land before any gate.**
- **β — R7 chokepoint: primitives + profiles.** New additive raw-`Value` length chokepoint (C1)
  + apply to box/box_centered/cylinder/sphere/tube/cone/wedge/torus dims, `half_space` px/py/pz,
  rectangle/circle/ellipse dims. Introduces the `DiagnosticCode` (D9) and retrofits the existing
  Contract C sites. **LEAF.** Signal: `reify eval` on `box(20, 20, 10)` exits 1 with the C1
  message + code (boundary rows 1–3); `box(20mm,20mm,10mm)` unchanged. Dep: α.
- **γ — R7 chokepoint: modify + sweep.** fillet radius, chamfer d, chamfer_asymmetric d1/d2,
  shell thickness, thicken offset, offset_solid/offset_curve distance, extrude/extrude_symmetric
  distance, pipe radius, zone_slab width. **LEAF.** Signal: `fillet(box(10mm,10mm,10mm), 1)`
  exits 1 with a units diagnostic naming `radius` instead of the span-less OCCT failure
  (boundary row 4). Dep: β.
- **δ — R3: gate the decoded value-form routes.** `point3_components`' length-position callers,
  `decode_plane`/`decode_axis` origins, NurbsSurface control points. **LEAF.** Signal:
  `mirror(b, plane_yz(10))` exits 1 naming the plane origin; `plane_yz(10mm)` unchanged
  (boundary rows 5–6). Dep: β.
- **ε — R11: `make_plane`/`make_axis` require a LENGTH offset; flip the locking test.**
  `reify-stdlib/src/geometry.rs:1112`/`:1152`; flip `plane_xy_real_zero_produces_dimensionless_origin`
  (:2928) from "asserts the dimensionless origin" to "expects the LENGTH rejection". **LEAF.**
  Signal: `plane_xy(0.0)` exits 1; `plane_xy(0mm)` exits 0 and yields a LENGTH origin triple
  (via `make_zero`'s dimension mirroring). Dep: δ. *Flipping that test is the fix, not a
  regression — it pins an accidental pre-doctrine gap (D4).*
- **ζ — R8 + R12: transform routes.** `decompose_transform_to_arrays` drops the `DIMENSIONLESS`
  admission (`geometry_ops.rs:10318-10322`, D11); `affine_translate`/`affine_map` translation
  require LENGTH via `decompose_xyz3` (`reify-stdlib/src/geometry.rs:19`, `:446`, `:460`).
  **LEAF.** Signal: `apply_transform(box, transform3(…, vec3(5,0,0)))` and
  `affine_translate(5kg,0kg,0kg)` both exit 1 with a **units** diagnostic (boundary rows 7–8).
  Dep: β.
- **η — LENGTH `CheckableArg` slots for statically-visible length positions.**
  `reify-compiler/src/builtin_signatures.rs`: arity-keyed slots for primitive/modify/sweep/profile
  length args; register keys in `NON_SELECTOR_ARG_SLOT_KEYS`; resolve the `linear_pattern_2d`
  arity-6 gap (`compile_api_tests.rs:220` slips past today's `arg_count == 11` guard — either a
  missing overload slot or a malformed fixture; decide by reading the compiler arm, do not leave
  it silently un-slotted); reconcile the compile-slot message onto the C1 template **with** the
  migration hint (D9). Migrates the 29 compile-side sites and **deliberately flips the two
  inverting assertions** (`let_scope_tests.rs:1729`, `:1783`). **LEAF.** Signal: `reify eval`
  exits 1 for a bare primitive dimension **and** `reify check` exits 1 for the same file (the
  compile layer is the only one check sees today) — boundary rows 9, 9b. Dep: γ.
- **θ — Kernel tripwire at both kernels.** `cfg(debug_assertions)` assertion naming op kind +
  field at `reify-kernel-occt/src/lib.rs` `extract_f64` and `reify-kernel-fidget/src/kernel.rs`;
  release-mode diagnostics naming op + field. **LEAF.** Signal: a test-only injection of a bare
  length for a gated field panics in debug with a message naming op kind and field, and emits a
  named kernel diagnostic in release (boundary rows 13–14); release accept/reject behaviour is
  otherwise byte-identical. Dep: γ. *Not a hard gate (D5) — the 546/242/40 legitimate
  `Value::Real` kernel-test inputs stay green.*
- **ι — Closure guard + Contract C doc shrink.** Build the C5 harness in
  `crates/reify-eval/tests/`, driven by `compile_geometry_op_probe`; dimension-keyed allowlist
  with per-entry justification (D14); seeded anti-vacuity self-tests; **drift-guard
  registrations same-diff (C7)**. Shrink `arg_acceptance.rs`'s Contract C module doc to a
  pointer at the guard (coordinating with 5658's sharpen-note instruction — whichever lands
  later owns the final text). **LEAF.** Signal: the guard is green on the merge gate over all 64
  builtins × arity 0..=14, and its seeded self-tests demonstrate it fires (boundary rows 10–12).
  Deps: γ, δ, ε, ζ, η, 5623, 5658, 5661.
- **κ — `VARIANT_COUNT` backstops for the 8 kind families that lack one.** `PrimitiveKind`,
  `BooleanOp`, `TransformKind`, `PatternKind`, `SweepKind`, `CurveKind`, `ProfileKind`,
  `SurfaceKind` in `reify-compiler/src/types.rs`, mirroring `ModifyKind::VARIANT_COUNT` (:1448)
  and its assertion (`geometry_ops/tests.rs:20435`). **LEAF.** Signal: adding an unregistered
  variant to any of the 8 makes the registry-completeness test fail (boundary row 15) — a seeded
  self-test demonstrates it for at least one family. Dep: none (parallel-safe; but see the
  file-lock note — it owns `reify-compiler/src/types.rs` ahead of λ).
- **λ — Diagnostic-label + `isosurface.iso` fixes.** `PatternKind::Linear` → `"linear_pattern"`,
  `Linear2D` → `"linear_pattern_2d"` (`types.rs:1510`/`:1513` + the pinning table test at
  `:1876`/`:1879`); LENGTH check on `isosurface.iso`'s value path (`geometry_ops.rs:1374`),
  keeping the documented absent-arg default (D12). **LEAF.** Signal: the `linear_pattern`
  rejection names `linear_pattern`; `isosurface(grid, iso: 5)` exits 1 while `isosurface(grid)`
  still exits 0 silently (boundary rows 18–19). Deps: β, **κ** (both own
  `reify-compiler/src/types.rs`).
- **μ — GUI parameter editor.** `gui/src-tauri/src/engine.rs`: `parse_value_string` (:5932)
  rejects a bare number destined for a dimensioned parameter; `UNIT_TABLE` (:5918) reconciles
  with `reify-core::units::unit_symbol_to_si` (:24-51) — preferably by delegating rather than
  duplicating — covering both lookup sites (:5932, :4870). **LEAF.** Signal: typing `20` into a
  dimensioned cell is rejected at input time with a message naming the expected dimension;
  `20mm` and `3in` are both accepted (boundary rows 16–17), observed via the GUI vitest/tauri
  test path. Dep: β. *`in`/`kg`/`g`/`s`/`K`/`A`/`mol`/`cd` are the 8 symbols the GUI table lacks.*
- **ν — Doc-chunk update, registry-verified** (docs-truth gate leaf 1).
  `crates/reify-mcp/src/tools/chunks/units.md` and `geometry.md`: state that bare numbers are
  rejected at dimensioned geometry positions, with the migration idiom; every documented
  signature verified against `reify-compiler/src/{geometry,geometry_curve,geometry_transform,
  geometry_modify,geometry_boolean}.rs` and the `units.rs` registries. **LEAF.** Signal: each
  documented signature compiles as written in a smoke `.ri`; the chunk names the rejection in
  intent terms. Deps: γ, η.
- **ξ — Exemplar corpus + cheatsheet + discoverability** (docs-truth gate leaves 2–4). Add a
  best-practices exemplar under `examples/best_practices/` for the dimensioned-argument idiom
  **and** update `symmetry_mirror.ri`'s now-stale sentence about `reify check` not telling you;
  add both `INDEX.md` rows; add the one-line index entry in
  `.claude/skills/reify-design/SKILL.md`. **LEAF.** Signal: `cargo test -p reify-compiler --test
  examples_smoke` green (the bidirectional index invariant), and an author who knows the *goal*
  ("why did my part come out 1000× too big?") finds the idiom from the chunk/index by intent.
  Deps: ν.
- **ο — Two-way boundary-test suite (integration gate).** Implement §6's 20 rows; drift-guard
  registrations same-diff (C7). **LEAF.** Signal: the 20-row suite exists and is green on the
  merge gate. Deps: β, γ, δ, ε, ζ, η, θ, ι, κ, λ, μ, 5623, 5658, 5661, 5662.

**DAG.** `α → {β, 5623}`; `β → {γ, δ, ζ, μ}`; `{β, κ} → λ`; `δ → ε`; `γ → {η, θ, ν}`;
`η → 5662`; `5623 → 5658 → 5661`; `{γ, δ, ε, ζ, η, 5623, 5658, 5661} → ι`; `ν → ξ`;
κ parallel from the start; everything → `ο`.

**File-lock note.** `crates/reify-eval/src/geometry_ops.rs` is a single hot 500 KB file that
β, γ, δ, ζ, λ, 5623, 5658 and 5661 all edit; the DAG above **deliberately serializes** them
rather than letting them thrash (the `real-dimensionless-unification.md:64` ordering precedent).
`crates/reify-compiler/src/types.rs` is the second shared file (κ adds `VARIANT_COUNT` impls, λ
edits `PatternKind`'s `Display` + its pinning test), hence `κ → λ`. α (`reify-compiler/src/
geometry.rs`), ε (`reify-stdlib`), η (`builtin_signatures.rs`), θ (the two kernel crates) and
μ (GUI) are off both locks and can run in parallel.

**G7 walk (advisory here, blocking at decompose)** against `docs/legibility/design-invariants.md`:
- `undef-has-provenance` (INV-SF-1): D10 routes `Acceptance::Undefined` into the existing
  *unresolved (Undef)* diagnostic at every new chokepoint — no new silent-Undef path.
- `error-severity-exits-nonzero` (INV-SF-2): C1(iv) — rejections are Error severity and
  `reify eval` exits nonzero; **no per-code escalation list is added**. The residual
  `reify check` exit-0-while-printing-`error:` asymmetry is PRD 2's, and this PRD explicitly
  does not paper over it with a bolt-on.
- `declared-intent-consumed-or-diagnosed` (INV-SF-3): no declaration is dropped; N/A.
- `indeterminate-attributable-transient` (INV-SF-4): no new Indeterminate outcome.
- `placeholders-owned-and-loud` (INV-SF-5): D14 — every allowlist residual cites a live task per
  the PTODO grammar; blanket escapes banned.
- `diagnostics-carry-codes` (INV-SF-6): D9 — a `DiagnosticCode` on every rejection, including a
  retrofit of the existing code-less Contract C sites.
- `parse-is-value-faithful` (INV-SF-7): no grammar change in this PRD (middle-dot is PRD 3's).
- *`no-lockstep-duplication` (dark-factory INV-5 sibling, invoked as design pressure not as a
  reify slug):* M9 prefers delegating the GUI's `UNIT_TABLE` to `unit_symbol_to_si` over
  maintaining a second copy; ι replaces a prose contract with a machine check.

No waivers proposed.

## 9. Out of scope

- **All ANGLE gating**, including `draft.angle` (an R7 slot), `circular_pattern`'s
  `resolve_bare_angle` retirement, ANGLE compile slots, `angle_spec` + its migration hint, `Nm`,
  Energy↔Torque diagnostics, middle-dot/label round-trip → **PRD 3**. (Task α's dimensioned
  `revolve_full` TAU is the one exception, owned here by seam-table decree.)
- **`reify check` semantics** — `cmd_check`, `finish_check`, build-diagnostic collection, exit
  codes, `--strict`, the `main.rs:621` discard → **PRD 2**.
- **Ctor conformance / defaults gates / task 5627's ruling / D5 reinstatement / trajectory
  placeholder migration** → **PRD 4**.
- **Stdlib & native readers, solver load extraction, `.ri` load-struct retyping, the guard's
  second universe** → **PRD 5**.
- **Typed-IR-by-construction (option B).** Recorded as the endgame (D3); best folded into the
  `Real → Scalar{DIMENSIONLESS}` unification. Not built here.
- `arc` sweep radius consumption (D13) — triaged inside 5658/5661, never a signal.
- The `isosurface` absent-`iso` default (D12) — deliberate, documented, stays.

## 10. Open questions (tactical)

1. **`DiagnosticCode` spelling** for the bare/wrong-dimension argument rejection
   (`BareDimensionedArg`? reuse `DimensionMismatch`, which exists at
   `reify-core/src/diagnostics.rs:497`?). One code shared by PRDs 1/3/5 vs one per dimension.
   *Suggested:* one shared new code; `DimensionMismatch` already means arithmetic mismatch.
   Decide during **β**.
2. **Closure-guard target synthesis for ops needing a geometry target or a profile.** The probe
   must synthesize a valid call for e.g. `fillet`, `shell`, `loft`, `pipe`. *Suggested:* a small
   per-family target template (`box(1mm,1mm,1mm)` solid, `circle(1mm)` profile) plus a synthetic
   `step_handles` vec, exactly as `compile_geometry_op_characterization.rs`'s `run()` does; names
   for which no template compiles are recorded as guard-universe residuals with a cited task
   (D14), never silently skipped. Decide during **ι**.
3. **Guard placement and runtime.** In-crate `#[cfg(test)]` module vs a
   `crates/reify-eval/tests/*.rs` integration test (the latter needs the
   `test-instrumentation` feature, already a self-dev-dep). *Suggested:* integration test, next
   to `compile_geometry_op_characterization.rs`. Decide during **ι**; whichever is chosen, C7's
   registrations land same-diff.
4. **GUI rejection surface.** Inline cell error vs a toast vs disabling commit. *Suggested:*
   inline, mirroring existing GUI validation. Decide during **μ**.
5. **Whether `μ` should delete the GUI `UNIT_TABLE` entirely** (delegating both lookup sites to
   `unit_symbol_to_si`) or keep it as a display-ordering table. *Suggested:* delete the value
   half, keep any ordering metadata. Decide during **μ**.
6. **Exact split of the 38 R7 slots between β and γ** if either diff proves too large in
   practice. *Suggested:* the split in §8 (primitives+profiles / modify+sweep); a further split
   is a decompose-time judgement, not a design change.
7. **Whether `ξ` adds one exemplar or two** (a dedicated dimensioned-arguments file vs folding
   the idiom into `symmetry_mirror.ri`'s update). *Suggested:* one new file + the
   `symmetry_mirror.ri` correction. Decide during **ξ**.
