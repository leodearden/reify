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

Read the non-vacuity half honestly: at β the *only* resident is the
`_placement-probe/` sentinel below, which is not a conformance fixture, so it is
what satisfies "at least one `*.ri`". The test says so in its own message and
reports the count of real section residents (those outside `fixtures/_*/`)
alongside it — when leaf η (#6765) lands the first real §9.2 wave, the
tightening is to assert on that count instead.

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

That property is itself machine-checked, not merely asked for here. **Two**
tests hold it up, in two crates, each covering what the other cannot:

* `spec_conformance_placement_probe_is_a_live_violator` — in
  `corpus_no_bare_scalar.rs` **itself**. It runs the guard's REAL predicate over
  this file, so it has zero drift surface and is the authoritative check: any
  narrowing of the predicate that would stop flagging the probe reds in the very
  file whose exclusion arm it silently disarms.
* `placement_probe_sentinel_still_violates_the_corpus_guard` — in
  `crates/reify-spec-conformance/tests/fixture_tree.rs`. It re-runs a *mirror*
  of that predicate (the crate has empty `[dependencies]` at β, and depending on
  `reify-cli` to share the real one would make it occt-touching). This is the
  fast-feedback copy: it still fires under a `.ri`-only scope narrowing that
  never builds `reify-cli`. The mirror's own unit cases (`mirror_predicate_tests`)
  are ported from the guard's discriminating ones, so a mirror that drifts
  LOOSER than the guard reds rather than keeping this sentinel falsely green.

Either way, migrating the bare `Scalar` annotation to `Length` — the plausible
drive-by cleanup this section's prose alone could not stop — reds immediately
with the reason. Both were run against a probe mutated to `Length`, and both
went red.

The mirror is safe in one direction only, so a third test covers the other:
`corpus_guard_still_registers_this_tree` reds if `corpus_no_bare_scalar.rs`
disappears (its own header anticipates becoming compiler-redundant once γ adds
`E_BARE_SCALAR`) or loses its `spec_conformance_fixtures` exclusion binding —
because a sentinel standing watch over a retired guard is exactly the vacuity
all of this exists to prevent. If that guard IS retired, retire the probe and
these tests in the same change.

Naming the probe is the one place any tracked `.rs` spells out a resident of
this tree by basename, and it is allowed precisely because the probe is a
sentinel rather than a conformance fixture.

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
tree was run from a clean worktree. **Every row below was re-measured together
at the current tree state** (the β amendment that added the second sentinel);
counts that moved since the first run are noted inline.

| Command | Verdict |
|---|---|
| `cargo build -p reify-spec-conformance` | Finished dev profile, exit 0 |
| `cargo test -p reify-spec-conformance --test fixture_tree` | ok. 24 passed; 0 failed *(4 at the first run; the amendment added the seeded-fire and mirror unit cases)* |
| `cargo test -p reify-cli --test harness_cli corpus_no_bare_scalar::` | ok. 27 passed; 0 failed *(the headline observation; 26 + the guard-side sentinel)* |
| `cargo test -p reify-compiler --test harness_compilation_surface examples_smoke::` | ok. 11 passed; 0 failed (walks `examples/` only; 9 before this branch was rebased onto a main carrying two more examples — drift in `examples/`, not here) |
| `cargo test -p reify-test-support --test ignore_reason_hygiene` | ok. 1 passed; 0 failed (repo-wide over `*.rs`; nothing here carries `#[ignore]`) |
| `bash tests/infra/test_verify_scope.sh` | 283 passed, 0 failed (PG-DRIFT / PG-DRIFT-DIR unaffected; 255 before the same rebase) |
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

### The RED half, seeded and observed

A green table only shows the exclusion working; these are the mutations that
were seeded, observed RED, and reverted — so the arm and its sentinels are known
to be non-vacuous rather than merely believed to be:

| Seeded mutation | Observed |
|---|---|
| probe's `Scalar` migrated to `Length` | BOTH sentinels red (guard-side and mirror) |
| the guard's `spec_conformance_fixtures` exclusion arm deleted | `corpus_guard_still_registers_this_tree` red |
| `corpus_no_bare_scalar.rs` deleted outright | `corpus_guard_still_registers_this_tree` red |
| mirror loosened (Debug carve-out / `::Scalar` / `Scalar<…>` / trailing-comment strip / codomain arm each dropped in turn) | 1–2 `mirror_predicate_tests` cases red per mutation |
| `loose_ri_at_root` `is_dir` inverted, or its extension typo'd to `rs` | `loose_ri_at_root_fires_on_a_seeded_violator` red |
| `collect_ri` made non-recursive | its self-test and `fixture_tree_is_not_vacuous` red |

One mutation is recorded because it did NOT fire: dropping the mirror's
pure-comment early return changes nothing, since `strip_trailing_line_comment`
already strips a leading `//`. The same redundancy exists in the guard itself —
worth knowing before anyone "covers" it.
