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
`workspace-feature-union.workspace = true`. A HOST-UNION line quotes a
`--workspace` path that reaches the union on the host side. It also names
the dependency edge to remove and the manifest that holds it. `--help`
explains every finding.

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

For `cargo clippy`, count `Checking` lines as well as `Compiling` lines.

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
rm -rf tree-sitter-reify/src/{parser.c,grammar.json,node-types.json,tree_sitter,.grammar_hash.stamp,.generated_outputs.stamp}
```

The last line deletes the eight gitignored generated sources that a fresh seed
lacks and the first probe recreates (see the tree-sitter paragraph under
Measurements). They live outside `target/`, so restoring `target/` does not
restore their absence. Remove `target/.pristine` after the last probe.

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
at 30.

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
only under `-p reify-expr`, through that package's dev-dependencies.

Hypothesis, not isolated by these probes: the workspace units are the sum of
four contributors and the downstream rebuild each triggers: the tree-sitter
regeneration above, the seed's `env!()`/build-dir invalidation, the
reify-test-support change, and workspace-member feature splits (#9825, see
Limits). The tree-sitter share belongs neither to the union nor to #9825, so a
measurement of the #9825 share has to separate it out.

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
  workspace units. That is #9825.
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
