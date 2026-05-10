import { type Component, For, Show } from 'solid-js';
import type { AutoResolveLoopState } from '../stores/engineStore';
import styles from './AutoResolvePanel.module.css';

// ---------------------------------------------------------------------------
// AutoResolvePanel — surfaces `param x = auto` loop iteration progress
// (Task 2967). Conditionally mounted by App.tsx when autoResolve.active.
// ---------------------------------------------------------------------------

export interface AutoResolvePanelProps {
  state: AutoResolveLoopState;
}

export const AutoResolvePanel: Component<AutoResolvePanelProps> = (props) => {
  const latestIteration = () =>
    props.state.iterations.length > 0
      ? props.state.iterations[props.state.iterations.length - 1]
      : null;

  return (
    <div class={styles.panel} data-testid="auto-resolve-panel">
      <header class={styles.panelHeader} data-testid="panel-title-auto-resolve">
        <Show
          when={props.state.iterations.length > 0}
          fallback={<span class={styles.panelTitle}>Auto-Resolve</span>}
        >
          <span class={styles.panelTitle}>
            Iteration {props.state.iterations.length}
          </span>
        </Show>
      </header>

      {/* ── Parameters section ──────────────────────────────────────────── */}
      <Show when={latestIteration() !== null}>
        <section class={styles.section}>
          <div class={styles.sectionLabel}>Parameters</div>
          <For each={Object.entries(latestIteration()!.parameters)}>
            {([cellId, paramValue]) => (
              <div class={styles.row}>
                <span class={styles.cellId}>{cellId}</span>
                <span class={styles.value}>{paramValue.display}</span>
              </div>
            )}
          </For>
        </section>
      </Show>
    </div>
  );
};
