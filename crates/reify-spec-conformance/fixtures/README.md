# `crates/reify-spec-conformance/fixtures/` — Ring-1 language-spec conformance corpus

The fixture corpus for the language-spec conformance suite
(PRD `docs/prds/v0_6/spec-conformance-suite.md`, D2). Seeded by leaf β (#6759);
the corpus itself belongs to leaf η (#6765) and the Phase-3 waves.

**This file is the normative home for this tree's charter and for the sentinel
arrangement described below.** `src/lib.rs`, `tests/fixture_tree.rs` and the
exclusion arm in `corpus_no_bare_scalar.rs` carry pointers here plus their own
local detail only — do not re-explain the mechanism there.

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

That property is itself machine-checked, not merely asked for here.

### The predicate is single-sourced, so there is no mirror to drift

`crates/reify-cli/tests/harness_cli/bare_scalar_predicate.rs` holds ONE copy of
the detection predicate and its two helpers. Both test targets pull it in with
`#[path]` — **source inclusion, never a Cargo dependency edge**, because a
`reify-spec-conformance` → `reify-cli` dependency would make this crate
occt-touching and drag in the hand-synced `scripts/occt-touching-crates.txt` /
`.config/nextest.toml` pair (see `../src/lib.rs`). Its unit tests
(`predicate_tests`) live in `corpus_no_bare_scalar.rs`, whose own file is
self-excluded from the scan and so may spell violating literals out in full.

Two tests then run that one predicate over the probe, in two crates, each
covering what the other cannot:

* `spec_conformance_placement_probe_is_a_live_violator` — in
  `corpus_no_bare_scalar.rs` **itself**, adjacent to the arm it defends. Any
  narrowing of the predicate that would stop flagging the probe reds in the very
  file whose exclusion arm it silently disarms.
* `placement_probe_sentinel_still_violates_the_corpus_guard` — in
  `crates/reify-spec-conformance/tests/fixture_tree.rs`. Its distinct value is
  reach, not independence: it still fires under a `.ri`-only scope narrowing
  that never builds `reify-cli`.

Either way, migrating the bare `Scalar` annotation to `Length` — the plausible
drive-by cleanup this section's prose alone could not stop — reds immediately
with the reason.

### The guard itself must stay registered

A sentinel standing watch over a *retired* guard is exactly the vacuity all of
this exists to prevent, so a third test —
`corpus_guard_still_registers_this_tree` — reds if `corpus_no_bare_scalar.rs`
disappears (its own header anticipates becoming compiler-redundant once γ adds
`E_BARE_SCALAR`) or drops its exclusion arm.

It pins **one token**, carried in a comment beside the arm and documented there
as a machine-read contract:

    MARKER: spec-conformance-fixtures-exclusion-arm

A token rather than a source identifier, deliberately: grepping another crate's
private local binding or a function name would red this test on any pure rename
or file move over there, with a failure message wrongly claiming the arm was
lost. The token is rename-proof from both sides and both sides know it exists.
If the arm is retired, delete the token in the same change.

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
tree was run from a clean worktree, and each one was **green**.

The verdict column records the property that is stable and that actually
matters. Absolute pass counts are deliberately *not* recorded: every future task
that adds a test to any of these suites would silently falsify them, and a
reader re-running the table could not then tell drift from regression.

| Command | Verdict |
|---|---|
| `cargo build -p reify-spec-conformance` | exit 0 |
| `cargo test -p reify-spec-conformance --test fixture_tree` | exit 0, all green |
| `cargo test -p reify-cli --test harness_cli corpus_no_bare_scalar::` | exit 0, all green — the headline observation |
| `cargo test -p reify-compiler --test harness_compilation_surface examples_smoke::` | exit 0 (walks `examples/` only; this tree is invisible to it) |
| `cargo test -p reify-test-support --test ignore_reason_hygiene` | exit 0 (repo-wide over `*.rs`; nothing here carries `#[ignore]`) |
| `bash tests/infra/test_verify_scope.sh` | exit 0; PG-DRIFT / PG-DRIFT-DIR unaffected |
| `bash tests/infra/test_heavy_filter_atoms.sh` | exit 0; atom count still exactly 8 |
| `bash scripts/gui-test.sh --no-typecheck -- src/__tests__/reifyGrammarCorpus.test.ts` | exit 0 (Lezer `CORPUS_ROOTS` is explicit-inclusion) |
| `cargo metadata --format-version 1 --locked` | exit 0 — the `Cargo.lock` entry is current, so `scripts/affected-crates-lib.sh` does not degrade to "ALL crates affected" |

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
| probe's `Scalar` migrated to `Length` | BOTH sentinels red (guard-side and crate-local) |
| the guard's `MARKER:` token deleted | `corpus_guard_still_registers_this_tree` red |
| the whole exclusion arm (token + `retain`) deleted | `corpus_has_zero_bare_scalar` red naming the probe, AND `corpus_guard_still_registers_this_tree` red |
| `corpus_no_bare_scalar.rs` deleted outright | `corpus_guard_still_registers_this_tree` red |
| `bare_scalar_predicate.rs` predicate loosened (Debug carve-out / `::Scalar` / `Scalar<…>` / trailing-comment strip / codomain arm each dropped in turn) | `predicate_tests` cases red — once, for both consumers, because there is one copy |
| `loose_ri_at_root` `is_dir` inverted, or its extension typo'd to `rs` | `loose_ri_at_root_fires_on_a_seeded_violator` red |
| `collect_ri` made non-recursive | its self-test and `fixture_tree_is_not_vacuous` red |

One mutation is recorded because it did NOT fire: dropping the predicate's
pure-comment early return changes nothing, since `strip_trailing_line_comment`
already strips a leading `//`. Worth knowing before anyone "covers" it.
