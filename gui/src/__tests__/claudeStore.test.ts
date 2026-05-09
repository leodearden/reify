import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createClaudeStore } from '../stores/claudeStore';
import type { OutboundMessage } from '../../sidecar/src/types';

describe('claudeStore', () => {
  function makeStore(overrides?: {
    onSend?: ReturnType<typeof vi.fn>;
    onAbort?: ReturnType<typeof vi.fn>;
    onPermissionDecision?: ReturnType<typeof vi.fn>;
  }) {
    const onSend = overrides?.onSend ?? vi.fn();
    const onAbort = overrides?.onAbort ?? vi.fn();
    const onPermissionDecision = overrides?.onPermissionDecision ?? vi.fn();
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return { ...createClaudeStore({ onSend, onAbort, onPermissionDecision } as any), onSend, onAbort, onPermissionDecision };
  }

  describe('initial state', () => {
    it('has sessionStatus="idle"', () => {
      const { state } = makeStore();
      expect(state.sessionStatus).toBe('idle');
    });

    it('has empty messages array', () => {
      const { state } = makeStore();
      expect(state.messages).toEqual([]);
    });

    it('has currentMessageId=null', () => {
      const { state } = makeStore();
      expect(state.currentMessageId).toBeNull();
    });
  });

  describe('handleOutboundMessage', () => {
    it('Ready event keeps idle state', () => {
      const { state, handleOutboundMessage } = makeStore();
      handleOutboundMessage({ type: 'ready' } as OutboundMessage);
      expect(state.sessionStatus).toBe('idle');
    });

    it('TextDelta accumulates responseText on current assistant message', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      handleOutboundMessage({ type: 'text_delta', id: state.currentMessageId!, content: 'Hello' } as OutboundMessage);
      // Flush rAF
      handleOutboundMessage({ type: 'done', id: state.currentMessageId! } as OutboundMessage);
      const assistantMsg = state.messages.find((m) => m.role === 'assistant');
      expect(assistantMsg).toBeTruthy();
      expect(assistantMsg!.responseText).toBe('Hello');
    });

    it('TextDelta sets sessionStatus to "responding"', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      handleOutboundMessage({ type: 'text_delta', id: state.currentMessageId!, content: 'Hi' } as OutboundMessage);
      expect(state.sessionStatus).toBe('responding');
    });

    it('ThinkingDelta accumulates thinkingText', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      handleOutboundMessage({ type: 'thinking_delta', id: state.currentMessageId!, content: 'Let me think...' } as OutboundMessage);
      // Flush via done
      handleOutboundMessage({ type: 'done', id: state.currentMessageId! } as OutboundMessage);
      const assistantMsg = state.messages.find((m) => m.role === 'assistant');
      expect(assistantMsg!.thinkingText).toBe('Let me think...');
    });

    it('ThinkingDelta sets sessionStatus to "thinking"', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      handleOutboundMessage({ type: 'thinking_delta', id: state.currentMessageId!, content: 'hmm' } as OutboundMessage);
      expect(state.sessionStatus).toBe('thinking');
    });

    it('ToolCall adds ToolCallInfo with status="pending"', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      handleOutboundMessage({
        type: 'tool_call',
        id: msgId,
        tool_use_id: 'tuid-1',
        tool_name: 'reify_get_parameters',
        tool_input: { entity: 'box1' },
      } as OutboundMessage);
      const assistantMsg = state.messages.find((m) => m.role === 'assistant');
      expect(assistantMsg!.toolCalls).toHaveLength(1);
      expect(assistantMsg!.toolCalls[0].toolName).toBe('reify_get_parameters');
      expect(assistantMsg!.toolCalls[0].status).toBe('pending');
    });

    it('ToolCall sets sessionStatus to "tool-calling"', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      handleOutboundMessage({
        type: 'tool_call',
        id: state.currentMessageId!,
        tool_use_id: 'tuid-1',
        tool_name: 'reify_get_parameters',
        tool_input: {},
      } as OutboundMessage);
      expect(state.sessionStatus).toBe('tool-calling');
    });

    it('ToolResult updates matching tool call status to "complete" and stores result', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      handleOutboundMessage({
        type: 'tool_call',
        id: msgId,
        tool_use_id: 'tuid-1',
        tool_name: 'reify_get_parameters',
        tool_input: {},
      } as OutboundMessage);
      handleOutboundMessage({
        type: 'tool_result',
        id: msgId,
        tool_name: 'reify_get_parameters',
        result: [{ name: 'width', value: 10 }],
      } as OutboundMessage);
      const assistantMsg = state.messages.find((m) => m.role === 'assistant');
      expect(assistantMsg!.toolCalls[0].status).toBe('complete');
      expect(assistantMsg!.toolCalls[0].result).toEqual([{ name: 'width', value: 10 }]);
    });

    it('Done marks assistant message complete and sets sessionStatus="idle"', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      handleOutboundMessage({ type: 'text_delta', id: msgId, content: 'Hi' } as OutboundMessage);
      handleOutboundMessage({ type: 'done', id: msgId } as OutboundMessage);
      const assistantMsg = state.messages.find((m) => m.role === 'assistant');
      expect(assistantMsg!.complete).toBe(true);
      expect(state.sessionStatus).toBe('idle');
    });

    it('ErrorMessage sets error on assistant message and sets sessionStatus="idle"', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      handleOutboundMessage({
        type: 'error',
        id: msgId,
        message: 'Rate limit exceeded',
      } as OutboundMessage);
      const assistantMsg = state.messages.find((m) => m.role === 'assistant');
      expect(assistantMsg!.error).toBe('Rate limit exceeded');
      expect(state.sessionStatus).toBe('idle');
    });

    it('error while mid-thinking sets BOTH complete and thinkingComplete to true (regression: stuck throbber)', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      // Simulate being mid-thinking when error fires
      handleOutboundMessage({ type: 'thinking_delta', id: msgId, content: 'Let me think...' } as OutboundMessage);
      handleOutboundMessage({
        type: 'error',
        id: msgId,
        message: 'Something went wrong',
      } as OutboundMessage);
      const assistantMsg = state.messages.find((m) => m.role === 'assistant') as any;
      expect(assistantMsg.complete).toBe(true);
      expect(assistantMsg.thinkingComplete).toBe(true);
    });
  });

  describe('sendMessage', () => {
    it('adds a user ChatMessage to messages', () => {
      const { state, sendMessage } = makeStore();
      sendMessage('hello world', {});
      const userMsg = state.messages.find((m) => m.role === 'user');
      expect(userMsg).toBeTruthy();
      expect(userMsg!.text).toBe('hello world');
    });

    it('sets currentMessageId', () => {
      const { state, sendMessage } = makeStore();
      sendMessage('hello', {});
      expect(state.currentMessageId).not.toBeNull();
    });

    it('calls onSend callback with message text and context', () => {
      const onSend = vi.fn();
      const { sendMessage } = makeStore({ onSend });
      const ctx = { selectedEntity: 'box1' };
      sendMessage('hello', ctx);
      expect(onSend).toHaveBeenCalledTimes(1);
      expect(onSend).toHaveBeenCalledWith(expect.any(String), 'hello', ctx);
    });

    it('creates an assistant message shell after user message', () => {
      const { state, sendMessage } = makeStore();
      sendMessage('hello', {});
      expect(state.messages).toHaveLength(2);
      expect(state.messages[0].role).toBe('user');
      expect(state.messages[1].role).toBe('assistant');
    });
  });

  describe('claudeAbort', () => {
    it('calls onAbort callback', () => {
      const onAbort = vi.fn();
      const { claudeAbort } = makeStore({ onAbort });
      claudeAbort();
      expect(onAbort).toHaveBeenCalledTimes(1);
    });

    it('sets sessionStatus to "idle"', () => {
      const { state, sendMessage, claudeAbort } = makeStore();
      sendMessage('hello', {});
      claudeAbort();
      expect(state.sessionStatus).toBe('idle');
    });

    it('marks in-flight assistant message complete and thinkingComplete on abort (regression: stuck throbber/cursor)', () => {
      const { state, sendMessage, handleOutboundMessage, claudeAbort } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      // Simulate mid-thinking when abort fires
      handleOutboundMessage({ type: 'thinking_delta', id: msgId, content: 'thinking...' } as OutboundMessage);
      claudeAbort();
      const assistantMsg = state.messages.find((m) => m.role === 'assistant') as any;
      expect(assistantMsg.complete).toBe(true);
      expect(assistantMsg.thinkingComplete).toBe(true);
    });

    it('claudeAbort with no in-flight message is a safe no-op (does not throw, leaves messages unchanged)', () => {
      const { state, claudeAbort } = makeStore();
      // No sendMessage called — currentMessageId is null
      expect(() => claudeAbort()).not.toThrow();
      expect(state.messages).toHaveLength(0);
    });
  });

  describe('clearSession', () => {
    it('resets messages to empty array', () => {
      const { state, sendMessage, clearSession } = makeStore();
      sendMessage('hello', {});
      expect(state.messages.length).toBeGreaterThan(0);
      clearSession();
      expect(state.messages).toEqual([]);
    });
  });

  describe('tool_result FIFO matching', () => {
    it('resolves sequential same-tool calls in order', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;

      // Two tool_call events with the same tool_name but distinct tool_use_ids
      handleOutboundMessage({
        type: 'tool_call',
        id: msgId,
        tool_use_id: 'tuid-1',
        tool_name: 'reify_get_parameters',
        tool_input: { entity: 'box1' },
      } as OutboundMessage);
      handleOutboundMessage({
        type: 'tool_call',
        id: msgId,
        tool_use_id: 'tuid-2',
        tool_name: 'reify_get_parameters',
        tool_input: { entity: 'box2' },
      } as OutboundMessage);

      // Two tool_result events with the same tool_name but different results
      handleOutboundMessage({
        type: 'tool_result',
        id: msgId,
        tool_name: 'reify_get_parameters',
        result: [{ name: 'width', value: 10 }],
      } as OutboundMessage);
      handleOutboundMessage({
        type: 'tool_result',
        id: msgId,
        tool_name: 'reify_get_parameters',
        result: [{ name: 'height', value: 20 }],
      } as OutboundMessage);

      const assistantMsg = state.messages.find((m) => m.role === 'assistant') as any;
      expect(assistantMsg.toolCalls[0].status).toBe('complete');
      expect(assistantMsg.toolCalls[0].result).toEqual([{ name: 'width', value: 10 }]);
      expect(assistantMsg.toolCalls[1].status).toBe('complete');
      expect(assistantMsg.toolCalls[1].result).toEqual([{ name: 'height', value: 20 }]);
    });

    it('does not re-update an already-completed tool call', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;

      handleOutboundMessage({
        type: 'tool_call',
        id: msgId,
        tool_use_id: 'tuid-1',
        tool_name: 'reify_get_parameters',
        tool_input: { entity: 'box1' },
      } as OutboundMessage);

      // Complete it
      handleOutboundMessage({
        type: 'tool_result',
        id: msgId,
        tool_name: 'reify_get_parameters',
        result: 'first result',
      } as OutboundMessage);

      // Send another result with same tool_name — should not overwrite
      handleOutboundMessage({
        type: 'tool_result',
        id: msgId,
        tool_name: 'reify_get_parameters',
        result: 'second result',
      } as OutboundMessage);

      const assistantMsg = state.messages.find((m) => m.role === 'assistant') as any;
      expect(assistantMsg.toolCalls[0].result).toBe('first result');
    });

    it('tool calls have locally-unique IDs', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;

      handleOutboundMessage({
        type: 'tool_call',
        id: msgId,
        tool_use_id: 'tuid-1',
        tool_name: 'reify_get_parameters',
        tool_input: {},
      } as OutboundMessage);
      handleOutboundMessage({
        type: 'tool_call',
        id: msgId,
        tool_use_id: 'tuid-2',
        tool_name: 'reify_get_parameters',
        tool_input: {},
      } as OutboundMessage);

      const assistantMsg = state.messages.find((m) => m.role === 'assistant') as any;
      expect(assistantMsg.toolCalls[0].id).not.toBe(assistantMsg.toolCalls[1].id);
    });

    it('mixed tool names still match correctly', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;

      // Send: A, B, A — each with a unique tool_use_id
      handleOutboundMessage({
        type: 'tool_call',
        id: msgId,
        tool_use_id: 'tuid-1',
        tool_name: 'reify_get_parameters',
        tool_input: { entity: 'box1' },
      } as OutboundMessage);
      handleOutboundMessage({
        type: 'tool_call',
        id: msgId,
        tool_use_id: 'tuid-2',
        tool_name: 'reify_update_source',
        tool_input: { code: 'x' },
      } as OutboundMessage);
      handleOutboundMessage({
        type: 'tool_call',
        id: msgId,
        tool_use_id: 'tuid-3',
        tool_name: 'reify_get_parameters',
        tool_input: { entity: 'box2' },
      } as OutboundMessage);

      // Results: B, A, A
      handleOutboundMessage({
        type: 'tool_result',
        id: msgId,
        tool_name: 'reify_update_source',
        result: 'source updated',
      } as OutboundMessage);
      handleOutboundMessage({
        type: 'tool_result',
        id: msgId,
        tool_name: 'reify_get_parameters',
        result: 'params-box1',
      } as OutboundMessage);
      handleOutboundMessage({
        type: 'tool_result',
        id: msgId,
        tool_name: 'reify_get_parameters',
        result: 'params-box2',
      } as OutboundMessage);

      const assistantMsg = state.messages.find((m) => m.role === 'assistant') as any;
      // toolCalls[0] = get_parameters (box1) → params-box1
      expect(assistantMsg.toolCalls[0].result).toBe('params-box1');
      // toolCalls[1] = update_source → source updated
      expect(assistantMsg.toolCalls[1].result).toBe('source updated');
      // toolCalls[2] = get_parameters (box2) → params-box2
      expect(assistantMsg.toolCalls[2].result).toBe('params-box2');
      // All complete
      expect(assistantMsg.toolCalls.every((tc: any) => tc.status === 'complete')).toBe(true);
    });
  });

  describe('rAF delta batching', () => {
    let rafCallbacks: Array<() => void>;
    let origRAF: typeof globalThis.requestAnimationFrame;
    let origCancelRAF: typeof globalThis.cancelAnimationFrame;

    beforeEach(() => {
      rafCallbacks = [];
      origRAF = globalThis.requestAnimationFrame;
      origCancelRAF = globalThis.cancelAnimationFrame;
      globalThis.requestAnimationFrame = (cb: FrameRequestCallback) => {
        const id = rafCallbacks.length + 1;
        rafCallbacks.push(() => cb(performance.now()));
        return id;
      };
      globalThis.cancelAnimationFrame = (_id: number) => {
        // For simplicity, we just let cancelAndFlush handle this
      };
    });

    afterEach(() => {
      globalThis.requestAnimationFrame = origRAF;
      globalThis.cancelAnimationFrame = origCancelRAF;
    });

    it('buffers multiple text_delta events and flushes on rAF', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;

      // Send 10 text deltas synchronously
      for (let i = 0; i < 10; i++) {
        handleOutboundMessage({ type: 'text_delta', id: msgId, content: `w${i}` } as OutboundMessage);
      }

      // Before rAF fires, responseText should still be empty
      const assistantBefore = state.messages.find((m) => m.role === 'assistant');
      expect(assistantBefore!.responseText).toBe('');

      // Fire the rAF callback
      rafCallbacks[0]();

      // Now all 10 deltas should be concatenated
      const assistantAfter = state.messages.find((m) => m.role === 'assistant');
      expect(assistantAfter!.responseText).toBe('w0w1w2w3w4w5w6w7w8w9');
    });

    it('flushes immediately on done event without waiting for rAF', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;

      handleOutboundMessage({ type: 'text_delta', id: msgId, content: 'partial' } as OutboundMessage);
      handleOutboundMessage({ type: 'done', id: msgId } as OutboundMessage);

      // Text should be flushed immediately despite rAF not having fired
      const assistant = state.messages.find((m) => m.role === 'assistant');
      expect(assistant!.responseText).toBe('partial');
      expect(assistant!.complete).toBe(true);
    });

    it('clears pending buffer on abort', () => {
      const { state, sendMessage, handleOutboundMessage, claudeAbort } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;

      handleOutboundMessage({ type: 'text_delta', id: msgId, content: 'partial' } as OutboundMessage);
      claudeAbort();

      // Fire rAF — should not flush anything since buffer was cleared
      if (rafCallbacks.length > 0) rafCallbacks[0]();

      const assistant = state.messages.find((m) => m.role === 'assistant');
      expect(assistant!.responseText).toBe('');
    });

    it('handles 200 text_delta events in rapid succession — single flush concatenates all', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;

      // Send 200 deltas synchronously (high token rate)
      const expected = Array.from({ length: 200 }, (_, i) => `t${i}`).join('');
      for (let i = 0; i < 200; i++) {
        handleOutboundMessage({ type: 'text_delta', id: msgId, content: `t${i}` } as OutboundMessage);
      }

      // Before rAF fires, responseText should still be empty (all buffered)
      const assistantBefore = state.messages.find((m) => m.role === 'assistant');
      expect(assistantBefore!.responseText).toBe('');

      // Fire a single rAF — all 200 deltas should be flushed in one update
      expect(rafCallbacks).toHaveLength(1);
      rafCallbacks[0]();

      const assistantAfter = state.messages.find((m) => m.role === 'assistant');
      expect(assistantAfter!.responseText).toBe(expected);
    });
  });

  describe('system messages', () => {
    it('addSystemMessage adds a message with role="system" to messages array', () => {
      const { state, addSystemMessage } = makeStore();
      addSystemMessage('network', 'Connection failed.');
      expect(state.messages).toHaveLength(1);
      expect(state.messages[0].role).toBe('system');
    });

    it('system message has errorType and text fields', () => {
      const { state, addSystemMessage } = makeStore();
      addSystemMessage('auth', 'Please authenticate.');
      const msg = state.messages[0] as any;
      expect(msg.errorType).toBe('auth');
      expect(msg.text).toBe('Please authenticate.');
    });

    it('system messages appear in correct position in timeline (after the last message)', () => {
      const { state, sendMessage, handleOutboundMessage, addSystemMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      handleOutboundMessage({ type: 'done', id: msgId } as OutboundMessage);
      addSystemMessage('network', 'Connection lost.');
      expect(state.messages).toHaveLength(3); // user + assistant + system
      expect(state.messages[2].role).toBe('system');
    });

    it('error OutboundMessage with auth-pattern text adds a classified system message automatically', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      handleOutboundMessage({
        type: 'error',
        id: msgId,
        message: 'Authentication failed: 401 Unauthorized',
      } as OutboundMessage);
      const systemMsgs = state.messages.filter((m) => m.role === 'system');
      expect(systemMsgs).toHaveLength(1);
      expect((systemMsgs[0] as any).errorType).toBe('auth');
    });

    it('error OutboundMessage with rate-limit pattern adds rate-limit system message', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      handleOutboundMessage({
        type: 'error',
        id: msgId,
        message: 'Rate limit exceeded (429)',
      } as OutboundMessage);
      const systemMsgs = state.messages.filter((m) => m.role === 'system');
      expect(systemMsgs).toHaveLength(1);
      expect((systemMsgs[0] as any).errorType).toBe('rate-limit');
    });

    it('first auth error includes first-run instructions in text', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      handleOutboundMessage({
        type: 'error',
        id: msgId,
        message: '401 Unauthorized',
      } as OutboundMessage);
      const systemMsg = state.messages.find((m) => m.role === 'system') as any;
      expect(systemMsg.text).toContain('claude login');
    });
  });

  describe('stuck state recovery on unmatched message id', () => {
    let origRAF: typeof globalThis.requestAnimationFrame;
    let origCancelRAF: typeof globalThis.cancelAnimationFrame;

    beforeEach(() => {
      origRAF = globalThis.requestAnimationFrame;
      origCancelRAF = globalThis.cancelAnimationFrame;
      globalThis.requestAnimationFrame = (cb: FrameRequestCallback) => {
        // Immediately invoke to flush buffers synchronously for test simplicity
        cb(performance.now());
        return 1;
      };
      globalThis.cancelAnimationFrame = () => {};
    });

    afterEach(() => {
      globalThis.requestAnimationFrame = origRAF;
      globalThis.cancelAnimationFrame = origCancelRAF;
    });

    it('done with unmatched id still sets sessionStatus to idle', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      // Put store into responding state
      handleOutboundMessage({ type: 'text_delta', id: msgId, content: 'Hi' } as OutboundMessage);
      expect(state.sessionStatus).toBe('responding');
      // Dispatch done with a non-existent id
      handleOutboundMessage({ type: 'done', id: 'unknown-id-999' } as OutboundMessage);
      expect(state.sessionStatus).toBe('idle');
    });

    it('error with unmatched id still sets sessionStatus to idle', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      // Put store into responding state
      handleOutboundMessage({ type: 'text_delta', id: msgId, content: 'Hi' } as OutboundMessage);
      expect(state.sessionStatus).toBe('responding');
      // Dispatch error with a non-existent id
      handleOutboundMessage({
        type: 'error',
        id: 'unknown-id-999',
        message: 'Something went wrong',
      } as OutboundMessage);
      expect(state.sessionStatus).toBe('idle');
    });

    it('error with unmatched id still adds classified system message', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      // Put store into responding state
      handleOutboundMessage({ type: 'text_delta', id: msgId, content: 'Hi' } as OutboundMessage);
      // Dispatch error with unmatched id and rate-limit pattern
      handleOutboundMessage({
        type: 'error',
        id: 'unknown-id-999',
        message: 'Rate limit exceeded (429)',
      } as OutboundMessage);
      const systemMsgs = state.messages.filter((m) => m.role === 'system');
      expect(systemMsgs).toHaveLength(1);
      expect((systemMsgs[0] as any).errorType).toBe('rate-limit');
    });

    it('done with valid id still works correctly after fix', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      handleOutboundMessage({ type: 'text_delta', id: msgId, content: 'Hello' } as OutboundMessage);
      handleOutboundMessage({ type: 'done', id: msgId } as OutboundMessage);
      const assistantMsg = state.messages.find((m) => m.role === 'assistant') as any;
      expect(assistantMsg.complete).toBe(true);
      expect(assistantMsg.thinkingComplete).toBe(true);
      expect(state.sessionStatus).toBe('idle');
    });
  });

  describe('pendingPermissionRequests', () => {
    it('initial state has empty pendingPermissionRequests array', () => {
      const { state } = makeStore();
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      expect((state as any).pendingPermissionRequests).toEqual([]);
    });

    it('permission_request outbound appends to pendingPermissionRequests', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;

      handleOutboundMessage({
        type: 'permission_request',
        id: msgId,
        request_id: 'r1',
        tool_name: 'Write',
        tool_input: { path: '/tmp/x' },
      } as OutboundMessage);

      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const pending = (state as any).pendingPermissionRequests as any[];
      expect(pending).toHaveLength(1);
      expect(pending[0]).toMatchObject({
        requestId: 'r1',
        toolName: 'Write',
        toolInput: { path: '/tmp/x' },
        messageId: msgId,
      });
    });

    it('duplicate request_id does not duplicate in pendingPermissionRequests', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;
      const pr = {
        type: 'permission_request' as const,
        id: msgId,
        request_id: 'r1',
        tool_name: 'Write',
        tool_input: { path: '/tmp/x' },
      };

      handleOutboundMessage(pr as OutboundMessage);
      handleOutboundMessage(pr as OutboundMessage);

      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const pending = (state as any).pendingPermissionRequests as any[];
      expect(pending).toHaveLength(1);
    });

    it('multiple distinct requests are all queued', () => {
      const { state, sendMessage, handleOutboundMessage } = makeStore();
      sendMessage('hello', {});
      const msgId = state.currentMessageId!;

      handleOutboundMessage({
        type: 'permission_request', id: msgId,
        request_id: 'r1', tool_name: 'Write', tool_input: { path: '/a' },
      } as OutboundMessage);
      handleOutboundMessage({
        type: 'permission_request', id: msgId,
        request_id: 'r2', tool_name: 'Bash', tool_input: { command: 'ls' },
      } as OutboundMessage);

      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const pending = (state as any).pendingPermissionRequests as any[];
      expect(pending).toHaveLength(2);
      expect(pending.map((p: any) => p.requestId)).toEqual(['r1', 'r2']);
    });
  });

  describe('decidePermission', () => {
    function setupWithPermissionRequest() {
      const onPermissionDecision = vi.fn();
      const store = makeStore({ onPermissionDecision });
      store.sendMessage('hello', {});
      const msgId = store.state.currentMessageId!;
      store.handleOutboundMessage({
        type: 'permission_request', id: msgId,
        request_id: 'r1', tool_name: 'Write', tool_input: { path: '/tmp/x' },
      } as OutboundMessage);
      return { ...store, onPermissionDecision };
    }

    it('decidePermission(allow) calls onPermissionDecision with the decision', () => {
      const { decidePermission, onPermissionDecision } = setupWithPermissionRequest() as any;
      decidePermission('r1', { behavior: 'allow' });
      expect(onPermissionDecision).toHaveBeenCalledOnce();
      expect(onPermissionDecision).toHaveBeenCalledWith({ requestId: 'r1', behavior: 'allow' });
    });

    it('decidePermission(allow) removes the entry from pendingPermissionRequests', () => {
      const { state, decidePermission } = setupWithPermissionRequest() as any;
      decidePermission('r1', { behavior: 'allow' });
      expect(state.pendingPermissionRequests).toHaveLength(0);
    });

    it('decidePermission(allow, remember:true) forwards remember:true to callback', () => {
      const { decidePermission, onPermissionDecision } = setupWithPermissionRequest() as any;
      decidePermission('r1', { behavior: 'allow', remember: true });
      expect(onPermissionDecision).toHaveBeenCalledWith({ requestId: 'r1', behavior: 'allow', remember: true });
    });

    it('decidePermission(deny, message) forwards both fields to callback', () => {
      const { decidePermission, onPermissionDecision } = setupWithPermissionRequest() as any;
      decidePermission('r1', { behavior: 'deny', message: 'not allowed' });
      expect(onPermissionDecision).toHaveBeenCalledWith({
        requestId: 'r1', behavior: 'deny', message: 'not allowed',
      });
    });

    it('decidePermission for unknown requestId is a no-op', () => {
      const { state, decidePermission, onPermissionDecision } = setupWithPermissionRequest() as any;
      decidePermission('unknown-id', { behavior: 'allow' });
      expect(onPermissionDecision).not.toHaveBeenCalled();
      expect(state.pendingPermissionRequests).toHaveLength(1); // original request still there
    });

    it('clearSession clears pendingPermissionRequests', () => {
      const { state, clearSession } = setupWithPermissionRequest() as any;
      expect(state.pendingPermissionRequests).toHaveLength(1);
      clearSession();
      expect(state.pendingPermissionRequests).toHaveLength(0);
    });
  });
});
