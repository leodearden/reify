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

import {
  describeRpcFailure,
  describeScopedToggleNoBleed,
  describeUnscopedAmbiguity,
  openFileWithRetry,
} from "./smokeDriverGuards.mjs";
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

describe("describeUnscopedAmbiguity — the anti-vacuity precondition for #5891 scoping", () => {
  const TEST_ID = "fea-mode-enable-toggle";
  const opts = { testId: TEST_ID, expectedMatchCount: 2 };

  it("returns null when the unscoped probe really did find both panes' toggles", () => {
    // The live shape: dom_query's own literal plus paneDiagnostics' {viewportId,
    // matchCount}, which bridge.ts:339 emits ONLY above one match.
    const payload = {
      exists: true,
      visible: true,
      tagName: "input",
      matchCount: 2,
      viewportId: "design-main",
    };
    expect(describeUnscopedAmbiguity(payload, opts)).toBeNull();
  });

  it("delegates a null payload to describeRpcFailure, naming the tool and testId", () => {
    const diagnosis = describeUnscopedAmbiguity(null, opts);
    expect(diagnosis).toContain("returned null");
    expect(diagnosis).toContain(TEST_ID);
  });

  it("surfaces a §2a in-band error VERBATIM rather than reporting exists: undefined", () => {
    const diagnosis = describeUnscopedAmbiguity({ error: "boom" }, opts);
    expect(diagnosis).toContain("boom");
    expect(diagnosis).toContain(TEST_ID);
  });

  it("names an ABSENT testid distinctly from a count mismatch", () => {
    // exists:false is dom_query's collapse of both absence errors (bridge.ts:793).
    // "the toolbar never rendered" and "only one pane rendered it" are different
    // repairs, so they must not share wording.
    const diagnosis = describeUnscopedAmbiguity({ exists: false }, opts);
    expect(diagnosis).toContain("no element");
    expect(diagnosis).not.toContain("vacuous");
  });

  it("names the VACUITY when only one element matched — the load-bearing branch", () => {
    // paneDiagnostics returns {} at matchCount <= 1, so an ABSENT matchCount on an
    // existing element means exactly one match. Without this branch the whole
    // scoped no-bleed scenario would pass trivially in a future where only one
    // pane rendered a toolbar: there would be nothing for scoping to disambiguate.
    const diagnosis = describeUnscopedAmbiguity({ exists: true }, opts);
    expect(diagnosis).toContain("vacuous");
    expect(diagnosis).toContain(TEST_ID);
    // The ABSENT reading must not be worded as a broken wire contract: the two
    // have different repairs, and this suite is what keeps them apart.
    expect(diagnosis).not.toContain("MALFORMED");
  });

  it("names BOTH the expected and the observed count on a mismatch", () => {
    const diagnosis = describeUnscopedAmbiguity(
      { exists: true, matchCount: 3, viewportId: "design-main" },
      opts,
    );
    expect(diagnosis).toContain("3");
    expect(diagnosis).toContain("2");
  });

  it("type-checks matchCount before comparing, so a STRING count cannot slip through", () => {
    // The one-sided-guard hazard smoke_multi_pane_e2e.mjs documents at length:
    // `'2' < 2` and `undefined < 2` are both FALSE, so a magnitude-only guard
    // reports green on a malformed payload. JSON.stringify in the message is what
    // distinguishes "not a number" from a plain count mismatch.
    //
    // A PRESENT non-number is a distinct branch from an ABSENT matchCount, and
    // must be worded as one: paneDiagnostics either omits the field or writes a
    // number above 1, so `'2'` cannot have come from it at all. Telling the
    // operator "the field was absent, so exactly one element matched" would send
    // them to re-check the fixture's pane count when the wire shape is what broke.
    const diagnosis = describeUnscopedAmbiguity(
      { exists: true, matchCount: "2", viewportId: "design-main" },
      opts,
    );
    expect(diagnosis).toContain('"2"');
    expect(diagnosis).toContain("MALFORMED");
    expect(diagnosis).not.toContain("vacuous");
  });

  it("names a first match attributed to NO pane — scoping cannot work unstamped", () => {
    // resolveByTestId reads viewportId off `el.closest('[data-viewport-id]')`
    // (bridge.ts:307), so null means the element carries no pane ancestor at all.
    const diagnosis = describeUnscopedAmbiguity(
      { exists: true, matchCount: 2, viewportId: null },
      opts,
    );
    expect(diagnosis).toContain("attributed to no pane");
    expect(diagnosis).toContain("null");
  });
});

describe("describeScopedToggleNoBleed — the live counterpart of the jsdom #5891 pin", () => {
  /**
   * The all-green live reading, written out ONCE so every case below is a spread
   * override differing in exactly the field it pins.
   *
   * Shapes are the real ones: `click_element` answers a bare `{ok: true}` when
   * the scope narrowed the resolve to one element (paneDiagnostics adds its pair
   * only ABOVE one match), and `dom_query` reports the FEA body's presence, which
   * is the only per-pane proxy for `enabled` the wire offers.
   *
   * `otherToggle` and `drivenBefore` are the two probes that keep the verdict from
   * being vacuous or misattributed: the first proves `otherPaneId` is a pane that
   * really exists and carries the toggle (dom_query answers exists:false for an
   * unresolvable viewportId exactly as it does for a closed body), the second
   * proves the driven pane started CLOSED, since the click is a toggle and a
   * pre-opened pane would be closed by it and read as "the click never landed".
   */
  const PASSING = {
    drivenPaneId: "pane-1",
    otherPaneId: "design-main",
    toggleTestId: "fea-mode-enable-toggle",
    bodyTestId: "fea-mode-channel-select",
    clickResult: { ok: true } as unknown,
    otherToggle: { exists: true, visible: true, tagName: "input" } as unknown,
    otherBefore: { exists: false } as unknown,
    drivenBefore: { exists: false } as unknown,
    drivenAfter: { exists: true, visible: true, tagName: "select" } as unknown,
    otherAfter: { exists: false } as unknown,
  };

  it("returns null for the all-green reading: driven pane opened, other pane untouched", () => {
    expect(describeScopedToggleNoBleed(PASSING)).toBeNull();
  });

  it("delegates a null click result to describeRpcFailure", () => {
    const diagnosis = describeScopedToggleNoBleed({ ...PASSING, clickResult: null });
    expect(diagnosis).toContain("returned null");
    expect(diagnosis).toContain("pane-1");
  });

  it("surfaces the resolver's own not-found error VERBATIM", () => {
    // RESOLVE_BY_TESTID_ERRORS.notFoundForViewport is what a scoped click gets
    // when the pane exists but does not carry the testid — the single most likely
    // live failure, and useless to an operator unless quoted.
    const notFound =
      "element with data-testid=\"fea-mode-enable-toggle\" not found for viewport 'pane-1'";
    const diagnosis = describeScopedToggleNoBleed({
      ...PASSING,
      clickResult: { error: notFound },
    });
    expect(diagnosis).toContain(notFound);
  });

  it("names a well-formed click that simply did not report ok", () => {
    const diagnosis = describeScopedToggleNoBleed({ ...PASSING, clickResult: { ok: false } });
    expect(diagnosis).toContain("did not report ok");
  });

  it("fails when the SCOPED click still resolved several elements — scoping did nothing", () => {
    // THE scope-uniqueness branch. paneDiagnostics emits {viewportId, matchCount}
    // only above one match, so their presence on a SCOPED call says the viewportId
    // failed to disambiguate — the pane was still guessed. Their ABSENCE on the
    // passing case is correspondingly the positive evidence that it did not.
    const diagnosis = describeScopedToggleNoBleed({
      ...PASSING,
      clickResult: { ok: true, viewportId: "design-main", matchCount: 2 },
    });
    expect(diagnosis).toContain("2");
    expect(diagnosis).toContain("disambiguate");
  });

  it("refuses to certify no-bleed against a pane that is NOT ADDRESSABLE", () => {
    // THE SECOND ANTI-VACUITY BRANCH. dom_query collapses notFound and
    // notFoundForViewport alike to {exists:false} (bridge.ts:789-808), so a
    // renamed/absent otherPaneId — or one that lost its feaModeStore, since
    // Viewport gates the whole toolbar on <Show when={props.feaModeStore}> —
    // answers "closed" both before and after the click, and the guard would report
    // green having probed nothing at all. describeUnscopedAmbiguity cannot close
    // this: it pins the COUNT of matching elements, not WHICH panes carry them,
    // and the driver deliberately never asserts the pane the unscoped probe names.
    const diagnosis = describeScopedToggleNoBleed({
      ...PASSING,
      otherToggle: { exists: false },
    });
    expect(diagnosis).toContain("ADDRESSABLE");
    expect(diagnosis).toContain("design-main");
    expect(diagnosis).toContain("fea-mode-enable-toggle");
  });

  it("reports the unaddressable pane even when a later probe also looks wrong", () => {
    // Ordering pin for the precondition: with no addressable pane there is no
    // baseline to call dirty and no bleed to report, so the diagnosis must name
    // the precondition rather than a downstream symptom of it.
    const diagnosis = describeScopedToggleNoBleed({
      ...PASSING,
      otherToggle: { exists: false },
      otherBefore: { exists: true },
      otherAfter: { exists: true },
    });
    expect(diagnosis).toContain("ADDRESSABLE");
    expect(diagnosis).not.toContain("baseline");
    expect(diagnosis).not.toContain("BLEED");
  });

  it("refuses to certify no-bleed from a DIRTY baseline", () => {
    // If the other pane's body was already open before the click, "still open
    // after" proves nothing and "unchanged" is a meaningless verdict.
    const diagnosis = describeScopedToggleNoBleed({
      ...PASSING,
      otherBefore: { exists: true },
    });
    expect(diagnosis).toContain("baseline");
    expect(diagnosis).toContain("design-main");
  });

  it("names a DRIVEN pane that was already open — the click toggled it OFF", () => {
    // click_element calls el.click(), a TOGGLE and not an idempotent set. Against a
    // GUI left dirty by an earlier run that never reached the restore click, the
    // scoped click CLOSES pane-1's body: drivenAfter.exists is then false and the
    // next branch would report "the click did not land" of a click that landed and
    // worked. The asymmetry of probing one pane's baseline and assuming the other's
    // is exactly how that misattribution gets built in.
    const diagnosis = describeScopedToggleNoBleed({
      ...PASSING,
      drivenBefore: { exists: true },
      drivenAfter: { exists: false }, // the real consequence of clicking an open pane
    });
    expect(diagnosis).toContain("ALREADY");
    expect(diagnosis).toContain("pane-1");
    expect(diagnosis).not.toContain("did not land");
  });

  it("names a click that never landed on the driven pane", () => {
    const diagnosis = describeScopedToggleNoBleed({
      ...PASSING,
      drivenAfter: { exists: false },
    });
    expect(diagnosis).toContain("did not land");
    expect(diagnosis).toContain("pane-1");
  });

  it("names BOTH panes when driving one changed the other — the no-bleed branch", () => {
    // Pre-#5891 the unscoped first-match would have driven design-main, so this
    // is the branch the whole scenario exists to keep red.
    const diagnosis = describeScopedToggleNoBleed({
      ...PASSING,
      otherAfter: { exists: true },
    });
    expect(diagnosis).toContain("BLEED");
    expect(diagnosis).toContain("pane-1");
    expect(diagnosis).toContain("design-main");
  });

  it("type-checks `exists` as a boolean on every probe, so a folded payload cannot pass", () => {
    // describeRpcFailure documents a blind spot: a NON-STRING error (`{error: 500}`)
    // folds to "healthy". Comparing `exists` without checking its type would then
    // let `{}` read as "not present" and certify a silent pass.
    //
    // EVERY probe, not just the post-click pair: describeExistsProbe is
    // module-private, so each call site's type check is reachable only through
    // this function. A field left out of this loop is a guard a future edit could
    // drop — and for the three preconditions "not present" is precisely the
    // reading that certifies a pass, so those are the ones that fail silently.
    for (const field of [
      "otherToggle",
      "otherBefore",
      "drivenBefore",
      "drivenAfter",
      "otherAfter",
    ] as const) {
      const diagnosis = describeScopedToggleNoBleed({ ...PASSING, [field]: {} });
      expect(diagnosis).toContain("boolean");
      expect(diagnosis).toContain("exists");
    }
  });

  it("returns ONE diagnosis and pins WHICH branch wins when two probes are wrong", () => {
    // Branch order is a contract, not an accident: a click that never landed
    // explains the other pane's state too, so reporting the bleed first would send
    // the operator after the wrong cause.
    const diagnosis = describeScopedToggleNoBleed({
      ...PASSING,
      drivenAfter: { exists: false },
      otherAfter: { exists: true },
    });
    expect(typeof diagnosis).toBe("string");
    expect(diagnosis).toContain("did not land");
    expect(diagnosis).not.toContain("BLEED");
  });

  it("health-checks each probe rather than merely field-reading it", () => {
    const diagnosis = describeScopedToggleNoBleed({
      ...PASSING,
      otherAfter: { error: "boom" },
    });
    expect(diagnosis).toContain("boom");
  });
});

// The bare-`node` load constraint on the shared .mjs modules is pinned once, in
// ./sharedModuleLoad.test.ts.
