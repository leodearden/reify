# Workspace feature union: `-p <member>` resolves like `--workspace`

**Task #9761 | 2026-11-05.** Crate: `crates/workspace-feature-union`.
Tool: `scripts/workspace-feature-union.py` (`check` / `generate`). Gate:
`crates/reify-build-utils/tests/workspace_feature_union.rs`.

## Problem

Under resolver 2, cargo unifies a package's features over the packages a
command *selects*. The warm base is built with `--workspace`, but an agent in
a lane runs `cargo test -p <member>`. That selection can resolve an external
package (serde, syn, tokio, nalgebra, ...) to a feature set the base never
compiled, and every such unit is rebuilt cold in every freshly seeded lane.

Measured by the task on 2026-11-03: a seeded lane's first
`cargo test -p reify-eval --no-run` compiled 81 units, 54 of them external,
in 530-593 s. The side-aware `cargo tree` census on main `0febf2a04f` found
SPLIT variants in 35 of the 37 covered members (only reify-gcode and
reify-spec-conformance had none), over 33 external packages. reify-eval's
host syn resolved without `fold,visit`, which is exactly the task's observed
`syn-fc4fd18d8d7f06bf` feature list.

## Mechanism

`workspace-feature-union` has no code. Its manifest pins each external package
to the features `--workspace` resolves, and every member depends on it, so any
`-p` selection pulls those features in and resolves the same variants.

- **Sides.** Resolver 2 keeps one feature set per side. HOST is
  build-dependencies, proc-macros and everything below them; TARGET is the
  rest. The workspace really has both (serde host `{alloc,..}` vs target
  `{rc,..}`). HOST pins go in `[target.<host>.build-dependencies]` and TARGET
  pins in `[target.<host>.dependencies]`. Merging the sides would push
  target-only crates (tokio, url, nalgebra, ...) into the host graph and
  change every existing host variant, re-colding the base. Keying on the host
  triple keeps the pins away from a wasm build. No build.rs is needed: cargo
  applies a build-dependency's features even when the declaring crate has no
  build script.
- **Coverage.** Every member that is not host-side depends on the union as a
  normal dependency. A host-side member is a build-dependency of another
  member or a proc-macro. Today the only one is reify-build-utils; no member
  is a proc-macro. A host-side member must not reach the union, directly or
  through a covered member, because its graph is compiled for the host and
  the union's target pins would become host units there. As a result
  `cargo test -p reify-build-utils` alone stays split.
- **Normal, not dev, pins.** The pins are computed from the dev-active
  resolution (`-e normal,build,dev`) but declared as normal dependencies. So
  `cargo check -p X` and `cargo build -p X`, not just `cargo test -p X`,
  resolve to the base's variants.
- **Base stays valid.** The union re-states what `--workspace` already
  unifies: `--workspace` resolved the same 450 external variants before and
  after the union landed. No external unit in the base is invalidated; only
  workspace crates rehash, once, because each gains a dependency.
- **Engine hash.** reify-eval's normal closure grows, because the union's
  pins are reify-eval's normal deps now:
  `crates/reify-eval/engine_hash_closure.txt` went from 156 to 204 names. The
  union crate has a `WORKSPACE_CRATE_COVERAGE` row Hashed on its `Cargo.toml`,
  since a feature change is invisible to the (name, version) pins.
  Regenerating the union therefore changes `ENGINE_VERSION_HASH`, which is
  the safe direction.

## The invariant and its gate

Every covered member's `cargo tree -p <m>` external (name, version, side,
features) variants all occur in `cargo tree --workspace`. The gate
`real_workspace_resolves_every_member_like_the_workspace` runs
`python3 scripts/workspace-feature-union.py check` (resolve-only,
`--frozen`, about 20 s). It runs on every `crates/*/Cargo.toml` edit, because
it lives in reify-build-utils, the crate `scripts/reify_path_attribution.py`
routes manifest edits to.

When it reds after a dependency or feature edit:

1. `python3 scripts/workspace-feature-union.py generate` rewrites the marked
   region of `crates/workspace-feature-union/Cargo.toml` to a fixpoint (it
   resolves `--offline`, so it may update `Cargo.lock`).
2. Regenerate the data lines of `crates/reify-eval/engine_hash_closure.txt`
   with the command in its header.

If generate fails part-way, for example because `cargo tree` errors, it
restores the union manifest and `Cargo.lock` as it found them.

An UNCOVERED line names the manifest that needs
`workspace-feature-union.workspace = true`. HOST-UNION is read from
`cargo metadata` workspace edges. It fires on a normal or build edge from a
host-side member to the union or to a covered member, and on any member's
build edge to the union. It is not read from `cargo tree`:
`cargo tree --workspace` prints each member reached on the host side as a
second depth-0 root, textually identical to its target root, and dedupes
`(*)` across roots, so a check derived from the tree depended on the sort
order of member names. Task #9834 reproduced this with real cargo 1.96. A
HOST-UNION line also names the dependency edge to remove and the manifest
that holds it. `--help` explains every finding.

## Probe procedure

Run the probe as a lane's first build, under the verify env
(`RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0`, the agent's `CARGO_HOME`, empty
`RUSTFLAGS`):

```bash
{ uptime; git rev-parse HEAD; } > /tmp/<id>.ctx
/usr/bin/time -v cargo test -p reify-eval --no-run > /tmp/<id>.log 2>&1; echo rc=$?
grep -E '^ +Compiling ' /tmp/<id>.log | grep -vc ' (/'   # external units
grep -E '^ +Compiling ' /tmp/<id>.log | grep -c ' (/'    # workspace units
```

For `cargo clippy`, count `Checking` lines as well as `Compiling` lines, and
pass the base's arguments, `-- -D warnings` (the clippy leaf in
`scripts/verify.sh`). Without them `CLIPPY_ARGS` differs from the base's, which
dirties workspace units: 30 were rebuilt instead of 21 or 22 (see Tree-sitter
share).

Probing at the base commit gives a zero-delta first build, one with no
workspace source newer than the seed's base. Do it before the snapshot and the
arm B copy below. The gen lookup compares `target/`'s own mtime, which the
snapshot's `mkdir` changes, and the arm B copy is generated from the checked-out
`grammar.js`, so a copy made at HEAD describes HEAD's grammar, not the base's.

```bash
BASE=/home/leo/src/warm-lanes/base
G=$(for g in "$BASE"/target.gen.[0-9]*; do [ -d "$g" ] && [ "$(stat -c %y "$g")" = "$(stat -c %y target)" ] && echo "$g"; done)
echo "$G"   # one gen; empty means target/ is no longer a pristine seed
B=$(cat "$G.basecommit")
find target ! -type d -newer "$G.basecommit" | wc -l   # must print 0: nothing was built since the seed
git diff --name-only --no-renames "$B" HEAD > /tmp/<id>-D.lst   # both names of a rename
git checkout --detach "$B"
# The checkout rewrote the paths in D. Give each one that still exists the mtime the seed gives it at $B:
while IFS= read -r f; do [ -e "$f" ] && touch -h -d 2020-01-01T00:00:00 "$f"; done < /tmp/<id>-D.lst
# Then touch to now the paths of D that the seed's env!() relink touches. Its rule is `_ENV_PATH_MACRO_RE` and
# the `find` that feeds it, in scripts/seed-warm-lane.sh; the seed applies it to absolute paths, so test "$PWD/$f".
git diff --name-only "$B"   # must print nothing
find . \( -path ./target -o -path ./.git -o -path ./.task \) -prune -o -type f -newermt 2020-01-02 -print
# must list only the sources that relink touches, plus incidentals such as .mcp.json
```

After the last probe, return with `git checkout <task branch>`, which rewrites
the delta with fresh mtimes, the state the seed gives a branch's HEAD. An
orchestrator restart runs `git rebase main` in the lane and moves a detached
HEAD to main's tip; if one lands mid-run, repeat the checkout and the re-stamp,
and restore `target/`.

To take several first builds in one lane, snapshot the seeded `target/` before
the first probe and restore it before each later one. Without the restore,
each later probe reuses the workspace units the earlier ones built, so only the
first is a first build. Keep the snapshot inside `target/`: `/target` is
gitignored, and `--reflink=always` needs the lane's own filesystem (`/tmp` may
be another). `cp -a` keeps the nanosecond mtimes that cargo's fingerprints
compare, and the listing check proves each copy:

```bash
S=target/.pristine; mkdir "$S"
for e in target/* target/.[!.]*; do [ "$e" = "$S" ] || cp -a --reflink=always "$e" "$S/"; done
(cd "$S" && find . ! -type d -printf '%P\t%T@\t%s\n' | LC_ALL=C sort) > /tmp/<id>-seed.lst
check() { (cd target && find . -path ./.pristine -prune -o ! -type d -printf '%P\t%T@\t%s\n' | LC_ALL=C sort) | cmp - /tmp/<id>-seed.lst; }
check; echo "snapshot-matches rc=$?"   # must be 0 before the first probe

# before every later probe:
find target -mindepth 1 -maxdepth 1 ! -name .pristine -exec rm -rf {} +
cp -a --reflink=always "$S"/. target/
check; echo "restored-matches rc=$?"   # must be 0 before probing
git clean -Xfd -- tree-sitter-reify/src
```

The last line deletes the gitignored generated sources under
`tree-sitter-reify/src`, which a fresh seed lacks and the first probe recreates
(see the tree-sitter paragraph under Measurements). It names no files. `-X`
removes only ignored files, so the set is whatever the root `.gitignore`'s
"Tree-sitter generated" block lists, and the acquire's
`git clean -xfd -e target` deletes those same files. A new output of
`tree-sitter generate` is covered as soon as that block lists it. The files
live outside `target/`, so restoring `target/` does not restore their absence,
and the listing check does not see them either. Remove `target/.pristine` after
the last probe.

To exclude the tree-sitter share, probe in arm B of the Tree-sitter share
subsection: the generated sources present and back-dated to 2020-01-01, so the
build script stays Fresh. Make the copy once, before the first probe. It holds
the same files the reset line removes, listed from `.gitignore`:

```bash
bash scripts/tree-sitter-generate.sh   # about 5 s: tree-sitter generate only, no cargo
mkdir /tmp/<id>-tsgen
(cd tree-sitter-reify/src && git ls-files -z -oi --exclude-standard --directory . | xargs -0 cp -a -t /tmp/<id>-tsgen/)
find /tmp/<id>-tsgen -mindepth 1 -exec touch -h -d 2020-01-01T00:00:00 {} +
bash scripts/tree-sitter-freshness.sh --print-fingerprint > /tmp/<id>-fp.txt
for s in target/debug/build/tree-sitter-reify-*/out/tree_sitter_inputs.stamp; do cmp -s "$s" /tmp/<id>-fp.txt && echo "match $s" || echo "DIFF $s"; done
```

Every stamp must match: that proves the copy holds the bytes the seeded
archive was compiled from. A DIFF means the generated or compiled inputs differ
from the base's: `grammar.js`, `scanner.c`, or the tree-sitter CLI that wrote
`parser.c` and its headers, which `scripts/tree-sitter-generate.sh` does not
pin. Arm B would then link bytes its sources do not describe. Probing at the
base commit cures the first two: remove `/tmp/<id>-tsgen` and repeat this block
after the checkout. A DIFF that survives that points to the CLI, and then the
copy cannot be proven identical to the base's inputs, so arm B is not valid in
this lane. Before every probe, the first included, put the copy in place of the
`git clean` line above: `cp -a /tmp/<id>-tsgen/. tree-sitter-reify/src/`.

## Measurements

The `before` and `after` rows come from lane `_lane-50`, acquired at 07:05.
Its `target/` carries base `target.gen.618`'s mtime to the nanosecond
(basecommit `735214893919`). gen.619 was published at 07:30, after the
acquire. The `post-landing` rows come from a second lane, described below.

| Probe | HEAD | Base | Units (external + workspace) | Wall | Load avg (start → end) |
|---|---|---|---|---|---|
| before: `test -p reify-eval --no-run` | `0febf2a04f` (main) | gen.618 | 54 + 27 | 10:08.89 | 70/81/117 → 128/170/154 |
| after: `test -p reify-eval --no-run` | `e6770239e8` (wired) | gen.618 | **0** + 30 | 5:33.83 | 77/139/156 → 135/122/139 |
| after: `test -p reify-expr --no-run` | `e6770239e8` | gen.618 | **0** + 10 | 1:44.28 | 265/143/136 → 341/225/167 |
| after: `test -p reify-compiler --no-run` | `e6770239e8` | gen.618 | **0** + 9 | 3:09.23 | 341/225/167 → 90/159/152 |
| after: `clippy -p reify-eval --all-targets` | `e6770239e8` | gen.618 | **0** + 30 | 2:37.12 | 90/159/152 → 82/125/140 |
| post-landing: `test -p reify-eval --no-run` | `363d090775` | gen.623 | **0** + 23 | 3:34.02 | 59/103/146 → 90/96/134 |
| post-landing: `test -p reify-expr --no-run` | `363d090775` | gen.623 | **0** + 12 | 2:18.99 | 312/171/139 → 400/275/183 |
| post-landing: `test -p reify-compiler --no-run` | `363d090775` | gen.623 | **0** + 17 | 2:54.25 | 92/182/170 → 113/154/162 |
| post-landing: `clippy -p reify-eval --all-targets` | `363d090775` | gen.623 | **0** + 30 | 2:18.34 | 108/149/158 → 56/114/144 |

The task's own clippy measurement before the union was 40 external + 29
workspace units. Among the `before` and `after` rows, only the reify-eval rows
are first builds. The later `after` rows reuse the workspace units the
reify-eval after-probe built. The zero externals cannot come from the
before-probe either: the units it built are the split variants, which `-p` no
longer resolves.

These after-walls are pre-landing and over-count workspace units: every
member rehashes against the gen.618 base until the merge-verify lane refreshes
the base with this change. The `post-landing` rows above are the re-probe in a
lane seeded from the refreshed base (#9824).

The `post-landing` rows come from lane `_lane-1`, checked out at 07:19 and
seeded at 07:23. Its `target/` carried base `target.gen.623`'s mtime to the
nanosecond (basecommit `8326547b31`, which contains the union merge
`33578e66f4`), and nothing had been built since. gen.624 was published at
07:53, after the seed. Unlike the later `after` rows, each `post-landing` row
is a first build: the probes followed the several-probes recipe in Probe
procedure, and `target/` matched the seed's (path, mtime, size) listing of
90,105 non-directory entries when each probe started. HEAD differs from the
basecommit in `crates/reify-test-support/src/values.rs` and in one
reify-stdlib integration test, which no probe builds.

All four probes built zero external units. The reify-eval first build that
cost 54 external + 27 workspace units before the union now costs 0 + 23, in
3:34.02 against 10:08.89 (the host load differs, so walls are indicative
only). Clippy, 40 + 29 before the union by the task's own measurement, is
0 + 30. Against the pre-landing `after` rows, which the caveat above says
over-count workspace units, reify-eval fell from 30 to 23 and clippy stayed
at 30. The clippy workspace counts depend on the command's arguments: see the
`CLIPPY_ARGS` paragraph under Tree-sitter share.

What remains is workspace units, which the union does not cover. Observed:
tree-sitter-reify, reify-test-support and seven more units (reify-ast,
reify-compiler, reify-config, reify-expr, reify-ir, reify-stdlib,
reify-syntax) are rebuilt in all four probes; reify-core is rebuilt under
`-p reify-expr` and in the clippy probe, not under `-p reify-eval` or
`-p reify-compiler`.

One contributor is visible in the sources. tree-sitter-reify's generated
sources are gitignored (root `.gitignore`, "Tree-sitter generated"), so the
acquire's `git clean -xfd -e target` deletes them: a fresh seed has no
`parser.c`, `grammar.json` or `node-types.json`, none of the three headers in
`src/tree_sitter/`, and neither stamp. `build.rs` emits `rerun-if-changed` for
`src/parser.c` and those headers, and runs `tree-sitter generate` when an
output is missing, so a fresh lane's first build reruns the script and
rebuilds tree-sitter-reify. By manifest edges that rebuild reaches
reify-syntax (tree-sitter-reify's only dependent) and, among the nine,
reify-syntax's normal dependents reify-compiler and reify-test-support: with
tree-sitter-reify itself, four of the nine. In these probes it does not reach
reify-ast, reify-config, reify-ir or reify-stdlib, and it reaches reify-expr
only under `-p reify-expr`, through that package's dev-dependencies. That is the
reach by manifest edges. The next subsection measures it: a first build pays
only for tree-sitter-reify itself, and the reach shows in the next invocation.

Hypothesis, not isolated by these probes: once tree-sitter regeneration is
excluded (next subsection), the workspace units that remain are the union of
what two contributors rebuild, each with the downstream rebuild it triggers: the
seed's `env!()`/build-dir invalidation, and workspace-member feature splits
(#9825, see Limits). A unit either one dirties is built once, so the counts do
not add, and the tree-sitter share measured in the next subsection holds only
with the other contributors present. A measurement of #9825's contributor probes
without the tree-sitter regeneration (Probe procedure, arm B).

### Tree-sitter share (#9862)

Lane `_lane-52`, 2026-11-06 22:52 to 2026-11-07 00:34 UTC. Its `target/` carried
base `target.gen.625`'s mtime to the nanosecond (`09:31:02.634473980`, built
under `_merge-verify`, basecommit `35a914623f`), and no file under it was newer
than the `.basecommit` stamp. The lane's HEAD, `3cee7ab20d`, differs from the
basecommit in 41 files, among them `crates/reify-core`, so a probe at HEAD would
rebuild every reify-core dependent in both arms and hide the cascade. The probes
therefore ran at the basecommit, as in Probing at the base commit: of the 41
files the checkout rewrote, 32 were re-stamped to 2020-01-01 and 4 `env!()` test
sources were left at now, and 5 files that exist only at HEAD were gone. The
lane then matched the seed's model exactly. The last row below, arm B's
`-- -D warnings` clippy, was taken after an orchestrator restart (00:18:44 UTC)
moved the detached HEAD to main's tip. The zero-delta state was re-established:
the basecommit was checked out again and the 53 files that differ from main's
tip re-stamped (37 to 2020-01-01, 5 `env!()` test sources left at now, 11 files
gone), `target/` was restored, and the lane was verified against the seed's
model again.

Arm A is a fresh lane as it is today: the eight generated sources absent. Arm B
has them present and back-dated to 2020-01-01, as in the arm B recipe under
Probe procedure: generated at the basecommit by
`scripts/tree-sitter-generate.sh` (4.76 s), then
`scripts/tree-sitter-freshness.sh --print-fingerprint` compared with the
`out/tree_sitter_inputs.stamp` of the four seeded build-script run dirs that
hold one. All four stamps are identical to each other and to the fingerprint, so
arm B links the bytes the base's archive was compiled from. A minus B, the units
arm A builds that arm B does not, is the tree-sitter share, measured with the
other contributors present; B is what they rebuild. Every probe started from the
same seeded `target/`, restored from a reflink snapshot (90,107 non-directory
entries, matched before every run); the arm that ran first alternated between
probes, and each restore took 1:06 to 10:25.

| Probe | HEAD | Base | Units (external + workspace) | Wall | Load avg (start → end) |
|---|---|---|---|---|---|
| A: `test -p reify-eval --no-run` | `35a914623f` | gen.625 | **0** + 23 | 3:57.23 | 197/231/194 → 180/184/181 |
| A, 2nd run: same command, nothing restored | `35a914623f` | gen.625 | **0** + 8 | 1:30.44 | 111/165/175 → 92/146/167 |
| B: `test -p reify-eval --no-run` | `35a914623f` | gen.625 | **0** + 22 | 3:33.02 | 74/130/157 → 169/144/156 |
| B, 2nd run: same command, nothing restored | `35a914623f` | gen.625 | **0** + 0 | 0:04.75 | 121/134/152 → 119/134/152 |
| A: `test -p reify-expr --no-run` | `35a914623f` | gen.625 | **0** + 12 | 1:21.31 | 468/293/202 → 230/262/199 |
| B: `test -p reify-expr --no-run` | `35a914623f` | gen.625 | **0** + 11 | 1:59.63 | 118/129/141 → 176/143/145 |
| A: `test -p reify-compiler --no-run` | `35a914623f` | gen.625 | **0** + 17 | 2:00.38 | 82/188/180 → 140/169/173 |
| B: `test -p reify-compiler --no-run` | `35a914623f` | gen.625 | **0** + 16 | 1:18.75 | 100/174/181 → 77/149/172 |
| A: `clippy -p reify-eval --all-targets` | `35a914623f` | gen.625 | **0** + 30 | 1:04.43 | 159/213/175 → 104/188/168 |
| B: `clippy -p reify-eval --all-targets` | `35a914623f` | gen.625 | **0** + 30 | 6:19.62 | 105/116/143 → 95/106/130 |
| A: `clippy -p reify-eval --all-targets -- -D warnings` | `35a914623f` | gen.625 | **0** + 22 | 0:59.32 | 54/110/142 → 59/102/137 |
| B: `clippy -p reify-eval --all-targets -- -D warnings` | `35a914623f` | gen.625 | **0** + 21 | 1:43.89 | 317/219/165 → 157/191/160 |

All twelve runs exited 0 and built zero external units. In the three `test`
probes and in the clippy probe whose arguments match the base's, arm A builds
exactly one workspace unit more than arm B, `tree-sitter-reify`: 23 against 22,
12 against 11, 17 against 16, 22 against 21. That unit is the whole first-build
share. Its cost is the build-script run: in the `-- -D warnings` pair,
`--timings` gives arm A 4.34 s for the run (`tree-sitter generate`, then the
`parser.c` compile) and 0.09 s for the check unit, against 0.0 s for both in arm
B, and reify-syntax's check started 0.01 s after the unit finished. At zero
delta arm A reproduces the unit counts of all four `post-landing` rows (23, 12,
17 and 30), and reify-test-support rebuilds in arm B, so the `values.rs`
difference between the `post-landing` probes' HEAD and their basecommit is not
needed to explain it.

The second invocation shows the cascade. After arm A's first `-p reify-eval`
build, the same command with nothing changed compiled 8 workspace units again,
in 1:30.44: tree-sitter-reify and its dependents reify-syntax, reify-compiler,
reify-test-support, reify-doc-build, reify-eval, reify-mesh-morph and
reify-runtime. That is the "one bounded extra run" that `build.rs` names as the
cost of watching its outputs. Arm B's second invocation compiled nothing, in
4.75 s. The seven dependents also rebuild in arm B's first build, which is why a
first build is charged for tree-sitter-reify alone today; once the other
contributors are gone, it would pay for all eight.

Arm B still builds 22, 11, 16 and 21 workspace units. Seven are common to all
four arm-B probes (reify-ast, reify-compiler, reify-expr, reify-ir,
reify-stdlib, reify-syntax and reify-test-support), and 25 distinct units appear
across them. These are what the seed's `env!()`/build-dir invalidation and
#9825's workspace-member feature splits must explain; this measurement does not
attribute them between the two.

The walls compare nothing across arms. The plain clippy pair builds the same 30
units in both arms, in 1:04.43 and 6:19.62, and the load average at the start of
the two runs of a pair differs by up to 6x (317 against 54). sccache is on, as
in the verify env, so a later run of a pair can also hit units an earlier run
cached; its hit counts were not recorded. The unit counts and the `--timings`
durations are the result.

The plain `clippy -p reify-eval --all-targets` rows cannot separate the arms:
both build the same 30 units, tree-sitter-reify included. The base's clippy runs
with `-- -D warnings` (`scripts/verify.sh`), which sets `CLIPPY_ARGS`, and the
plain command does not. A re-run of arm B with
`CARGO_LOG=cargo::core::compiler::fingerprint=info` (4:19.00, 30 units again)
gives the dirty reason for the tree-sitter-reify build script's compile unit:
`ChangedEnv { var: "CLIPPY_ARGS", previous:
Some("-D__CLIPPY_HACKERY__warnings__CLIPPY_HACKERY__"), current: Some("") }`.
Its run unit and the lib check are dirty only through their dependency. In the
arm B run itself the generated sources were not regenerated: `parser.c`,
`grammar.json`, `node-types.json` and both stamps still carried 2020-01-01
afterwards. The `post-landing` clippy row above used the plain command too, and
at a two-file delta from its basecommit it is comparable with these arms: its 30
workspace units are 8 to 9 more than the 22 and 21 that the base's arguments
leave in arms A and B. The `after` clippy row cannot be read that way: it ran
against gen.618 before the union reached the base, when every member rehashes,
so its 30 units are not separable into a `CLIPPY_ARGS` part and a rehash part.

### Cold build

Every row above is a warm-lane build. The cold cost is
`cargo build -p reify-core --locked --offline --timings`, run on 2026-11-06
with sccache off and an empty target directory per tree: on main just before
the union (`7f0bdbdf3e`), then on `363d090775` plus this note, back to back.

| Tree | Units (external + workspace) | Wall | reify-core start offset | Load avg (start → end) |
|---|---|---|---|---|
| `7f0bdbdf3e` (before the union) | 9 + 1 | 1:03.63 | 61.29 s | 153/304/239 → 158/277/234 |
| `363d090775` (with the union) | 99 + 2 | 2:55.88 | 174.08 s | 158/277/234 → 107/209/216 |

The union started at 174.05 s, when nalgebra's metadata became ready, behind
serde_derive (a proc-macro, usable only once fully built), serde, num-complex
and simba. The host was loaded, so walls and offsets are indicative only:
serde_derive started 63 s after its last dependency finished.

## Limits

- **Workspace-member features.** Members' own features (reify-ast `serde`,
  reify-compiler/reify-stdlib `test-support`, kernel `test-fixtures`, ...)
  still differ between `-p` and `--workspace`, so a lane still rebuilds
  workspace units. That is #9825, whose measurement probes without the
  tree-sitter regeneration (Probe procedure, arm B), because the causes overlap
  and a unit count cannot be reduced by the tree-sitter share.
- **Tree-sitter regeneration.** The generated sources are gitignored, so
  the acquire deletes them, and `build.rs` watches them, so a fresh lane's first
  build reruns the build script. Measured on `-p reify-eval` (Tree-sitter
  share), with the other contributors present: the first build gains one unit,
  tree-sitter-reify, a 4.34 s build-script run in the clippy pair's `--timings`;
  the next invocation of the same test command compiles 8 workspace units again,
  in 1:30.44 against 4.75 s with the sources present, so the second invocation
  is most of the cost today.
  Decision: accepted until a ruling. The fix that removes both costs is to
  restore the content-attested generated sources at acquire, from a snapshot
  carried in the base gen (#9896). It needs a ruling first, because the seed is
  documented as source-tree-inert (CLAUDE.md, Warm lanes;
  `tests/infra/test_seed_warm_lane.sh`, Block X). Not taken: unwatching the
  outputs in `build.rs` reverses #5629 and #9741, generating into `OUT_DIR`
  redesigns verify's tree-sitter leaves and the `src/` consumers, and
  back-dating the outputs in `build.rs` leans on cargo's internal reference
  time.
- **Feature selections.** A non-default selection such as
  `-p reify-gui --features gui` is a resolution of its own, and the gate does
  not cover it.
- **What the gate proves.** The gate checks `cargo tree` resolution, not
  compiled units. Resolution reproduced the task's observed syn fingerprint
  exactly, and the probes above are the compile-level evidence.
- **Cold and release builds.** The Measurements rows are warm-lane builds,
  where the externals are already compiled. A cold target must compile the
  union's whole dependency set: tokio, the url/icu stack, nalgebra, regex,
  futures and serde_json. Every covered member depends on the union, including
  the DAG root reify-core and, by their manifests, the otherwise
  dependency-free reify-gcode and reify-spec-conformance, so none of them can
  start before the union's metadata exists. The Cold build subsection of
  Measurements gives the cost for reify-core.

  Dev-only features come along. The union pins tokio with `test-util`, so cold
  and release builds compile it in too. This is deliberate; the reason is the
  **Normal, not dev, pins** bullet in Mechanism.
- **A lower bound only.** `--workspace` includes the union's own pins, so the
  gate cannot see over-pinning. Two pins pass it:
  - a pinned feature no member needs any more;
  - a pinned package no member reaches any more, for example after a
    dependency is removed.

  Either one is still compiled into every member, release builds included.
  Only `generate` sheds them, because it starts from an empty region. After
  removing a dependency or a feature, run `generate` and review the diff of
  the union manifest.
- **HOST-UNION over-approximates.** Optional and platform-specific path
  dependencies count even when inactive, so the rule fails loud, never silent.
- **Host roots in the SPLIT comparison.** `parse_tree` still attributes every
  non-proc-macro depth-0 `--workspace` root to TARGET, so a host root's
  external variants are filed under TARGET. That usually shows up as a
  spurious `SPLIT ... no host variant`, and it could rarely mask a target
  split. Owned by #9873.
- **A host-side member's dev-dependencies in the SPLIT comparison.**
  `cargo tree` lists a member's `[dev-dependencies]` wherever the member
  appears, including below another member's build edge to it, and
  `parse_tree` files everything under a host node as HOST. Measured on the real
  `--workspace` tree on 2026-11-07: reify-build-utils' dev-dependencies
  (proc-macro2, syn, tempfile) are listed below its `[build-dependencies]`
  edges. That puts HOST variants of tempfile 3.27.0, rustix 1.1.4, getrandom
  0.4.2, linux-raw-sys 0.12.1 and once_cell 1.21.4 into the `--workspace` set,
  though no build compiles them for the host: none of them is a HOST variant
  of `cargo tree --workspace -e normal,build`. Such a variant can mask a real
  host split of the same package, or let `generate` pin a host unit nobody
  builds. None is pinned today.

  Skipping a dev section below a non-root node does not fix it, because the
  `(*)` dedupe can make that section the only expanded copy of a subtree. On
  the real tree, reify-build-utils and its toml 0.8.2 are expanded only below
  reify-audit -> reify-syntax's `[dev-dependencies]`. A probe that skipped such
  sections lost real variants, the HOST toml 0.8.2 among them. A fix needs a
  resolution without the dedupe, or a walk over the deduped graph. Owned by
  #9873.
