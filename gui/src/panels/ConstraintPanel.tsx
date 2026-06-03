import { type Component, createSignal, createMemo, For, Show, onCleanup } from 'solid-js';
import type { ConstraintData, ValueData } from '../types';
import styles from './ConstraintPanel.module.css';

export interface ConstraintPanelProps {
  constraints: Record<string, ConstraintData>;
  values: Record<string, ValueData>;
  onConstraintSelect?: (constraint: ConstraintData) => void;
  onAskClaude?: (context: string) => void;
}

const STATUS_PRIORITY: Record<string, number> = {
  violated: 0,
  indeterminate: 1,
  satisfied: 2,
};

function statusIcon(status: string): string {
  switch (status) {
    case 'satisfied': return '\u2713';
    case 'violated': return '\u2717';
    default: return '?';
  }
}

function statusTitle(status: string): string {
  switch (status) {
    case 'satisfied': return 'Satisfied \u2014 constraint holds';
    case 'violated': return 'Violated \u2014 constraint is not met';
    default: return 'Indeterminate \u2014 not yet evaluated';
  }
}

export const ConstraintPanel: Component<ConstraintPanelProps> = (props) => {
  const [expandedNodes, setExpandedNodes] = createSignal<Set<string>>(new Set());
  const [contextMenu, setContextMenu] = createSignal<{ constraint: ConstraintData; x: number; y: number } | null>(null);

  const sortedConstraints = createMemo(() => {
    const list = Object.values(props.constraints);
    return list.sort((a, b) => {
      const pa = STATUS_PRIORITY[a.status] ?? 1;
      const pb = STATUS_PRIORITY[b.status] ?? 1;
      return pa - pb;
    });
  });

  const isEmpty = createMemo(() => Object.keys(props.constraints).length === 0);

  function isExpandable(status: string): boolean {
    return status !== 'satisfied';
  }

  function isExpanded(nodeId: string): boolean {
    return expandedNodes().has(nodeId);
  }

  function toggleExpand(nodeId: string) {
    setExpandedNodes((prev) => {
      const next = new Set(prev);
      if (next.has(nodeId)) {
        next.delete(nodeId);
      } else {
        next.add(nodeId);
      }
      return next;
    });
  }

  function getContributingParams(paramIds: string[]): ValueData[] {
    return paramIds
      .map((id) => props.values[id])
      .filter((v): v is ValueData => v != null);
  }

  function handleContextMenu(e: MouseEvent, constraint: ConstraintData) {
    if (!props.onAskClaude) return;
    e.preventDefault();
    setContextMenu({ constraint, x: e.clientX, y: e.clientY });
    document.addEventListener('click', handleDismissMenu, { once: true });
  }

  function handleDismissMenu() {
    setContextMenu(null);
  }

  function buildContextString(constraint: ConstraintData): string {
    const params = getContributingParams(constraint.parameter_ids);
    const paramStr = params.map((p) => `${p.name}=${p.value}`).join(', ');
    let result = `Constraint: ${constraint.expression}\nStatus: ${constraint.status}`;
    if (paramStr) {
      result += `\nParameters: ${paramStr}`;
    }
    return result;
  }

  function handleAskClaude(constraint: ConstraintData) {
    const contextStr = buildContextString(constraint);
    props.onAskClaude?.(contextStr);
    setContextMenu(null);
  }

  onCleanup(() => {
    document.removeEventListener('click', handleDismissMenu);
  });

  return (
    <div data-testid="constraint-panel" class={styles.container}>
      <div class="panel-title" data-testid="panel-title-constraints">Constraints</div>
      <Show when={isEmpty()}>
        <div class={styles.emptyState}>No constraints</div>
      </Show>
      <Show when={!isEmpty()}>
        <div class={styles.list} role="list">
          <For each={sortedConstraints()}>
            {(constraint) => (
              <div
                data-testid={`constraint-row-${constraint.node_id}`}
                class={`${styles.row} ${isExpandable(constraint.status) ? styles.expandable : ''}`}
                role="listitem"
                tabindex="0"
                onContextMenu={(e: MouseEvent) => handleContextMenu(e, constraint)}
                onClick={() => {
                  props.onConstraintSelect?.(constraint);
                  if (isExpandable(constraint.status)) toggleExpand(constraint.node_id);
                }}
                onKeyDown={(e: KeyboardEvent) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    props.onConstraintSelect?.(constraint);
                    if (isExpandable(constraint.status)) toggleExpand(constraint.node_id);
                  }
                }}
              >
                <div class={styles.rowHeader}>
                  <Show when={isExpandable(constraint.status)}>
                    <span class={styles.expandIcon}>
                      {isExpanded(constraint.node_id) ? '▼' : '▶'}
                    </span>
                  </Show>
                  <span class={styles.expression}>{constraint.expression}</span>
                  <span class={styles.statusBadge} data-status={constraint.status} aria-label={constraint.status} title={statusTitle(constraint.status)}>
                    {statusIcon(constraint.status)}
                  </span>
                </div>
                <Show when={isExpanded(constraint.node_id) && isExpandable(constraint.status)}>
                  <div class={styles.details}>
                    <Show when={constraint.label}>
                      <div class={styles.detailsText}>{constraint.label}</div>
                    </Show>
                    <Show when={constraint.parameter_ids.length > 0}>
                      <div class={styles.params}>
                        <For each={getContributingParams(constraint.parameter_ids)}>
                          {(param) => (
                            <div class={styles.paramEntry}>
                              {param.name} = {param.value}
                            </div>
                          )}
                        </For>
                      </div>
                    </Show>
                  </div>
                </Show>
              </div>
            )}
          </For>
        </div>
      </Show>
      <Show when={contextMenu()}>
        {(menu) => (
          <div
            data-testid="constraint-context-menu"
            class={styles.contextMenu}
            style={{ position: 'absolute', left: `${menu().x}px`, top: `${menu().y}px` }}
          >
            <button
              class={styles.contextMenuItem}
              onClick={() => handleAskClaude(menu().constraint)}
            >
              Ask Claude about this constraint
            </button>
          </div>
        )}
      </Show>
    </div>
  );
};
