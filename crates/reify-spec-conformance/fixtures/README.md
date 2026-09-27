# `crates/reify-spec-conformance/fixtures/` — Ring-1 language-spec conformance corpus

The fixture corpus for the language-spec conformance suite
(PRD `docs/prds/v0_6/spec-conformance-suite.md`, D2). Seeded by leaf β (#6759);
the corpus itself belongs to leaf η (#6765) and the Phase-3 waves.

## Layout

    crates/reify-spec-conformance/fixtures/<section>/<name>.ri

One directory per spec section — **never a flat pile at the tree root**.
`../tests/fixture_tree.rs` pins that structurally: no loose `*.ri` at the root,
at least one `*.ri` somewhere beneath. Non-`.ri` files at the root (this README)
are fine.

Section-directory naming (`s09_2` vs `section-09-02` vs `09.2`), and with it the
per-section depth, is **not** decided here — it belongs to leaf γ (#6761), whose
manifest generator must parse it, and leaf η (#6765), the first real §9.2 wave.

Cite a fixture by **full repo-relative path** everywhere — probe sets,
capability manifests, PRD prose, task `metadata.files`. Bare `fixtures/<name>.ri`
and bare stems are unresolvable by machine; this is the same rule the placement
standard in `tests/prd-gate/README.md` imposes on every other fixture tier.

## Must-reject fixtures are chartered residents

Deliberately unparseable, deliberately-diagnostic-emitting and deliberately
spec-violating `.ri` files belong here **by design**. A conformance corpus that
could only hold well-formed programs would test half the spec: every "the
compiler must reject X" clause needs a committed X. So nothing in this tree is
required to parse, to type-check, or to pass `reify check`.

## The one repo-wide walker, and its exclusion arm

Every zero-Error / corpus walker in the repo is *explicit-inclusion* (the Lezer
grammar ledger's `CORPUS_ROOTS`, `examples_smoke`, the eval-side corpus gates),
so each is blind to this tree by construction. The one exception is
`crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs`, which walks
`crates/**/*.ri` and therefore carries a **registered exclusion arm** for this
directory: bare-`Scalar` rejection is itself a spec clause this suite must be
free to test with a violating fixture.

## `_placement-probe/` — the one non-section resident

A leading `_` on a directory directly under `fixtures/` marks it as **not a spec
section**. Leaf γ's manifest generator and directive harness walk
`fixtures/**/*.ri` and **must skip `fixtures/_*/`** — that obligation travels
with the code in `../src/lib.rs`, Obligation 2.

`_placement-probe/placement_probe.ri` is the only such resident today. It is a
**sentinel**: unparseable, and carrying a real bare `Scalar` annotation, so it
is a live violator sitting under the exclusion arm above. The guard's own
`spec_conformance_placement_probe_is_a_live_violator` test, beside the arm, reds
if it stops violating, which is what keeps the arm from going vacuous. Delete
the arm, narrow its path, or move this tree, and `corpus_has_zero_bare_scalar`
reds naming the probe. If that guard is ever retired (it becomes
compiler-redundant once γ adds `E_BARE_SCALAR`), retire the probe with it.
The walker survey and seeded-mutation evidence behind this arrangement are in
the #6759 commit history.

## Not yet present at β

The directive/annotation format (`//@ key: value`, `//~ ERROR E_*`) and the
generated `manifest.json` arrive with leaf γ (#6761), together with the harness
that consumes them. At β this tree carries fixtures and this charter only.
