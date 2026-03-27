/**
 * Runtime type guards for Claude sidecar event payloads.
 *
 * Each guard validates the wire-format payload fields that arrive from
 * the sidecar IPC channel. The `type` discriminator is injected by the
 * bridge mapper, so guards validate everything *except* `type`.
 *
 * On validation failure the caller should console.warn and skip the
 * message — matching the project's established pattern (meshManager.ts).
 */

/** Check that `value` is a non-null, non-array object (a record). */
export function isRecordPayload(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** TextDelta payload: { id: string, content: string } */
export function isTextDeltaPayload(value: unknown): value is { id: string; content: string } {
  return (
    isRecordPayload(value) &&
    typeof value.id === 'string' &&
    typeof value.content === 'string'
  );
}

/** ThinkingDelta payload: { id: string, content: string } */
export function isThinkingDeltaPayload(value: unknown): value is { id: string; content: string } {
  return (
    isRecordPayload(value) &&
    typeof value.id === 'string' &&
    typeof value.content === 'string'
  );
}

/** ToolCall payload: { id: string, tool_name: string, tool_input: object } */
export function isToolCallPayload(
  value: unknown,
): value is { id: string; tool_name: string; tool_input: Record<string, unknown> } {
  return (
    isRecordPayload(value) &&
    typeof value.id === 'string' &&
    typeof value.tool_name === 'string' &&
    isRecordPayload(value.tool_input)
  );
}

/** ToolResult payload: { id: string, tool_name: string, result: any (must exist) } */
export function isToolResultPayload(
  value: unknown,
): value is { id: string; tool_name: string; result: unknown } {
  return (
    isRecordPayload(value) &&
    typeof value.id === 'string' &&
    typeof value.tool_name === 'string' &&
    'result' in value
  );
}

/** Done payload: { id: string } */
export function isDonePayload(value: unknown): value is { id: string } {
  return isRecordPayload(value) && typeof value.id === 'string';
}

/** ErrorMessage payload: { id: string, message: string } */
export function isErrorPayload(value: unknown): value is { id: string; message: string } {
  return (
    isRecordPayload(value) &&
    typeof value.id === 'string' &&
    typeof value.message === 'string'
  );
}
