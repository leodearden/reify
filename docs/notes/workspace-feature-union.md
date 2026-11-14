# Workspace feature union: `-p <member>` resolves like `--workspace`

**Task #9761 | 2026-11-05.** Crate: `crates/workspace-feature-union`.
Tool: `scripts/workspace-feature-union.py` (`check` / `generate`). Gate:
`crates/reify-build-utils/tests/workspace_feature_union/`.

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

For every covered member, every external (name, version, side, features)
variant that `-p <m>` compiles, as read from `cargo tree -p <m>`, is one that
`--workspace` compiles. The gate
`real_workspace_resolves_every_member_like_the_workspace` runs
`python3 scripts/workspace-feature-union.py check` (resolve-only,
`--frozen`; about 20 s, though 74–81 s on a heavily loaded host on 2026-11-07,
of which parsing was 0.02 s). It runs on every
`crates/*/Cargo.toml` edit, because it lives in reify-build-utils, the crate
`scripts/reify_path_attribution.py` routes manifest edits to. Sides come from a
walk over cargo's deduped tree, and a member printed twice as a root is placed
by cargo's printing rules (Limits, "Side attribution"). `check` also compares
every `-p <m>` unit's profile with `--workspace`'s from build-free unit graphs,
which adds 45-62 s under load (Host profiles). While a HOST-UNION finding
stands, check reports it and compares nothing.

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
`RUSTFLAGS`). The first step, before anything else in the lane and whether or
not you probe at the base commit, is to find the base gen the lane was seeded
from and record it:

```bash
BASE=/home/leo/src/warm-lanes/base
G=$(for g in "$BASE"/target.gen.[0-9]*; do [ -d "$g" ] && [ "$(stat -c %y "$g")" = "$(stat -c %y target)" ] && echo "$g"; done)
echo "$G"   # one gen; empty means target/ is no longer a pristine seed
B=$(cat "$G.basecommit")
find target ! -type d -newer "$G.basecommit" | wc -l   # must print 0: nothing was built since the seed
[ -n "$B" ] && printf 'L=%q\nG=%q\nB=%q\n' "$PWD" "$G" "$B" > /tmp/<id>-base.env
```

The lookup works once: it compares `target/`'s own mtime with each gen's, and
the snapshot below changes that mtime. Its `mkdir` does, and so does each
restore's `cp -a`, which gives `target/` the snapshot's mtime. Shell variables
do not survive between Bash calls either, so every later block that needs `$B`
starts with `. /tmp/<id>-base.env`. The `[ -n "$B" ]` guard stops a mistaken
re-run from overwriting a good file. `L` is the lane's path: `/tmp` is shared by
every lane, so the base-commit block stops on a file another lane wrote. Give
each lane its own `<id>`. Then probe:

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
workspace source newer than the seed's base. Do it before the arm B copy below.
The arm B copy is generated from the checked-out `grammar.js`, so a copy made at
HEAD describes HEAD's grammar, not the base's. The base-commit block reads `$B`
from `/tmp/<id>-base.env`, so it also works after the snapshot.

```bash
. /tmp/<id>-base.env
[ "$L" = "$PWD" ] || { echo "base.env records lane $L, not $PWD: stop" >&2; exit 1; }
git diff --name-only --no-renames "$B" HEAD > /tmp/<id>-D.lst   # both names of a rename
git checkout --detach "$B"
# The checkout rewrote the paths in D. Give each one that still exists the mtime the seed gives it at $B:
while IFS= read -r f; do [ -e "$f" ] && touch -h -d 2020-01-01T00:00:00 "$f"; done < /tmp/<id>-D.lst
# Then touch to now the paths of D that the seed's env!() relink touches. The regex is read from the seed; the
# case pattern restates the seed's `find`, which matches absolute paths, so it tests "$PWD/$f".
RE=$(sed -n "s/^ *_ENV_PATH_MACRO_RE='\(.*\)'$/\1/p" scripts/seed-warm-lane.sh)
echo "${RE:-relink rule not found in scripts/seed-warm-lane.sh: stop}"
relink_sources() {
  [ -n "$RE" ] || { echo 'relink rule not found in scripts/seed-warm-lane.sh' >&2; return 1; }
  while IFS= read -r f; do
    case "$PWD/$f" in
      */tests/*.rs|*/benches/*.rs) [ -f "$f" ] && [ ! -L "$f" ] && grep -qE "$RE" "$f" && printf '%s\n' "$f" ;;
    esac
  done
}
relink_sources < /tmp/<id>-D.lst | xargs -r -d '\n' touch --
git diff --name-only "$B"   # must print nothing
# Check the whole tree both ways: every relink source at $B against every file newer than 2020-01-02.
git ls-files | relink_sources | LC_ALL=C sort > /tmp/<id>-relink.lst
find . \( -path ./target -o -path ./.git -o -path ./.task \) -prune -o -type f -newermt 2020-01-02 -printf '%P\n' |
  LC_ALL=C sort | LC_ALL=C comm -3 - /tmp/<id>-relink.lst
# must print only incidentals such as .mcp.json, unindented; an indented line is a relink source still at 2020
```

After the last probe, return with `git checkout <task branch>`, which rewrites
the delta with fresh mtimes, the state the seed gives a branch's HEAD. An
orchestrator restart runs `git rebase main` in the lane and moves a detached
HEAD to main's tip; if one lands mid-run, run the base-commit block again and
restore `target/`. The block recomputes the delta against the new HEAD (main's
tip) and re-applies the re-stamp and the relink.

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
git status --porcelain --untracked-files=normal --ignored -- tree-sitter-reify/src   # must print nothing: only unmodified tracked files remain
```

The `git clean` line deletes the gitignored generated sources under
`tree-sitter-reify/src`, which a fresh seed lacks and the first probe recreates
(see the tree-sitter paragraph under Measurements). It names no files. `-X`
removes only ignored files, so the set is whatever the ignore rules match
there, chiefly the root `.gitignore`'s "Tree-sitter generated" block, which
owns the generated outputs. The acquire's `git clean -xfd -e target` deletes
those same files. A new output of `tree-sitter generate` is covered as soon as
that block lists it.

The generated sources live outside `target/`, so restoring `target/` does not
restore their absence, and the listing check does not see them. The
`git status` line does. It prints nothing only when the directory holds nothing
but unmodified tracked files, which is what a fresh seed has: `??` marks an
untracked file that no ignore rule matches, `!!` an ignored file that is still
there, and `M` a modified tracked file. `-X` leaves a `??` file alone, but the
acquire's `-x` deletes it. A new `tree-sitter generate` output that the block
does not list yet is one. `--untracked-files=normal` stops a
`status.showUntrackedFiles=no` in user config from hiding the `??` lines.
Remove `target/.pristine` after the last probe.

To exclude the tree-sitter share, probe in arm B of the Tree-sitter share
subsection: the generated sources present and back-dated to 2020-01-01, so the
build script stays Fresh. Make the copy once, before the first probe. It holds
the same files the reset line removes, chosen by the same ignore rules:

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
base commit cures the first two. Remove `/tmp/<id>-tsgen`, run the base-commit
block, then repeat this block. A DIFF that survives that points to the CLI, and
then the copy cannot be proven identical to the base's inputs, so arm B is not
valid in this lane. Before every probe, the first included, put the copy in
place of the `git clean` and `git status` lines above, since `--ignored` would
list the copy: `cp -a /tmp/<id>-tsgen/. tree-sitter-reify/src/`.

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
seed's `env!()`/build-dir invalidation, and workspace-member feature splits.
A unit either one dirties is built once, so the counts do not add, and the
tree-sitter share measured in the next subsection holds only with the other
contributors present. Member features (#9825) measures the split share with
the generated sources both absent and present, and finds a further
contributor: build scripts that `-p` compiles as new unit variants, whose
cause Host profiles measures (fixed by #9884).

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
attribute them between the two. Member features (#9825) attributes the
`-p reify-eval` 22: 9 to the splits its probe C closes, and 13 to
reify-git-env's split and to the build-script variants of Host profiles (fixed
by #9884), with the dependents they reach by normal edges.

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

## Member features (#9825)

The union pins external packages only. A workspace member's own features can
also resolve wider under `--workspace` than under `-p`, from two sources:

- every member's dev-dependencies, which `--workspace` activates for all
  members at once, the self-dev-dep pattern included;
- normal dependencies of members outside the selection's closure: reify-lsp
  enabled reify-ast's and reify-ir's `serde` before #9825 deleted both features.

A `-p` build then compiles that member as a variant the base never built, and
rebuilds every member that reaches it. The union cannot pin a member, because
the union depending on a member would be a dependency cycle. Instead, `check`
prints one advisory line per such variant, naming every selection that
resolves it:

    MEMBER-SPLIT reify-stdlib v0.1.0 (target): -p resolves {} under reify-cli, ...; --workspace resolves {test-support}

The lines never fail the check, and `generate` ignores them.

### Census

Resolve-only, on `8c9f190b81`; no manifest changed between it and
`42f524d798`. Each selection was resolved with
`cargo tree --frozen -e normal,build,dev -f '{p}|{f}' --target x86_64-unknown-linux-gnu --charset ascii`
and parsed with the script's side-aware parser, member packages kept. It
reproduces the nine `-p reify-eval` rows of #9825's task text exactly:

| Member | `-p reify-eval` | `--workspace` |
|---|---|---|
| reify-ast | none | serde |
| reify-ir | none | serde |
| reify-compiler | none | test-support |
| reify-stdlib | none | test-support |
| reify-git-env | none | test-utils |
| reify-kernel-gmsh | mesh-morph | mesh-morph, stub_register |
| reify-kernel-occt | none | mesh-morph, test-fixtures |
| reify-kernel-openvdb | test-fixtures | stub_register, test-fixtures |
| reify-solver-elastic | none | linear-solve-fault-injection |

Over the 37 covered `-p` selections:

- 31 selections split, with 211 (selection, member) split pairs over 16
  distinct members;
- 341 members are affected, summed over the selections: split, or reaching a
  split member by normal or build lib edges;
- under `-p reify-eval`, 21 of the 30 members in its closure are affected.

`python3 scripts/workspace-feature-union.py check` reproduces the pairs from
its MEMBER-SPLIT lines: before the serde change below, 19 lines naming 211
pairs, 31 selections and 16 members.

### Probes

Each probe is a first build of `cargo test -p reify-eval --no-run` in lane
`_lane-24`, from a `target/` seeded from base `target.gen.625` (basecommit
`35a914623f`), under the verify env (`RUSTC_WRAPPER=sccache`,
`CARGO_INCREMENTAL=0`, `CARGO_HOME=/tmp/reify-agent-cargo-home`, `RUSTFLAGS`
unset), following Probe procedure. A and B ran at `8c9f190b81` with the two
crate-source files that differ from the basecommit set back to its content at
the seed's 2020-01-01 mtime. B and C close every split that `--features` can
name:

    --features reify-ast/serde,reify-ir/serde,reify-compiler/test-support,reify-stdlib/test-support,reify-kernel-gmsh/stub_register,reify-kernel-occt/mesh-morph,reify-kernel-occt/test-fixtures,reify-kernel-openvdb/stub_register,reify-solver-elastic/linear-solve-fault-injection

`reify-git-env/test-utils` cannot be named, because reify-git-env is not a
direct dependency of reify-eval, so it stays split in B and C.

| Probe | HEAD | Base | Units (external + workspace) | Wall | Load avg (start → end) |
|---|---|---|---|---|---|
| A: generated sources absent, no `--features` (2026-11-06) | `8c9f190b81` | gen.625 | **0** + 23 | 5:47.30 | 78/112/138 → 154/133/137 |
| B: generated sources absent, `--features` (2026-11-06) | `8c9f190b81` | gen.625 | **0** + 15 | 5:35.96 | 92/118/130 → 290/178/147 |
| C: generated sources present, `--features` (2026-11-07) | `35a914623f` | gen.625 | **0** + 13 | 5:47.89 | 491.99/294.48/211.81 → 297.40/298.14/239.20 |

A and B match a fresh lane today, which has no tree-sitter generated sources.
C used the arm B branch of Probe procedure. After `git checkout --detach
35a914623f`, the 60 existing delta files were re-stamped to 2020-01-01, the 6
that match the seed's `env!()` relink rule were left at now, and the 13
HEAD-only files were gone. `git diff --name-only 35a914623f` then printed
nothing, and the 233 files newer than 2020-01-02 were all under `tests/` or
`benches/`, except `.mcp.json`. The generated sources came from
`scripts/tree-sitter-generate.sh` at the basecommit, back-dated, and all four
seeded `tree_sitter_inputs.stamp` files matched
`tree-sitter-freshness.sh --print-fingerprint`.

With #9862's arm B (Tree-sitter share: generated sources present, no
`--features`, same base and command), the four probes form a 2x2 of workspace
units:

| | splits present | splits closed |
|---|---|---|
| generated sources absent | 23 (A) | 15 (B) |
| generated sources present | 22 (#9862 arm B) | 13 (C) |

The split-only units:

- With the sources absent, A − B = 8: reify-ast, reify-constraints,
  reify-expr, reify-geometry, reify-ir, reify-kernel-manifold,
  reify-shell-extract and reify-stdlib.
- With the sources present, 22 − 13 = 9: those 8 plus reify-syntax.
  Hypothesis on the set: #9862 does not list its full arm B set, but its
  count is A's 23 minus tree-sitter-reify, which #9862 measured as the whole
  arm A − arm B difference.
- B − C is tree-sitter-reify and reify-syntax.

C's 13 are reify-compiler, reify-compute-contract, reify-config,
reify-doc-build, reify-eval, reify-git-env, reify-kernel-gmsh,
reify-kernel-occt, reify-kernel-openvdb, reify-mesh-morph, reify-runtime,
reify-solver-elastic and reify-test-support. Apart from reify-git-env, which
is still split, they rebuild through four build scripts (reify-kernel-gmsh,
-occt, -openvdb and reify-solver-elastic) that `-p` compiled as new unit
variants, because their host reify-build-utils dependency got another
debuginfo level (Host profiles, fixed by #9884), and through the normal edges
that reach them. The edges come from the manifests: reify-config
normal-depends on the three kernels, reify-compiler on reify-config,
reify-test-support, reify-doc-build, reify-runtime and reify-mesh-morph on
reify-compiler, reify-compute-contract on reify-solver-elastic, and reify-eval
on them. The seed's `env!()` touches land only on `tests/` and `benches/`
sources (the relink rule in `scripts/seed-warm-lane.sh`), so they dirty test
targets, not lib units.

reify-eval rebuilds in every probe. In these probes closing every split saved
CPU, not the long pole, because the build-script variants still rebuilt
reify-eval. #9884 removed their profile cause (Host profiles); the kernels'
accepted member-feature splits still make a plain `-p reify-eval` build them
as feature variants. The walls (5:36 to 5:48 under very different load) are
indicative only.

### Decision

| Class | Features | Decision |
|---|---|---|
| (a) serde vocabulary | reify-core, reify-ast and reify-ir `serde` | **Closed** |
| (b) test-only API | `test-support`, `test-utils`, `test-fixtures`, `testing`, `containment-doubles`, `eval-helpers`, reify-kernel-gmsh/-occt `mesh-morph` | Accepted |
| (c) semantic | `stub_register`, `linear-solve-fault-injection`, `test-instrumentation` | Accepted |

- **(a) Closed.** reify-core's `serde` is default-on, and reify-ast's and
  reify-ir's pass-through `serde` features are deleted, with reify-ast's
  optional serde dependency. Every shipped binary already enabled all three,
  as `cargo tree -e normal` shows for reify-cli, reify-gui and
  `reify-gui --features gui`; the ast and ir features gated no code. reify-core
  keeps an opt-out feature instead of unconditional derives, because those
  would rewrite 149 `cfg(feature = "serde")` sites, 145 of them in
  `crates/reify-core/src/diagnostics.rs`, a file many tasks append to. No
  member sets `default-features = false` on reify-core.
  Effect, from the trial on `8c9f190b81`: 211 → 146 split pairs, 341 → 212
  summed affected members, and `-p reify-eval` affected 21 → 15. It saves 5
  of the 8 split-only units with the sources absent (reify-ast, reify-ir,
  reify-geometry, reify-kernel-manifold and reify-shell-extract), and 6 of the
  9 with them present. The sixth is reify-syntax, whose normal dependencies
  are only reify-core, reify-ast, tree-sitter and tree-sitter-reify; that is
  derived from manifest edges, not probed. On this branch `check` reports 146
  pairs over 26 selections and 13 members.
  Side effect: `cargo test -p reify-core` now runs its serde wire-form tests,
  which ran only under the workspace gate before.
- **(b) Accepted.** Enabling these uniformly would compile into production
  the test API the manifests hide on purpose, and the `dep:` features grow
  dependency graphs.
- **(c) Accepted.** They change behaviour (kernel routing, fault injection,
  instrumentation) and are never enabled in production.
- **Generated dev-dependency pins in each dependent: rejected.** After the
  serde change they would take 146 pins across about 31 manifests, and each
  pin activates a feature the pinning crate does not use, which #7124's
  `feature_activation_liveness` gate refuses for its registered pairs. The
  `dep:` features (reify-test-support `containment-doubles`/`eval-helpers`,
  reify-kernel-occt `mesh-morph`) would put reify-expr, reify-eval and
  reify-mesh-morph into low crates' test graphs, so an edit to reify-eval
  would rebuild `cargo test -p reify-syntax`. The payoff is at most 9 of 22
  units in CPU (probe C against #9862's arm B), with no long-pole gain at the
  time, when the build-script variants of Host profiles (fixed since by
  #9884) still rebuilt reify-eval.
- **Advisory MEMBER-SPLIT reporting: adopted.** Failing on the accepted
  splits would red the gate permanently.
- **Next candidate** once #9896 lands (the build-script variants were fixed
  by #9884): reify-stdlib `test-support`, behind the remaining 3 split-only
  units (reify-stdlib, reify-expr and reify-constraints).

Landing cost: the reify-core, reify-ast and reify-ir member variants change,
so the base rehashes every workspace unit once. External units are unchanged:
`check` stays OK with the same `--workspace` external variant set, so the
union and `crates/reify-eval/engine_hash_closure.txt` need no regeneration.

The gate `real_workspace_resolves_the_serde_vocabulary_crates_like_the_workspace`
reds when a MEMBER-SPLIT line names reify-core, reify-ast or reify-ir, for
example after an opt-out or a re-added non-default pass-through. Remedy: a
feature every shipped binary enables is default-on, or is not a feature.

MEMBER-SPLIT reads the same side-aware walk as SPLIT (#9873).

## Host profiles (#9884)

The union pins features; it cannot pin a profile. When the profile leaves a
host unit's debuginfo unset (a build script, a proc-macro, or a dependency of
either), cargo defers it: the unit keeps the profile's level only if the same
graph also holds a runtime unit of that package with identical features, and
is weakened to 0 otherwise. A host unit's profile, and with it the metadata
hash of every unit that depends on it, therefore depends on the selection.
`cargo test --workspace` builds reify-build-utils' own tests, so its host lib
keeps debuginfo 2 there, while `-p reify-eval` builds it at 0. Every build
script that links reify-build-utils (reify-kernel-gmsh, -occt and -openvdb,
reify-solver-elastic, reify-config, reify-eval, reify-mesh-morph, ...) then
gets a new metadata hash under `-p`, and a fresh lane compiles it as a new
unit variant. These are not reruns of the seeded units; the seed is not
involved.

Evidence:

- First session: lane `_lane-33`, a pristine seed of base `target.gen.625`
  (basecommit `35a914623f`), HEAD `0e6e461088`, verify env
  (`RUSTC_WRAPPER=sccache`, `CARGO_INCREMENTAL=0`,
  `CARGO_HOME=/tmp/reify-agent-cargo-home`, `RUSTFLAGS` unset).
  `CARGO_LOG=cargo::core::compiler::fingerprint=info cargo test -p reify-eval --no-run --features <probe B list>`
  logged `failed to read .../.fingerprint/<crate>-<hash>/...` for every
  build-script compile and run unit of gmsh, occt, openvdb and solver-elastic:
  a hash the base never had, not a dirty reason. In the seeded fingerprint
  JSON the new gmsh build-script unit matches the base's `--workspace`
  variant in features and profile; its one dependency, the host
  reify-build-utils lib, does not: same features, another debuginfo level.
- Re-measurement: HEAD `e821a27dda`, cargo 1.96.0, a scratch
  `CARGO_TARGET_DIR`, build-free unit graphs. 25 of the 37 covered members
  split. 22 split only through the host reify_build_utils lib (debuginfo 0
  against 2, features identical); `-p reify-compute-contract` adds the toml
  chain (toml 0.8.2, toml_edit, toml_datetime, serde_spanned, winnow),
  `-p reify-config` adds rustc-hash, and `-p reify-gui` adds byteorder and
  fnv. With the stanza below: 0 of 37.
- The gate at implementation: lane `_lane-33`, verify env, at the commit
  "feat(9884): workspace-feature-union check reports PROFILE-SPLIT from cargo
  test --unit-graph". `check` printed 33 PROFILE-SPLIT lines without the
  stanza, the 25 reify_build_utils lines and 8 for the external units above,
  and none with it.

The probe compares unit identity across selections without building, in about
a second per selection, and writes only `.rustc_info.json` into the scratch
dir:

    RUSTC_BOOTSTRAP=1 CARGO_TARGET_DIR=<scratch> cargo test <selection> --no-run --frozen --unit-graph -Z unstable-options

Without `--target` every unit's platform is null, so a package's host copy
and its runtime copy share one key and differ only in profile: compare each
key's set of profiles, not one profile per key.

Decision: `[profile.dev.build-override] debug = false` in the workspace
`Cargo.toml`. An explicit setting replaces the deferral, so every host unit
gets the same level under every selection, and `false` is what cargo already
picks for every host unit no runtime unit shares. `profile.test` inherits it,
`[profile.dev.package."*"]` sets no `debug`, and release builds are
unaffected. Not taken:

- a per-package `debug` pin for reify-build-utils: it covers one instance and
  needs a hand-kept list that every new build-dependency escapes, and the
  toml chain, rustc-hash, byteorder and fnv are already others;
- `debug = 2`: it re-hashes as well, and makes every build script and
  proc-macro heavier for good.

Landing cost, from `--workspace` unit graphs taken before and after the stanza
at implementation: 1,326 → 1,368 units, so every cold `--workspace` graph
holds 42 more host-only units, and 1,283 of the 1,368 change recursive
signature, so they re-hash once. The first build after landing, the merge
verify included, is effectively cold; the base then refreshes. #9825 landed a
whole-workspace re-hash on the same terms.

The gate: `check` prints `PROFILE-SPLIT <m> <pkg_id> <target> (<mode>)` for
every unit `-p <covered member>` compiles under a key (package, target, mode,
features, platform) that `--workspace` also compiles, at a profile
`--workspace` never compiles that key to, and lists only the fields that
differ. A key `--workspace` never compiles is a feature split, already
reported as SPLIT or MEMBER-SPLIT, so it is not compared again.
`-p reify-build-utils`, the host-side member, stays exempt, as it is for
features: with the stanza in place it still compiles 3 units whose key
`--workspace` compiles to another profile, quote's lib and build-script run
and hashbrown 0.17.1 (re-measured at implementation). The 38 unit graphs took
45-62 s sequentially under load average 95-130.
`real_workspace_resolves_every_member_like_the_workspace`, the gate test,
holds the invariant; a PROFILE-SPLIT line reds it like any other finding.

The gate compares each unit's own profile, not its dependency edges, so it is
a lower bound on hash identity. When `--workspace` compiles one key at two
profiles, a host copy and a runtime copy, a `-p` unit can carry the right
profile and still link the other copy of a dependency than `--workspace`'s
unit does: every unit passes, yet the dependent gets a new hash. With the
stanza every host unit compiles at debuginfo 0 and every runtime unit at 2, so
a unit's profile names its copy, a host copy links only host copies, and the
case does not arise today.

No compile-level re-probe here. Base gen.625 predates the stanza, so a first
build in a lane seeded from it re-hashes the host graph and measures nothing
about the fix. The unit graph is the evidence: a unit's metadata hash is a
function of what the graph shows (package, target, mode, features, profile
and its dependencies' identities) plus rustc and the target, which are equal
across selections. The compile-level re-probe on the first base gen built
with the stanza is #9927. The fix pays off where features already match: the
kernels' accepted member-feature splits (Member features) still make a plain
`-p reify-eval` build those packages as feature variants.

## Limits

- **Workspace-member features.** Test-only and semantic member features
  still differ between `-p` and `--workspace`, so a lane still rebuilds the
  members that carry them and their dependents. They are accepted, and `check`
  reports each as an advisory MEMBER-SPLIT line. Measurements and decision:
  Member features (#9825).
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
- **What the gate proves.** The gate checks `cargo tree` resolution and
  `cargo test --unit-graph` profiles, not compiled units: profiles are
  compared per unit key, not along dependency edges (Host profiles), and
  nothing is built. Resolution reproduced the task's observed syn
  fingerprint exactly, and the probes above are the compile-level evidence
  for features. The compile-level re-probe for host
  profiles, on the first base gen built with the stanza, is #9927.
- **Cold and release builds.** Every Measurements row outside the Cold build
  subsection is a warm-lane build, whose base has already compiled the
  `--workspace` external variants the union pins. A cold target must compile the
  union's whole dependency set: tokio, the url/icu stack, nalgebra, regex,
  futures and serde_json. Every covered member depends on the union, including
  the DAG root reify-core and, by their manifests, the otherwise dependency-free
  reify-gcode and reify-spec-conformance, so none of them can start before the
  union's metadata exists. The Cold build subsection of Measurements gives the
  cost for reify-core.

  Dev-only features come along. The union pins tokio with `test-util`, so
  every build of a covered member, production `-p` and release builds
  included, compiles it in. This is deliberate; the reason is the
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
- **Side attribution (#9873).** `cargo tree`'s text has no host/target
  marker. `--workspace` prints a member reached on the host side as two depth-0
  roots, which can be textually identical, and its `(*)` dedupe spans roots.
  `--workspace` also lists every member's `[dev-dependencies]` wherever the
  member appears, including below another member's build edge to it, while `-p`
  lists only the selected member's: the `-p` trees of reify-eval, reify-audit,
  reify-config and reify-syntax hold 0 nested dev sections, against 27 in
  `--workspace`.

  Decision. `--no-dedupe` is infeasible: on the real workspace it was stopped
  after 600 s at 4.28 GB / 36.2M lines, against 1,660 lines in 0.68 s deduped.
  So `check` walks the deduped graph, where a `(*)` line resolves to the first
  line of the same (node, side). It takes the closure from every member root,
  that is every root but the host copy of a member printed twice, so each target
  root and a proc-macro's one root, which is on the host side. It follows
  dev-dependencies only at those roots. It places a member's two roots by
  cargo's printing rules: a node's first line lists its dependencies and every
  later line is `(*)`, which a node without dependencies never gets, and there
  is one feature set per package and side. Each pair that no line printed so far
  decides doubles the side assignments still open, so `check` exits 2, naming
  the undecided pairs, once more than 64 are open; the real workspace has one
  pair.

  Effect on the real workspace. One member, reify-build-utils, is printed as
  two roots, and exactly one side assignment survives. The `--workspace`
  external set drops the 5 phantom HOST variants that reify-build-utils'
  dev-dependencies, listed below its build edges, used to put there (tempfile
  3.27.0, rustix 1.1.4, getrandom 0.4.2, linux-raw-sys 0.12.1, once_cell 1.21.4;
  450 → 445 variants), and adds none. The SPLIT verdict, the 37 `-p` sets and
  the 146 MEMBER-SPLIT (variant, selection) pairs are unchanged. `generate` on
  the real workspace leaves the union manifest and `Cargo.lock` byte-identical,
  so neither the union nor `crates/reify-eval/engine_hash_closure.txt` needs
  regenerating.

  Scratch reproductions, fixed and pinned by the gate's fixtures. These are
  real cargo 1.96 workspaces of path members plus offline-cached memchr 2.8.1 /
  log 0.4.30. A host-side member whose host root is printed before the build
  edge to it used to give a spurious `SPLIT ... no host variant`, and in the same
  shape it masked a real target split, even one whose differing dependency is a
  leaf. Fixtures:
  `check_and_generate_are_green_when_a_host_side_members_host_root_is_printed_before_its_build_edge`,
  `check_reds_and_generate_pins_a_target_split_that_a_host_root_used_to_mask` and
  `check_reds_on_a_leaf_target_split_that_a_host_root_used_to_mask`. A fourth,
  `check_finds_a_host_variant_whose_only_expanded_line_sits_below_a_dev_section`,
  pins that a subtree whose only expanded copy sits inside another member's dev
  section is still read. Skipping such sections, the refuted fix, lost real
  variants: on the real tree reify-build-utils and its HOST toml 0.8.2 are
  expanded only below reify-audit -> reify-syntax's `[dev-dependencies]`.

  Refuted risk. A phantom variant can neither mask a host split nor be pinned.
  One feature set per side makes a phantom line the same node as any real line
  of its key, and `-p` trees list no nested dev sections, so no `-p` set holds
  a phantom.

  Remaining limit. `check` exits 2, naming the member, when two side
  assignments obey the rules and resolve different packages
  (`check_exits_2_when_the_tree_cannot_say_which_root_of_a_member_is_its_host_copy`).
  The shape: both of a member's roots are first expanded at the roots, its own
  features are identical on both sides, and a dependency is reached only through
  those two roots, with side-dependent features. No such shape exists on the
  real workspace today.

  Context: the `--no-dedupe`, nested-dev-section and scratch-workspace numbers
  were measured in #9873's architect session (HEAD a9a17b3254, cargo 1.96.0);
  the real-workspace effect was re-measured at implementation on the same base
  and cargo, 2026-11-07.
