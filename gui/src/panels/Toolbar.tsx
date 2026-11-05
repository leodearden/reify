import { type Component, Show } from 'solid-js';
import styles from './Toolbar.module.css';
import { shortcutKey } from '../shortcuts';

export interface ToolbarProps {
  onExport: () => void;
  onFitToView: () => void;
  /** Whether the edges-in-black view is on (drives the toggle's pressed state). */
  edges: boolean;
  onToggleEdges: () => void;
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
      <button
        data-testid="toolbar-edges-toggle"
        class={styles.button}
        aria-pressed={props.edges}
        title={`Toggle edges-in-black view (${shortcutKey('toggleEdges')})`}
        onClick={() => props.onToggleEdges()}
      >
        Edges
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
