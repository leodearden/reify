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
});
