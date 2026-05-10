import { type Component, For, Show } from 'solid-js';
import type { AutoResolveLoopState } from '../stores/engineStore';
import type { AutoResolveConstraintProgress } from '../types';
import styles from './AutoResolvePanel.module.css';

// ---------------------------------------------------------------------------
// Constraint target-bound formatting helper
// ---------------------------------------------------------------------------

/** Format the target-bound expression for a constraint row (e.g. "≤ 200MPa"). */
function formatTarget(c: AutoResolveConstraintProgress): string | null {
  const unit = c.unit ?? '';
  if (c.target_lower !== undefined && c.target_upper !== undefined) {
    return `${c.target_lower}${unit} – ${c.target_upper}${unit}`;
  }
  if (c.target_upper !== undefined) return `≤ ${c.target_upper}${unit}`;
  if (c.target_lower !== undefined) return `≥ ${c.target_lower}${unit}`;
  return null;
}

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

        {/* ── Constraints section ──────────────────────────────────────── */}
        <section class={styles.section}>
          <div class={styles.sectionLabel}>Constraints</div>
          <For each={Object.entries(latestIteration()!.constraints)}>
            {([, constraint]) => {
              const target = formatTarget(constraint);
              const unit = constraint.unit ?? '';
              return (
                <div class={styles.row}>
                  <span class={styles.cellId}>{constraint.name}</span>
                  <span class={styles.value}>
                    {constraint.value}{unit}
                  </span>
                  <Show when={target !== null}>
                    <span class={styles.targetBound}>{target}</span>
                  </Show>
                  <span
                    class={styles.statusMarker}
                    data-status={constraint.satisfied ? 'ok' : 'violation'}
                  >
                    {constraint.satisfied ? '✓' : '✗'}
                  </span>
                </div>
              );
            }}
          </For>
        </section>
      </Show>
    </div>
  );
};
