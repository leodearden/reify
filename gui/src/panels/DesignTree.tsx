import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from 'solid-js';
import type { Component } from 'solid-js';
import { DesignTreeContextMenu } from './DesignTreeContextMenu';
import type { MenuAction } from './DesignTreeContextMenu';
import { ViewSelector } from './ViewSelector';
import type { ViewStateStore } from '../stores/viewStateStore';
import type { EntityTreeNode } from '../types';
import styles from './DesignTree.module.css';

interface Props {
  tree: EntityTreeNode[];
  viewStateStore: ViewStateStore;
  selectedEntity?: string | null;
  selectedEntities?: readonly string[];
  anchorEntity?: string | null;
  onSelect?: (path: string, modifiers: { ctrl: boolean; shift: boolean }) => void;
  onRangeSelect?: (paths: string[]) => void;
  onSelectAll?: (paths: string[]) => void;
  onOpenManage?: () => void;
  /** Optional callback for "Save views" action; forwarded to ViewSelector. */
  onSaveViews?: () => void;
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

/** Display label for a tree row: prefer `display_name` (set by backend for
 *  realization nodes — carries the original binding name like `"body"`),
 *  otherwise fall back to the last dot-segment of `entity_path`. */
function displayLabel(node: EntityTreeNode): string {
  return node.display_name ?? nodeName(node.entity_path);
}

/** DFS flatten: returns all visible entity paths respecting the expanded set. */
function flattenVisible(nodes: EntityTreeNode[], expandedSet: Set<string>): string[] {
  const result: string[] = [];
  function visit(node: EntityTreeNode) {
    result.push(node.entity_path);
    if (expandedSet.has(node.entity_path)) {
      for (const child of node.children) visit(child);
    }
  }
  for (const node of nodes) visit(node);
  return result;
}

const DesignTree: Component<Props> = (props) => {
  const [expanded, setExpanded] = createSignal<Set<string>>(new Set());
  const [menu, setMenu] = createSignal<MenuState | null>(null);

  // Stale paths: present in explicit overrides but absent from the current tree.
  // Used to apply greyed styling to forward-compat stale-row display.
  const stalePaths = createMemo(() => new Set(props.viewStateStore.getStalePaths()));

  // Unifies single and multi-selection: prefer selectedEntities when provided,
  // else fall back to [selectedEntity] (or empty). Returns a Set for O(1) lookup.
  const effectiveSelected = createMemo((): Set<string> => {
    if (props.selectedEntities !== undefined) {
      return new Set(props.selectedEntities);
    }
    return props.selectedEntity ? new Set([props.selectedEntity]) : new Set();
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
    // Ctrl+A / Meta+A: select-all visible nodes. Must come BEFORE the
    // general ctrlKey/metaKey early-exit guard below.
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a') {
      e.preventDefault();
      props.onSelectAll?.(flattenVisible(props.tree, expanded()));
      return;
    }

    // Don't steal browser/OS shortcuts (Ctrl+S = save, etc.)
    if (e.ctrlKey || e.metaKey || e.altKey) return;

    // Apply visibility shortcuts to ALL currently-selected rows so that
    // multi-selection behaves consistently with the bulk eye-icon cycle.
    const sel = effectiveSelected();
    if (sel.size === 0) return;
    const vs = props.viewStateStore;
    let mode: 'hidden' | 'ghost' | 'show' | null = null;
    switch (e.key.toLowerCase()) {
      case 'h': mode = 'hidden'; break;
      case 'g': mode = 'ghost'; break;
      case 's':
      case 'enter': mode = 'show'; break;
    }
    if (mode !== null) {
      e.preventDefault();
      for (const path of sel) {
        vs.setVisibility(path, mode, true);
      }
    }
  }

  onMount(() => {
    function handleDocumentClick(e: MouseEvent) {
      if (!menu()) return;
      // If the click lands inside the open context menu, let the item's own
      // click handler call setMenu(null) via handleAction — don't dismiss early.
      // Uses a stable non-test data attribute so the dismiss selector isn't
      // coupled to a test-only marker.
      if ((e.target as Element).closest?.('[data-design-tree-menu]')) return;
      setMenu(null);
    }
    function handleDocumentKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape' && menu()) setMenu(null);
    }
    // Use capture phase so the dismiss fires even when a child element (e.g. chevron
    // or eye-icon button) calls stopPropagation(), which would otherwise suppress the
    // bubbled click before it reaches the document listener.
    document.addEventListener('click', handleDocumentClick, { capture: true });
    document.addEventListener('keydown', handleDocumentKeyDown);
    onCleanup(() => {
      document.removeEventListener('click', handleDocumentClick, { capture: true });
      document.removeEventListener('keydown', handleDocumentKeyDown);
    });
  });

  const renderNode = (node: EntityTreeNode, depth = 0) => {
    // Memoized accessor — tracks reactive reads inside getEffectiveVisibility
    // so aria-label and glyph update whenever explicit/inherited state changes.
    const eff = createMemo(() => props.viewStateStore.getEffectiveVisibility(node.entity_path));
    return (
      <div class={styles.nodeWrapper} style={{ 'padding-left': `${depth * 16}px` }}>
        <div
          classList={{ [styles.row]: true, [styles.selected]: effectiveSelected().has(node.entity_path), [styles.stale]: stalePaths().has(node.entity_path) }}
          data-testid={`tree-row-${node.entity_path}`}
          data-selected={effectiveSelected().has(node.entity_path) ? 'true' : undefined}
          data-stale={stalePaths().has(node.entity_path) ? 'true' : undefined}
          onContextMenu={(e) => openMenu(node.entity_path, e)}
          onClick={(e) => {
            if (e.shiftKey && props.anchorEntity && props.onRangeSelect) {
              // Shift+click with anchor: compute visible range and invoke onRangeSelect
              const visibleOrder = flattenVisible(props.tree, expanded());
              const anchorIdx = visibleOrder.indexOf(props.anchorEntity);
              const targetIdx = visibleOrder.indexOf(node.entity_path);
              if (anchorIdx !== -1 && targetIdx !== -1) {
                const lo = Math.min(anchorIdx, targetIdx);
                const hi = Math.max(anchorIdx, targetIdx);
                props.onRangeSelect(visibleOrder.slice(lo, hi + 1));
                return;
              }
            }
            props.onSelect?.(node.entity_path, { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey });
          }}
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
          <span class={styles.name}>{displayLabel(node)}</span>
          <Show when={node.type_name}>
            <span class={styles.typeName}>{node.type_name}</span>
          </Show>
          <Show when={node.freshness !== 'final' && node.freshness !== 'aggregate'}>
            <span
              class={styles.freshnessBadge}
              data-freshness={node.freshness}
              data-testid={`row-freshness-${node.entity_path}`}
              aria-label={`freshness ${node.freshness}`}
            />
          </Show>
          <button
            class={styles.eyeIcon}
            data-testid={`eye-icon-${node.entity_path}`}
            aria-label={eff()}
            onClick={(e) => {
              e.stopPropagation();
              const sel = effectiveSelected();
              // Bulk-cycle: if the clicked row is part of a multi-selection (>1),
              // cycle all selected paths. Otherwise fall back to single-path cycle.
              if (sel.size > 1 && sel.has(node.entity_path)) {
                for (const p of sel) props.viewStateStore.cycleCascading(p);
              } else {
                props.viewStateStore.cycleCascading(node.entity_path);
              }
            }}
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
      <div class="panel-title" data-testid="panel-title-outline">Outline</div>
      <Show when={props.onOpenManage !== undefined}>
        <ViewSelector
          store={props.viewStateStore}
          onOpenManage={props.onOpenManage!}
          onSaveViews={props.onSaveViews}
        />
      </Show>
      <div class={styles.treeScroll}>
        <For each={props.tree}>
          {(node) => renderNode(node)}
        </For>
      </div>
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
