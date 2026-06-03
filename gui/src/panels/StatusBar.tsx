import { type Component, createMemo, Show } from 'solid-js';
import type { EvaluationStatus, MeshData, ConstraintData, DiagnosticInfo } from '../types';
import type { SessionStatus } from '../stores/claudeStore';
import styles from './StatusBar.module.css';

interface DiagnosticSummary { errorCount: number; warningCount: number }

/** Shared badge content for diagnostic trigger buttons.
 *  Accepts a reactive accessor so SolidJS tracks each summary property fine-grained. */
const DiagBadgeContent: Component<{ getSummary: () => DiagnosticSummary }> = (props) => (
  <>
    <Show when={props.getSummary().errorCount > 0}>
      <span class={styles.errorBadge}>
        {pluralize(props.getSummary().errorCount, 'error')}
      </span>
    </Show>
    <Show when={props.getSummary().warningCount > 0}>
      <span class={styles.warningBadge}>
        {pluralize(props.getSummary().warningCount, 'warning')}
      </span>
    </Show>
  </>
);

function summarize(diags: DiagnosticInfo[] | undefined): DiagnosticSummary {
  let errorCount = 0;
  let warningCount = 0;
  for (const d of diags ?? []) {
    if (d.severity === 'Error') errorCount++;
    else if (d.severity === 'Warning') warningCount++;
  }
  return { errorCount, warningCount };
}

function pluralize(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? '' : 's'}`;
}

export interface StatusBarProps {
  evalStatus: EvaluationStatus;
  meshes: Record<string, MeshData>;
  constraints: Record<string, ConstraintData>;
  claudeStatus?: SessionStatus;
  onToggleChat?: () => void;
  tessellationDiagnostics?: DiagnosticInfo[];
  compileDiagnostics?: DiagnosticInfo[];
  onToggleDiagnostics?: () => void;
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

  const diagnosticSummary = createMemo(() => summarize(props.tessellationDiagnostics));

  const compileSummary = createMemo(() => summarize(props.compileDiagnostics));

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
        <Show
          when={triangleCount() > 0}
          fallback={
            <span class={styles.value}>
              {diagnosticSummary().errorCount > 0 ? 'Tessellation error' : 'No geometry'}
            </span>
          }
        >
          <span class={styles.value}>{triangleCount()}</span>
        </Show>
      </span>
      <Show when={(props.tessellationDiagnostics?.length ?? 0) > 0}>
        <span class={styles.divider} />
        <button
          type="button"
          class={`${styles.section} ${styles.diagnosticsTrigger}`}
          data-testid="tessellation-errors"
          data-has-errors={diagnosticSummary().errorCount > 0 ? 'true' : 'false'}
          aria-label={`Show ${pluralize(props.tessellationDiagnostics?.length ?? 0, 'tessellation diagnostic')}`}
          onClick={() => props.onToggleDiagnostics?.()}
        >
          <span class={styles.pipelineLabel}>Tessellation</span>
          <DiagBadgeContent getSummary={diagnosticSummary} />
        </button>
      </Show>
      <Show when={(props.compileDiagnostics?.length ?? 0) > 0}>
        <span class={styles.divider} />
        <button
          type="button"
          class={`${styles.section} ${styles.diagnosticsTrigger}`}
          data-testid="diagnostics-count"
          aria-label={`Show ${pluralize(props.compileDiagnostics?.length ?? 0, 'compile diagnostic')}`}
          onClick={() => props.onToggleDiagnostics?.()}
        >
          <span class={styles.pipelineLabel}>Compile</span>
          <DiagBadgeContent getSummary={compileSummary} />
        </button>
      </Show>
      <Show
        when={
          (props.tessellationDiagnostics?.length ?? 0) > 0 &&
          (props.compileDiagnostics?.length ?? 0) > 0
        }
      >
        {() => {
          const total =
            (props.tessellationDiagnostics?.length ?? 0) +
            (props.compileDiagnostics?.length ?? 0);
          return (
            <>
              <span class={styles.divider} />
              <span
                class={`${styles.section} ${styles.diagnosticsTotal}`}
                data-testid="diagnostics-total"
                aria-label={`${total} ${total === 1 ? 'diagnostic' : 'diagnostics'} total`}
              >
                {total}
              </span>
            </>
          );
        }}
      </Show>
      <span class={styles.divider} />
      <span class={styles.section}>
        <span
          class={styles.constraintCount}
          data-status="satisfied"
          title={`${constraintSummary().satisfied} satisfied`}
          aria-label={`${constraintSummary().satisfied} satisfied`}
        >
          {constraintSummary().satisfied}
        </span>
        <span
          class={styles.constraintCount}
          data-status="violated"
          title={`${constraintSummary().violated} violated`}
          aria-label={`${constraintSummary().violated} violated`}
        >
          {constraintSummary().violated}
        </span>
        <span
          class={styles.constraintCount}
          data-status="indeterminate"
          title={`${constraintSummary().indeterminate} indeterminate`}
          aria-label={`${constraintSummary().indeterminate} indeterminate`}
        >
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
