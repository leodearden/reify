# Module Declaration & Visibility Hardening

> Spec §7.1/§7.2 promise that "every `.ri` file must begin with a `module` declaration
> specifying its full path" and that "the declared path must match the file's location in
> the source tree (enforced by tooling)." Neither is true today: the `module` keyword is
> reserved (§17) but has **no grammar production**, so `module a.b.c` does not parse, and
> the compiler derives every module's path from the *filename / import dot-path*, never
> from an in-file declaration. There is therefore nothing to enforce. Separately, §7.4
> parks `priv` ("hidden parameters on public definitions not yet justified", §18 row 12) —
> today parameters and named sub-entities are *always* externally visible with no way to
> hide one. This PRD lands the `module` declaration + its path-vs-location check, and adds
> the `priv` modifier as the inverse axis of `pub`.

---

## §0 — Purpose & cohesion

**Purpose.** Two small module/visibility hardening gaps that share grammar-gate work and
the §7.4 visibility machinery:

1. **Module-declaration path enforcement (§7.1/§7.2).** Parse a top-of-file
   `module a.b.c` declaration and emit a diagnostic when the declared path disagrees with
   the path the file is resolved under (filename for single-file, import dot-path for DAG
   modules).
2. **`priv` modifier (§18 row 12 / §7.4).** Allow `priv param` / `priv sub` / `priv port`
   on a definition so an author can hide a member that is otherwise externally visible by
   default, including on a `pub` definition.

**Cohesion call — why these two and not three.** This PRD is the first half of the
"module & visibility hardening" cluster. Conditional compilation (§18 row 13) is the
second half and lives in a **separate PRD** (`v0_6/conditional-compilation.md`). The split
is grammar-driven: path-enforcement and `priv` **both require new tree-sitter grammar**
(a `module_declaration` production; a `priv` keyword on members), so they share the same
G3 grammar-prerequisite task and the same `optional('pub' | 'priv')` member-modifier
surface. Conditional compilation needs **no new grammar** — the `#cfg(...)` pragma form
already parses on the existing pragma rule — so coupling it here would chain a grammar-free
feature behind grammar-gated tasks for no benefit. See `conditional-compilation.md` §0 for
the reciprocal justification.

**This PRD introduces NO tuples and no method-call syntax** (per project grammar norms).

---

## §1 — Spec grounding & current-state evidence

- **§7.1 (verbatim):** "Every `.ri` file must begin with a `module` declaration specifying
  its full path. The declared path must match the file's location in the source tree
  (enforced by tooling)." Example: `module std.mechanical.fasteners.bolt` "must be located
  at `std/mechanical/fasteners/bolt.ri`".
- **§7.2:** "`module company.products.actuators` — One per file, at the top. Module path
  corresponds to file location."
- **§7.4:** members default to visible (params, named subs/ports) or private (`let`,
  constraints); `pub let` promotes a private. Last bullet: "No `priv` modifier in v0.1."
- **§17 keyword list:** `module` **is** a reserved keyword (one of the 46). `priv` is
  **not** — it must be added.
- **Current state (verified 2026-05-27):**
  - `tree-sitter-reify/grammar.js` has **no** `module_declaration` rule. `module a.b.c`
    parses to `(ERROR ...)` (confirmed via `tree-sitter parse`).
  - No `.ri` file under `examples/`, `stdlib/`, or the corpus begins with `module ` —
    the declaration is pure spec fiction at the syntax layer today.
  - `crates/reify-compiler/src/module_dag.rs` derives module path from the import
    dot-path (`ModulePath::from_dotted(module_path)`); `compile_project_with_entry_source`
    derives the entry path from `entry_path.file_stem()`; `reify-cli/src/main.rs`
    `parse_and_compile` uses `file_stem()` and `ModulePath::single(...)`. None reads a
    `module` declaration.
  - `crates/reify-ast/src/decl.rs`: `StructureDef`/`OccurrenceDef`/`TraitDecl`/etc all
    carry `is_pub: bool`; `LetDecl` carries `is_pub`. `ParamDecl`, `SubDecl`, `PortDecl`
    carry **no** visibility flag (params/subs/ports are visible by default per §7.4).
  - `priv param`, `priv sub`, `priv port` all parse to `(ERROR ...)` today.

---

## §2 — Sketch of approach

### Slice A — `module` declaration grammar + AST + parse (grammar prerequisite)

Add a `module_declaration` production to `tree-sitter-reify/grammar.js`:
`module <dotted_path>` at the top of `source_file` (reuse the existing `import_path`
dot-path shape). Add it to `_declaration`'s choice (or a dedicated optional-first slot).
Add a `ModuleDecl { path: String, span, content_hash }` to `reify-ast`'s `Declaration`
enum and lower it in the CST→AST step. The parser stores the declared path on
`ParsedModule` (new field `declared_module_path: Option<ModulePath>`), leaving the existing
`ParsedModule.path` (the resolver-derived path) untouched. **No enforcement yet** — this
slice only makes the declaration parse and reach the AST, so the next slice has something
to compare against.

### Slice B — path-vs-location enforcement diagnostic (compile pass)

A compile pass compares `declared_module_path` (Slice A) against the resolver-derived path
that `ParsedModule.path` already carries. Two enforcement sites, both already have both
values in hand:
- **DAG / multi-module** (`module_dag.rs::compile_module`): the resolver-derived
  `module_path` argument vs the just-parsed `declared_module_path`.
- **Entry / single-file** (`compile_project_with_entry_source`, `reify-cli parse_and_compile`):
  the `file_stem`-derived `ModulePath::single(...)` vs the declared path.

Emit `E_MODULE_PATH_MISMATCH` (error) when they disagree, naming both the declared path and
the expected path. **Missing declaration** is a separate, softer signal: emit
`W_MODULE_DECL_MISSING` (warning) so the corpus of existing declaration-less `.ri` files
keeps compiling (see §4 decision D-1). The check runs in the existing module-pragma pre-pass
neighbourhood (`compile_builder/pre_pass.rs`) where `parsed` and the expected path are both
available.

### Slice C — `priv` modifier grammar + AST + visibility wiring

Add `priv` as a member-level modifier. Grammar: where members today accept
`optional('pub')`, accept `optional(choice('pub', 'priv'))` for the member kinds where
`priv` is meaningful, and add `priv` to the keyword set. Per §7.4 the only members that are
visible-by-default — and therefore the only ones `priv` can *hide* — are **`param`**,
**named `sub`**, and **`port`**. `priv` on a `let` (already private) or a `constraint`
(already private) is a no-op the compiler rejects with `E_PRIV_REDUNDANT` (see §4 D-3).

Represent visibility on `ParamDecl`/`SubDecl`/`PortDecl` with a new `is_priv: bool` (NOT a
`pub`/`priv` enum — params have no `pub` form; the only states are default-visible and
`priv`-hidden). Wire the compiler's downward-visibility boundary (§8.3 / the member-export
path that lets a parent reach `motor.shaft_diameter` and an importer see a `pub` def's
params) to treat `is_priv` members as private: not externally accessible via dot-notation
from outside the defining scope, not importable. Emit `E_PRIV_MEMBER_ACCESS` when an
out-of-scope reference resolves to a `priv` member.

### Slice D — integration gate

One `.ri` example exercising both features end-to-end through `reify check`, plus the
two-way visibility boundary test (§6). This is the leaf that proves the chain.

---

## §3 — Substrate (G3) — grammar gate results

Run 2026-05-27, `tree-sitter parse --quiet`, grammar at HEAD:

| Fragment | Fixture | Parses today? | Resolution |
|---|---|---|---|
| `module company.products.actuators` (file top) | `mv-1-module-decl.ri` | **NO** (`(ERROR)`) | **grammar prerequisite** → Slice A |
| `priv param rated_torque : Torque = 5` | `mv-2-priv-param.ri` | **NO** (`(ERROR)`) | **grammar prerequisite** → Slice C |
| `priv sub …` / `priv port …` | `mv-3-priv-sub-port.ri` | **NO** (`(ERROR)`) | **grammar prerequisite** → Slice C |
| `pub structure def …`, `param … : …`, member bodies | existing examples | YES | unchanged |

**G3 conclusion:** both features carry novel substrate. The brief's assumption that
path-enforcement is "compile-only / `grammar_confirmed=true`" is **false** — there is no
`module` production to enforce against. Slices A and C are grammar-prerequisite tasks; B
and D depend on them. `grammar_confirmed=false` for the grammar tasks (they *create* the
production), `true` for the compile-only tasks B and D (they ride A/C's grammar).

---

## §4 — Resolved design decisions

- **D-1 — Missing `module` decl is a WARNING, not an error.** The spec says "must begin
  with a `module` declaration", but enforcing that as a hard error would break every
  existing declaration-less `.ri` file (the entire corpus + stdlib + examples today). Land
  enforcement as: *mismatch* = error (`E_MODULE_PATH_MISMATCH`), *absent* = warning
  (`W_MODULE_DECL_MISSING`). A future migration task can flip absent→error once the corpus
  is annotated. Rationale: the value is catching *wrong* declarations; *absent* is a
  migration cliff, not a correctness bug.
- **D-2 — Single-file `reify check <foo.ri>` derives expected path from `file_stem`.** A
  single file `foo.ri` checked directly is expected to declare `module foo` (single
  segment). A multi-segment declaration (`module a.b.c`) on a directly-checked single file
  with no matching directory structure is a mismatch → `E_MODULE_PATH_MISMATCH`. This keeps
  the single-file and DAG paths consistent (both compare against a resolver-derived path).
- **D-3 — `priv` is only valid on visible-by-default members.** `param`, named `sub`,
  `port`. `priv let` / `priv constraint` are rejected (`E_PRIV_REDUNDANT`) because those
  are already private; allowing `priv` there would imply a `pub`/`priv`/default tri-state
  the language does not have. `priv` on a top-level definition (`priv structure def`) is a
  **grammar error** — top-level visibility is `pub`-or-private; `priv` there is meaningless
  (the default already IS private).
- **D-4 — `is_priv: bool`, not a `Visibility` enum, on members.** Params/subs/ports have
  exactly two reachable states (default-visible, `priv`-hidden); `let` has two (default-
  private, `pub`-visible). No member has three. A unifying enum is over-engineering;
  `is_pub` and `is_priv` stay as separate bools on the decls that each apply to.
- **D-5 — `module` declaration is positional: top of file, before imports.** Matches §7.2
  "One per file, at the top." Grammar makes it an optional first element of `source_file`;
  a `module` decl appearing after any other declaration is a parse error.
- **D-6 — Path enforcement uses the SAME path the resolver already computed.** The check
  introduces no new path-derivation logic; it compares the declared path to the value
  `ParsedModule.path` / the `compile_module` argument already holds. This avoids a second
  source of truth for "what path is this module."

---

## §5 — Out of scope

- Conditional compilation (`#cfg`) — `v0_6/conditional-compilation.md`.
- Flipping missing-`module`-decl from warning to error (future migration task).
- Annotating the existing corpus/stdlib/examples with `module` declarations (mechanical
  migration; not a language-design task).
- `priv` on trait members with override semantics, or visibility *narrowing/widening*
  across refinement — `priv` here is a flat per-member hide, no inheritance interaction.
- Re-export (`pub import`) visibility interaction beyond "a `priv` member of a re-exported
  def stays hidden" (which falls out of Slice C's export-path wiring for free).

---

## §6 — Boundary-test sketch (G5 = bare B, but the visibility seam is two-way)

This PRD is bare-B shaped (2 crates: `reify-syntax`/`reify-ast` + `reify-compiler`; ~6
mechanisms; not a load-bearing seam per the overlay's list). But the `priv` visibility
boundary faces two ways and earns one explicit two-way test, named as Slice D's signal:

| Scenario | Producer side (defining module) | Consumer side (external access) | Expected |
|---|---|---|---|
| `priv param` hidden from importer | `pub structure def Motor { priv param p : … ; param q : … }` | another module imports `Motor`, reads `Motor.q` then `Motor.p` | `q` resolves; `p` → `E_PRIV_MEMBER_ACCESS` |
| `priv sub` hidden from parent dot-access | `pub structure def A { priv sub inner = Inner() }` | sibling/parent reads `a.inner` | `E_PRIV_MEMBER_ACCESS` |
| default-visible param still works | `pub structure def M { param w : Length = 1mm }` | external `M.w` | resolves (no regression) |
| declared path matches location | file resolved as `a.b.c` declares `module a.b.c` | `reify check` | no diagnostic |
| declared path mismatches | file resolved as `a.b.c` declares `module a.b.WRONG` | `reify check` | `E_MODULE_PATH_MISMATCH` (exit nonzero) |

---

## §7 — Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `v0_6/conditional-compilation.md` | sibling | shares the §7.4 visibility framing + module-DAG; `#cfg` rides pragmas, this PRD rides `pub`/`priv` member modifiers — **disjoint mechanisms** | this-prd owns `priv`/`module`; cond-comp owns `#cfg` | independent, no shared task |
| spec §7.4 / §8.3 visibility boundary | consumes | the member-export / downward-visibility path that `priv` must gate | this-prd | wired in Slice C |

No contested-ownership pair from the overlay's catalogue is touched.

---

## §8 — Decomposition plan (per-task observable signal)

- **Task α — Grammar: `module` declaration production.** `module_declaration` rule in
  `grammar.js`; parser test in `tree-sitter-reify/tests/`; CST→AST lowering to a new
  `Declaration::Module` + `ParsedModule.declared_module_path`. Signal: fixture
  `mv-1-module-decl.ri` parses (`tree-sitter parse --quiet` exit 0); a hand-written `.ri`
  with a top-of-file `module a.b.c` round-trips to an AST node holding the dotted path
  (asserted by a lowering test). `grammar_confirmed=false` (creates the production).
- **Task β — Grammar: `priv` member modifier.** `priv` keyword; `optional(choice('pub',
  'priv'))` (or member-appropriate) on `param`/`sub`/`port`; `is_priv: bool` on
  `ParamDecl`/`SubDecl`/`PortDecl`; CST→AST lowering. Signal: fixtures `mv-2-priv-param.ri`
  and `mv-3-priv-sub-port.ri` parse (exit 0); lowering test asserts `is_priv == true`.
  `grammar_confirmed=false`.
- **Task γ — Path-vs-location enforcement pass (depends α).** Compile pass comparing
  `declared_module_path` to the resolver-derived path at both the DAG and entry sites.
  Signal: `reify check` on a file whose `module` decl mismatches its location emits
  `E_MODULE_PATH_MISMATCH` and exits nonzero; a matching decl emits nothing; an absent decl
  emits `W_MODULE_DECL_MISSING` (warning, still exit zero). `grammar_confirmed=true`.
- **Task δ — `priv` visibility wiring (depends β).** Gate the member-export / downward-
  visibility boundary so `priv` members are not externally accessible or importable; reject
  `priv let`/`priv constraint` with `E_PRIV_REDUNDANT`. Signal: `reify check` on a module
  that reads another module's `priv` param via dot-notation emits `E_PRIV_MEMBER_ACCESS`;
  a `priv let` emits `E_PRIV_REDUNDANT`; a default-visible param still resolves.
  `grammar_confirmed=true`.
- **Task ε — Integration gate (leaf; depends γ, δ).** One stdlib/example `.ri` pair under
  CI exercising the §6 boundary table end-to-end: a `pub` def with a `priv` param + a
  correct `module` decl in the defining file, a consumer file that accesses the visible
  member (passes) and the `priv` member (errors), and a mismatched-`module` variant. Signal:
  the CI example produces exactly the §6 expected diagnostics through `reify check`.
  `grammar_confirmed=true`.

Intra-batch deps: γ→α, δ→β, ε→{γ, δ}.

---

## §9 — Open (tactical) questions

- Exact diagnostic codes/wording (`E_MODULE_PATH_MISMATCH`, `W_MODULE_DECL_MISSING`,
  `E_PRIV_MEMBER_ACCESS`, `E_PRIV_REDUNDANT`) — implementer picks final strings; the
  *structural marker* (stable code prefix) is the load-bearing part.
- Whether the `module` decl should also be accepted (and validated) by the GUI dirty-buffer
  compile path (`compile_project_with_entry_source` already in scope; the GUI calls it).
  Tactical: same pass, same comparison, no new design.
- Whether `priv` on a `port` body member (a sub-declaration inside a `port { … }`) is in
  scope or only top-level `port` declarations. Default: top-level members only; nested port
  bodies follow in a tiny follow-up if a user wants it.
