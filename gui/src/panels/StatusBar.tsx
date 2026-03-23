import { type Component, createMemo, Show } from 'solid-js';
import type { EvaluationStatus, MeshData, ConstraintData } from '../types';
import type { SessionStatus } from '../stores/claudeStore';
import styles from './StatusBar.module.css';

export interface StatusBarProps {
  evalStatus: EvaluationStatus;
  meshes: Record<string, MeshData>;
  constraints: Record<string, ConstraintData>;
  claudeStatus?: SessionStatus;
  onToggleChat?: () => void;
}

export const StatusBar: Component<StatusBarProps> = (props) => {
  const triangleCount = createMemo(() => {
    let total = 0;
    for (const mesh of Object.values(props.meshes)) {
      total += Math.floor(mesh.indices.length / 3);
    }
    return total;
  });

  const constraintSummary = createMemo(() => {
    const counts = { satisfied: 0, violated: 0, indeterminate: 0 };
    for (const c of Object.values(props.constraints)) {
      if (c.status === 'satisfied') counts.satisfied++;
      else if (c.status === 'violated') counts.violated++;
      else counts.indeterminate++;
    }
    return counts;
  });

  function claudeStatusText(status: SessionStatus): string {
    switch (status) {
      case 'thinking': return 'thinking...';
      case 'tool-calling': return 'calling tool...';
      case 'responding': return 'responding...';
      default: return status;
    }
  }

  return (
    <div data-testid="status-bar" class={styles.container} role="status" aria-live="polite">
      <span class={styles.section}>
        <span class={styles.label}>Status:</span>
        <span class={styles.phase} data-phase={props.evalStatus.phase}>
          {props.evalStatus.phase}
        </span>
      </span>
      <span class={styles.divider} />
      <span class={styles.section}>
        <span class={styles.label}>Triangles:</span>
        <span class={styles.value}>{triangleCount()}</span>
      </span>
      <span class={styles.divider} />
      <span class={styles.section}>
        <span class={styles.constraintCount} data-status="satisfied">
          {constraintSummary().satisfied}
        </span>
        <span class={styles.constraintCount} data-status="violated">
          {constraintSummary().violated}
        </span>
        <span class={styles.constraintCount} data-status="indeterminate">
          {constraintSummary().indeterminate}
        </span>
      </span>
      <Show when={props.claudeStatus}>
        {(status) => (
          <>
            <span class={styles.divider} />
            <span
              class={`${styles.section} ${styles.claudeStatus}`}
              data-testid="claude-status"
              data-claude-status={status()}
              onClick={() => props.onToggleChat?.()}
            >
              <span class={styles.label}>Claude:</span>
              <span class={styles.claudeStatusText}>{claudeStatusText(status())}</span>
            </span>
          </>
        )}
      </Show>
    </div>
  );
};
