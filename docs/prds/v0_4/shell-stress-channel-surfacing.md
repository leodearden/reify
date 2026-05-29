# Shell through-thickness stress channel surfacing (`ElasticResult.shell_channels`)

**Status:** active · **Milestone:** v0.4 · **Authored:** 2026-05-29 (via `/prd`)

## §0 — Why this PRD exists (supersession + cross-PRD context)

The v0.4 shell stack can compute through-thickness stress in the kernel
(`ShellElementStress {top, mid, bottom}`, `crates/reify-solver-elastic/src/shell_result.rs:79`)
and persists it at the runtime layer (`ElasticResult.shell_channels: Option<ShellChannels>`,
`crates/reify-eval/src/persistent_cache.rs:548`, landed by task **β / 3534**). But the
**DSL-visible** `ElasticResult` that a `.ri` design file sees
(`crates/reify-compiler/stdlib/solver_elastic.ri:305`) has **no way to reach top/bottom
stress**. A `ShellStress {top, mid, bottom}` struct exists at the stdlib level
(`solver_elastic.ri:362`) but is **orphaned** — declared by Shells T16 (task **3028**, marked
done) and never wired into `ElasticResult`. So `result.shell_channels.top`
(task 3594's wording) and `result.stress.top.von_mises` (parent shells PRD §8.2 wording) are
both unreachable today. This PRD closes that surfacing gap.

This was surfaced as a hard, independent blocker on **task 3594** (the v0.4 shell-solve
end-to-end leaf), escalation **esc-3594-10**: its RED test references a non-existent field.
3594 is being re-spec'd in the same rotation to a bare-MITC3-realistic accuracy signal
(sign + order-of-magnitude, per `crates/reify-solver-elastic/tests/shell_benchmarks.rs`'s
"smoke tests, NOT validated benchmarks" convention) and **gated on this PRD's task**, not on
MITC3+ accuracy (task 3392). This PRD is purely the **surfacing** concern; validated shell
accuracy remains the concern of curved-element MITC3+ (3392) + ANS membrane (4065).

### Reconciling two conflicting design intents (the deeper design call)

The repo carries two incompatible intents for how through-thickness stress reaches the DSL:

- **Intent X** — parent shells PRD `structural-analysis-shells.md` §8.2 + tasks **3583 / 3575**:
  make `stress` *itself* a `ShellStress`; bare `result.stress` aliases `result.stress.mid`.
- **Intent Y** — bridge PRD `shell-extract-engine-bridge.md` §3 + the **landed Rust runtime**
  + task β: a *separate, additive* `shell_channels` field; `stress` stays a flat mid-surface
  tensor field.

The landed Rust runtime already committed to **Intent Y** (`stress: Vec<f64>` flat +
`shell_channels: Option<ShellChannels>`). That makes Intent X's alias
`result.stress == result.stress.mid` **ill-formed** — there is no distinct `.mid` to alias to,
because under Intent Y *mid is `stress`*. This PRD adopts **Intent Y** (decision DR-1 below)
and replaces the broken alias with the well-formed identity
`result.shell_channels.mid == result.stress`. Tasks 3583 and 3575 carry Intent-X premises and
are re-spec'd to this API as part of the decompose batch (decision DR-4).

## §1 — Consumer + user-observable surface (G1)

**Mechanism introduced:** a DSL-visible `shell_channels : ShellStress` field on `ElasticResult`,
plus the Rust→`Value` mapping that populates it from the runtime
`ElasticResult.shell_channels: Option<ShellChannels>`.

**Named consumers** (this is the integration-seam catalogue — no orphan producer):

| Consumer | How it consumes |
|---|---|
| **Task 3594** (shell-solve e2e leaf, bridge δ) — *primary* | reads `result.shell_channels.top` and `von_mises(result.shell_channels.top)` for the cantilever sign/order-of-magnitude signal |
| **Task 3583** (stdlib alias) | re-spec'd: asserts `result.stress == result.shell_channels.mid` (replaces the ill-formed `result.stress.mid` alias) |
| **Task 3575** (`to_global` + envelope helpers) | re-spec'd: operates over the `ShellStress {top,mid,bottom}` shape this task wires in |
| **Task 3599** (bridge ι, thin-walled bracket e2e) | reads through-thickness channels for the `vonMises_top` colormap path |
| **Bridge task θ** (engine→GUI populator) | maps `shell_channels` → `MeshData.scalar_channels` `vonMises_top|mid|bottom` |
| v0.5 PRDs (`varying-thickness-shells`, `composite-laminated-shells`) | extend the same `shell_channels` surface (bridge §3 cross-ref) |

**Engine-seam plug-in (G1 engine sub-check):** this is **not a new seam**. The `shell_channels`
`Value` field is emitted at the existing ComputeNode-dispatch trampoline's `ElasticResult`
`StructureInstance` build site (`crates/reify-eval/src/compute_targets/elastic_static.rs:343`,
engine-integration-norm §3.4 / CN-contract §8 task η). This task adds the field + an `Undef`
baseline + the mapping helper; **task 3594/δ** calls the helper with real kernel data on the
shell-routing path.

**User-observable surface:** a `.ri` design file can now write
`result.shell_channels.top` / `.mid` / `.bottom` and `von_mises(result.shell_channels.top)`,
and it type-checks and evaluates — where before it was a "no field `shell_channels`" error.

## §2 — Sketch of approach

Additive, non-breaking. Three coordinated edits + boundary tests:

1. **stdlib `solver_elastic.ri`** — add `param shell_channels : ShellStress` to
   `ElasticResult` (wiring in the existing orphaned `ShellStress {top, mid, bottom}` struct).
   Reconcile the stale doc comments (the T16/T18–T20 "homogeneous for tet" / "always all three
   populated" language) to the landed reality: tet → `Undef`; `mid == stress`; `frame` stays
   the top-level field.
2. **Rust mapping helper** (`reify-eval`, near the trampoline) — a pure function
   `shell_channels_to_value(channels: &Option<ShellChannels>, mid_stress: &Value) -> Value`
   that builds a `ShellStress`-shaped `Value::StructureInstance` with `top`/`bottom` from
   `ShellChannels` and `mid` from the flat `stress` field, or returns `Value::Undef` when the
   channels are `None`.
3. **Trampoline emission** (`elastic_static.rs:343`) — add the `shell_channels` key to the
   `ElasticResult` `StructureInstance` `fields` map, `Undef` in the baseline (tet) slice.
   Task 3594/δ replaces that `Undef` with `shell_channels_to_value(Some(_), mid)` on the
   shell-routing path.

## §3 — The contract (H component — design-first per G5)

The DSL `ElasticResult` after this task:

```reify
structure def ShellStress {        // already exists at solver_elastic.ri:362 — wired in here
    param top : Field<Point3<Length>, Tensor<2, 3, Pressure>>
    param mid : Field<Point3<Length>, Tensor<2, 3, Pressure>>
    param bottom : Field<Point3<Length>, Tensor<2, 3, Pressure>>
}

structure def ElasticResult {
    param displacement : Field<Point3<Length>, Vector3<Length>>
    param stress : Field<Point3<Length>, Tensor<2, 3, Pressure>>  // UNCHANGED — flat mid layer
    param frame : Field<Point3<Length>, Matrix<3, 3, Real>>        // UNCHANGED — top-level
    param shell_channels : ShellStress                            // NEW — additive
    param max_von_mises : Pressure
    param converged : Bool
    param iterations : Int
    constraint iterations >= 0
    constraint max_von_mises >= 0
}
```

**Mapping table (Rust runtime → DSL `Value`):**

| Source (Rust) | DSL target | Tet (solid) | Shell |
|---|---|---|---|
| `ElasticResult.stress: Vec<f64>` | `ElasticResult.stress` (flat) | full Cauchy field | mid layer |
| `ShellChannels.top` | `shell_channels.top` | `Undef` | top fibre (z=+t/2) |
| `ShellChannels.bottom` | `shell_channels.bottom` | `Undef` | bottom fibre (z=−t/2) |
| `stress` (mid) | `shell_channels.mid` | `Undef` | == `stress` (identity) |
| `ShellChannels.frame` | `ElasticResult.frame` (top-level) | `Undef` | local→global rotation |
| `shell_channels: None` | whole `shell_channels` field | `Undef` | — |

**Invariants:**
- **I-1 (non-breaking):** `result.stress` is byte-identical to its pre-task value for every
  tet *and* shell solve. No existing `.stress` consumer changes.
- **I-2 (mid identity):** for a shell solve, `result.shell_channels.mid == result.stress`
  (both derive from the same flat `stress` Vec). This is the well-formed replacement for the
  ill-formed Intent-X alias `result.stress == result.stress.mid`.
- **I-3 (honest absence):** for a tet/solid solve, `result.shell_channels == Undef`. Through-
  thickness top/mid/bottom is **undefined** for a general solid (no mid-surface, no thickness
  axis, no linear-through-thickness law) — see DR-3. We do not fabricate look-alike channels.
- **I-4 (von_mises field-path):** `von_mises(result.shell_channels.top)` lowers through the
  existing `compute_von_mises` tensor-field path (`crates/reify-expr/src/analysis.rs:157`) to
  a scalar `Field<Point3, Pressure>`. No new accessor is introduced.

## §4 — Pre-conditions / substrate (G3 — verified, `grammar_confirmed=true`)

All substrate exists today (grep/parse-verified 2026-05-29):
- `ShellStress {top,mid,bottom}` struct — present, orphaned (`solver_elastic.ri:362`).
- Struct-typed `param` — grammatically valid; precedent `param material : ElasticMaterial`
  (`FEAMaterialInput`) and `param cases : Map<String, ElasticResult>` (`MultiCaseResult`) in
  the same file. `tree-sitter-reify parse` on the new `ElasticResult` + `von_mises(...)`
  fixture exits 0.
- `von_mises` — free-function builtin (`reify-stdlib/src/analysis.rs:14`) + tensor-field path
  `compute_von_mises` (`reify-expr/src/analysis.rs:157`, dispatched `reify-expr/src/lib.rs:349`).
  Reify has **no** method-call syntax (GR-040) — the free-function form is the only form.
- Rust `ShellChannels {top,bottom,frame}` + `ElasticResult.shell_channels: Option<_>` — landed
  (β/3534, `persistent_cache.rs:527,548`).
- `Value::StructureInstance` / `StructureInstanceData` — the trampoline already builds
  `ElasticResult` this way (`elastic_static.rs:357`).
- `.ri` struct ctors accept any value (SIR-α / task 3540, landed) — so a fixture can construct
  a `ShellStress(...)` and an `ElasticResult(shell_channels: ...)` to exercise the read path
  at runtime without a full solve.

## §5 — Resolved design decisions

- **DR-1 — Adopt Intent Y (additive `shell_channels`), not Intent X (restructure `stress`).**
  Forced by: the landed Rust runtime (`stress` flat + `shell_channels` Option); non-breaking
  requirement (Intent X breaks every `.stress` consumer — `fea_multi_case` envelopes,
  `solver_buckling`, `error_estimator`, `result.rs`); and the bridge PRD §3 being the active
  source-of-truth for engine-side integration. Candidate "re-point to flat `stress`" rejected
  on physics: for a bending cantilever the mid/neutral-plane stress ≈ 0, so the clamped-edge
  bending signal must read `top`/`bottom`, not mid.
- **DR-2 — Reuse `ShellStress {top, mid, bottom}` for the field type.** Wires in the orphaned
  T16 struct; `shell_channels.mid` redundantly equals `stress`, giving the well-formed mid
  identity (I-2) and a symmetric three-layer read. `frame` stays the top-level `ElasticResult`
  field (its existing DSL home; Rust bundles it in `ShellChannels` only for serialization).
- **DR-3 — tet/solid → `shell_channels = Undef`; uniformity via `stress`/`von_mises`, not fake
  channels.** Top/mid/bottom is *undefined* (not merely imprecise) for a general solid: a
  sphere/torus/foam/void-enclosing body has no mid-surface, no thickness axis, and a full 3-D
  stress gradient — there is nothing true to compute eagerly or lazily. Homogeneous fill
  (`top==mid==bottom==stress`) makes the affirmative *false* claim "zero through-thickness
  bending gradient." The landed β/3534 runtime already overruled parent PRD §8.2's homogeneous
  text in favour of `None` for this reason. The genuinely-uniform, genuinely-safe cross-element
  surface is the already-uniform `stress` (full Cauchy field / mid layer) + `von_mises(stress)`
  + `max_von_mises` (the uniform "worst stress anywhere" scalar). A uniform `surface_stress`
  accessor was considered and **rejected for v0.4** (see §6): its uniformity is type-level only,
  not semantic — a shell-assuming consumer (two-sided fibres, membrane/bending split, mid-
  surface registration) handed a tet result through it gets a plausible-looking but
  assumption-violating field, i.e. a *silent* correctness bug strictly worse than `Undef`'s
  *loud* irregularity.
- **DR-4 — Re-spec sibling tasks 3583 + 3575 to this API in the decompose batch.** Both carry
  Intent-X premises (`result.stress.mid`; structured `stress`). 3583 → assert
  `result.stress == result.shell_channels.mid`; 3575 → operate over `ShellStress {top,mid,bottom}`.
  Both gain a real dependency edge on this PRD's task.

## §6 — Out of scope

- **Validated / tight shell accuracy** — bare-MITC3 v0.4 ships honest order-of-magnitude bands;
  tightening gates on curved-element MITC3+ (task 3392) + ANS membrane (task 4065). 3594's
  signal is re-spec'd to sign + order-of-magnitude and must **not** gate on 3392.
- **`surface_stress(result)` uniform accessor** — deferred. If a real consumer ever needs a
  uniform "surface yield stress," add it then, documented as "opaque worst-surface-σ for yield
  checks only — **not** a bending decomposition" (shell → top/bottom fibre; solid → boundary-σ).
  Not needed by 3594 or any v0.4 leaf.
- **`to_global(stress, frame)` global-frame transform** — owned by task 3575 (re-spec'd here to
  the `ShellStress` shape but implemented there).
- **Per-element → per-vertex nodal recovery** — bridge §11 OQ-1, owned by GUI populator task θ.
- **Cache/serialization format** — already landed (β/3534, `ELASTIC_RESULT_FORMAT_VERSION` bump).

## §7 — Cross-PRD relationship + seam ownership (G4)

| Seam | Owner | Note |
|---|---|---|
| DSL `ElasticResult.shell_channels` schema + Rust→`Value` mapping | **this PRD** | the surfacing layer |
| Populating `shell_channels: Some(_)` for shell-classified bodies (end-to-end) | bridge δ / **task 3594** | depends on this PRD's task |
| `result.stress == shell_channels.mid` alias assertion | **task 3583** (re-spec'd) | depends on this PRD's task |
| `to_global` + envelope helpers over `ShellStress` | **task 3575** (re-spec'd) | depends on this PRD's task |
| `shell_channels` → `MeshData.scalar_channels` (`vonMises_*`) | bridge θ | downstream; this PRD provides the source field |
| Validated accuracy (MITC3+ / ANS membrane) | tasks 3392 / 4065 | explicitly **not** a gate on 3594 |

No new contested-ownership pair is introduced. This task is additive to the
`structural-analysis-shells ↔ shell-extract-engine-bridge` mild-contradiction seam already
noted in the audit breadcrumb map — and *resolves* it for the stress-surfacing slice by
adopting the bridge PRD's Intent Y over the parent PRD's Intent X.

## §8 — Boundary-test sketch (two-way, H component per G5)

Facing **DSL-down**:
- `.ri` example (CI): construct a shell-shaped `ElasticResult` via struct ctor with a hand/
  kernel-built `ShellStress`; assert `result.shell_channels.top`, `.mid`, `.bottom` are finite
  `Field`s; assert `von_mises(result.shell_channels.top)` evaluates to a finite scalar field;
  assert `result.shell_channels.mid == result.stress` (I-2).

Facing **Rust-up**:
- Unit/boundary test on `shell_channels_to_value`: feed a `Some(ShellChannels{top,bottom,frame})`
  built from **real `shell_element_stress` kernel output** (not hand-typed numbers — closes the
  "synthetic input" G2 gap) + a mid `stress` `Value`; assert the produced `Value` is a
  `ShellStress` `StructureInstance` whose `top`/`bottom`/`mid` members carry the mapped tensor
  fields. Feed `None`; assert `Value::Undef`.
- Trampoline test: a tet solve's `ElasticResult` `StructureInstance` has `shell_channels` ==
  `Undef` (I-3), and `stress` is byte-identical to the pre-task value (I-1).

## §9 — Decomposition plan

One task (additive, single coherent unit; small blast radius). Filed `planning_mode=True`.

- **Task S1 — Surface `ElasticResult.shell_channels : ShellStress` on the DSL + Rust→Value
  mapping.**
  - *Observable signal:* (a) a `.ri` CI example reads `result.shell_channels.top/.mid/.bottom`
    and `von_mises(result.shell_channels.top)` and they type-check + evaluate to finite fields,
    with `result.shell_channels.mid == result.stress`; (b) a Rust boundary test maps real
    `shell_element_stress` kernel output through `shell_channels_to_value` into a `ShellStress`
    `StructureInstance` and maps `None`→`Undef`; (c) a tet solve emits `shell_channels == Undef`
    and `stress` byte-identical to pre-task (I-1).
  - *Consumer:* task 3594 (primary) + 3583 / 3575 / 3599 / bridge θ.
  - *Grammar:* confirmed (tree-sitter parse exit 0; precedent `param material : ElasticMaterial`).
  - *Crates:* `reify-compiler` (stdlib `solver_elastic.ri`), `reify-eval`
    (`compute_targets/elastic_static.rs` + mapping helper).
  - *Prereqs:* SIR-α (3540, landed), β/3534 (landed) — no new dep edges needed (both on main).

**Downstream wiring done at decompose time:** `add_dependency(3594 → S1)`,
`add_dependency(3583 → S1)`, `add_dependency(3575 → S1)`; 3583/3575 descriptions re-spec'd to
this API (DR-4); 3594 re-spec'd per §0 (separate reconciliation, owned by this session's
hand-back; escalation esc-3594-10 resolution owned by the escalation-watcher).

## §10 — Open (tactical) questions

- **OQ-1:** Should `shell_channels_to_value` live in `reify-eval` (next to the trampoline) or
  `reify-solver-elastic` (next to `ShellChannels`)? Tactical; either keeps the mapping in one
  place. Suggested: `reify-eval`, since it owns the `Value` shape and the trampoline call site.
- **OQ-2:** Does the baseline tet trampoline (`elastic_static.rs`) eventually populate `stress`
  (currently `Undef` in that slice) — and if so does the mid-identity test need a real
  `stress` field rather than `Undef`? Tactical; the boundary test constructs its own `stress`
  `Value`, so it does not block. Real-solve population is 3594/δ's concern.
