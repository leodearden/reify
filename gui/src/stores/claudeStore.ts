import { batch } from 'solid-js';
import { createStore, produce } from 'solid-js/store';
import type { OutboundMessage } from '../../sidecar/src/types';
import { classifyError } from '../utils/errorClassifier';

// --- Domain types for UI consumption ---

export type SessionStatus = 'idle' | 'thinking' | 'responding' | 'tool-calling';

export interface ToolCallInfo {
  id: string;
  toolName: string;
  toolInput: Record<string, unknown>;
  status: 'pending' | 'complete' | 'error';
  result?: unknown;
}

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

export interface MessageContext {
  selectedEntity?: string;
  diagnostics?: string[];
  constraints?: string[];
  currentFile?: string;
  attachedContexts?: string[];
}

export type ChatMessage = UserMessage | AssistantMessage | SystemMessage;

export interface PendingPermissionRequest {
  requestId: string;
  toolName: string;
  toolInput: Record<string, unknown>;
  messageId: string;
}

export interface ClaudeState {
  messages: ChatMessage[];
  sessionStatus: SessionStatus;
  currentMessageId: string | null;
  pendingPermissionRequests: PendingPermissionRequest[];
}

export interface PermissionDecision {
  behavior: 'allow' | 'deny';
  message?: string;
  updatedInput?: Record<string, unknown>;
  remember?: boolean;
}

export interface ClaudeStoreOptions {
  onSend: (id: string, text: string, context: MessageContext) => void;
  onAbort: () => void;
  onPermissionDecision: (decision: { requestId: string } & PermissionDecision) => void;
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
    pendingPermissionRequests: [],
  });

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
        // Clear any permission prompts that were outstanding during this turn —
        // their request_ids are now stale and showing them would be misleading.
        setState('pendingPermissionRequests', []);
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

      case 'notice': {
        // Non-terminal diagnostic from the sidecar. Logs to console.warn for operator
        // visibility (dev-tools / production logs) but does NOT cancel/flush the
        // pending RAF, does NOT mutate sessionStatus, does NOT touch the in-flight
        // assistant message, and does NOT add a SystemMessage to the chat transcript.
        // The pre-regression behavior was stderr-only invisibility on the sidecar
        // side; this upgrades to host-operator visibility while preserving the
        // graceful-degradation lifecycle for the in-flight turn.
        // eslint-disable-next-line no-console
        console.warn(`[claudeStore] sidecar notice: code=${msg.code} id=${msg.id} message=${msg.message}`);
        break;
      }

      case 'error': {
        cancelAndFlush();
        setState('sessionStatus', 'idle');
        // Clear any permission prompts that were outstanding during this turn —
        // their request_ids are now stale and showing them would be misleading.
        setState('pendingPermissionRequests', []);
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

      case 'permission_request': {
        // Deduplicate by requestId
        const alreadyPending = state.pendingPermissionRequests.some(
          (r) => r.requestId === msg.request_id,
        );
        if (alreadyPending) break;
        setState('pendingPermissionRequests', (reqs) => [
          ...reqs,
          {
            requestId: msg.request_id,
            toolName: msg.tool_name,
            toolInput: msg.tool_input,
            messageId: msg.id,
          },
        ]);
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

    options.onSend(id, text, context);
  }

  function claudeAbort(): void {
    cancelAndClear();
    setState('sessionStatus', 'idle');
    // Clear any pending permission prompts — after abort they are stale (the
    // underlying request_ids are gone) and the UI would show non-functional controls.
    setState('pendingPermissionRequests', []);
    options.onAbort();
  }

  function decidePermission(requestId: string, decision: PermissionDecision): void {
    const exists = state.pendingPermissionRequests.some((r) => r.requestId === requestId);
    if (!exists) return;
    options.onPermissionDecision({ requestId, ...decision });
    setState('pendingPermissionRequests', (reqs) => reqs.filter((r) => r.requestId !== requestId));
  }

  function clearSession(): void {
    cancelAndClear();
    batch(() => {
      setState('messages', []);
      setState('sessionStatus', 'idle');
      setState('currentMessageId', null);
      setState('pendingPermissionRequests', []);
    });
  }

  return {
    state,
    handleOutboundMessage,
    sendMessage,
    addSystemMessage,
    claudeAbort,
    clearSession,
    decidePermission,
  };
}
