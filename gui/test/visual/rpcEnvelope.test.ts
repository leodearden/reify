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
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

import { isInBandError, parseTextPayload } from "./rpcEnvelope.mjs";

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

describe("rpcEnvelope.mjs — the load constraint that makes it a .mjs", () => {
  const modulePath = path.join(
    path.dirname(fileURLToPath(import.meta.url)),
    "rpcEnvelope.mjs",
  );

  it("is plain ESM with no `node:` imports, so a bare-`node` driver can load it", () => {
    // Not a style rule: the six smoke runners invoke `node <driver>.mjs` (never
    // `tsx`), while vitest loads the same module through vite's browser-condition
    // resolver. A `node:` import would resolve for the drivers and break here —
    // and vitest cannot catch that by merely importing the module, because this
    // test file itself imports `node:fs` quite happily. A source-level check is
    // the only way the constraint can fail loudly instead of at the next live run.
    const source = fs.readFileSync(modulePath, "utf8");
    expect(source).not.toMatch(/from\s+["']node:/);
    expect(source).not.toMatch(/require\s*\(/);
  });
});
