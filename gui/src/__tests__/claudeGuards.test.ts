import { describe, it, expect } from 'vitest';
import {
  isRecordPayload,
  isTextDeltaPayload,
  isThinkingDeltaPayload,
  isToolCallPayload,
  isToolResultPayload,
  isDonePayload,
  isErrorPayload,
} from '../claudeGuards';

// ── Base check ──────────────────────────────────────────────────────

describe('isRecordPayload', () => {
  it('accepts a plain object', () => {
    expect(isRecordPayload({ id: 'x' })).toBe(true);
  });

  it('accepts an empty object', () => {
    expect(isRecordPayload({})).toBe(true);
  });

  it('rejects null', () => {
    expect(isRecordPayload(null)).toBe(false);
  });

  it('rejects undefined', () => {
    expect(isRecordPayload(undefined)).toBe(false);
  });

  it('rejects a string', () => {
    expect(isRecordPayload('hello')).toBe(false);
  });

  it('rejects a number', () => {
    expect(isRecordPayload(42)).toBe(false);
  });

  it('rejects a boolean', () => {
    expect(isRecordPayload(true)).toBe(false);
  });

  it('rejects an array', () => {
    expect(isRecordPayload([1, 2])).toBe(false);
  });
});

// ── TextDelta guard ─────────────────────────────────────────────────

describe('isTextDeltaPayload', () => {
  it('accepts minimal valid payload', () => {
    expect(isTextDeltaPayload({ id: 'msg-1', content: 'Hello' })).toBe(true);
  });

  it('accepts payload with extra fields', () => {
    expect(isTextDeltaPayload({ id: 'msg-1', content: 'Hello', extra: 42 })).toBe(true);
  });

  it('rejects null', () => {
    expect(isTextDeltaPayload(null)).toBe(false);
  });

  it('rejects undefined', () => {
    expect(isTextDeltaPayload(undefined)).toBe(false);
  });

  it('rejects a string', () => {
    expect(isTextDeltaPayload('hello')).toBe(false);
  });

  it('rejects object missing id', () => {
    expect(isTextDeltaPayload({ content: 'Hello' })).toBe(false);
  });

  it('rejects object missing content', () => {
    expect(isTextDeltaPayload({ id: 'msg-1' })).toBe(false);
  });

  it('rejects object with non-string id', () => {
    expect(isTextDeltaPayload({ id: 123, content: 'Hello' })).toBe(false);
  });

  it('rejects object with non-string content', () => {
    expect(isTextDeltaPayload({ id: 'msg-1', content: 42 })).toBe(false);
  });
});

// ── ThinkingDelta guard ─────────────────────────────────────────────

describe('isThinkingDeltaPayload', () => {
  it('accepts minimal valid payload', () => {
    expect(isThinkingDeltaPayload({ id: 'msg-t1', content: 'Let me think...' })).toBe(true);
  });

  it('accepts payload with extra fields', () => {
    expect(isThinkingDeltaPayload({ id: 'msg-t1', content: 'thinking', extra: true })).toBe(true);
  });

  it('rejects null', () => {
    expect(isThinkingDeltaPayload(null)).toBe(false);
  });

  it('rejects undefined', () => {
    expect(isThinkingDeltaPayload(undefined)).toBe(false);
  });

  it('rejects a number', () => {
    expect(isThinkingDeltaPayload(99)).toBe(false);
  });

  it('rejects object missing id', () => {
    expect(isThinkingDeltaPayload({ content: 'thinking' })).toBe(false);
  });

  it('rejects object missing content', () => {
    expect(isThinkingDeltaPayload({ id: 'msg-t1' })).toBe(false);
  });

  it('rejects object with non-string id', () => {
    expect(isThinkingDeltaPayload({ id: true, content: 'thinking' })).toBe(false);
  });

  it('rejects object with non-string content', () => {
    expect(isThinkingDeltaPayload({ id: 'msg-t1', content: null })).toBe(false);
  });
});

// ── ToolCall guard ──────────────────────────────────────────────────

describe('isToolCallPayload', () => {
  it('accepts minimal valid payload', () => {
    expect(isToolCallPayload({ id: 'msg-2', tool_name: 'edit_file', tool_input: { path: 'main.ri' } })).toBe(true);
  });

  it('accepts payload with extra fields', () => {
    expect(isToolCallPayload({ id: 'msg-2', tool_name: 'edit_file', tool_input: {}, extra: 'stuff' })).toBe(true);
  });

  it('rejects null', () => {
    expect(isToolCallPayload(null)).toBe(false);
  });

  it('rejects undefined', () => {
    expect(isToolCallPayload(undefined)).toBe(false);
  });

  it('rejects a string', () => {
    expect(isToolCallPayload('tool_call')).toBe(false);
  });

  it('rejects object missing id', () => {
    expect(isToolCallPayload({ tool_name: 'edit_file', tool_input: {} })).toBe(false);
  });

  it('rejects object missing tool_name', () => {
    expect(isToolCallPayload({ id: 'msg-2', tool_input: {} })).toBe(false);
  });

  it('rejects object missing tool_input', () => {
    expect(isToolCallPayload({ id: 'msg-2', tool_name: 'edit_file' })).toBe(false);
  });

  it('rejects object with non-string tool_name', () => {
    expect(isToolCallPayload({ id: 'msg-2', tool_name: 42, tool_input: {} })).toBe(false);
  });

  it('rejects object with non-object tool_input (string)', () => {
    expect(isToolCallPayload({ id: 'msg-2', tool_name: 'edit_file', tool_input: 'not-object' })).toBe(false);
  });

  it('rejects object with null tool_input', () => {
    expect(isToolCallPayload({ id: 'msg-2', tool_name: 'edit_file', tool_input: null })).toBe(false);
  });

  it('rejects object with array tool_input', () => {
    expect(isToolCallPayload({ id: 'msg-2', tool_name: 'edit_file', tool_input: [1, 2] })).toBe(false);
  });
});

// ── ToolResult guard ────────────────────────────────────────────────

describe('isToolResultPayload', () => {
  it('accepts minimal valid payload', () => {
    expect(isToolResultPayload({ id: 'msg-tr1', tool_name: 'read_file', result: 'file contents' })).toBe(true);
  });

  it('accepts payload with result as object', () => {
    expect(isToolResultPayload({ id: 'msg-tr1', tool_name: 'read_file', result: { ok: true } })).toBe(true);
  });

  it('accepts payload with result as null (explicit result exists)', () => {
    expect(isToolResultPayload({ id: 'msg-tr1', tool_name: 'read_file', result: null })).toBe(true);
  });

  it('accepts payload with extra fields', () => {
    expect(isToolResultPayload({ id: 'msg-tr1', tool_name: 'read_file', result: 'ok', extra: 1 })).toBe(true);
  });

  it('rejects null', () => {
    expect(isToolResultPayload(null)).toBe(false);
  });

  it('rejects undefined', () => {
    expect(isToolResultPayload(undefined)).toBe(false);
  });

  it('rejects a boolean', () => {
    expect(isToolResultPayload(false)).toBe(false);
  });

  it('rejects object missing id', () => {
    expect(isToolResultPayload({ tool_name: 'read_file', result: 'ok' })).toBe(false);
  });

  it('rejects object missing tool_name', () => {
    expect(isToolResultPayload({ id: 'msg-tr1', result: 'ok' })).toBe(false);
  });

  it('rejects object missing result field', () => {
    expect(isToolResultPayload({ id: 'msg-tr1', tool_name: 'read_file' })).toBe(false);
  });

  it('rejects object with non-string tool_name', () => {
    expect(isToolResultPayload({ id: 'msg-tr1', tool_name: 123, result: 'ok' })).toBe(false);
  });
});

// ── Done guard ──────────────────────────────────────────────────────

describe('isDonePayload', () => {
  it('accepts minimal valid payload', () => {
    expect(isDonePayload({ id: 'msg-3' })).toBe(true);
  });

  it('accepts payload with extra fields', () => {
    expect(isDonePayload({ id: 'msg-3', usage: { tokens: 100 } })).toBe(true);
  });

  it('rejects null', () => {
    expect(isDonePayload(null)).toBe(false);
  });

  it('rejects undefined', () => {
    expect(isDonePayload(undefined)).toBe(false);
  });

  it('rejects a string', () => {
    expect(isDonePayload('done')).toBe(false);
  });

  it('rejects object missing id', () => {
    expect(isDonePayload({})).toBe(false);
  });

  it('rejects object with non-string id', () => {
    expect(isDonePayload({ id: 42 })).toBe(false);
  });
});

// ── Error guard ─────────────────────────────────────────────────────

describe('isErrorPayload', () => {
  it('accepts minimal valid payload', () => {
    expect(isErrorPayload({ id: 'msg-4', message: 'rate limit exceeded' })).toBe(true);
  });

  it('accepts payload with extra fields', () => {
    expect(isErrorPayload({ id: 'msg-4', message: 'error', code: 429 })).toBe(true);
  });

  it('rejects null', () => {
    expect(isErrorPayload(null)).toBe(false);
  });

  it('rejects undefined', () => {
    expect(isErrorPayload(undefined)).toBe(false);
  });

  it('rejects a number', () => {
    expect(isErrorPayload(500)).toBe(false);
  });

  it('rejects object missing id', () => {
    expect(isErrorPayload({ message: 'error' })).toBe(false);
  });

  it('rejects object missing message', () => {
    expect(isErrorPayload({ id: 'msg-4' })).toBe(false);
  });

  it('rejects object with non-string id', () => {
    expect(isErrorPayload({ id: 42, message: 'error' })).toBe(false);
  });

  it('rejects object with non-string message', () => {
    expect(isErrorPayload({ id: 'msg-4', message: 42 })).toBe(false);
  });
});
