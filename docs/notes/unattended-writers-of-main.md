# Unattended writers of reify `main`: landing, identity, publication

**Task #7790 | 2026-09-28**

An *unattended writer* is any job that commits to the main checkout
(`/home/leo/src/reify`, resolved by `scripts/lib_main_checkout.sh::reify_main_checkout`)
while no human is watching: a timer, a nightly pipeline, a deploy script. This
note is the one place that records which writers exist and the rules they
follow. The legibility writers' gate charter and refusal ruling live in
`docs/legibility/landing-contract.md`. They are referenced here, not restated.

## Rules

**W1. Land by a hook-gated `git commit --only -- <written paths>` on `main`.**
`hooks/pre-commit` gates only `main`, so the commit must be made there, and
never with `--no-verify`. `--only` keeps the commit to the paths the writer
wrote. A bare `git commit` commits the whole index, which puts any work a
human has staged in the shared checkout under the machine's name. Docs-only
writes do not go through the merge queue or `scripts/land.sh`: both force the
full `--scope all --profile both` gate (CLAUDE.md "Landing on main";
landing-contract §3 "Rejected alternatives").

**W2. Never push.** Only dark-factory's merge worker publishes to `origin`:
`git_ops.GitOps.push_main` runs after every `advance_main`
(`merge_gates.py`; `push_after_advance` defaults to true and reify does not
override it). A writer's commit therefore reaches `origin` on the next
merge-queue advance, after a merge-tier gate has verified a tree that
contains it. A writer that pushes becomes a second publisher, and the only one
that publishes a tree no merge-tier gate has seen.

**W3. Never a human identity.** The author AND the committer are
`<writer-id> <<writer-id>@automation.reify.invalid>`, set by
`GIT_AUTHOR_NAME/EMAIL` and `GIT_COMMITTER_NAME/EMAIL` assignments on the
commit command. The env outranks git config and any ambient identity. Author
matters as well as committer because rebase, cherry-pick and amend rewrite
the committer and keep the author. `.invalid` is a reserved TLD (RFC 2606/6761),
so the address can never belong to a person. No trailer: the identity fields
are the one structured record, and `--author` filters on them directly.

**W4. On refusal, follow landing-contract §3 R4.** Restore the written paths
to HEAD and quarantine the refused content outside the tracked tree, so the
main checkout is never left dirty. A dirty main checkout makes the
orchestrator's dirty-start guard refuse to restart (CLAUDE.md "Deploying the
orchestrator").

**W5. Preflight before writing.** Refuse (exit 1) when the checkout is not on
`main`, since a commit there would skip the gate (W1). Also refuse (exit 1)
when a written path is untracked at HEAD. Defer (exit 75, EX_TEMPFAIL) when a
written path already differs from HEAD in the index or the worktree. That
difference is a human's WIP, and the writer must neither commit it nor, on
refusal, restore it away.

## Inventory

| Writer | Owner | Trigger | Landing (W1) | Identity (W3) | Pushes (W2) |
|---|---|---|---|---|---|
| Legibility trickle, `scripts/legibility/nightly.py::_git_commit_docs_only` | dark-factory | `legibility-trickle@reify.timer`, daily 03:00 | hook-gated `git commit --only` | ambient (`Leo Dawn`), #7968 | no |
| Census, `scripts/legibility/census.py::_build_default_commit` | dark-factory | census trigger (landing-contract §1(b)) | hook-gated `git commit --only` | ambient (`Leo Dawn`), #7968 | no |
| `scripts/review-readme.sh` | reify | host-only `~/.config/systemd/user/reify-readme-review.timer`, every 4 days | hook-gated `git commit --only -- README.md docs/getting-started.md` | `review-readme <review-readme@automation.reify.invalid>` | no |
| Merge worker (`Merge task/N into main`) | dark-factory | merge queue | `git merge` through `hooks/pre-merge-commit` (full gate) | ambient (`Leo Dawn`); decision open in #7968 | **yes**: the sole publisher (W2) |
| `scripts/deploy/flip-reify-gate-exclude-heavy.sh` | dark-factory | manual deploy, dormant | `git commit --no-verify` (**violates W1**) | ambient | no |

`scripts/review-readme.sh` conforms to W1–W5. Its refusal (W4) takes this
form: `git diff --binary HEAD` of the targets goes to
`logs/readme-review-<ts>.refused.patch`, next to the run log (`logs/` is
untracked). The targets are then restored with
`git restore --source=HEAD --staged --worktree`, and the script exits with the
commit's status. `tests/infra/test_review_readme.py` pins this behaviour.

## Audit

```sh
git log --author=automation.reify.invalid     # every W3-conforming machine commit
git log --author='<review-readme@'            # one writer
git log -1 --format='%an <%ae> | %cn <%ce>' <sha>
```

Caveat: commits made before a writer adopted W3 still read `Leo Dawn`. That
includes review-readme's four earlier commits (04dfeef85f, bff70cede5,
8a12ead605, 96596df36f) and every dark-factory writer until #7968 lands.

## Corrections to #7790's premises

- The review-readme commit was already hook-gated. The 2026-06-04 and
  2026-06-08 run logs show `verify.sh: nothing to verify (action=all
  scope=staged)` followed by a sanctioned main-gate move. Only the push skipped
  a gate. Refusal has been a live outcome since #7831:
  `verify.sh::select_cited_test_path_gate` now runs the cited-test-path gate on
  every non-empty staged commit. Before W4, a refusal aborted the script and
  left `README.md` modified in the main checkout.
- review-readme was not the only writer that reached a remote. The merge
  worker's `push_main` already published `main` after every advance, which is
  why `origin/main` equalled `main` (39a2151a23) when this was measured.

## Residuals

- The claude session runs with `--dangerously-skip-permissions` in the shared
  main checkout. It may touch files other than the two targets. The script
  neither restores nor reports such a file, because in a shared checkout the
  edit cannot be attributed to claude, and restoring it could destroy someone
  else's work. Follow-up: #7967 (run the session in a disposable worktree).
- The timer and service units are untracked host files, and the service's
  comment still says the network is needed for "git push". Follow-up filed from
  #7790 as ticket `tkt_0RV5C38E5BV0QXTJ4TAHY4YPE5`.
- The dark-factory writers have not adopted W3, and the flip script violates
  W1. Follow-up: #7968.
