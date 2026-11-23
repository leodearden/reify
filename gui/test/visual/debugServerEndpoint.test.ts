/**
 * CI cover for `./debugServerEndpoint.mjs` — read that module's header for WHY
 * the port rule and the readiness poll live outside the drivers.
 *
 * The readiness poll's effectful edges (`rpc`, `sleepImpl`, `now`, `fail`) are
 * injected, so every case drives the REAL loop with no server and no wall-clock
 * delay, and the budget is pinned against the injected clock.
 */
import { describe, it, expect } from "vitest";

import {
  DEFAULT_DEBUG_PORT,
  debugUrlForPort,
  parsePort,
  resolveDebugPort,
  waitForServer,
} from "./debugServerEndpoint.mjs";

const PORT = 4500;

describe("parsePort — the port grammar shared with Rust parse_tcp_port", () => {
  it.each([
    ["4500", 4500],
    ["1", 1],
    ["65535", 65535],
    // Rust's `str::parse::<u16>` also yields 8: pinned as agreement, not a new rule.
    ["08", 8],
  ])("accepts %j as %d", (raw, port) => {
    expect(parsePort(raw)).toBe(port);
  });

  it.each([
    undefined,
    "",
    "0",
    "65536",
    " 4500",
    "4500 ",
    "4500abc",
    "abc",
    "-1",
    "+4500",
    "4500.0",
    "1e3",
    "0x10",
  ])("refuses %j", (raw) => {
    expect(parsePort(raw)).toBeUndefined();
  });
});

describe("resolveDebugPort", () => {
  it("returns the port from REIFY_DEBUG_PORT when valid", () => {
    expect(resolveDebugPort({ REIFY_DEBUG_PORT: "4500" })).toBe(4500);
  });

  it("falls back to 3939 when REIFY_DEBUG_PORT is unset", () => {
    expect(DEFAULT_DEBUG_PORT).toBe(3939);
    expect(resolveDebugPort({})).toBe(3939);
  });

  it.each(["bad", "0", "65536", " 4500", "4500 ", "4500abc", ""])(
    "falls back to 3939 when REIFY_DEBUG_PORT is %j",
    (raw) => {
      expect(resolveDebugPort({ REIFY_DEBUG_PORT: raw })).toBe(3939);
    },
  );

  it("reads only its own variable", () => {
    expect(resolveDebugPort({ REIFY_VITE_PORT: "5173" })).toBe(3939);
  });
});

describe("debugUrlForPort", () => {
  it.each([
    [4500, "http://127.0.0.1:4500/mcp"],
    [3939, "http://127.0.0.1:3939/mcp"],
  ])("formats port %d as %s", (port, url) => {
    expect(debugUrlForPort(port)).toBe(url);
  });
});

/**
 * A fake clock: `sleepImpl` records each delay and advances `now` by it, so the
 * loop's deadline is reached in no wall-clock time.
 */
function fakeClock() {
  const delays: number[] = [];
  let t = 0;
  return {
    delays,
    now: () => t,
    advance: (ms: number) => {
      t += ms;
    },
    sleepImpl: (ms: number) => {
      delays.push(ms);
      t += ms;
      return Promise.resolve();
    },
  };
}

type FakeClock = ReturnType<typeof fakeClock>;

/**
 * A recording `rpc` stub answering a scripted sequence of outcomes.
 *
 * The LAST entry repeats once the script runs out. An `Error` entry REJECTS,
 * as a transport failure does under rpcEnvelope's throw/resolve split. A
 * function entry receives the fake clock and returns the outcome, so a single
 * call can consume time.
 */
function stubRpc(script: unknown[], clock: FakeClock) {
  const calls: Array<[string, Record<string, unknown> | undefined]> = [];
  const rpc = (method: string, args?: Record<string, unknown>) => {
    const entry = script[Math.min(calls.length, script.length - 1)];
    calls.push([method, args]);
    const outcome = typeof entry === "function" ? entry(clock) : entry;
    return outcome instanceof Error ? Promise.reject(outcome) : Promise.resolve(outcome);
  };
  return { calls, rpc };
}

/**
 * A recording `fail` that returns, unlike the drivers' own. It also records how
 * many rpc calls had been made when it fired, so a case can prove none followed.
 */
function stubFail(calls: unknown[]) {
  const messages: string[] = [];
  const callsAtFail: number[] = [];
  return {
    messages,
    callsAtFail,
    fail: (message: string) => {
      messages.push(message);
      callsAtFail.push(calls.length);
    },
  };
}

function refused() {
  return new Error("fetch failed: connect ECONNREFUSED 127.0.0.1:4500");
}

describe("waitForServer — the drivers' debug-server readiness poll", () => {
  it("issues one bare health call and returns when the first reply is non-null", async () => {
    const clock = fakeClock();
    const { calls, rpc } = stubRpc([{ status: "ok" }], clock);
    const { messages, fail } = stubFail(calls);

    await waitForServer(rpc, PORT, { now: clock.now, sleepImpl: clock.sleepImpl, fail });

    expect(calls).toStrictEqual([["health", undefined]]);
    expect(clock.delays).toEqual([]);
    expect(messages).toEqual([]);
  });

  it("keeps polling while the rpc rejects — the transport is not up yet", async () => {
    const clock = fakeClock();
    const { calls, rpc } = stubRpc([refused(), refused(), { status: "ok" }], clock);
    const { messages, fail } = stubFail(calls);

    await waitForServer(rpc, PORT, { now: clock.now, sleepImpl: clock.sleepImpl, fail });

    expect(calls).toHaveLength(3);
    expect(clock.delays).toEqual([500, 500]);
    expect(messages).toEqual([]);
  });

  it("keeps polling on a null payload", async () => {
    const clock = fakeClock();
    const { calls, rpc } = stubRpc([null, {}], clock);
    const { messages, fail } = stubFail(calls);

    await waitForServer(rpc, PORT, { now: clock.now, sleepImpl: clock.sleepImpl, fail });

    expect(calls).toHaveLength(2);
    expect(clock.delays).toEqual([500]);
    expect(messages).toEqual([]);
  });

  it("returns on an in-band {error} reply: the server answered", async () => {
    const clock = fakeClock();
    const { calls, rpc } = stubRpc([{ error: "boom" }], clock);
    const { messages, fail } = stubFail(calls);

    await waitForServer(rpc, PORT, { now: clock.now, sleepImpl: clock.sleepImpl, fail });

    expect(calls).toHaveLength(1);
    expect(clock.delays).toEqual([]);
    expect(messages).toEqual([]);
  });

  it("fails once after the default 60 s budget of 500 ms polls, then stops calling", async () => {
    const clock = fakeClock();
    const { calls, rpc } = stubRpc([refused()], clock);
    const { messages, callsAtFail, fail } = stubFail(calls);

    await waitForServer(rpc, PORT, { now: clock.now, sleepImpl: clock.sleepImpl, fail });

    expect(calls).toHaveLength(120);
    expect(clock.delays).toEqual(Array(120).fill(500));
    expect(messages).toHaveLength(1);
    expect(callsAtFail).toEqual([calls.length]);
    expect(messages[0]).toContain("4500");
    expect(messages[0]).toContain("60000ms");
  });

  it("honours a custom timeoutMs", async () => {
    const clock = fakeClock();
    const { calls, rpc } = stubRpc([refused()], clock);
    const { messages, fail } = stubFail(calls);

    await waitForServer(rpc, PORT, {
      timeoutMs: 2000,
      now: clock.now,
      sleepImpl: clock.sleepImpl,
      fail,
    });

    expect(calls).toHaveLength(4);
    expect(messages).toHaveLength(1);
    expect(messages[0]).toContain("2000ms");
  });

  it("measures the budget on the clock, not by counting polls", async () => {
    const clock = fakeClock();
    const slowRefusal = (c: FakeClock) => {
      c.advance(70_000);
      return refused();
    };
    const { calls, rpc } = stubRpc([slowRefusal], clock);
    const { messages, fail } = stubFail(calls);

    await waitForServer(rpc, PORT, { now: clock.now, sleepImpl: clock.sleepImpl, fail });

    expect(calls).toHaveLength(1);
    expect(messages).toHaveLength(1);
  });

  it("honours a custom pollMs", async () => {
    const clock = fakeClock();
    const { calls, rpc } = stubRpc([refused(), refused(), { status: "ok" }], clock);
    const { messages, fail } = stubFail(calls);

    await waitForServer(rpc, PORT, {
      pollMs: 250,
      now: clock.now,
      sleepImpl: clock.sleepImpl,
      fail,
    });

    expect(calls).toHaveLength(3);
    expect(clock.delays).toEqual([250, 250]);
    expect(messages).toEqual([]);
  });

  it("THROWS by default when the caller passes no fail", async () => {
    const clock = fakeClock();
    const { rpc } = stubRpc([refused()], clock);

    await expect(
      waitForServer(rpc, PORT, { now: clock.now, sleepImpl: clock.sleepImpl, timeoutMs: 1000 }),
    ).rejects.toThrow(/4500/);
  });
});
