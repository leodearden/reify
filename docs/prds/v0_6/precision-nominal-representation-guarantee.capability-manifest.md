# Capability manifest — `precision-nominal-representation-guarantee`

**PRD:** `docs/prds/v0_6/precision-nominal-representation-guarantee.md` (landed `a7ffbafcf8`)
**Decomposed:** 2026-08-10 · **Substrate HEAD:** `0560ca6f0a`
**Batch:** tasks **6166–6177** (12) · 21 dependency edges (20 intra-batch, 1 out-of-batch → task **6085**)

| Label | Task | Prio | Prereqs | Kind |
|---|---|---|---|---|
| α | **6166** | high | — | intermediate → γ1 |
| β | **6167** | high | — | intermediate → γ1, δ, θ |
| δ | **6168** | high | β | leaf |
| ζ | **6169** | high | — | leaf |
| η | **6170** | **critical** | — | leaf · **the §1.1 fix** |
| γ1 | **6171** | high | α, β, δ | leaf |
| γ2 | **6172** | high | γ1, ζ | leaf |
| θ | **6173** | medium | γ1, β, **6085** | leaf |
| ι | **6174** | medium | γ1, γ2, ζ, η | leaf (docs-truth 1+4) |
| κ | **6175** | medium | γ1 | leaf (docs-truth 2+3) |
| λ | **6176** | medium | γ1, γ2 | leaf (docs-truth prose) |
| μ | **6177** | high | γ1, γ2, δ, ζ, η | leaf · **integration gate** |

**Machine-readable twin:** `docs/prds/v0_6/precision-nominal-representation-guarantee.capability-manifest.yaml`
(12 labels · 64 capability bindings · **0 FAIL** · 42 mechanical `delivered_check`s, 22 `manual`).

**G3 grammar gate: N/A.** This PRD adds **no novel syntax** — every `.ri` construct it
relies on (`#precision(<Length>)`, `RepresentationWithin(subject, B)`, `structure`/`param`/
`constraint`, `STLOutput`) is shipped and exercised by the five existing bound-carrying
fixtures. No grammar fixture is fabricated; per `references/grammar-gate.md` the gate is a
no-op here and is recorded as such rather than as a passing fixture.

**D3 harness (`scripts/prd-decompose-verify.mjs`) was NOT invoked.** The Workflow tool is
disabled for this session by an explicit operator directive. Every binding below was
therefore verified **by hand, by direct read of tracked source at `0560ca6f0a`**, with the
exact `git grep` / `sed` anchors recorded per row. Two rows (η, δ) are `PASS-with-amendment`:
the substrate read *falsified* the PRD's scoping and the correction is written verbatim into
the filed task as a **BINDING DECOMPOSE AMENDMENT**. No row is bound to a script that does
not exist, and no row is marked PASS on the strength of a sibling leaf's future work — every
producer cited is either wired on `main` today or an **upstream** task in the same batch.

---

## Substrate drift since the PRD's own §3 verification

The PRD verified its anchors at `9d08d3d3d8`; `main` is now `0560ca6f0a`. All 14 §3 rows
re-verified. Line numbers drifted by ≤ 20 in three files; **no anchor moved crate, function
or meaning.** Because line anchors go stale, every `delivered_check` in the sidecar is
**pattern-anchored** (`kind: grep` with an ERE), never `file:line` — the `file:line` values
below are decompose-time *evidence*, not dispatch-time checks.

| PRD §3 anchor | PRD line | Verified line @ `0560ca6f0a` |
|---|---|---|
| `measure_mesh_deviation` (C++) | `occt_wrapper.cpp:6491` | **`:6510`** |
| … decl / FFI | `occt_wrapper.h:1607`, `ffi.rs:1327` | **`:1608`**, **`:1329`** |
| `achieved_repr_tol` population | `geometry_ops.rs:10950-10955` | **`:10951-10956`** |
| tessellate call site | `geometry_ops.rs:10935` | **`:10937`** |
| `PLANAR_FLOOR` | `tolerance_combine.rs:232` | `:232` ✓ |
| `resolve_repr_tol_key` | `:316-360` | **`:320`** |
| `match_representation_within_shape` | `:116-196` | `:116` ✓ |
| `eval_representation_within` | `:267-313` | **`:266`** |
| `dispatch_constraints` fast-path guard | `engine_constraints.rs:333` | **`:334`** |
| `module_has_representation_within` | `main.rs:2462` | `:2462` ✓ |
| `set_capture_repr_tol(true)` call site | `main.rs:607` | `:607` ✓ |
| `compute_tessellation_budgets` | `engine_build.rs:6168` | `:6168` ✓ |
| `build_outputs` / `_with_result` | `:5044` / `:5094` | `:5044` / `:5094` ✓ |
| `Diagnostic::{error,warning,info}` / `with_code` | `diagnostics.rs:3888/3898/3908/3928` | ✓ all four |
| `DEFAULT_STL_TESSELLATION_TOLERANCE = 0.1` | `kernel lib.rs:4073` | `:4073` ✓ |
| §3.1(c) circularity `unwrap_or` | `engine_build.rs:6205` | **`:6201`, and the landed form is `unwrap_or_else(\|\| Self::…)`** — semantically identical (see κ4) |

**Every one of the 42 mechanical `delivered_check`s in the sidecar was executed at decompose
time** (`git grep -lE <pattern> -- <paths>`, polarity checked against `expect`): **42/42
agree**. The κ4 row is the one that did *not* agree on first authoring — it was bound to the
PRD's literal `unwrap_or(…)` spelling, which does not exist; running the checks caught it and
the row was rebound to the landed `unwrap_or_else` form. That is the manifest's own
"never bind a row to something that does not exist" rule catching itself.

---

## Two decompose-time falsifications (the rows that block-then-resolve)

### F1 — η's named producer does not cover the surface η's signal names (`producer-extent-short`)

**The finding.** `reify build` has **two disjoint export modes**, and the PRD binds η to the
wrong one.

| Mode | Trigger | CLI site | Engine producer |
|---|---|---|---|
| **A** — imperative single-output | `reify build -o out.stl` | `main.rs:1020-1031` | `Engine::build` → `build_with_geometry_output(.., emit=true)` (`engine_build.rs:3925`) |
| **B** — declarative `: Output` occurrences | `reify build` (no `-o`) | `main.rs:1145` | `Engine::build_outputs_with_result` (`engine_build.rs:5094`) |

η's *signal*, §1.1's motivating failure and BT-7's *command* are all
`reify build -o out.stl` — **Mode A**. η's *Modules* line names `build_outputs` /
`build_outputs_with_result` — **Mode B only**. Delivering η exactly as scoped would close the
task green while leaving `reify build -o out.stl` still writing a 0.1 m-deflection STL at
exit 0 — the precise fake-done shape this manifest exists to catch.

**Second-order hazard found while resolving it.** `Engine::build` is **not** export-exclusive:
it is also called by `cmd_check`'s geometry-precondition arm (`main.rs:621`, result discarded
via `let _ =`) and by `cmd_eval`'s geometry path (`main.rs:1573`, whose `result.diagnostics`
**do** reach the user and **do** feed an `Severity::Error` exit gate). An unconditional
refusal emitted inside `Engine::build` would therefore make `reify eval` exit non-zero on
every bounded module — a regression the PRD does not sanction (§4.6's surface table has no
`reify eval` row). Mode A's refusal must be discriminated to the **exporting** call site.

**Resolution — G6 (c), *change the asserted configuration so the claim becomes true*.**
η's extent is widened to both modes and its lock charter gains
`crates/reify-cli/src/main.rs`. The PRD's η *Lock note* ("no CLI file is touched") is
**corrected**: it holds for Mode B only. Mode A's cheapest correct shape reuses the helper
already on `main` — `module_has_representation_within(&compiled)` (`main.rs:2462`, private to
the same module as `cmd_build`) — inside `cmd_build`'s Mode-A arm, pushing an Error-severity
coded diagnostic into the vector `has_error_diagnostic` reads at `main.rs:1106-1107`, whose
`build_is_success` gate (`:1108-1112`) already converts it to `ExitCode::FAILURE`. Whether the
out-of-spec bytes are still written before the non-zero exit stays **tactical** (BT-7 requires
only that the file is not *presented as success*).

Written verbatim into task η and into task μ (BT-7 splits into 7a/7b/7c).

### F2 — δ cannot reach the bound table without a threading change in `engine_build.rs`

**The finding.** δ's *Modules* line names only `crates/reify-eval/src/geometry_ops.rs`. The
tessellate closure that owns the `measure_mesh_deviation` call (`geometry_ops.rs:10937-10956`)
does **not** hold the `CompiledModule`; `capture_repr_tol` and `achieved_repr_tol` are
**threaded in explicitly** from `engine_build.rs` — passed at `:5927-5928`, declared as helper
params at `:6282` / `:6289`, forwarded at `:6720-6721`. β's `compute_representation_bounds`
output must travel the same wire, so δ necessarily edits `engine_build.rs` as well.

**Resolution.** δ's `metadata.files` gains `crates/reify-eval/src/engine_build.rs`, with the
three threading sites named in the task text. Not a re-scope — same crate, same signal, same
deliverable; only the declared lock charter changes so BRE does not have to acquire mid-flight.

### Ordering corrections (lock topology, not direction)

Two edges are wired that §9's DAG does not draw. Neither changes any signal, scope or the
E+F+D direction; both convert a same-file collision into a sequence — the technique §9 itself
uses for θ↔6085.

1. **γ1 depends on δ.** Both rewrite the *same ~12-line closure* at
   `geometry_ops.rs:10937-10956`; C-LOOP's own pseudocode carries δ's early-return as a line
   (`if bound is None: push(mesh); return  # δ`). Landing δ first means γ1's architect wraps
   the loop around an already-narrowed site instead of reconciling a conflict.
2. **γ2 depends on ζ.** Both edit `engine_constraints.rs`, and C-LOG's emission table row 5
   (*Indeterminate, surface does not measure*) **is** ζ's diagnostic — γ2 should give the
   existing ζ diagnostic its `DiagnosticCode`, not invent a second one. ζ is dependency-free,
   so this delays nothing on ζ's side.

**η is deliberately left dependency-free** (brief constraint, PRD §1.1) and filed `critical`,
so it is not ordered behind the refine loop. It contends with β/γ1/δ on `engine_build.rs`;
that is accepted hot-file contention, not a wiring defect.

---

## Per-task bindings

Verdict vocabulary in this `.md`: `PASS` · `PASS-with-amendment` (substrate falsified the
PRD; correction written into the filed task) · FAIL values per `gates.md` (none present).
The **sidecar's** `verdict` field is a strict `PASS`/`FAIL` literal
(`CapabilityManifestDoc`), so the three amended rows carry `verdict: PASS` there with the
correction signalled by a leading `PASS-WITH-AMENDMENT.` in the `binding` prose. The two
twins say the same thing; only the encoding differs.

### α — Calibrate the refine envelope and the per-iteration cost *(intermediate → γ1)*

| # | Capability asserted | Evidence | Verdict |
|---|---|---|---|
| α1 | The probe recipe is executable today: a bound on a **separate checker** structure forces a violation whose message carries the achieved value | `crates/reify-eval/src/tolerance_combine.rs:266` `eval_representation_within` returns `(Satisfaction, Option<Diagnostic>)`; the violation message carries the sampled value (`sampled facet deviation … exceeds bound …`). Shipped by **4198/4199, both `done`**. | PASS |
| α2 | `#precision` is threaded bit-exact so the measured ratio is attributable to OCCT, not to a reify pre-scale | `crates/reify-eval/src/tolerance_budget.rs:34` `SAFETY_FACTOR`; `dispatcher.rs:548-561` `per_stage_tolerance_for_plan` empty-chain pass-through — the `0.8` does **not** fire on the BRep→BRep empty-conversion path. PRD §3 row, re-read. | PASS |
| α3 | **Negative**: `reify build -o *.stl` is useless as a probe vector (ignores `#precision`) | `crates/reify-kernel-occt/src/lib.rs:4073` `DEFAULT_STL_TESSELLATION_TOLERANCE: f64 = 0.1`, consumed at `:4099`, `:4106`, `:4157` — a hardcoded constant, no `#precision` read. **Observed present**, so the "useless" warning in α's text is substantiated, not folklore. | PASS |
| α4 | α cannot falsify the design | D measures rather than predicts (§4.2 termination holds for **any** finite `K`), so an unexpected overshoot regime changes iteration count only. No numeric bound is asserted by α — it *produces* one. | PASS (no premise) |

*No numeric floor applies: α asserts no bound.* `REFINE_ATTEMPT_CAP` provisional **4** is a
cost/reach knob, never a soundness constant (§4.2).

### β — Bound pre-pass + shared subject→realization predicate *(intermediate → γ1, δ, θ)*

| # | Capability asserted | Evidence | Verdict |
|---|---|---|---|
| β1 | A shape matcher exists to scan module constraints for `RepresentationWithin` | `tolerance_combine.rs:116` `pub(crate) fn match_representation_within_shape` — wired, called at `:220`, `:273`, `:443`. | PASS |
| β2 | `struct_name` is statically available **pre-tessellation** (so the bound table can be built before any mesh exists) | `tolerance_combine.rs:116-196`: `struct_name` comes from `arg0.result_type = Type::StructureRef(name)` — the subject's *declared type*, not a realized handle. | PASS |
| β3 | A type-name fallback exists inside `resolve_repr_tol_key` that can be **extracted** into `realization_belongs_to` | `tolerance_combine.rs:320` `fn resolve_repr_tol_key` — value-based path first, then the `"{struct_name}#realization["` prefix scan with **max** over matches. Present and wired (called at `:276`). | PASS |
| β4 | Extraction cannot drift the two consumers apart | C-BOUND-1 makes loop and assertion call the *same* fn; BT-1 (task μ, **downstream**) asserts refined-set ≡ evaluated-set. Producer of the guarantee is β itself; the *test* is downstream, which is the correct direction. | PASS |

### γ1 — Measure-and-refine loop *(deps: α, β, δ)*

| # | Capability asserted | Evidence | Verdict |
|---|---|---|---|
| γ1-1 | The metric is callable at the tessellate site and takes **no** tolerance argument (structural anti-circularity) | `crates/reify-geometry/src/lib.rs:111` trait method; `crates/reify-kernel-occt/cpp/occt_wrapper.cpp:6510` `double measure_mesh_deviation(const OcctShape&, const TessResult&)`; FFI `ffi.rs:1329`. **Wired on the production path**, not test-only: called at `geometry_ops.rs:10953`. | PASS |
| γ1-2 | The tessellate site holds kernel + `placed_id` + fresh mesh + `entity_path` simultaneously | `geometry_ops.rs:10935-10956` — the closure signature `&mut |kernel, placed_id, entity_path, default_visible, t, r, diag|` carries all four. | PASS |
| γ1-3 | A per-realization deflection is available to halve | `engine_build.rs:6168` `compute_tessellation_budgets` → `tessellation_budgets[t][r]`, consumed at `geometry_ops.rs:10936-10937` `kernel.tessellate(placed_id, budget)`. | PASS |
| γ1-4 | Re-tessellating the **same** handle at a finer deflection is legal (no re-run of geometry ops) | `RealizationCache` keys `KernelHandle` on `(entity, repr, demanded_tol)` and the loop re-calls `kernel.tessellate` on the *same* handle — no cache key changes (§4.9). `kernel.tessellate(handle, tol)` is called repeatedly across sites today (`engine_build.rs:7996`, `:8928`, `:9190`, `:9733`), so repeat invocation is an exercised path. | PASS |
| γ1-5 | **End-to-end capability** (§10 row 1): every capability γ1's signal needs is on `main` or **upstream** | metric ✓ (γ1-1), budget array ✓ (γ1-3), bound table ← **β** (upstream), cap ← **α** (upstream), narrowed measure site ← **δ** (upstream, see F2/ordering). **Nothing is owned by a task that depends on γ1.** | PASS |
| γ1-6 | **Numeric floor** (G6 branch 1): `bound > f32_floor` | f32 ULP at coordinate magnitude `S` is `2⁻²³·S ≈ 1.19e-7·S` — `occt_wrapper.cpp:6360-6362` emits vertices as `static_cast<float>(p.X/Y/Z())`, so sample points inherit the quantization. Independently corroborated by the shipped `PLANAR_FLOOR = 1e-5` doc's "~1e-6 m f32 quantization" note (`tolerance_combine.rs:232`). C-FLOOR enforces `eff > f32_floor(mesh)` **at runtime**, so the floor is a live guard, not prose. | PASS (`floor: bound > 1.19e-7·S`) |
| γ1-7 | **Termination**: `a_n ≤ K·d₀·2⁻ⁿ → 0` for any finite regime supremum `K` | Pure geometric bisection (§4.2); no constant is pinned, so termination does not depend on α's measured `K`. The corrective step `d·(B/achieved)` is rejected on non-monotonicity + unbounded cost. | PASS |
| γ1-8 | **Negative signal**: an unbounded module is byte-identical | `set_capture_repr_tol(true)` has **exactly one production call site** — `crates/reify-cli/src/main.rs:607`, inside `cmd_check`, gated on `module_has_representation_within(&compiled)` (`:600`). Every other hit in tracked source is a comment or a `tests/` call. GUI and `reify build` never set it. F's scoping is therefore **structural**, not new gating code. | PASS |
| γ1-9 | Zero-bound semantics unchanged | `tolerance_combine.rs:285` `let eff = if bound <= 0.0 { PLANAR_FLOOR } else { bound };` — C-ZERO reuses this comparator verbatim. | PASS |
| γ1-10 | The `repr_refine_log` field has a reset choke-point to attach to | `engine_build.rs:3171` destructures `achieved_repr_tol` in the per-build reset; `:3251` `achieved_repr_tol.clear()`; INV-BUILD-1 documented at `:3112`. Sibling field lands in the same destructure. | PASS |

### γ2 — Refine telemetry as user-visible diagnostics *(deps: γ1, ζ)*

| # | Capability asserted | Evidence | Verdict |
|---|---|---|---|
| γ2-1 | Constraint-channel diagnostics reach the user (the tessellate channel does **not**) | `engine_constraints.rs:674` `fn push_constraint_result` folds `ConstraintResult.diagnostics.messages` into `CheckResult.diagnostics` → `report_eval_output` (`main.rs:2756-2766`). Confirmed converse: `main.rs:626` calls `engine.tessellate_realizations(&compiled);` as a **bare statement** — the `TessellateResult` is discarded, so a tessellate-channel diagnostic is unreachable. C-LOG's routing is forced, not preferred. | PASS |
| γ2-2 | `Info` severity exists and is printed | `crates/reify-core/src/diagnostics.rs:3908` `pub fn info`; `Severity` at `:98`; `report_eval_output` (`main.rs:2763`) prints every diagnostic to stderr regardless of severity. | PASS |
| γ2-3 | `DiagnosticCode` can be attached (INV-SF-6) | `diagnostics.rs:3928` `pub fn with_code(mut self, code: DiagnosticCode) -> Self`. | PASS |
| γ2-4 | `eval_representation_within`'s return type can widen `Option<Diagnostic>` → `Vec<Diagnostic>` without a foreign consumer break | Sole production caller is `engine_constraints.rs:365` (inside `dispatch_constraints`); remaining callers are `tolerance_combine.rs`'s own `#[cfg(test)]` block (`:1351, :1374, :1403, :1425, :1446`). Blast radius is one crate. | PASS |
| γ2-5 | **Rejection / cap-hit is observed, not assumed** (G6 branch 4) | BT-4's fixture is committed and its premise re-derived arithmetically: `examples/representation_within.ri` is a **1 m** sphere at `#precision(50mm)` with a **1 µm** bound. At `REFINE_ATTEMPT_CAP = 4`, `d` reaches `50/2⁴ = 3.125 mm`; achieved ≈ `2.07 × 3.125 mm ≈ 6.5 mm` — **three orders above** the 1 µm bound, so the cap is genuinely exhausted. The Error path is exercised by an existing committed file, not a hypothetical. | PASS |
| γ2-6 | BT-4 is a **cap-hit**, not a C-FLOOR refusal — the two Error paths are distinguishable | At `S = 1 m` the f32 floor is `1.19e-7 m = 0.119 µm`; BT-4's bound is `1 µm` = **8.4 × above** it, so C-FLOOR does not fire. BT-6 (`1e-9 m` on a 1 m part) is `119 ×` **below** it and fires. Distinct fixtures, distinct diagnostics, no aliasing. | PASS (`floor: 1e-6 > 1.19e-7`; `1e-9 ≤ 1.19e-7` ⇒ BT-6 refuses **by design**) |
| γ2-7 | **INV-SF-2 caveat** — the cap-hit Error does not itself drive `reify check`'s exit | `reify check` decides exit from constraint outcomes alone (`finish_check`; recorded as a known INV-SF-2 gap in `docs/legibility/design-invariants.md` INV-SF-2 *Evidence*). γ2's cap-hit case is **already `Violated`**, so the non-zero exit comes from the verdict, not the severity. γ2 introduces **no new** INV-SF-2 violation, but must not be implemented on the assumption that the Error alone exits. Written into the task. | PASS (bound, with the assumption named) |

### δ — Narrow measurement to bounded subjects *(deps: β)*

| # | Capability asserted | Evidence | Verdict |
|---|---|---|---|
| δ1 | The measure call is already gated by a boolean at exactly one site — narrowing is a predicate swap, not a new seam | `geometry_ops.rs:10951-10956`: `if capture_repr_tol && !mesh.indices.is_empty() && let Some(dev) = kernel.measure_mesh_deviation(placed_id, &mesh) { achieved_repr_tol.insert(entity_path.clone(), dev); }`. | PASS |
| δ2 | The bound table can **reach** that site | **FALSIFIED as scoped — see F2.** The closure holds no `CompiledModule`; `capture_repr_tol`/`achieved_repr_tol` are threaded from `engine_build.rs` (`:5927-5928` call, `:6282`/`:6289` helper params, `:6720-6721` forward). Resolved by widening δ's lock charter to `engine_build.rs` and naming the three sites in the task. | **PASS-with-amendment** |
| δ3 | **Negative assertion** — unbounded realizations are not measured — is *structurally* checkable, not wall-clock | The regression test asserts the `achieved_repr_tol` **key set** equals the bounded subjects' keys. Accessor exists: `engine_admin.rs:608` `pub fn achieved_repr_tol(&self, occurrence: &str) -> Option<f64>`, plus `set_achieved_repr_tol_for_test` (`:2540`) for the twin direction. No wall-clock upper bound is asserted — `tests/infra/test_no_new_wallclock_upper_bounds.sh` stays green. | PASS |
| δ4 | Narrowing cannot change a verdict | `resolve_repr_tol_key` only ever reads keys matching `"{struct_name}#realization["` for a `struct_name` that **has** a bound (`tolerance_combine.rs:320-360`), so unbounded keys are unreadable by construction. | PASS |
| δ5 | No production reader depends on unbounded-subject keys | Only doc reference found is `crates/reify-eval/src/lib.rs:1061`; no production caller of `Engine::achieved_repr_tol()`. Confirmation before landing is PRD Open Question 3 — **tactical**, recorded, not a blocking premise. | PASS |

### ζ — Attributable `Indeterminate` on surfaces that do not measure *(deps: none)*

| # | Capability asserted | Evidence | Verdict |
|---|---|---|---|
| ζ1 | **Negative assertion — today's reason is wrong — is observed, not inferred** | Measured verbatim in PRD §3.2 (`reify build --verbose` → `warning: constraint SphereCheck#constraint[0] indeterminate: operator undefined for these operand kinds: StructureInstance`, exit 0). **Structurally re-derived at `0560ca6f0a`, which is stronger than a re-run:** (i) `set_capture_repr_tol` has one production call site, in `cmd_check` only (`main.rs:607`), so `build` leaves `achieved_repr_tol` empty; (ii) `dispatch_constraints`' fast-path guard fires on exactly that condition (`engine_constraints.rs:334`, `if self.achieved_repr_tol.is_empty() && self.optimization_registry.is_empty()`); (iii) the assertion then falls through to the language checker, which emits that literal string at `crates/reify-constraints/src/lib.rs:201`. All three links present and wired. | PASS |
| ζ2 | The peel can be lifted **above** the fast-path guard | The peel and the guard are in the same fn, `dispatch_constraints` (`engine_constraints.rs:313`); the guard is a plain early-return at `:334`, the peel loop begins `:363`. Reordering is local. | PASS |
| ζ3 | The zero-allocation non-assertion hot path is preserved | Documented invariant C2 at `engine_constraints.rs:310-312`; the fast path builds no `rw_slots`. PRD Open Question 2 leaves the shape (a precomputed per-check boolean) **tactical** — no premise rests on a specific implementation. | PASS |
| ζ4 | `Info` severity keeps ζ INV-SF-2-safe | C-LOG's Indeterminate row is **Info**, so no Error is emitted on a healthy path and no exit code changes on `reify build` / `reify eval` / GUI. | PASS |

### η — Export refuses rather than shipping an unenforced bound *(deps: none)* — **highest value**

| # | Capability asserted | Evidence | Verdict |
|---|---|---|---|
| η1 | **Rejection mechanism exists and fires** (G6 branch 4) | `main.rs:1106-1107` `let has_error_diagnostic = result.diagnostics.iter().any(\|d\| d.severity == Severity::Error);` → `:1108-1112` `if build_is_success(&outcome, has_error_diagnostic) { SUCCESS } else { FAILURE }`. The Mode-B twin is the same shape at `:1259-1265`. **Both gates read on `main` today** — η rides a live mechanism, it does not invent one. | PASS |
| η2 | **Rejection is observed ABSENT today** (the vacuity check) | Zero `.ri` in the repo pairs a bound with an export: all five files carrying `RepresentationWithin` — `crates/reify-cli/tests/fixtures/{dfm_with_repr_within,representation_within_satisfied}.ri`, `examples/{fea_bracket_member_access,representation_within}.ri`, `examples/tolerancing/gdt_pass_weave.ri` — return **0** matches for `Output\|STLOutput\|StepOutput`. Re-measured at `0560ca6f0a`. η therefore breaks no existing fixture and fires only on the currently-silent case. | PASS |
| η3 | The refusal predicate is already implemented | `main.rs:2462` `fn module_has_representation_within(module: &CompiledModule) -> bool`, unit-tested at `:3610`. Engine-side twin arrives with β's `compute_representation_bounds`, but η **needs neither β nor 6085 nor any measurement** — the boolean is sufficient, which is why η is dependency-free. | PASS |
| η4 | The named producer covers the surface the signal names | **FALSIFIED — see F1.** `build_outputs`/`build_outputs_with_result` are Mode **B** only; `reify build -o out.stl` is Mode **A** (`Engine::build`, `main.rs:1031`). Resolved by widening η to both modes + adding `crates/reify-cli/src/main.rs` to the lock charter; the PRD's "no CLI file is touched" Lock note is corrected to Mode B only. | **PASS-with-amendment** |
| η5 | Mode A's refusal must not regress `reify eval` / `reify check` | `Engine::build` (`engine_build.rs:3925` → `build_with_geometry_output(.., emit=true)`) is shared with `cmd_check`'s precondition realization (`main.rs:621`, discarded) and `cmd_eval`'s geometry path (`main.rs:1573`, whose diagnostics **do** reach an Error exit gate). `emit_geometry_output` does **not** discriminate — all three pass `true`. Refusal must be sited at the exporting call site. Named as a must-not-regress constraint in η, with the three bound-carrying fixtures as the check. | PASS (hazard named, bound) |

### θ — Enforce the bound on export *(deps: γ1, β, task 6085)*

| # | Capability asserted | Evidence | Verdict |
|---|---|---|---|
| θ1 | **DAG direction** — the plumbing θ consumes is **upstream**, never downstream | Task **6085** is a real `add_dependency` edge (same project ⇒ integer edge, not `metadata.external_deps`). Status re-read at decompose: **`pending`**. | PASS (`producer: task-6085 upstream`) |
| θ2 | The capability θ needs is genuinely absent today (so the dep is load-bearing, not decorative) | `crates/reify-kernel-occt/src/lib.rs:4073` `DEFAULT_STL_TESSELLATION_TOLERANCE: f64 = 0.1` hardcoded at all three export call sites (`:4099`, `:4106`, `:4157`) — no `#precision`, no demanded tolerance reaches `kernel.export()`. 6085 owns both that constant and its wrong `0.1 mm` doc comment. | PASS |
| θ3 | `metadata.files` is `[]` | Deliberate: θ's footprint depends on 6085's landed `ExportOptions` shape (PRD §9, Open Question 5). `[]` is first-class — BRE acquires the real footprint before editing. Guard: `lock-charter-guard.sh check` (empty list) → exit 0. | PASS |

*θ asserts a numeric capability (“an exported STL measures within `B`”) whose floor is
γ1's — `bound > 1.19e-7·S`, enforced by the same C-FLOOR guard, since θ reuses C-LOOP.*

### ι — Doc chunks, registry-verified *(deps: γ1, γ2, ζ, η)*

| # | Capability asserted | Evidence | Verdict |
|---|---|---|---|
| ι1 | The two target chunks exist and this is an **add**, not an update | `crates/reify-mcp/src/tools/chunks/` contains `constraints.md` and `syntax.md`. `git grep -niE "representationwithin\|#precision" -- 'crates/reify-mcp/src/tools/chunks/*.md'` returns **zero** hits — PRD §3.1(d) re-confirmed at `0560ca6f0a`. | PASS |
| ι2 | Documented signatures are checkable | Docs-truth arm 1: each documented signature must compile as written in a smoke `.ri`. `RepresentationWithin(subject, B)` and `#precision(<Length>)` are exercised by five committed `.ri` files today, so the smoke has live exemplars to mirror. | PASS |
| ι3 | Discoverability (docs-truth arm 4) is a real, non-vacuous requirement | The intent-level phrasing ("make sure the exported mesh is within 0.05 mm of the true surface") has **no** chunk presence today (ι1), so the acceptance cannot pass vacuously against pre-existing text. | PASS |
| ι4 | Ordered **after** the mechanism it documents | Real edges on γ1, γ2, ζ, η — the docs-truth decompose rule (no prose-only ordering) is satisfied. | PASS |

### κ — Exemplar + index *(deps: γ1)*

| # | Capability asserted | Evidence | Verdict |
|---|---|---|---|
| κ1 | The corpus compile gate exists and is the signal | `crates/reify-compiler/tests/examples_smoke.rs` — auto-compiles `examples/best_practices/`. | PASS |
| κ2 | The index-consistency test exists by the exact name the signal cites | `crates/reify-compiler/tests/examples_smoke.rs:548` `fn best_practices_index_matches_corpus_directory()`. **Bound to a test that exists**, not a script that might. | PASS |
| κ3 | The three index sites exist | `examples/best_practices/INDEX.md` ✓ and `.claude/skills/reify-design/SKILL.md` ✓ (docs-truth arm 3 — a one-line index entry, not an inline playbook). | PASS |
| κ4 | The anti-pattern κ documents is real | `engine_build.rs:6200-6201` — `let req_tol = demanded_tols[t_idx][r_idx].unwrap_or_else(\|\| Self::effective_tessellation_tolerance(module));`. A **replace**, never a min-fold, so a bound on the geometry-owning template displaces `#precision` outright and the assertion is tautologically violated. The circular layout κ warns against is a live trap. **Text drift:** §3.1(c) writes this as `unwrap_or(…)` at `:6205`; the landed form is `unwrap_or_else(\|\| Self::…)` at `:6201`. Semantically identical — §3.1(c)'s conclusion stands verbatim, only the spelling and line differ. | PASS |

### λ — Prose sites *(deps: γ1, γ2)*

| # | Capability asserted | Evidence | Verdict |
|---|---|---|---|
| λ1 | The language-spec claim λ corrects is present verbatim | `docs/reify-language-spec.md` §12.2: "**`#precision`** -- hint to toolchain about numeric precision:" followed by a lone `#precision(float64)` block — the `Length` form (the one with semantics) is **absent**. Re-read at `0560ca6f0a`. | PASS |
| λ2 | The stdlib-reference sentence λ deletes is present verbatim | `docs/reify-stdlib-reference.md`: "Use a finer `#precision` directive to reduce the measurement gap." inside the PRD §8.3 sampled-lower-bound caveat block. | PASS |
| λ3 | `docs/prds/pragmas.md` exists and is a shipped record (pointer only, not a rewrite) | File present; §2-equivalent "Items" section at `:20`. | PASS |
| λ4 | `examples/representation_within.ri`'s documented behaviour is **preserved** by this PRD | Its header documents `reify check → exit non-zero, "VIOLATED"`. Post-γ1/γ2 it is a **cap-hit** (γ2-5), so the verdict stays `Violated` and the exit stays non-zero — λ adds the attempts/cap prose without contradicting the file's own contract. | PASS |
| λ5 | **Scope isolation from 5976/5977/6058 holds** | λ touches none of `crates/reify-cli/tests/fixtures/*.ri` or `cli_dfm_overhang.rs`. **Status drift found at decompose:** 5976 and 5977 are **`done`** (PRD §7 and the brief both say `in-progress`), and 6058 is **`in-progress`** (PRD §7 says `pending`). Drift is favourable — the file owners have landed — and changes nothing: λ stays scoped away and takes **no** dependency on them, per the brief. | PASS (with status correction) |
| λ6 | λ's sampling-gap claim is the honest one | ≤ **1.0046** measured across 48 configurations; **16/15 = 1.0667** provable, and provable only for the sphere-vertex quadratic-identity family — λ documents both numbers with that scope caveat rather than projecting 16/15 repo-wide (§4.3). | PASS |

### μ — Two-way boundary tests *(deps: γ1, γ2, δ, ζ, η)* — the integration gate (G5 B+H closure)

| # | Capability asserted | Evidence | Verdict |
|---|---|---|---|
| μ1 | Both target test binaries exist | `crates/reify-eval/tests/representation_within_assertion.rs` ✓ · `crates/reify-cli/tests/harness_cli/cli_determinacy_gate.rs` ✓. | PASS |
| μ2 | Both are **already registered** in the `occt` nextest test-group — so **no** drift-guard registration task is needed | `.config/nextest.toml:113-114`: `filter = 'package(reify-kernel-occt) \| package(reify-kernel-conformance) \| package(reify-eval) \| package(reify-cli) \| package(reify-config)'`, `test-group = 'occt'` (cap `:35`). Landing in these binaries means **no new** `crates/*/tests/*.rs` and **no new** `tests/infra/test_*.sh` — the overlay's gate-test drift-guard trigger does not fire, so there is no registration task that could be ordered *after* the test-adding task. **The esc-4914-162 A3-before-A6 shape is structurally impossible in this batch.** | PASS |
| μ3 | The no-wall-clock rule is enforceable | `tests/infra/test_no_new_wallclock_upper_bounds.sh` ✓ present. BT-4's cost check is recorded as a task measurement, never a gate assertion. | PASS |
| μ4 | Every capability BT-1…BT-10 exercises is **upstream** of μ | γ1, γ2, δ, ζ, η are all real `add_dependency` edges. BT-1 needs β's shared predicate (transitively upstream via γ1/δ). **No BT depends on a task that depends on μ.** | PASS |
| μ5 | BT-3's byte-identity claim is achievable | Byte-identity is asserted only for the **unbounded** module — a path that never enters the loop (γ1-8, structural). The *refined* result is iterative, so per `decisions_byte_identity_iterative_vs_closedform` byte-identity is deliberately **not** claimed there; BT-10 asserts determinism instead. Correct regime split. | PASS |
| μ6 | BT-7 as written mixes the two export modes | **FALSIFIED — see F1.** Precondition names an `STLOutput` occurrence (Mode B) while the command is `reify build -o out.stl` (Mode A). Resolved by splitting into **BT-7a** (Mode A, `-o`, no `Output` occurrence), **BT-7b** (Mode B, `STLOutput` occurrence, plain `reify build`) and **BT-7c** (non-regression: `reify eval` / `reify check` on a bounded module unchanged). Written into μ. | **PASS-with-amendment** |
| μ7 | BT-5's C-KEEP check is expressible | `measure_mesh_deviation` is recomputable on any mesh via the trait method (`reify-geometry/src/lib.rs:111`), and `TessellateResult.meshes` carries the retained mesh, so "verdict == metric recomputed on the retained mesh" is directly assertable. | PASS |

---

## G7 walk — `docs/legibility/design-invariants.md` (INV-SF-1…7)

Walked across **all 12** tasks, not only leaves. **No waiver required**; `metadata.g7_waivers`
is unset on every filed task.

| Invariant | Batch-wide verdict |
|---|---|
| `undef-has-provenance` (INV-SF-1) | No task creates a root `Undef` cell. The loop writes measured `f64`s or nothing; C-KEEP guarantees the recorded value is always the one measured on the retained mesh. **Clear.** |
| `error-severity-exits-nonzero` (INV-SF-2) | γ2's satisfied-after-refinement diagnostic is **Info**, honouring the severity-hygiene corollary (expected on a healthy path ⇒ not Error). η's and γ2's Error diagnostics both ride the **existing** gates (`main.rs:1108`/`:1261`) — no per-code bolt-on. **Caveat recorded (γ2-7):** `reify check`'s exit is verdict-driven, not severity-driven; γ2's cap-hit is already `Violated`, so the batch neither depends on nor worsens that known gap. **Clear.** |
| `declared-intent-consumed-or-diagnosed` (INV-SF-3) | This is the PRD's third goal. η and ζ exist precisely so a declared bound is never silently dropped by a surface that cannot honour it. **Clear — this batch *closes* an INV-SF-3 hole.** |
| `indeterminate-attributable-transient` (INV-SF-4) | ζ replaces the misattributed `operator undefined for these operand kinds` with the true runtime reason and names what clears it (`reify check`). C-FLOOR's permanently-unachievable case is reported as **Violated with a named floor** — never a permanent `Indeterminate`, which would itself violate INV-SF-4. **Clear.** |
| `placeholders-owned-and-loud` (INV-SF-5) | No placeholder type, sentinel default or stand-in signature introduced. `REFINE_ATTEMPT_CAP` is a tuned constant with an owner (α), not a placeholder. **Clear.** |
| `diagnostics-carry-codes` (INV-SF-6) | Every new diagnostic carries a `DiagnosticCode` (C-LOG); γ2 additionally **retrofits** a code onto the existing code-less `RepresentationWithin` violation message. **Clear — net reduction in code-less diagnostics.** |
| `parse-is-value-faithful` (INV-SF-7) | No grammar change anywhere in the batch (G3 grammar gate N/A). **Clear.** |

---

## Gate summary

| Gate | Verdict |
|---|---|
| **G1** — consumer named | **PASS.** Every mechanism names a consumer: β → γ1/δ/θ; α → γ1; the loop → `reify check` + μ; `RefineRecord` → `eval_representation_within` → `push_constraint_result` → user stderr; ζ → `reify build`/GUI; η → `cmd_build`'s exit gate; θ → export path. Engine-integration sub-check: the loop plugs into `engine-integration-norm.md` **§3.2 realization-kind dispatch** — an existing seam, no new one. |
| **G2** — user-observable leaf | **PASS.** α and β are **intermediates** naming their downstream unlocks. The other ten each name a CLI-observable signal (exit code, stdout verdict, stderr diagnostic, compile gate). No leaf's only signal is a synthetic-input unit test. |
| **G3** — substrate verified | **PASS.** All 14 PRD §3 anchors re-verified at `0560ca6f0a` (drift table above); grammar gate **N/A** (no novel syntax). Two extent gaps found and resolved (F1, F2). |
| **G4** — seam ownership | **PASS.** §7's table has a named owner per row and no reciprocal-ownership pattern. 6085 owns the export plumbing (θ consumes it, upstream edge); determinacy-intrinsics shipped the assertion; per-purpose-tolerance is deliberately untouched (§4.8); 5976/5977/6058 own the CLI fixture prose (λ scoped away, no edge). `esc-6060-1` is **provenance only — not touched**. No fourth contested pair introduced. |
| **G5** — B+H | **PASS.** Contract §5 (C-BOUND/C-LOOP/C-LOG/C-SURFACE + C-KEEP/C-CAP/C-FLOOR/C-GLOBAL/C-ZERO) and boundary-test sketch §6 both present; integration-gate task **μ** names the §6 table as its signal. |
| **G6** — premise validity | **PASS.** Numeric floor bound (`1.19e-7·S`, runtime-enforced); termination proved without pinning `K`; γ1's end-to-end capability set is entirely on `main` or upstream; δ/ζ/η's negative assertions each bound to an **observed** absence or an **observed-present** rejection mechanism; θ's dependency is upstream. |
| **G7** — design invariants | **PASS**, no waiver. |
| **Manifest** | **0 FAIL.** 2 `PASS-with-amendment` (η, δ; plus the μ/BT-7 twin of η's), each written verbatim into the filed task. |
