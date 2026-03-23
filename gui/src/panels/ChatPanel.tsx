import { type Component, createSignal, createEffect, For, Show } from 'solid-js';
import type { ChatMessage } from '../stores/claudeStore';
import { MessageGroup } from './chat/MessageGroup';
import { AbortButton } from './chat/AbortButton';
import styles from './ChatPanel.module.css';

export interface ChatPanelProps {
  store: {
    state: {
      messages: ChatMessage[];
      sessionStatus: string;
      currentMessageId: string | null;
    };
    sendMessage: (text: string, context: Record<string, unknown>) => void;
    claudeAbort: () => void;
  };
}

export const ChatPanel: Component<ChatPanelProps> = (props) => {
  const [inputText, setInputText] = createSignal('');
  let messageListRef: HTMLDivElement | undefined;

  // Auto-scroll to bottom when new messages arrive or content changes
  createEffect(() => {
    const _msgs = props.store.state.messages;
    if (messageListRef) {
      messageListRef.scrollTop = messageListRef.scrollHeight;
    }
  });

  function handleSend() {
    const text = inputText().trim();
    if (!text) return;
    props.store.sendMessage(text, {});
    setInputText('');
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  const isActive = () => props.store.state.sessionStatus !== 'idle';

  return (
    <div data-testid="chat-panel" class={styles.panel}>
      <div ref={messageListRef} class={styles.messageList}>
        <Show when={props.store.state.messages.length === 0}>
          <div class={styles.emptyState}>Start a conversation</div>
        </Show>
        <For each={props.store.state.messages}>
          {(msg) => (
            <Show
              when={msg.role === 'assistant'}
              fallback={
                <div data-testid="user-message" class={styles.userMessage}>
                  {(msg as { text: string }).text}
                </div>
              }
            >
              <MessageGroup message={msg as import('../stores/claudeStore').AssistantMessage} />
            </Show>
          )}
        </For>
      </div>
      <div class={styles.inputArea}>
        <textarea
          data-testid="chat-input"
          class={styles.textarea}
          placeholder="Ask Claude..."
          value={inputText()}
          onInput={(e) => setInputText(e.currentTarget.value)}
          onKeyDown={handleKeyDown}
          rows={1}
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
