# Best-practices corpus — index

Small, single-idiom `.ri` exemplars. Each file demonstrates **one** language or
stdlib idiom that is easy to get wrong, together with the anti-pattern it
replaces. They are reference material for authoring designs, not designs
themselves — nothing here is meant to be a useful part.

**Everything in this directory is compile-gated.** `examples/` is walked
recursively by `crates/reify-compiler/tests/examples_smoke.rs`, so every file
here must parse and compile with the stdlib prelude at zero Error severity, and
is additionally evaluated by the corpus-wide eval gates in `crates/reify-eval/`.
A file that cannot reach a clean compile does **not** belong here — an exemplar
that does not work is worse than no exemplar. Do not add a `SKIP_SET` entry to
exempt one.

## How to use this index

**Before probing the language, grep this file.** The corpus exists because
design sessions repeatedly burn verification runs on throwaway probe files
re-discovering the same semantics. If the idiom is listed below, read the
exemplar instead of probing.

**Before adding a file, add its row.** `crates/reify-compiler/tests/best_practices_index_sync.rs`
pins a bidirectional invariant — every `.ri` here is named in this index, and
every file named here exists — so a file and its row land in one commit, never
two. The full graduation procedure is in `.claude/skills/reify-design/SKILL.md`
under "Session wrap — graduate your probes".

Verify both with:

```sh
cargo test -p reify-compiler --test examples_smoke --test best_practices_index_sync
```

## Idioms

| Exemplar | Idiom | Anti-pattern it replaces |
|---|---|---|
| `negation.ri` | Unary minus is a first-class prefix operator on dimensioned and dimensionless values, and composes in operand position. | `0mm - x` / `x * -1` as a sign-flip workaround. |
| `hollow_primitives.ri` | `tube(outer_r, inner_r, height)` (base at z=0; needs `inner_r < outer_r`) and `cylinder_centered(radius, height)` for hollow and origin-centred solids. `box_centered` is an op-identical alias for `box`. | `difference(cylinder(R,h), cylinder(r,h))`; a manual `translate(..., -h/2)` to centre a cylinder. |
| `symmetry_mirror.ri` | `mirror` returns a reflected **copy** — `union(g, mirror(g, plane_yz(0mm)))`. Plane ctors take exactly one offset arg; the 7-arg scalar form needs a dimensioned origin, and `reify check` will not tell you when it doesn't. | Authoring both halves of a symmetric part by hand. |
