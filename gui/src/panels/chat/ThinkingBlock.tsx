import { type Component, createSignal, Show } from 'solid-js';
import styles from './ThinkingBlock.module.css';

export interface ThinkingBlockProps {
  text: string;
  complete: boolean;
}

export const ThinkingBlock: Component<ThinkingBlockProps> = (props) => {
  const [expanded, setExpanded] = createSignal(false);

  return (
    <Show
      when={props.complete}
      fallback={
        <div data-testid="thinking-indicator" class={styles.indicator}>
          <span class={styles.dots}>
            <span class={styles.dot} />
            <span class={styles.dot} />
            <span class={styles.dot} />
          </span>
          <span class={styles.label}>Thinking...</span>
        </div>
      }
    >
      <div data-testid="thinking-block" class={styles.block}>
        <div
          class={styles.header}
          role="button"
          tabindex="0"
          onClick={() => setExpanded((v) => !v)}
          onKeyDown={(e: KeyboardEvent) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault();
              setExpanded((v) => !v);
            }
          }}
        >
          <span class={styles.chevron}>{expanded() ? '▼' : '▶'}</span>
          <span>Thinking</span>
        </div>
        <Show when={expanded()}>
          <div class={styles.content}>{props.text}</div>
        </Show>
      </div>
    </Show>
  );
};
