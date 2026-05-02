import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Mock Tauri API modules (must be before imports that use them)
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  claudeSendMessage,
  claudeAbort,
  claudeClearSession,
  subscribeToClaudeEvents,
  MESSAGE_CONTEXT_FIELD_MAP,
  BUILD_CONTEXT_HANDLED_FIELDS,
  mapContextToWire,
  type WireMessageContext,
} from '../bridge';

const mockInvoke = vi.mocked(invoke);
const mockListen = vi.mocked(listen);

/** Helper: capture the internal listener for a given event name */
function captureListener(eventName: string) {
  let captured: ((event: { payload: unknown }) => void) | undefined;
  mockListen.mockImplementation(async (name, handler) => {
    if (name === eventName) {
      captured = handler as (event: { payload: unknown }) => void;
    }
    return vi.fn();
  });
  return {
    async setup(handler: ReturnType<typeof vi.fn>) {
      await subscribeToClaudeEvents(handler);
      if (!captured) {
        throw new Error(
          `captureListener: no handler was registered for event "${eventName}". ` +
          `Check that subscribeToClaudeEvents registers this event.`,
        );
      }
      return captured;
    },
  };
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe('claude invoke wrappers', () => {
  it('claudeSendMessage calls invoke with command and text only', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeSendMessage('hello world');

    expect(mockInvoke).toHaveBeenCalledWith('claude_send_message', {
      text: 'hello world',
      context: undefined,
    });
  });

  it('claudeSendMessage maps camelCase context to snake_case', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeSendMessage('fix this', {
      selectedEntity: 'Box.body',
      diagnostics: ['error: type mismatch'],
      constraints: ['x > 0'],
    });

    expect(mockInvoke).toHaveBeenCalledWith('claude_send_message', {
      text: 'fix this',
      context: {
        selected_entity: 'Box.body',
        diagnostics: ['error: type mismatch'],
        constraints: ['x > 0'],
      },
    });
  });

  it('claudeSendMessage maps currentFile and attachedContexts to snake_case', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeSendMessage('help me', {
      selectedEntity: 'Bracket.body',
      diagnostics: ['warning: unused variable'],
      constraints: ['width > 5'],
      currentFile: 'bracket.ri',
      attachedContexts: ['design-spec.md', 'notes.txt'],
    });

    expect(mockInvoke).toHaveBeenCalledWith('claude_send_message', {
      text: 'help me',
      context: {
        selected_entity: 'Bracket.body',
        diagnostics: ['warning: unused variable'],
        constraints: ['width > 5'],
        current_file: 'bracket.ri',
        attached_contexts: ['design-spec.md', 'notes.txt'],
      },
    });
  });

  it('claudeSendMessage omits undefined-valued fields from wire context', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeSendMessage('hello', {
      selectedEntity: 'Bracket.w',
    });

    expect(mockInvoke.mock.calls[0][1]).toStrictEqual({
      text: 'hello',
      context: { selected_entity: 'Bracket.w' },
    });
  });

  it('claudeSendMessage normalizes empty context {} to undefined', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeSendMessage('hello', {});

    expect(mockInvoke).toHaveBeenCalledWith('claude_send_message', {
      text: 'hello',
      context: undefined,
    });
  });

  it('claudeSendMessage normalizes all-undefined-fields context to undefined', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeSendMessage('hello', {
      selectedEntity: undefined,
      diagnostics: undefined,
    });

    expect(mockInvoke).toHaveBeenCalledWith('claude_send_message', {
      text: 'hello',
      context: undefined,
    });
  });

  it('claudeSendMessage preserves context when at least one field is defined', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeSendMessage('fix this', {
      selectedEntity: undefined,
      diagnostics: ['error: type mismatch'],
    });

    expect(mockInvoke.mock.calls[0][1]).toStrictEqual({
      text: 'fix this',
      context: { diagnostics: ['error: type mismatch'] },
    });
  });

  it('claudeAbort calls invoke with correct command', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeAbort();

    expect(mockInvoke).toHaveBeenCalledWith('claude_abort');
  });

  it('claudeClearSession calls invoke with correct command', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeClearSession();

    expect(mockInvoke).toHaveBeenCalledWith('claude_clear_session');
  });

  it('claudeSendMessage propagates invoke rejection', async () => {
    mockInvoke.mockRejectedValue(new Error('IPC failed'));

    await expect(claudeSendMessage('hello')).rejects.toThrow('IPC failed');
  });

  it('claudeAbort propagates invoke rejection', async () => {
    mockInvoke.mockRejectedValue(new Error('IPC failed'));

    await expect(claudeAbort()).rejects.toThrow('IPC failed');
  });

  it('claudeClearSession propagates invoke rejection', async () => {
    mockInvoke.mockRejectedValue(new Error('IPC failed'));

    await expect(claudeClearSession()).rejects.toThrow('IPC failed');
  });

  it('MESSAGE_CONTEXT_FIELD_MAP covers every key of MessageContext', () => {
    // Build a fully-populated MessageContext to extract its keys at runtime
    const fullContext: Required<MessageContext> = {
      selectedEntity: 'x',
      diagnostics: ['d'],
      constraints: ['c'],
      currentFile: 'f',
      attachedContexts: ['a'],
    };
    const expectedKeys = Object.keys(fullContext).sort();
    const mapKeys = Object.keys(MESSAGE_CONTEXT_FIELD_MAP).sort();
    expect(mapKeys).toEqual(expectedKeys);
  });

  it('BUILD_CONTEXT_HANDLED_FIELDS matches MESSAGE_CONTEXT_FIELD_MAP keys', () => {
    expect([...BUILD_CONTEXT_HANDLED_FIELDS].sort()).toEqual(
      Object.keys(MESSAGE_CONTEXT_FIELD_MAP).sort(),
    );
  });

  it('MESSAGE_CONTEXT_FIELD_MAP values match the expected snake_case wire names', () => {
    const expectedWireNames = [
      'attached_contexts',
      'constraints',
      'current_file',
      'diagnostics',
      'selected_entity',
    ];
    const mapValues = Object.values(MESSAGE_CONTEXT_FIELD_MAP).sort();
    expect(mapValues).toEqual(expectedWireNames);
  });

  it('mapContextToWire maps all fields to snake_case', () => {
    const input: MessageContext = {
      selectedEntity: 'Box.body',
      diagnostics: ['error: type mismatch'],
      constraints: ['x > 0'],
      currentFile: 'bracket.ri',
      attachedContexts: ['design-spec.md'],
    };

    const wire = mapContextToWire(input);

    expect(wire).toEqual({
      selected_entity: 'Box.body',
      diagnostics: ['error: type mismatch'],
      constraints: ['x > 0'],
      current_file: 'bracket.ri',
      attached_contexts: ['design-spec.md'],
    });

    // Verify no extra keys beyond those in the mapping table
    const wireKeys = Object.keys(wire).sort();
    const expectedWireKeys = Object.values(MESSAGE_CONTEXT_FIELD_MAP).sort();
    expect(wireKeys).toEqual(expectedWireKeys);
  });

  it('mapContextToWire omits undefined fields from output', () => {
    const input: MessageContext = {
      selectedEntity: 'Bracket.w',
    };

    const wire = mapContextToWire(input);

    // Only the defined field should be present
    expect(wire).toStrictEqual({ selected_entity: 'Bracket.w' });
    // Undefined-valued fields must be absent from the object's own keys
    expect(Object.keys(wire).sort()).toEqual(['selected_entity']);
    expect(Object.keys(wire)).not.toContain('diagnostics');
    expect(Object.keys(wire)).not.toContain('constraints');
    expect(Object.keys(wire)).not.toContain('current_file');
    expect(Object.keys(wire)).not.toContain('attached_contexts');
  });

  it('mapContextToWire output matches JSON.stringify round-trip', () => {
    const input: MessageContext = {
      selectedEntity: 'Bracket.w',
      diagnostics: ['err1'],
    };

    const wire = mapContextToWire(input);
    const roundTripped = JSON.parse(JSON.stringify(wire)) as typeof wire;

    // No information should be lost: the round-tripped object must equal the original wire object
    expect(roundTripped).toStrictEqual(wire);
  });
});

describe('subscribeToClaudeEvents', () => {
  const ALL_EVENT_NAMES = [
    'claude-text-delta',
    'claude-thinking-delta',
    'claude-tool-call',
    'claude-tool-result',
    'claude-done',
    'claude-error',
    'claude-ready',
  ];

  it('calls listen() for all 7 claude event types', async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const handler = vi.fn();
    await subscribeToClaudeEvents(handler);

    const listenedEvents = mockListen.mock.calls.map((call) => call[0]);
    for (const name of ALL_EVENT_NAMES) {
      expect(listenedEvents).toContain(name);
    }
    expect(mockListen).toHaveBeenCalledTimes(7);
  });

  it('returns a combined unlisten that calls all individual unlisteners', async () => {
    const unlisteners = ALL_EVENT_NAMES.map(() => vi.fn());
    let callIdx = 0;
    mockListen.mockImplementation(async () => {
      return unlisteners[callIdx++];
    });

    const handler = vi.fn();
    const combinedUnlisten = await subscribeToClaudeEvents(handler);

    combinedUnlisten();

    for (const unsub of unlisteners) {
      expect(unsub).toHaveBeenCalledTimes(1);
    }
  });

  it('maps claude-text-delta event to OutboundMessage { type: "text_delta" }', async () => {
    const { setup } = captureListener('claude-text-delta');
    const handler = vi.fn();
    const listener = await setup(handler);

    listener({ payload: { id: 'msg-1', content: 'Hello' } });

    expect(handler).toHaveBeenCalledWith({
      type: 'text_delta',
      id: 'msg-1',
      content: 'Hello',
    });
  });

  it('maps claude-tool-call event to OutboundMessage { type: "tool_call" }', async () => {
    const { setup } = captureListener('claude-tool-call');
    const handler = vi.fn();
    const listener = await setup(handler);

    listener({
      payload: { id: 'msg-2', tool_use_id: 'tu-2a', tool_name: 'edit_file', tool_input: { path: 'main.ri' } },
    });

    expect(handler).toHaveBeenCalledWith({
      type: 'tool_call',
      id: 'msg-2',
      tool_use_id: 'tu-2a',
      tool_name: 'edit_file',
      tool_input: { path: 'main.ri' },
    });
  });

  it('maps claude-ready event to OutboundMessage { type: "ready" }', async () => {
    const { setup } = captureListener('claude-ready');
    const handler = vi.fn();
    const listener = await setup(handler);

    listener({ payload: {} });

    expect(handler).toHaveBeenCalledWith({ type: 'ready' });
  });

  it('maps claude-done event to OutboundMessage { type: "done" }', async () => {
    const { setup } = captureListener('claude-done');
    const handler = vi.fn();
    const listener = await setup(handler);

    listener({ payload: { id: 'msg-3' } });

    expect(handler).toHaveBeenCalledWith({ type: 'done', id: 'msg-3' });
  });

  it('maps claude-error event to OutboundMessage { type: "error" }', async () => {
    const { setup } = captureListener('claude-error');
    const handler = vi.fn();
    const listener = await setup(handler);

    listener({ payload: { id: 'msg-4', message: 'rate limit exceeded' } });

    expect(handler).toHaveBeenCalledWith({
      type: 'error',
      id: 'msg-4',
      message: 'rate limit exceeded',
    });
  });

  it('maps claude-thinking-delta event to OutboundMessage { type: "thinking_delta" }', async () => {
    const { setup } = captureListener('claude-thinking-delta');
    const handler = vi.fn();
    const listener = await setup(handler);

    listener({ payload: { id: 'msg-t1', content: 'Let me think...' } });

    expect(handler).toHaveBeenCalledWith({
      type: 'thinking_delta',
      id: 'msg-t1',
      content: 'Let me think...',
    });
  });

  it('maps claude-tool-result event to OutboundMessage { type: "tool_result" }', async () => {
    const { setup } = captureListener('claude-tool-result');
    const handler = vi.fn();
    const listener = await setup(handler);

    listener({
      payload: { id: 'msg-tr1', tool_name: 'read_file', result: 'file contents here' },
    });

    expect(handler).toHaveBeenCalledWith({
      type: 'tool_result',
      id: 'msg-tr1',
      tool_name: 'read_file',
      result: 'file contents here',
    });
  });

  it('extra unknown fields in payload are not forwarded to handler', async () => {
    const { setup } = captureListener('claude-done');
    const handler = vi.fn();
    const listener = await setup(handler);

    // Simulate payload with extra unknown fields that should NOT be forwarded
    listener({ payload: { id: 'x', _internal: true, debug_ts: 12345 } });

    expect(handler).toHaveBeenCalledWith({ type: 'done', id: 'x' });
  });

  it('extra unknown fields in tool_call payload are not forwarded to handler', async () => {
    const { setup } = captureListener('claude-tool-call');
    const handler = vi.fn();
    const listener = await setup(handler);

    // Simulate tool_call payload with extra _debug field that should NOT be forwarded
    listener({
      payload: {
        id: 'tc1',
        tool_use_id: 'tu-tc1',
        tool_name: 'read',
        tool_input: { path: '/f' },
        _debug: true,
      },
    });

    expect(handler).toHaveBeenCalledWith({
      type: 'tool_call',
      id: 'tc1',
      tool_use_id: 'tu-tc1',
      tool_name: 'read',
      tool_input: { path: '/f' },
    });
  });

  it('extra unknown fields in tool_call with complex tool_input are not forwarded', async () => {
    const { setup } = captureListener('claude-tool-call');
    const handler = vi.fn();
    const listener = await setup(handler);

    const complexInput = {
      path: '/main.ri',
      operations: [{ type: 'insert', line: 5 }],
      options: { backup: true, tags: ['draft'] },
    };

    // Simulate tool_call with deeply nested tool_input AND extra top-level fields
    listener({
      payload: {
        id: 'tc-complex',
        tool_use_id: 'tu-complex',
        tool_name: 'edit_file',
        tool_input: complexInput,
        _trace_id: 'abc-123',
        _timestamp: 1711640000,
      },
    });

    expect(handler).toHaveBeenCalledWith({
      type: 'tool_call',
      id: 'tc-complex',
      tool_use_id: 'tu-complex',
      tool_name: 'edit_file',
      tool_input: complexInput,
    });
    // Verify the complex nested tool_input is preserved intact
    const received = handler.mock.calls[0][0];
    expect(received.tool_input.operations).toEqual([{ type: 'insert', line: 5 }]);
    expect(received.tool_input.options).toEqual({ backup: true, tags: ['draft'] });
    // Verify extra top-level fields are excluded
    expect(received).not.toHaveProperty('_trace_id');
    expect(received).not.toHaveProperty('_timestamp');
  });

  it('extra unknown fields in text_delta payload are not forwarded to handler', async () => {
    const { setup } = captureListener('claude-text-delta');
    const handler = vi.fn();
    const listener = await setup(handler);

    listener({ payload: { id: 'x', content: 'hi', _timestamp: 999, _meta: {} } });

    expect(handler).toHaveBeenCalledWith({ type: 'text_delta', id: 'x', content: 'hi' });
  });

  it('extra unknown fields in thinking_delta payload are not forwarded to handler', async () => {
    const { setup } = captureListener('claude-thinking-delta');
    const handler = vi.fn();
    const listener = await setup(handler);

    listener({ payload: { id: 't1', content: 'thinking...', _debug: true, _trace: 'abc' } });

    expect(handler).toHaveBeenCalledWith({ type: 'thinking_delta', id: 't1', content: 'thinking...' });
  });

  it('extra unknown fields in tool_result payload are not forwarded to handler', async () => {
    const { setup } = captureListener('claude-tool-result');
    const handler = vi.fn();
    const listener = await setup(handler);

    listener({
      payload: { id: 'tr1', tool_name: 'read_file', result: { data: 'contents' }, _internal: true, _debug_ts: 12345 },
    });

    expect(handler).toHaveBeenCalledWith({
      type: 'tool_result',
      id: 'tr1',
      tool_name: 'read_file',
      result: { data: 'contents' },
    });
  });

  it('extra unknown fields in error payload are not forwarded to handler', async () => {
    const { setup } = captureListener('claude-error');
    const handler = vi.fn();
    const listener = await setup(handler);

    listener({ payload: { id: 'e1', message: 'rate limit', _stack: 'trace...', _code: 429 } });

    expect(handler).toHaveBeenCalledWith({ type: 'error', id: 'e1', message: 'rate limit' });
  });

  it.each([
    { eventName: 'claude-thinking-delta', expectedType: 'thinking_delta', payload: { id: 'x', content: 'think', type: 'WRONG' } },
    { eventName: 'claude-text-delta', expectedType: 'text_delta', payload: { id: 'x', content: 'hi', type: 'WRONG' } },
    { eventName: 'claude-error', expectedType: 'error', payload: { id: 'x', message: 'oops', type: 'WRONG' } },
  ])('payload type field does not override mapped event type for $eventName', async ({ eventName, expectedType, payload }) => {
    const { setup } = captureListener(eventName);
    const handler = vi.fn();
    const listener = await setup(handler);

    listener({ payload });

    expect(handler).toHaveBeenCalledWith(
      expect.objectContaining({ type: expectedType }),
    );
  });

  describe('listener rollback on partial failure', () => {
    it('cleans up already-registered listeners when a middle listen() fails', async () => {
      const unlisteners = [vi.fn(), vi.fn(), vi.fn()];
      let callIdx = 0;
      mockListen.mockImplementation(async () => {
        if (callIdx < 3) {
          return unlisteners[callIdx++];
        }
        throw new Error('listen failed on call 4');
      });

      const handler = vi.fn();
      await expect(subscribeToClaudeEvents(handler)).rejects.toThrow('listen failed on call 4');

      // All 3 previously-resolved unlisteners must be called to avoid leaking
      for (const unsub of unlisteners) {
        expect(unsub).toHaveBeenCalledTimes(1);
      }
    });

    it('rejects cleanly when the very first listen() fails (no unlisteners to clean up)', async () => {
      mockListen.mockRejectedValue(new Error('listen failed on call 1'));

      const handler = vi.fn();
      await expect(subscribeToClaudeEvents(handler)).rejects.toThrow('listen failed on call 1');

      // No unlisteners should have been called (none were registered)
      // Verify listen was only called once before failure
      expect(mockListen).toHaveBeenCalledTimes(1);
    });

    it('cleans up all 6 prior listeners when the last (7th) listen() fails', async () => {
      const unlisteners = Array.from({ length: 6 }, () => vi.fn());
      let callIdx = 0;
      mockListen.mockImplementation(async () => {
        if (callIdx < 6) {
          return unlisteners[callIdx++];
        }
        throw new Error('listen failed on call 7');
      });

      const handler = vi.fn();
      await expect(subscribeToClaudeEvents(handler)).rejects.toThrow('listen failed on call 7');

      // All 6 previously-resolved unlisteners must be called
      for (const unsub of unlisteners) {
        expect(unsub).toHaveBeenCalledTimes(1);
      }
    });
  });

  describe('payload validation guards', () => {
    let warnSpy: ReturnType<typeof vi.spyOn>;

    beforeEach(() => {
      warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    });

    afterEach(() => {
      vi.restoreAllMocks();
    });

    const PAYLOAD_EVENTS = [
      'claude-text-delta',
      'claude-thinking-delta',
      'claude-tool-call',
      'claude-tool-result',
      'claude-done',
      'claude-error',
    ] as const;

    const INVALID_PAYLOADS = [
      ['null', null],
      ['undefined', undefined],
      ['string', 'some-string'],
      ['number', 42],
      ['array', [1, 2, 3]],
    ] as const;

    for (const eventName of PAYLOAD_EVENTS) {
      for (const [label, payload] of INVALID_PAYLOADS) {
        it(`drops ${eventName} when payload is ${label}`, async () => {
          const { setup } = captureListener(eventName);
          const handler = vi.fn();
          const listener = await setup(handler);

          listener({ payload });

          expect(handler).not.toHaveBeenCalled();
          expect(warnSpy).toHaveBeenCalledOnce();
          expect(warnSpy).toHaveBeenCalledWith(
            expect.stringContaining(eventName),
            payload,
          );
        });
      }
    }

    it('rejects primitive number payload with "not a plain object" warning', async () => {
      const { setup } = captureListener('claude-text-delta');
      const handler = vi.fn();
      const listener = await setup(handler);

      listener({ payload: 42 });

      expect(handler).not.toHaveBeenCalled();
      expect(warnSpy).toHaveBeenCalledOnce();
      expect(warnSpy).toHaveBeenCalledWith(
        expect.stringContaining('not a plain object'),
        42,
      );
    });

    describe('required string field validation', () => {
      it('drops text_delta when id is a number', async () => {
        const { setup } = captureListener('claude-text-delta');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: { id: 123, content: 'hello' } });
        expect(handler).not.toHaveBeenCalled();
        expect(warnSpy).toHaveBeenCalledOnce();
        expect(warnSpy).toHaveBeenCalledWith(
          expect.stringContaining('claude-text-delta'),
          { id: 123, content: 'hello' },
        );
      });

      it('drops text_delta when content field is missing', async () => {
        const { setup } = captureListener('claude-text-delta');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: { id: 'msg-1' } });
        expect(handler).not.toHaveBeenCalled();
        expect(warnSpy).toHaveBeenCalledOnce();
        expect(warnSpy).toHaveBeenCalledWith(
          expect.stringContaining('claude-text-delta'),
          { id: 'msg-1' },
        );
      });

      it('drops thinking_delta when content is null', async () => {
        const { setup } = captureListener('claude-thinking-delta');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: { id: 'msg-t1', content: null } });
        expect(handler).not.toHaveBeenCalled();
        expect(warnSpy).toHaveBeenCalledOnce();
        expect(warnSpy).toHaveBeenCalledWith(
          expect.stringContaining('claude-thinking-delta'),
          { id: 'msg-t1', content: null },
        );
      });

      it('drops tool_call when tool_name is missing', async () => {
        const { setup } = captureListener('claude-tool-call');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: { id: 'tc1', tool_input: {} } });
        expect(handler).not.toHaveBeenCalled();
        expect(warnSpy).toHaveBeenCalledOnce();
        expect(warnSpy).toHaveBeenCalledWith(
          expect.stringContaining('claude-tool-call'),
          { id: 'tc1', tool_input: {} },
        );
      });

      it('drops tool_result when tool_name is a number', async () => {
        const { setup } = captureListener('claude-tool-result');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: { id: 'tr1', tool_name: 42, result: 'ok' } });
        expect(handler).not.toHaveBeenCalled();
        expect(warnSpy).toHaveBeenCalledOnce();
        expect(warnSpy).toHaveBeenCalledWith(
          expect.stringContaining('claude-tool-result'),
          { id: 'tr1', tool_name: 42, result: 'ok' },
        );
      });

      it('drops done when id is undefined', async () => {
        const { setup } = captureListener('claude-done');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: { id: undefined } });
        expect(handler).not.toHaveBeenCalled();
        expect(warnSpy).toHaveBeenCalledOnce();
        expect(warnSpy).toHaveBeenCalledWith(
          expect.stringContaining('claude-done'),
          { id: undefined },
        );
      });

      it('drops error when message is an array', async () => {
        const { setup } = captureListener('claude-error');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: { id: 'e1', message: ['bad', 'stuff'] } });
        expect(handler).not.toHaveBeenCalled();
        expect(warnSpy).toHaveBeenCalledOnce();
        expect(warnSpy).toHaveBeenCalledWith(
          expect.stringContaining('claude-error'),
          { id: 'e1', message: ['bad', 'stuff'] },
        );
      });

      it('drops error when id is missing', async () => {
        const { setup } = captureListener('claude-error');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: { message: 'rate limit exceeded' } });
        expect(handler).not.toHaveBeenCalled();
        expect(warnSpy).toHaveBeenCalledOnce();
        expect(warnSpy).toHaveBeenCalledWith(
          expect.stringContaining('claude-error'),
          { message: 'rate limit exceeded' },
        );
      });
    });

    describe('tool_input normalization', () => {
      it('normalizes tool_input=null to empty object', async () => {
        const { setup } = captureListener('claude-tool-call');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: { id: 'tc1', tool_use_id: 'tu-tc1', tool_name: 'edit', tool_input: null } });
        expect(handler).toHaveBeenCalledWith({
          type: 'tool_call', id: 'tc1', tool_use_id: 'tu-tc1', tool_name: 'edit', tool_input: {},
        });
        expect(warnSpy).not.toHaveBeenCalled();
      });

      it('normalizes tool_input=[1,2,3] (array) to empty object', async () => {
        const { setup } = captureListener('claude-tool-call');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: { id: 'tc2', tool_use_id: 'tu-tc2', tool_name: 'read', tool_input: [1, 2, 3] } });
        expect(handler).toHaveBeenCalledWith({
          type: 'tool_call', id: 'tc2', tool_use_id: 'tu-tc2', tool_name: 'read', tool_input: {},
        });
        expect(warnSpy).not.toHaveBeenCalled();
      });

      it('normalizes tool_input=undefined to empty object', async () => {
        const { setup } = captureListener('claude-tool-call');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: { id: 'tc3', tool_use_id: 'tu-tc3', tool_name: 'write', tool_input: undefined } });
        expect(handler).toHaveBeenCalledWith({
          type: 'tool_call', id: 'tc3', tool_use_id: 'tu-tc3', tool_name: 'write', tool_input: {},
        });
        expect(warnSpy).not.toHaveBeenCalled();
      });

      it('normalizes tool_input="string" to empty object', async () => {
        const { setup } = captureListener('claude-tool-call');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: { id: 'tc4', tool_use_id: 'tu-tc4', tool_name: 'run', tool_input: 'bad' } });
        expect(handler).toHaveBeenCalledWith({
          type: 'tool_call', id: 'tc4', tool_use_id: 'tu-tc4', tool_name: 'run', tool_input: {},
        });
        expect(warnSpy).not.toHaveBeenCalled();
      });

      it('normalizes tool_input absent (key not present) to empty object', async () => {
        const { setup } = captureListener('claude-tool-call');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: { id: 'tc-absent', tool_use_id: 'tu-tc5', tool_name: 'exec' } });
        expect(handler).toHaveBeenCalledWith({
          type: 'tool_call', id: 'tc-absent', tool_use_id: 'tu-tc5', tool_name: 'exec', tool_input: {},
        });
        expect(warnSpy).not.toHaveBeenCalled();
      });

      it('passes through valid tool_input object unchanged', async () => {
        const { setup } = captureListener('claude-tool-call');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: { id: 'tc5', tool_use_id: 'tu-tc6', tool_name: 'edit', tool_input: { path: '/f' } } });
        expect(handler).toHaveBeenCalledWith({
          type: 'tool_call', id: 'tc5', tool_use_id: 'tu-tc6', tool_name: 'edit', tool_input: { path: '/f' },
        });
        expect(warnSpy).not.toHaveBeenCalled();
      });
    });

    describe('tool_result passthrough contract', () => {
      it.each([
        { label: 'null', id: 'tr-null', payload: { id: 'tr-null', tool_name: 'read_file', result: null }, expected: null },
        { label: 'undefined (absent key)', id: 'tr-undef', payload: { id: 'tr-undef', tool_name: 'read_file' }, expected: undefined },
        { label: '{stdout:"ok"} object', id: 'tr-obj', payload: { id: 'tr-obj', tool_name: 'read_file', result: { stdout: 'ok' } }, expected: { stdout: 'ok' } },
        { label: '["line1","line2"] array', id: 'tr-arr', payload: { id: 'tr-arr', tool_name: 'read_file', result: ['line1', 'line2'] }, expected: ['line1', 'line2'] },
      ])('passes result=$label through to handler', async ({ id, payload, expected }) => {
        const { setup } = captureListener('claude-tool-result');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload });
        const callArg = handler.mock.calls[0][0] as Record<string, unknown>;
        expect(callArg).toEqual({ type: 'tool_result', id, tool_name: 'read_file', result: expected });
        if (expected === undefined) {
          expect('result' in callArg).toBe(true);
        }
      });
    });

    describe('claude-ready bypass', () => {
      it('claude-ready fires unconditionally even with null payload', async () => {
        const { setup } = captureListener('claude-ready');
        const handler = vi.fn();
        const listener = await setup(handler);
        listener({ payload: null as unknown as Record<string, unknown> });
        expect(handler).toHaveBeenCalledWith({ type: 'ready' });
      });
    });
  });
});

// ── Compile-time type assertions ───────────────────────────────────
// ClaudeMessageContext (bridge.ts) must be exactly MessageContext (claudeStore.ts).
// This catches any divergence at compile time — tsc will fail if they differ.
import type { ClaudeMessageContext } from '../bridge';
import type { MessageContext } from '../stores/claudeStore';
import type {
  TextDelta,
  ThinkingDelta,
  ToolCall,
  ToolResult,
  Done,
  ErrorMessage,
  InboundToolResult,
} from '../../sidecar/src/types';

type Equals<A, B> =
  (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false;
type AssertTrue<T extends true> = T;
type _AssertClaudeContextIsMessageContext = AssertTrue<Equals<ClaudeMessageContext, MessageContext>>;

// MESSAGE_CONTEXT_FIELD_MAP must cover every key of MessageContext (compile-time guard).
// If a field is added to MessageContext but not to the map, tsc will fail here.
type _AssertFieldMapExhaustive = AssertTrue<Equals<keyof typeof MESSAGE_CONTEXT_FIELD_MAP, keyof Required<MessageContext>>>;

// MESSAGE_CONTEXT_FIELD_MAP values must be literal string types (not widened to `string`).
// This catches typos in snake_case wire names at compile time.
type FieldMapValues = (typeof MESSAGE_CONTEXT_FIELD_MAP)[keyof typeof MESSAGE_CONTEXT_FIELD_MAP];
type ExpectedWireNames = 'selected_entity' | 'diagnostics' | 'constraints' | 'current_file' | 'attached_contexts';
type _AssertFieldMapValuesLiteral = AssertTrue<Equals<FieldMapValues, ExpectedWireNames>>;

// WireMessageContext must match the expected snake_case shape derived from MessageContext.
// All keys are optional — matching the JSON wire format where absent fields are omitted.
type ExpectedWireShape = {
  selected_entity?: string | undefined;
  diagnostics?: string[] | undefined;
  constraints?: string[] | undefined;
  current_file?: string | undefined;
  attached_contexts?: string[] | undefined;
};
type _AssertWireMessageContextShape = AssertTrue<Equals<WireMessageContext, ExpectedWireShape>>;
// Guard: WireMessageContext must be identical to its own Partial (i.e., all keys already optional).
type _AssertWireMessageContextIsPartial = AssertTrue<Equals<WireMessageContext, Partial<WireMessageContext>>>;

// mapContextToWire must return WireMessageContext (not Record<string, unknown>).
type _AssertMapReturnType = AssertTrue<Equals<ReturnType<typeof mapContextToWire>, WireMessageContext>>;

// Each Omit<Interface, 'type'> must match the payload shape used in subscribeToClaudeEvents.
// If a field is added/removed/renamed in types.ts, tsc will fail here.
type _AssertTextDeltaPayload = AssertTrue<Equals<Omit<TextDelta, 'type'>, { id: string; content: string }>>;
type _AssertThinkingDeltaPayload = AssertTrue<Equals<Omit<ThinkingDelta, 'type'>, { id: string; content: string }>>;
type _AssertToolCallPayload = AssertTrue<Equals<Omit<ToolCall, 'type'>, { id: string; tool_use_id: string; tool_name: string; tool_input: Record<string, unknown> }>>;
type _AssertInboundToolResultPayload = AssertTrue<Equals<Omit<InboundToolResult, 'type'>, { id: string; tool_use_id: string; tool_name: string; result: unknown }>>;
type _AssertToolResultPayload = AssertTrue<Equals<Omit<ToolResult, 'type'>, { id: string; tool_name: string; result: unknown }>>;
type _AssertDonePayload = AssertTrue<Equals<Omit<Done, 'type'>, { id: string }>>;
type _AssertErrorMessagePayload = AssertTrue<Equals<Omit<ErrorMessage, 'type'>, { id: string; message: string }>>;

// EventEntry's payload type is `unknown` (not `Record<string, unknown>`) because
// each handler validates event.payload via validatePayload() before accessing fields.
// `unknown` prevents accidental uncast property access and doesn't falsely
// constrain the payload to be an object with string keys.
