import { batch } from 'solid-js';
import { createStore, produce } from 'solid-js/store';
import type { OutboundMessage } from '../../sidecar/src/types';
import { classifyError } from '../utils/errorClassifier';
import type { MessageContext, ToolCallInfo, ClaudeSessionStatus } from '../types';
import {
  onClaudeTextDelta,
  onClaudeThinkingDelta,
  onClaudeToolCall,
  onClaudeToolResult,
  onClaudeDone,
  onClaudeError,
  onClaudeReady,
  claudeSendMessage as bridgeSendMessage,
  claudeAbort as bridgeAbort,
  claudeClearSession as bridgeClearSession,
} from '../bridge';

// --- Domain types for UI consumption ---

export type SessionStatus = ClaudeSessionStatus;

export interface AssistantMessage {
  role: 'assistant';
  id: string;
  thinkingText: string;
  thinkingComplete: boolean;
  responseText: string;
  toolCalls: ToolCallInfo[];
  complete: boolean;
  error?: string;
}

export interface UserMessage {
  role: 'user';
  id: string;
  text: string;
}

export interface SystemMessage {
  role: 'system';
  id: string;
  errorType: string;
  text: string;
}

export type ChatMessage = UserMessage | AssistantMessage | SystemMessage;

export interface ClaudeState {
  messages: ChatMessage[];
  sessionStatus: SessionStatus;
  currentMessageId: string | null;
}

export interface ClaudeStoreOptions {
  onSend?: (id: string, text: string, context: MessageContext) => void;
  onAbort?: () => void;
}

let messageCounter = 0;
function generateId(): string {
  return `msg-${Date.now()}-${++messageCounter}`;
}

export function createClaudeStore(options: ClaudeStoreOptions) {
  const [state, setState] = createStore<ClaudeState>({
    messages: [],
    sessionStatus: 'idle',
    currentMessageId: null,
  });

  // --- Abort generation counter (monotonically increasing) ---
  // Incremented on every abort; sendMessage captures the current value
  // so its .then() can detect if an abort occurred while awaiting.
  let abortGeneration = 0;

  // --- Pending bridge ID for event buffering ---
  // Between calling bridgeSendMessage and receiving the resolved bridgeId,
  // events may arrive using the bridge-assigned ID before reconciliation.
  // We buffer them here and replay once the ID is known.
  let pendingLocalId: string | null = null;
  let pendingEventBuffer: OutboundMessage[] = [];

  // --- Subscription idempotency ---
  let activeCleanup: (() => void) | null = null;

  // --- Delta batching for 60fps rendering ---
  let textBuffer: string[] = [];
  let thinkingBuffer: string[] = [];
  let rafHandle: number | null = null;

  function flushBuffers() {
    rafHandle = null;
    const currentId = state.currentMessageId;
    if (!currentId) return;

    const textChunk = textBuffer.join('');
    const thinkingChunk = thinkingBuffer.join('');
    textBuffer = [];
    thinkingBuffer = [];

    if (!textChunk && !thinkingChunk) return;

    setState(
      'messages',
      (m: ChatMessage) => m.id === currentId && m.role === 'assistant',
      produce((msg: ChatMessage) => {
        if (msg.role !== 'assistant') return;
        if (textChunk) msg.responseText += textChunk;
        if (thinkingChunk) msg.thinkingText += thinkingChunk;
      }),
    );
  }

  function scheduleFlush() {
    if (rafHandle === null) {
      rafHandle = requestAnimationFrame(flushBuffers);
    }
  }

  function cancelAndFlush() {
    if (rafHandle !== null) {
      cancelAnimationFrame(rafHandle);
      rafHandle = null;
    }
    flushBuffers();
  }

  function cancelAndClear() {
    if (rafHandle !== null) {
      cancelAnimationFrame(rafHandle);
      rafHandle = null;
    }
    textBuffer = [];
    thinkingBuffer = [];
  }

  // --- Find the assistant message index for a given id ---
  function findAssistantIdx(id: string): number {
    return state.messages.findIndex(
      (m) => m.id === id && m.role === 'assistant',
    );
  }

  // --- Actions ---

  function handleOutboundMessage(msg: OutboundMessage): void {
    // Buffer events that arrive with an unknown ID while we're waiting for
    // bridge ID reconciliation. After reconciliation, these are replayed.
    if (
      'id' in msg &&
      msg.id &&
      pendingLocalId &&
      msg.id !== pendingLocalId &&
      msg.id !== state.currentMessageId &&
      state.messages.findIndex((m) => m.id === msg.id) === -1
    ) {
      pendingEventBuffer.push(msg);
      // Still allow status transitions for buffered events
      if (msg.type === 'text_delta') setState('sessionStatus', 'responding');
      if (msg.type === 'thinking_delta') setState('sessionStatus', 'thinking');
      if (msg.type === 'tool_call') setState('sessionStatus', 'tool-calling');
      return;
    }

    switch (msg.type) {
      case 'ready':
        // No state change needed
        break;

      case 'text_delta': {
        textBuffer.push(msg.content);
        setState('sessionStatus', 'responding');
        scheduleFlush();
        break;
      }

      case 'thinking_delta': {
        thinkingBuffer.push(msg.content);
        setState('sessionStatus', 'thinking');
        scheduleFlush();
        break;
      }

      case 'tool_call': {
        const idx = findAssistantIdx(msg.id);
        if (idx === -1) break;
        batch(() => {
          setState('sessionStatus', 'tool-calling');
          setState(
            'messages',
            idx,
            produce((m: ChatMessage) => {
              if (m.role !== 'assistant') return;
              m.toolCalls.push({
                id: `${msg.id}-tc-${m.toolCalls.length}`,
                toolName: msg.tool_name,
                toolInput: msg.tool_input,
                status: 'pending',
              });
            }),
          );
        });
        break;
      }

      case 'tool_result': {
        const idx = findAssistantIdx(msg.id);
        if (idx === -1) break;
        setState(
          'messages',
          idx,
          produce((m: ChatMessage) => {
            if (m.role !== 'assistant') return;
            const tc = m.toolCalls.find((t) => t.toolName === msg.tool_name && t.status === 'pending');
            if (tc) {
              tc.status = 'complete';
              tc.result = msg.result;
            }
          }),
        );
        break;
      }

      case 'done': {
        cancelAndFlush();
        setState('sessionStatus', 'idle');
        const idx = findAssistantIdx(msg.id);
        if (idx === -1) break;
        setState(
          'messages',
          idx,
          produce((m: ChatMessage) => {
            if (m.role !== 'assistant') return;
            m.complete = true;
            m.thinkingComplete = true;
          }),
        );
        break;
      }

      case 'error': {
        cancelAndFlush();
        setState('sessionStatus', 'idle');
        // Auto-classify error and add system message
        const classified = classifyError(msg.message);
        addSystemMessage(classified.type, classified.userMessage);
        const idx = findAssistantIdx(msg.id);
        if (idx === -1) break;
        setState(
          'messages',
          idx,
          produce((m: ChatMessage) => {
            if (m.role !== 'assistant') return;
            m.error = msg.message;
            m.complete = true;
          }),
        );
        break;
      }
    }
  }

  function addSystemMessage(errorType: string, text: string): void {
    const id = generateId();
    const sysMsg: SystemMessage = { role: 'system', id, errorType, text };
    setState('messages', (msgs) => [...msgs, sysMsg]);
  }

  function sendMessage(text: string, context: MessageContext): void {
    const id = generateId();
    const userMsg: UserMessage = { role: 'user', id, text };
    const assistantMsg: AssistantMessage = {
      role: 'assistant',
      id,
      thinkingText: '',
      thinkingComplete: false,
      responseText: '',
      toolCalls: [],
      complete: false,
    };

    batch(() => {
      setState('messages', (msgs) => [...msgs, userMsg, assistantMsg]);
      setState('currentMessageId', id);
    });

    if (options.onSend) {
      options.onSend(id, text, context);
    } else {
      // Track the abort generation at call time so we can detect stale resolutions.
      const sendGeneration = abortGeneration;
      pendingLocalId = id;
      pendingEventBuffer = [];

      bridgeSendMessage(text, context).then((bridgeId) => {
        // If an abort happened after we sent but before the promise resolved,
        // discard the reconciliation — the session has been cancelled.
        if (abortGeneration !== sendGeneration) {
          pendingLocalId = null;
          pendingEventBuffer = [];
          return;
        }

        batch(() => {
          setState('messages', (m) => m.id === id && m.role === 'assistant', 'id', bridgeId);
          setState('currentMessageId', bridgeId);
        });

        // Replay any events that arrived with bridgeId before reconciliation.
        const buffered = pendingEventBuffer;
        pendingLocalId = null;
        pendingEventBuffer = [];
        for (const bufferedMsg of buffered) {
          handleOutboundMessage(bufferedMsg);
        }
      }).catch((err: unknown) => {
        pendingLocalId = null;
        pendingEventBuffer = [];
        const errMsg = err instanceof Error ? err.message : String(err);
        const classified = classifyError(errMsg);
        setState('sessionStatus', 'idle');
        addSystemMessage(classified.type, classified.userMessage);
        const idx = state.messages.findIndex((m) => m.id === id && m.role === 'assistant');
        if (idx !== -1) {
          setState(
            'messages',
            idx,
            produce((m: ChatMessage) => {
              if (m.role !== 'assistant') return;
              m.error = errMsg;
              m.complete = true;
            }),
          );
        }
      });
    }
  }

  function claudeAbort(): void {
    abortGeneration++;
    pendingLocalId = null;
    pendingEventBuffer = [];
    cancelAndClear();
    setState('sessionStatus', 'idle');
    if (options.onAbort) {
      options.onAbort();
    } else {
      bridgeAbort().catch((err: unknown) => {
        console.warn('Claude abort failed:', err);
      });
    }
  }

  async function subscribeToEvents(): Promise<() => void> {
    // Idempotency guard: clean up any previous subscription before re-subscribing.
    if (activeCleanup) {
      activeCleanup();
      activeCleanup = null;
    }

    const results = await Promise.allSettled([
      onClaudeTextDelta((p) => handleOutboundMessage({ type: 'text_delta', id: p.id, content: p.content })),
      onClaudeThinkingDelta((p) => handleOutboundMessage({ type: 'thinking_delta', id: p.id, content: p.content })),
      onClaudeToolCall((p) => handleOutboundMessage({ type: 'tool_call', id: p.id, tool_name: p.tool_name, tool_input: p.tool_input })),
      onClaudeToolResult((p) => handleOutboundMessage({ type: 'tool_result', id: p.id, tool_name: p.tool_name, result: p.result })),
      onClaudeDone((p) => handleOutboundMessage({ type: 'done', id: p.id })),
      onClaudeError((p) => handleOutboundMessage({ type: 'error', id: p.id, message: p.message })),
      onClaudeReady(() => handleOutboundMessage({ type: 'ready' })),
    ]);

    const unlisteners: (() => void)[] = [];
    for (const result of results) {
      if (result.status === 'fulfilled') {
        unlisteners.push(result.value);
      } else {
        console.warn('Failed to subscribe to Claude event:', result.reason);
      }
    }

    const cleanup = () => {
      for (const unlisten of unlisteners) {
        unlisten();
      }
      if (activeCleanup === cleanup) {
        activeCleanup = null;
      }
    };
    activeCleanup = cleanup;

    return cleanup;
  }

  function clearSession(): void {
    cancelAndClear();
    pendingLocalId = null;
    pendingEventBuffer = [];
    batch(() => {
      setState('messages', []);
      setState('sessionStatus', 'idle');
      setState('currentMessageId', null);
    });
    // In bridge mode, also clear the sidecar's conversation history.
    if (!options.onSend) {
      bridgeClearSession().catch((err: unknown) => {
        console.warn('Claude clear session failed:', err);
      });
    }
  }

  return {
    state,
    handleOutboundMessage,
    sendMessage,
    addSystemMessage,
    claudeAbort,
    clearSession,
    subscribeToEvents,
  };
}
