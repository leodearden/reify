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

Nesting depth *below* the root is deliberately not pinned at β — a fixture at
`fixtures/a/b/c.ri` passes today. Section-directory naming is leaf γ's (#6761),
and its manifest generator is what will pin the per-section depth; guessing a
depth before that decision exists would only have to be undone.

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

That property is itself machine-checked, not merely asked for here:
`placement_probe_sentinel_still_violates_the_corpus_guard` in
`crates/reify-spec-conformance/tests/fixture_tree.rs` reads this file and
re-runs a mirror of the guard's own predicate over it, so migrating the bare
`Scalar` annotation to `Length` — the plausible drive-by cleanup this section's
prose alone could not stop — reds immediately with the reason. It is the one
place any tracked `.rs` names a resident of this tree by basename, and it is
allowed to precisely because the probe is a sentinel rather than a conformance
fixture.

The leading `_` is a rule with a consumer, not decoration: leaf γ's (#6761)
manifest generator and directive harness walk `fixtures/**/*.ri` and **must
skip `fixtures/_*/`**, or they will ingest a directive-less unparseable file.
That obligation is carried forward in `crates/reify-spec-conformance/src/lib.rs`
so it travels with the code γ edits.

Section-directory naming (`s09_2` vs `section-09-02` vs `09.2`) is deliberately
**not** decided here — it belongs to leaf γ (#6761), whose manifest generator
must parse it, and leaf η (#6765), the first real §9.2 wave.

## Not yet present at β

The directive/annotation format (`//@ key: value`, `//~ ERROR E_*`) and the
generated `manifest.json` arrive with leaf γ (#6761), together with the harness
that consumes them. At β this tree carries fixtures and this charter only.

## Placement probe (leaf β, #6759)

A committed, re-runnable observation — not a claim. With
`_placement-probe/placement_probe.ri` present, unparseable *and* carrying a real
bare `Scalar` annotation, every walker and gate that could plausibly reach this
tree was run from a clean worktree:

| Command | Verdict |
|---|---|
| `cargo build -p reify-spec-conformance` | Finished dev profile, exit 0 |
| `cargo test -p reify-spec-conformance --test fixture_tree` | ok. 4 passed; 0 failed |
| `cargo test -p reify-cli --test harness_cli corpus_no_bare_scalar::` | ok. 26 passed; 0 failed *(the headline observation)* |
| `cargo test -p reify-compiler --test harness_compilation_surface examples_smoke::` | ok. 9 passed; 0 failed (walks `examples/` only) |
| `cargo test -p reify-test-support --test ignore_reason_hygiene` | ok. 1 passed; 0 failed (repo-wide over `*.rs`; nothing here carries `#[ignore]`) |
| `bash tests/infra/test_verify_scope.sh` | 255 passed, 0 failed (PG-DRIFT / PG-DRIFT-DIR unaffected) |
| `bash tests/infra/test_heavy_filter_atoms.sh` | 23 passed, 0 failed; atom count still exactly 8 |
| `bash scripts/gui-test.sh --no-typecheck -- src/__tests__/reifyGrammarCorpus.test.ts` | 477 passed (Lezer `CORPUS_ROOTS` is explicit-inclusion) |
| `cargo metadata --format-version 1 --locked` | exit 0 (the `Cargo.lock` entry is current, so `scripts/affected-crates-lib.sh` does not degrade to "ALL crates affected") |

The walker set was re-derived from the worktree rather than taken on trust.
Of the 22 sources in the repo that both recurse a directory and mention `.ri`,
every root resolves to `examples/` (`examples_smoke`,
`auto_type_param_determinism_tests`, `no_stale_undef_invariant_gate`,
`snapshot_cache_divergence_gate`, `examples/kernel_queries` for
`selector_coercion_golden`, `examples/best_practices` for
`best_practices_constraint_gate`), to `['examples', 'tests/prd-gate/fixtures']`
(the Lezer ledger's `CORPUS_ROOTS`), to a `tempfile` cache dir, or to a named
non-`.ri` corpus — **except one**:

* `crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs`, whose
  `collect_files(&root.join("crates"), "ri", …)` is the sole repo-wide sweep of
  `crates/**/*.ri`. It carries the registered exclusion arm for this tree.

Two walkers are repo-wide over `*.rs` and auto-cover the crate harmlessly:
`reify-test-support`'s `walk_rs_files` (checks `#[ignore]` reasons — nothing here
has one) and `ambient_default_material_integration_gate`, which walks `crates/`
but only enters `src/` mode and greps for two unrelated symbols. No shell or
Python walker globs `.ri` at all; `run-gui.sh` / `run-gui-dev.sh` only check an
argument's extension.

**Conclusion:** an unparseable, bare-`Scalar`-bearing fixture in this tree is
inert to every repo walker, because exactly one walker reaches `crates/**/*.ri`
and it carries a registered exclusion arm. Re-run the table above after any
change to this tree's location or to that arm.
