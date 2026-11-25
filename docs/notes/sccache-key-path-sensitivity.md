# sccache: what the cache key depends on, and what does not hit across worktrees

Established 2026-11-18 for sccache 0.14.0 (the installed version) by reading the `v0.14.0` source and by
experiments against a private server. This note is the one home for the question "does sccache share
compiles across worktrees?". The short answer: only partly, and much less than older documents assume.

The full study (method, per-seat evidence, an independent critique, raw data) is in dark-factory at
`plans/sccache-path-key-study-2026-11-18/REPORT.md`. The laptop experiment that prompted it is at
`plans/warm-lane-hosts-study-2026-11-17/STAGE1-RESULTS.md`.

## What goes into a Rust key

`RustHasher::generate_hash_key` in `src/compiler/rust.rs` hashes, among other things:

- the working directory of the compile, unmodified;
- every `CARGO_*` environment variable except `CARGO_MAKEFLAGS`, `CARGO_REGISTRIES_*` and
  `CARGO_BUILD_JOBS` (the canonical-build-path guard, below, removes `CARGO_INCREMENTAL`, `CARGO_TERM_*`
  and `CARGO_LOG` before sccache sees them);
- the value of every environment variable rustc lists in its dep-info output, that is, everything the
  crate reads with `env!` or `option_env!`;
- the whole contents of every `--extern` rlib or rmeta;
- for each static library named by `-l static=`, its archive member names and their contents;
- the argument string, minus `--out-dir`, `-L`, `--extern`, `--check-cfg` and `--diagnostic-width`.

Source files are hashed by content only. Cargo's `-C metadata` hash is the same in every worktree.

## What that means for reify

- **Every workspace library misses at a path it has not been built at.** Cargo runs rustc for a workspace
  member with the worktree root as the working directory and sets `CARGO_MANIFEST_DIR` and
  `CARGO_MANIFEST_PATH` under it. Each of the three is enough on its own.
- **About a third of registry libraries miss too.** A registry crate that reads `env!("OUT_DIR")`
  (`serde_core`, `serde`, `thiserror` and others) gets `<worktree>/target/...` in its key. Its rlib then
  embeds that path, so every crate depending on it misses as well. In a task verify's scope, 680 of 995
  registry library units are free of this and hit at any path.
- **The same path with an empty `target/` hits everything cacheable.**
- **Test binaries, bins, build scripts and proc-macros are never cached**, at any path (sccache reports
  them as `crate-type`).
- **Under `cargo clippy`, no workspace unit is cached.** Cargo passes rustc's absolute path as the first
  argument to `clippy-driver`; sccache treats that as a second input file and refuses the compile
  (`multiple input files`). With `RUSTC=rustc` in the environment cargo passes the literal `rustc`, which
  sccache recognises, and workspace libraries become cacheable under clippy. In a toy workspace the lint
  output was replayed on a hit, clippy and `cargo check` entries stayed separate, and adding
  `-- -D warnings` produced a different key. Measured on reify it saves about 1.2 CPU-minutes per
  cold-target verify (37 library units), so it is not adopted in `verify.sh`.
- C and C++ objects built by `cc` with `-g` carry the working directory in their preprocessed output, so
  a workspace build script's objects miss at a new path.

## What does not help on 0.14.0

| Measure | Result |
|---|---|
| `RUSTFLAGS=--remap-path-prefix=<root>=...` | Worse. The flag text is hashed, so registry crates miss too. |
| cargo `trim-paths` | Not stable on the pinned toolchain; it also injects per-path remap flags. |
| A symlink at a fixed path | No effect. Cargo and rustc use the physical directory. |
| `SCCACHE_BASEDIRS` | C and C++ only. Releases 0.14.0 to 0.19.0 never apply it to Rust. |

Upstream `main` applies basedirs to Rust keys (merged 2026-11-02, in no release as of this note). Every
worktree root has to be listed when the server starts. For reify it would still miss on
`tree-sitter-reify`, `reify-kernel-occt` and `reify-kernel-openvdb`: their static archives are built by
`cc` from sources under `OUT_DIR`, `cc` names each object by a hash of the source directory, and sccache
hashes the member names. Most of the workspace depends on one of those three.

## What does help

- **Build at a path that was built before.** A stable worktree path per verify slot gives full hits with a
  cold `target/`; keeping the `target/` as well is better still, because cargo then skips the compile
  without asking sccache.
- **One canonical path for every worktree**, by binding the worktree at a fixed path in a mount namespace.
  Measured on reify on 2026-11-18: a task verify's four build steps from a never-built real path, bound at
  the canonical path with an empty `target/`, gave 1,150 hits and 0 misses, the same as a same-path build,
  and the tests of three packages that read files through baked paths passed inside the namespace (a test
  binary built there fails with a missing-file error when run outside it). The sccache server reads sources
  and writes outputs itself, so it has to run inside the same namespace; servers in different namespaces
  cannot share a local disk cache live, because each only knows the entries present when it started plus
  its own. From the host, a namespaced process's working directory reads as the canonical path, so
  anything that identifies a lane's processes by path has to change first.

  The repository now does this. The contract is dark-factory
  `plans/canonical-build-path-shared-cache-prd.md`; in brief:
  - `scripts/canon_run.py` and `scripts/canon_cache.py` run a build at the canonical path under the host
    policy (`shared`, `mixed` or `incremental`). Under `shared` and `mixed` each outermost invocation
    that runs at the canonical path starts a private sccache server on the unix socket
    `<CANON>/target/.reify-sccache.<pid>.sock`, whatever `CARGO_TARGET_DIR` or `--target-dir` names,
    reaching the WebDAV store through a loopback relay, or a local fallback cache when the store is
    unreachable.
  - The guard `scripts/canon-bin/guard/sccache` sends `-march`/`-mtune`/`-mcpu=native` and
    `target-cpu=native` compiles around the cache. It also hides `CARGO_INCREMENTAL`, `CARGO_TERM_*` and
    `CARGO_LOG` from sccache's key, so registry crates key the same under `shared` and `mixed`. cargo
    gets the incremental setting from the environment because a `cargo --config` is not forwarded to
    external subcommands such as nextest.
  - `CARGO_HOME` is pinned to `/tmp/reify-agent-cargo-home` in every policy.
  - Only a supervised invocation writes a `build_cache` record, to `REIFY_VERIFY_EVENT_LOG` when set and
    to `<build dir>/.reify-canon-journal.jsonl`: one at the canonical path, or one at its real path on an
    enabled host with reason `legacy_build_dir` or `canon_unavailable`. A disabled host, a build directory
    outside the tree, a pass-through command and a nested invocation write nothing.
  - The journal and the session files are per-tree state. `scripts/drop-canon-invocation-files.py`
    removes them from every warm-lane copy (base refresh and lane seed), and `canon_cache.open_session`
    removes session files whose supervisor is dead.

## Soundness

Serving a library built at another path is sound for reify today: the path appears in such an artefact
only in debuginfo and in the rmeta source map, and no non-test library source bakes a path it reads at run
time. Every functional use of `env!("CARGO_MANIFEST_DIR")`, `CARGO_BIN_EXE_*` and the like is in a test
binary, bin or build script, which sccache does not cache. That would change if a library started reading
`CARGO_MANIFEST_DIR` through a proc-macro at expansion time, which leaves no dep-info record.
