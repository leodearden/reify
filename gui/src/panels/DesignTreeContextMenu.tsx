import type { Component } from 'solid-js';
import styles from './DesignTreeContextMenu.module.css';

export type MenuAction =
  | 'show-cascade'
  | 'ghost-cascade'
  | 'hide-cascade'
  | 'show-only'
  | 'reset'
  | 'show-only-no-cascade';

interface Props {
  entityPath: string;
  x: number;
  y: number;
  onAction: (action: MenuAction, path: string) => void;
}

export const DesignTreeContextMenu: Component<Props> = (props) => {
  const act = (action: MenuAction) => props.onAction(action, props.entityPath);

  return (
    <div
      class={styles.contextMenu}
      data-testid="design-tree-context-menu"
      style={{ position: 'fixed', left: `${props.x}px`, top: `${props.y}px` }}
    >
      <button class={styles.contextMenuItem} data-testid="ctx-show-cascade" onClick={() => act('show-cascade')}>
        Show this and children
      </button>
      <button class={styles.contextMenuItem} data-testid="ctx-ghost-cascade" onClick={() => act('ghost-cascade')}>
        Ghost this and children
      </button>
      <button class={styles.contextMenuItem} data-testid="ctx-hide-cascade" onClick={() => act('hide-cascade')}>
        Hide this and children
      </button>
      <button class={styles.contextMenuItem} data-testid="ctx-show-only" onClick={() => act('show-only')}>
        Show only this
      </button>
      <button class={styles.contextMenuItem} data-testid="ctx-reset" onClick={() => act('reset')}>
        Reset to default
      </button>
      <hr class={styles.separator} />
      <button class={styles.contextMenuItem} data-testid="ctx-show-only-no-cascade" onClick={() => act('show-only-no-cascade')}>
        Show only this (no cascade)
      </button>
    </div>
  );
};
