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

**Measured 2026-08-26 at `181d1ec24c`** ("Merge task/2930 into main" — a `main`
merge commit, and therefore permanently reachable). **All four measured sections
— §§1, 2, 3 and 5 — share this one stamp.** α's own migration is not in it: α
sits on top of this commit, and every "MIGRATED by task 5777" row below marks a
change made after it.

One stamp, because an earlier revision used two and they disagreed. §§1–2 were
stamped at `50bc85d168` (2026-08-01) and §3 at `975cbfc301`, and task 5623's
LENGTH-gate leaf landed in between — moving bucket 2 from 31 to 36 (§2.4) *and*
bucket 3 from 14 to 20, two of those six being bare `arc` angle slots (§3). Every
one of the five new bucket-2 sites is γ's, so a reader sizing γ off the old §2.1
table under-scoped it by five.

That was not a one-off, and the case for a single stamp is now measured rather
than argued. This file's previous whole-file stamp was `0675683952` (2026-08-21),
five days before this one. **Every one of the four measured sections moved across
those five days:**

| Section | at `0675683952` | at `181d1ec24c` | What moved |
|---|---|---|---|
| §1 bucket 1 | 21 | **23** | `occt_non_length_fields_stay_ungated` landed in `reify-kernel-occt/src/lib.rs`, adding one `CircularPattern` **and** one `Draft` angle |
| §2 bucket 2 | 36 | **37** | one new δ site in `reify-eval/src/geometry_ops/tests.rs` — and it is an *inverting* test, not a retype (§2.4) |
| §3 bucket 3 | 20 sites, 22 raw hits | 20 sites, **23** raw hits | one new raw hit, a false positive (§3.4); the site total held |
| §5 `.ri` corpus | 21 | **22** | one new already-dimensioned `rotate` in `prj/printer_v01/printer.ri` |

Three of the four totals changed and the fourth's arithmetic did. Re-stamping any
one section alone would have left this file internally inconsistent again.

### Stamp measurements onto a commit that is ON MAIN, never a branch-local one

`main` grows by merge and is append-only, so a SHA on it stays reachable
forever. A task-branch SHA does not: a rebase rewrites every branch-local commit,
and each rewrite silently invalidates any stamp citing the old identity — the
number stays right while the coordinate that lets a reader reproduce it stops
resolving. Cite a merge-base or a `main` merge commit.

This file is the worked example. §3 was first stamped at a branch-local SHA;
a rebase rewrote it, so a follow-up commit re-stamped it — onto the *rebased*
branch-local SHA, which the very next rebase rewrote in turn, leaving that
commit's own "verified with `git merge-base --is-ancestor`" claim false at the
tip it created. §§1–2's stamp sat on `50bc85d168`, a `main` merge commit, and
survived both rebases untouched. Every SHA cited in this file is now on `main`.

### …and stamp the WHOLE file at ONE commit

Re-stamping one section at a time is how the rule above gets obeyed and the note
still ends up wrong. A per-section stamp is individually reproducible and
collectively meaningless: two sections measured at different commits cannot be
added, differenced or cross-checked, and nothing in the prose warns you which
pairs are safe. That is not hypothetical either — it is the §2/§3 drift the
header describes. **Re-derive every bucket at the new commit and move all the
stamps together, or move none of them.**

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

**23 sites at the stamp: 21 literal `angle: Value::Real(..)` + 2 `angle: r(..)`
closure forms.** That is the *pre-α* total — α's migration sits on top of the
stamp, so re-running the grep at a tip that contains α returns different numbers.
§1.2.1 reconciles the two.

### 1.1 The finding that matters

Every one of these builds a `GeometryOp` **by hand** and reaches the kernel via
`kernel.execute` / `geometry_op_to_operation`. None of them passes through
`compile_geometry_op`, which is where γ/δ/ε install their gate.

Therefore:

- **No bucket-1 site is broken by γ, δ or ε.** Migrating them is
  *representational* — it aligns the fixtures with the **post-δ (5780) target**
  representation, **not** with what production emits today — and is never
  gate-forced. Be precise about this: production's draft path is still an ungated
  raw-`Value` passthrough (`let angle = eval_arg("angle")?` in `modify_draft`,
  `crates/reify-eval/src/geometry_ops.rs`), so a bare `.ri` draft literal still
  reaches the kernel as `Value::Real` until δ lands. `circular_pattern` is the one
  exception that already coerces, through `resolve_bare_angle` (§4). The same note
  is carried at a migrated fixture itself, in the assertion message of
  `mock_execute_draft_records_op` (`crates/reify-test-support/src/mocks.rs`), so
  the two statements can be checked against each other.
- **A green run over bucket 1 is NOT evidence that any gate works.** If you need
  evidence a gate fires, it has to come from bucket 2 or from a `.ri` fixture.

### 1.2 Status

| Consumer | Sites at the stamp | Status |
|---|---|---|
| `GeometryOp::Draft` | 14 | 13 **MIGRATED by task 5777** → `Value::angle(..)`; 1 deliberately left bare (§1.2.1) |
| `GeometryOp::CircularPattern` | 9 | Out of α's scope — still bare |

The 13 migrated Draft sites: 6 in `reify-kernel-occt/src/lib.rs`, 3 in
`reify-ir/src/geometry.rs`, 3 in `reify-eval/src/engine_build/tests.rs` (2
literal + the one closure form of §1.3), 1 in `reify-test-support/src/mocks.rs`.

The 9 that remain bare — **every one a `CircularPattern`**:
`reify-ir/src/geometry.rs` (1), `reify-kernel-occt/src/lib.rs` (**2**),
`reify-kernel-occt/tests/harness_occt/pattern_single_pass_counter.rs` (1),
`reify-eval/src/engine_build/tests.rs` (3 — 2 literal + 1 closure),
`reify-eval/src/primitive_attribute_seed.rs` (1),
`reify-test-support/src/mocks.rs` (1).

Draft and CircularPattern are in fact the **only** two bucket-1 consumers there
can be. `GeometryOp::Revolve` and `GeometryOp::Rotate` declare their angle as a
bare `angle_rad: f64` field, not a `Value` at all (`crates/reify-ir/src/geometry.rs`),
so they are structurally outside this question — do not go looking for hand-built
Revolve/Rotate angles to migrate.

#### 1.2.1 Grep hits and migration targets are DIFFERENT sets

Post-α the two counts a reader naturally reaches for are **10** and **9**, and
conflating them is the easiest way to mis-derive this table:

| Which count | Composition |
|---|---|
| **10** hits of `rg 'angle: Value::Real' crates/` | 8 grep-visible bare `CircularPattern` **+ 2** deliberate bare `Draft` controls (below) |
| **9** migration targets still outstanding | *those same* 8 **+ 1** `r(1.57)` closure-form `CircularPattern`, which that grep cannot see (§1.3) |

The two sets overlap in 8 and differ at both ends. So the work still outstanding
in bucket 1 is **9 `CircularPattern` sites**, all ε's (5781) — not 10 grep hits.
Neither control arm is a migration target, and the closure form does not show up
in the grep you would naturally reach for.

Reconciling with §1's pre-α header: 23 = 14 `Draft` + 9 `CircularPattern`; α
migrated 13 of the 14 `Draft` and added one further bare `Draft` control of its
own (the first bullet below), which is why the literal grep falls 21 → 10 while
the `CircularPattern` work is unchanged at 9.

**Both remaining `Draft` grep hits are deliberate, and they fall to DIFFERENT
leaves — warn off both, not just δ:**

- **`draft_angle_dimensioned_matches_bare_real_volume`**
  (`reify-kernel-occt/src/lib.rs`) — α's own equivalence pin. It executes the
  same draft twice, once with `Value::Real` and once with `Value::angle`, and
  asserts the volumes match. Retyping the bare arm deletes the comparison.
  **δ's (5780).**
- **`occt_non_length_fields_stay_ungated`** (`reify-kernel-occt/src/lib.rs`) —
  the `Draft` arm. This is a control for the PRD's 46 = 41 + 3 + 2
  ungated-field split, not a corpus fixture, and its bareness is load-bearing:
  a bare `Value::Real` is the one shape `check_length_field` can never wave
  through, because its early return is gated on the `Value::Scalar` variant.
  Keep it bare and the arm still catches a rewire to `extract_length_f64` even
  if that predicate is later loosened to accept any dimensioned `Scalar`. A
  retyped arm would still warn on a rewire *today* — an ANGLE `Scalar` is not
  `LENGTH`, so `check_length_field` still returns `Some` — but it gives that
  extra reach up for nothing.
  **δ's (5780)** — but its sibling `CircularPattern` arm in the *same* test is
  **ε's (5781)**, and carries exactly the same reason.

Every Draft *fixture* is migrated; what is left is two controls.

### 1.3 The closure form the literal grep misses

`crates/reify-eval/src/engine_build/tests.rs` has a local
`let r = |v| Value::Real(v);` whose call sites include one Draft angle (migrated
by α) and one CircularPattern angle (still bare — it is the 9th site of §1.2).
`rg 'angle: Value::Real'` cannot see either; `rg 'angle: r\('` is the grep that
does. This is why the PRD's original Draft count was 8 rather than 13.

**That closure is shared** with the Chamfer `distance`, Shell `thickness` and
Thicken `offset` cases in the same table, all of which are LENGTH-semantic. Do
not retype, generalise or redefine `r` — change individual call sites only.

---

## 2. Bucket 2 — GATE-REJECTED (`("angle", <bare literal>)` via `compile_geometry_op`)

**37 sites.** These are the hand-written `CompiledExpr` args that break when a
gate lands.

> **Scope caveat — bucket 2 is not the whole blast radius.** It covers only
> angles written as Rust `CompiledExpr` literals. Rust tests that embed bare
> `.ri` SOURCE TEXT and compile it through the same chokepoint break too, and
> they are counted separately in [§3](#3-bucket-3--bare-angles-in-ri-source-text-embedded-in-rust-tests).
> Sizing a leaf off the 6-file / 37-site table below alone will under-scope it.

### 2.1 Split by consuming leaf

| Task | Builtin | Sites |
|---|---|---|
| **γ (5779)** | `revolve` | 18 |
| | `rotate_around` | 4 |
| | `rotate` | 1 |
| | `arc` (`start_angle` + `end_angle`) | 4 |
| | **γ total** | **27** |
| **δ (5780)** | `draft` | 3 |
| **ε (5781)** | `circular_pattern` | 7 |

Per file: `reify-eval/src/geometry_ops/tests.rs` 18,
`reify-eval/tests/compile_geometry_op_characterization.rs` 8,
`reify-eval/tests/harness_geometry/geometry_error_handling.rs` 7,
`reify-eval/tests/harness_fea_solver_e2e/stress_sweep_degenerate.rs` 2,
`reify-eval/tests/harness_sweep/swept_kind_classifier_e2e.rs` 1,
`reify-eval/tests/harness_topology_selector/topology_attribute_extrude_revolve_e2e.rs` 1.

Both splits total 37. The per-file column is the cheap one to re-derive — it is
a direct `rg -c` of the §"Derivation commands" bucket-2 regex plus the single
`Int` site of §2.2. The per-leaf column is not: it needs every match attributed
to its enclosing `SweepKind` / `TransformKind` / `PatternKind` / `CurveKind` /
`ModifyKind`, which no grep does for you. If the two columns ever disagree,
trust the per-file one.

### 2.2 Two sub-forms the obvious grep will not find

- **`arc`'s angles are not called `"angle"`.** They are `("start_angle", ..)` and
  `("end_angle", ..)`, consumed by the `Arc` arm of `geometry_ops.rs`. A literal
  `"angle"` grep misses both. They belong to γ.
- **One site passes a bare `Int`, not a bare `Real`.** In
  `reify-eval/src/geometry_ops/tests.rs`, the `circular_pattern` bare-integer test
  binds `CompiledExpr::literal(Value::Int(360), Type::Int)` to a local and passes
  the local, so no `lit(..)` / `literal_f64(..)` helper appears at the call site.
  It belongs to ε, and it is the reason the helper-shaped regex above returns 36
  rather than 37.

### 2.3 Which helper makes a literal "bare"

Bare (`Value::Real` + `Type::dimensionless_scalar()`) — these are bucket 2:

- `literal_f64` — `reify-eval/src/geometry_ops/tests.rs`
- `lit` — `reify-eval/tests/compile_geometry_op_characterization.rs`
- `real_literal` — free fn in `harness_topology_selector/…`, and per-test closures
  in `harness_geometry/geometry_error_handling.rs`,
  `harness_fea_solver_e2e/stress_sweep_degenerate.rs`,
  `harness_sweep/swept_kind_classifier_e2e.rs`

Already dimensioned — these are **NOT** bucket 2, do not "migrate" them:

- `literal_angle` — `reify-eval/src/geometry_ops/tests.rs` (10 angle call sites;
  a raw `rg -o 'literal_angle\('` returns 11 because it also matches the `fn`
  definition)
- `rad_literal` — `reify-eval/tests/curve_constructors_e2e.rs` (2 arc call sites)

### 2.4 Why this is 37, and earlier revisions said 36 and 31

Two landings moved this number, in that order.

**36 → 37 (`0675683952` → `181d1ec24c`).** One site, in
`reify-eval/src/geometry_ops/tests.rs`, which goes 17 → 18:
`compile_geometry_op_draft_angle_stays_on_the_bare_path`, whose args are
`vec![("angle".to_string(), literal_f64(0.1))]`.

> **This one is δ's, and it INVERTS — it is not a retype.** Its own doc comment
> calls itself a "NEGATIVE SCOPE LOCK": it asserts that a bare `Real` draft angle
> still compiles to `Ok` *and* is stored as the bare `Real` it was written as,
> and says in as many words that "re-wrapping it as an ANGLE `Scalar` would be
> just as wrong as rejecting it". It was written to hold the angle surface open
> for *this* PRD. When δ (5780) lands its gate, this test must be inverted the
> way §3.2's two eval-side tests must be — a migrator working uniformly down the
> table would silently retype away the assertion that guards δ's own boundary.

**31 → 36 (`50bc85d168` → `0675683952`).** If you are holding a copy of this
table that reads **31**, it was measured at `50bc85d168`. Task 5623's LENGTH-gate
leaf landed between that commit and the next stamp, and it added five bucket-2
sites — all five in `reify-eval/src/geometry_ops/tests.rs`, which went 12 → 17:

| Added by 5623 | Leaf | Why it is bucket 2 |
|---|---|---|
| `revolve_with_origin` | γ | shared LENGTH-gate case builder; angle deliberately bare |
| `compile_geometry_op_revolve_bare_origin_beats_degenerate_axis` | γ | precedence test over the same builder |
| `rotate_around_with_point` | γ | shared case builder; comment says "ANGLE is PRD 3's, never ours → stays bare" |
| `arc_with_center_radius` (`start_angle`) | γ | arc's angle pair, §2.2's first sub-form |
| `arc_with_center_radius` (`end_angle`) | γ | " |

So that delta was γ-only: revolve 16 → 18, `rotate_around` 3 → 4, `arc` 2 → 4,
γ total 22 → 27. ε was untouched at 7, and δ stood at 2 until the 36 → 37
landing above took it to 3.

These are all **deliberately** bare, not oversights — 5623's own comments say
the angle stays bare because it is *this* PRD's to gate. They are still γ's
work: a gate at `compile_geometry_op` does not read comments.

The same 5623 landing moved bucket 3 from 14 to 20 (§3). One commit, both
buckets — which is one half of why this file carries a single stamp; the other
half is the four-way move tabulated in the header.

---

## 3. Bucket 3 — bare angles in `.ri` source text embedded in Rust tests

**20 sites across 7 files.** *(At the file-wide stamp `181d1ec24c` — the same
commit as §§1–2 and §5. Re-derive before acting.)*

An earlier revision of this section reported **14 across 6**, measured on a tree
predating task 5623's LENGTH-gate leaf signal — the same bucket-3 content
`50bc85d168` still carries, where the §"Derivation commands" greps return 18 raw
`let … = builtin(…)` hits and 0 `arc` hits, i.e. (18 − 5) + 0 + 1 = 14. The
current stamp is later and includes that file, which adds 6 eval-chokepoint
sites and the 2 bare `arc` slots, giving 23 + 2 and hence 20 (§3.4). That same
landing is what moved bucket 2 from 31 to 36 (§2.4). The raw `let`-form hit count
has since gone 22 → 23 without the *site* total moving at all — the extra hit is
a false positive (§3.4), which is exactly why §3.4 exists.

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
| | `reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs` | 1 | ε |
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
source at all. Bare inline **`arc`** sites: **2** (still zero at `50bc85d168`) —
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

### 3.4 Reproducing 20 from 23 + 2 raw hits

The §"Derivation commands" `let … = builtin(…)` regex returns 23 and the `arc`
regex returns 2. Of the 23, six are false positives; one true site is invisible
to both:

- **2 in `rotate_e2e.rs`** — the trailing unsuffixed numeric belongs to
  `vec3(0.0, 0.0, 1.0)` nested inside `orient_axis_angle(…, 90deg)`. The angle
  itself is dimensioned.
- **1 in `mirror_circular_value_forms_e2e.rs`** — same shape, different nest:
  `circular_pattern(b, axis_z(point3(12, 0, 0)), 6, 60deg)`. The regex matched
  `point3(12, 0, 0)`'s closing paren, not the angle, which is already `60deg`.
  This is the hit that took the raw count 22 → 23 without moving the site total.
- **1 in `let_scope_tests.rs`** — a commented-out line.
- **2 in `geometry_arg_count_span_tests.rs`** — deliberate arg-COUNT-error
  fixtures (`circular_pattern(box(...), 1.0)`, `rotate(box(...), 0.0, 0.0)`).
  They have the wrong arity, so there is no angle slot and they fail before any
  angle gate could fire. Not migration targets.
- **+1 invisible**, the `format!`-built `circular_pattern_angle.rs` site (§3.2).

(23 − 6) + 2 + 1 = **20**.

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

A migrator working uniformly down the 37-site table would make this error in
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

A sweep of every tracked `.ri` file (`git ls-files '*.ri'`) finds **22**
executable angle-consuming builtin call sites (`revolve` / `rotate` /
`rotate_around` / `circular_pattern` / `arc`) at the file-wide stamp.

Reproducing 22 from the raw sweep, which returns **34 matches**:

- **−7 comment-only lines** — the regex matches commented-out call sites and the
  word "arc" in prose.
- **−5 duplicate matches** — five call sites in
  `crates/reify-compiler/tests/fixtures/stdlib_geometry_ops_smoke.ri` carry a
  trailing `// revolve(…)`-style signature comment, so the builtin name appears
  twice on ONE line and `rg -o` counts it twice. Count distinct `file:line`
  pairs, not matches.

34 − 7 − 5 = **22**.

The corpus was flat at **21** across `50bc85d168` and `0675683952` — same
per-file counts, same call sites on the same lines — and moved by exactly one
between `0675683952` and this stamp: `prj/printer_v01/printer.ri` gained a
`rotate(circle(groove_r), 1.0, 0.0, 0.0, 0deg - 90deg)`. That site is **already
dimensioned**, so it is not a migration target for anyone; it moves the sweep
total and nothing else. Do not read a flat corpus total as a promise that the
corpus is static.

No absolute count of tracked `.ri` files is given here on purpose: it rots on the
next example that lands, exactly the way a line number does. Measured, it went
595 → 639 between `50bc85d168` and the current stamp — 44 new files in under four
weeks — while the angle sites above went 21 → 22. That gap is the whole argument
for counting sites rather than files.

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
- `crates/reify-compiler/tests/harness_compilation_surface/examples_smoke.rs` —
  auto-discovers it by recursive walk, with **no** `SKIP_SET` entry. `examples_smoke`
  is a MODULE inside the `harness_compilation_surface` test binary, not a test target
  of its own, so the invocation is
  `cargo test -p reify-compiler --test harness_compilation_surface -- examples_smoke`.
  A `SKIP_SET` entry would be the opposite of coverage — it drops the file from the
  walk entirely — so do not add one.

It is correctly **not** a `_RUST_COUPLED_RI_FIXTURES` member in `scripts/verify.sh`
— that list is scoped to `tests/prd-gate/fixtures/` — but the coupling is real.
Any future edit to this file must keep all three green.

---

## 7. Sibling tasks

Verified against live task records rather than copied from prose:

| | Task | Scope |
|---|---|---|
| α | 5777 | this ledger; `.ri` corpus site; the 13 Draft fixtures |
| β | 5778 | `angle_spec()` in `crates/reify-eval/src/arg_acceptance.rs`, and routing `resolve_scalar_dim_arg`'s inline ANGLE spec through it — the rejection/hint surface γ/δ/ε/ζ build their diagnostics from. **No literal site in any bucket here**: β changes how a rejection reads, never an angle literal. |
| γ | 5779 | gate `rotate` / `rotate_around` / `revolve` / `arc` at the eval chokepoint |
| δ | 5780 | `draft` |
| ε | 5781 | retire `resolve_bare_angle` (`circular_pattern`) |
| ζ | 5782 | `builtin_signatures` ANGLE `CheckableArg` slots |
| ν | 5783 | extends PRD 1 task ι's (5752) closure-guard allowlist/universe with the ANGLE positions — additive entries only, never a harness rewrite. Its `metadata.files` is deliberately EMPTY (the harness path is fixed by 5752 and was not knowable at decompose). **No literal site in any bucket here.** |

γ's other hard edge — PRD 1's task α, 5742 — is already `done`.
