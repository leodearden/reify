import { type Component, type JSX, Show, For, createSignal, createEffect } from 'solid-js';
import type { ChatMessage, SessionStatus } from '../types';
import { Splitter } from '../components/Splitter';
import styles from './ChatPanel.module.css';

/** Lightweight markdown renderer for assistant messages.
 *  Supports: code blocks (```), inline code (`), bold (**), italic (*).
 */
function renderMarkdown(text: string): JSX.Element {
  // Split on code blocks (``` ... ```)
  const codeBlockParts = text.split(/```(?:\w*\n?)?/);
  const elements: JSX.Element[] = [];

  for (let i = 0; i < codeBlockParts.length; i++) {
    if (i % 2 === 1) {
      // Inside a code block
      const code = codeBlockParts[i].replace(/^\n/, '').replace(/\n$/, '');
      elements.push(<pre class={styles.codeBlock}><code>{code}</code></pre>);
    } else {
      // Normal text — apply inline formatting
      elements.push(renderInline(codeBlockParts[i]));
    }
  }

  return <>{elements}</>;
}

function renderInline(text: string): JSX.Element {
  // Process inline code, bold, and italic via regex split
  // Order matters: inline code first (to avoid processing markdown inside backticks)
  const parts: JSX.Element[] = [];
  // Split on inline code spans: `...`
  const codeParts = text.split(/`([^`]+)`/);
  for (let i = 0; i < codeParts.length; i++) {
    if (i % 2 === 1) {
      parts.push(<code>{codeParts[i]}</code>);
    } else {
      // Process bold and italic in non-code segments
      parts.push(renderBoldItalic(codeParts[i]));
    }
  }
  return <>{parts}</>;
}

function renderBoldItalic(text: string): JSX.Element {
  // Bold: **...**
  const boldParts = text.split(/\*\*([^*]+)\*\*/);
  const parts: JSX.Element[] = [];
  for (let i = 0; i < boldParts.length; i++) {
    if (i % 2 === 1) {
      parts.push(<strong>{boldParts[i]}</strong>);
    } else {
      // Italic: *...*
      const italicParts = boldParts[i].split(/\*([^*]+)\*/);
      for (let j = 0; j < italicParts.length; j++) {
        if (j % 2 === 1) {
          parts.push(<em>{italicParts[j]}</em>);
        } else {
          parts.push(<>{italicParts[j]}</>);
        }
      }
    }
  }
  return <>{parts}</>;
}

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
  const [inputText, setInputText] = createSignal('');
  let messageListRef: HTMLDivElement | undefined;

  createEffect(() => {
    // Track messages length to trigger on new messages
    const len = props.messages.length;
    if (len > 0 && messageListRef) {
      const el = messageListRef;
      // Check if user is near bottom (within 50px) before scrolling
      const nearBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 50;
      if (nearBottom) {
        el.scrollTop = el.scrollHeight;
      }
    }
  });

  function handleSend() {
    const text = inputText().trim();
    if (text === '') return;
    props.onSendMessage(text);
    setInputText('');
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  return (
    <Show when={props.open}>
      <div
        data-testid="chat-panel"
        class={styles.container}
        style={{ height: `${props.height}px` }}
      >
        <Splitter
          orientation="horizontal"
          onResize={(delta) => props.onResize(-delta)}
          data-testid="chat-resize-handle"
        />
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
        <Show when={props.messages.length > 0}>
          <div data-testid="chat-message-list" class={styles.messageList} ref={messageListRef}>
            <For each={props.messages}>
              {(msg) => (
                <div
                  data-testid={`chat-message-${msg.id}`}
                  data-role={msg.role}
                  class={`${styles.message} ${msg.role === 'user' ? styles.userMessage : styles.assistantMessage}`}
                >
                  {msg.role === 'assistant' ? renderMarkdown(msg.content) : msg.content}
                </div>
              )}
            </For>
          </div>
        </Show>
        <div class={styles.inputBar}>
          <textarea
            data-testid="chat-input"
            class={styles.textarea}
            placeholder="Ask Claude about your design..."
            value={inputText()}
            onInput={(e) => setInputText(e.currentTarget.value)}
            onKeyDown={handleKeyDown}
            disabled={props.sessionStatus !== 'idle'}
          />
          <button
            data-testid="chat-send-btn"
            class={styles.sendButton}
            onClick={handleSend}
            disabled={props.sessionStatus !== 'idle'}
          >
            Send
          </button>
        </div>
      </div>
    </Show>
  );
};
