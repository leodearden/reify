import { type Component, Show } from 'solid-js';
import type { ChatMessage, SessionStatus } from '../types';
import styles from './ChatPanel.module.css';

export interface ChatPanelProps {
  messages: ChatMessage[];
  sessionStatus: SessionStatus;
  onSendMessage: (text: string) => void;
  onClearSession: () => void;
  onToggle: () => void;
  open: boolean;
  height: number;
  onResize: (delta: number) => void;
}

export const ChatPanel: Component<ChatPanelProps> = (props) => {
  return (
    <div
      data-testid="chat-panel"
      class={styles.container}
      style={{ height: `${props.height}px` }}
    >
      <div class={styles.header}>
        <span class={styles.headerTitle}>Claude Session</span>
        <button
          data-testid="chat-clear-btn"
          class={styles.headerBtn}
          onClick={() => props.onClearSession()}
          title="Clear session"
        >
          &#x1f5d1;
        </button>
        <button
          data-testid="chat-minimize-btn"
          class={styles.headerBtn}
          onClick={() => props.onToggle()}
          title="Minimize"
        >
          &#x2500;
        </button>
        <button
          data-testid="chat-close-btn"
          class={styles.headerBtn}
          onClick={() => props.onToggle()}
          title="Close"
        >
          &#x00d7;
        </button>
      </div>
      <Show when={props.messages.length === 0}>
        <div class={styles.emptyState}>
          Start a conversation with Claude to get help with your design.
        </div>
      </Show>
    </div>
  );
};
