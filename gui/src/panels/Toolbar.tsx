import { type Component, Show } from 'solid-js';
import styles from './Toolbar.module.css';

export interface ToolbarProps {
  onExport: () => void;
  onFitToView: () => void;
  onToggleChatPanel?: () => void;
}

export const Toolbar: Component<ToolbarProps> = (props) => {
  return (
    <div data-testid="toolbar" class={styles.container} role="toolbar">
      <button class={styles.button} onClick={() => props.onExport()}>
        Export
      </button>
      <button class={styles.button} onClick={() => props.onFitToView()}>
        Fit to View
      </button>
      <Show when={props.onToggleChatPanel}>
        <div class={styles.spacer} />
        <button class={styles.button} onClick={() => props.onToggleChatPanel?.()}>
          Claude
        </button>
      </Show>
    </div>
  );
};
