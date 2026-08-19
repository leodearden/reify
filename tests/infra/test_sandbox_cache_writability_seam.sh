#!/usr/bin/env bash
# Infrastructure test for task 5332 (OS-sandbox rollout γ6a).
#
# Landlock cache-writability seam guard: dark-factory-orchestrator.yaml's
# sandbox block (sandbox.enabled: true, backend: landlock; see PRD
# os-sandbox-worktree-containment, task γ6) makes the whole filesystem
# read-only for the three sandboxed roles (implementer, debugger,
# simple_task — roles.py sandboxed=True) except dark-factory's
# compute_write_set() (orchestrator/agents/write_set.py, DF-owned): the
# worktree, <base>/.task-meta/<name>/, main .git carve-outs, ~/.cache/uv/,
# ~/.claude/fleet/, /tmp/, and /dev/. That set has NO carve-out for
# $CARGO_HOME (~/.cargo, cargo's default) or the npm cache (~/.npm, npm's
# default).
#
# Those three are what DF sandboxes TODAY. This guard's redirect set
# (CACHE_REDIRECT_ROLES below) is deliberately WIDER — reify's half of the
# contract lands AHEAD of the DF-side sandboxed=True flip, so a role is
# redirected here BEFORE DF confines it (task 5858 added architect,
# reviewer_comprehensive and judge on exactly that basis).
#
# THE WIDER ENTRIES ARE NOT UNIFORMLY "INERT UNTIL DF SANDBOXES THE ROLE".
# workflow.py:_build_agent_env merges role_env_overrides.get(role.name, {})
# LAST for EVERY role, sandboxed or not, so what an entry actually does
# depends on that role's tool grant, not on its sandbox status:
#   implementer / debugger / simple_task — roles.py sandboxed=True today.
#     The redirect is load-bearing: without it they die on the EACCES
#     reproduced below.
#   architect — NOT sandboxed, but holds unrestricted `Bash`
#     (roles.py:401), so its entry is LIVE from the next orchestrator
#     restart: it moves the architect's ad-hoc cargo/npm off the warm
#     ~/.cargo (1.3G) / ~/.npm (6.0G) onto the shared /tmp pair. That is a
#     real behaviour change on a live role, and calling it "inert" would be
#     wrong. It is ACCEPTED because the /tmp cache is shared with, and
#     already pre-warmed by, the three sandboxed roles — so the architect
#     inherits a warm cache rather than a cold one — and its exposure to the
#     /tmp tmpfiles age-deletion gap is exactly the one implementer already
#     carries (ticket tkt_0RRSYZX0Y446V2HSW0GHHNRYYH).
#   merger — NOT sandboxed either (roles.py:45 defaults sandboxed=False and
#     MERGER does not set it), but likewise holds unrestricted `Bash`
#     (roles.py:1161) and its system prompt rule 3 directs it to "Run tests
#     after resolving", so its entry is LIVE from the next orchestrator
#     restart, on the conflict-resolution path. Accepted on exactly the
#     architect's grounds — the /tmp pair is already warm, so merger
#     inherits a warm cache rather than paying a cold start — and carrying
#     exactly the same tmpfiles age-deletion exposure
#     (tkt_0RRSYZX0Y446V2HSW0GHHNRYYH). Added by #6275; see the SECOND
#     COUPLING section below for why ADDING beat restoring the exclusion.
#   reviewer_comprehensive / judge — allowed_tools are read-only
#     (_READ_ONLY_TOOLS, roles.py:106 = Read/Glob/Grep/Bash(git:*), plus
#     verdict/jcodemunch tools), so NEITHER can invoke cargo or npm at all.
#     These two entries are genuinely inert today AND would stay inert after
#     a sandboxed=True flip; they are kept as defense-in-depth against a
#     future tool-grant widening, NOT on the "dies on its first cargo write"
#     rationale that applies to the sandboxed three.
#
# WHY `judge` IS SAFE despite workflow.py:10358's "do not add 'judge' to
# role_env_overrides": that rule is scoped to ENDPOINT env
# (ANTHROPIC_BASE_URL / ANTHROPIC_AUTH_TOKEN routing judge through vLLM,
# where max_model_len breaks tool-use after 2 rounds, 3cd380a079).
# CARGO_HOME / npm_config_cache route nothing; the global `env_overrides`
# that would carry ANTHROPIC_* is merged only for implementer/debugger and
# is not even set in reify's yaml; and judge already receives a non-None env
# dict today via apply_mcp_startup_env (mcp_lifecycle.py:51). Consumption is
# `env.update(...)` (invoke.py:277) — a merge, never a replacement. This
# reasoning is reify-side and would need re-checking if DF ever generalizes
# that merge; the entry buys defense-in-depth only, so dropping it is a
# cheap resolution if DF objects.
#
# REPRODUCED on this host with the real helper
# (orchestrator/agents/landlock_exec.py):
#   (A) uncached dep, default CARGO_HOME: `cargo fetch` -> exit 101,
#       "error: failed to open `/home/leo/.cargo/registry/cache/
#        index.crates.io-*/hex-literal-0.4.1.crate`
#        Caused by: Permission denied (os error 13)"
#   (B) default ~/.npm: `npm install` -> exit 1,
#       "Log files were not written due to an error writing to the
#        directory: /home/leo/.npm/_logs"
# Warm paths still succeed, so this fails exactly on dependency-adding /
# cold-cache tasks — routine in a Rust workspace + Tauri GUI repo.
#
# FIX: dark-factory-orchestrator.yaml's `role_env_overrides` (DF
# config.py:3801, a dict[role -> dict[env]] merged LAST into EVERY role's
# env by workflow.py:_build_agent_env — not just the three infra roles)
# redirects CARGO_HOME and npm_config_cache to /tmp paths for each
# sandboxed role. /tmp is the one host-global writable root in the write
# set that a static yaml value can name. `verify_env` (elsewhere in this
# file) is the WRONG surface FOR THIS problem: it feeds VERIFY subprocesses,
# which are not sandboxed, so setting CARGO_HOME there would not keep a
# sandboxed AGENT alive. (verify_env does now carry the same CARGO_HOME, for
# the unrelated reason in THIRD COUPLING below: fingerprint agreement over a
# shared warm target/, not write access.)
#
# This guard pins the reify-owned leaf of the contract only.
# compute_write_set itself is entirely DF-owned and not reify-observable
# from this repo; extending it (e.g. adding ~/.cargo/~/.npm carve-outs
# directly) is upstream dark-factory work, filed as dark_factory:3162
# ("add ~/.cargo and ~/.npm to compute_write_set...") -- landing that
# would retire both this guard and the /tmp redirect below.
#
# SECOND COUPLING (now PARTLY guarded -- task 5858):
#   GUARDED, both directions of a covered-set drift:
#     - MISSING side: a role silently DROPPED from the yaml fails the
#       per-role loop's role_key_present assertions (8 FAILs naming it).
#     - EXTRA side: a role given a cache redirect in the yaml but NOT
#       declared in CACHE_REDIRECT_ROLES fails no_undeclared_cache_redirect.
#       That check is deliberately scoped to the CACHE contract -- it flags
#       only entries defining CARGO_HOME/npm_config_cache -- because
#       role_env_overrides is a general-purpose DF surface whose primary
#       documented use is per-role ENDPOINT env (ANTHROPIC_BASE_URL /
#       ANTHROPIC_AUTH_TOKEN, config.py:4008). An unrelated, entirely
#       legitimate `merger: {ANTHROPIC_BASE_URL: ...}` must NOT turn this
#       cache-writability guard red and misdirect its author here.
#     - MEMBER NAMES: (C) below pins every CACHE_REDIRECT_ROLES entry
#       against KNOWN_DISPATCH_ROLES, so a typo mirrored into BOTH the yaml
#       and this file -- `simple-task`, `reviewer_comprehensiv` -- fails
#       loudly instead of passing every value-shape assertion while
#       _build_agent_env's role.name lookup silently misses.
#     - COLLAPSED-KEY TRAP: role_key_absent forbids a `reviewer:` entry
#       outright (cache-bearing or not). It passes DF's KNOWN_ROLE_NAMES
#       shape guard silently -- shared/task_metadata.py:70 lists BOTH names,
#       so _warn_unknown_role_env_overrides_keys (config.py:4003) stays mute
#       -- yet delivers NO redirect, because _build_agent_env keys on
#       role.name, which _reviewer_role (roles.py:643) builds as
#       f'reviewer_{name}' == 'reviewer_comprehensive'.
#   STILL UNGUARDED: CACHE_REDIRECT_ROLES, KNOWN_DISPATCH_ROLES and
#   UNCOVERED_DISPATCH_ROLES are all hand-maintained reify-side
#   declarations. roles.py is DF-owned and NOT reify-observable, so if DF
#   marks one of the two still-uncovered dispatch roles sandboxed -- namely
#   STEWARD or DEEP_REVIEWER -- this guard stays green while that role runs
#   with an unwritable ~/.cargo, the exact EACCES-mid-build failure this
#   guard exists to prevent. Naming those two exactly (rather than a vague
#   "some other role") is the point: a DF-side author flipping
#   sandboxed=True on either needs a breadcrumb back to this file and to
#   dark-factory-orchestrator.yaml's role_env_overrides comment.
#   merger WAS excluded on purpose, and is now COVERED. Its original
#   exclusion rationale ("redirecting the merge agent's CARGO_HOME to a cold
#   /tmp cache would diverge it from the verify subprocess cache path on the
#   one gate that lands everything") was INVERTED by task 5836, which moved
#   verify_env.CARGO_HOME onto that same /tmp path: with merger left on the
#   default ~/.cargo, the divergence the exclusion existed to prevent
#   existed in the OTHER direction. Task 6275 re-evaluated the exclusion on
#   that repaired footing and resolved it by ADDING merger to the redirect
#   rather than restoring the exclusion on a rewritten rationale.
#   WHY ADD BEAT RESTORE-THE-EXCLUSION. Merger's low frequency bounds the
#   BENEFIT, not the cost, so it argues low URGENCY rather than wrongness,
#   and the payoff is asymmetric. The entry costs ~zero: the /tmp pair is
#   already warmed by the three sandboxed roles plus every DF-launched
#   verify, so merger inherits a warm cache instead of paying a cold start
#   -- the identical acceptance task 5858 granted `architect`, also
#   unsandboxed and also holding unrestricted Bash. On the invocations where
#   merger DOES compile, leaving it on ~/.cargo dirties the registry deps in
#   the shared target/ and the following verify re-dirties them back:
#   cargo's per-unit Fingerprint embeds the registry source path while
#   `-C metadata` is CARGO_HOME-independent, so both homes overwrite the
#   SAME .fingerprint file and the thrash is bidirectional and recurring,
#   not a one-time switching cost. Accepted cost: it extends the /tmp
#   tmpfiles age-deletion exposure (tkt_0RRSYZX0Y446V2HSW0GHHNRYYH) to one
#   more role, identically to architect's.
#   MEASURED EXPOSURE (re-derived for #6275 on 2026-08-19; the task asked
#   for merger's own measurement rather than an assumed frequency):
#     - merger is 391 of 36,076 agent invocations = 1.08% of all roles, and
#       1.15% of the nine-dispatch-role subset. Source:
#       data/orchestrator/runs.db table `invocations` (project reify,
#       2026-04-07..2026-08-19), read-only; re-run with
#       `sqlite3 "file:<repo>/data/orchestrator/runs.db?mode=ro" "SELECT
#       role, COUNT(*) FROM invocations GROUP BY role ORDER BY 2 DESC;"`.
#     - ALL of those are conflict-resolution runs, by construction, not by
#       sampling: MERGER has exactly ONE dispatch site in the orchestrator,
#       workflow.py:_resolve_conflicts (the `_invoke(MERGER, ...)` call).
#       There is no non-conflict merger dispatch, so the "bounded to merges
#       that hit a conflict" caveat is already priced into the 1.08%.
#     - WHETHER MERGER ACTUALLY RUNS CARGO IS **NOT INSTRUMENTED** on this
#       host, and this is a negative finding, not a proxy. runs.db records
#       role/model/cost/tokens/duration per invocation and no tool calls;
#       the only place commands survive is the retained
#       data/orchestrator/agent-transcripts/ tree, whose retention is
#       partial -- 7 genuine merger sessions survive against 391 recorded
#       invocations (~1.8%). Of those 7, 2 issued at least one `cargo`
#       command. N=7 is an observation that the "Run tests after resolving"
#       directive is sometimes obeyed -- so the entry is not vacuous -- and
#       is FAR too small to be a rate. Do not quote it as one.
#   Even with merger added, do NOT "close the gap" by blanket-adding every
#   role in DF's ROLES registry: reviewer_comprehensive and judge are here
#   as defense-in-depth against a tool-grant widening, not because every
#   name belongs. The residual uncovered set is now pinned mechanically by
#   (C)'s complement assertion against UNCOVERED_DISPATCH_ROLES, so a
#   blanket add fails that assertion until the author states the new split
#   deliberately.
#   This paragraph is the AUTHORITATIVE statement of the coupling and its
#   resolution; dark-factory-orchestrator.yaml's role_env_overrides comment
#   deliberately carries only the conclusion plus a pointer here, so a
#   future re-evaluation does not have to edit two full copies in lockstep.
#   That split is not decorative: the two copies drifted once already, which
#   is what #6275 had to repair.
#
# THIRD COUPLING -- CARGO_HOME AGREEMENT ACROSS A SHARED target/ (task 5836,
# guarded by (D)+(E) below). Distinct from the two above: those are about
# WRITE ACCESS (a sandboxed agent dying on an unwritable ~/.cargo). This one
# is about FINGERPRINT IDENTITY, and its failure mode is silent slowness
# rather than an error, which is why it needs a test rather than review.
#   THREE CONSUMERS write and re-read one warm lane's target/, and all three
#   must agree on CARGO_HOME:
#     1. the base-warming merge-verify build, whose target/
#        scripts/refresh-warm-base.sh promotes to the pool base;
#     2. the landlock-sandboxed agent roles, during their TDD turns;
#     3. verify subprocesses, which are NOT sandboxed.
#   Cargo's per-unit Fingerprint embeds `path = hash_u64(path_args(...))`,
#   and for a REGISTRY package that is the absolute
#   $CARGO_HOME/registry/src/index.crates.io-<hash>/<crate>/src/... path. So
#   two consumers disagreeing on CARGO_HOME invalidate each other's units --
#   cargo's own words, "Dirty <crate>: the path to the source changed",
#   cascading into "Dirty <workspace crate>: info of dependency <crate>
#   changed". Reify's Cargo.lock carries 685 registry entries.
#   WHY /tmp AND NOT ~/.cargo: the agent side cannot move. DF's
#   compute_write_set grants no ~/.cargo carve-out and /tmp is the one
#   host-global writable root a static yaml value can name, so agents are
#   PINNED there; verify and merge-verify are unsandboxed and are therefore
#   the movable side. The upstream change that would retire the redirect and
#   let all three sit on ~/.cargo is dark_factory:3162, UNLANDED (reify 5724
#   was cancelled and refiled there).
#   WHY SEEDING CANNOT SUBSTITUTE FOR AGREEMENT -- the tempting wrong fix:
#     - It was never an mtime problem. Cargo stamps extracted sources with a
#       canonical fixed mtime; crates present in BOTH homes read the
#       identical stamp (1153704088) on either side, different inodes.
#       Freshness here never turned on mtime, so pre-seeding registry/src
#       (task 5729) fixes cold START and leaves fingerprint IDENTITY exactly
#       as split as before.
#     - "Just warm two caches" is structurally impossible, not merely
#       expensive: `-C metadata` is CARGO_HOME-INDEPENDENT, so both homes
#       write the SAME .fingerprint/<pkg>-<hash> file and each overwrites
#       the other. One target/ holds exactly one fingerprint set per unit.
#       Measured bidirectional: home-a (all Fresh) -> home-b (Dirty) ->
#       home-a again (Dirty again).
#   WHAT (D)+(E) STILL CANNOT OBSERVE -- limits, stated so this guard's
#   green is not read as more coverage than it has. It reads the YAML, not
#   the running orchestrator, so it CANNOT:
#     - prove the activating restart happened. The yaml is restart-tier
#       (loaded once at startup), so (D) goes green the moment the file is
#       edited while the live orchestrator may still be running the old
#       split config, for an unbounded time.
#     - prove merge-verify actually BUILT the promoted warm base under this
#       CARGO_HOME. refresh-warm-base.sh promotes whatever target/ it is
#       given; nothing here inspects the promoted base's dep-info paths.
#     - detect a target/ that is ALREADY split from before the flip. Lanes
#       carrying pre-flip fingerprints keep paying one rebuild until they
#       are re-seeded, and that shows up as slowness, never as a red test.
#     - cover a verify that dark-factory did not launch. verify_env is
#       applied only when DF spawns the verify subprocess; scripts/verify.sh
#       sets no CARGO_HOME of its own, and neither does hooks/pre-merge-commit
#       nor scripts/land.sh (measured: no CARGO_HOME reference in any of the
#       three). So a hand-run `bash scripts/verify.sh` in a warm lane — and
#       the land.sh -> pre-merge-commit path — inherits the ambient value,
#       normally unset = ~/.cargo, and re-splits exactly the fingerprint set
#       this flip unified, leaving the target/ in the other state for the
#       next agent turn. (D)'s invariant therefore holds for
#       orchestrator-launched verifies only. Tracked as #5853.
#     - see the .package-cache LOCK population the flip CONSOLIDATES. Before
#       it, verifies contended on ~/.cargo/.package-cache and sandboxed
#       agents on the /tmp one; after it, every warm-lane verify AND every
#       agent take the single /tmp/reify-agent-cargo-home/.package-cache.
#       Bounded rather than free: cargo BLOCKS on that lock instead of
#       failing, and scripts/assert-crate-dag.sh:31-37 already retries 5x on
#       the empty-`cargo metadata` stdout that a sibling lock holder produced
#       in esc-4419-41. Watch for a resurgence of that exact shape after the
#       activating restart -- the fingerprint win is expected to dominate,
#       but nothing in this guard can observe the trade.
#   To observe the first three you must inspect the artifacts, not the
#   config -- e.g. count target/debug/deps/*.d dep-info files referencing
#   each home in a live lane, which is how the original split was found
#   (105 under /tmp alongside 1176 under ~/.cargo, in one pool lane). The
#   last two are runtime behaviours (whose shell launched verify; who holds
#   the package-cache lock) and are outside any config-parsing guard.
#
# Pins, against dark-factory-orchestrator.yaml:
#   (A) STRUCTURE (python3+PyYAML, SKIP-guarded): IF sandbox.enabled is
#       true, THEN for EACH role in CACHE_REDIRECT_ROLES,
#       role_env_overrides[<role>] defines BOTH CARGO_HOME
#       and npm_config_cache, each an absolute path lying under /tmp (the
#       landlock write set's host-global writable root) and explicitly
#       NOT under $HOME/.cargo or $HOME/.npm — the exact regression this
#       guard exists to catch. Gated on sandbox.enabled so it stays
#       correct if sandbox is ever flipped back off (mirrors the
#       precondition-gated structure of
#       test_merge_role_lint_first_seam.sh's concurrent_verify guard).
#       PLUS, after the per-role loop:
#         - no_undeclared_cache_redirect: no role_env_overrides entry
#           defines CARGO_HOME or npm_config_cache unless it is declared in
#           CACHE_REDIRECT_ROLES. Closes the extra-side half of a
#           covered-set drift (a silent yaml-only widening of the redirect)
#           WITHOUT firing on an unrelated per-role endpoint override,
#           which is role_env_overrides' other documented use.
#         - role_key_absent reviewer: the collapsed 'reviewer' key is
#           NOT present. It would validate cleanly against DF's
#           KNOWN_ROLE_NAMES yet deliver no redirect at all. Not subsumed
#           by the check above: this forbids ANY `reviewer:` entry, not
#           just a cache-bearing one.
#   (B) KNOB-NAME CROSS-CHECK (plain bash grep, always runs, no python
#       needed): `backend: landlock` and `enabled: true` are cited under
#       the sandbox block, and `role_env_overrides:`, `CARGO_HOME:` and
#       `npm_config_cache:` are all cited verbatim in the yaml.
#   (C) ROLE-NAME DECLARATION CROSS-CHECK (plain bash, always runs, does
#       not read the yaml at all), two assertions:
#       (a) SUBSET -- every CACHE_REDIRECT_ROLES member is a member of
#           KNOWN_DISPATCH_ROLES, the hand-mirrored copy of DF's ROLES
#           registry (roles.py:1512). Catches a misspelled member that
#           would satisfy every value-shape assertion in (A) while
#           _build_agent_env's role.name lookup silently missed.
#       (b) COMPLEMENT (#6275) -- KNOWN_DISPATCH_ROLES minus
#           CACHE_REDIRECT_ROLES equals UNCOVERED_DISPATCH_ROLES, compared
#           as an ORDER-INSENSITIVE set so reordering either declaration is
#           not a spurious FAIL. (a) alone is blind to which roles are left
#           OUT; that residual set lived only in prose, in two files, and
#           both copies went stale the moment merger joined the redirect.
#       Together these convert the nine role names this header states in
#       prose -- and the covered/uncovered split between them -- into an
#       executable constraint. (a) alone only half-served that claim.
#   (D) CARGO_HOME AGREEMENT (task 5836; python3+PyYAML, gated on the SAME
#       sandbox.enabled precondition as (A) -- with sandbox off there is no
#       agent-side redirect for verify to agree WITH): verify_env.CARGO_HOME
#       is present, absolute, under /tmp, and byte-equal to
#       role_env_overrides.<role>.CARGO_HOME for EVERY member of
#       CACHE_REDIRECT_ROLES.
#   (E) FLIP PRECONDITIONS (task 5836; same sandbox.enabled gate as (D)):
#       (a) `cargo nextest --version` succeeds under the CONFIGURED
#           verify_env.CARGO_HOME (parsed from the yaml, not hardcoded).
#           That home has no bin/ of its own, so this passes only via the
#           ~/.cargo/bin PATH fallthrough. Control arm: if nextest also
#           fails under the default (unset) CARGO_HOME it is absent from
#           the host, reported SKIP rather than misattributed to the
#           redirect. (b) verify_env does NOT carry npm_config_cache --
#           a deliberate asymmetry, pinned so it stays deliberate.
#
# (A) is SKIPPED if python3 + PyYAML are unavailable (mirrors the SKIP
#     idiom used by test_merge_role_lint_first_seam.sh /
#     test_cpu_governance_config.sh).
# (B) and (C) always run — plain bash, no python needed.
#
# RED at authoring time (task 5332): role_env_overrides is absent from the
# yaml entirely while sandbox.enabled is true.
# RED when the redirect set was widened (task 5858): architect,
# reviewer_comprehensive and judge are required by CACHE_REDIRECT_ROLES but
# absent from role_env_overrides, so the per-role loop reports 8 FAILs for
# each of the three.
# RED when (D) was added (task 5836): verify_env carried no CARGO_HOME at
# all, so (D) reported 9 FAILs -- 3 shape plus one equality per redirected
# role -- against a green 63-assertion baseline.
# RED when merger was added to the redirect (task 6275): merger is required
# by CACHE_REDIRECT_ROLES but absent from role_env_overrides, so (A)'s
# per-role loop reports 9 FAILs plus 1 from (D)'s equality check, against a
# green 74-assertion baseline.
# RED when (C)'s complement assertion was added (task 6275): the new
# UNCOVERED_DISPATCH_ROLES declaration was seeded with the value this
# header and the yaml BOTH still asserted in prose at the time -- "merger
# steward deep_reviewer" -- while the computed complement was already
# "steward deep_reviewer". 84 passed, 1 failed, the diagnostic naming the
# drifted role ("declared uncovered but IS redirected: merger"). Seeding
# from the stale prose rather than the true value is what made that red a
# measurement of the prose instead of a self-fulfilling assertion.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== sandbox cache-writability seam contract tests (task 5332) ==="

ORCH_YAML="$REPO_ROOT/dark-factory-orchestrator.yaml"
# The roles whose toolchain caches reify's yaml REDIRECTS -- deliberately a
# SUPERSET of DF's roles.py sandboxed=True set, because reify's half of the
# contract lands AHEAD of the DF-side sandboxed=True flip (see the file
# header). Not reify-observable to keep in sync automatically; a DF-side role
# addition needs a manual update here.
CACHE_REDIRECT_ROLES="implementer debugger simple_task architect reviewer_comprehensive judge merger"

# Hand-mirrored copy of DF's ROLES dispatch registry (roles.py:1512) -- the
# exact set of names _build_agent_env can ever look up via
# role_env_overrides.get(role.name, {}). Its only job is to catch a MISSPELLED
# member of CACHE_REDIRECT_ROLES: a typo mirrored into both that list and the
# yaml (`simple-task`, `reviewer_comprehensiv`, `deep-reviewer`) passes every
# value-shape assertion in (A) while the real role silently gets no redirect
# at all. Same accepted shape as CACHE_REDIRECT_ROLES itself -- a
# hand-maintained mirror of a DF-owned fact reify cannot observe.
KNOWN_DISPATCH_ROLES="architect implementer debugger merger steward deep_reviewer reviewer_comprehensive judge simple_task"

# The residual UNCOVERED dispatch roles -- the members of
# KNOWN_DISPATCH_ROLES that CACHE_REDIRECT_ROLES does NOT redirect, i.e. the
# roles whose CARGO_HOME/npm_config_cache still resolve to the DEFAULT
# ~/.cargo and ~/.npm. Same accepted shape as CACHE_REDIRECT_ROLES and
# KNOWN_DISPATCH_ROLES themselves: a hand-maintained mirror, kept honest by
# the (C) assertion below rather than by anything reify can observe.
#
# WHAT THIS PIN CANNOT OBSERVE (same disclosure habit as the "WHAT (D)+(E)
# STILL CANNOT OBSERVE" section above): it constrains REIFY'S DECLARED
# COVERED SET against reify's own mirror of the role registry -- nothing
# more. It does NOT read DF's roles.py, so a DF-side author flipping
# sandboxed=True on `steward` or `deep_reviewer` still gets NO red here;
# that role would begin running under landlock with an unwritable ~/.cargo
# and this suite would stay green. What the pin does buy is that the
# reify-side coverage set can no longer change SILENTLY: adding a role to
# (or removing one from) CACHE_REDIRECT_ROLES now fails until the author
# consciously edits this line, which drags them back to the prose that
# describes the split.
UNCOVERED_DISPATCH_ROLES="steward deep_reviewer"

# ---------------------------------------------------------------------------
# (A) STRUCTURE -- parse YAML and assert key/value shape
# ---------------------------------------------------------------------------

if ! python3 -c 'import yaml' 2>/dev/null; then
    echo "SKIP: python3 'yaml' (PyYAML) not available; skipping YAML structure assertions"
else
    echo "--- (A) structural assertions via PyYAML ---"

    _PARSE_PY="$(mktemp /tmp/sandbox_cache_seam_parse_XXXXXX.py)"
    trap 'rm -f "$_PARSE_PY"' EXIT

    cat > "$_PARSE_PY" << 'PYEOF'
"""Validate dark-factory-orchestrator.yaml's sandbox cache-writability
contract (landlock toolchain-cache redirect, task 5332 / os-sandbox rollout
gamma6a).

Usage:
  python3 <script> <orch_yaml> <check> [args...]

Checks:
  parse_ok                                 -- file parses as valid YAML
  sandbox_enabled_true                     -- top-level sandbox.enabled == true
  role_key_present <role> <key>            -- role_env_overrides[role][key]
                                               exists, is a string, and is
                                               non-empty
  role_key_absolute <role> <key>           -- ...and is an absolute path
  role_key_under_tmp <role> <key>          -- ...and lies STRICTLY under
                                               /tmp (a proper descendant --
                                               /tmp itself is rejected) --
                                               /tmp is the one host-global
                                               writable root in the landlock
                                               write set a static yaml value
                                               can name; see orchestrator/
                                               agents/write_set.py
  role_key_not_under <role> <key> <prefix> -- ...and does NOT lie under
                                               <prefix> (used to pin it is
                                               NOT under $HOME/.cargo or
                                               $HOME/.npm -- the exact
                                               regression this guard exists
                                               to catch)
  role_keys_distinct <role> <k1> <k2>      -- role_env_overrides[role][k1]
                                               != role_env_overrides[role][k2]
                                               (catches a degenerate same-dir
                                               target: cargo's and npm's
                                               cache-directory layouts would
                                               collide if CARGO_HOME and
                                               npm_config_cache pointed at
                                               the same path)
  no_undeclared_cache_redirect <role...>   -- every role_env_overrides entry
                                               that defines CARGO_HOME or
                                               npm_config_cache is a member
                                               of the passed role set. Closes
                                               the EXTRA-side half of a
                                               covered-set drift: a silent
                                               yaml-only widening of the
                                               redirect (e.g. quietly undoing
                                               the merger exclusion) fails
                                               here, so any covered-set change
                                               must be a deliberate, reviewed
                                               edit to CACHE_REDIRECT_ROLES in
                                               the caller -- which is where
                                               the header note explaining the
                                               coupling lives. Deliberately
                                               NOT an exact-set check:
                                               role_env_overrides is a
                                               general-purpose DF surface
                                               whose other documented use is
                                               per-role ENDPOINT env
                                               (ANTHROPIC_BASE_URL etc.,
                                               config.py:4008), and an
                                               unrelated entry of that kind
                                               must not turn this
                                               cache-writability guard red.
                                               The MISSING side needs no
                                               check here -- role_key_present
                                               above already fails loudly for
                                               a dropped role.
  role_key_absent <role>                   -- role_env_overrides has NO entry
                                               for <role> AT ALL, cache-
                                               bearing or not (pins the inert
                                               collapsed-key trap; see the
                                               'reviewer' assertion in the
                                               caller)

Section-aware variants (task 5836), for the ONE-level top-level mappings —
`verify_env` — as opposed to role_env_overrides' TWO-level role -> env -> value
shape. Same three predicates as the role_* trio above, generalized from "a key
under role_env_overrides[<role>]" to "a key under a named top-level mapping":

  top_key_present <section> <key>          -- d[section][key] exists, is a
                                               string, and is non-empty
  top_key_absolute <section> <key>         -- ...and is an absolute path
  top_key_under_tmp <section> <key>        -- ...and lies STRICTLY under /tmp
                                               (same strict-descendant rule,
                                               same rationale, as
                                               role_key_under_tmp above)
  top_key_absent <section> <key>           -- d[section] has NO <key> at all.
                                               Pins a DELIBERATE asymmetry, not
                                               an oversight (see the
                                               npm_config_cache assertion in
                                               the caller)
  top_key_print <section> <key>            -- PRINT the value to stdout instead
                                               of asserting on it (exit 1 with
                                               no output if absent/empty), so a
                                               bash caller can drive a real
                                               subprocess with the configured
                                               value rather than a second
                                               hardcoded copy of it
  cross_section_key_equals <section> <key> <role> <rolekey>
                                           -- d[section][key] is EQUAL to
                                               role_env_overrides[role][rolekey].
                                               The anti-split invariant: cargo's
                                               per-unit Fingerprint embeds the
                                               registry source path, so if
                                               verify and the sandboxed agents
                                               disagree on CARGO_HOME they
                                               invalidate each other's units in
                                               a SHARED warm-lane target/ and
                                               each rebuild all 685 registry
                                               deps. A divergence here has no
                                               symptom other than slowness,
                                               which is exactly why it needs a
                                               test rather than review.

Exit 0 on pass, 1 on fail (or missing key/role), 2 on unknown check/bad args.
"""
import sys
import yaml
from pathlib import Path

orch_yaml_path = sys.argv[1]
check = sys.argv[2]
rest = sys.argv[3:]

with open(orch_yaml_path) as f:
    d = yaml.safe_load(f)


def _role_value(role, key):
    overrides = d.get("role_env_overrides")
    if not isinstance(overrides, dict):
        return None
    role_map = overrides.get(role)
    if not isinstance(role_map, dict):
        return None
    val = role_map.get(key)
    if not isinstance(val, str) or val.strip() == "":
        return None
    return val


def _top_value(section, key):
    """Value of d[<section>][<key>], or None unless it is a non-empty string.

    Deliberately a SEPARATE accessor from _role_value rather than a shared
    generic walker: role_env_overrides is a TWO-level mapping
    (role -> env -> value) while verify_env is ONE level (key -> value), so
    the two lookups differ in arity and collapsing them would only obscure
    which shape a given check expects.
    """
    section_map = d.get(section)
    if not isinstance(section_map, dict):
        return None
    val = section_map.get(key)
    if not isinstance(val, str) or val.strip() == "":
        return None
    return val


if check == "parse_ok":
    sys.exit(0)

if check == "sandbox_enabled_true":
    sandbox = d.get("sandbox")
    ok = isinstance(sandbox, dict) and sandbox.get("enabled") is True
    sys.exit(0 if ok else 1)

if check == "role_key_present":
    role, key = rest
    sys.exit(0 if _role_value(role, key) is not None else 1)

if check == "role_key_absolute":
    role, key = rest
    val = _role_value(role, key)
    sys.exit(0 if val is not None and Path(val).is_absolute() else 1)

if check == "role_key_under_tmp":
    role, key = rest
    val = _role_value(role, key)
    if val is None:
        sys.exit(1)
    p = Path(val)
    # Strict descendant only -- /tmp itself is deliberately REJECTED here.
    # CARGO_HOME: /tmp (or npm_config_cache: /tmp) would pass every other
    # assertion in this suite yet make cargo/npm scatter registry/, git/,
    # bin/, .package-cache (or npm's cacache tree) directly into the
    # world-writable shared /tmp root of a many-concurrent-task host --
    # colliding with every other tenant of /tmp, inside the landlock write
    # set, so nothing else would catch it either.
    sys.exit(0 if Path("/tmp") in p.parents else 1)

if check == "role_keys_distinct":
    role, key1, key2 = rest
    val1 = _role_value(role, key1)
    val2 = _role_value(role, key2)
    if val1 is None or val2 is None:
        sys.exit(1)
    sys.exit(0 if val1 != val2 else 1)

if check == "role_key_not_under":
    role, key, prefix = rest
    val = _role_value(role, key)
    if val is None:
        sys.exit(1)
    # Plain string-prefix containment (deliberately not a resolve()-based
    # check -- symlink resolution could mask the exact literal-default
    # regression this guard exists to catch).
    norm_val = str(Path(val))
    norm_prefix = str(Path(prefix))
    sys.exit(1 if (norm_val == norm_prefix or norm_val.startswith(norm_prefix + "/")) else 0)

if check == "no_undeclared_cache_redirect":
    if not rest:
        print("no_undeclared_cache_redirect needs at least one role", file=sys.stderr)
        sys.exit(2)
    overrides = d.get("role_env_overrides")
    if not isinstance(overrides, dict):
        print("role_env_overrides is absent or not a mapping", file=sys.stderr)
        sys.exit(1)
    declared = set(rest)
    # Scoped to the CACHE contract on purpose: an entry that carries only
    # endpoint env (ANTHROPIC_BASE_URL/AUTH_TOKEN -- role_env_overrides' other
    # documented use, config.py:4008) is none of this guard's business.
    undeclared = sorted(
        role
        for role, role_map in overrides.items()
        if isinstance(role_map, dict)
        and any(k in role_map for k in ("CARGO_HOME", "npm_config_cache"))
        and role not in declared
    )
    if undeclared:
        # The `assert` helper dumps captured output only on FAIL, so naming
        # the offending roles turns a bare non-zero exit into a directly
        # actionable message.
        print(
            "role_env_overrides entries define a toolchain-cache redirect but are "
            f"NOT declared in CACHE_REDIRECT_ROLES: {undeclared}",
            file=sys.stderr,
        )
        sys.exit(1)
    sys.exit(0)

if check == "role_key_absent":
    (role,) = rest
    overrides = d.get("role_env_overrides")
    if not isinstance(overrides, dict):
        # Trivially absent. Every other assertion already fails loudly in
        # this state, so reporting a pass here is honest rather than masking.
        sys.exit(0)
    if role in overrides:
        print(f"role_env_overrides carries the key {role!r}, which must be absent", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

if check == "top_key_present":
    section, key = rest
    sys.exit(0 if _top_value(section, key) is not None else 1)

if check == "top_key_absolute":
    section, key = rest
    val = _top_value(section, key)
    sys.exit(0 if val is not None and Path(val).is_absolute() else 1)

if check == "top_key_under_tmp":
    section, key = rest
    val = _top_value(section, key)
    if val is None:
        sys.exit(1)
    # Strict descendant only -- /tmp itself is REJECTED, for the same reason
    # spelled out under role_key_under_tmp above.
    sys.exit(0 if Path("/tmp") in Path(val).parents else 1)

if check == "top_key_absent":
    section, key = rest
    section_map = d.get(section)
    if not isinstance(section_map, dict):
        # Trivially absent. Every other assertion against this section already
        # fails loudly in that state, so passing here is honest, not masking.
        sys.exit(0)
    if key in section_map:
        print(
            f"{section} carries the key {key!r}, whose ABSENCE is deliberate "
            "-- see the caller's rationale before adding it",
            file=sys.stderr,
        )
        sys.exit(1)
    sys.exit(0)

if check == "top_key_print":
    section, key = rest
    val = _top_value(section, key)
    if val is None:
        sys.exit(1)
    print(val)
    sys.exit(0)

if check == "cross_section_key_equals":
    section, key, role, rolekey = rest
    section_val = _top_value(section, key)
    role_val = _role_value(role, rolekey)
    if section_val is None or role_val is None:
        print(
            f"missing value: {section}.{key}={section_val!r}, "
            f"role_env_overrides.{role}.{rolekey}={role_val!r}",
            file=sys.stderr,
        )
        sys.exit(1)
    # Exact scalar equality, NOT a normalized/resolved path comparison: cargo
    # hashes the CARGO_HOME-derived source path as a STRING into each unit's
    # fingerprint, so two spellings that resolve to the same directory (a
    # trailing slash, a symlink, a `/tmp/./x`) still produce DIFFERENT
    # fingerprints and still thrash. Normalizing here would hide exactly the
    # divergence this check exists to catch.
    if section_val != role_val:
        print(
            f"toolchain-cache SPLIT: {section}.{key}={section_val!r} != "
            f"role_env_overrides.{role}.{rolekey}={role_val!r}. A warm-lane "
            "target/ is shared between verify and the sandboxed agent, and "
            "cargo's per-unit fingerprint embeds the registry source path, so "
            "this divergence makes each side dirty the other's units and "
            "rebuild the whole registry dep tree.",
            file=sys.stderr,
        )
        sys.exit(1)
    sys.exit(0)

print(f"unknown check or bad args: {check} {rest}", file=sys.stderr)
sys.exit(2)
PYEOF

    assert "dark-factory-orchestrator.yaml parses as valid YAML" \
        python3 "$_PARSE_PY" "$ORCH_YAML" parse_ok

    if python3 "$_PARSE_PY" "$ORCH_YAML" sandbox_enabled_true; then
        echo "sandbox.enabled == true; asserting cache-writability contract for cache-redirect roles ($CACHE_REDIRECT_ROLES)"
        for role in $CACHE_REDIRECT_ROLES; do
            for key in CARGO_HOME npm_config_cache; do
                assert "role_env_overrides.${role}.${key} is present and non-empty" \
                    python3 "$_PARSE_PY" "$ORCH_YAML" role_key_present "$role" "$key"

                assert "role_env_overrides.${role}.${key} is an absolute path" \
                    python3 "$_PARSE_PY" "$ORCH_YAML" role_key_absolute "$role" "$key"

                assert "role_env_overrides.${role}.${key} lies under /tmp (the landlock write set's host-global writable root)" \
                    python3 "$_PARSE_PY" "$ORCH_YAML" role_key_under_tmp "$role" "$key"
            done

            assert "role_env_overrides.${role}.CARGO_HOME is NOT under \$HOME/.cargo (the regression this guard exists to catch)" \
                python3 "$_PARSE_PY" "$ORCH_YAML" role_key_not_under "$role" CARGO_HOME "$HOME/.cargo"

            assert "role_env_overrides.${role}.npm_config_cache is NOT under \$HOME/.npm (the regression this guard exists to catch)" \
                python3 "$_PARSE_PY" "$ORCH_YAML" role_key_not_under "$role" npm_config_cache "$HOME/.npm"

            assert "role_env_overrides.${role}.CARGO_HOME and .npm_config_cache are distinct paths (a same-dir target would pass every check above yet collide cargo's and npm's cache layouts)" \
                python3 "$_PARSE_PY" "$ORCH_YAML" role_keys_distinct "$role" CARGO_HOME npm_config_cache
        done

        # Unquoted expansion is intended -- the checker takes the roles as
        # separate argv entries.
        # shellcheck disable=SC2086
        assert "no role_env_overrides entry defines a toolchain-cache redirect without being declared in CACHE_REDIRECT_ROLES (catches a silent yaml-only widening of the covered set; deliberately ignores unrelated per-role endpoint overrides, which are role_env_overrides' other documented use)" \
            python3 "$_PARSE_PY" "$ORCH_YAML" no_undeclared_cache_redirect $CACHE_REDIRECT_ROLES

        assert "role_env_overrides does NOT carry the collapsed 'reviewer' key at all (KNOWN_ROLE_NAMES accepts it, but _build_agent_env keys on role.name == 'reviewer_comprehensive', so a 'reviewer' entry is silently INERT -- no DF config warning, no redirect)" \
            python3 "$_PARSE_PY" "$ORCH_YAML" role_key_absent reviewer

        # -------------------------------------------------------------------
        # (D) CARGO_HOME AGREEMENT -- verify_env vs role_env_overrides
        #     (task 5836; gated on the SAME sandbox_enabled_true precondition
        #     as (A) above, because with sandbox off there is no agent-side
        #     redirect for verify to agree WITH)
        # -------------------------------------------------------------------
        echo "--- (D) CARGO_HOME agreement (verify_env vs role_env_overrides) ---"

        assert "verify_env.CARGO_HOME is present and non-empty (verify subprocesses are NOT sandboxed, so nothing forces this -- but they share the warm lane's target/ with the sandboxed agent that built it, and leaving verify on the default ~/.cargo is the pre-5836 split)" \
            python3 "$_PARSE_PY" "$ORCH_YAML" top_key_present verify_env CARGO_HOME

        assert "verify_env.CARGO_HOME is an absolute path" \
            python3 "$_PARSE_PY" "$ORCH_YAML" top_key_absolute verify_env CARGO_HOME

        assert "verify_env.CARGO_HOME lies under /tmp (not a free choice: /tmp is the one host-global writable root in the landlock write set, which PINS the agent side and therefore fixes where the two sides can meet)" \
            python3 "$_PARSE_PY" "$ORCH_YAML" top_key_under_tmp verify_env CARGO_HOME

        # THE load-bearing invariant of this block. Iterating CACHE_REDIRECT_ROLES
        # rather than checking one representative role keeps agreement exact even
        # if the yaml's `&sandbox_cache_env` anchor is ever expanded into per-role
        # literals and one of them drifts.
        for role in $CACHE_REDIRECT_ROLES; do
            assert "verify_env.CARGO_HOME == role_env_overrides.${role}.CARGO_HOME (anti-split invariant: they share one warm target/, and a divergence has NO symptom except every sandboxed task rebuilding the whole registry dep tree twice -- agent, then verify)" \
                python3 "$_PARSE_PY" "$ORCH_YAML" cross_section_key_equals verify_env CARGO_HOME "$role" CARGO_HOME
        done

        # -------------------------------------------------------------------
        # (E) PRECONDITIONS THAT MAKE THE (D) FLIP SAFE AND BOUNDED (task 5836)
        # -------------------------------------------------------------------
        echo "--- (E) verify_env flip preconditions (toolchain resolution; deliberate asymmetry) ---"

        # (E)(a) TOOLCHAIN-RESOLUTION PRECONDITION.
        # Non-obvious and load-bearing. The redirect destination has NO bin/ at
        # all -- it holds registry/ plus cargo's lock files, nothing else -- so
        # `cargo nextest` under it resolves ONLY because ~/.cargo/bin is on PATH
        # and cargo's subcommand lookup falls through from $CARGO_HOME/bin to
        # PATH. (RUSTC_WRAPPER=sccache in verify_env resolves the same way.) If
        # PATH ever loses ~/.cargo/bin, verify breaks confusingly at nextest
        # rather than here, at the seam that caused it. Driven off the value
        # PARSED from the yaml, never a second hardcoded copy, so the probe
        # cannot silently test a path the config no longer names.
        _VERIFY_CARGO_HOME="$(python3 "$_PARSE_PY" "$ORCH_YAML" top_key_print verify_env CARGO_HOME || true)"

        if [ -z "$_VERIFY_CARGO_HOME" ]; then
            echo "SKIP: verify_env.CARGO_HOME is absent/empty, so there is no configured value to probe (the (D) assertions above already report that as FAIL -- not re-reported here as a second failure for one cause)"
        elif ! env -u CARGO_HOME cargo nextest --version >/dev/null 2>&1; then
            # CONTROL ARM. Deliberately `env -u CARGO_HOME` rather than the
            # AMBIENT env: this suite is itself run by a sandboxed agent whose
            # ambient CARGO_HOME is already the redirect value, which would make
            # the control and the probe the same measurement. Unsetting it tests
            # cargo's genuine default (~/.cargo) whoever invokes the suite. A
            # host where nextest is simply not installed must report SKIP, not a
            # FAIL misattributed to the redirect.
            echo "SKIP: 'cargo nextest --version' also fails under the DEFAULT (unset) CARGO_HOME, so nextest is genuinely unavailable on this host rather than broken by the redirect (control arm)"
        else
            assert "cargo nextest resolves under the configured verify_env.CARGO_HOME ($_VERIFY_CARGO_HOME), which has no bin/ of its own -- it works only via the ~/.cargo/bin PATH fallthrough, and the control arm above has already established nextest IS available on this host" \
                env CARGO_HOME="$_VERIFY_CARGO_HOME" cargo nextest --version
        fi

        # (E)(b) DELIBERATE-ASYMMETRY PIN.
        # Mirroring npm_config_cache too would be the obvious symmetric move and
        # is deliberately NOT done: the measured defect is specific to cargo's
        # path-keyed per-unit fingerprint, npm has no equivalent, and widening
        # the merge gate's cache path buys nothing measured. Pinning the ABSENCE
        # (same shape and same rationale as the collapsed-'reviewer' assertion
        # above) forces any future npm mirroring to arrive as a deliberate edit
        # carrying its own evidence, rather than as a symmetry-driven drive-by.
        assert "verify_env does NOT carry npm_config_cache -- its absence is DELIBERATE (cargo's fingerprint embeds the registry source path; npm has no path-keyed equivalent, so mirroring it would widen the merge gate's blast radius for no measured gain)" \
            python3 "$_PARSE_PY" "$ORCH_YAML" top_key_absent verify_env npm_config_cache
    else
        echo "SKIP: sandbox.enabled is not true; none of the (A) cache-redirect, (D) CARGO_HOME-agreement or (E) flip-precondition contracts apply (guard is conditional, see header)"
    fi
fi

# ---------------------------------------------------------------------------
# (B) KNOB-NAME / VALUE CROSS-CHECK -- always runs (bash grep, no python needed)
# ---------------------------------------------------------------------------
echo "--- (B) knob-name / value cross-check (bash grep) ---"

# Windowed grep (LINT_PLAN idiom from test_merge_role_lint_first_seam.sh (C) /
# test_release_mode_in_test_command.sh): isolate the sandbox: block's own
# lines so `enabled: true` / `backend: landlock` are pinned to THAT block,
# not any other `enabled:`-keyed block in the file (e.g. usage_cap.enabled).
# `|| true` guards the command substitution under `set -euo pipefail`: if
# `sandbox:` is ever renamed/moved/deleted, a bare failing grep here would
# abort the whole script before test_summary runs, losing the diagnostic
# for every assertion below (including the unrelated (B) knob-name checks)
# behind a bare non-zero exit. With `|| true`, SANDBOX_BLOCK is empty and
# the two windowed asserts below report a real FAIL against it instead.
SANDBOX_BLOCK="$(grep -A5 '^sandbox:' "$ORCH_YAML" || true)"
export SANDBOX_BLOCK

assert "'enabled: true' cited under the sandbox block" \
    bash -c 'printf "%s\n" "$SANDBOX_BLOCK" | grep -qE "^[[:space:]]*enabled:[[:space:]]*true[[:space:]]*$"'

assert "'backend: landlock' cited under the sandbox block" \
    bash -c 'printf "%s\n" "$SANDBOX_BLOCK" | grep -qE "^[[:space:]]*backend:[[:space:]]*landlock[[:space:]]*$"'

assert "'role_env_overrides:' cited in dark-factory-orchestrator.yaml" \
    grep -qE '^role_env_overrides:[[:space:]]*$' "$ORCH_YAML"

assert "'CARGO_HOME:' cited in dark-factory-orchestrator.yaml" \
    grep -qE '^[[:space:]]*CARGO_HOME:' "$ORCH_YAML"

assert "'npm_config_cache:' cited in dark-factory-orchestrator.yaml" \
    grep -qE '^[[:space:]]*npm_config_cache:' "$ORCH_YAML"

# ---------------------------------------------------------------------------
# (C) ROLE-NAME DECLARATION CROSS-CHECK -- always runs (plain bash, no python,
#     does not read the yaml at all)
# ---------------------------------------------------------------------------
echo "--- (C) role-name declaration cross-check (bash) ---"

# Exported for the `bash -c` checker below (same idiom as SANDBOX_BLOCK in (B)).
export CACHE_REDIRECT_ROLES KNOWN_DISPATCH_ROLES UNCOVERED_DISPATCH_ROLES

# A misspelled member of CACHE_REDIRECT_ROLES mirrored into the yaml too would
# satisfy every assertion in (A) -- present, absolute, under /tmp, distinct,
# declared -- while _build_agent_env's role_env_overrides.get(role.name, {})
# lookup silently misses and that role gets NO redirect. This is the
# generalized form of the collapsed-'reviewer' trap, and the only check here
# that catches it.
assert "every CACHE_REDIRECT_ROLES member is a real DF dispatch role name (KNOWN_DISPATCH_ROLES, mirroring the ROLES registry at roles.py:1512 -- a name not in ROLES can never be matched by _build_agent_env's role.name lookup)" \
    bash -c '
        rc=0
        for r in $CACHE_REDIRECT_ROLES; do
            case " $KNOWN_DISPATCH_ROLES " in
                *" $r "*) ;;
                *) echo "not a DF dispatch role name: $r" >&2; rc=1 ;;
            esac
        done
        exit $rc
    '

# The assertion above is SUBSET-DIRECTION ONLY: it proves every redirected
# role is a real dispatch name, but it is blind to the COMPLEMENT -- which
# roles are left OUT. That residual set was stated only in prose, in two
# files, and both copies went stale the moment `merger` joined the redirect
# (task 6275). Pinning the complement makes the next role addition or removal
# impossible to land without a conscious edit to UNCOVERED_DISPATCH_ROLES,
# which drags the author back to the prose that describes it.
assert "the residual UNCOVERED dispatch-role set (KNOWN_DISPATCH_ROLES minus CACHE_REDIRECT_ROLES) equals the UNCOVERED_DISPATCH_ROLES declaration -- pins the complement the subset check above cannot see" \
    bash -c '
        computed=""
        for r in $KNOWN_DISPATCH_ROLES; do
            case " $CACHE_REDIRECT_ROLES " in
                *" $r "*) ;;
                *) computed="$computed $r" ;;
            esac
        done
        # Order-insensitive on BOTH sides: compare sorted token streams, so
        # merely reordering either declaration is not a spurious FAIL.
        c_sorted=$(printf "%s\n" $computed | sort | tr "\n" " ")
        d_sorted=$(printf "%s\n" $UNCOVERED_DISPATCH_ROLES | sort | tr "\n" " ")
        [ "$c_sorted" = "$d_sorted" ] && exit 0
        echo "residual uncovered dispatch-role set drifted from its declaration" >&2
        echo "  computed (KNOWN_DISPATCH_ROLES minus CACHE_REDIRECT_ROLES): $c_sorted" >&2
        echo "  declared (UNCOVERED_DISPATCH_ROLES):                        $d_sorted" >&2
        for r in $computed; do
            case " $UNCOVERED_DISPATCH_ROLES " in
                *" $r "*) ;;
                *) echo "  uncovered in fact but NOT declared uncovered: $r" >&2 ;;
            esac
        done
        for r in $UNCOVERED_DISPATCH_ROLES; do
            case " $computed " in
                *" $r "*) ;;
                *) echo "  declared uncovered but IS redirected: $r" >&2 ;;
            esac
        done
        exit 1
    '

test_summary
