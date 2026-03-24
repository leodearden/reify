import { type Component, createSignal, Show } from 'solid-js';
import type { ToolCallInfo } from '../../types';
import { DiffView } from './DiffView';
import styles from './ToolCallCard.module.css';

export interface ToolCallCardProps {
  toolCall: ToolCallInfo;
}

function isReadTool(name: string): boolean {
  return name.startsWith('reify_get');
}

function isWriteTool(name: string): boolean {
  return name.startsWith('reify_update');
}

function toolType(name: string): 'read' | 'write' | 'other' {
  if (isReadTool(name)) return 'read';
  if (isWriteTool(name)) return 'write';
  return 'other';
}

function statusChar(status: string): string {
  switch (status) {
    case 'complete': return '✓';
    case 'error': return '✗';
    default: return '';
  }
}

function paramSummary(toolInput: Record<string, unknown>): string {
  const keys = Object.keys(toolInput);
  if (keys.length === 0) return '';
  const firstKey = keys[0];
  const firstVal = toolInput[firstKey];
  const valStr = typeof firstVal === 'string' ? firstVal : JSON.stringify(firstVal);
  const truncated = valStr && valStr.length > 30 ? valStr.slice(0, 30) + '…' : valStr;
  return `${firstKey}: ${truncated}`;
}

function resultSummary(toolCall: ToolCallInfo): string | null {
  if (toolCall.toolName === 'reify_update_source') {
    return 'View diff';
  }
  if (toolCall.toolName === 'reify_get_parameters' && Array.isArray(toolCall.result)) {
    return `${toolCall.result.length} parameters`;
  }
  return null;
}

function isSourceUpdateDiff(toolCall: ToolCallInfo): boolean {
  return toolCall.toolName === 'reify_update_source' && toolCall.status === 'complete';
}

function extractDiffBefore(toolInput: Record<string, unknown>): string {
  if (typeof toolInput.content === 'string') return toolInput.content;
  if (typeof toolInput.source === 'string') return toolInput.source;
  return '';
}

function extractDiffAfter(result: unknown): string {
  if (typeof result === 'string') return result;
  return '';
}

export const ToolCallCard: Component<ToolCallCardProps> = (props) => {
  const [expanded, setExpanded] = createSignal(false);
  const type = () => toolType(props.toolCall.toolName);
  const summary = () => resultSummary(props.toolCall);

  return (
    <div data-testid="tool-call-card" class={styles.card}>
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
        <span class={styles.icon} data-tool-type={type()}>
          {props.toolCall.toolName.charAt(0).toUpperCase()}
        </span>
        <span class={styles.toolName}>{props.toolCall.toolName}</span>
        <Show when={paramSummary(props.toolCall.toolInput)}>
          <span class={styles.paramHint}>{paramSummary(props.toolCall.toolInput)}</span>
        </Show>
        <span class={styles.spacer} />
        <Show when={summary()}>
          <span class={styles.summary}>{summary()}</span>
        </Show>
        <span class={styles.status} data-status={props.toolCall.status}>
          <Show when={props.toolCall.status === 'pending'}>
            <span class={styles.spinner} />
          </Show>
          <Show when={props.toolCall.status !== 'pending'}>
            {statusChar(props.toolCall.status)}
          </Show>
        </span>
      </div>
      <Show when={expanded()}>
        <Show
          when={isSourceUpdateDiff(props.toolCall)}
          fallback={
            <div data-testid="tool-call-details" class={styles.details}>
              <div class={styles.detailSection}>
                <div class={styles.detailLabel}>Input</div>
                <pre class={styles.json}>{JSON.stringify(props.toolCall.toolInput, null, 2)}</pre>
              </div>
              <Show when={props.toolCall.result !== undefined}>
                <div class={styles.detailSection}>
                  <div class={styles.detailLabel}>Result</div>
                  <pre class={styles.json}>{JSON.stringify(props.toolCall.result, null, 2)}</pre>
                </div>
              </Show>
            </div>
          }
        >
          <div data-testid="tool-call-details" class={styles.details}>
            <DiffView
              before={extractDiffBefore(props.toolCall.toolInput)}
              after={extractDiffAfter(props.toolCall.result)}
            />
          </div>
        </Show>
      </Show>
    </div>
  );
};
