/**
 * Typed result from parseRpcResponse — either a successful value or an error string.
 */
export type RpcResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: string };

/**
 * Parse an MCP tools/call response envelope into a typed RpcResult.
 *
 * Branch table (evaluated in order):
 * 1. Envelope has top-level `error` field → ok:false from error.message
 * 2. `result.isError === true` → ok:false from content[0].text or "(unknown error)"
 * 3. `content[0].type === "image"` → ok:true with { data: content[0].data }
 * 4. `content[0].type === "text"` → ok:true; try JSON.parse, fall back to raw string
 * 5. Otherwise → ok:true with the result object itself
 */
export function parseRpcResponse<T = unknown>(envelope: unknown): RpcResult<T> {
  const env = envelope as Record<string, unknown>;

  // Branch 1: transport-level error
  if (env.error !== undefined) {
    const err = env.error as { message?: string };
    return { ok: false, error: err.message ?? String(env.error) };
  }

  const result = env.result as Record<string, unknown> | undefined;
  if (result === undefined) {
    return { ok: false, error: "No result in RPC response" };
  }

  // Branch 2: tool-level error (isError flag)
  if (result.isError === true) {
    const content = result.content as Array<{ type: string; text?: string }> | undefined;
    const text = content?.[0]?.text ?? "(unknown error)";
    return { ok: false, error: text };
  }

  const content = result.content as Array<Record<string, unknown>> | undefined;

  if (Array.isArray(content) && content.length > 0) {
    const first = content[0];

    // Branch 3: image content
    if (first.type === "image") {
      if (typeof first.data !== "string") {
        return { ok: false, error: "image content missing data field" };
      }
      return { ok: true, value: { data: first.data } as unknown as T };
    }

    // Branch 4: text content — try JSON parse, fall back to raw string
    if (first.type === "text") {
      if (typeof first.text !== "string") {
        return { ok: false, error: "text content missing text field" };
      }
      const text = first.text;
      try {
        return { ok: true, value: JSON.parse(text) as T };
      } catch {
        return { ok: true, value: text as unknown as T };
      }
    }
  }

  // Branch 5: no recognisable content — return result object
  return { ok: true, value: result as unknown as T };
}
