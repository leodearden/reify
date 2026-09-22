# Machine-written legibility artifacts on reify main: who validates what lands

Dark-factory's legibility pipeline writes LLM-authored prose into reify and
commits it straight to `main`, unattended. This file records who validates that
content, which gate charter it is judged under, and what a writer must do when
reify refuses a commit. Recorded by #7788 (2026-09-22).

Two mechanisms named below were on branches, not on `main`, when this was
written. They are cited by branch and commit because the numbers in their names
are not their task ids: reify tasks #7784 and #7785 are unrelated work, and
neither branch has a task record (esc-7788-5). Each entry also names the symbol
that carries the mechanism. Once its branch has landed, that symbol is the cite
to follow.

- **The codebook-exempt branch**, `task/7784-codebook-exempt` (e5044ebadb),
  adds the codebook row to
  `tests/infra/cited-test-path-lib.sh::cited_test_path_exclusions`, which takes
  it out of the cited-test-path scan.
- **The staged-cited-gate branch**, `task/7785-staged-cited-gate` (b68becc336),
  adds `scripts/verify.sh::select_cited_test_path_gate`, which runs the
  cited-test-path gate on `--scope staged`.

`git merge-base --is-ancestor <commit> main` tells whether either has landed.

## §1 The writers and what they land

**(a) Trickle ε.** Dark-factory's `scripts/legibility/nightly.py::run_nightly`,
run daily at 03:00 by `legibility-trickle@reify.timer`. The chain is
`coder.py::build_prompt` (one LLM call per sampled digest) →
`codebook.py::apply_coding_record` / `_build_sighting` → `codebook.dump` →
`nightly._git_commit_docs_only`, i.e.
`git -C /home/leo/src/reify commit --only docs/legibility/confusion-codebook.yaml`.
`project_root` comes from `docs/legibility/legibility.yaml`.

**(b) Census η.** Dark-factory's `scripts/legibility/census.py` commits
`plans/confusion-census-<date>.md`, the codebook and
`docs/legibility/census-state.json`, plus `plans/confusion-census-<date>-payloads.json`
on a dry run, through `census._build_default_commit` (`git commit --only`),
best-effort. Precedent: 79be2e6606, the only census commit so far. Whether the
2026-09-22 03:20 census trigger actually ran is #7789's question.

**(c) Landing path.** Both land through reify's hook-gated commit on `main`,
never through the merge queue: `hooks/pre-commit` → `hooks/project-checks` →
`scripts/verify.sh all --profile debug --scope staged --include-infra`.
`verify.sh::decide_scope` treats `docs/**`, `*.md`, `*.yaml` and `*.yml` as
inert (`scripts/affected-crates-lib.sh::reify_is_inert_path`), so a
codebook-only stage gets an empty plan: "nothing to verify", exit 0 (re-measured
2026-09-22 with `--print-plan`). The merge tier's `tests/infra/run_all.sh` pool
never runs against these commits before they are on `main`. A `plans/*.json`
payloads file is not inert and takes `decide_scope`'s conservative catch-all
arm.

**(d) Provenance.** `cause:`, `evidence_quote:`, sighting `note:` and `title:`
are LLM-authored. `_build_sighting` copies them through unchecked, and
`codebook.validate` checks shape, enums and ids, never what a prose field says.
`evidence_quote:` is not a verbatim transcript record either: the prompt asks
for `"evidence_quote": "..."` and never for byte-exact quoting. Measured case:
47f904c2ea (sightings for 2026-09-11, session
390af25e-0d17-4a70-8bf5-8cf2482a3a46) quotes a failed Read of
`registry_seed_result_types.rs` under reify-compiler's
`tests/harness_compilation_surface/`, then a summary of the digest's signal
counts ("4× 'not_found' signals; automated session (n_user_turns=0)"). The probe
is real: the parent agent briefed a review subagent with that nonexistent path,
and the subagent's Read failed before it found the unit under
`tests/harness_builtin_registry/`. The appended summary occurs in none of the
session's transcripts. The row records a true confusion in composed words.

**(e) Append-only.** `codebook.py::_reject_deletion_directive` and
`assert_no_deletion` (`NeverDeleteError`) keep every entry, candidate and
sighting. No op rewrites a sighting; a `corrections` op may reword an entry's
`title` or `cause` but never clears one. A committed sighting can never be
cleaned.

**(f) Feedback loop.** `coder.py::build_codebook_index` renders every entry's
id, title and one-line cause into each later night's prompt, and the census
promotes candidates, with their cause, into entries.

**(g) Which gates see it** (planning-time survey; each item's facts re-checked
2026-09-22):

- Exactly two merge-tier gates can be flipped by codebook content: the
  cited-test-path ratchet (`tests/infra/test_cited_test_paths_resolve.sh`; the
  codebook-exempt branch excludes the codebook) and part B of the canonical-path
  absence invariant (`tests/infra/test_orchestrator_config_canonical_path.sh`,
  task 5242; #7788 excludes the codebook). The latter has no grandfather
  baseline.
- PTODO does not sweep `.yaml` (reify-audit's `ptodo.rs::is_swept_ext`).
- `tests/infra/test_jcodemunch_index_units.sh` keeps every absence check
  path-scoped by design; its header reason (b) cites this corpus.
- `scripts/test_legibility_reify_config.py` reads the codebook and asserts it
  is empty, but no gate runs it (#7627).

## §2 The failure class: an unattended whole-tree red

A machine commit passes its own staged gate, then fails a gate only the merge
tier runs. Nobody watches the commit, so the first observer is the next
unrelated merge. Incident: f7b607527e (2026-09-22 03:20:36) reddened every merge
until #7783's a92e5b5e49, the codebook's third grandfather commit in
`tests/infra/cited-test-path-baseline.manifest`, after 7d0181334e and
7807976e9e (#7466).

The six grandfathered codebook rows, by the trickle commit that wrote them:

| Commit | Cited unit | When written | Today |
|---|---|---|---|
| aa3ecd1db5 | `examples_smoke.rs` | resolved | stale-resolvable |
| 1cac5ea301 | `modal_options_validation_tests.rs` | resolved | stale-resolvable |
| e606a62949 | `reflection_det_negative_integration.rs` | unresolvable | stale-resolvable |
| 47f904c2ea | `registry_seed_result_types.rs` | unresolvable | stale-resolvable |
| 51b233e3b9 | flat `modal_options_validation_tests.rs` (1cac5ea301's row, cited again) | stale-resolvable | stale-resolvable |
| f7b607527e | `geometry_traits_inference_tests.rs`, `selective_demand_redemand_staleness.rs` | stale-resolvable | stale-resolvable |

*Resolved*: the cited path was a tracked file. *Unresolvable*: no unit with that
basename existed under the crate's `tests/`. *Stale-resolvable*: the path is
gone but its basename resolves elsewhere under the same crate's `tests/`, which
is what the cited-test-path gate reports.

The first four went stale only when a later merge moved or created the unit:
a1116ef21b (2026-08-22), d1d857f435 (2026-09-09), 87b744b189 (2026-09-12) and
454b4b3896 (2026-09-20). With f7b607527e that is roughly one
grandfather-worthy event every 6-8 days since 2026-08-22. Five more citations in
the file wait the same way: four resolve today, and c528d05445's
`idler_seat_e2e.rs` resolves to nothing.

To re-derive: for each trickle commit C, take the added lines of
`git diff C^ C -- docs/legibility/confusion-codebook.yaml`, extract hits with
`grep -oE "$CITED_TEST_PATH_REGEX"` (from `tests/infra/cited-test-path-lib.sh`),
and check each hit with `git cat-file -e C:<path>`, else by basename under that
crate's `tests/` at C.

## §3 Ruling

**R1.** Reify owns what may land on its `main`: its gates, each gate's charter,
and landing-path parity. Dark-factory never re-implements a reify check; a copy
would be a second source of truth across repos, drifting as reify's gates
change.

**R2.** For the codebook, the answer is the gate **charter** (mention, not use)
applied to every repo-wide resolution or absence gate. The principle is the one
the codebook-exempt branch records in `tests/infra/cited-test-path-lib.sh`'s
exclusion block, and it is not restated here. It is **not** write-time
validation, for three reasons:

1. Repointing or rewording a failed-probe record manufactures evidence in a
   corpus fed back into later prompts (§1(f)). In 47f904c2ea the stale path is
   the confusion the row exists to record (§1(d)). That, not verbatim-ness, is
   the justification. It supersedes 7807976e9e's "verbatim `evidence_quote:`"
   wording, which §1(d) shows does not hold.
2. The codebook is append-only (§1(e)), so sanitisation cannot reach committed
   sightings.
3. Four of the six grandfathered rows went stale only when a later merge moved
   or created the unit (§2). No write-time validator, dark-factory
   side or hook side, can foresee that.

Instances: the codebook-exempt branch (cited-test-path gate) and #7788's
`legacy_config_ref_exclusions` row (canonical-path gate).

**R3.** Everything still in charter gets landing-path parity.
`verify.sh::select_cheap_ptodo_gate` (#6817) is the precedent, and the
staged-cited-gate branch's `select_cited_test_path_gate` is the cited-test-path
instance. Census report and payload citations are use, not mention, so they
stay in charter.

**R4.** Dark-factory owns the writer's reaction and the writer's fidelity. Never
commit with `--no-verify`. On a refused commit: restore the written paths to
HEAD, quarantine the refused content outside the tracked tree, and do not
advance census-state. Ground `evidence_quote` in the digest.

What a refusal costs today: the dump is left uncommitted in `/home/leo/src/reify`
(`run_nightly`'s commit-failure branch; `census.py` only logs a WARNING).
`scripts/orchestrator-redeploy-restart.sh` then refuses to schedule a restart, a
dark-factory orchestrator start files a born-at-L2 dirty-tree escalation
(dark-factory's `orchestrator/src/orchestrator/harness.py::_file_dirty_tree_escalation`,
dark-factory task 2380), and the next night starts from the dirty file and
re-commits the same content. Git sends a pre-commit hook's stdout to stderr
(measured on git 2.43.0), so the writers' existing failure text already carries
the gate's findings: `commit_result.stderr` in the trickle, the `RuntimeError`
message in the census.

**Rejected alternatives:**

- Routing machine commits through the merge queue: a full `--scope all
  --profile both` gate per night for a YAML append, contrary to CLAUDE.md's
  docs-only landing rule.
- A reify hook that restores the tree on refusal: a hook must not mutate the
  writer's data.
- Dark-factory-side regex sanitisation: a second source of truth (R1), and inert
  for committed rows (R2, reason 2).
- A shared mention-corpus manifest read by every gate: deferred until a third
  consumer exists. Two pathspec rows, each pointing here, are cheaper.

## §4 Follow-ups and the interim ordering constraint

**Ordering.** The staged-cited-gate branch must not land before the
codebook-exempt branch while dark-factory leaves refused dumps in place
(esc-7788-1). Landed alone, it turns the next at-write stale citation into a
refused trickle commit and a dirty `/home/leo/src/reify`, instead of a red
merge.

**Filed from #7788:**

- dark-factory #5780 (R4): restore reify's main checkout when the trickle or
  census commit is refused.
- dark-factory #5781 (R4): ground `evidence_quote` in the digest.
- #7797 (R3): run `tests/infra/test_orchestrator_config_canonical_path.sh` on
  `--scope staged`, beside the staged-cited-gate branch's selector once it
  lands.

## §5 Re-audit triggers

1. The `fix` / `fix_where` trigger the codebook-exempt branch records with its
   exclusion. If dark-factory starts writing those remediation-shaped fields,
   part of the codebook becomes use, and both exclusions (that branch's and
   #7788's) become too coarse.
2. Any new merge-tier gate that scans inert paths repo-wide, the general hazard
   being a resolution or **absence** scan. The corpus exists to quote what
   confused agents, so it will eventually contain every stale path and banned
   literal. Such a gate needs the mention-corpus exclusion plus a
   `--scope staged` selector.
3. Any new unattended job that commits to `/home/leo/src/reify`: add it to §1
   and re-check §3. `scripts/review-readme.sh`, an unattended writer outside
   this file's legibility scope, is #7790's.
