import { describe, it, expect, vi, beforeEach } from 'vitest';

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
} from '../bridge';

const mockInvoke = vi.mocked(invoke);
const mockListen = vi.mocked(listen);

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

  it('claudeSendMessage passes undefined fields correctly', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await claudeSendMessage('hello', {
      selectedEntity: 'Bracket.w',
    });

    expect(mockInvoke).toHaveBeenCalledWith('claude_send_message', {
      text: 'hello',
      context: {
        selected_entity: 'Bracket.w',
        diagnostics: undefined,
        constraints: undefined,
      },
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
    let capturedHandler: ((event: { payload: unknown }) => void) | undefined;
    mockListen.mockImplementation(async (eventName, handler) => {
      if (eventName === 'claude-text-delta') {
        capturedHandler = handler as (event: { payload: unknown }) => void;
      }
      return vi.fn();
    });

    const handler = vi.fn();
    await subscribeToClaudeEvents(handler);

    capturedHandler!({ payload: { id: 'msg-1', content: 'Hello' } });

    expect(handler).toHaveBeenCalledWith({
      type: 'text_delta',
      id: 'msg-1',
      content: 'Hello',
    });
  });

  it('maps claude-tool-call event to OutboundMessage { type: "tool_call" }', async () => {
    let capturedHandler: ((event: { payload: unknown }) => void) | undefined;
    mockListen.mockImplementation(async (eventName, handler) => {
      if (eventName === 'claude-tool-call') {
        capturedHandler = handler as (event: { payload: unknown }) => void;
      }
      return vi.fn();
    });

    const handler = vi.fn();
    await subscribeToClaudeEvents(handler);

    capturedHandler!({
      payload: { id: 'msg-2', tool_name: 'edit_file', tool_input: { path: 'main.ri' } },
    });

    expect(handler).toHaveBeenCalledWith({
      type: 'tool_call',
      id: 'msg-2',
      tool_name: 'edit_file',
      tool_input: { path: 'main.ri' },
    });
  });

  it('maps claude-ready event to OutboundMessage { type: "ready" }', async () => {
    let capturedHandler: ((event: { payload: unknown }) => void) | undefined;
    mockListen.mockImplementation(async (eventName, handler) => {
      if (eventName === 'claude-ready') {
        capturedHandler = handler as (event: { payload: unknown }) => void;
      }
      return vi.fn();
    });

    const handler = vi.fn();
    await subscribeToClaudeEvents(handler);

    capturedHandler!({ payload: {} });

    expect(handler).toHaveBeenCalledWith({ type: 'ready' });
  });

  it('maps claude-done event to OutboundMessage { type: "done" }', async () => {
    let capturedHandler: ((event: { payload: unknown }) => void) | undefined;
    mockListen.mockImplementation(async (eventName, handler) => {
      if (eventName === 'claude-done') {
        capturedHandler = handler as (event: { payload: unknown }) => void;
      }
      return vi.fn();
    });

    const handler = vi.fn();
    await subscribeToClaudeEvents(handler);

    capturedHandler!({ payload: { id: 'msg-3' } });

    expect(handler).toHaveBeenCalledWith({ type: 'done', id: 'msg-3' });
  });

  it('maps claude-error event to OutboundMessage { type: "error" }', async () => {
    let capturedHandler: ((event: { payload: unknown }) => void) | undefined;
    mockListen.mockImplementation(async (eventName, handler) => {
      if (eventName === 'claude-error') {
        capturedHandler = handler as (event: { payload: unknown }) => void;
      }
      return vi.fn();
    });

    const handler = vi.fn();
    await subscribeToClaudeEvents(handler);

    capturedHandler!({ payload: { id: 'msg-4', message: 'rate limit exceeded' } });

    expect(handler).toHaveBeenCalledWith({
      type: 'error',
      id: 'msg-4',
      message: 'rate limit exceeded',
    });
  });

  it('maps claude-thinking-delta event to OutboundMessage { type: "thinking_delta" }', async () => {
    let capturedHandler: ((event: { payload: unknown }) => void) | undefined;
    mockListen.mockImplementation(async (eventName, handler) => {
      if (eventName === 'claude-thinking-delta') {
        capturedHandler = handler as (event: { payload: unknown }) => void;
      }
      return vi.fn();
    });

    const handler = vi.fn();
    await subscribeToClaudeEvents(handler);

    capturedHandler!({ payload: { id: 'msg-t1', content: 'Let me think...' } });

    expect(handler).toHaveBeenCalledWith({
      type: 'thinking_delta',
      id: 'msg-t1',
      content: 'Let me think...',
    });
  });

  it('maps claude-tool-result event to OutboundMessage { type: "tool_result" }', async () => {
    let capturedHandler: ((event: { payload: unknown }) => void) | undefined;
    mockListen.mockImplementation(async (eventName, handler) => {
      if (eventName === 'claude-tool-result') {
        capturedHandler = handler as (event: { payload: unknown }) => void;
      }
      return vi.fn();
    });

    const handler = vi.fn();
    await subscribeToClaudeEvents(handler);

    capturedHandler!({
      payload: { id: 'msg-tr1', tool_name: 'read_file', result: 'file contents here' },
    });

    expect(handler).toHaveBeenCalledWith({
      type: 'tool_result',
      id: 'msg-tr1',
      tool_name: 'read_file',
      result: 'file contents here',
    });
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
});
