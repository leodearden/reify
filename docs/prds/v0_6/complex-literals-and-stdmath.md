# Complex Literals & `std.math.complex` Breadth

**Status:** deferred (spec-gap-filling batch `spec-gap-2026-05-27`) · **Milestone:** v0.6 · **Authored:** 2026-05-27
**Approach:** bare B (vertical slices) + a light contract table for the imaginary-literal grammar seam. See §G5 note.
**Cluster:** `complex-literals-and-stdmath`

## 1. Goal

Close two spec gaps around complex numbers:

- **Imaginary-literal sugar** (spec §18.15 deferred-feature #15, `3.2 + 4.1j`). Today the only
  way to build a `Complex` value is the `complex(re, im)` constructor call. After this PRD lands,
  a user writing a `.ri` file can write `4.1j` for the pure-imaginary value and `3.2 + 4.1j` for
  a full complex literal, and have it evaluate to `Value::Complex`.
- **`std.math.complex` mathematical breadth** (spec §11.1 module tree; §11.3 summary promises
  "complex numbers" under `std.math`). The `Complex<Q>` *type*, the `complex` constructor, and
  the accessors `real`/`imag`/`conjugate`/`complex_magnitude`/`phase` already exist and are
  documented in `docs/reify-stdlib-reference.md` §1.4. What is missing is the **function breadth a
  real complex-number library needs**: spec-aligned names `abs`/`arg`, complex division, and the
  transcendental/power functions (`complex_exp`, `complex_sqrt`, `complex_pow`). After this PRD
  lands, those are callable from `.ri` and documented in the stdlib reference.

The user-observable surface is the CLI evaluator: `reify eval <file>` on a `.ri` file using the new
literal sugar and the new functions prints the expected `Complex` / `Scalar` / `Real` values.

## 2. Background: what exists, what's missing

The complex stack is **already substantially built**. The audit of the current tree found:

| Layer | Complex support today | Source |
|---|---|---|
| `Type::Complex(Box<Type>)` | YES — dimensioned, `Display` = `Complex<Q>` | `reify-core/src/ty.rs:97` |
| `Value::Complex { re, im, dimension }` | YES — re/im share one `DimensionVector` | `reify-ir/src/value.rs:513` |
| Constructor + accessors | YES — `complex`, `re`/`real`, `im`/`imag`, `conjugate`, `phase`, `complex_magnitude`, `complex_add`, `complex_mul` (Rust builtins, dispatched by name via `eval_builtin`) | `reify-stdlib/src/complex.rs` |
| Method-path duplicates | YES — `.magnitude`/`.phase`/`.conjugate`/`.re`/`.im` (see §6 note on method syntax) | `reify-expr/src/complex.rs` |
| Binary `+`/`-`/`*`/`/` on `Complex` | PARTIAL — `Complex op Complex` for `+`/`-`/`*`; `Complex * Real`, `Complex / Real`; **NO `Real ± Complex`** | `reify-expr/src/lib.rs:2002,2070,2185,2279,2540` |
| Unary `-` on `Complex` | YES | `reify-expr/src/lib.rs:1857` |
| Stdlib-reference §1.4 doc | YES — documents type + 6 fns | `docs/reify-stdlib-reference.md:145-158` |
| **Imaginary literal `4.1j`** | **NO** — misparses (§2.1) | `tree-sitter-reify/grammar.js:1062` |
| **`abs`/`arg` on Complex (spec names)** | **NO** — `abs` handles only Int/Real/Scalar; `arg` unbound | `reify-stdlib/src/numeric.rs:8` |
| **`Real ± Complex` promotion** | **NO** — `eval_add`/`eval_sub` lack the mixed arm → `3.2 + 4.1j` ⇒ `Undef` | `reify-expr/src/lib.rs:2002-2042` |
| **Complex division (`/`) for Complex / Complex** | **NO** — only `Complex / Real` exists | `reify-expr/src/lib.rs:2540` |
| **`complex_exp`/`complex_sqrt`/`complex_pow`** | **NO** | — |

So this PRD is mostly **filling out an existing, well-tested kernel**, plus one genuinely-new
grammar production (the imaginary suffix).

### 2.1 The silent-misparse trap (motivates `grammar_confirmed=false`)

`tree-sitter parse --quiet` exits **0** on `4.1j`, `2j`, and `4.1i` today — but the CST is wrong.
The external scanner's `_unit_expr_start` (matches `[A-Za-z_(]` immediately after a number) fires on
the `j`, so the parser reads `4.1j` as a **quantity literal** `4.1` with a `unit_name` of `j`:

```
4.1j   →  quantity_literal{ value: number_literal "4.1", unit: unit_expr{ unit_name "j" } }
2j     →  quantity_literal{ value: number_literal "2",   unit: unit_expr{ unit_name "j" } }
```

i.e. `4.1` *joules*-or-whatever-unit-`j`-resolves-to, not the imaginary value `4.1i`. A naive
G3 "exit 0 = parses" check passes; the feature is still absent and, worse, the input means
something else entirely. The imaginary-literal grammar task therefore carries
`grammar_confirmed=false` and its observable signal asserts the **CST node kind**
(`imaginary_literal`, not `quantity_literal`), not merely exit 0.

This is the **same `number_literal`/`quantity_literal` collision** that the sibling PRD
`numeric-and-range-literal-forms.md` (§2.1) works in for digit separators and hex/binary — see
§6 cross-PRD seam.

## 3. Resolved design decisions

`AskUserQuestion` was unreachable in the authoring environment, so each load-bearing fork below is
a reasoned default, **flagged for Leo in `## DESIGN FORKS FOR LEO`**.

### D1 — Imaginary suffix is `j` only (not `i`).

`4.1j`, `2j`, `1.5e-3j` are imaginary literals; `4.1i` is **not** (it stays an identifier-adjacency
case and remains a parse-disambiguation hazard). Spec §18.15's own example uses `j` (`3.2 + 4.1j`).

- **Rationale:** `j` is the EE convention (and the spec example). Supporting `i` as well doubles the
  collision surface: `i` is by far the most common loop/index identifier, and `2i` vs an `i`-suffixed
  unit (none today, but units are open-namespace) is a worse foot-gun than `j`. Picking one suffix
  keeps the grammar production unambiguous. Fork F1 lets Leo add `i` if desired.

### D2 — Imaginary literals are **dimensionless**.

`4.1j` evaluates to `Value::Complex { re: 0.0, im: 4.1, dimension: DIMENSIONLESS }`, i.e.
`Complex<Real>`. There is **no dimensioned imaginary literal** (`4.1mmj` is out of scope and remains
a parse error / quantity-misparse). Dimensioned complex values are built compositionally via the
existing `complex(re_q, im_q)` constructor, e.g. `complex(0ohm, 4.1ohm)` for a reactance, or
`r + complex(0mm, 4.1mm)`.

- **Rationale:** spec §3.6 requires re and im to share a dimension. A bare suffix literal has nowhere
  to put a unit without re-opening the `4.1mmj` collision (now a *three*-way scanner fight between
  unit-start, the `j` suffix, and `m`). Dimensionless-only keeps the grammar a clean token-level
  alternative and matches every other language's imaginary literal (Python `4.1j`, Julia `4.1im` are
  dimensionless). The constructor already covers the dimensioned case losslessly. Fork F2.

### D3 — `3.2 + 4.1j` works via `Real ± Complex` promotion in `eval_add`/`eval_sub`, NOT a fused literal.

`3.2 + 4.1j` parses as `binary_expression{ Real(3.2) '+' imaginary_literal(4.1j) }` and evaluates by
promoting the `Real` operand to `Complex { re: 3.2, im: 0, dimension: DIMENSIONLESS }` and adding.
There is **no fused `complex_literal` grammar node** combining real + imaginary parts.

- **Rationale:** `3.2 + 4.1j` is already two tokens joined by `+`; making the parser fuse them would
  require lookahead across the `+` and special-case `re_literal + im_literal` while leaving
  `x + 4.1j` (variable real part) to the eval path anyway. Far cleaner to make the **eval path** the
  single source of truth: add `Real ± Complex` / `Int ± Complex` arms (promote the scalar to a
  dimensionless complex), and the literal case falls out for free. This also fixes the **pre-existing
  gap** that `3.2 + complex(0, 4.1)` is `Undef` today (§2). The promotion is dimensionless-only:
  `5mm + 4.1j` is a dimension mismatch ⇒ `Undef` (a dimensionless imaginary cannot add to a
  length). Fork F3 covers whether mixed-dimension promotion should diagnose vs silently `Undef`.

### D4 — Spec-name functions `abs` and `arg` are **added as the canonical surface**; existing names kept as aliases.

Spec §11.3 lists `abs` under `std.math`; the universal complex-analysis names are `abs(z)` (modulus)
and `arg(z)` (argument/phase). This PRD:

- Extends the existing `abs` builtin (`numeric.rs:8`) with a `Value::Complex` arm delegating to
  `complex_abs` — so `abs(3+4j) == 5`. `magnitude` (the `linalg.rs` vector builtin) gets the same
  Complex arm for symmetry.
- Adds `arg` as an alias of the existing `phase` (returns `Angle`).
- **Keeps** `complex_magnitude` and `phase` as documented aliases (no removal — they appear in
  shipped examples like `examples/linalg.ri` and the §1.4 reference).

- **Rationale:** `abs`/`arg` are what a user reaches for and what the spec's `std.math` summary
  implies; not wiring them is the live gap (`abs(3+4j)` ⇒ `Undef` today). Aliasing rather than
  renaming avoids breaking the shipped surface. Fork F4 covers whether to *deprecate* the long
  names.

### D5 — Complex transcendentals: ship `complex_exp`, `complex_sqrt`, `complex_pow`, complex `/`. Defer trig.

- `complex_exp(z) = e^re · (cos(im) + i·sin(im))` — **dimensionless input only** (the exponent of a
  dimensioned quantity is dimensionally meaningless), returns `Complex<Real>`.
- `complex_sqrt(z)` — principal square root; **dimensionless input only** for v0.6 (a dimensioned
  `sqrt` would need `Q^(1/2)`, which the complex dimension model doesn't carry cleanly — defer to a
  future dimensional-complex PRD).
- `complex_pow(z, n: Int)` — integer power by repeated multiplication; dimension composes as
  `Q^n` (consistent with the existing `complex_mul` dimension rule). Real/complex exponents deferred.
- Complex division `Complex / Complex`: `(a+bi)/(c+di) = ((ac+bd) + (bc-ad)i)/(c²+d²)`; dimension
  divides (`ad.div(bd)`); division by `0+0i` ⇒ `Undef`. (Complex `/` Real already exists.)
- **Deferred:** `complex_sin`/`cos`/`log`/`asin`/… and dimensioned `sqrt`/`pow`. Named in §8.

- **Rationale:** `exp`, `sqrt`, integer `pow`, and division are the functions modal-analysis /
  impedance / control-systems consumers (§G1) actually call. The full transcendental suite is large
  and lower-value; deferring keeps the batch tight. Fork F5 lets Leo pull trig forward.

### D6 — `std.math.complex` is documented as a **stdlib reference module**, not a new `.ri` file.

The complex functions are Rust builtins reached by name through `eval_builtin` (no `.ri` declaration
needed — an unrecognized free-function call lowers to a stdlib `FunctionCall` that the runtime
resolves). This PRD **does not** add a `crates/reify-compiler/stdlib/complex.ri`; it extends the
Rust builtins and updates `docs/reify-stdlib-reference.md` §1.4 to list the new functions.

- **Rationale:** matches how `std.math.numeric`/`trig`/`linalg` already work (all Rust builtins, all
  documented in §1.4's siblings; none have a `.ri` module). Adding a `.ri` shim would be inert. The
  "module" is a documentation/namespace concept, realized by the Rust dispatch + the reference doc.

## 4. Sketch of approach

```reify
// Imaginary-literal sugar (slice 1)
let pure_imag = 4.1j                 // Complex { re: 0, im: 4.1 } (dimensionless)
let z         = 3.2 + 4.1j           // Complex { re: 3.2, im: 4.1 }
let z2        = 3 + 4j               // Complex { re: 3, im: 4 }

// Spec-name accessors / modulus / argument (slice 2)
let m   = abs(z2)                    // Scalar/Real 5.0
let a   = arg(z2)                    // Angle atan2(4,3)
let zc  = conjugate(z2)              // 3 - 4j

// Transcendentals + division (slice 3)
let e   = complex_exp(0j)            // 1 + 0j
let q   = z2 / complex(1.0, 1.0)     // complex division
let zp  = complex_pow(z2, 2)         // z2 squared
let s   = complex_sqrt(complex(-1.0, 0.0))   // 0 + 1j (principal root)
```

Dimensioned complex stays constructor-built (no literal sugar):

```reify
let impedance = complex(50ohm, -30ohm)   // Complex<Resistance>; re/im share ohm
let reactance = imag(impedance)          // -30ohm
```

## 5. Pre-conditions for activating

- **Cross-PRD grammar seam (hard prerequisite):** the imaginary-literal grammar task (α) must
  sequence **after** the `number_literal`-token edits in the sibling PRD
  `numeric-and-range-literal-forms.md`. See §6 for the exact task dependency. This is the only
  external pre-condition; the coordinator wires the edge.
- **No** GR-001 / ComputeNode / multi-kernel dependency. `Complex` is a leaf scalar value; the
  builtins are pure functions of `Value`.
- Slices 2 and 3 (stdlib breadth) are grammar-independent and can land before slice 1.

## 6. Cross-PRD relationship

One **cross-PRD grammar seam**, declared not wired (G4 — coordinator owns the edge).

The imaginary suffix `4.1j` is a numeric-literal form sharing the **same grammar collision** as the
sibling PRD's digit-separator and hex/binary work: the `_unit_expr_start` scanner greedily lexes the
trailing letters as a unit, so all four features (`_`, `0x`, `0b`, `j`) are alternatives/guards that
must coexist in the `number_literal` / `quantity_literal` region of `grammar.js` and require a
**serialized `tree-sitter generate`**. Landing them concurrently would produce a conflicting parser
regen (the generated `src/parser.c` is a single artifact).

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_6/numeric-and-range-literal-forms.md` | this PRD's grammar task **sequences after** theirs | `number_literal` token + `_unit_expr_start` collision in `tree-sitter-reify/grammar.js` + `src/parser.c` regen | **numeric-and-range owns the `number_literal` token shape**; this PRD adds an `imaginary_literal` alternative/sibling on top | seam declared — **coordinator wires edge** |

**Exact dependency to wire (named for the coordinator):**

> This PRD's **task α (imaginary-literal grammar)** `depends_on` sibling-PRD **task γ — "Grammar:
> hex (0x) and binary (0b) integer number_literal forms"** (task **3910**), AND sibling-PRD **task α
> — "Grammar: digit separators (_) in number_literal"** (task **3909**).

Rationale for both edges: 3910 (hex/binary) restructures the `number_literal` token into a
multi-alternative `choice`, which is the structure the imaginary suffix must extend or sit beside;
3909 (separators) changes the decimal-run regex that the imaginary mantissa (`1.5e-3j`) reuses. Task
ε (3911, range arms) does **not** touch `number_literal`, so no edge to it is required. Sequencing
after 3910 (the deepest `number_literal` edit) is sufficient for correctness; adding the 3909 edge
makes the ordering total and avoids a two-way regen race if 3909 lands last.

No other cross-PRD seams. The stdlib-breadth tasks (β–ε below) touch only `reify-stdlib`,
`reify-expr`, and `docs/reify-stdlib-reference.md`, disjoint from every sibling cluster.

## 7. Decomposition plan

Greek labels are PRD-internal; task IDs assigned at decompose time. "Crates touched" drives the G5
blast-radius read.

### Slice 1 — imaginary-literal sugar (grammar + lowering)

- **α — Grammar: `imaginary_literal` (`j` suffix on a number).**
  Crates: `tree-sitter-reify`. Add an `imaginary_literal` production (or `number_literal` alternative)
  matching a decimal/scientific mantissa immediately followed by `j` with **no further unit chars**
  (`4.1j`, `2j`, `1.5e-3j`), winning over `quantity_literal` for the bare-`j` case via token
  longest-match / precedence. Must NOT capture `4.1mm` (still quantity) or `4.1jk` (the scanner's
  unit path). `tree-sitter generate`. Corpus test `test/corpus/imaginary_literal.txt` pins `4.1j`,
  `2j`, `1.5e-3j` as `imaginary_literal` nodes (NOT `quantity_literal` — defeats the §2.1 misparse),
  and pins `4.1mm` as `quantity_literal` (regression guard).
  *Signal (intermediate):* unlocks β; corpus asserts `4.1j` is an `imaginary_literal` node, not
  `quantity_literal{unit:j}`. `grammar_confirmed=false`.
  *Prereqs:* **sibling 3910 + 3909 (cross-PRD, §6)** — coordinator-wired.

- **β — Lowering + eval: imaginary literal → `Complex`, and `Real ± Complex` promotion.**
  Crates: `reify-syntax`, `reify-expr` (+ `reify-compiler` if a new AST node is needed). Lower
  `imaginary_literal(x)` to `Complex { re: 0, im: x, dimension: DIMENSIONLESS }` (per D2). Add the
  `Real ± Complex` / `Int ± Complex` / `Complex ± Real` / `Complex ± Int` arms to `eval_add` and
  `eval_sub` in `reify-expr/src/lib.rs` (promote scalar → dimensionless complex; dimension mismatch
  ⇒ `Undef`, per D3). Leaf.
  *Signal (leaf):* `examples/complex_literals.ri` — `let z = 3.2 + 4.1j` then `re(z)`/`im(z)` —
  evaluated by `reify eval` prints `re == 3.2`, `im == 4.1`; and `3 + 4j` yields a `Complex` (not
  `Undef`). `reify-expr` unit test pins `eval_add(Real(3.2), Complex{0,4.1}) == Complex{3.2,4.1}`.
  *Prereqs:* α. `grammar_confirmed=false` (depends on grammar task).

### Slice 2 — spec-name modulus / argument (`abs`, `arg`) — grammar-independent

- **γ — `abs`/`magnitude` Complex arm + `arg` alias.**
  Crates: `reify-stdlib` (`numeric.rs`, `linalg.rs`, `complex.rs`). Add a `Value::Complex` arm to
  `abs` (numeric.rs) and `magnitude` (linalg.rs) delegating to `complex_abs(re, im, dim)` (returns
  `Real` if dimensionless, else `Scalar<Q>`). Add `arg` as a name alias for the existing `phase`
  arm (returns `Angle`). Keep `complex_magnitude`/`phase` (D4). Leaf.
  *Signal (leaf):* `examples/complex_abs_arg.ri` — `abs(complex(3.0,4.0))` and `arg(complex(3.0,4.0))`
  — via `reify eval` print `5.0` and `atan2(4,3)` (≈0.927 rad); `reify-stdlib` test pins
  `eval_builtin("abs", &[complex(3,4)]) == Real(5.0)` and `eval_builtin("arg", …) == phase(…)`.
  *Prereqs:* none. `grammar_confirmed=true` (no grammar change).

### Slice 3 — complex transcendentals + division — grammar-independent

- **δ — Complex division (`Complex / Complex`) + `complex_div` builtin.**
  Crates: `reify-expr` (`eval_div` arm), `reify-stdlib` (`complex.rs` — `complex_div` builtin for
  symmetry with `complex_add`/`complex_mul`). `(a+bi)/(c+di)` per D5; dimension divides; `/(0+0i)` ⇒
  `Undef`. Leaf.
  *Signal (leaf):* `examples/complex_div.ri` — `complex(1.0,0.0) / complex(0.0,1.0)` — via
  `reify eval` prints `0 - 1j` (i.e. `re==0, im==-1`); `reify-expr` test pins the formula and the
  divide-by-zero `Undef`.
  *Prereqs:* none. `grammar_confirmed=true`.

- **ε — `complex_exp`, `complex_sqrt`, `complex_pow(z, n: Int)` builtins.**
  Crates: `reify-stdlib` (`complex.rs`). Per D5: `complex_exp`/`complex_sqrt` dimensionless-only
  (else `Undef`); `complex_pow` integer exponent, dimension `Q^n` via repeated `complex_mul`. Leaf.
  *Signal (leaf):* `examples/complex_transcendental.ri` — `complex_exp(complex(0.0,0.0))`,
  `complex_sqrt(complex(-1.0,0.0))`, `complex_pow(complex(3.0,4.0), 2)` — via `reify eval` print
  `1+0j`, `0+1j`, `-7+24j` respectively; `reify-stdlib` tests pin each (incl. `complex_exp` of a
  dimensioned input ⇒ `Undef`).
  *Prereqs:* none. `grammar_confirmed=true`.

### Documentation / integration gate

- **ζ — Update `std.math.complex` reference (§1.4) + combined example.**
  Crates: `docs` + `examples` (+ wherever the example-corpus CI runner lives). Extend
  `docs/reify-stdlib-reference.md` §1.4 with `abs`/`arg`/`complex_div`/`complex_exp`/`complex_sqrt`/
  `complex_pow` signatures and a note that `4.1j` literal sugar exists (dimensionless). One
  `examples/complex_numbers.ri` exercising literal sugar + `abs`/`arg` + division + a transcendental
  in a single realistic structure (e.g. an RLC impedance `complex(50ohm, -30ohm)` with
  `abs`/`arg` readouts, plus a dimensionless `3+4j` worked example). Leaf; **integration-gate for
  the batch**.
  *Signal (leaf):* `examples/complex_numbers.ri` parses (exit 0, no ERROR) AND evaluates without
  diagnostics in the example-corpus CI test — the single end-to-end proof that the literal sugar and
  the new functions compose; §1.4 reference lists every new function (doc lint passes).
  *Prereqs:* β, γ, δ, ε. `grammar_confirmed=false` (transitively depends on grammar via β).

### DAG

```
(sibling 3910, 3909) ┄┄→ α ─→ β ─┐
γ ───────────────────────────────┤
δ ───────────────────────────────┼─→ ζ
ε ───────────────────────────────┘
```

Slice 1 (α→β) is the only grammar-gated chain; slices 2/3 (γ, δ, ε) are independent and can land in
any order; ζ is the integration gate that pulls them together. The `┄┄→` edge is the cross-PRD seam
the coordinator wires (§6).

## 8. Out of scope for this PRD

- **`i` imaginary suffix** (D1) — `j` only; `i` is a future fork (F1).
- **Dimensioned imaginary literals** (`4.1mmj`) (D2) — use `complex(re_q, im_q)`. Would re-open a
  three-way scanner collision; a future dimensional-imaginary-literal PRD could revisit.
- **Fused `complex_literal` grammar node** (D3) — `3.2 + 4.1j` is `Real + imaginary_literal` via the
  eval promotion path, not a single parser node.
- **Complex trig / log / inverse-trig** (`complex_sin`, `complex_cos`, `complex_log`, `complex_tan`,
  `complex_asin`, …) (D5) — deferred; ship exp/sqrt/integer-pow/division only.
- **Dimensioned `complex_sqrt` / non-integer `complex_pow`** (D5) — needs `Q^(1/2)` in the complex
  dimension model; deferred to a future dimensional-complex PRD.
- **`polar(magnitude, angle)` / `from_polar` constructor** — convenient but additive; a future
  small follow-up. Not blocking any §G1 consumer (they build via `complex(re, im)`).
- **Removing/deprecating `complex_magnitude` / `phase` long names** (D4) — kept as aliases; any
  deprecation is a separate doc/lint decision (fork F4).

## 9. Open questions (tactical — decide at impl time)

1. **`imaginary_literal` as a `number_literal` alternative vs a sibling `_primary_expression` rule.**
   Either works; the choice interacts with how 3910's hex/binary `choice` is structured (whichever
   lands first sets the shape). Decide during α, after reading the post-3910 `number_literal` rule.
2. **`reify eval` vs example-corpus CI harness for the `.ri` signals.** β/γ/δ/ε/ζ name "`reify eval`";
   confirm the exact subcommand the example-eval CI already uses during β (shared with the sibling
   PRD's open question #3).
3. **`complex_pow` negative-exponent handling.** D5 says integer `n`; whether `n < 0` (reciprocal via
   division) is in scope or `Undef` for v0.6 — tactical, decide during ε. Default: support `n < 0`
   via `complex_pow(z, -n) = 1 / complex_pow(z, n)` since complex `/` lands in δ (same batch).

## G5 note

Bare B + a light §6 seam table (no full contract section / boundary-test sketch). Blast radius:
α=`tree-sitter-reify`; β=`reify-syntax`+`reify-expr` (+maybe `reify-compiler`) — 2-3 crates, the only
task near the G5 threshold; γ/δ/ε=`reify-stdlib`(+`reify-expr` for δ) — 1-2 crates; ζ=docs+examples.
6 mechanisms but each is a thin slice over an **already-built, well-tested** kernel (the §2 table
shows the type/value/constructor/most-arithmetic already ship). One cross-PRD grammar seam, handled
by §6's declared dependency + coordinator wiring (approach E). No load-bearing engine seam (Complex
is a leaf scalar value; builtins are pure `Value→Value`). Full B+H contract not warranted.

## DESIGN FORKS FOR LEO

These were resolved with reasoned defaults (above) but are genuine forks — broker as needed:

- **F1 — Imaginary suffix `j` only, or also `i`?** Default: **`j` only** (D1; EE convention, spec
  example, lowest collision risk). Adding `i` doubles the identifier-adjacency hazard (`i` is the
  archetypal loop variable). If Leo wants Python/math-style `i`, α adds a second token alternative
  and the corpus test pins both.
- **F2 — Imaginary literals dimensionless only, or allow dimensioned (`4.1mmj`)?** Default:
  **dimensionless only** (D2); dimensioned complex via `complex(re_q, im_q)`. Dimensioned literal
  sugar re-opens a three-way scanner collision (unit-start vs `j` vs unit letters) for marginal
  benefit. Leo could greenlight a future dimensional-imaginary PRD instead.
- **F3 — `5mm + 4.1j` (mixed dimension): silent `Undef` or a diagnostic?** Default: **silent `Undef`**
  (D3), matching every other dimension-mismatch in `eval_add` today (they all return `Undef`, no
  diagnostic). A complex-specific diagnostic ("cannot add dimensionless imaginary to Length") would
  be friendlier but is inconsistent with the current arithmetic-error policy. Leo may want a
  follow-up to add diagnostics to *all* dimension-mismatch arithmetic, not just this one.
- **F4 — Deprecate `complex_magnitude`/`phase` in favour of `abs`/`arg`?** Default: **keep both as
  aliases** (D4), no deprecation — the long names ship in `examples/linalg.ri` and the §1.4
  reference. Deprecation would need a migration pass over examples/tests. Leo decides if the
  redundancy is worth a cleanup.
- **F5 — Pull complex trig/log forward into v0.6, or defer (D5)?** Default: **defer** — ship
  exp/sqrt/integer-pow/division only (the modal/impedance/control consumers' actual needs). The full
  transcendental suite is ~8 more builtins of lower value. Leo can expand ε if a consumer needs trig.
- **F6 — `std.math.complex` documentation-only module, or a real `complex.ri` stdlib file?** Default:
  **doc-only** (D6) — matches how every other `std.math.*` submodule works (all Rust builtins). A
  `.ri` shim would be inert given the name-dispatch architecture. Flag only if Leo wants stdlib
  modules to become real `.ri` files as a general direction.
