# Angle-literal migration ledger

Companion measurement note for
[`docs/prds/v0_6/angle-units-surface-convergence.md`](../prds/v0_6/angle-units-surface-convergence.md).
Produced by task 5777 (angle-units α). This file carries only **measured data
the PRD does not** — it deliberately does not restate the PRD's normative
contract table.

Consumers: tasks γ (5779), δ (5780), ε (5781). Each migrates its own subset in
its own diff, per PRD contract C6. α migrated only the `.ri` corpus site and the
hand-built `GeometryOp::Draft` fixtures, and published this ledger.

---

## Read this first — the numbers below are a snapshot, not the source of truth

**Measured 2026-08-01 at `50bc85d168`** (an in-branch ancestor of `task/5777`'s
tip — the merge-base at measurement time, before the branch was rebased onto a
newer `main`; bucket 2 and the `.ri` corpus are unchanged by α except where
marked).

Nothing in this repository validates a `file:line` citation, and line numbers rot
on the first edit to the cited file. This is not hypothetical: the site list α
inherited had drifted +17..+22 lines in one file and was missing five sites
entirely, within three days of being written.

So this file follows the convention `scripts/verify.sh` already states for the
same class of artifact — *"Deliberately NO file:line citations: nothing validates
them and they rot on the first edit"* — and inverts the emphasis:

> **The derivation commands below are the source of truth. Re-run them at your
> own HEAD before acting. Treat every tabulated line number as a snapshot.**

Per-file *counts* are given wherever a count suffices, because a count survives
edits that a line number does not.

### Derivation commands

```sh
# Bucket 1 — hand-built GeometryOp fixtures (gate-inert).
rg -n 'angle: Value::Real' crates/
rg -c 'angle: Value::Real' crates/          # per-file counts
# ...plus the closure form, which the literal grep above CANNOT see:
rg -n 'angle: r\(' crates/

# Bucket 2 — ("angle", <bare literal>) tuples fed through compile_geometry_op.
# Multiline, because the arg tuples wrap. Covers the .into() / .to_string() forms.
rg -n -U --multiline-dotall \
  '\("(angle|start_angle|end_angle)"(\.into\(\)|\.to_string\(\))?,\s*(lit|literal_f64|real_literal)\s*\(' \
  crates/ --glob '*.rs'

# ...then widen to ANY right-hand side and diff against the above. The complement
# contains the already-dimensioned siblings, the compiler's own production code,
# and the one bare-Int site the helper regex cannot match (see §2.2).
rg -n -U --multiline-dotall \
  '\("(angle|start_angle|end_angle)"(\.into\(\)|\.to_string\(\))?\s*,' crates/ --glob '*.rs'

# Bucket 3 — bare angles in .ri source text EMBEDDED IN RUST TESTS. Matches a
# DSL `let x = builtin(...)` whose final argument is an unsuffixed numeric.
rg -n --glob '*.rs' \
  'let\s+\w+\s*=\s*(revolve|rotate|rotate_around|circular_pattern)\s*\(.*,\s*-?[0-9]+(\.[0-9]+)?\s*\)' \
  crates/
# ...and the arc form, whose angles are args 5 and 6 rather than the last one:
rg -n --glob '*.rs' 'arc\s*\([^)]*,\s*-?[0-9]+(\.[0-9]+)?\s*,\s*-?[0-9]+(\.[0-9]+)?\s*,' crates/

# .ri corpus — angle-consuming builtin call sites.
rg -n -g '*.ri' '\b(revolve|rotate|rotate_around|circular_pattern|arc)\s*\(' .
```

---

## 1. Bucket 1 — GATE-INERT (hand-built `GeometryOp`)

**21 sites: 19 literal `angle: Value::Real(..)` + 2 `angle: r(..)` closure forms.**

### 1.1 The finding that matters

Every one of these builds a `GeometryOp` **by hand** and reaches the kernel via
`kernel.execute` / `geometry_op_to_operation`. None of them passes through
`compile_geometry_op`, which is where γ/δ/ε install their gate.

Therefore:

- **No bucket-1 site is broken by γ, δ or ε.** Migrating them is
  *representational* — it keeps fixtures aligned with what production emits —
  and is never gate-forced.
- **A green run over bucket 1 is NOT evidence that any gate works.** If you need
  evidence a gate fires, it has to come from bucket 2 or from a `.ri` fixture.

### 1.2 Status

| Consumer | Sites | Status |
|---|---|---|
| `GeometryOp::Draft` | 13 | **MIGRATED by task 5777** → `Value::angle(..)` |
| `GeometryOp::CircularPattern` | 8 | Out of α's scope — still bare |

The 13 Draft sites: 6 in `reify-kernel-occt/src/lib.rs`, 3 in
`reify-ir/src/geometry.rs`, 3 in `reify-eval/src/engine_build/tests.rs`, 1 in
`reify-test-support/src/mocks.rs`.

The 8 that remain bare — **every one a `CircularPattern`**:
`reify-ir/src/geometry.rs` (1), `reify-kernel-occt/src/lib.rs` (1),
`reify-kernel-occt/tests/harness_occt/pattern_single_pass_counter.rs` (1),
`reify-eval/src/engine_build/tests.rs` (3), `reify-eval/src/primitive_attribute_seed.rs`
(1), `reify-test-support/src/mocks.rs` (1).

Draft and CircularPattern are in fact the **only** two bucket-1 consumers there
can be. `GeometryOp::Revolve` and `GeometryOp::Rotate` declare their angle as a
bare `angle_rad: f64` field, not a `Value` at all (`crates/reify-ir/src/geometry.rs`),
so they are structurally outside this question — do not go looking for hand-built
Revolve/Rotate angles to migrate.

#### 1.2.1 The two eights are DIFFERENT sets

Two post-α counts both come to 8. They are **not** the same 8, and conflating
them is the easiest way to mis-derive this table:

| Which 8 | Composition |
|---|---|
| hits of `rg 'angle: Value::Real' crates/` | 7 grep-visible bare CircularPattern **+ 1** deliberate `Draft` control arm (below) |
| bare fixture sites (the §1.2 table row) | *those same* 7 **+ 1** `r(1.57)` closure-form CircularPattern, which that grep cannot see (§1.3) |

The two sets overlap in 7 and differ only in the 8th member. So the migration
work still outstanding in bucket 1 is **8 CircularPattern sites**, not 8 grep
hits — the control arm is not a migration target, and the closure form does not
show up in the grep you would naturally reach for.

Pre-α the bucket-1 total was 21 = 13 Draft + 8 CircularPattern; α migrated the
13, which is why §1's header counts 19 literal + 2 closure forms while the grep
now returns 8.

The one remaining `Draft` grep hit is **deliberate**: it is the bare control arm
of α's own equivalence pin, `draft_angle_dimensioned_matches_bare_real_volume` in
`reify-kernel-occt/src/lib.rs`, which executes the same draft twice — once with
`Value::Real`, once with `Value::angle` — and asserts the volumes match. Retyping
it would delete the comparison. Every Draft *fixture* is migrated.

### 1.3 The closure form the literal grep misses

`crates/reify-eval/src/engine_build/tests.rs` has a local
`let r = |v| Value::Real(v);` whose call sites include one Draft angle (migrated
by α) and one CircularPattern angle (still bare — it is the 8th site of §1.2).
`rg 'angle: Value::Real'` cannot see either; `rg 'angle: r\('` is the grep that
does. This is why the PRD's original Draft count was 8 rather than 13.

**That closure is shared** with the Chamfer `distance`, Shell `thickness` and
Thicken `offset` cases in the same table, all of which are LENGTH-semantic. Do
not retype, generalise or redefine `r` — change individual call sites only.

---

## 2. Bucket 2 — GATE-REJECTED (`("angle", <bare literal>)` via `compile_geometry_op`)

**31 sites.** These are the hand-written `CompiledExpr` args that break when a
gate lands.

> **Scope caveat — bucket 2 is not the whole blast radius.** It covers only
> angles written as Rust `CompiledExpr` literals. Rust tests that embed bare
> `.ri` SOURCE TEXT and compile it through the same chokepoint break too, and
> they are counted separately in [§3](#3-bucket-3--bare-angles-in-ri-source-text-embedded-in-rust-tests).
> Sizing a leaf off the 6-file / 31-site table below alone will under-scope it.

### 2.1 Split by consuming leaf

| Task | Builtin | Sites |
|---|---|---|
| **γ (5779)** | `revolve` | 16 |
| | `rotate_around` | 3 |
| | `rotate` | 1 |
| | `arc` (`start_angle` + `end_angle`) | 2 |
| | **γ total** | **22** |
| **δ (5780)** | `draft` | 2 |
| **ε (5781)** | `circular_pattern` | 7 |

Per file: `reify-eval/src/geometry_ops/tests.rs` 12,
`reify-eval/tests/compile_geometry_op_characterization.rs` 8,
`reify-eval/tests/harness_geometry/geometry_error_handling.rs` 7,
`reify-eval/tests/harness_fea_solver_e2e/stress_sweep_degenerate.rs` 2,
`reify-eval/tests/harness_sweep/swept_kind_classifier_e2e.rs` 1,
`reify-eval/tests/harness_topology_selector/topology_attribute_extrude_revolve_e2e.rs` 1.

### 2.2 Two sub-forms the obvious grep will not find

- **`arc`'s angles are not called `"angle"`.** They are `("start_angle", ..)` and
  `("end_angle", ..)`, consumed by the `Arc` arm of `geometry_ops.rs`. A literal
  `"angle"` grep misses both. They belong to γ.
- **One site passes a bare `Int`, not a bare `Real`.** In
  `reify-eval/src/geometry_ops/tests.rs`, the `circular_pattern` bare-integer test
  binds `CompiledExpr::literal(Value::Int(360), Type::Int)` to a local and passes
  the local, so no `lit(..)` / `literal_f64(..)` helper appears at the call site.
  It belongs to ε, and it is the reason the helper-shaped regex above returns 30
  rather than 31.

### 2.3 Which helper makes a literal "bare"

Bare (`Value::Real` + `Type::dimensionless_scalar()`) — these are bucket 2:

- `literal_f64` — `reify-eval/src/geometry_ops/tests.rs`
- `lit` — `reify-eval/tests/compile_geometry_op_characterization.rs`
- `real_literal` — free fn in `harness_topology_selector/…`, and per-test closures
  in `harness_geometry/geometry_error_handling.rs`,
  `harness_fea_solver_e2e/stress_sweep_degenerate.rs`,
  `harness_sweep/swept_kind_classifier_e2e.rs`

Already dimensioned — these are **NOT** bucket 2, do not "migrate" them:

- `literal_angle` — `reify-eval/src/geometry_ops/tests.rs` (11 angle call sites)
- `rad_literal` — `reify-eval/tests/curve_constructors_e2e.rs` (2 arc call sites)

---

## 3. Bucket 3 — bare angles in `.ri` source text embedded in Rust tests

**20 sites across 7 files.** *(Measured at `cc6a903a9d`, later than §§1–2's
`50bc85d168` — re-derive before acting.)*

Originally measured at `8e2d63be17` as **14 across 6**. That is the pre-rebase
identity of `cc6a903a9d` (same patch-id, now a dangling object that no longer
resolves from this branch); re-measuring at the in-branch SHA added 6
eval-chokepoint sites, all in one file that task 5623's LENGTH-gate leaf signal
landed on `main` in between.

Buckets 1 and 2 are both about angles written as **Rust values**. This third
class is angles written as **DSL text** inside a Rust string literal, which the
test then parses and compiles. They are invisible to every grep in §§1–2 — no
`Value::` and no `("angle", ..)` tuple appears anywhere near them — yet the ones
that reach `compile_geometry_op` break exactly like bucket 2 does.

### 3.1 Split by chokepoint — this is what decides whether a site breaks

| Where it lands | File | Sites | Leaf |
|---|---|---|---|
| **Eval chokepoint** (`Engine::build` → `compile_geometry_op`) | `reify-eval/tests/harness_geometry/geometry_length_args_units_e2e.rs` | 6 | γ |
| | `reify-eval/tests/rotate_e2e.rs` | 1 | γ |
| | `reify-eval/tests/unified_dag_geometry_executors.rs` | 1 | γ |
| | `reify-eval/tests/circular_pattern_angle.rs` | 1 | ε |
| | **eval total — γ/δ/ε break these** | **9** | |
| **Compile only** (no `Engine`, no kernel) | `reify-compiler/tests/harness_langcore/let_scope_tests.rs` | 8 | 5 γ / 3 ε |
| | `reify-compiler/tests/harness_geometry_solver/geometry_profile_precondition_tests.rs` | 2 | γ |
| | `reify-compiler/tests/compile_api_tests.rs` | 1 | ε |
| | **compile total — ζ's (5782) concern, NOT γ/δ/ε's** | **11** | |

**`geometry_length_args_units_e2e.rs` (6 sites) is the trap in this table.** It
is task 5623's LENGTH-gate leaf signal, and its header comment says it leaves the
angle bare in *both* arms of every `assert_length_gate` pair deliberately. The
bare arm already expects an error, but the **dimensioned control** asserts *zero*
`Error` diagnostics and no units diagnostic at all. γ's gate fires on that
control's still-bare angle, so the break surfaces as a LENGTH-worded assertion
failing on a form that has no bare length left in it. Migrate all six — bare arm
included — or γ's own run reads as a length regression.

The three `reify-compiler` files were checked for `Engine::new` / `engine.build`
/ `build_with_kernel` and contain **none** — they only compile, so a gate
installed at the eval chokepoint cannot see them. They are listed anyway because
a compile-side ANGLE `CheckableArg` slot (ζ, 5782) *would* reach them, and
because a reader who greps for bare angles will find them and needs to know
which leaf owns them.

By builtin the 20 split γ 15 / ε 5 / **δ 0** — `draft` has no inline `.ri` test
source at all. Bare inline **`arc`** sites: **2** (was zero at `8e2d63be17`) —
both arms of `geometry_length_args_units_e2e.rs`'s `arc` case, each bare in the
start-angle *and* end-angle slot; every other inline `arc` writes `rad` or `deg`.
Nothing outside `crates/` matched.

### 3.2 The two INVERTING tests

Two of the nine eval-chokepoint sites are not migration targets at all — the
test's whole point is that the bare form is currently ACCEPTED, so γ and ε must
**invert the assertion**, not add a unit suffix:

- `rotate_e2e.rs` — `rotate_with_bare_radian_literal_lands_in_kernel` asserts a
  bare radian literal reaches the kernel. γ's gate rejects it.
- `circular_pattern_angle.rs` — `circular_pattern_bare_360_emits_deprecation
  _warning` asserts the bare-degrees path warns and coerces. ε retires
  `resolve_bare_angle`, so the warning it asserts stops existing. Its sibling
  `circular_pattern_360deg_no_deprecation_warning` is already dimensioned and
  survives. Both share `plate_source(angle_expr)`, so the bare site is
  `format!`-built and the §"Derivation commands" regex **cannot see it** — it is
  the +1 that makes 20 out of 19 grep-visible hits.

The remaining seven are plain retypes. `unified_dag_geometry_executors.rs` (a
sibling-realization *cycle* test) is incidental — its angle is bare only because
nothing made it otherwise — and the six in `geometry_length_args_units_e2e.rs`
are retypes with the §3.1 caveat attached.

### 3.3 The remedy differs from bucket 2 — fix the TEXT, not a `Value`

A bucket-3 site is fixed by editing the DSL source: `rotate(a, 0, 0, 1, 90)`
becomes `rotate(a, 0, 0, 1, 90deg)`. Do **not** reach for `Value::angle(..)` —
there is no `Value` at the site.

This also means §4's degrees/radians trap resolves itself here: the compiler
converts `360deg` to radians as part of unit resolution, so writing the suffix
*is* the conversion. Bucket 2's "compute π/2 by hand" rule is a bucket-2 rule
and must not be carried into a `.ri` string, where `Value::angle(PI/2.0)` is not
even expressible.

### 3.4 Reproducing 20 from 22 + 2 raw hits

The §"Derivation commands" `let … = builtin(…)` regex returns 22 and the `arc`
regex returns 2. Of the 22, five are false positives; one true site is invisible
to both:

- **2 in `rotate_e2e.rs`** — the trailing unsuffixed numeric belongs to
  `vec3(0.0, 0.0, 1.0)` nested inside `orient_axis_angle(…, 90deg)`. The angle
  itself is dimensioned.
- **1 in `let_scope_tests.rs`** — a commented-out line.
- **2 in `geometry_arg_count_span_tests.rs`** — deliberate arg-COUNT-error
  fixtures (`circular_pattern(box(...), 1.0)`, `rotate(box(...), 0.0, 0.0)`).
  They have the wrong arity, so there is no angle slot and they fail before any
  angle gate could fire. Not migration targets.
- **+1 invisible**, the `format!`-built `circular_pattern_angle.rs` site (§3.2).

(22 − 5) + 2 + 1 = **20**.

---

## 4. The degrees/radians trap — load-bearing for ε (5781)

**`circular_pattern` is the one builtin whose bare angle means DEGREES. Every
other bucket-2 site means RADIANS.**

- `circular_pattern` routes its angle through `resolve_bare_angle`
  (`crates/reify-eval/src/geometry_ops.rs`, called from both the value-axis and
  the scalar-axis branches of the `CircularPattern` arm). That function matches
  `Value::Real | Value::Int`, computes `deg * PI / 180.0`, pushes a warning, and
  returns `Value::angle(rad)`.
- Everything else routes through `f64_arg` / `eval_arg` → `eval_named_arg_f64` →
  `Value::as_f64`, which applies **no** conversion at all.

So the seven `circular_pattern` sites carry values like `90.0` and `360.0` that
**already mean 90° and 360°**.

> **ε must CONVERT, not retype.** `lit(90.0)` becomes `Value::angle(PI / 2.0)`,
> not `Value::angle(90.0)`.

The failure mode is silent, and worth being precise about. `resolve_bare_angle`
falls through its `_ => raw` arm for anything that is not `Real` or `Int` — so a
`Value::Scalar` passes through **unconverted**. Retyping `lit(90.0)` to
`Value::angle(90.0)` therefore does not merely skip the conversion; it feeds 90
*radians* into a slot that used to receive π/2. Nothing in the current suite
catches this: the re-baselined goldens would simply absorb the wrong value.

A migrator working uniformly down the 31-site table would make this error in
seven places at once.

**This rule is bucket-2-only — do not carry it into bucket 1.** Bucket 1 also
holds CircularPattern angle sites (§1.2), and they are the *opposite* case: they
are hand-built, so they sit **downstream** of `resolve_bare_angle` and already
hold post-conversion radians (`TAU`, `FRAC_PI_2`, `1.57`). The kernel reads them
through `extract_f64` → `Value::as_f64` with no conversion. Converting a
bucket-1 CircularPattern angle would introduce the very error §4 exists to
prevent, just in the other direction.

> **Bucket 2 CONVERTS (degrees → radians). Bucket 1 RETYPES ONLY.**

---

## 5. The `.ri` corpus

A sweep of every tracked `.ri` file (`git ls-files '*.ri'`) finds **21**
executable angle-consuming builtin call sites (`revolve` / `rotate` /
`rotate_around` / `circular_pattern` / `arc`).

No absolute count of tracked `.ri` files is given here on purpose: it rots on the
next example that lands, exactly the way a line number does — it already moved by
one between this note's measurement SHA and the branch that published it. Re-run
the §"Derivation commands" sweep, and discard its comment-only hits (the raw
regex matches commented-out call sites and the word "arc" in prose) to reproduce
the 21.

Before α, exactly **three** carried a bare angle. After α, **two** — and both are
deliberate:

| Site | Disposition |
|---|---|
| `examples/geometric_relations/feature_datum_axis.ri` | **MIGRATED by task 5777** → `6.283185307179586rad` |
| `tests/prd-gate/fixtures/bare_angle_silently_accepted.ri` — `rotate(b, 0.0, 0.0, 1.0, 45)` | **STAYS BARE** |
| `tests/prd-gate/fixtures/bare_angle_silently_accepted.ri` — `circular_pattern(…, 4, 360)` | **STAYS BARE** |

The two fixtures in `bare_angle_silently_accepted.ri` are **deliberate negative
fixtures** and are **not** migration targets — they are γ's and ε's RED fixture.
Migrating them would delete the evidence the gate is supposed to produce.

This is what makes PRD §3.9's "exactly ONE bare-angle `.ri` site / ZERO bare
`circular_pattern` sites" arithmetic true: it counts *non-deliberate* sites,
which is the count that matters.

Every other `.ri` angle site already writes `deg`.

**Axis direction components stay bare everywhere** (PRD C1 invariant 4). In the
corpus these appear as bare `0, 1, 0` / `0.0, 1.0, 0.0` immediately beside a
dimensioned angle — e.g. `rotate(solid, 0, 0, 1, 90deg)`. They are dimensionless
unit-vector components and are explicitly not gated. Do not migrate them.

---

## 6. Coupling note — `feature_datum_axis.ri`

`examples/geometric_relations/feature_datum_axis.ri` is Rust-coupled by tests the
PRD does not name:

- `crates/reify-compiler/tests/feature_datum_axis_example_tests.rs` — reads it by
  `concat!(env!("CARGO_MANIFEST_DIR"), …)` path
- `crates/reify-eval/tests/feature_datum_tests.rs` — same, for the OCCT-backed B8
  end-to-end witness
- `crates/reify-compiler/tests/examples_smoke.rs` — auto-discovers it by recursive
  walk, with an empty SKIP_SET entry

It is correctly **not** a `_RUST_COUPLED_RI_FIXTURES` member in `scripts/verify.sh`
— that list is scoped to `tests/prd-gate/fixtures/` — but the coupling is real.
Any future edit to this file must keep all three green.

---

## 7. Sibling tasks

Verified against live task records rather than copied from prose:

| | Task | Scope |
|---|---|---|
| α | 5777 | this ledger; `.ri` corpus site; the 13 Draft fixtures |
| β | 5778 | |
| γ | 5779 | gate `rotate` / `rotate_around` / `revolve` / `arc` at the eval chokepoint |
| δ | 5780 | `draft` |
| ε | 5781 | retire `resolve_bare_angle` (`circular_pattern`) |
| ζ | 5782 | `builtin_signatures` ANGLE `CheckableArg` slots |
| ν | 5783 | |

γ's other hard edge — PRD 1's task α, 5742 — is already `done`.
