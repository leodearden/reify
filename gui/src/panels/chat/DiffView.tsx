import { type Component, createSignal, Show, For } from 'solid-js';
import { computeUnifiedDiff } from '../../utils/diff';
import styles from './DiffView.module.css';

export interface DiffViewProps {
  before: string;
  after: string;
}

export const DiffView: Component<DiffViewProps> = (props) => {
  const [expanded, setExpanded] = createSignal(true);

  const diffLines = () => computeUnifiedDiff(props.before, props.after);
  const noChanges = () => props.before === props.after;

  return (
    <div data-testid="diff-view" class={styles.container}>
      <div
        data-testid="diff-header"
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
        <span>Source diff</span>
      </div>
      <Show when={expanded()}>
        <div data-testid="diff-content" class={styles.content}>
          <Show when={noChanges()}>
            <div class={styles.noChanges}>No changes</div>
          </Show>
          <Show when={!noChanges()}>
            <div class={styles.table}>
              <For each={diffLines()}>
                {(line, idx) => (
                  <div data-diff={line.type} class={styles.line}>
                    <span data-testid="line-number" class={styles.lineNum}>
                      {idx() + 1}
                    </span>
                    <span class={styles.prefix}>
                      {line.type === 'add' ? '+' : line.type === 'remove' ? '-' : ' '}
                    </span>
                    <span class={styles.lineContent}>{line.content}</span>
                  </div>
                )}
              </For>
            </div>
          </Show>
        </div>
      </Show>
    </div>
  );
};
