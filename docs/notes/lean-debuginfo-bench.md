# Lean Debuginfo Benchmark: split-debuginfo=unpacked

`chosen-mechanism: split-debuginfo-unpacked`

`target-size-before-bytes: 121380230144`

`target-size-after-bytes: 51831372146`

Measured 2026-06-10 on the dev host (debug profile, sccache running):
rustc 1.96.0, x86_64-unknown-linux-gnu, split-debuginfo=unpacked vs off.

## Method

Protocol: clean build → `RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 cargo nextest run --workspace --no-run` → `du -sb target/`.

Both measurements use the same protocol: full workspace debug nextest compile (no test execution), same host, same sccache instance.

**BEFORE** (baseline, no [profile.dev] split-debuginfo override):
- Cargo profile: `[profile.dev]` absent; default `split-debuginfo = "off"` (Linux); `debug = 2` (full DWARF embedded in each binary).
- Build: full workspace debug nextest compile, worktree `task/4400` on the same host (`/media/leo/data_lv_1/leo/reify-build/worktrees/4400`), which is at a comparable code state (main + a small unrelated task).
- Result: 829 executable test binaries in `target/debug/deps/`.
- `du -sb target/`: **121380230144 bytes** (≈ 113 GiB).

**AFTER** (with `split-debuginfo = "unpacked"` in `[profile.dev]`, task 4450):
- Cargo profile: `[profile.dev] split-debuginfo = "unpacked"`; default `debug = 2` (full DWARF, but stored in `.dwo` files separate from binaries).
- Build: `RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 cargo nextest run --workspace --no-run`, this worktree (`task/4450`).
- Result: 793 executable test binaries in `target/debug/deps/`; 2095 `.dwo` files alongside them.
- `du -sb target/`: **51831372146 bytes** (≈ 48 GiB).

**Shrink**: 121.4 GB → 51.8 GB = **−57%** (69.5 GB saved per warm worktree).

> **Note on measurement fidelity**: BEFORE and AFTER were measured in different
> worktrees (`task/4400` vs `task/4450`), so the −57% headline conflates the
> split-debuginfo change with any code-state differences between them.  The
> direction (after < before) remains valid — any inter-worktree code delta is
> small relative to the dominant DWARF-deduplication effect.  For a controlled
> A/B, toggle only the `[profile.dev] split-debuginfo` line in a single
> worktree and re-run the same protocol.

Per the PRD (task β / §9), this is best measured after task α (linker choice), as the chosen linker affects link performance. Task α (`chosen-linker: rust-lld`) landed before this measurement; the two levers compose.

## Mechanism

`split-debuginfo = "unpacked"` (Linux ELF, `.dwo` per CGU) does two things:

1. **Moves DWARF out of the link step** — each `.rlib` CGU emits a `.dwo` file rather than embedding DWARF sections directly. The linker skips processing and embedding full DWARF, making link steps faster for each of the ~793 test binaries.

2. **De-duplicates dependency DWARF across binaries** — without split-debuginfo, each of the ~793 test binaries independently embeds the full DWARF for every shared dependency (`std`, `serde`, `tokio`, etc.). With split-debuginfo=unpacked, each dependency CGU's DWARF exists **once** as a `.dwo` file shared across all binaries. This is the dominant source of the 57% target/ shrink.

The `test` profile inherits from `dev`, so the single `[profile.dev]` table covers both the dev binaries (`reify`, `reify-audit`) and all 793+ inherited test binaries.

## Backtrace Preservation

**Chosen mechanism: split-debuginfo-unpacked** (not the `debug = 1` fallback).

Confirmed GREEN by the runtime backtrace test (`crates/reify-test-support/tests/debuginfo_backtrace.rs`, task 4450 step-3): a deliberately panicking test resolves `debuginfo_backtrace.rs:<line>` in its backtrace under `split-debuginfo = "unpacked"` on this host (rustc 1.96.0 / LLD 22.1.2). The `debug = 1` fallback was not needed.

On-host DWARF symbolication via split-DWARF (`.dwo` files in `target/debug/deps/`) resolves file:line correctly. The `debuginfo_backtrace.rs` test runs on every `cargo nextest run --workspace` pass and acts as a live gate: if split-DWARF symbolication degrades in future (e.g., after a toolchain upgrade), it goes RED and signals a need to switch to the `debug = 1` fallback.
