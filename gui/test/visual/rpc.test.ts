import { describe, it, expect } from "vitest";
import { parseRpcResponse } from "./rpc";
import { isInBandError, normalizeRpcEnvelope } from "./rpcEnvelope.mjs";

describe("parseRpcResponse", () => {
  it("(a) text content with valid JSON → ok:true, value parsed as object", () => {
    const envelope = {
      result: {
        content: [{ type: "text", text: '{"foo":1}' }],
      },
    };
    const result = parseRpcResponse(envelope);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value).toEqual({ foo: 1 });
    }
  });

  it("(b) image content → ok:true, value:{data:\"AAA=\"}", () => {
    const envelope = {
      result: {
        content: [{ type: "image", data: "AAA=" }],
      },
    };
    const result = parseRpcResponse(envelope);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value).toEqual({ data: "AAA=" });
    }
  });

  it("(c) text content with non-JSON string → ok:true, value is the raw string", () => {
    const envelope = {
      result: {
        content: [{ type: "text", text: "hello" }],
      },
    };
    const result = parseRpcResponse(envelope);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value).toBe("hello");
    }
  });

  it("(d) result.isError=true → ok:false, error from content[0].text", () => {
    const envelope = {
      result: {
        isError: true,
        content: [{ type: "text", text: "bad input" }],
      },
    };
    const result = parseRpcResponse(envelope);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toBe("bad input");
    }
  });

  it("(e) top-level error field → ok:false, error from error.message", () => {
    const envelope = {
      error: { code: -32601, message: "method not found" },
    };
    const result = parseRpcResponse(envelope);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toBe("method not found");
    }
  });

  it("(f) result with empty/missing content → ok:true, value is the result object", () => {
    const envelope = {
      result: {},
    };
    const result = parseRpcResponse(envelope);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value).toEqual({});
    }
  });

  it("(g) image content with no data field → ok:false, error:'image content missing data field'", () => {
    const envelope = {
      result: {
        content: [{ type: "image" }], // data field absent
      },
    };
    const result = parseRpcResponse(envelope);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toBe("image content missing data field");
    }
  });

  it("(h) text content with no text field → ok:false, error:'text content missing text field'", () => {
    const envelope = {
      result: {
        content: [{ type: "text" }], // text field absent
      },
    };
    const result = parseRpcResponse(envelope);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toBe("text content missing text field");
    }
  });

  // task-4305 E1: in-band {error:<string>} envelope — debug handlers return failures as
  // Ok(json!({"error":...})) (no MCP isError flag), so the error rides inside the text
  // content block and must be mapped to {ok:false, error} rather than silently swallowed.
  it("(i) text content carrying {\"error\":\"timeout\"} → ok:false, error:\"timeout\"", () => {
    const envelope = {
      result: {
        content: [{ type: "text", text: '{"error":"timeout"}' }],
      },
    };
    const result = parseRpcResponse(envelope);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toBe("timeout");
    }
  });

  it("(j) text content carrying {\"error\":\"engine_phase\",\"phase\":\"error\"} → ok:false, error:\"engine_phase\"", () => {
    const envelope = {
      result: {
        content: [{ type: "text", text: '{"error":"engine_phase","phase":"error"}' }],
      },
    };
    const result = parseRpcResponse(envelope);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toBe("engine_phase");
    }
  });

  it("(k) regression guard: text content {\"meshCount\":1} (no error field) → ok:true, value:{meshCount:1}", () => {
    const envelope = {
      result: {
        content: [{ type: "text", text: '{"meshCount":1}' }],
      },
    };
    const result = parseRpcResponse(envelope);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value).toEqual({ meshCount: 1 });
    }
  });

  // task-4305 amendment (Suggestion 1): Branch 5 coverage.
  // inBandError is applied to the bare result object (Branch 5) as well as to the
  // parsed text-content JSON (Branch 4), but the original step-1/step-2 cases only
  // covered Branch 4. These two cases verify the newly-introduced {ok:false} return
  // path in Branch 5 and guard against regression to the unconditional ok:true.
  it("(l) Branch 5: bare result {error:'timeout'} (no content array) → ok:false, error:'timeout'", () => {
    const envelope = {
      result: { error: "timeout" },
    };
    const result = parseRpcResponse(envelope);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toBe("timeout");
    }
  });

  it("(m) Branch 5 regression guard: bare result {meshCount:1} (no error field) → ok:true, value:{meshCount:1}", () => {
    const envelope = {
      result: { meshCount: 1 },
    };
    const result = parseRpcResponse(envelope);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value).toEqual({ meshCount: 1 });
    }
  });
});

describe("parseRpcResponse vs normalizeRpcEnvelope — the documented divergence", () => {
  // Two decoders read the same debug-MCP envelopes, and after task 5731 they
  // live one import apart and share the §2a discriminator and the text parse.
  // That closeness is exactly what makes the REMAINING divergence dangerous:
  // it now looks like an oversight a tidy-minded agent should collapse. It is
  // not. parseRpcResponse collapses every failure into {ok:false,error} for the
  // typed TS harness; normalizeRpcEnvelope preserves the in-band shape so a
  // driver's isInBandError can still tell a tool OUTAGE from a wrong-shaped
  // answer. Collapsing either into the other breaks five of the cases above.
  //
  // Each case below asserts BOTH sides, so the file states what is shared and
  // what is not rather than leaving it to be rediscovered from a failing run.

  it("1. transport error: `.message` vs the JSON-stringified whole", () => {
    const envelope = { error: { code: -32601, message: "method not found: x" } };

    const typed = parseRpcResponse(envelope);
    expect(typed).toEqual({ ok: false, error: "method not found: x" });

    // The driver decoder keeps the whole error object, and reports it under a
    // DIFFERENT key — a transport failure must never read as a tool answer.
    expect(normalizeRpcEnvelope(envelope)).toEqual({
      transportError: 'RPC error: {"code":-32601,"message":"method not found: x"}',
    });
  });

  it("2. no result: an error to the harness, `null` to the driver", () => {
    // `null` is load-bearing on the driver side: waitForServer polls on
    // `if (r !== null) return;`, so "nothing to interpret" must not be a failure.
    expect(parseRpcResponse({})).toEqual({ ok: false, error: "No result in RPC response" });
    expect(normalizeRpcEnvelope({})).toEqual({ payload: null });
  });

  it("3. isError with a non-first text block: POSITIONAL vs `.find`", () => {
    // The sharpest divergence, and the reason the §2b fold cannot be shared:
    // parseRpcResponse reads content[0].text and finds `undefined` on the image
    // block, falling back to "(unknown error)"; normalizeRpcEnvelope searches
    // for the text block and recovers the real message.
    const envelope = {
      result: {
        content: [
          { type: "image", data: "AAA=" },
          { type: "text", text: "Error: x" },
        ],
        isError: true,
      },
    };
    expect(parseRpcResponse(envelope)).toEqual({ ok: false, error: "(unknown error)" });
    expect(normalizeRpcEnvelope(envelope)).toEqual({ payload: { error: "Error: x" } });
  });

  it("4. image content: a decoded value to the harness, `null` to the driver", () => {
    // parseRpcResponse has an image branch because the screenshot scenarios
    // consume the base64. No driver reads image data, so that branch has no
    // counterpart here and the block is simply not interpreted.
    const envelope = { result: { content: [{ type: "image", data: "AAA=" }] } };
    expect(parseRpcResponse(envelope)).toEqual({ ok: true, value: { data: "AAA=" } });
    expect(normalizeRpcEnvelope(envelope)).toEqual({ payload: null });
  });

  it("5. text block with no `text` field: an error to the harness, `null` to the driver", () => {
    const envelope = { result: { content: [{ type: "text" }] } };
    expect(parseRpcResponse(envelope)).toEqual({
      ok: false,
      error: "text content missing text field",
    });
    expect(normalizeRpcEnvelope(envelope)).toEqual({ payload: null });
  });

  // ─── What IS shared, and what step-10's delegation must preserve ──────────
  // These two are the whole reason the extraction is safe. They pass both
  // before and after rpc.ts starts importing the shared primitives; they go RED
  // only if that delegation changed a verdict.

  it("agree that a §2a in-band error IS one, by their own renderings", () => {
    const envelope = {
      result: { content: [{ type: "text", text: '{"error":"timeout"}' }] },
    };
    expect(parseRpcResponse(envelope)).toEqual({ ok: false, error: "timeout" });
    expect(isInBandError(normalizeRpcEnvelope(envelope).payload)).toBe(true);
  });

  it("agree that a NON-STRING error field is not one", () => {
    // §2a's discriminator is specifically a string `error`; a handler is free to
    // use the key for something else. Both decoders must let this through as a
    // successful payload, or real data gets masked as a failure.
    const envelope = {
      result: { content: [{ type: "text", text: '{"error":500}' }] },
    };
    expect(parseRpcResponse(envelope)).toEqual({ ok: true, value: { error: 500 } });
    expect(isInBandError(normalizeRpcEnvelope(envelope).payload)).toBe(false);
  });
});
