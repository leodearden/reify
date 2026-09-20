# `crates/reify-spec-conformance/fixtures/` — Ring-1 language-spec conformance corpus

The fixture corpus for the language-spec conformance suite
(PRD `docs/prds/v0_6/spec-conformance-suite.md`, D2). Seeded by leaf β (#6759);
the corpus itself belongs to leaf η (#6765) and the Phase-3 waves.

**This file is the normative home for this tree's charter and for the sentinel
arrangement described below.** `src/lib.rs`, `tests/fixture_tree.rs` and the
exclusion arm in `corpus_no_bare_scalar.rs` carry pointers here plus their own
local detail only — do not re-explain the mechanism there. The converse also
holds: the occt-dependency-edge rule has exactly one home, `../src/lib.rs`
Obligation 1, and is not restated here.

## Layout

    crates/reify-spec-conformance/fixtures/<section>/<name>.ri

Per-section subdirectories, one directory per spec section — **never a flat
pile at the tree root**. `crates/reify-spec-conformance/tests/fixture_tree.rs`
pins that structurally: no loose `*.ri` at the root, at least one `*.ri`
somewhere beneath. Non-`.ri` files at the root (this README) are fine.

Read the non-vacuity half honestly: at β the *only* resident is the
`_placement-probe/` sentinel below, which is not a conformance fixture. What
that does and does not establish, and the one-line tightening leaf η (#6765)
will apply, are on `fixture_tree_is_not_vacuous` itself.

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
rather than being expected to satisfy it.

## No repo-wide walker sweeps this tree

Every zero-Error / corpus walker in the repo is *explicit-inclusion* — the
Lezer grammar ledger's `CORPUS_ROOTS`, `examples_smoke`, the eval-side corpus
gates — so each is blind to this tree by construction. There is exactly **one**
exception:

* `crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs` walks
  `crates/**/*.ri` repo-wide. It carries a **registered exclusion arm** for this
  directory, because bare-`Scalar` rejection is itself a spec clause the
  conformance suite must be free to test with a violating fixture.

That set was re-derived from the worktree rather than taken on trust; the
derivation is recorded with the placement probe below, which is a committed,
re-runnable observation of the same conclusion.

## `_placement-probe/` — the one non-section resident

`_placement-probe/` is the single directory here that is **not** a spec section;
the leading underscore marks it as such. It holds the leaf-β placement probe: a
permanently committed fixture that is both unparseable *and* carries a real bare
`Scalar` annotation, so it genuinely violates the corpus guard's predicate.

It is a **sentinel, not scaffolding**. Because it stays committed and stays a
live violator, the exclusion arm in `corpus_no_bare_scalar.rs` can never go
vacuous: delete the arm, narrow its path, or move this tree, and that guard reds
immediately naming the probe file. That property is itself machine-checked, not
merely asked for here — the probe file and both sentinel failure messages carry
the "do not migrate this annotation" instruction, which is where a reader
actually meets it.

The leading `_` is a rule with a consumer, not decoration: leaf γ's (#6761)
manifest generator and directive harness walk `fixtures/**/*.ri` and **must
skip `fixtures/_*/`**, or they will ingest a directive-less unparseable file.
That obligation is carried forward in `crates/reify-spec-conformance/src/lib.rs`
so it travels with the code γ edits.

Section-directory naming (`s09_2` vs `section-09-02` vs `09.2`) is deliberately
**not** decided here — it belongs to leaf γ (#6761), whose manifest generator
must parse it, and leaf η (#6765), the first real §9.2 wave.

### One predicate, two tests, no mirror

`crates/reify-cli/tests/common/bare_scalar_predicate.rs` holds ONE copy of the
detection predicate and its two helpers. Both test targets pull it in with
`#[path]` — source inclusion, never a Cargo dependency edge (why: `../src/lib.rs`
Obligation 1; why `tests/common/` rather than `tests/harness_cli/`: that file's
own header). Its unit tests (`predicate_tests`) live in
`corpus_no_bare_scalar.rs`, whose own file is self-excluded from the scan and so
may spell violating literals out in full.

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

### The guard itself must stay registered

A sentinel standing watch over a *retired* guard is exactly the vacuity all of
this exists to prevent, so a third test —
`corpus_guard_still_registers_this_tree` — reds if `corpus_no_bare_scalar.rs`
disappears (its own header anticipates becoming compiler-redundant once γ adds
`E_BARE_SCALAR`) or stops naming this crate at all.

It pins the guard file's readability plus the literal path segment
`reify-spec-conformance`, which the exclusion arm cannot work without spelling.
An executable spelling, deliberately, rather than a comment token planted to be
grepped: a token pin buys a cross-crate coupling to comment text and no
coverage, because deleting the `retain` while leaving the token green-lights the
pin, and deleting the arm for real reds `corpus_has_zero_bare_scalar` on the
probe without any help. The segment pin is therefore
**necessary-but-not-sufficient by design** — the RED table below records it
staying green when only the arm is deleted. What it uniquely catches is the
guard being kept while this tree is unregistered from it outright, the one case
that would leave both sentinels above watching nothing.

## Not yet present at β

The directive/annotation format (`//@ key: value`, `//~ ERROR E_*`) and the
generated `manifest.json` arrive with leaf γ (#6761), together with the harness
that consumes them. At β this tree carries fixtures and this charter only.

## Placement probe (leaf β, #6759)

A committed, re-runnable observation — not a claim. With
`_placement-probe/placement_probe.ri` present, unparseable *and* carrying a real
bare `Scalar` annotation, every walker and gate that could plausibly reach this
tree was run against this branch's tree, and each one was **green**. The last rows
are not walkers: they are the gates that the single-sourced predicate's
cross-crate `#[path]` include, and its home in `crates/reify-cli/tests/common/`,
could plausibly disturb.

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
| `bash tests/infra/test_heavy_filter_atoms.sh` | exit 0; heavy-filter atom set unchanged by this diff |
| `bash scripts/gui-test.sh --no-typecheck -- src/__tests__/reifyGrammarCorpus.test.ts` | exit 0 (Lezer `CORPUS_ROOTS` is explicit-inclusion) |
| `cargo metadata --format-version 1 --locked` | exit 0 — the `Cargo.lock` entry is current, so `scripts/affected-crates-lib.sh` does not degrade to "ALL crates affected" |
| `cargo tree -p reify-spec-conformance -e normal,dev` | prints the crate alone, zero dependency edges — the `#[path]` predicate include adds none, so this crate stays off the occt-touching set |
| `bash tests/infra/test_occt_gated_scope.sh` | exit 0; Test 3's bidirectional declared-set-equals-derived-set assertion still holds |
| `bash tests/infra/test_harness_kloc_cap.sh` | exit 0; the predicate's move out of `harness_cli/` only re-attributes its lines from the unit's `module` component to its `external` one (measured: `external` 312/1 file → 448/2 files, the delta being the predicate exactly), so the `harness_cli` unit total is unchanged and the cap is not evaded |
| `bash tests/infra/test_harness_baseline_registration_gate.sh` | exit 0; and the diff-scoped `scripts/check-harness-baseline-registration.sh` reports `violations=0` for the predicate's new path (nesting below `tests/` is outside the re-accretion predicate) |

The walker set: of the 22 sources in the repo that both recurse a directory and
mention `.ri`, every root resolves to `examples/` (`examples_smoke`,
`auto_type_param_determinism_tests`, `no_stale_undef_invariant_gate`,
`snapshot_cache_divergence_gate`, `examples/kernel_queries` for
`selector_coercion_golden`, `examples/best_practices` for
`best_practices_constraint_gate`), to `['examples', 'tests/prd-gate/fixtures']`
(the Lezer ledger's `CORPUS_ROOTS`), to a `tempfile` cache dir, or to a named
non-`.ri` corpus — **except**
`crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs`, whose
`collect_files(&root.join("crates"), "ri", …)` is the sole repo-wide sweep of
`crates/**/*.ri`.

Two walkers are repo-wide over `*.rs` and auto-cover the crate harmlessly:
`reify-test-support`'s `walk_rs_files` (checks `#[ignore]` reasons — nothing here
has one) and `ambient_default_material_integration_gate`, which walks `crates/`
but only enters `src/` mode and greps for two unrelated symbols. No shell or
Python walker globs `.ri` at all; `run-gui.sh` / `run-gui-dev.sh` only check an
argument's extension. Re-run the table above after any change to this tree's
location or to that one arm.

### The RED half, seeded and observed

A green table only shows the exclusion working; these are the mutations that
were seeded one at a time, observed, and reverted — so the arm and its sentinels
are known to be non-vacuous rather than merely believed to be. Every row below
was re-observed first-hand after the predicate moved to `tests/common/` and the
marker-token pin was replaced; the working tree was verified clean after every
revert.

| Seeded mutation | Observed |
|---|---|
| probe's `Scalar` migrated to `Length` | BOTH sentinels red — `placement_probe_sentinel_still_violates_the_corpus_guard` (crate-local) and `spec_conformance_placement_probe_is_a_live_violator` (guard-side) |
| the exclusion arm's path + `retain` deleted, guard otherwise intact | `corpus_has_zero_bare_scalar` red with exactly one violation, `crates/reify-spec-conformance/fixtures/_placement-probe/placement_probe.ri:14`. `corpus_guard_still_registers_this_tree` stays GREEN — the guard still names this crate elsewhere. That is the pin being necessary-not-sufficient, not a gap: the arm's removal is already loud |
| this tree fully unregistered — arm, its comment, the guard-side sentinel and the header row all removed (0 mentions left) | `corpus_guard_still_registers_this_tree` red |
| `corpus_no_bare_scalar.rs` moved away outright | `corpus_guard_still_registers_this_tree` red |
| `bare_scalar_predicate.rs` loosened — Debug carve-out dropped | three red in `reify-cli`: `predicate_tests::excludes_rust_debug_scalar_struct_field{,_underscore_ident}` and `corpus_has_zero_bare_scalar` on the real `{:#?}` goldens in the corpus. `reify-spec-conformance` stays green — the probe's violation is an annotation, untouched by that carve-out. One copy of the predicate, one set of unit tests, and they live beside the consumer that can see the corpus |
| `bare_scalar_predicate.rs` moved out of `tests/common/` | `reify-spec-conformance` fails to COMPILE: ``error: couldn't read `…/tests/../../reify-cli/tests/common/bare_scalar_predicate.rs` ``. A relative `#[path]` across a crate boundary cannot be defended by a diagnostic; it is defended by the file's HOME — `tests/common/` is the retained sibling the harness-layout contract never moves, which `tests/harness_cli/` is not |
| `loose_ri_at_root` `is_dir` inverted | `loose_ri_at_root_fires_on_a_seeded_violator` red |
| `collect_ri` made non-recursive | `collect_ri_recurses_and_filters_by_extension` AND `fixture_tree_is_not_vacuous` red |

One mutation is recorded because it did NOT fire: dropping the predicate's
pure-comment early return changes nothing, since `strip_trailing_line_comment`
already strips a leading `//`. Worth knowing before anyone "covers" it.
