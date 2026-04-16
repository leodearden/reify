import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from 'solid-js';
import type { Component } from 'solid-js';
import { DesignTreeContextMenu } from './DesignTreeContextMenu';
import type { MenuAction } from './DesignTreeContextMenu';
import { createViewStateStore } from '../stores/viewStateStore';
import type { EntityTreeNode } from '../types';
import styles from './DesignTree.module.css';

interface Props {
  tree: EntityTreeNode[];
  viewStateStore: ReturnType<typeof createViewStateStore>;
  selectedEntity?: string | null;
  onSelect?: (path: string) => void;
}

interface MenuState {
  path: string;
  x: number;
  y: number;
}

function nodeName(entityPath: string): string {
  const parts = entityPath.split('.');
  return parts[parts.length - 1];
}

const DesignTree: Component<Props> = (props) => {
  const [expanded, setExpanded] = createSignal<Set<string>>(new Set());
  const [menu, setMenu] = createSignal<MenuState | null>(null);

  createEffect(() => {
    props.viewStateStore.setTree(props.tree);
  });

  function toggleExpand(path: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }

  function openMenu(path: string, e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ path, x: e.clientX, y: e.clientY });
  }

  function handleAction(action: MenuAction, path: string) {
    const vs = props.viewStateStore;
    switch (action) {
      case 'show-cascade':    vs.setVisibility(path, 'show', true); break;
      case 'ghost-cascade':   vs.setVisibility(path, 'ghost', true); break;
      case 'hide-cascade':    vs.setVisibility(path, 'hidden', true); break;
      case 'show-only':       vs.showOnly(path, true); break;
      case 'reset':           vs.resetToInherit(path); break;
      case 'show-only-no-cascade': vs.showOnly(path, false); break;
    }
    setMenu(null);
  }

  function handleKeyDown(e: KeyboardEvent) {
    const selected = props.selectedEntity;
    if (!selected) return;
    // Don't steal browser/OS shortcuts (Ctrl+S = save, etc.)
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const vs = props.viewStateStore;
    switch (e.key.toLowerCase()) {
      case 'h': e.preventDefault(); vs.setVisibility(selected, 'hidden', true); break;
      case 'g': e.preventDefault(); vs.setVisibility(selected, 'ghost', true); break;
      case 's':
      case 'enter': e.preventDefault(); vs.setVisibility(selected, 'show', true); break;
    }
  }

  onMount(() => {
    function handleDocumentClick() {
      if (menu()) setMenu(null);
    }
    document.addEventListener('click', handleDocumentClick);
    onCleanup(() => document.removeEventListener('click', handleDocumentClick));
  });

  const renderNode = (node: EntityTreeNode, depth = 0) => {
    // Memoized accessor — tracks reactive reads inside getEffectiveVisibility
    // so aria-label and glyph update whenever explicit/inherited state changes.
    const eff = createMemo(() => props.viewStateStore.getEffectiveVisibility(node.entity_path));
    return (
      <div class={styles.nodeWrapper} style={{ 'padding-left': `${depth * 16}px` }}>
        <div
          class={styles.row}
          data-testid={`tree-row-${node.entity_path}`}
          onContextMenu={(e) => openMenu(node.entity_path, e)}
          onClick={() => props.onSelect?.(node.entity_path)}
        >
          <Show when={node.children.length > 0} fallback={<span class={styles.chevronPlaceholder} />}>
            <button
              class={styles.chevron}
              data-testid={`chevron-${node.entity_path}`}
              onClick={(e) => { e.stopPropagation(); toggleExpand(node.entity_path); }}
              aria-expanded={expanded().has(node.entity_path)}
            >
              {expanded().has(node.entity_path) ? '▾' : '▸'}
            </button>
          </Show>
          <span class={styles.name}>{nodeName(node.entity_path)}</span>
          <Show when={node.type_name}>
            <span class={styles.typeName}>{node.type_name}</span>
          </Show>
          <button
            class={styles.eyeIcon}
            data-testid={`eye-icon-${node.entity_path}`}
            aria-label={eff()}
            onClick={(e) => { e.stopPropagation(); props.viewStateStore.cycleCascading(node.entity_path); }}
          >
            {eff() === 'show' ? '👁' : eff() === 'ghost' ? '◑' : '○'}
          </button>
        </div>
        <Show when={expanded().has(node.entity_path)}>
          <For each={node.children}>
            {(child) => renderNode(child, depth + 1)}
          </For>
        </Show>
      </div>
    );
  };

  return (
    <div
      class={styles.tree}
      data-testid="design-tree"
      tabindex="0"
      onKeyDown={handleKeyDown}
    >
      <For each={props.tree}>
        {(node) => renderNode(node)}
      </For>
      <Show when={menu()}>
        {(m) => (
          <DesignTreeContextMenu
            entityPath={m().path}
            x={m().x}
            y={m().y}
            onAction={handleAction}
          />
        )}
      </Show>
    </div>
  );
};

export { DesignTree };
