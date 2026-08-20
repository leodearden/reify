/**
 * CI cover for the shared debug-MCP RPC envelope decoder (task 5731).
 *
 * `./rpcEnvelope.mjs` is the single JS-side home for decoding the three response
 * shapes `docs/debug-mcp-contract.md` §2 defines:
 *   §2a frontend in-band `{error: "<msg>"}`  → {@link isInBandError}
 *   §2b Rust `isError: true` + `Error: <msg>` text block → folded into the §2a
 *       shape by `normalizeRpcEnvelope`
 *   §2c JSON-RPC method error → surfaced as `transportError`
 *
 * Before this module existed the decode was copy-pasted across six `.mjs` smoke
 * drivers plus `./meshCountParity.mjs`, and only the last copy had any CI cover
 * at all — the drivers need a live webview + OCCT, so CI can never run them.
 * This suite is therefore the ONLY executable pin on the decode the drivers use.
 *
 * ASSERT ON THE DECODED VALUE, not on prose: every case below checks the shape
 * a driver actually branches on (`isInBandError(payload)`, `payload === null`,
 * a thrown transport error), because that is what silently changes when someone
 * edits the branch table.
 */
import { describe, it, expect } from "vitest";

import {
  isInBandError,
  makeDebugRpc,
  normalizeRpcEnvelope,
  parseTextPayload,
} from "./rpcEnvelope.mjs";
// Namespace import on purpose: the single-home guard below asserts on what this
// module does NOT export, which a named import cannot express.
import * as meshCountParity from "./meshCountParity.mjs";

describe("isInBandError — the §2a tool-outage discriminator", () => {
  // Moved verbatim from meshCountParity.test.ts, which pinned this while it was
  // that module's export. It is the discriminator every driver uses to tell "the
  // tool FAILED" from "the tool answered with something I did not expect" — a
  // distinction that decides whether the invariant under test was even exercised.
  it("accepts the frontend in-band shape", () => {
    expect(isInBandError({ error: "no active session" })).toBe(true);
  });

  it("accepts an in-band error carrying extra fields (§2a permits them)", () => {
    expect(isInBandError({ error: "boom", size: 17825792 })).toBe(true);
  });

  it("accepts the Rust isError dialect once normalizeRpcEnvelope has folded it", () => {
    // debug_server.rs answers a Rust-dispatched handler failure with
    // {content: [{type:'text', text:'Error: <msg>'}], isError: true}; the fold in
    // normalizeRpcEnvelope turns that into {error: '<text>'} so this ONE detector
    // covers both dialects. Without the fold the outage arrives as a bare string
    // and gets misreported downstream as a payload-shape problem.
    expect(isInBandError({ error: "Error: engine thread died" })).toBe(true);
  });

  it("rejects a healthy payload, and non-objects", () => {
    expect(isInBandError({ meshCount: 1 })).toBe(false);
    expect(isInBandError({ full_scope: false, eval_set: [] })).toBe(false);
    expect(isInBandError(null)).toBe(false);
    expect(isInBandError(undefined)).toBe(false);
    expect(isInBandError("Error: engine thread died")).toBe(false);
    expect(isInBandError(42)).toBe(false);
    expect(isInBandError([])).toBe(false);
    expect(isInBandError([{ error: "nested" }])).toBe(false);
  });

  it("rejects a non-string error field (§2a's discriminator is a STRING error)", () => {
    // Misfiring here would mask real data — a handler is free to use `error` for
    // something else, and §2a is specific that the in-band shape is a STRING.
    expect(isInBandError({ error: null, full_scope: false })).toBe(false);
    expect(isInBandError({ error: 500, full_scope: false })).toBe(false);
  });
});

describe("parseTextPayload — the shared try-JSON / fall-back-to-raw idiom", () => {
  // Duplicated in rpc.ts Branch 4 and meshCountParity.mjs before this module;
  // hoisting it is behaviour-preserving precisely because of the last case below
  // — a raw-string fallback is never an object, so it can never satisfy
  // isInBandError, so no caller's outage/answer verdict can change.
  it("parses a JSON object", () => {
    expect(parseTextPayload('{"foo":1}')).toEqual({ foo: 1 });
  });

  it("hands back the identical raw string when the text is not JSON", () => {
    const raw = "Error: debug-request timed out after 5000ms";
    expect(parseTextPayload(raw)).toBe(raw);
  });

  it("parses the other JSON scalars and containers faithfully", () => {
    expect(parseTextPayload("[1,2]")).toEqual([1, 2]);
    expect(parseTextPayload('"str"')).toBe("str");
    expect(parseTextPayload("null")).toBeNull();
  });

  it("never throws, for any string input", () => {
    for (const text of ["", "  ", "{", "undefined", "NaN", "{bad json}", "Error: x"]) {
      expect(() => parseTextPayload(text)).not.toThrow();
    }
  });

  it("a raw-string fallback is never an in-band error", () => {
    // The load-bearing equivalence: rpc.ts's Branch 4 used to reach its raw-string
    // return only via a caught JSON.parse, so hoisting the parse out cannot
    // introduce a new in-band-error verdict.
    expect(isInBandError(parseTextPayload("Error: engine thread died"))).toBe(false);
  });
});

describe("normalizeRpcEnvelope — the two failure dialects folded into one shape", () => {
  // Ported verbatim from meshCountParity.test.ts, which pinned this while the
  // decoder lived there, then extended with the branches those cases left open.
  // The live driver's rpc() used to inline this branch table, which made the ONE
  // piece of genuinely new transport logic in a smoke — folding the Rust `isError`
  // envelope into the frontend's in-band `{error}` shape so a single isInBandError
  // check covers both dialects — the only part with no CI cover.
  const textEnvelope = (value: unknown) => ({
    result: { content: [{ type: "text", text: JSON.stringify(value) }] },
  });

  it("returns the parsed payload for a healthy JSON text block", () => {
    const { transportError, payload } = normalizeRpcEnvelope(
      textEnvelope({ full_scope: false, eval_set: ["a"] }),
    );
    expect(transportError).toBeUndefined();
    expect(payload).toEqual({ full_scope: false, eval_set: ["a"] });
  });

  // ─── Branch 1: §2c transport error ────────────────────────────────────────
  it("reports a top-level (transport) error separately from a payload", () => {
    // The driver throws on this branch; it must never be mistaken for a tool
    // payload, in-band error or otherwise.
    const { transportError, payload } = normalizeRpcEnvelope({
      error: { code: -32601, message: "Method not found" },
    });
    expect(typeof transportError).toBe("string");
    expect(transportError).toContain("Method not found");
    expect(payload).toBeUndefined();
  });

  it("renders the transport error verbatim, and emits NO payload key at all", () => {
    // The message is pinned byte-for-byte because makeDebugRpc throws it and the
    // five drivers' waitForServer swallows it in a bare `catch {}` — a wording
    // change would be invisible until a live run. The absent `payload` KEY (not
    // merely an undefined value) is what stops a dead server from reading as a
    // tool answer: `"payload" in result` is false, so no `?? null` default can
    // resurrect it.
    const result = normalizeRpcEnvelope({
      error: { code: -32601, message: "method not found: x" },
    });
    expect(result).toEqual({
      transportError: 'RPC error: {"code":-32601,"message":"method not found: x"}',
    });
    expect("payload" in result).toBe(false);
  });

  // ─── Branch 2: §2b isError fold ───────────────────────────────────────────
  it("folds the Rust isError envelope into the in-band {error} shape", () => {
    // debug_server.rs answers a Rust-dispatched handler failure with isError:true
    // plus a plain-text `Error: <msg>` block. Normalising it here is what lets a
    // driver's ONE isInBandError check cover engine_state, mesh_stats and
    // demand_dispatch as well as the frontend-mediated tools.
    const result = normalizeRpcEnvelope({
      result: {
        content: [{ type: "text", text: "Error: engine thread died" }],
        isError: true,
      },
    });
    expect(result.transportError).toBeUndefined();
    expect(result).toEqual({ payload: { error: "Error: engine thread died" } });
    expect(isInBandError(result.payload)).toBe(true);
  });

  it("still yields an in-band error when isError carries no text block", () => {
    // Degrading to `null` here would send the outage downstream as a shape
    // problem — the misdiagnosis this module exists to prevent. This is also the
    // ONE place the shared decoder differs from the inlined driver helpers it
    // replaces, which returned null; see the plan's design decision. Unreachable
    // against debug_server.rs (its Err arm always attaches a text block), so the
    // synthesised message is the pin rather than a live-run observation.
    const { payload } = normalizeRpcEnvelope({ result: { content: [], isError: true } });
    expect(isInBandError(payload)).toBe(true);
    expect(payload).toEqual({ error: "tool reported isError with no text content block" });
  });

  it("finds the text block even when it is not first in content", () => {
    // `.find(type === "text")`, NOT content[0] — an envelope may lead with an
    // image block. parseRpcResponse (./rpc.ts) reads this positionally instead
    // and so answers differently; that divergence is pinned in ./rpc.test.ts.
    const { payload } = normalizeRpcEnvelope({
      result: {
        content: [
          { type: "image", data: "iVBORw0KGgo=", mimeType: "image/png" },
          { type: "text", text: "Error: engine thread died" },
        ],
        isError: true,
      },
    });
    expect(payload).toEqual({ error: "Error: engine thread died" });
  });

  // ─── Branch 3: nothing to interpret ───────────────────────────────────────
  it("yields null when there is no text block to interpret", () => {
    // Empty/absent content, or a non-text block such as the image one that the
    // screenshot tools answer with: "nothing to interpret", not a failure. No
    // caller in this module reads image data, so the block is not decoded.
    // `null` (not undefined) is load-bearing: waitForServer polls on
    // `if (r !== null) return;`.
    for (const result of [
      { content: [{ type: "image", data: "iVBORw0KGgo=", mimeType: "image/png" }] },
      { content: [] },
      {},
    ]) {
      const { transportError, payload } = normalizeRpcEnvelope({ result });
      expect(transportError).toBeUndefined();
      expect(payload).toBeNull();
    }
  });

  it("yields null when the envelope carries no result at all", () => {
    expect(normalizeRpcEnvelope({})).toEqual({ payload: null });
  });

  // ─── Branches 4 & 5: the text payload ─────────────────────────────────────
  it("leaves a frontend in-band error untouched for isInBandError to catch", () => {
    // viewport_state (via query_frontend) already speaks this dialect natively;
    // normalisation must be a no-op for it rather than double-wrapping.
    const { payload } = normalizeRpcEnvelope(textEnvelope({ error: "viewport not ready" }));
    expect(isInBandError(payload)).toBe(true);
    expect(payload).toEqual({ error: "viewport not ready" });
  });

  it("hands back the raw text when the block is not JSON", () => {
    const { payload } = normalizeRpcEnvelope({
      result: { content: [{ type: "text", text: "pong" }] },
    });
    expect(payload).toBe("pong");
  });

  it("never throws on a malformed or absent envelope", () => {
    for (const envelope of [undefined, null, {}, { result: null }, { result: {} }, 42, "str", []]) {
      expect(() => normalizeRpcEnvelope(envelope as never)).not.toThrow();
      expect(normalizeRpcEnvelope(envelope as never).payload ?? null).toBeNull();
    }
  });
});

describe("makeDebugRpc — the transport the six smoke drivers duplicate", () => {
  // FIRST-EVER cover for this layer. Each driver inlined its own copy of the
  // fetch + JSON-RPC body + decode, and since a driver needs a live webview and
  // OCCT, CI could never execute any of them. The `fetchImpl` injection point
  // exists solely so this suite can drive the real code path with no server.
  const DEBUG_URL = "http://127.0.0.1:3939/mcp";

  /** A fetch stub that records its call and answers with `envelope`. */
  function stubFetch(envelope: unknown) {
    const calls: Array<[string, RequestInit]> = [];
    const fetchImpl = (url: string, init: RequestInit) => {
      calls.push([url, init]);
      return Promise.resolve({ json: () => Promise.resolve(envelope) });
    };
    return { calls, fetchImpl };
  }

  const textEnvelope = (value: unknown) => ({
    result: { content: [{ type: "text", text: JSON.stringify(value) }] },
  });

  // ─── Request shape — the half that has never been tested ──────────────────
  it("POSTs one MCP tools/call to the url it was built with", async () => {
    const { calls, fetchImpl } = stubFetch(textEnvelope({ ok: true }));
    const rpc = makeDebugRpc(DEBUG_URL, { fetchImpl });

    await rpc("viewport_state", { pane: 2 });

    expect(calls).toHaveLength(1);
    const [url, init] = calls[0];
    expect(url).toBe(DEBUG_URL);
    expect(init.method).toBe("POST");
    expect(init.headers).toEqual({ "Content-Type": "application/json" });
    expect(JSON.parse(init.body as string)).toEqual({
      jsonrpc: "2.0",
      id: 1,
      method: "tools/call",
      params: { name: "viewport_state", arguments: { pane: 2 } },
    });
  });

  it("defaults `arguments` to {} when the caller omits args", async () => {
    // Every driver's waitForServer polls with a bare `rpc('health')`.
    const { calls, fetchImpl } = stubFetch(textEnvelope({ ok: true }));
    await makeDebugRpc(DEBUG_URL, { fetchImpl })("health");
    expect(JSON.parse(calls[0][1].body as string).params).toEqual({
      name: "health",
      arguments: {},
    });
  });

  // ─── Response handling — assert the seam, not normalizeRpcEnvelope's table ─
  it("resolves to the decoded payload for a healthy JSON text block", async () => {
    const { fetchImpl } = stubFetch(textEnvelope({ meshCount: 7 }));
    await expect(makeDebugRpc(DEBUG_URL, { fetchImpl })("mesh_stats")).resolves.toEqual({
      meshCount: 7,
    });
  });

  it("RESOLVES to {error} for a §2b isError response rather than throwing", async () => {
    // The fold the five drivers gain: they previously JSON.parse-failed on the
    // `Error: <msg>` text and handed back the bare string, which then reached
    // `'error' in x` and threw a TypeError.
    const { fetchImpl } = stubFetch({
      result: {
        content: [{ type: "text", text: "Error: debug-request timed out after 5000ms" }],
        isError: true,
      },
    });
    const payload = await makeDebugRpc(DEBUG_URL, { fetchImpl })("viewport_state");
    expect(isInBandError(payload)).toBe(true);
    expect(payload).toEqual({ error: "Error: debug-request timed out after 5000ms" });
  });

  it("resolves to null when there is no text block to interpret", async () => {
    // Load-bearing for waitForServer, which polls on `if (r !== null) return;`.
    const { fetchImpl } = stubFetch({
      result: { content: [{ type: "image", data: "iVBORw0KGgo=", mimeType: "image/png" }] },
    });
    await expect(makeDebugRpc(DEBUG_URL, { fetchImpl })("screenshot")).resolves.toBeNull();
  });

  it("resolves to the raw text when the block is not JSON", async () => {
    const { fetchImpl } = stubFetch({ result: { content: [{ type: "text", text: "pong" }] } });
    await expect(makeDebugRpc(DEBUG_URL, { fetchImpl })("health")).resolves.toBe("pong");
  });

  // ─── The throw/resolve split waitForServer's bare catch{} depends on ───────
  it("REJECTS with the verbatim transport-error message on a §2c envelope error", async () => {
    // Pinned byte-for-byte: waitForServer swallows this in a bare `catch {}`, so
    // a wording change would be invisible until a live run.
    const { fetchImpl } = stubFetch({ error: { code: -32601, message: "method not found: x" } });
    await expect(makeDebugRpc(DEBUG_URL, { fetchImpl })("x")).rejects.toThrow(
      new Error('RPC error: {"code":-32601,"message":"method not found: x"}'),
    );
  });

  it("propagates a rejecting fetch, so waitForServer keeps polling", async () => {
    // Connection refused while the GUI is still booting: the driver must see a
    // rejection (its catch swallows it and retries), never a null payload that
    // would read as "server up, nothing to interpret".
    const refused = () => Promise.reject(new Error("fetch failed"));
    await expect(
      makeDebugRpc(DEBUG_URL, { fetchImpl: refused })("health"),
    ).rejects.toThrow("fetch failed");
  });
});

describe("one home — meshCountParity.mjs no longer carries an envelope decoder", () => {
  // THE anti-regression guard this task exists to install. Without it nothing
  // stops a future agent re-adding a fourth copy of the decode, which is exactly
  // how seven copies accumulated: each new driver reasonably copy-pasted the last
  // one. These are runtime module-surface assertions, not documentation checks.
  it("does not re-export the decoders that moved to rpcEnvelope.mjs", () => {
    // Deliberately NOT re-exported for back-compat: two valid import paths for
    // one symbol would quietly recreate the ambiguity the move removes.
    expect(meshCountParity.normalizeRpcEnvelope).toBeUndefined();
    expect(meshCountParity.isInBandError).toBeUndefined();
  });

  it("still exports its own surface — the parity decision and its extraction", () => {
    expect(typeof meshCountParity.checkMeshCountParity).toBe("function");
    expect(typeof meshCountParity.extractMeshCountInputs).toBe("function");
    expect(typeof meshCountParity.formatFailures).toBe("function");
    expect(typeof meshCountParity.MESH_COUNT_PARITY_MIN_BODIES).toBe("number");
  });

  it("still routes its own outage diagnosis through the SHARED discriminator", () => {
    // The internal usablePayload → isInBandError path must survive the move.
    // If the import broke, this read would degrade from `outage` to `shape` —
    // reporting a tool that FAILED as a tool that answered oddly, which is the
    // precise misdiagnosis meshCountParity.mjs's gate taxonomy exists to prevent.
    const { failures } = meshCountParity.extractMeshCountInputs({
      meshStats: { error: "boom" },
    });
    expect(failures).toContainEqual({ gate: "outage", tool: "mesh_stats", observed: "boom" });
  });
});

// The bare-`node` load constraint on the shared .mjs modules is pinned once, in
// ./sharedModuleLoad.test.ts.
