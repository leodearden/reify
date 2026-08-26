# `crates/reify-spec-conformance/fixtures/` — Ring-1 language-spec conformance corpus

The fixture corpus for the language-spec conformance suite
(PRD `docs/prds/v0_6/spec-conformance-suite.md`, D2). Seeded by leaf β (#6759);
the corpus itself belongs to leaf η (#6765) and the Phase-3 waves.

## Layout

    crates/reify-spec-conformance/fixtures/<section>/<name>.ri

Per-section subdirectories, one directory per spec section — **never a flat
pile at the tree root**. `crates/reify-spec-conformance/tests/fixture_tree.rs`
pins that structurally: no loose `*.ri` at the root, at least one `*.ri`
somewhere beneath. Non-`.ri` files at the root (this README) are fine.

Cite a fixture by **full repo-relative path** everywhere — probe sets,
capability manifests, PRD prose, task `metadata.files`. Bare `fixtures/<name>.ri`
and bare stems are unresolvable by machine. This is the same rule the placement
standard in `tests/prd-gate/README.md` imposes on every other fixture tier.

## Must-reject fixtures are chartered residents

Deliberately unparseable, deliberately-diagnostic-emitting and deliberately
spec-violating `.ri` files belong here **by design**. A conformance corpus that
could only hold well-formed programs would test half the spec: every "the
compiler must reject X" clause needs a committed X. So nothing in this tree is
required to parse, to type-check, or to pass `reify check`.

That is precisely why the tree is excluded from the corpus-cleanliness guard
(next section) rather than being expected to satisfy it.

## No repo-wide walker sweeps this tree

Every zero-Error / corpus walker in the repo is *explicit-inclusion* — the
Lezer grammar ledger's `CORPUS_ROOTS`, `examples_smoke`, the eval-side corpus
gates — so each is blind to this tree by construction. There is exactly **one**
exception:

* `crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs` walks
  `crates/**/*.ri` repo-wide. It carries a **registered exclusion arm** for this
  directory, because bare-`Scalar` rejection is itself a spec clause the
  conformance suite must be free to test with a violating fixture.

Adding a fixture to this tree is therefore inert to every repo-wide gate — see
the placement probe below, which is a committed, re-runnable observation of
exactly that.

## `_placement-probe/` — the one non-section resident

`_placement-probe/` is the single directory here that is **not** a spec section;
the leading underscore marks it as such. It holds the leaf-β placement probe: a
permanently committed fixture that is both unparseable *and* carries a real bare
`Scalar` annotation, so it genuinely violates the corpus guard's predicate.

It is a **sentinel, not scaffolding**. Because it stays committed and stays a
live violator, the exclusion arm in `corpus_no_bare_scalar.rs` can never go
vacuous: delete the arm, narrow its path, or move this tree, and that guard
reds immediately naming the probe file. Do not "fix" the probe.

Section-directory naming (`s09_2` vs `section-09-02` vs `09.2`) is deliberately
**not** decided here — it belongs to leaf γ (#6761), whose manifest generator
must parse it, and leaf η (#6765), the first real §9.2 wave.

## Not yet present at β

The directive/annotation format (`//@ key: value`, `//~ ERROR E_*`) and the
generated `manifest.json` arrive with leaf γ (#6761), together with the harness
that consumes them. At β this tree carries fixtures and this charter only.
