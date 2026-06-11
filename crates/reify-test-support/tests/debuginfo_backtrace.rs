//! Runtime debuginfo guarantee: a deliberately-panicking test still resolves
//! file:line in its backtrace under the lean dev/test debuginfo profile
//! (task 4450, PRD §9 β / §12 Q2 empirical gate).
//!
//! This is the decision gate for the unpacked-vs-debug=1 mechanism choice:
//! - GREEN under `split-debuginfo = "unpacked"`: on-host split-DWARF symbolication
//!   resolves file:line for OUR code → keep "unpacked".
//! - RED under `split-debuginfo = "unpacked"`: symbolication degraded → switch to
//!   `debug = 1` (line-tables-only, embedded) which GUARANTEES file:line.
//!
//! The test:
//!   1. Takes the current panic hook (restores it after), sets a temporary hook
//!      that captures `Backtrace::force_capture()` — hermetic, ignores
//!      RUST_BACKTRACE, captures while the panicking frame is still live.
//!   2. Catches a deliberate panic via `catch_unwind`.
//!   3. Restores the original hook.
//!   4. Asserts the captured text is non-empty AND contains THIS file's own frame
//!      (`debuginfo_backtrace.rs:<digits>`) — NOT merely any ".rs:" (which std
//!      internal frames always satisfy). This distinguishes "our source resolved"
//!      from "some Rust stdlib frame happened to appear".
//!
//! Safety notes:
//!   - nextest per-test process isolation makes `set_hook`/`take_hook` safe (no
//!     cross-test process-global interference).
//!   - The hook is always restored even if assertions fail (hook restored before
//!     assertions).
//!   - This file is a single-test integration file; no other tests share the hook.
//!   - reify-test-support compiles at dev opt-level 0 (the [profile.dev.package."*"]
//!     opt=3 override applies to dependency packages, not workspace members), so
//!     the test's own frames are not inlined away and resolve cleanly.
//!
//! Host requirements:
//!   This test is a hard, host-validated gate for split-DWARF symbolication.  It
//!   was validated on `x86_64-unknown-linux-gnu`, rustc 1.96.0, LLD 22.1.2, with
//!   the `.dwo` files present alongside binaries in `target/debug/deps/`.  Reify's
//!   verify pipeline runs on that same host class (single dev machine per
//!   CLAUDE.md), so this is not a cross-host portability concern in practice.
//!
//!   If verify is ever run on a different host (different toolchain, partial clean,
//!   or a relocated binary missing its companion `.dwo` files), this test may go
//!   RED for toolchain reasons rather than a profile regression.  In that case:
//!     • Check that `target/debug/deps/*.dwo` files exist alongside the test binary.
//!     • Or switch `[profile.dev]` to `debug = 1` (line-tables-only, embedded) —
//!       the PRD §12 Q2 fallback — which embeds line tables in the binary and
//!       guarantees symbolication regardless of `.dwo` file presence.
//!   Setting `REIFY_SKIP_BACKTRACE_TEST=1` skips this test for ad-hoc foreign-host
//!   runs without modifying the profile.

use std::backtrace::Backtrace;
use std::sync::{Arc, Mutex};

#[test]
fn backtrace_resolves_own_file_line() {
    // ── 0. Env opt-out for foreign-host / partial-clean runs ───────────────────
    //
    // When REIFY_SKIP_BACKTRACE_TEST=1 is set, skip rather than fail.  This is
    // intended only for ad-hoc runs on non-standard hosts (e.g. a different
    // toolchain, a relocated binary missing its companion .dwo files) where a
    // symbolication failure would be a host-portability issue, not a profile
    // regression.  Normal verify runs on the dev host do NOT set this var.
    if std::env::var("REIFY_SKIP_BACKTRACE_TEST").as_deref() == Ok("1") {
        eprintln!("REIFY_SKIP_BACKTRACE_TEST=1: skipping backtrace symbolication check");
        return;
    }

    // ── 1. Save the existing panic hook ────────────────────────────────────────
    let old_hook = std::panic::take_hook();

    // ── 2. Shared slot for the captured backtrace text ─────────────────────────
    let captured: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let captured_hook = Arc::clone(&captured);

    // ── 3. Install a temporary hook that force-captures a backtrace ────────────
    //
    // `force_capture()` is called INSIDE the hook, while the panicking frame is
    // still on the stack, so the captured backtrace includes the panic call site
    // from this test file.  It ignores `RUST_BACKTRACE` — always captures.
    std::panic::set_hook(Box::new(move |_info| {
        let bt = Backtrace::force_capture();
        *captured_hook.lock().unwrap() = format!("{bt}");
    }));

    // ── 4. Deliberately panic, caught so the test continues ────────────────────
    let _ = std::panic::catch_unwind(|| {
        panic!("deliberate panic for debuginfo backtrace check"); // LINE_PANIC
    });

    // ── 5. Restore the original hook (always, before any assertions) ───────────
    std::panic::set_hook(old_hook);

    // ── 6. Extract the captured backtrace text ─────────────────────────────────
    let bt_text = captured.lock().unwrap().clone();

    // ── 7. Assert the backtrace was non-empty ──────────────────────────────────
    assert!(
        !bt_text.is_empty(),
        "panic hook did not capture a backtrace (hook may not have run)"
    );

    // ── 8. Assert THIS file's own frame resolves to file:line ──────────────────
    //
    // Look for "debuginfo_backtrace.rs:" followed by at least one ASCII digit
    // somewhere in the captured text.  This proves that OUR source file's frame
    // resolved (not just a stdlib frame like /rustc/…/*.rs).
    let basename = "debuginfo_backtrace.rs:";
    let our_frame_resolves = bt_text.lines().any(|line| {
        line.find(basename)
            .is_some_and(|pos| {
                let after = &line[pos + basename.len()..];
                after.chars().next().is_some_and(|c| c.is_ascii_digit())
            })
    });

    assert!(
        our_frame_resolves,
        "backtrace does not resolve file:line for {basename}\n\
         This indicates split-DWARF symbolication failed under the current\n\
         lean debuginfo profile.  Switch [profile.dev] to `debug = 1` (line-\n\
         tables-only) as the PRD §12 Q2 fallback.\n\
         \n\
         Captured backtrace:\n{bt_text}"
    );
}
