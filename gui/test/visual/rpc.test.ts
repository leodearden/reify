import { describe, it, expect } from "vitest";
import { parseRpcResponse } from "./rpc";

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
});
