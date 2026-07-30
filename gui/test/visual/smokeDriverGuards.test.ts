/**
 * CI cover for `./smokeDriverGuards.mjs` (task 5827) — read that module's header
 * for WHY these two behaviours live outside the drivers at all.
 *
 * This suite is the ONLY executable pin on either: a `smoke_*.mjs` driver needs
 * a running reify-gui with a WebKit WebView and OCCT, so CI can never run one in
 * situ. Following `./rpcEnvelope.test.ts` on the same seam, the effectful edges
 * (`rpc`, `sleepImpl`, `log`, `fail`) are injected exactly as `makeDebugRpc`
 * injects `fetchImpl`, so every case drives the REAL code path with no server and
 * no wall-clock delay.
 *
 * ASSERT ON OBSERVABLE BEHAVIOUR, never on prose: how many times the RPC was
 * issued, how long the helper waited, whether `fail` fired, and whether the
 * operator-visible message carries the verbatim underlying error. Each message
 * assertion names the DISCRIMINATING substring — a check that would still pass
 * with the branch it claims to pin deleted is not cover.
 */
import { describe, it, expect } from "vitest";

import { describeRpcFailure, openFileWithRetry } from "./smokeDriverGuards.mjs";
import { isInBandError } from "./rpcEnvelope.mjs";

const FIXTURE = "/repo/gui/test/fixtures/find_uses_smoke.ri";

/**
 * A recording `rpc` stub answering a scripted sequence of payloads.
 *
 * The LAST entry repeats once the script runs out, so an "every attempt fails"
 * case is written as a one-element script and the assertion on call COUNT is
 * what pins the retry budget — not the length of the script.
 *
 * An `Error` in the script REJECTS instead of resolving: rpcEnvelope's
 * throw/resolve split makes both outcomes reachable for a single call, and the
 * throwing one is what a §2c transport error or an ECONNREFUSED `fetch` gives.
 */
function stubRpc(payloads: unknown[]) {
  const calls: Array<[string, Record<string, unknown> | undefined]> = [];
  const rpc = (method: string, args?: Record<string, unknown>) => {
    const payload = payloads[Math.min(calls.length, payloads.length - 1)];
    calls.push([method, args]);
    return payload instanceof Error ? Promise.reject(payload) : Promise.resolve(payload);
  };
  return { calls, rpc };
}

/** A recording `sleepImpl`: records the delay, advances no wall clock. */
function stubSleep() {
  const delays: number[] = [];
  return {
    delays,
    sleepImpl: (ms: number) => {
      delays.push(ms);
      return Promise.resolve();
    },
  };
}

/**
 * A recording `fail`.
 *
 * The real drivers' `fail` calls `process.exit(1)` and never returns; this one
 * returns, so the helper runs on to its `return` and the test can inspect both
 * the message and the returned value in one pass.
 */
function stubFail() {
  const messages: string[] = [];
  return { messages, fail: (message: string) => void messages.push(message) };
}

/** Swallow the helper's progress logging so the suite output stays readable. */
const quiet = () => {};

const TIMED_OUT = "debug-request timed out after 5000ms";
const CONN_REFUSED = "fetch failed: connect ECONNREFUSED 127.0.0.1:3939";

describe("openFileWithRetry — the retry policy smoke_find_uses.mjs was missing", () => {
  it("issues exactly one open_file when the first attempt reports ok", async () => {
    const { calls, rpc } = stubRpc([{ ok: true, path: FIXTURE }]);
    const { delays, sleepImpl } = stubSleep();
    const { messages, fail } = stubFail();

    const result = await openFileWithRetry(rpc, FIXTURE, { sleepImpl, log: quiet, fail });

    // The request shape is the contract with debug_server.rs's open_file handler.
    expect(calls).toEqual([["open_file", { path: FIXTURE }]]);
    expect(delays).toEqual([]); // a healthy GUI must not pay the retry delay
    expect(messages).toEqual([]);
    expect(result).toEqual({ ok: true, path: FIXTURE });
  });

  it("retries past a still-initialising WebView and returns the eventual success", async () => {
    // The in-band dialect of the boot window: the debug server answers `health`
    // before the WebKit WebView has finished loading.
    const { calls, rpc } = stubRpc([
      { error: TIMED_OUT },
      { error: TIMED_OUT },
      { ok: true, path: FIXTURE },
    ]);
    const { delays, sleepImpl } = stubSleep();
    const { messages, fail } = stubFail();

    const result = await openFileWithRetry(rpc, FIXTURE, { sleepImpl, log: quiet, fail });

    expect(calls).toHaveLength(3);
    expect(delays).toEqual([3000, 3000]); // one wait per FAILED attempt, none after the success
    expect(messages).toEqual([]);
    expect(result).toEqual({ ok: true, path: FIXTURE });
  });

  it("rides out a REJECTING rpc — the transport dialect of the same boot window", async () => {
    // The other half of the window: rpc() throws on a §2c transport error and
    // `fetch` itself rejects on ECONNREFUSED/ECONNRESET while the GUI settles.
    // An escaping throw would abort the whole budget on attempt 1.
    const { calls, rpc } = stubRpc([
      new Error(CONN_REFUSED),
      new Error(CONN_REFUSED),
      { ok: true, path: FIXTURE },
    ]);
    const { delays, sleepImpl } = stubSleep();
    const { messages, fail } = stubFail();

    const result = await openFileWithRetry(rpc, FIXTURE, { sleepImpl, log: quiet, fail });

    expect(calls).toHaveLength(3); // each throw cost exactly one attempt, not the budget
    expect(delays).toEqual([3000, 3000]);
    expect(messages).toEqual([]);
    expect(result).toEqual({ ok: true, path: FIXTURE });
  });

  it("gives up after 8 attempts and fails LOUDLY, naming the underlying RPC error", async () => {
    // THE gap this task closes: smoke_find_uses.mjs previously swallowed an
    // open_file failure and limped on to its activeFile check, misreporting a
    // tool outage as a wrong-file mismatch.
    const { calls, rpc } = stubRpc([{ error: TIMED_OUT }]);
    const { delays, sleepImpl } = stubSleep();
    const { messages, fail } = stubFail();

    await openFileWithRetry(rpc, FIXTURE, { sleepImpl, log: quiet, fail });

    expect(calls).toHaveLength(8); // the default budget, lifted from the sibling drivers
    expect(delays).toEqual([3000, 3000, 3000, 3000, 3000, 3000, 3000]); // 7 — NO trailing sleep
    expect(messages).toHaveLength(1);
    expect(messages[0]).toContain(TIMED_OUT);
    expect(messages[0]).toContain("after 8 attempts");
  });

  it("fails loudly when EVERY attempt throws, and quotes the transport text", async () => {
    // A GUI that never comes back must still produce the clean exit-1 verdict
    // naming the cause, not exit 2 "Unexpected error" out of main().catch.
    const { calls, rpc } = stubRpc([new Error(CONN_REFUSED)]);
    const { delays, sleepImpl } = stubSleep();
    const { messages, fail } = stubFail();

    await openFileWithRetry(rpc, FIXTURE, { sleepImpl, log: quiet, fail });

    expect(calls).toHaveLength(8); // the full budget was still consumed
    expect(delays).toHaveLength(7);
    expect(messages).toHaveLength(1);
    expect(messages[0]).toContain("after 8 attempts");
    expect(messages[0]).toContain(CONN_REFUSED);
  });

  it("names a null payload rather than reporting an empty diagnosis", async () => {
    // `rpc()` resolves to null when the response carried no text block to
    // interpret. Assert the branch-1 wording specifically: `toContain("null")`
    // alone would also pass on the `??` fallback's JSON.stringify(null).
    const { rpc } = stubRpc([null]);
    const { sleepImpl } = stubSleep();
    const { messages, fail } = stubFail();

    await openFileWithRetry(rpc, FIXTURE, { sleepImpl, log: quiet, fail });

    expect(messages).toHaveLength(1);
    expect(messages[0]).toContain("open_file returned null");
  });

  it("fails when every attempt answers a well-formed payload with ok: false", async () => {
    // A refusal, not an outage: the tool ANSWERED and declined. There is no
    // `error` string to quote, so the message must carry the payload itself.
    const { calls, rpc } = stubRpc([{ ok: false, path: FIXTURE }]);
    const { sleepImpl } = stubSleep();
    const { messages, fail } = stubFail();

    await openFileWithRetry(rpc, FIXTURE, { sleepImpl, log: quiet, fail });

    expect(calls).toHaveLength(8);
    expect(messages).toHaveLength(1);
    expect(messages[0]).toContain("did not report ok");
    expect(messages[0]).toContain('"ok":false');
  });

  it("honours attempts / delayMs overrides", async () => {
    const { calls, rpc } = stubRpc([{ error: TIMED_OUT }]);
    const { delays, sleepImpl } = stubSleep();
    const { messages, fail } = stubFail();

    await openFileWithRetry(rpc, FIXTURE, { attempts: 3, delayMs: 10, sleepImpl, log: quiet, fail });

    expect(calls).toHaveLength(3);
    expect(delays).toEqual([10, 10]); // attempts - 1
    expect(messages).toHaveLength(1);
    expect(messages[0]).toContain("after 3 attempts");
  });

  it("REJECTS by default, so a driver that forgets to pass `fail` cannot continue", async () => {
    // The default `fail` throws. Silently resolving on exhaustion would rebuild
    // the exact swallow this task removes, one forgotten argument at a time.
    const { rpc } = stubRpc([{ error: TIMED_OUT }]);
    const { sleepImpl } = stubSleep();

    await expect(
      openFileWithRetry(rpc, FIXTURE, { attempts: 1, sleepImpl, log: quiet }),
    ).rejects.toThrow(TIMED_OUT);
  });
});

describe("describeRpcFailure — the diagnosis smoke_multi_pane_e2e.mjs was missing", () => {
  it("returns null for a healthy payload, so the caller proceeds to its own checks", () => {
    expect(describeRpcFailure({ viewports: { "pane-1": { meshCount: 2 } } }, "store_state")).toBeNull();
  });

  it("returns null for an EMPTY object — an empty result is not an RPC failure", () => {
    // Load-bearing: gap 2's genuine "no viewports registered" case must stay a
    // viewport-count failure with its own accurate message.
    expect(describeRpcFailure({}, "store_state")).toBeNull();
  });

  it("names the label when the payload is null or undefined", () => {
    for (const empty of [null, undefined]) {
      const diagnosis = describeRpcFailure(empty, "store_state");
      expect(diagnosis).toBe("store_state returned null");
    }
  });

  it("surfaces a §2a in-band error VERBATIM, alongside the label", () => {
    // The core of gap 2: this text is the only thing that says what went wrong.
    const diagnosis = describeRpcFailure({ error: "no active session" }, "store_state");
    expect(diagnosis).toContain("store_state");
    expect(diagnosis).toContain("no active session");
  });

  it("surfaces the folded §2b dialect the same way", () => {
    // normalizeRpcEnvelope folds a Rust `isError: true` + `Error: <msg>` block
    // into the §2a shape precisely so ONE check covers both dialects.
    const diagnosis = describeRpcFailure({ error: "Error: engine thread died" }, "mesh_stats");
    expect(diagnosis).toContain("mesh_stats");
    expect(diagnosis).toContain("Error: engine thread died");
  });

  it("diagnoses a non-object payload instead of throwing on it", () => {
    // `rpc()` hands back the RAW TEXT when a content block is not JSON, so a
    // guard that assumes an object is one live run away from a TypeError.
    for (const payload of ["Error: engine thread died", 42, ["pane-1"], true]) {
      let diagnosis: string | null = null;
      expect(() => {
        diagnosis = describeRpcFailure(payload, "store_state");
      }).not.toThrow();
      expect(diagnosis).toContain("store_state");
      expect(diagnosis).toContain(JSON.stringify(payload));
    }
  });

  it("agrees with isInBandError on a NON-STRING error field", () => {
    // Delegation pin: §2a specifies a STRING `error` and 5731 made isInBandError
    // its single home, whereas the sibling drivers' `'error' in X` idiom fires on
    // any `error` key. Asserting both sides means the two cannot drift silently.
    expect(isInBandError({ error: 500 })).toBe(false);
    expect(describeRpcFailure({ error: 500 }, "store_state")).toBeNull();
  });
});

// The bare-`node` load constraint on the shared .mjs modules is pinned once, in
// ./sharedModuleLoad.test.ts.
