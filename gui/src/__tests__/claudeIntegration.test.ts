import { describe, it, expect, vi, beforeEach, afterEach, afterAll } from 'vitest';
import type { OutboundMessage } from '../../sidecar/src/types';

// Polyfill rAF for test environment (claudeStore uses it for delta batching)
let rafCallbacks: Array<() => void> = [];
const origRAF = globalThis.requestAnimationFrame;
const origCancelRAF = globalThis.cancelAnimationFrame;
globalThis.requestAnimationFrame = (cb: FrameRequestCallback) => {
  const id = rafCallbacks.length + 1;
  rafCallbacks.push(() => cb(performance.now()));
  return id;
};
globalThis.cancelAnimationFrame = () => {};

/** Drain and invoke all pending rAF callbacks (exercises the batching path). */
function flushRaf() {
  const batch = rafCallbacks.splice(0);
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
  rafCallbacks = [];
});

afterEach(() => {
  rafCallbacks = [];
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

    // Simulate claude-done event (flushes buffers)
    capturedHandlers['claude-done']({
      payload: { id: msgId },
    });

    // Check store state reflects the events
    const assistantMsg = store.state.messages.find(
      (m): m is AssistantMessage => m.role === 'assistant' && m.id === msgId,
    );
    expect(assistantMsg).toBeTruthy();
    expect(assistantMsg!.responseText).toBe('Here is my response');
    expect(assistantMsg!.complete).toBe(true);
    expect(store.state.sessionStatus).toBe('idle');
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
});
