# Source-Level Display-Unit Preference — PRD

> **Class:** reify-language design (source-level display-unit preference surface) +
> cross-surface formatting unification. Downstream of `docs/prds/annotation-args.md`
> (the annotation arg-schema registry `@display` registers into, §3) and task #5199
> (the GUI unit-picker + `unit_ladders()` registry this PRD reuses and extends, §2c/§4).
> Structurally a sibling of `docs/prds/money-dimension.md` (a decision-doc that spawns
> a coverage-per-task decomposition rather than shipping runtime code itself).
>
> **Approach:** decision-only PRD. This document makes no runtime change itself — §1–§6
> record the decisions and §7 files the implementation leaves that realize them.
>
> **Scope** This document specifies how a `.ri` source file declares a durable
> display-unit preference for a `let`/`param` binding (the `@display("unit")`
> annotation, §3), how derived-unit names are curated and registered (§4), the
> magnitude auto-scaling / engineering-notation policy (§5), and the precedence order
> and formatter-ownership rule (§6) that unify the places a `Value` is rendered for a
> human: eval `Value` Display (CLI), the GUI parameter cell, LSP hover, and string
> interpolation.

---

## §1 — Consumer & user-observable surface (G1)

**Concrete G1 consumer.** A GUI Parameters-panel cell for a source `let` binding
carrying a durable display-unit preference, rendering in the author's chosen unit **the
moment the file loads** — before any picker interaction. Worked scenario:

```
structure def LitterTray {
    param length : Length = 400mm
    param width  : Length = 350mm
    param height : Length = 50mm

    @display("L")
    let capacity : Volume = length * width * height
}
```

`capacity`'s SI value is `0.4 * 0.35 * 0.05 = 0.007` (m³).

**G1 acceptance:** once the leaves filed in §7 land, opening this file's Parameters
panel shows the `capacity` cell as **`7.0 L`** — not `7000000 mm³` (today's GUI
default; see §2c) and not `0.007 m³` (today's CLI/hover default; see §2b) — with zero
clicks. The already-shipped GUI picker (task #5199) remains available to override that
cell to a different Volume-ladder rung, per-cell and client-local, without touching
source (§6 precedence).

Note: the repo already has a `LitterTray` structure fixture at `examples/litter_tray.ri`
(task #5201, `rounded_box`/`rounded_rect` geometry primitives — a different concern). It
has no `capacity` binding and its dimensions (480×260×80mm) do not reduce to a round
liter figure, so it is cited here only as prior art that the domain object is already
real in the codebase; the worked example above uses its own illustrative dimensions
chosen to land exactly on the task's target `7.0 L` figure.

**This task ships no runtime code.** #5200's own observable output is this committed
PRD plus the leaf tasks filed in §7. The G1 scenario above becomes true only once those
leaves land — it is the acceptance bar *they* are held to, named here so every decision
in §3–§6 is judged against one concrete rendering instead of an abstract preference.

---

## §2 — Premise correction — what exists today (empirically verified)

### (a) The annotation channel — real, proven, and where `@display` plugs in

- Parser-level carrier: `Annotation { name: String, args: Vec<Expr>, span: SourceSpan }`
  — `crates/reify-ast/src/decl.rs:1187`. Both `ParamDecl.annotations: Vec<Annotation>`
  (`decl.rs:239`) and `LetDecl.annotations: Vec<Annotation>` (`decl.rs:262`) carry it —
  no new grammar is needed for a `let`/`param`-scoped annotation.
- Proven working today by shipped annotations `@test`, `@solver_hint`, `@deprecated`
  (plus `@optimized`, `@shell`, `@solid`, `@test_eval`, `@version`).
- Compiled/validated via a declarative schema registry:
  `crates/reify-compiler/src/annotations/schema.rs`, a `const SCHEMAS:
  &[AnnotationSchema]` (`schema.rs:130`) dispatched by `validate_via_schema`. Each entry
  declares `name`, `label`, `valid_contexts: &[&str]`, an optional bespoke
  `arg_check: Option<fn(&Annotation, &str, &mut Vec<Diagnostic>)>` shape-checker, and
  emits `reify_core::Diagnostic`/`DiagnosticLabel`. `@solver_hint` is already registered
  valid on exactly `["structure", "occurrence", "param", "let"]` (`schema.rs:161-170`)
  — the same `param`/`let` contexts `@display` needs. `@shell`'s `check_shell_args` is
  the concrete bespoke-arg-check precedent: match `ann.args.as_slice()` against the
  expected shape, push a `Diagnostic::warning`/`error` with
  `DiagnosticLabel::new(ann.span, ...)` on mismatch. **No `"display"`-named entry
  exists in `SCHEMAS` today** (confirmed by direct inspection: test / deprecated /
  optimized / solver_hint / shell / solid / test_eval / version — no display), and no
  code path anywhere constructs a display-preference value from an `Annotation`. This
  registry, and its governing PRD `docs/prds/annotation-args.md`, is the concrete seam
  §3 designs into.
- Caveat for §3: `arg_check` only sees the annotation's own args plus `context` (the
  declaration kind, as a string) — it has no access to the binding's resolved
  dimension/type. A shape check ("`@display` takes exactly one String arg") fits this
  hook directly; the *semantic* check ("does `\"L\"`'s dimension match `capacity`'s
  `Volume` type?") necessarily happens later, once the binding's dimension is resolved
  — a distinct validation pass, not a same-hook extension.

### (b) Three different Rust value-formatting functions exist today — not one, and not four

The task brief names "four surfaces" (eval Display, GUI cell, LSP hover, string
interpolation). Empirically these reduce to **three independent Rust functions**, two
of which independently reinvent a curated/SI split:

1. **`impl std::fmt::Display for Value`** (`crates/reify-ir/src/value.rs:3431`, `Scalar`
   arm at `:3447-3452`) — `write!(f, "{} {}", si_value, dimension)`: the **raw,
   unscaled** SI magnitude plus `DimensionVector`'s own `Display`
   (`crates/reify-core/src/dimension.rs:562-591`), which composes a product of
   base-SI symbols from a fixed table (`["m","kg","s","A","K","mol","cd","rad","sr","USD"]`)
   — e.g. Pressure prints as a composed `"kg·m^-1·s^-2"`-shaped string, never `"Pa"`,
   never `"SI"`. **This is the surface `reify eval` (CLI) uses**:
   `crates/reify-cli/src/main.rs:1599`, `println!("{} = {}", id, v)`.
2. **`Value::format_hover`** (`value.rs:2324-2340`) — raw unscaled `si_value` +
   `dimension_unit_label`'s curated label (`value.rs:2764-2782`): `"m"` / `"m²"` /
   `"m³"` / `"kg"` / `"rad"` / `"USD"` / `""` for Length/Area/Volume/Mass/Angle/Money/
   dimensionless, else the **literal string `"SI"`** (Pressure, Density, Force, Energy,
   Power, … all say `"SI"`). **This is the surface LSP hover uses**:
   `crates/reify-lsp/src/analysis.rs:464-466`, `format_value` delegates straight to
   `format_hover`.
3. **`Value::format_display_pair`** (`value.rs`, canonical impl `~2680-2701`) —
   **scales** the magnitude via `DimensionVector::to_display_units`
   (`crates/reify-core/src/dimension.rs:424-440`: Length→mm ×1e3, Angle→deg,
   Area→mm² ×1e6, Volume→mm³ ×1e9, Money→USD ×1, dimensionless→`""`, else raw SI value
   + literal `"SI"`) and returns `(scaled_value, label)`. **This is the surface both
   the GUI cell and string interpolation use**: the GUI wrapper
   `gui/src-tauri/src/types.rs:1299` (`format_value` delegates to `format_display_pair`)
   and `crates/reify-expr/src/lib.rs:1934-1943`'s `interp_render` (its own
   integration-test docstring: *"must render as `5 mm` via `format_display_pair`, not
   Display"*).

Net: #1 never says `"SI"` (it always composes real symbols) while #2 and #3 do, for the
identical set of non-curated dimensions; #2 and #3 additionally disagree on
**magnitude** for the same curated dimensions (#2 shows raw SI, #3 shows
engineering-scaled). So today, the same `Value::Scalar` for a 0.08 m length renders as
`"0.08 m"` (CLI), `"0.08 m"` (hover — same label, still raw magnitude), and `"80 mm"`
(GUI cell / interpolation) — three different strings for one value, none reading any
source-level preference.

### (c) The 5199 ladder registry is a fourth, independent source of curated names — reused as this PRD's registry substrate

- `unit_ladders() -> Vec<DimensionLadder>` (`gui/src-tauri/src/display_units.rs:46-189`);
  `DimensionLadder { dimension: String, units: Vec<UnitOption> }`;
  `UnitOption { label: String, si_scale: f64, is_default: bool }`
  (`display_magnitude = si_value / si_scale`). Covers exactly 7 dimensions: Length,
  Area, Volume, Angle, Mass, Pressure, Density. Exposed to the frontend via the
  `get_unit_ladders` Tauri command (`main.rs`).
- Self-earmarked for this task: the module doc (`display_units.rs:1-14`) states
  verbatim: *"Doubles as the future substrate for auto-scaling defaults and the DSL
  `@display` annotation follow-up (task #5200)."*
- **Confirmed deliberate divergence from (b)#3** — not an oversight. Mass/Pressure/
  Density's `is_default` rungs carry the curated labels `"kg"` / `"Pa"` / `"kg/m³"`
  (`si_scale: 1.0`), while `to_display_units` — the very function the ladder pins its
  numeric value against (`default_si_scale_matches_to_display_units_numeric_value`,
  `display_units.rs:327-361`) — gives the identical magnitude the generic label
  `"SI"`. The ladder's own test doc (`display_units.rs:320-325`) says so explicitly:
  *"that label divergence is deliberate, not drift."* Net effect today: the same
  Pressure value shows `"101325 SI"` everywhere in (b), but `"101.325 kPa"` (or
  whichever rung) the instant a user opens the GUI picker for that cell — because only
  the picker reads this table.
- The picker's own override precedence (`gui/src/panels/PropertyEditor.tsx:109-131`,
  `chosenOptionFor`): **in-memory pick this session** (`selectedUnits`, a Solid signal
  keyed by `cell_id`) → **localStorage-persisted prior pick** (`persistedUnits`,
  snapshotted once at mount) → **the ladder's `is_default` rung**. This state lives
  entirely client-side (memory + browser `localStorage`, keyed per cell); it is never
  written back into the `.ri` source, and nothing in
  `reify-compiler`/`reify-ir`/`reify-lsp` observes it.

### (d) Net premise

Five uncoordinated "what unit does this show in" answers exist on the base branch —
three backend Rust functions ((b)#1–#3: two different curated-label sets, two
different magnitude-scaling conventions) plus one frontend-only ladder table with a
third curated-label set (c) plus one frontend-only per-cell override store layered on
top of it — and **none of the five reads anything from `.ri` source**. The annotation
channel that would let source express a preference exists and is proven (a), but no
`"display"` schema entry or downstream consumer exists yet. §3–§6 design the missing
piece and the reconciliation; §7 files the leaves that build it.

---

## §3 — Source-level display-unit preference — surface decision

**Candidates.**

- **(A) `@display("L")` annotation** on `let`/`param`, carried by the existing
  `Vec<Annotation>` decl channel (§2a) — parses today with zero grammar changes as
  `Annotation { name: "display", args: [Expr::StringLit("L")], .. }`.
- **(B) type-level syntax**, e.g. `let capacity : Volume in L` — a new `TypeExpr`
  production coupling the dimension type to a display concern.

**DECISION: (A), the `@display` annotation, is the primary source surface.**

Rationale:
- The channel already exists and is proven (§2a) — no parser/grammar change, no new
  `TypeExpr` variant, no ripple into every place that pattern-matches on `TypeExpr`.
- Display preference is a *metadata* concern, not a *structural* one. Two bindings
  that are both `Volume` remain the same type regardless of which unit they render in
  — keeping that in an annotation means type comparison, unification, and function
  signatures never have to reason about a caller's display preference. (B) would
  entangle the two: a `Volume` display-tagged `in L` and a `Volume` display-tagged
  `in cm³` would need to either be the same `TypeExpr` (so the tag is decorative only,
  weakening the case for new grammar in the first place) or different ones (so a
  function accepting `Volume` would need to reconcile mismatched display tags at every
  call site).
- The annotation's single string argument matches a ladder rung's `label` (§2c)
  directly — no new concept, just a new consumer of an existing `UnitOption.label`.

**Annotation contract:**

- **Name & registration:** `display`, registered in `crates/reify-compiler/src/annotations/schema.rs`'s
  `SCHEMAS` (§2a) — a new `AnnotationSchema` entry alongside `@solver_hint`/`@shell`,
  following the `reify_core::*_ANNOTATION` naming precedent (`TEST_ANNOTATION`,
  `SOLVER_HINT_ANNOTATION`, `DEPRECATED_ANNOTATION`).
- **Valid contexts:** `["param", "let"]` — matches `@solver_hint`'s exact param/let
  subset (`schema.rs:161-170`); NOT valid on `structure`/`occurrence`/`function`/
  `constraint_def`, since display preference is a per-binding-*value* concern, not a
  declaration-kind concern.
- **Arg shape:** exactly one required positional argument, `ArgType::String`,
  `EvalTime::CompileConst` (the label is compared against a static registry, not a
  runtime value). Enforced by a bespoke `arg_check: Some(check_display_args)` following
  the `check_shell_args` precedent (§2a): match `ann.args.as_slice()`, push a
  `Diagnostic` on mismatch.
- **Diagnostics — two independent failure modes**, per the project convention that
  parser-contract violations produce diagnostics rather than silently misbehaving:
  1. **Shape violation** (wrong arg count/type, e.g. `@display(5)` or
     `@display("L", "cm")`) — caught by `arg_check` at schema-validation time.
     Severity Warning, matching every existing `arg_check` entry's
     `on_extra: ExtraArgsPolicy::WarnIgnore` convention (a malformed `@display`
     degrades to "ignored," falling through to the default rung, rather than aborting
     compilation).
  2. **Dimension mismatch** (well-formed but semantically wrong — `@display("L")` on a
     `Length`-typed binding, or a label matching no rung in *any* dimension's ladder)
     — this cannot be caught by `arg_check` (§2a caveat: no binding-type context
     there). It is caught once the binding's dimension is resolved
     (type-checking/dimension-resolution phase), as an **Error**-severity diagnostic —
     unlike the shape case, there is no sensible "ignore and continue" for a display
     unit that cannot apply to its own binding's dimension. The message names both the
     offending label and the binding's actual dimension by name, mirroring
     `docs/prds/money-dimension.md`'s dimension-mismatch-diagnostic rule (render
     user-visible names, never raw exponent vectors).
- **(B) is explicitly declined as the primary surface** but recorded as a possible
  future ergonomic sugar — a `TypeExpr`-level shorthand that desugars to the same
  `@display` annotation at parse time. Not designed further here and out of scope for
  the leaves filed in §7.

---

## §4 — Derived-unit names — curated vs composed

**Question:** curated derived-unit names (`Pa`, `N`, `J`, `W`, `kg/m³`, …) vs composed
dimension symbols (`kg·m^-1·s^-2`)?

**DECISION: a single curated derived-unit registry, keyed by canonical
`DimensionVector`** (via `canonical_name()` / `NAMED_DIMENSIONS`,
`crates/reify-core/src/dimension.rs:479-560`), reconciling `to_display_units`'s /
`dimension_unit_label`'s `"SI"` fallback (§2b) with 5199's already-curated `"Pa"` /
`"kg/m³"` (§2c).

- **(a) The registry IS the 5199 ladder table (`unit_ladders()`), extended** — not a
  second, freshly invented table. Each `DimensionLadder`'s default `UnitOption`
  becomes the single source of the curated label for that dimension (§7 leaf L1
  relocates and extends this table; §6 covers where it must live to be reachable from
  eval/LSP).
- **(b) Composed symbols (`DimensionVector::Display`, §2b#1) remain the fallback ONLY
  for dimensions with no curated registry entry** — so nothing currently working
  regresses: a dimension the registry hasn't named yet still renders as a real
  composed-symbol string (today's CLI behavior), never a bare `"SI"` token and never a
  crash.
- **(c) All consumers read this ONE registry.** Eval Display (CLI), `format_hover`
  (LSP), and `format_display_pair` (GUI cell + interpolation) — the three divergent
  functions in §2b — must all resolve a dimension's label from the shared registry
  instead of their own independent curated sets. This is what makes Pressure render as
  `"Pa"` uniformly in eval/hover *and* the GUI cell, closing the §2c divergence where
  the same value shows `"101325 SI"` in three places and `"101.325 kPa"` only behind
  the picker.
- **Initial curated set to seed:** the 7 already-curated ladder dimensions — Length,
  Area, Volume, Angle, Mass, Pressure, Density — plus **Force → `"N"`, Energy → `"J"`,
  Power → `"W"`**, named explicitly by the task. All three already exist as
  `DimensionVector` consts with `NAMED_DIMENSIONS` entries (`dimension.rs:493-495`), so
  adding ladder rungs for them is additive — no new dimension plumbing, only new
  `DimensionLadder` table rows (§7 leaf L1).
- Composed-symbol fallback (b) stays in force for the ~24 other `NAMED_DIMENSIONS`
  entries not in the initial curated set (Frequency, Voltage, Charge, Stiffness, …) —
  growing the curated set further is future ladder work, not a blocker for this PRD's
  leaves.

---

## §5 — Magnitude auto-scaling / engineering-notation policy

**Question:** which defaults auto-scale — opt-in or default-on, and for which
dimensions?

- **(a) Per-dimension policy, sourced from the same registry (§4).** Each ladder
  entry can declare whether auto-scaling across its rungs is enabled, and the target
  magnitude band — conventionally `1 ≤ |mantissa| < 1000`, one SI-prefix step.
- **(b) Default posture — split by whether the dimension's default rung is already
  scaled:**
  - **Default-ON** for the length-family dimensions with a full multi-rung ladder
    already spanning that band: **Length, Area, Volume**. (**Angle is excluded** —
    deg/rad is a discrete either/or choice, not a magnitude-driven rung ladder, so
    auto-scaling has nothing to act on.)
  - **Default-OFF / opt-in** for dimensions whose curated default is the bare SI base
    unit: **Mass, Pressure, Density**. Auto-scaling these by default would mean a
    value silently flips units as it crosses a magnitude threshold — e.g. `"2.5 kg"`
    becoming `"2500 g"`, or a Pressure value hopping Pa→kPa→MPa→GPa — which is a
    surprise for a value the author is used to seeing in one unit.
  - This split is not arbitrary: it mirrors the ladder's *existing* default-rung
    choices (§2c) — Length/Area/Volume already default to a scaled rung (mm/mm²/mm³);
    Mass/Pressure/Density already default to the raw SI base rung. Auto-scaling's
    default posture follows the same line the ladder already drew.
- **(c) Engineering notation** (powers-of-3 exponent, e.g. rendering `4.2e-3` in a
  `4.2×10⁻³`-style form) is the formatting **mode** used when auto-scaling is enabled
  for a dimension but no ladder rung keeps the mantissa in-band (e.g. a Length small
  enough that no rung — mm, cm, m, in — lands in `[1,1000)`). It is a fallback *within*
  the auto-scaling policy, not a separate feature with its own on/off switch.
- **(d) Interaction rule: an explicit unit pin suppresses auto-scaling.** Whenever a
  unit is pinned explicitly — by `@display` (§3) or by the GUI picker (§6) — that pin
  is authoritative and stable regardless of magnitude; auto-scaling only acts on a cell
  with **no** explicit pin (i.e. one still showing its ladder's bare default rung).
  Concrete tie-in to the G1 example (§1): `capacity` is pinned to `"L"` via
  `@display("L")`. Even if a later edit changed `length`/`width`/`height` so
  `capacity` became `0.0007 m³` or `0.7 m³`, the cell stays in liters (`"0.7 L"` /
  `"700 L"`) — auto-scaling never overrides an explicit pin to hop it to mL or m³.

---

## §6 — Precedence order & formatter ownership

### (1) Precedence

**DECISION — total order, highest first:**

1. **Source `@display` annotation** (durable, in-source default — §3).
2. **GUI picker override** (per-cell, client-local: in-memory this session, else
   localStorage-persisted prior pick — §2c). **Never written back to source.**
3. **Ladder default rung** (`is_default` — §2c/§4).
4. **Curated derived-unit name** (§4) where no ladder rung applies but a curated name
   exists.
5. **Composed-symbol fallback** (§2b#1/§4b) — last resort, for a dimension with
   neither a ladder nor a curated name.

**How annotation and picker interact:** `@display` sets the *default* a cell renders
at — what loading the file with zero interaction shows (the G1 acceptance bar, §1).
The GUI picker is a *session/client override* layered on top of it: picking a
different rung in the GUI does not rewrite the `@display("L")` in source. Reloading
the file — or opening it in a different browser profile/machine without that
`localStorage` entry — reverts the cell to the annotation's rung. If a binding carries
no `@display`, the picker's own baseline is unchanged from today: the ladder default
rung (rung 3). **Auto-scaling (§5) only acts at rung 3** — the instant rung 1 or rung
2 pins an explicit unit, auto-scaling is suppressed (§5d).

### (2) Formatter ownership

**DECISION: one shared formatter owns all four surfaces** — CLI/eval `Value` Display,
the GUI cell (`format_display_pair`), LSP hover (`format_hover`), and
`__interp_render` — replacing the three divergent functions cataloged in §2b.

**API seam sketch:**

```
fn resolve_display(
    si_value: f64,
    dimension: &DimensionVector,
    preference: Option<&DisplayPreference>,  // the rung that already won §6(1)'s order
) -> (String, String)                        // (formatted_magnitude, unit_label)
```

- **Inputs:** `si_value` + `DimensionVector` (what every current function already
  takes), plus a resolved `preference` carrying whichever rung won the §6(1)
  precedence order for this call site. The formatter does not re-run precedence
  itself — the caller resolves `@display` vs GUI-picker vs "none" and passes the
  result in; with `preference: None`, the formatter falls through §6(1) rungs 3–5
  internally (ladder default → curated name → composed symbol).
- **Output** mirrors `format_display_pair`'s existing `(String, String)` shape, so the
  GUI and interpolation call sites need no shape change — only the CLI (§2b#1,
  currently `Display`-trait-based) and LSP hover (§2b#2, currently
  `format_hover`-based) call sites change to call the shared function instead of their
  own.

**Named blocker (implementation-leaf concern, not decided here):** `display_units.rs`
— and the `unit_ladders()` registry this PRD reuses (§2c/§4a) — lives in
`gui/src-tauri`, which `reify-ir`/`reify-expr`/`reify-lsp` cannot depend on (the GUI is
a leaf consumer of the compiler/eval stack, not a dependency of it). The registry must
relocate to a crate all four surfaces can reach. `crates/reify-core` is the natural
candidate: `DimensionVector`, `to_display_units`, `NAMED_DIMENSIONS`, and
`canonical_name` already live there, and its own `Cargo.toml` describes it as a "leaf
crate" with near-zero dependencies (the intentional dependency floor per
`docs/prds/core-ast-ir-layering.md` task γ). The exact target crate and migration
shape is left to the owning leaf (§7 L1/L3) — not fixed here.
