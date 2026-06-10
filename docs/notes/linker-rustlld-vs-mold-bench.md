# Linker Benchmark: rust-lld vs mold

`chosen-linker: rust-lld`

Measured 2026-06-09 on the host (debug profile, sccache warm):
rustc 1.96.0, rust-lld/LLD 22.1.2, mold 2.30.0, x86_64-unknown-linux-gnu.

## Pre-1 Audit: linker-arg scan

`git grep -nE "fuse-ld|link-arg|link-args|rustc-link-arg|-Wl," -- "**/build.rs" "**/Cargo.toml" ".cargo/config.toml"` found **only `-Wl,-rpath,…` directives** in build.rs files (crates: reify-cli, reify-config, reify-kernel-gmsh, reify-kernel-occt, reify-kernel-openvdb, reify-solver-elastic, gui/src-tauri). **Zero bfd-specific linker flags** anywhere in the workspace. Both rust-lld and mold handle rpath directives correctly.

Both linkers confirmed to resolve:
- `/usr/bin/mold` — mold 2.30.0 (`mold --version`)
- `$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld` — LLD 22.1.2 (`readelf -p .comment` → `Linker: LLD 22.1.2`)

## Measurement

### Method A: link-only (binary removed, all .rlib objects cached)

The two main workspace executables (`target/debug/reify`, `target/debug/reify-audit`) were removed while all intermediate `.rlib` files were left in place, then `cargo build --workspace` was timed. This isolates pure linker overhead from compilation.

| Run | Linker | Wall-clock |
|-----|--------|-----------|
| 1 | rust-lld (no flag, toolchain default) | 1.405 s |
| 2 | rust-lld | 1.744 s |
| 3 | rust-lld | 1.191 s |
| 1 | mold (`RUSTFLAGS="-Clink-arg=-fuse-ld=mold"`) | 1.658 s |
| 2 | mold | 1.775 s |
| 3 | mold | 1.979 s |

| Linker | Median | vs rust-lld |
|--------|--------|-------------|
| rust-lld (baseline) | **1.405 s** | — |
| mold | **1.775 s** | +0.37 s (+26%) |

**Linker verification** (`readelf -p .comment target/debug/reify`):
- rust-lld builds: `Linker: LLD 22.1.2 (…)`
- mold builds: `mold 2.30.0 (compatible with GNU ld)`

### Method B: broad relink (touch reify-core, `cargo build --workspace`)

`touch crates/reify-core/src/lib.rs` forces reify-core recompilation and downstream
relinking. The compilation portion is sccache-served but its hit rate varies across
sessions (absolute-path folding in debuginfo). Method A is the cleaner signal; these
are provided for context.

| Linker | Runs (post-warmup) | Median |
|--------|-------------------|--------|
| rust-lld | 11.406 s, 11.209 s, 11.428 s | 11.406 s |
| mold | 10.8–24.2 s across two sessions | n/a (high variance) |

Mold's broad-relink variance (10–24 s) across sessions likely reflects sccache path
sensitivity when toggling RUSTFLAGS; the link-only Method A gives a stable signal.

## Decision

**Winner: rust-lld** (tie-break per PRD D3).

rust-lld is already the active default on this host — confirmed by
`.comment = "Linker: LLD 22.1.2"` in every ELF binary built without a `-fuse-ld` flag.
In the controlled link-only measurement, rust-lld (median 1.4 s) was **faster** than
mold (median 1.8 s). The broad-relink measurements showed no repeatable mold improvement.

Per PRD D3: adopt mold only on a clear, repeatable margin. Since rust-lld is faster or
equal in all controlled measurements and travels reproducibly across worktrees/hosts
without requiring a host dependency, **no `.cargo/config.toml` change is made.**
rust-lld is already the fast default; mold provides no improvement for merge-verify.

`.cargo/config.toml`: **unchanged** — no `-fuse-ld` flag added; rust-lld remains the
toolchain default for `x86_64-unknown-linux-gnu`.
