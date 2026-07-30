/**
 * CI cover for the driver-side assertion policy the live smokes depend on
 * (task 5827).
 *
 * `./smokeDriverGuards.mjs` owns two behaviours the `smoke_*.mjs` drivers need
 * but CI can never execute in situ — a driver requires a running reify-gui with
 * a WebKit WebView and OCCT, so the drivers themselves are unrunnable here:
 *
 *   openFileWithRetry   the retry-until-`.ok` policy for `open_file`, which the
 *                       WebView is not ready to serve for the first few seconds
 *                       after the debug server answers `health`
 *   describeRpcFailure  the diagnosis that turns a failed RPC into a message
 *                       naming the UNDERLYING error, instead of letting it decay
 *                       into a downstream field-shape complaint (arrives in the
 *                       step-3/4 pair)
 *
 * This suite is the ONLY executable pin on either. It follows the pattern
 * `./rpcEnvelope.test.ts` established for the same seam: the effectful edges
 * (`rpc`, `sleepImpl`, `log`, `fail`) are injected exactly as `makeDebugRpc`
 * injects `fetchImpl`, so every case drives the REAL code path with no server
 * and no wall-clock delay.
 *
 * ASSERT ON OBSERVABLE BEHAVIOUR, never on prose: each case checks what a
 * driver would actually see — how many times the RPC was issued, how long the
 * helper waited, whether `fail` fired, and whether the operator-visible message
 * carries the verbatim underlying error. "Fails loudly, naming the cause" is a
 * covered behaviour here rather than an assertion about source text.
 */
import { describe, it, expect } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

import { describeRpcFailure, openFileWithRetry } from "./smokeDriverGuards.mjs";
import { isInBandError } from "./rpcEnvelope.mjs";

const FIXTURE = "/repo/gui/test/fixtures/find_uses_smoke.ri";

/**
 * A recording `rpc` stub answering a scripted sequence of payloads.
 *
 * The LAST entry repeats once the script runs out, so an "every attempt fails"
 * case is written as a one-element script and the assertion on call COUNT is
 * what pins the retry budget — not the length of the script.
 */
function stubRpc(payloads: unknown[]) {
  const calls: Array<[string, Record<string, unknown> | undefined]> = [];
  const rpc = (method: string, args?: Record<string, unknown>) => {
    const payload = payloads[Math.min(calls.length, payloads.length - 1)];
    calls.push([method, args]);
    return Promise.resolve(payload);
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
    // The case the policy exists for: the debug server answers `health` before
    // the WebKit WebView has finished loading, so the first open_file calls come
    // back in-band-failed while startup completes.
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

  it("gives up after 8 attempts and fails LOUDLY, naming the underlying RPC error", async () => {
    // THE gap this task closes. smoke_find_uses.mjs previously swallowed an
    // open_file failure entirely and limped on to its activeFile check, which
    // then misreported a tool outage as a wrong-file mismatch. The operator must
    // see the verbatim RPC error text, not a downstream field complaint.
    const { calls, rpc } = stubRpc([{ error: TIMED_OUT }]);
    const { delays, sleepImpl } = stubSleep();
    const { messages, fail } = stubFail();

    await openFileWithRetry(rpc, FIXTURE, { sleepImpl, log: quiet, fail });

    expect(calls).toHaveLength(8); // the default budget, lifted from the five sibling drivers
    expect(delays).toEqual([3000, 3000, 3000, 3000, 3000, 3000, 3000]); // 7 — NO trailing sleep
    expect(messages).toHaveLength(1);
    expect(messages[0]).toContain(TIMED_OUT);
    expect(messages[0]).toContain("8");
  });

  it("names a null payload rather than reporting an empty diagnosis", async () => {
    // `rpc()` resolves to null when the response carried no text block to
    // interpret; JSON.stringify(null) would still read as "null", but the
    // message must say so rather than interpolating an empty string.
    const { rpc } = stubRpc([null]);
    const { sleepImpl } = stubSleep();
    const { messages, fail } = stubFail();

    await openFileWithRetry(rpc, FIXTURE, { sleepImpl, log: quiet, fail });

    expect(messages).toHaveLength(1);
    expect(messages[0]).toContain("null");
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
  // GAP 2. `if (!storeState) fail('store_state returned null')` passes an
  // in-band `{error: "<msg>"}` straight through — the payload is truthy — and
  // the driver then reads `.viewports` as undefined, reports "got 0 viewports",
  // and never prints the RPC error at all. A tool OUTAGE dressed up as a
  // frontend assertion failure. Returning a diagnosis STRING (rather than a
  // boolean) is what forces the underlying text to reach the operator.

  it("returns null for a healthy payload, so the caller proceeds to its own checks", () => {
    expect(describeRpcFailure({ viewports: { "pane-1": { meshCount: 2 } } }, "store_state")).toBeNull();
  });

  it("returns null for an EMPTY object — an empty result is not an RPC failure", () => {
    // Load-bearing, and the reason this returns null rather than "looks wrong":
    // gap 2's genuine "no viewports registered" case must stay a viewport-count
    // failure with its own accurate message. Swallowing it here would trade one
    // misdiagnosis for another.
    expect(describeRpcFailure({}, "store_state")).toBeNull();
  });

  it("names the label when the payload is null or undefined", () => {
    // `rpc()` resolves to null when there was no text block to interpret.
    for (const empty of [null, undefined]) {
      const diagnosis = describeRpcFailure(empty, "store_state");
      expect(diagnosis).toContain("store_state");
      expect(diagnosis).toContain("null");
    }
  });

  it("surfaces a §2a in-band error VERBATIM, alongside the label", () => {
    // The core of gap 2: this text is the only thing that says what actually
    // went wrong, and today it is discarded entirely.
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
    // guard that assumes an object is one live run away from a TypeError — the
    // failure mode that used to kill these drivers at exit 2 instead of
    // reporting a clean exit-1 verdict.
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
    // Delegation pin. The sibling guards test `'error' in X`, which fires on any
    // `error` key; debug-mcp-contract.md §2a specifies a STRING `error`, and
    // 5731 made isInBandError the single home of that discriminator. Asserting
    // both sides here means a future edit cannot give the contract a second,
    // subtly-different definition without this failing.
    expect(isInBandError({ error: 500 })).toBe(false);
    expect(describeRpcFailure({ error: 500 }, "store_state")).toBeNull();
  });
});

describe("smokeDriverGuards.mjs — the load constraint that makes it a .mjs", () => {
  const modulePath = path.join(
    path.dirname(fileURLToPath(import.meta.url)),
    "smokeDriverGuards.mjs",
  );

  it("is plain ESM with no `node:` imports, so a bare-`node` driver can load it", () => {
    // Inherited from ./rpcEnvelope.mjs and not a style rule: the smoke runners
    // invoke `node <driver>.mjs` (never `tsx`), while vitest loads this same
    // module through vite's browser-condition resolver. A `node:` import would
    // resolve for the drivers and break here — and vitest cannot catch that by
    // merely importing the module, because this test file itself imports
    // `node:fs` quite happily. A source-level check is the only way the
    // constraint can fail loudly instead of at the next live run.
    const source = fs.readFileSync(modulePath, "utf8");
    expect(source).not.toMatch(/from\s+["']node:/);
    expect(source).not.toMatch(/require\s*\(/);
  });
});
