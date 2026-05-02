import { describe, it, expect, vi, beforeEach, afterEach, afterAll } from 'vitest';
import type { OutboundMessage } from '../../sidecar/src/types';

// Polyfill rAF for test environment (claudeStore uses it for delta batching)
let rafCallbacks = new Map<number, () => void>();
let nextRafId = 1;
const origRAF = globalThis.requestAnimationFrame;
const origCancelRAF = globalThis.cancelAnimationFrame;
globalThis.requestAnimationFrame = (cb: FrameRequestCallback) => {
  const id = nextRafId++;
  rafCallbacks.set(id, () => cb(performance.now()));
  return id;
};
globalThis.cancelAnimationFrame = (id: number) => {
  rafCallbacks.delete(id);
};

/** Drain and invoke all pending rAF callbacks (exercises the batching path). */
function flushRaf() {
  const batch = [...rafCallbacks.values()];
  rafCallbacks.clear();
  batch.forEach((cb) => cb());
}

// Mock Tauri API modules (must be before imports that use them)
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { createClaudeStore, type AssistantMessage } from '../stores/claudeStore';
import {
  claudeSendMessage,
  claudeAbort,
  subscribeToClaudeEvents,
} from '../bridge';

const mockInvoke = vi.mocked(invoke);
const mockListen = vi.mocked(listen);

beforeEach(() => {
  vi.clearAllMocks();
  rafCallbacks.clear();
  nextRafId = 1;
});

afterEach(() => {
  rafCallbacks.clear();
});

afterAll(() => {
  // Restore the original globals that were overwritten at module load time
  if (origRAF) globalThis.requestAnimationFrame = origRAF;
  if (origCancelRAF) globalThis.cancelAnimationFrame = origCancelRAF;
});

describe('claude bridge integration', () => {
  it('store.sendMessage triggers invoke("claude_send_message") with correct args', async () => {
    mockInvoke.mockResolvedValue(undefined);

    const store = createClaudeStore({
      onSend: (id, text, context) => {
        claudeSendMessage(text, {
          selectedEntity: context.selectedEntity,
          diagnostics: context.diagnostics,
          constraints: context.constraints,
        }).catch(console.error);
      },
      onAbort: () => {
        claudeAbort().catch(console.error);
      },
    });

    store.sendMessage('fix the bracket', {
      selectedEntity: 'Bracket.body',
      diagnostics: ['error: undefined reference'],
    });

    // Allow microtask for invoke to be called
    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('claude_send_message', {
        text: 'fix the bracket',
        context: {
          selected_entity: 'Bracket.body',
          diagnostics: ['error: undefined reference'],
          constraints: undefined,
        },
      });
    });
  });

  it('store.claudeAbort triggers invoke("claude_abort")', async () => {
    mockInvoke.mockResolvedValue(undefined);

    const store = createClaudeStore({
      onSend: () => {},
      onAbort: () => {
        claudeAbort().catch(console.error);
      },
    });

    store.claudeAbort();

    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('claude_abort');
    });
  });

  it('claude events flow through subscribeToClaudeEvents into store state', async () => {
    // Capture event handlers by event name
    const capturedHandlers: Record<string, (event: { payload: unknown }) => void> = {};
    mockListen.mockImplementation(async (eventName, handler) => {
      capturedHandlers[eventName as string] = handler as (event: { payload: unknown }) => void;
      return vi.fn();
    });

    const store = createClaudeStore({
      onSend: () => {},
      onAbort: () => {},
    });

    // Wire up events
    await subscribeToClaudeEvents(store.handleOutboundMessage);

    // First, send a message so there's a current message ID
    store.sendMessage('hello', {});
    const msgId = store.state.currentMessageId!;
    expect(msgId).toBeTruthy();

    // Simulate claude-text-delta event
    capturedHandlers['claude-text-delta']({
      payload: { id: msgId, content: 'Here is my response' },
    });

    // Flush rAF to exercise the batching path — responseText should already be populated
    // before the done event fires (the done event's cancelAndFlush is a fallback, not the
    // primary mechanism for applying buffered text).
    flushRaf();
    const midMsg = store.state.messages.find(
      (m): m is AssistantMessage => m.role === 'assistant' && m.id === msgId,
    );
    expect(midMsg!.responseText).toBe('Here is my response');
    expect(midMsg!.complete).toBe(false);

    // Simulate claude-done event (marks message complete, flushes any remaining buffers)
    capturedHandlers['claude-done']({
      payload: { id: msgId },
    });

    // Check final store state reflects the events
    const assistantMsg = store.state.messages.find(
      (m): m is AssistantMessage => m.role === 'assistant' && m.id === msgId,
    );
    expect(assistantMsg).toBeTruthy();
    expect(assistantMsg!.responseText).toBe('Here is my response');
    expect(assistantMsg!.complete).toBe(true);
    expect(store.state.sessionStatus).toBe('idle');
  });

  it('text-delta events update responseText via rAF flush without done event', async () => {
    const capturedHandlers: Record<string, (event: { payload: unknown }) => void> = {};
    mockListen.mockImplementation(async (eventName, handler) => {
      capturedHandlers[eventName as string] = handler as (event: { payload: unknown }) => void;
      return vi.fn();
    });

    const store = createClaudeStore({
      onSend: () => {},
      onAbort: () => {},
    });

    await subscribeToClaudeEvents(store.handleOutboundMessage);

    store.sendMessage('hello', {});
    const msgId = store.state.currentMessageId!;
    expect(msgId).toBeTruthy();

    // Send multiple text deltas (exercises the rAF batching path)
    capturedHandlers['claude-text-delta']({
      payload: { id: msgId, content: 'Hello ' },
    });
    capturedHandlers['claude-text-delta']({
      payload: { id: msgId, content: 'world' },
    });

    // Before flushing rAF, text should still be buffered (not yet applied)
    const beforeFlush = store.state.messages.find(
      (m): m is AssistantMessage => m.role === 'assistant' && m.id === msgId,
    );
    expect(beforeFlush!.responseText).toBe('');

    // Flush rAF — this exercises the batching path without relying on done/cancelAndFlush
    flushRaf();

    // Now responseText should reflect the batched deltas
    const afterFlush = store.state.messages.find(
      (m): m is AssistantMessage => m.role === 'assistant' && m.id === msgId,
    );
    expect(afterFlush!.responseText).toBe('Hello world');
    // Message should NOT be marked complete (no done event was sent)
    expect(afterFlush!.complete).toBe(false);
    expect(store.state.sessionStatus).toBe('responding');
  });

  it('thinking-delta events update thinkingText via rAF flush without done event', async () => {
    const capturedHandlers: Record<string, (event: { payload: unknown }) => void> = {};
    mockListen.mockImplementation(async (eventName, handler) => {
      capturedHandlers[eventName as string] = handler as (event: { payload: unknown }) => void;
      return vi.fn();
    });

    const store = createClaudeStore({
      onSend: () => {},
      onAbort: () => {},
    });

    await subscribeToClaudeEvents(store.handleOutboundMessage);

    store.sendMessage('hello', {});
    const msgId = store.state.currentMessageId!;
    expect(msgId).toBeTruthy();

    // Send multiple thinking deltas (exercises the rAF batching path for thinkingBuffer)
    capturedHandlers['claude-thinking-delta']({
      payload: { id: msgId, content: 'Let me ' },
    });
    capturedHandlers['claude-thinking-delta']({
      payload: { id: msgId, content: 'think...' },
    });

    // Before flushing rAF, thinkingText should still be buffered (not yet applied)
    const beforeFlush = store.state.messages.find(
      (m): m is AssistantMessage => m.role === 'assistant' && m.id === msgId,
    );
    expect(beforeFlush!.thinkingText).toBe('');

    // Flush rAF — this exercises the thinkingBuffer batching path
    flushRaf();

    // Now thinkingText should reflect the batched deltas
    const afterFlush = store.state.messages.find(
      (m): m is AssistantMessage => m.role === 'assistant' && m.id === msgId,
    );
    expect(afterFlush!.thinkingText).toBe('Let me think...');
    // responseText should remain empty (buffer isolation)
    expect(afterFlush!.responseText).toBe('');
    // Message should NOT be marked complete (no done event was sent)
    expect(afterFlush!.complete).toBe(false);
    // thinkingComplete should still be false
    expect(afterFlush!.thinkingComplete).toBe(false);
    // Status should be 'thinking' (not 'responding')
    expect(store.state.sessionStatus).toBe('thinking');
  });

  it('cancelAnimationFrame polyfill removes pending callback by id', () => {
    const spy = vi.fn();
    const id = requestAnimationFrame(spy);
    cancelAnimationFrame(id);
    flushRaf();
    expect(spy).not.toHaveBeenCalled();
  });

  it('done event cancelAndFlush prevents stale rAF callback from double-flushing', async () => {
    const capturedHandlers: Record<string, (event: { payload: unknown }) => void> = {};
    mockListen.mockImplementation(async (eventName, handler) => {
      capturedHandlers[eventName as string] = handler as (event: { payload: unknown }) => void;
      return vi.fn();
    });

    const store = createClaudeStore({
      onSend: () => {},
      onAbort: () => {},
    });

    await subscribeToClaudeEvents(store.handleOutboundMessage);

    // Send a message so there's a current message ID
    store.sendMessage('hello', {});
    const msgId = store.state.currentMessageId!;
    expect(msgId).toBeTruthy();

    // Simulate text_delta (schedules rAF internally)
    capturedHandlers['claude-text-delta']({
      payload: { id: msgId, content: 'Some text' },
    });

    // Simulate done event — calls cancelAndFlush internally (cancels rAF + flushes buffers)
    capturedHandlers['claude-done']({
      payload: { id: msgId },
    });

    // The canceled callback should have been removed from the polyfill Map
    expect(rafCallbacks.size).toBe(0);

    // Capture the text after done's flush
    const afterDone = store.state.messages.find(
      (m): m is AssistantMessage => m.role === 'assistant' && m.id === msgId,
    );
    const textAfterDone = afterDone!.responseText;

    // A second flushRaf() should be a no-op — the stale callback was canceled
    flushRaf();

    const afterSecondFlush = store.state.messages.find(
      (m): m is AssistantMessage => m.role === 'assistant' && m.id === msgId,
    );
    expect(afterSecondFlush!.responseText).toBe(textAfterDone);
  });

  it('claude-error event adds system message and marks assistant message as error', async () => {
    const capturedHandlers: Record<string, (event: { payload: unknown }) => void> = {};
    mockListen.mockImplementation(async (eventName, handler) => {
      capturedHandlers[eventName as string] = handler as (event: { payload: unknown }) => void;
      return vi.fn();
    });

    const store = createClaudeStore({
      onSend: () => {},
      onAbort: () => {},
    });

    await subscribeToClaudeEvents(store.handleOutboundMessage);

    store.sendMessage('hello', {});
    const msgId = store.state.currentMessageId!;

    // Simulate claude-error event
    capturedHandlers['claude-error']({
      payload: { id: msgId, message: 'API key invalid' },
    });

    const assistantMsg = store.state.messages.find(
      (m): m is AssistantMessage => m.role === 'assistant' && m.id === msgId,
    );
    expect(assistantMsg!.error).toBe('API key invalid');
    expect(assistantMsg!.complete).toBe(true);
    expect(store.state.sessionStatus).toBe('idle');

    // Should also have a system message from error classification
    const systemMsg = store.state.messages.find((m) => m.role === 'system');
    expect(systemMsg).toBeTruthy();
  });

  it('claude-notice event preserves in-flight turn (non-terminal)', async () => {
    const capturedHandlers: Record<string, (event: { payload: unknown }) => void> = {};
    mockListen.mockImplementation(async (eventName, handler) => {
      capturedHandlers[eventName as string] = handler as (event: { payload: unknown }) => void;
      return vi.fn();
    });

    const store = createClaudeStore({
      onSend: () => {},
      onAbort: () => {},
    });

    await subscribeToClaudeEvents(store.handleOutboundMessage);

    store.sendMessage('hello', {});
    const msgId = store.state.currentMessageId!;

    // Receive a partial text_delta first
    capturedHandlers['claude-text-delta']({ payload: { id: msgId, content: 'partial ' } });
    flushRaf();

    // Receive the notice — must NOT terminate the turn or mutate state
    const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    capturedHandlers['claude-notice']({
      payload: { id: msgId, code: 'degraded_turn_boundary', message: 'detail...' },
    });
    consoleSpy.mockRestore();

    // Subsequent deltas continue to land on the SAME in-flight assistant message
    capturedHandlers['claude-text-delta']({ payload: { id: msgId, content: 'more text' } });
    flushRaf();

    const inFlight = store.state.messages.find(
      (m): m is AssistantMessage => m.role === 'assistant' && m.id === msgId,
    );
    expect(inFlight!.responseText).toBe('partial more text'); // deltas accumulated past the notice
    expect(inFlight!.complete).toBe(false); // not terminated
    expect(inFlight!.error).toBeUndefined(); // not errored
    expect(store.state.sessionStatus).not.toBe('idle'); // session still active
    // No system message added — host's notice handler does NOT addSystemMessage
    const systemMsgs = store.state.messages.filter((m) => m.role === 'system');
    expect(systemMsgs).toHaveLength(0);

    // Then claude-done finishes the turn cleanly
    capturedHandlers['claude-done']({ payload: { id: msgId } });
    flushRaf();
    expect(store.state.sessionStatus).toBe('idle');
    const final = store.state.messages.find(
      (m): m is AssistantMessage => m.role === 'assistant' && m.id === msgId,
    );
    expect(final!.complete).toBe(true);
    expect(final!.error).toBeUndefined(); // turn completed cleanly, not via error path
  });
});
