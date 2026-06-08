import { type Component, createSignal, createEffect, For, Show } from 'solid-js';
import type { ChatMessage, MessageContext, AssistantMessage, SystemMessage as SystemMessageType, PendingPermissionRequest, PermissionDecision } from '../stores/claudeStore';
import { MessageGroup } from './chat/MessageGroup';
import { AbortButton } from './chat/AbortButton';
import { SystemMessage } from './chat/SystemMessage';
import { ContextPicker, type ContextType } from './chat/ContextPicker';
import { ContextChip } from './chat/ContextChip';
import { PermissionPrompt } from './chat/PermissionPrompt';
import styles from './ChatPanel.module.css';

export interface AttachedContext {
  type: ContextType;
  label: string;
}

export interface ChatPanelProps {
  store: {
    state: {
      messages: ChatMessage[];
      sessionStatus: string;
      currentMessageId: string | null;
      pendingPermissionRequests: PendingPermissionRequest[];
    };
    sendMessage: (text: string, context: MessageContext) => void;
    claudeAbort: () => void;
    decidePermission: (requestId: string, decision: PermissionDecision) => void;
  };
  selectedEntity?: string;
  engineConstraints?: Array<{ expression?: string; status?: string }>;
  diagnostics?: string[];
  activeFile?: string;
}

export const ChatPanel: Component<ChatPanelProps> = (props) => {
  const [inputText, setInputText] = createSignal('');
  const [attachedContexts, setAttachedContexts] = createSignal<AttachedContext[]>([]);
  let messageListRef: HTMLDivElement | undefined;

  // Auto-scroll to bottom when new messages arrive or content changes
  createEffect(() => {
    const _msgs = props.store.state.messages;
    if (messageListRef) {
      messageListRef.scrollTop = messageListRef.scrollHeight;
    }
  });

  function buildContextLabel(type: ContextType): string {
    switch (type) {
      case 'selection':
        return props.selectedEntity ?? 'Selection';
      case 'diagnostics':
        return 'Diagnostics';
      case 'constraints':
        return 'Violated constraints';
      case 'file':
        return props.activeFile ?? 'Current file';
    }
  }

  function handleAttach(type: ContextType) {
    // Don't add duplicate types
    if (attachedContexts().some((c) => c.type === type)) return;
    const label = buildContextLabel(type);
    setAttachedContexts((prev) => [...prev, { type, label }]);
  }

  function handleRemoveChip(type: ContextType) {
    setAttachedContexts((prev) => prev.filter((c) => c.type !== type));
  }

  // SYNC: This function must populate every field of MessageContext.
  // When adding a field to MessageContext, also update:
  // - bridge.ts MESSAGE_CONTEXT_FIELD_MAP (compile-time enforced)
  // - bridge.ts BUILD_CONTEXT_HANDLED_FIELDS (compile-time enforced via types.typecheck.ts)
  // - this function (manual)
  // See: gui/src/__tests__/types.typecheck.ts for compile-time guards.
  function buildMessageContext(): MessageContext {
    const ctx: MessageContext = {};
    const attached = attachedContexts();

    // Always include selectedEntity if available (automatic context)
    if (props.selectedEntity) {
      ctx.selectedEntity = props.selectedEntity;
    }

    // Add explicitly attached contexts
    for (const item of attached) {
      switch (item.type) {
        case 'selection':
          if (props.selectedEntity) ctx.selectedEntity = props.selectedEntity;
          break;
        case 'diagnostics':
          if (props.diagnostics) ctx.diagnostics = props.diagnostics;
          break;
        case 'constraints':
          if (props.engineConstraints) {
            ctx.constraints = props.engineConstraints.map(
              (c) => c.expression ?? 'unknown'
            );
          }
          break;
        case 'file':
          if (props.activeFile) ctx.currentFile = props.activeFile;
          break;
      }
    }

    if (attached.length > 0) {
      ctx.attachedContexts = attached.map((c) => c.type);
    }

    return ctx;
  }

  function handleSend() {
    const text = inputText().trim();
    if (!text) return;
    const context = buildMessageContext();
    props.store.sendMessage(text, context);
    setInputText('');
    setAttachedContexts([]);
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      if (!isActive()) handleSend();
    }
  }

  const isActive = () => props.store.state.sessionStatus !== 'idle';

  const hasViolatedConstraints = () =>
    (props.engineConstraints ?? []).some((c) => c.status === 'violated');

  return (
    <div data-testid="chat-panel" class={styles.panel}>
      <div class="panel-title" data-testid="panel-title-assistant">Assistant</div>
      <div ref={messageListRef} class={styles.messageList}>
        <Show when={props.store.state.messages.length === 0}>
          <div class={styles.emptyState}>Start a conversation</div>
        </Show>
        <For each={props.store.state.messages}>
          {(msg) => (
            <Show
              when={msg.role === 'assistant'}
              fallback={
                <Show
                  when={msg.role === 'system'}
                  fallback={
                    <div data-testid="user-message" class={styles.userMessage}>
                      {(msg as { text: string }).text}
                      <Show when={props.selectedEntity}>
                        <span data-testid="auto-context-label" class={styles.autoContextLabel}>
                          [Context: {props.selectedEntity} selected]
                        </span>
                      </Show>
                    </div>
                  }
                >
                  <SystemMessage
                    errorType={(msg as SystemMessageType).errorType}
                    text={(msg as SystemMessageType).text}
                  />
                </Show>
              }
            >
              <MessageGroup message={msg as AssistantMessage} />
            </Show>
          )}
        </For>
      </div>
      <Show when={props.store.state.pendingPermissionRequests.length > 0}>
        <div data-testid="permission-prompts">
          {/*
           * Solid's <For> keys by object reference. The store appends entries via
           *   setState('pendingPermissionRequests', (reqs) => [...reqs, newReq])
           * and removes them via .filter(), both of which preserve the identity of
           * existing entries. So unchanged prompts retain their DOM/focus state
           * across re-renders without an explicit `key` prop.
           */}
          <For each={props.store.state.pendingPermissionRequests}>
            {(req) => (
              <PermissionPrompt
                toolName={req.toolName}
                toolInput={req.toolInput}
                onDecide={(d) => props.store.decidePermission(req.requestId, d)}
              />
            )}
          </For>
        </div>
      </Show>
      <Show when={attachedContexts().length > 0}>
        <div data-testid="context-chips" class={styles.contextChips}>
          <For each={attachedContexts()}>
            {(ctx) => (
              <ContextChip
                label={ctx.label}
                type={ctx.type}
                onRemove={() => handleRemoveChip(ctx.type)}
              />
            )}
          </For>
        </div>
      </Show>
      <div class={styles.inputArea}>
        <ContextPicker
          onAttach={handleAttach}
          hasSelection={!!props.selectedEntity}
          hasDiagnostics={(props.diagnostics ?? []).length > 0}
          hasViolatedConstraints={hasViolatedConstraints()}
          hasActiveFile={!!props.activeFile}
        />
        <textarea
          data-testid="chat-input"
          class={styles.textarea}
          aria-label="Ask Claude"
          placeholder="Ask Claude..."
          value={inputText()}
          onInput={(e) => setInputText(e.currentTarget.value)}
          onKeyDown={handleKeyDown}
          rows={2}
        />
        <Show
          when={isActive()}
          fallback={
            <button
              data-testid="send-button"
              class={styles.sendButton}
              disabled={!inputText().trim()}
              onClick={handleSend}
            >
              Send
            </button>
          }
        >
          <AbortButton onAbort={() => props.store.claudeAbort()} />
        </Show>
      </div>
    </div>
  );
};
