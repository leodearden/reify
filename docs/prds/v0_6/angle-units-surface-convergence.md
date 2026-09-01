# ANGLE convention convergence + units-surface round-trip — PRD

**Milestone:** v0_6 · **Status:** active (authored 2026-07-28, HEAD `dc83d4fd60`) ·
**Program:** units-gating (PRD 3 of 5) · **Approach:** B + H (contract + two-way boundary tests)

Sibling PRDs (same program, slugs fixed by the program brief):
`units-length-gate-completion` (1) — **landed `54afdee50b`**, `check-diagnostic-truthfulness` (2),
`dimensioned-construction-strictness` (4), `dimension-checked-readers` (5).

**Canonical evidence:** `docs/notes/units-gating-gap-research-2026-07-28.md` ("Angle class",
route census, RED-TEAM findings, ratified decisions 6 & 7). Every anchor below was
**re-verified against `dc83d4fd60`** at authoring time; drift from the research doc is called
out inline with `[drift]`.

---

## §1 — Goal

Three mutually contradictory bare-angle conventions ship in the same binary today. Converge
them on one — **reject bare numbers at angle-semantic positions** — and, separately, close the
units *surface* loop so that what Reify prints as a unit is something Reify can read back.

After this PRD:

- `rotate(b, 0,0,1, 45)` fails with `rotate: angle argument expects Angle, got Int; pass a
  dimensioned angle such as \`45deg\` or \`1.5rad\``, instead of silently meaning 45 **radians**
  (≈ 2578°). Same for `rotate_around`, `revolve`, `arc` start/end, `draft`, `circular_pattern`.
- `circular_pattern(..., 360)` stops silently meaning 360° — the one place bare numbers meant
  degrees. One language, one rule.
- A torque is writable as `5Nm` (or `5N*m/rad`), `Torque` is a first-class named dimension, and
  writing `5N*m` (an **Energy**) where a torque is expected teaches the difference instead of
  printing two raw exponent vectors.
- Every unit string Reify prints can be pasted back into a `.ri` file: `7850 kg·m^-3` parses
  (the middle dot becomes a unit-multiply operator), and the curated ladder labels stop using
  characters no lexer accepts (`kg/m³` → `kg/m^3`, `mm²` → `mm^2`), locked by a round-trip
  property test.

## §2 — Consumers (G1)

| Mechanism | Consumer | Surface |
|---|---|---|
| `angle_spec()` + angle eval gates | `.ri` authors; the 4 selector `tol` args (existing consumer, gains the migration hint) | `reify eval` diagnostics + exit 1 |
| ANGLE `CheckableArg` compile slots | `reify check` (compile slots are the only layer with teeth at check today — RED-TEAM 3, re-verified §3.4); PRD 2 widens this | `reify check` exit 1 |
| `Torque` named dimension | `crates/reify-compiler/stdlib/ports_mechanical.ri` — its `pub type Torque` alias existed **solely** as a workaround, self-documented at the time in that file's header; retiring it was the consumer proof. **DONE — η (task 5785, merge `61300d09`) deleted the alias**; `:17-28` is now a pointer comment, and `Torque` is a built-in `NAMED_DIMENSIONS` entry | stdlib compiles; `Torque` resolves without the alias ✓ |
| `Nm` unit symbol | `.ri` authors writing torque; `stdlib/flexures.ri:155` (`0N*m/rad`) becomes writable as `0Nm` | `reify eval` resolves `5Nm` |
| Energy↔Torque teaching diagnostic | `.ri` authors — fires **today's live** param-mismatch path (§3.5) | `reify check`/`eval` diagnostic text |
| U+00B7 unit-multiply | `.ri` authors copy-pasting Reify's own output and external SI text; PRD 1's GUI param editor (`parse_value_string`) inherits it — **noted, not owned** | `tree-sitter parse` + `reify eval` |
| ASCII curated labels | the GUI unit picker (`gui/src-tauri/src/main.rs:699-700 get_unit_ladders`); `@display("…")` label validation (`annotations/schema.rs:334`) | GUI dropdown; `@display` accepts what the dropdown shows |
| Round-trip property test | this PRD's own integration gate; the whole units surface | gate-resident cargo test |

No mechanism here is an in-engine seam requiring `engine-integration-norm.md §3.N` placement —
the eval gates sit on the existing `compile_geometry_op` path (§3.1 op-execute, already wired).

## §3 — Premise correction — what is true at `dc83d4fd60` (empirically verified)

Everything in this section was measured this session, not inherited.

### 3.1 The three conventions (all confirmed)

| Convention | Where | Anchor |
|---|---|---|
| **radians, silent** | `rotate` angle, `rotate_around` angle, `revolve` angle, `arc` start_angle/end_angle | `crates/reify-eval/src/geometry_ops.rs:2251, 2312, 2975, 3327, 3328` — each a `f64_arg("…")` closure over `eval_named_arg_f64` (`:202`) |
| **radians, silent (R7)** | `draft` angle — raw `Value` IR field passed to the kernel | IR `crates/reify-ir/src/geometry.rs:944`; eval read `geometry_ops.rs:1979` (`eval_arg`, not `_f64`); kernel `crates/reify-kernel-occt/src/lib.rs:2827` `extract_f64` |
| **degrees, warned + converted** | `circular_pattern` angle only | `resolve_bare_angle` at `geometry_ops.rs:880` **[drift** — `docs/prds/v0_6/type-hygiene.md:106` still cites `:418-439`**]**; 2 call sites, `:2585` (value-axis form) and `:2626` (scalar-axis form) |
| **rejected (ANGLE Scalar required)** | `faces_by_normal`, `edges_parallel_to`, `faces_perpendicular_to`, `edges_perpendicular_to` — the `tol` arg | eval `geometry_ops.rs:5299, 5313, 5345, 5359` via `resolve_angle_scalar_arg` (`:8786`); **plus** a compile slot |

Probe (release binary, fresh at `dc83d4fd60`):

```
rotate(b, 0.0, 0.0, 1.0, 45)                      → builds; ZERO diagnostics; exit 0
circular_pattern(b, 0mm,0mm,0mm, 0,0,1, 4, 360)   → warning: circular_pattern: bare numeric
    angle `360` interpreted as 360°; use `360deg` or `6.283185rad` for explicit units ; exit 0
faces_by_normal(b, 0.0, 0.0, 1.0, 0.01)           → error: faces_by_normal: tol argument
    expects Angle, got Real ; exit 1
```

The `circular_pattern` warning carries **no `DiagnosticCode`** (`Diagnostic::warning`,
`reify-core/src/diagnostics.rs:3885` sets `code: None`) — an INV-SF-6 violation this PRD must
not replicate.

### 3.2 `arg_acceptance` is dimension-generic and FROZEN; there is no `angle_spec`

`crates/reify-eval/src/arg_acceptance.rs` (219 lines): `ArgSpec` :28, `Acceptance` :40,
`ArgRejection` :52, `ArgRejection::message` :69, `density_spec` :86, `length_spec` :103,
`accept_arg` :117. Wording template:

```
{builtin}: {arg_name} argument expects {type_name}, got {got}[; {migration_hint}]
```

There is **no `angle_spec()`**. The ANGLE spec is built inline by `resolve_scalar_dim_arg`
(`geometry_ops.rs:8755`, call site `:8767-8771`) with `migration_hint: None` — which is why the
four shipped angle rejections give the author no repair instruction while the length ones do.
PRD 1 §7 and its **D9(i)** independently reach the same finding and assign the fix here: a
`migration_hint`-less `ArgSpec` is a live violation of the spec §14.5 breaking-change obligation.
Adding `angle_spec()` is additive (core stays frozen, per the program's G4 freeze) **and** repairs
those four messages.

### 3.3 `builtin_signatures.rs` already has an ANGLE slot **[drift]**

The program brief assigns me "ANGLE slots in `builtin_signatures.rs`" as if none existed. One
does: `crates/reify-compiler/src/builtin_signatures.rs:168-180` maps the four directional
selectors' `tol` (index 2) to `DimensionVector::ANGLE` / `type_name: "Angle"`, with a test
helper `angle_slot(...)` at `:490`. `CheckableArg` is at `:80`, `builtin_arg_slots` at `:145`,
consumer `check_builtin_arg_types` calls it at `:334`. **No geometry *producer*** (`rotate`,
`rotate_around`, `revolve`, `arc`, `draft`, `circular_pattern`) has a slot. So the work is
*extending an existing arm pattern*, not inventing one — smaller and lower-risk than briefed.

### 3.4 Compile slots, not eval gates, are what `reify check` sees

Verified by differential probe:

| Fixture | `reify eval` | `reify check` |
|---|---|---|
| `faces_by_normal(..., 0.01)` — eval gate **and** compile slot | `error:` exit 1 | `error:` exit 1 |
| `mirror(b, 0, 0, 0, …)` — eval gate **only** | `warning:` + `error: failed to compile geometry operation…` exit 1 | `All constraints satisfied.` **exit 0** |

This corroborates RED-TEAM finding 3 first-hand and is why cluster A carries a compile-slot leaf
rather than treating slots as optional polish. It is also the reason every leaf signal in this
PRD is phrased against **`reify eval`** — `reify check` semantics belong to PRD 2 (G4).

### 3.5 `Torque` already resolved — as a stdlib alias, not a dimension (pre-η analysis)

> **Superseded by η (task 5785, merge `61300d09`) — retained as the analysis that MOTIVATED this
> PRD's `Torque` leaf.** This section describes the tree as it stood BEFORE η. The alias is now
> deleted; `Torque` is a built-in `NAMED_DIMENSIONS` entry — `(DimensionVector::TORQUE, "Torque")`
> at `crates/reify-core/src/dimension.rs:627` — so a `param t : Torque` slot resolves via the
> registry with no module import. See §C for the change as planned.

`crates/reify-compiler/stdlib/ports_mechanical.ri:29` **declared** (pre-η)
`pub type Torque = Force * Length / Angle`, with a header comment stating the reason:
*"Torque is absent from NAMED_DIMENSIONS … Binary dimensional-operation expressions are admitted
ONLY on type-alias RHS, not in `param x : <type>` position"*. (That comment cited
`dimension.rs:443-494`. Both cites are now dead: η deleted the alias and replaced `:17-28` with a
pointer comment, and the alias-rationale item was dropped from the header's *Deviations from §11
spec* block, exactly as §C3 planned. NAMED_DIMENSIONS itself has since moved again — re-measured
2026-07-31 at `dimension.rs:565-650`, 52 rows, superseding the earlier `:514-595` **[drift]**
note.)

Consequences, all probe-verified:

- `param t : Torque = 5N*m/rad` type-checks today; `param bad : Torque = 5N*m` errors.
- The vector `kg·m²·s⁻²·rad⁻¹` **already has a NAMED_DIMENSIONS entry** — as
  `RotationalStiffness` (`crates/reify-core/src/dimension.rs:572`, const at `:275`). Torque and
  RotationalStiffness are the *same* `DimensionVector` in Reify's model. `canonical_name()` is
  first-match-wins, so entry ORDER is load-bearing (§6 D4).
- The live mismatch diagnostic today is:
  `error: parameter 'bad' declared \`Scalar[m^2·kg·s^-2·rad^-1]\` but its initializer evaluates
  to \`Scalar[m^2·kg·s^-2]\`; declared type and initializer dimension must agree` — it names
  neither dimension **and** prints two strings that cannot be parsed back. This single message is
  the live consumer for both the teaching diagnostic (cluster B) and the round-trip work
  (cluster C).

### 3.6 There is no `Nm`; `nm` is generated; lookup is case-sensitive

`Nm` does not exist in any `.ri`, Rust table, or generator (`si_units.rs` only concatenates
`<prefix><name>`; `N`'s prefix set is `Only(["k","M","G"])` at `si_units.rs:184`). `nm` is
**generated** (`si_units.rs:102-106` × prefix `("n", 1e-9)` `:22`). Lookup is an exact `HashMap`
key match — `UnitRegistry::lookup` `crates/reify-compiler/src/units.rs:1533`, plus the 14-arm
bootstrap fallback `crates/reify-core/src/units.rs:24`; zero case folding anywhere. Probe:

```
5nm → 0.000000005 m     5Mm → 5000000 m     5NM / 5MM / 5Kg / 5RAD → error: unknown unit
5Nm → error: unknown unit: Nm          (but tree-sitter parse exits 0 — see §3.8)
```

So `Nm` is free, and case-distinctness from `nm` is a property of the existing lookup, to be
*pinned by a test*, not built.

`unit` declarations take a **named dimension**, never a compound expression (`units.ri:48`
`pub unit lbf : Force = 4.4482216152605`; `:53` `pub unit psi : Pressure = …`). So `Nm` requires
`Torque` to be a *named dimension*, not merely a stdlib alias — which is exactly the deviation
that `ports_mechanical.ri`'s header recorded pre-η, and precisely what motivated making `Torque` a
named dimension. η (task 5785, merge `61300d09`) landed that, so `Nm` no longer needs the alias.

### 3.7 Four independent unit-display surfaces; none re-parses

| # | Surface | Anchor | Emits | Parses back? |
|---|---|---|---|---|
| S1 | `impl Display for DimensionVector` | `reify-core/src/dimension.rs:597-628`, `parts.join("·")` at `:626` | `kg·m^-3`, `m^2·kg·s^-2·rad^-1` | **no** — `·` |
| S2 | `DimensionVector::to_display_units` | `reify-core/src/dimension.rs:458-474` | `mm`, `deg`, `mm²`, `mm³`, `USD` | partly — `²`/`³` |
| S3 | `dimension_unit_label` | `reify-ir/src/value.rs:3068-3090` (**private `fn`**) **[drift** — brief said "~:10676"; that is its *test* module**]** | `m`, `rad`, `m²`, `m³`, `USD` | partly |
| S4 | curated ladders `unit_ladders()` | `reify-core/src/display_units.rs:379`; labels at `:414,422,427,432,440,448,453,463,540,548,553` | `mm²`, `cm³`, `kg/m³`, `g/cm³`, `L` | partly |

`docs/prds/display-unit-preference.md` §2(b)/(c) already documented this fan-out ("three
different Rust value-formatting functions … and a fourth, independent source of curated names") —
this PRD is the one that makes the four agree on a parseable alphabet.

**Extra defect found while measuring:** the Volume ladder's `L` rung
(`display_units.rs:458`, `si_scale: 1e-3`) names a unit **that does not exist**: `1L` →
`error: unknown unit: L`. `units.ri:56-60` explicitly notes no SI volume units are declared. So
the round-trip test is not merely a guard — it fails on live main today for a reason that has
nothing to do with character sets.

### 3.8 Grammar reality (G3) — measured with `tree-sitter parse --quiet`

Four fixtures are committed alongside this PRD, each carrying its measured result and its
producer leaf in its own header:

| Fixture | `tree-sitter parse --quiet` | `grammar_confirmed` | Producer |
|---|---|---|---|
| `tests/prd-gate/fixtures/unit_middot_mul.ri` | **exit 1** (ERROR node per `·`) | **false** | leaf **κ** |
| `tests/prd-gate/fixtures/unit_nm_torque_immediate.ri` | exit 0 | true | leaf θ (unit-table entry only) |
| `tests/prd-gate/fixtures/unit_curated_labels_ascii.ri` | exit 0 | true | leaf λ (label strings only) |
| `tests/prd-gate/fixtures/bare_angle_silently_accepted.ri` | exit 0 | true | leaves γ/ε (G6 negative-assertion baseline) |

Full probe matrix at `dc83d4fd60`, generated grammar present, CWD `tree-sitter-reify/`:

| Fragment | Exit | Verdict |
|---|---|---|
| `5Nm` | **0** | parses via `immediate_identifier` (`grammar.js:1535`). `grammar_confirmed: true` for cluster B — only the unit-table entry is new. |
| `5N*m`, `5N*m/rad`, `7850kg/m^3`, `9.81m/s^2` | 0 | baseline, unchanged |
| `3m^-1`, `3kg*m^-3` | 0 | negative exponents parse (`signed_integer: token.immediate(/-?\d+/)`, `grammar.js:1532`) |
| `3m^+2` | **1** | **[drift]** the brief's "signed exponents already parse" is true only for `-`; the regex has no `+`. Harmless (no surface emits `+`) but must not be asserted unqualified. |
| `5N·m`, `5N·m/rad` | **1** | **NOVEL SYNTAX — fails today BY DESIGN.** ERROR node at the `·`. |
| `7850kg/m³` | **1** | superscripts are outside the unit alphabet |
| `7850kg·m^-3`, `9.81m·s^-2`, `5m^2·kg·s^-2` (S1's own output) | **1** | Reify cannot read its own printed units |
| `1mm^2 1cm^2 1m^2 1mm^3 1cm^3 1m^3 1kg/m^3 1g/cm^3 1USD 1deg 1Pa 1N 1J 1W` (the ASCII normalization target) | **0** | cluster C's target alphabet is already in-grammar — λ needs no grammar work |

Round-trip proof that only `·` blocks S1: replacing `·` with `*` in S1's own output re-parses and
is dimensionally identical — `5N*m`, `5m^2*kg*s^-2` and their difference all evaluate to
`m^2·kg·s^-2`, difference `0`.

`_unit_mul_op` is **not a grammar rule** — it is an *external token* (`grammar.js:53-56`,
referenced at `:1511`), scanned in `tree-sitter-reify/src/scanner.c:244-258`
(`if (valid_symbols[UNIT_MUL_OP] && c == '*') { … if (is_unit_start(lookahead)) …}`), with
`is_unit_start` at `:101` matching `[A-Za-z_(]`. U+00B7 appears **nowhere** in `grammar.js` or
`scanner.c`. The change is therefore a scanner-local widening of one character class —
no UTF-8-aware decoding is required, because `lexer->lookahead` is an `int32_t` carrying an
already-decoded codepoint (see §5 C2's CORRECTION 2026-08-29) — **plus the `ts_parser.rs`
lowering update required by §5 C2's CORRECTION 2026-08-19, without which the declaration is
silently dropped**: a materially smaller diff than "a grammar change" implies, but still a
parser-seam change (→ G5 B+H).

### 3.9 Migration blast radius (MEASURED at authoring; re-measure at decompose per G6)

**`.ri` corpus — exactly ONE bare-angle call site**, confirming the research doc. Full
enumeration of all `revolve|rotate|rotate_around|arc|draft|circular_pattern` call sites across
`examples/`, `designs/`, `prj/`, `crates/**/fixtures`, `tests/`: 24 sites, 23 of which already
write `deg`. The exception is `examples/geometric_relations/feature_datum_axis.ri:24`
(bare `6.283185307179586` as `revolve`'s 9th arg). **Zero** bare-angle `circular_pattern` sites.

**Torque literals:** the research doc's "corpus has zero `N*m` literals" is **[drift]** — there
is one: `crates/reify-compiler/stdlib/flexures.ri:155`
`param effective_stiffness : RotationalStiffness = 0N*m/rad`. It is already dimensioned, so it
needs no migration; it is a *consumer* datum for `Nm`, not a migration site.

**Rust-side bare-angle fixtures (all behavior-preserving to migrate — see D2):**

| Site | Count | Anchors |
|---|---|---|
| `Draft { angle: Value::Real(..) }` | 8 | `reify-ir/src/geometry.rs:9651, 11025, 11043`; `reify-kernel-occt/src/lib.rs:8666, 8747, 8756, 8805, 9221` |
| `angle: Value::Real` / `Value::real(` elsewhere | 19 total (superset of the above) | repo-wide grep |
| Tests that **invert** (assert today's behaviour) | 3 | `reify-eval/tests/rotate_e2e.rs:216-235` `rotate_with_bare_radian_literal_lands_in_kernel` (its doc cites `geometry_ops.rs:437` — **[drift]**, now `:2251`); `reify-eval/tests/circular_pattern_angle.rs:47` `circular_pattern_bare_360_emits_deprecation_warning`; `:86` `circular_pattern_360deg_no_deprecation_warning` (this one *survives* — the explicit-unit path stays warning-free) |
| Curated-label test expectations to update | ~14 | `reify-core/src/display_units.rs:722, 727, 1025, 1169, 1221, 1350, 1385, 1390, 1397, 1406`; `reify-ir/src/value.rs:10684, 10699, 10825`. `dimension.rs:912, 931` (`·` goldens) **stay** — S1 keeps the middle dot. |

**Re-measured inventory — discharges this section's own "re-measure at decompose per G6"
instruction.** `docs/notes/angle-literal-migration-ledger.md` is the re-measurement of the call
sites tabulated above, published by task **5777** (leaf α). It is stamped **2026-08-26 at
`181d1ec24c`** — a `main` merge commit, and therefore permanently reachable — with all four of its
measured sections sharing that one stamp. It routes its buckets to this PRD's migration consumers:
**γ (5779)** the rotation/sweep angle builtins (`revolve` / `rotate` / `rotate_around` / `arc` —
all four, per the ledger's own §2.1 split), **δ (5780)** `draft`, **ε (5781)** `circular_pattern`,
each migrating its own subset per contract C6. The counts in the table above are the
**authoring-time snapshot** and have since moved; for sizing γ/δ/ε the ledger, not this table, is
the live source of truth. Its numbers are deliberately **not** mirrored here — the ledger carries
only measured data this PRD does not, and this PRD keeps the normative contract table, so a
pointer beats a second copy to keep in sync.

**`@display("…")` corpus uses: 0** (grep, whole repo). So normalizing S4's labels breaks no
committed source, and the `@display` label validator (`annotations/schema.rs:334` + the
dimension-phase rung check from task 5233) simply starts matching ASCII rungs.

**`RayleighDamping.alpha`:** `param alpha : Frequency`, declared at
`crates/reify-compiler/stdlib/modal_analysis.ri:161`. `crates/reify-eval/src/modal_ops.rs:1126`
builds `ω = 2π·f` and feeds it to `rayleigh_damping_ratio(alpha, beta, omega)` at `:1127`
— the declared `Frequency` is consumed on the rad/s scale, so a caller writing `alpha: 1.0Hz`
— reading Hz as cycles/s, which is what Hz means — gets it consumed as `1 rad/s`: a `2π`
error in the mass-proportional damping term. **Latent only:** `alpha` is exactly `0.0Hz` at all
five *migrated* corpus ctor sites —
`examples/modal/printer_gantry_modes.ri:136`, `examples/modal/transient_step_response.ri:79`,
`examples/trajectory/printer_print_envelope.ri:97`,
`tests/prd-gate/fixtures/r3b_displacement_at_selector_grammar.ri:49`,
`crates/reify-eval-fea-tests/tests/r3b_modal_selector_displacement.rs:489`; the two remaining
*un-migrated* ctor sites also pass a bare `alpha: 0.0` —
`crates/reify-eval/tests/harness_mechanism/mechanism_modal_damping_e2e.rs:72` and
`crates/reify-eval-fea-tests/tests/r3b_modal_selector_displacement.rs:633` — so no wrong answer
is reachable today. The reasoning —
including why `alpha` is typed `Frequency` rather than `AngularVelocity` — lives in
`modal_analysis.ri`'s ANGULAR-RATE TRAP comment (:131-150), which already names this PRD as the
surface's owner — not restated here. Pinned test-side by
`modal_ops::tests::trampoline_shapes_modal_result_with_rayleigh_damping` (`modal_ops.rs:4617`),
which recomputes `ω = 2π·f` from the emitted `Mode.frequency` and compares `damping_ratio`
against it at `1e-12` — the arm that actually observes the producer's scale.
`modal_ops::tests::rayleigh_damping_ratio_alpha_and_beta_terms_scale_oppositely_in_omega`
(`:4246`) pins the algebraic symmetry only and explicitly does not observe the producer's
scale. **Disposition:** already dimensioned, so it needs no migration; it is a *consumer* datum
for `Frequency`, not a migration site — but honestly, §5's contract (C1-C4) does not today
deliver the cycles/s-vs-s⁻¹ separation this site was handed off for; that gap is tracked as
§11 item 6 for a ruling at decompose, not something C1-C4 already covers. Stamped **2026-09-01
at `da22522934`** (a `main` merge commit).

### 3.10 Prior ruling this PRD reverses

Task **1763** (`done`, 2026-04): *"circular_pattern angle should accept degrees (CAD convention)
or convert internally"* — that ruling produced `resolve_bare_angle`. This PRD **reverses it**:
the degrees convention loses, rejection wins, and the reason is that the ruling was made
per-builtin without the whole-surface view (three conventions in one binary). Per
`preferences_supersession_same_prd` / the silent-reversal hazard, leaf **ε** must record the
reversal in its own commit message and in the `resolve_bare_angle` removal site, so a future
reader does not re-derive 1763's argument from scratch.

## §4 — Sketch of approach

Three clusters, one shared spine (`arg_acceptance`), plus guards and docs.

**A — bare-angle rejection.** Add `angle_spec()` beside `length_spec()`. Route the five
`eval_named_arg_f64` angle reads and the one R7 raw-`Value` read (`draft`) through it; delete
`resolve_bare_angle` and route `circular_pattern` the same way. Add ANGLE `CheckableArg` slots
for the statically-visible angle positions by extending `builtin_signatures.rs`'s existing angle
arm. Corpus + kernel-test fixtures are dimensioned **first** (behaviour-preserving), so the
workspace is green at every step — the landing shape of
`docs/prds/v0_6/real-dimensionless-unification.md` decision 2.

**B — torque.** Promote `Torque` to a first-class named dimension in `reify-core` (const +
NAMED_DIMENSIONS entry), retire `ports_mechanical.ri`'s alias workaround, declare
`pub unit Nm : Torque = 1` in `stdlib/units.ri`, and special-case the Energy↔Torque pair in the
dimension-mismatch diagnostic — both directions.

**C — round-trip.** Widen the external scanner's unit-multiply class to accept U+00B7; normalize
every curated label on S2/S3/S4 to the ASCII `^`-exponent alphabet (and declare the missing `L`);
lock both with one gate-resident property test asserting *every* label emitted by *any* of the
four surfaces re-parses to the same `DimensionVector`.

**D — guards + docs.** Extend PRD 1's closure-guard allowlist with the angle positions; carry all
four docs-truth leaves.

## §5 — Contract (G5, approach H)

### C1 — Angle acceptance

```rust
// crates/reify-eval/src/arg_acceptance.rs  (additive; core semantics FROZEN)
pub fn angle_spec() -> ArgSpec {
    ArgSpec {
        type_name: "Angle",
        dimension: reify_core::DimensionVector::ANGLE,
        migration_hint: Some("pass a dimensioned angle such as `45deg` or `1.5rad`"),
    }
}
```

**Invariants.**

1. **Single wording.** Every angle rejection — eval or compile, producer or selector — reads
   `"{builtin}: {arg_name} argument expects Angle, got {got}; pass a dimensioned angle such as
   \`45deg\` or \`1.5rad\`"`. `angle_spec()` is the only construction site; no inline `ArgSpec`
   with `dimension: ANGLE` may remain in `geometry_ops.rs`.
2. **Strict `DimensionVector` equality.** Bare `Real`/`Int` (including `0`), wrong-dimension
   `Scalar`, and non-`Scalar` values all reject. No degrees default, no radians default, no
   warn-and-convert. `Value::Undef` stays a quiet `Acceptance::Undefined` at the *value* layer,
   but every angle chokepoint routes it into the existing distinct unresolved-argument
   diagnostic (`geometry_ops.rs:344-347`) rather than silently continuing — **adopting PRD 1's
   D10 verbatim**, so the two PRDs' chokepoints behave identically on `Undef`.
3. **Severity, exit, and code — inherited, not invented.** Eval-layer rejection produces a
   `Severity::Error`-reaching outcome: `reify eval` exits **1**. Every diagnostic introduced or
   modified by this PRD carries the **same `DiagnosticCode` minted by PRD 1's task β** (its D9(ii)
   / open question 1) — this PRD mints **no** parallel code and does not fork the spelling. That
   includes replacing the code-less `circular_pattern` warning (§3.1) with a coded rejection.
   Leaf β therefore takes a real dependency edge onto PRD 1's task β.
4. **What stays bare.** Axis *direction* components (`ax`,`ay`,`az`, and `0.0, 1.0, 0.0` in
   `feature_datum_axis.ri:24`) are dimensionless unit-vector components and are **not** gated by
   this PRD. Axis *origin* components are LENGTH and belong to PRD 1.
5. **Gated positions (exhaustive for this PRD):** `rotate.angle`, `rotate_around.angle`,
   `revolve.angle`, `arc.start_angle`, `arc.end_angle`, `draft.angle`,
   `circular_pattern.angle` — plus the four selector `tol` args, which are already gated and
   only gain the hint.

### C2 — Unit-label alphabet (the two-way boundary)

**The rule: accept what we cannot enumerate; normalize what we curate.**

- **Accept** U+00B7 as a unit-multiply operator, because S1 composes labels from *arbitrary*
  dimension vectors — the label set is not enumerable, so it cannot be normalized at the source,
  and `·` is the SI-conventional multiply that authors also paste in from datasheets.
- **Normalize** the curated labels of S2/S3/S4 to ASCII, because those tables are finite,
  hand-written, and under our control; adding a second accepted alphabet (superscripts, an
  open-ended set including `⁰¹²³⁴…` and U+207B) would buy two spellings for one token and a
  parallel numeric lexer.

**Invariant R (the property under test).** For every dimension `d` and every label `ℓ` that any
of S1–S4 can emit for `d`:

```
parse_and_resolve(format!("1{ℓ}")) succeeds
    ∧ its DimensionVector == d
    ∧ its SI value == the scale that surface associated with ℓ   (rel-tol 1e-12)
```

Empty labels (dimensionless) are excluded. `g/cm^3` resolves to `999.9999999999999` — the
tolerance is load-bearing, not slack.

**Scanner contract.** `UNIT_MUL_OP` fires on ASCII `*` **or** U+00B7 (the decoded codepoint
`0xB7` — `lexer->lookahead == 0xB7`, **not** the UTF-8 byte pair `0xC2 0xB7`), in both
cases only when the following character satisfies `is_unit_start` (`scanner.c:101`). Nothing else
changes: `_unit_div_op` stays `/`, exponent stays `^`, `signed_integer` stays `/-?\d+/`. `·`
outside a `unit_expr` remains an error (there is no general `·` operator in Reify).

> **CORRECTION 2026-08-29 (task 5949, from task 5784's decompose addendum K1).** The contract
> sentence above previously read "`UNIT_MUL_OP` fires on ASCII `*` **or** U+00B7
> (UTF-8 `0xC2 0xB7`)"; unlike the 2026-08-19 note below it has been corrected **in place**,
> because an implementer coding to the byte-pair wording writes a branch that provably can
> never fire. Source: task 5784's decompose addendum **K1** (2026-07-29), re-confirmed by the
> sweep filed as **esc-5784-3** (2026-08-03, L0) — which also recommended the PRD amendment —
> and promoted the same day by the orphan reaper to **esc-5784-4** (L1). **2026-08-05** is the
> date that sweep carries where it was folded into task 5784's description, not either filing
> date; look the escalations up by ID, not by that date. It is also binding record C1 of this
> PRD's capability manifest. Substrate fact:
> `tree-sitter-reify/src/tree_sitter/parser.h:49` declares `int32_t lookahead;` — tree-sitter
> delivers **decoded codepoints**, not raw bytes. U+00B7 therefore arrives as the single value
> `0xB7`, the lead byte `0xC2` is never observable, and one unmodified `lexer->advance()`
> consumes the whole 2-byte codepoint. Controlled experiment (three isolated repo copies,
> isolated `XDG_CACHE_HOME`, recompilation proven by a deliberate `#error` variant that failed
> to build): HYP A — `(c == '*' || c == 0xB7)` — gives
> `tree-sitter parse --quiet tests/prd-gate/fixtures/unit_middot_mul.ri` **exit 0** with the
> correct tree (`left: N`, `right: m`); HYP B — the PRD's byte pair, `c == 0xC2` then require
> `0xB7` — gives **exit 1**, a branch that can never fire. The same correction voids §3.8's
> "UTF-8-aware read" phrase, corrected there too.

> **CORRECTION 2026-08-19 (Leo, via esc-5784-5).** The sentence "Nothing else changes" above is
> **wrong**, and is left in place only so the record of what task 5784 was planned against stays
> intact. Widening the scanner to accept U+00B7 is **not sufficient on its own**: with only the
> scanner change the declaration is silently **DROPPED**, because `ts_parser.rs` lowering must be
> updated too. The rest of the paragraph stands — `_unit_div_op`, the exponent token and
> `signed_integer` genuinely do not change, and `·` outside a `unit_expr` genuinely stays an error.
> The correction was measured while planning task 5784 and is folded into that task's plan; it is
> recorded here so a future reader of C2 does not repeat the omission. **Provenance — one
> measurement, five IDs, none of them a second finding.** It was first filed as **esc-5784-1**
> (2026-07-31, L0) and promoted by the orphan reaper to **esc-5784-2** (L1); re-filed carrying
> the PRD recommendation as **esc-5784-3** (2026-08-03, L0) and promoted the same way to
> **esc-5784-4** (L1); rolled up as **esc-5784-5** (L2, `member_ids == ["esc-5784-4"]`), the ID
> cited above. The **2026-08-05** stamp this correction once carried is the date the sweep bears
> in task 5784's description, not the filing date of any of the five.

### C3 — Torque dimension

```rust
// crates/reify-core/src/dimension.rs
/// Torque: N·m/rad = kg·m²·s⁻²·rad⁻¹ — Force·Length/Angle.
/// Numerically identical to ROTATIONAL_STIFFNESS; see §6 D4 for why both names exist.
pub const TORQUE: DimensionVector = DimensionVector::from_exps(&[(0,2),(1,1),(2,-2),(7,-1)]);
```

- NAMED_DIMENSIONS gains `(TORQUE, "Torque")` **after** the `RotationalStiffness` row, so
  `canonical_name()` for that vector is **unchanged** (`"RotationalStiffness"`) — non-breaking.
- `pub unit Nm : Torque = 1` in `crates/reify-compiler/stdlib/units.ri`, in a new
  `── Torque ──` section beside Force/Pressure. Structurally identical to `lbf : Force`.
- `Nm` ≠ `nm`: pinned by a test asserting `5Nm` resolves to `5 * TORQUE` and `5nm` to `5e-9 m`,
  and that `NM`/`nM` remain `unknown unit`.
- `Torque` (a NAMED_DIMENSIONS name) resolves as a type name with no module import, so
  `ports_mechanical.ri:25-29`'s alias block is deleted and replaced by a two-line pointer
  comment. The `Deviations from §11 spec` item 1 (`:9-11`) is deleted, not amended.

### C4 — Energy↔Torque teaching diagnostic

Fires on the dimension-mismatch path that today prints two raw exponent vectors (§3.5). Both
directions, appended to the existing message:

- Energy value at a Torque-expected position → `; torque in Reify carries an angle divisor —
  write \`5N*m/rad\` or \`5Nm\`, not \`5N*m\` (which is Energy)`
- Torque value at an Energy-expected position → `; \`N*m/rad\` is Torque; an energy is
  \`N*m\` (or \`J\`)`

Same message also stops printing raw vectors where a canonical name exists: `Scalar[Energy]`
rather than `Scalar[m^2·kg·s^-2]`. Where no canonical name exists the composed form is retained —
and, after cluster C, it re-parses.

## §6 — Resolved design decisions

**D1 — Hard rejection, no new deprecation window.** The shipped `circular_pattern` warning has
*been* the window. Corpus reliance is 0 for `circular_pattern` and 1 for `revolve` (§3.9), and
the spec already forbids a default unit system (`reify-language-spec.md:125`). Adding a second
window would leave two conventions live for longer at no measurable benefit.

**D2 — Migrate first, then gate (`real-dimensionless-unification.md` decision 2 shape).**
Leaf α dimensions `feature_datum_axis.ri:24` and the 8 kernel/IR `Draft{angle: Value::Real(..)}`
fixtures **before** any gate lands. This is behaviour-preserving by construction:
`extract_f64` = `Value::as_f64` (`reify-ir/src/value.rs:1635`) returns `si_value` for
`Value::Scalar`, so `Value::Real(0.1)` → `Value::angle(0.1)` reaches the kernel as the same
`0.1`. Verified by reading both functions, not assumed.

**D3 — Compile slots are a first-class deliverable, not polish.** §3.4's differential probe shows
an eval-only gate is invisible to `reify check`. Cluster A therefore ships slots (ζ) as a leaf
with its own signal, not as an optional follow-up. Slots complement, never replace, the eval
gates: `rotate(b, ax, ay, az, t)` where `t` is a runtime value stays statically invisible.

**D4 — `Torque` is added as an alias-position name, not a re-dimensioning.** Reify's model makes
Torque = `Force·Length/Angle`, and `ROTATIONAL_STIFFNESS` (`dimension.rs:275`) is *already* that
exact vector — so the two names share one vector, exactly as `Stiffness`/`TranslationalStiffness`
and `Impulse`/`Momentum` already do (`dimension.rs:572` neighbourhood). This PRD adds the name
after the existing row so no `canonical_name()` output changes. **It deliberately does not
re-dimension rotational stiffness** (arguably `Force·Length/Angle²`, since `τ = k·θ`) — that is a
flexures/joints/dynamics semantic change owned by PRD 5's territory, recorded in §8 and §10.

> **D4 ADDENDUM — the deferred half is now RULED (Leo, 2026-07-29; PRD 5 task #5799).**
> `ROTATIONAL_STIFFNESS` **does** become `Force·Length/Angle²` (`(7,-2)`), and
> `ROTATIONAL_DAMPING` likewise. D4's own alias framing above remains correct **for η as
> written** — `TORQUE` is still added at `(7,-1)`, which is unaffected — but two of its
> consequences expire once #5799 lands: (i) the two names stop sharing a vector, so the
> "placed after the existing row" ordering rationale stops being load-bearing and `TORQUE`
> becomes the sole holder of `(0,2)(1,1)(2,-2)(7,-1)`; (ii) `canonical_name()` for that vector
> flips from `"RotationalStiffness"` to `"Torque"`, so η's pinning test must be updated by
> #5799, not defended. Sequencing: **η lands first**; #5799 depends on it.
> *Why the ruling went that way:* a `NAMED_DIMENSIONS` alias row cannot separate two
> quantities for a dimension-checked reader — `accept_arg` keys on
> `*dimension == spec.dimension` (`reify-eval/src/arg_acceptance.rs:123`) and
> `ArgSpec.type_name` is display-only — so PRD 5's `rotational_stiffness_spec()` would
> otherwise have accepted a torque, permanently. Probe-measured at `d57cb55bc9`: with
> `rad⁻¹`, `k·θ` evaluates to `m^2·kg·s^-2` (Energy) and `½kθ²` to `m^2·kg·rad·s^-2` (matches
> nothing in the table); at `rad⁻²` they yield Torque and Energy respectively.

**D5 — Accept `·`; normalize superscripts.** Rationale is C2's "accept what we cannot enumerate".
*Rejected alternative:* make S1 emit `*` and change no grammar. It round-trips (probe-verified)
and is a one-line diff — but it makes Reify's own output less SI-conventional than the datasheets
it models and does nothing for authors pasting `N·m` in from outside. *Rejected alternative:*
also accept superscripts, keeping `mm²` — two spellings per token, an open-ended character class,
and a parallel numeric lexer in the scanner, for cosmetics.

**D6 — One label per rung, not a display/parse label pair.** Adding a `parse_label` beside the
pretty `label` would preserve `kg/m³` in the GUI dropdown. Rejected: the defect being fixed
*is* four independent label tables drifting; a fifth field doubles that surface, and Invariant R
would then only cover half of what users see. Basis for accepting the cosmetic loss: **0**
`@display` uses in the corpus, and the GUI picker is the only human-facing consumer.

**D7 — Declare `unit L : Volume = 0.001` rather than dropping the ladder rung.** `units.ri:56-60`
suggests extending `si_units.rs` with a cubic-metre base instead; rejected as disproportionate —
`L` is one hand-declared unit in the `lbf`/`psi` precedent, and generating an SI volume family is
its own change with its own prefix-ambiguity questions.

**D8 — Every leaf signal is phrased against `reify eval`.** PRD 2 owns `reify check` semantics
(G4). Leaf ζ's compile slots *do* surface at `check` today, so ζ's signal names both, but its
**binding** assertion is the `reify eval` one; no leaf here depends on PRD 2 landing.

**D9 — Record the 1763 reversal at the removal site.** §3.10.

**D10 — CONFIRMED: `revolve_full`'s synthesized TAU must be an ANGLE literal.** PRD 1's §7/§9 ask
this PRD to ratify the type its task α gives that literal. **ANGLE is correct and is what leaf γ
requires.** Verified at `54afdee50b`: `crates/reify-compiler/src/geometry.rs:2064-2067` builds
`CompiledExpr::literal(Value::Real(TAU), Type::dimensionless_scalar())` and wires it into the
`"angle"` arg of the `Revolve` `CompiledGeometryOp` at `:2080` — the *same* named arg γ gates via
`angle_spec()`. A LENGTH or dimensionless literal there would make every `revolve_full(...)` call
self-reject the instant γ lands. `revolve_full(rectangle(20mm,10mm), -10mm,0mm,0mm, 0.0,1.0,0.0)`
builds today (probe-verified) and must still build after γ; that is boundary row B2b. The
reciprocal obligation is PRD 1's: **α must land before γ** (§7, hard edge). This PRD does not
edit `reify-compiler/src/geometry.rs`.

**D11 — Compile-slot messages reuse PRD 1's reconciled template.** PRD 1's D9 also puts the
compile-slot message (which today lacks the eval layer's migration hint) onto the shared template
via its task η. Leaf ζ **consumes** that reconciliation rather than fixing the angle slots'
wording separately — a real edge onto PRD 1's η, so the four already-shipped ANGLE selector slots
and the new producer slots land on one template, not two.

## §7 — Pre-conditions for activating

PRD 1 (`docs/prds/v0_6/units-length-gate-completion.md`) **landed on main at `54afdee50b`**, so
every edge below resolves to a real sibling leaf at decompose (no "pending sibling" placeholders).

| Prerequisite | Why | Status |
|---|---|---|
| **PRD 1 task α** — dimension the compiler's synthesized geometry literals (`revolve_full` TAU `geometry.rs:2064-2067` → **ANGLE**; plus `cylinder_centered` dx/dy `:1502-1521`, `rounded_rect` dz `:2525`, `rounded_box` dz `:2362`/`:2439` → LENGTH) | `revolve_full` injects a **bare dimensionless** TAU into the exact `"angle"` arg leaf γ gates; without α, every `revolve_full(...)` self-rejects. Type choice ratified in **D10**. | hard `add_dependency` edge (α → γ) — **this PRD must not edit `reify-compiler/src/geometry.rs`** (G4) |
| **PRD 1 task β** — mints the shared `DiagnosticCode` for bare/wrong-dimension rejections (its D9(ii)) | C1 inv. 3: this PRD reuses that code and mints none of its own | hard edge (PRD 1 β → my β) |
| **PRD 1 task η** — reconciles the compile-slot message onto the shared hint-carrying template (its D9) | leaf ζ's new ANGLE producer slots must land on the reconciled template, not fork it (**D11**) | hard edge (PRD 1 η → ζ) |
| **PRD 1's closure-guard harness leaf** | ν extends its allowlist/universe; additive entries only, never a harness rewrite | hard edge |
| PRD 1's `ArgRejection::message` template + `Undef` chokepoint rule (D10) | β must not fork either; `angle_spec()` is an additive constructor on a frozen core | same-file additive coordination |
| Generated tree-sitter grammar present | G3 fixture re-runs at decompose need `src/parser.c` etc.; absent in fresh lanes — `bash scripts/tree-sitter-generate.sh` first | environmental |

No dependency on PRD 2, 4, or 5.

## §8 — Cross-PRD relationship (G4)

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `units-length-gate-completion` (1), landed `54afdee50b` | consumes (task **α**) | `revolve_full` TAU → **ANGLE** literal; all four desugared-literal families in `reify-compiler/src/geometry.rs` | **PRD 1** | **ANGLE choice CONFIRMED here — D10.** Hard edge α → γ |
| `units-length-gate-completion` (1) | consumes (task **β**) | the shared bare/wrong-dimension `DiagnosticCode` (its D9(ii)) | **PRD 1** | hard edge; this PRD mints no parallel code (C1 inv. 3) |
| `units-length-gate-completion` (1) | consumes (task **η**) | compile-slot message reconciled onto the hint-carrying template (its D9) | **PRD 1** | hard edge η → ζ (**D11**) |
| `units-length-gate-completion` (1) | consumes | closure-guard harness | **PRD 1** | hard edge from ν |
| `units-length-gate-completion` (1) | **assigned to me by its §7 / D9(i)** | `angle_spec` + the missing `migration_hint` at `geometry_ops.rs:8767-8771` (a live spec §14.5 breaking-change-obligation gap) | **PRD 3** | leaf β; accepted |
| `units-length-gate-completion` (1) | co-produces | `arg_acceptance.rs` — additive spec constructors only (`length_spec` theirs, `angle_spec` mine); core FROZEN | shared, non-contested | additive |
| `units-length-gate-completion` (1) | adopts | its **D10** `Undef`-at-chokepoint rule | **PRD 1** sets it, PRD 3 complies | C1 inv. 2 |
| `units-length-gate-completion` (1) | produces | `revolve`/`arc`/`rotate_around` **origin** (LENGTH) args in the same functions I edit for **angle** args (its tasks 5623/5658/5661) | **PRD 1** owns origins, **PRD 3** owns angles | file-lock coordination, not ownership contest |
| `units-length-gate-completion` (1) | notes | GUI `parse_value_string` inherits `·` acceptance | **PRD 1** | noted, not owned |
| `check-diagnostic-truthfulness` (2) | none binding | `reify check` exit/diagnostic semantics | **PRD 2** | D8 — no edge needed |
| `dimensioned-construction-strictness` (4) | none | ctor-slot conformance, `type_compat.rs` | **PRD 4** | untouched here |
| `dimension-checked-readers` (5) | **contact point** | `ROTATIONAL_STIFFNESS` re-dimensioning question (D4); joints/flexures `length_input`; task 2732's locking test for bare `Value::Real` cylindrical motion vars | **PRD 5** | §10 hand-off, no edge |
| `docs/prds/display-unit-preference.md` (shipped) | **consumes + modifies** | `unit_ladders()` label strings; `@display("…")` rung matching (`annotations/schema.rs:334`, task 5233); `resolve_display` (task 5234) | **PRD 3** owns the label *alphabet*; that PRD owns the registry *structure* and precedence | new seam, declared here; 0 corpus `@display` uses so no reciprocal edit needed |
| `docs/prds/v0_6/type-hygiene.md` | corrects | its `:106` `resolve_bare_angle :418-439` cite dies with ε | **PRD 3** | correction task in ε |
| GUI unit picker (`main.rs:699`) | consumes | ASCII ladder labels | **PRD 3** | λ's signal names it |

Checked against the overlay's three known contested pairs (persistent-naming↔multi-kernel,
imported-field-source↔multi-kernel, topology-selectors↔persistent-naming): no overlap, no fourth
instance introduced.

## §9 — Boundary-test sketch (G5, approach H)

Facing both sides of each seam. These are μ's and ρ's observable signals.

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| B1 | Bare angle rejected at every gated producer | β,γ,δ,ε landed; α's migration landed | For each of `rotate`,`rotate_around`,`revolve`,`arc`(×2),`draft`,`circular_pattern`: `reify eval` on a bare-angle fixture exits **1** and emits the C1 wording with `Angle` and the hint. **Negative-assertion probe:** the diagnostic is *observed*, not inferred from exit code alone. |
| B2 | Dimensioned angle still works, byte-identically | same | `45deg` / `1.5rad` / a let-bound ANGLE Scalar / an angle-typed arithmetic expr all build; **zero** diagnostics; resulting `angle_rad` unchanged from pre-PRD goldens. |
| B2b | **Compiler-desugared angle survives the gate** (the D10 ratification, facing PRD 1) | PRD 1 task α landed; γ landed | `revolve_full(rectangle(20mm,10mm), -10mm,0mm,0mm, 0.0,1.0,0.0)` builds with **zero** angle diagnostics and the same `angle_rad = TAU` as pre-PRD. Fails loudly if α typed the literal LENGTH or dimensionless — this row is the two-way test of the PRD-1↔PRD-3 seam. |
| B3 | Unit-vector components stay bare | same | `revolve(rect, -10mm,0mm,0mm, 0.0,1.0,0.0, 2*PI_rad)` builds clean — direction components are *not* gated (C1 inv. 4). |
| B4 | Compile slot fires where eval cannot be reached | ζ landed | `reify eval` on a bare-angle `rotate` reports the compile-slot diagnostic; the slot's arity guard does not mis-fire on `rotate(geo, orientation)` (2-arg form) or on `circular_pattern`'s 4-arg vs 9-arg forms. |
| B5 | Reify reads its own printed unit | κ landed | For every dimension in NAMED_DIMENSIONS: take S1's `Display` output `ℓ`, parse `1ℓ`, assert `DimensionVector` equality. Currently fails for every composite dimension. |
| B6 | Curated labels re-parse | λ landed | Invariant R over S2 ∪ S3 ∪ S4 (incl. `L`), with the 1e-12 rel-tol on scale. |
| B7 | `·` is scoped to unit expressions | κ landed | `let x = 5 · 3` is a parse error; `5N·m` parses; `5N· m` (space before unit start) does **not** consume the `·` as a mul-op — `is_unit_start` guard preserved. |
| B8 | Torque/Energy are distinguished and taught | η,θ,ι landed | `param t : Torque = 5N*m` errors naming **Energy** and **Torque** by name plus the C4 hint; `param e : Energy = 5N*m/rad` errors with the reverse hint; `param t : Torque = 5Nm` succeeds; existing `dimension.rs` Torque-vs-Energy guards (`:1438`, `:1470`, `:1513`) still pass unmodified. |
| B9 | `Nm` ≢ `nm` | θ landed | `5Nm` → torque `5`; `5nm` → `5e-9 m`; `5NM`,`5nM` → `unknown unit`. |
| B10 | Stdlib alias retired without regression | η landed | `ports_mechanical.ri` compiles with the alias deleted; `Torque` still resolves in `RotaryPort.max_torque`; `flexures.ri:155` unchanged and still compiling. |
| B11 | Closure guard covers angle positions | ν landed, PRD 1 harness landed | PRD 1's source-text probe over `GEOMETRY_FUNCTION_NAMES` reports **no** un-gated angle position; shrinking the angle allowlist makes the harness fire (anti-vacuity, mirroring `version_id_discipline_gate.rs`'s seeded self-tests). |
| B12 | Discoverability | ξ,ο,π landed | An author who knows the goal ("rotate this 45 degrees", "give this a torque rating") but not the feature name finds the deg/rad rule and `Nm` from `chunks/units.md` + `examples/best_practices/INDEX.md` without probing the compiler. |

## §10 — Out of scope (explicit)

- **`reify check` exit-code / diagnostic-collection semantics** — PRD 2. Every signal here is a
  `reify eval` signal (D8).
- **LENGTH-semantic args in the same functions** (`revolve` `ox/oy/oz`, `rotate_around` `px/py/pz`,
  `arc` centre/radius, `translate` d*) — PRD 1 / tasks 5623, 5658, 5661. Named in
  `arg_acceptance.rs:13-18`'s own residual note.
- **`reify-compiler/src/geometry.rs` desugarings** — PRD 1's task α, single leaf, all four literal
  families (`cylinder_centered` dx/dy, `rounded_rect` dz, `rounded_box` dz, `revolve_full` TAU).
  This PRD ratifies the ANGLE type for the TAU (D10) and edits none of them.
- **Re-dimensioning `ROTATIONAL_STIFFNESS` / `ROTATIONAL_DAMPING`** to `…/Angle²`. Discovered
  here (D4): under Reify's own model `τ = k·θ` forces `k = τ/θ`, so today's `rad⁻¹` makes
  rotational stiffness *dimensionally equal to torque*, and `k·Δθ` lands on the Energy vector.
  The const's own doc comment (`dimension.rs:275-277`) reflects the confusion. Fixing it changes
  flexures/joints/dynamics semantics — **PRD 5 territory**; hand-off in §11 Q1.
  **Still out of scope for this PRD, but no longer an open question: RULED YES by Leo
  2026-07-29 and owned by PRD 5 task #5799** (`(7,-2)` for both consts). See the D4 addendum
  for what that expires in η. A sibling defect found in the same session —
  `MOMENT_OF_INERTIA` is `kg·m²` so `½Iω²` yields `m^2·kg·rad^2·s^-2` rather than Energy — was
  ruled a **separate** decision and filed as **#5825**; it is out of scope for both PRDs here.
- **`×10ⁿ` engineering-notation magnitude formatting** (`reify-ir/src/value.rs:3002-3040`,
  `SUPERSCRIPT_DIGITS`). It formats the *number*, not the unit; `1.27×10³ kg/m^3` is not a
  literal form in any grammar and Invariant R does not cover it. Named so it is not silently
  assumed fixed.
- **The `unit_symbol_to_si` bootstrap fallback** (`reify-core/src/units.rs:24`, 14 arms) as a
  general drift problem. λ touches it only if `Nm`/`L` are needed inside stdlib fn bodies;
  reconciling it with the 210-symbol registry is a separate concern.
- **`+`-signed exponents** (`m^+2`). Does not parse (§3.8) and no surface emits it; not worth a
  grammar change.
- Task **2732**'s locking test (bare `Value::Real` accepted as cylindrical motion vars) — it pins
  the joints `length_input` path, PRD 5's.

## §11 — Open questions (tactical)

1. ~~**`ROTATIONAL_STIFFNESS` dimensional correctness.**~~ **RESOLVED — Leo, 2026-07-29.** Filed
   as a PRD 5 decision leaf (task **#5799**, ρ) at decompose exactly as suggested, escalated as
   `esc-5799-1`, and ruled in an interactive session: **YES, re-dimension** —
   `ROTATIONAL_STIFFNESS → [(0,2),(1,1),(2,-2),(7,-2)]`,
   `ROTATIONAL_DAMPING → [(0,2),(1,1),(2,-1),(7,-2)]`. #5799 now carries the ruling, the
   migration list, and a hard dependency on η (#5785). Rationale and its consequences for D4
   are in the D4 addendum above; the `MOMENT_OF_INERTIA` sibling defect is **#5825**. Nothing
   further is owed by this PRD.
2. **`dimension_unit_label` visibility.** It is private (`reify-ir/src/value.rs:3068`), so μ
   cannot call it cross-crate. *Suggested resolution:* make it `pub` (it is a pure label
   function, no invariant) so one property test covers all four surfaces. *Alternative:* keep it
   private and add an in-module ASCII-alphabet assertion, accepting that S3's parse round-trip is
   asserted structurally rather than by parsing. Decide during λ/μ.
3. **Where μ lives.** It needs `reify-core` (S1/S2/S4) + `reify-ir` (S3) + the parser. Only
   `reify-compiler` depends on all of `reify-ast`/`reify-core`/`reify-ir`/`reify-syntax`, so
   `crates/reify-compiler/tests/unit_label_round_trip.rs` is the natural home. Confirm at
   decompose that resolving a literal from a test there needs no `reify-eval`.
4. **Draft's arity forms.** `draft()` accepts 3 or 4 args (probe-verified); δ must gate the
   angle in both `modify_draft` arms (`geometry_ops.rs:1991`, `:2049`). Mechanical.
5. **Wording of the `got` label for an ANGLE-expected slot receiving a dimensionless `Scalar`.**
   `value_short_label` (`arg_acceptance.rs:134`) already prints `"dimensionless Scalar"`; confirm
   that reads well in the angle message or add a spec-level override. Cosmetic.
6. **`Frequency` cycles/s-vs-s⁻¹ separation (`RayleighDamping.alpha`).** §3.9 measures the site:
   `RayleighDamping.alpha` is declared `Frequency` but consumed on the rad/s scale
   (`crates/reify-eval/src/modal_ops.rs:1126`), and `modal_analysis.ri`'s ANGULAR-RATE TRAP
   comment names this PRD as the surface's owner — yet §5's contract (C1-C4) does not deliver a
   cycles/s-vs-s⁻¹ separation today. *Suggested resolution:* rule at decompose whether a new leaf
   takes this on or it is explicitly deferred to a later PRD. Decide during decompose.

## §12 — Decomposition plan

Signals are `reify eval`-phrased (D8). Greek labels; real task ids assigned at decompose.
Every leaf that adds or changes a diagnostic must give it a `DiagnosticCode` (INV-SF-6).

### Phase 0 — migrate first (workspace green throughout)

- **α — Dimension the bare-angle corpus and kernel/IR test fixtures.**
  *Modules:* `examples/geometric_relations/`, `crates/reify-ir/src/geometry.rs`,
  `crates/reify-kernel-occt/src/lib.rs`.
  *Change:* `feature_datum_axis.ri:24` bare `6.283185307179586` → `6.283185307179586rad` (leave
  the `0.0, 1.0, 0.0` direction bare); the 8 `Draft { angle: Value::Real(..) }` fixtures →
  `Value::angle(..)`. Behaviour-preserving by D2.
  *Signal:* `reify eval examples/geometric_relations/feature_datum_axis.ri` produces a
  byte-identical result to pre-change (the `cyl.axis` datum and `axis_dir` are unchanged), and
  the occt/ir Draft tests pass unmodified in assertion content.
  *Prereqs:* none. **Unlocks:** γ, δ, ε.

### Phase 1 — foundation

- **β — `angle_spec()` in `arg_acceptance`; retire the hint-less inline ANGLE spec.**
  *Modules:* `crates/reify-eval/src/arg_acceptance.rs`, `crates/reify-eval/src/geometry_ops.rs`
  (`:8767-8771`).
  *Change:* closes the spec §14.5 breaking-change-obligation gap PRD 1's §7/D9(i) assigns here.
  Reuses PRD 1 task β's `DiagnosticCode`; mints none.
  *Signal:* `reify eval` on `faces_by_normal(b, 0.0,0.0,1.0, 0.01)` now emits
  `faces_by_normal: tol argument expects Angle, got Real; pass a dimensioned angle such as
  \`45deg\` or \`1.5rad\`` **carrying PRD 1's `DiagnosticCode`** — hint and code are both new,
  observable, and shared by all four selectors.
  *Prereqs:* **PRD 1 task β** (hard edge). **Unlocks:** γ, δ, ε, ζ.

- **κ — Scanner accepts U+00B7 as unit-multiply.** *(G3 grammar-producer leaf.)*
  *Modules:* `tree-sitter-reify/src/scanner.c`, `tree-sitter-reify/test/corpus/unit_expr.txt`,
  `tree-sitter-reify/tests/` (new Rust grammar test, following
  `imaginary_literal_grammar_tests.rs`).
  *Signal:* `tests/prd-gate/fixtures/unit_middot_mul.ri` — which fails today with an ERROR node —
  parses with `tree-sitter parse --quiet` exit **0**; `reify eval` on `5N·m` yields the same
  value as `5N*m`; `let x = 5 · 3` remains a parse error (B7).
  *Prereqs:* none. **Unlocks:** μ.

- **η — `Torque` as a first-class named dimension; retire the stdlib alias.**
  *Modules:* `crates/reify-core/src/dimension.rs`, `crates/reify-compiler/stdlib/ports_mechanical.ri`.
  *Signal:* `reify eval` on a fixture using `param t : Torque = 5N*m/rad` succeeds with
  `ports_mechanical.ri`'s alias block deleted; `RotaryPort.max_torque` still resolves;
  `canonical_name()` for that vector still returns `"RotationalStiffness"` (pinned by test).
  *Prereqs:* none. **Unlocks:** θ, ι.

### Phase 2 — vertical slice: bare-angle rejection

- **γ — Gate `rotate` / `rotate_around` / `revolve` / `arc` angle args at the eval chokepoint.**
  *Modules:* `crates/reify-eval/src/geometry_ops.rs` (`:2251, 2312, 2975, 3327, 3328`).
  *Signal:* `reify eval` on `rotate(box(10mm,10mm,10mm), 0.0,0.0,1.0, 45)` exits **1** with the
  C1 wording + PRD 1's code; `45deg` still builds with zero diagnostics; **and
  `revolve_full(rectangle(20mm,10mm), -10mm,0mm,0mm, 0.0,1.0,0.0)` still builds** (B2b — the
  ANGLE-typed TAU from PRD 1 α, ratified in D10).
  *Prereqs:* α, β, **PRD 1 task α** (hard edge).

- **δ — Gate `draft.angle` (R7 raw-`Value` passthrough).**
  *Modules:* `crates/reify-eval/src/geometry_ops.rs` `modify_draft` (`:1979`, `:1991`, `:2049`).
  *Signal:* `reify eval` on a `draft(..., 5, ...)` fixture exits **1** with
  `draft: angle argument expects Angle, got Int; …`; `5deg` builds. Both arity forms covered
  (§11 Q4).
  *Prereqs:* α, β.

- **ε — Retire `resolve_bare_angle`; `circular_pattern` converges on rejection.**
  *Modules:* `crates/reify-eval/src/geometry_ops.rs` (`:880`, `:2585`, `:2626`),
  `crates/reify-eval/tests/circular_pattern_angle.rs`, `docs/prds/v0_6/type-hygiene.md:106`.
  *Change:* delete `resolve_bare_angle` and its code-less warning; route both call sites through
  `angle_spec()`. Invert `circular_pattern_bare_360_emits_deprecation_warning`; keep
  `circular_pattern_360deg_no_deprecation_warning` as-is. Record the **task-1763 reversal**
  (§3.10) in the commit message and a removal-site comment. Fix the stale `type-hygiene.md` cite.
  *Signal:* `reify eval` on `circular_pattern(b, 0mm,0mm,0mm, 0,0,1, 4, 360)` exits **1** with
  the C1 wording (previously: warning + exit 0); `360deg` builds warning-free.
  *Prereqs:* α, β.

- **ζ — ANGLE `CheckableArg` compile slots for the angle producers.**
  *Modules:* `crates/reify-compiler/src/builtin_signatures.rs` (extend the `:168` angle arm).
  *Change:* arity-guarded slots for `rotate` (5-arg form only — the 2-arg orientation form has no
  angle), `rotate_around`, `revolve`, `arc` (×2), `draft`, `circular_pattern` (4-arg and 9-arg
  forms).
  *Signal:* `reify eval` on the bare-angle `rotate` fixture reports the **compile-layer**
  diagnostic — on PRD 1 η's reconciled hint-carrying template (D11), not a forked one — and
  `rotate(geo, orient_identity())` is unaffected (B4).
  *Prereqs:* β, **PRD 1 task η** (hard edge). *(Also newly visible at `reify check` — noted, not
  the binding assertion, D8.)*

- **ν — Extend PRD 1's closure guard with the angle positions.**
  *Modules:* PRD 1's harness allowlist (additive entries only).
  *Signal:* the harness reports zero un-gated angle positions across `GEOMETRY_FUNCTION_NAMES`
  (64 names, `crates/reify-compiler/src/units.rs:21-81` — **[drift]**, research doc said 66); a
  seeded shrunken-angle-allowlist self-test fires (B11).
  *Prereqs:* γ, δ, ε, ζ, **PRD 1's harness leaf** (hard edge).

### Phase 3 — units surface

- **θ — `Nm` torque unit; case-distinctness pinned.**
  *Modules:* `crates/reify-compiler/stdlib/units.ri`, unit tests.
  *Signal:* `reify eval` on `let t = 5Nm` yields the same value as `5N*m/rad`; `5nm` still yields
  `5e-9 m`; `5NM`/`5nM` still `error: unknown unit` (B9).
  *Prereqs:* η.

- **ι — Energy↔Torque teaching diagnostics; named dimensions in mismatch messages.**
  *Modules:* the param/let dimension-mismatch diagnostic path (§3.5).
  *Signal:* `reify eval` on `param t : Torque = 5N*m` emits a message naming **Energy** and
  **Torque** by name plus the C4 hint; the reverse fixture emits the reverse hint; the existing
  `dimension.rs` Torque-vs-Energy regression guards pass unmodified (B8).
  *Prereqs:* η, θ.

- **λ — ASCII-normalize the curated labels on S2/S3/S4; declare `unit L : Volume`.**
  *Modules:* `crates/reify-core/src/dimension.rs:465,467`,
  `crates/reify-core/src/display_units.rs` (11 label sites + ~10 test expectations),
  `crates/reify-ir/src/value.rs:3072,3074` (+ 3 test expectations),
  `crates/reify-compiler/stdlib/units.ri`. **S1's `·` and `dimension.rs:912,931` stay.**
  *Signal:* `reify eval` on a fixture using every curated label (`1mm^2`, `1kg/m^3`, `1g/cm^3`,
  `1L`, …) resolves all of them — `1L` fails on main today with `error: unknown unit: L`.
  *Prereqs:* none (label alphabet is already in-grammar, §3.8).

- **μ — Round-trip property test (Invariant R). Integration gate for cluster C.**
  *Modules:* `crates/reify-compiler/tests/unit_label_round_trip.rs` (new; §11 Q3).
  *Change:* for every dimension in NAMED_DIMENSIONS (`crates/reify-core/src/dimension.rs`,
  superseding the earlier `:514-595` **[drift]** note) and every label from S1–S4, assert
  Invariant R. **Iterate the slice — do not hardcode a row count.** Every count written down
  for this table so far has rotted or been mis-derived by hand, which is why the slice's own
  doc comment deliberately quotes none.
  **Alias rows — harmless for R itself, fatal for a *name* assertion.** Invariant R (§5) is
  **name-blind**: it compares the resolved `DimensionVector` and the SI scale for a
  (dimension, label) pair and never looks at a dimension *name*. Iterating every row of the
  slice is therefore safe — NAMED_DIMENSIONS holds alias rows where several names share one
  `DimensionVector`, so a whole-slice sweep merely re-covers three dims a second time, at no
  cost but duplicate work. The hazard bites only if the harness *additionally* asserts name
  identity — e.g. deriving each row's label through `canonical_name()` and checking the
  returned name equals the row's name. **Do not write that assertion.** By the documented
  placement convention (`dimension.rs:554-559`) an alias row is placed AFTER the canonical
  row it shares a vector with, so the first-match scan in `canonical_name()` returns the
  canonical name: `"TranslationalStiffness"`, `"Curvature"`, and `"Momentum"` come back as
  `"Stiffness"`, `"AbsorptionCoeff"`, and `"Impulse"`.
  Seed one anti-vacuity self-test: a deliberately non-ASCII label injected into the harness's
  input makes it fail.
  *Signal:* the test is red on pre-κ/pre-λ main (for `·` and for `L`) and green after; a new
  ladder rung with a `³` in it fails the gate.
  *Prereqs:* κ, λ. *Drift-guard:* adds **no** `tests/infra/test_*.sh` and **no** wall-clock
  assertion, so `tests/infra/run-all-classification.manifest` and
  `test_no_new_wallclock_upper_bounds.sh` need no new rows — the leaf must *run* both guard
  scripts in its own diff to prove that, rather than assert it.

### Phase 4 — docs-truth (all four; language surface changed)

- **ξ — Doc-chunk update, registry-verified.**
  *Modules:* `crates/reify-mcp/src/tools/chunks/units.md`, `geometry.md`, `stdlib.md`.
  *Change:* the one angle rule (bare rejected everywhere, `deg`/`rad` required) replacing the
  unstated three; how to write a torque (`5Nm` / `5N*m/rad`) and why `5N*m` is Energy; the
  round-trip promise (what Reify prints, Reify reads). Fix `units.md`'s stale "35 standard named
  dimensions" (actual 51) and "Angle as Base Dimension" section, which asserts the torque/energy
  catch without telling the author how to spell either.
  *Signal:* every signature and literal documented in the touched chunks compiles verbatim in a
  smoke `.ri`; the doc-chunk truth gate passes.
  *Prereqs:* γ, δ, ε, θ, ι, λ.

- **ο — Best-practices exemplar + INDEX row.**
  *Modules:* `examples/best_practices/angles_and_torque.ri` (new), `INDEX.md`.
  *Change:* one compile-gated exemplar showing the dimensioned-angle idiom against the bare
  anti-pattern, and the torque-vs-energy distinction with `Nm`. Update
  `examples/best_practices/bolt_circle.ri`'s commentary if it implies bare angles are tolerated.
  *Signal:* `examples_smoke.rs` compiles it; `best_practices_index_matches_corpus_directory`
  passes (bidirectional invariant).
  *Prereqs:* ξ.

- **π — `reify-design` cheatsheet index line.**
  *Modules:* `.claude/skills/reify-design/SKILL.md` (index line only, no inline playbook).
  *Change:* one row pointing at ο's file; correct line 53 ("Quantities") and line 135 to state
  that bare angles are now **rejected**, not merely discouraged.
  *Signal:* the index line names the capability in intent terms ("rotating by an angle";
  "rating a torque").
  *Prereqs:* ο.

- **ρ — Discoverability acceptance (integration gate for Phase 4).**
  *Signal:* B12 — a goal-level search over the chunks + corpus index surfaces the deg/rad rule and
  `Nm` without reading compiler source. Executed as a scripted grep-and-read acceptance, not a
  claim.
  *Prereqs:* ξ, ο, π.

### DAG summary

```
PRD1.β ──→ β ─┬→ γ, δ, ε
α ────────────┘   (γ also ← PRD1.α, the ANGLE-typed TAU — D10)
PRD1.η ──→ ζ  (ζ also ← β)                                    — D11
γ,δ,ε,ζ ─→ ν   (ν also ← PRD1's closure-guard harness leaf)
κ ─┐
λ ─┴→ μ
η → θ → ι
γ,δ,ε,θ,ι,λ → ξ → ο → π → ρ
```

Cross-PRD edges (all real `add_dependency`, per `preferences_cross_prd_deps_real_edges`):
`PRD1.α → γ`, `PRD1.β → β`, `PRD1.η → ζ`, `PRD1.closure-guard → ν`. Resolve to real task ids at
decompose — PRD 1 is on main (`54afdee50b`), so none of these is a placeholder.

## §13 — G7 walk (advisory, author-mode)

Against `docs/legibility/design-invariants.md` (INV-SF-1..6):

- **INV-SF-2 `error-severity-exits-nonzero`** — direct hit, and the PRD *satisfies* it: every new
  rejection is Error-severity and exits 1 on `reify eval` (C1 inv. 3). No per-code escalation
  list is added. The one risk to watch at decompose: ζ's compile slots surface at `reify check`
  while γ/δ/ε's eval gates do not (§3.4) — that asymmetry is PRD 2's to close, and D8 keeps this
  PRD from depending on it.
- **INV-SF-6 `diagnostics-carry-codes`** — ε *removes* a live code-less diagnostic
  (`resolve_bare_angle`'s warning). C1 inv. 3 binds every diagnostic this PRD touches to carry a
  code. No waiver needed.
- **INV-SF-5 `placeholders-owned-and-loud`** — no placeholder types introduced. η *removes* one
  workaround (`ports_mechanical.ri`'s alias, which its own comment marks as a deviation).
- **INV-SF-1 `undef-has-provenance`** — `Acceptance::Undefined` keeps its existing quiet-degrade
  semantics (C1 inv. 2). This PRD adds no new root-undef path; it converts silent *acceptance*
  into loud rejection, which is the opposite direction.
- **INV-SF-3 / INV-SF-4** — no declared-intent consumption or Indeterminate outcomes touched.

No waivers recorded.
