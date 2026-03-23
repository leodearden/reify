import { type Component, createMemo, Show } from 'solid-js';
import { renderMarkdown } from './markdown';
import styles from './StreamingText.module.css';

export interface StreamingTextProps {
  text: string;
  streaming: boolean;
}

export const StreamingText: Component<StreamingTextProps> = (props) => {
  const html = createMemo(() => renderMarkdown(props.text));

  return (
    <div data-testid="streaming-text" class={styles.container}>
      <div class={styles.prose} innerHTML={html()} />
      <Show when={props.streaming}>
        <span data-testid="streaming-cursor" class={styles.cursor} />
      </Show>
    </div>
  );
};
