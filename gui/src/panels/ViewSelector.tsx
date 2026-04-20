/**
 * ViewSelector — dropdown for switching between auto and user-created named views.
 *
 * Auto views are listed first (in definition order), followed by user views in
 * `state.userViewOrder`.  A footer row "Organize views…" fires `onOpenManage` so
 * the parent can show the ViewManageModal.
 *
 * Pattern: mirrors MenuBar.tsx — click-outside dismiss via `onMount` mousedown
 * listener, Escape close, hover-switch highlighting, `role="menu"` on the panel.
 */
import { createSignal, onMount, onCleanup, Show, For, createMemo } from 'solid-js';
import type { Component } from 'solid-js';
import type { ViewStateStore } from '../stores/viewStateStore';
import styles from './ViewSelector.module.css';

export interface ViewSelectorProps {
  store: ViewStateStore;
  onOpenManage: () => void;
}

export const ViewSelector: Component<ViewSelectorProps> = (props) => {
  const [open, setOpen] = createSignal(false);
  let containerRef: HTMLDivElement | undefined;

  function close() {
    setOpen(false);
  }

  function toggle() {
    setOpen((v) => !v);
  }

  onMount(() => {
    function handleMouseDown(e: MouseEvent) {
      if (containerRef && e.target instanceof Node && !containerRef.contains(e.target)) {
        close();
      }
    }

    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        close();
      }
    }

    document.addEventListener('mousedown', handleMouseDown);
    document.addEventListener('keydown', handleKeyDown);

    onCleanup(() => {
      document.removeEventListener('mousedown', handleMouseDown);
      document.removeEventListener('keydown', handleKeyDown);
    });
  });

  /**
   * All views in canonical display order via the store's single source of truth.
   * Warns in dev when userViewOrder references an id absent from state.views,
   * which would indicate a transactional inconsistency in deleteView /
   * reorderUserViews.
   */
  const orderedViews = createMemo(() => {
    const ids = props.store.getOrderedViewIds();
    const result = [];
    for (const id of ids) {
      const v = props.store.state.views[id];
      if (v) {
        result.push(v);
      } else if (import.meta.env.DEV) {
        console.warn(
          `[ViewSelector] id "${id}" is in the ordered view list but missing from state.views — ` +
            'this indicates a bug in deleteView or reorderUserViews.',
        );
      }
    }
    return result;
  });

  /** Auto views in display order (auto:default pinned first). */
  const autoViews = createMemo(() => orderedViews().filter((v) => v.auto));

  /** User views in userViewOrder. */
  const userViews = createMemo(() => orderedViews().filter((v) => !v.auto));

  const activeViewName = createMemo(() => {
    const v = props.store.state.views[props.store.state.activeViewId];
    return v?.name ?? props.store.state.activeViewId;
  });

  function handleViewClick(id: string) {
    props.store.switchView(id);
    close();
  }

  function handleOrganize() {
    close();
    props.onOpenManage();
  }

  return (
    <div ref={containerRef} class={styles.container}>
      <button
        class={open() ? `${styles.trigger} ${styles.triggerOpen}` : styles.trigger}
        onClick={toggle}
        aria-haspopup="menu"
        aria-expanded={open()}
      >
        {activeViewName()}
      </button>
      <Show when={open()}>
        <div class={styles.dropdown} role="menu">
          <For each={autoViews()}>
            {(view) => (
              <button
                class={
                  view.id === props.store.state.activeViewId
                    ? `${styles.item} ${styles.itemActive}`
                    : styles.item
                }
                role="menuitem"
                onClick={() => handleViewClick(view.id)}
              >
                <span>{view.name}</span>
              </button>
            )}
          </For>
          <Show when={userViews().length > 0}>
            <hr class={styles.separator} />
            <For each={userViews()}>
              {(view) => (
                <button
                  class={
                    view.id === props.store.state.activeViewId
                      ? `${styles.item} ${styles.itemActive}`
                      : styles.item
                  }
                  role="menuitem"
                  onClick={() => handleViewClick(view.id)}
                >
                  <span>{view.name}</span>
                  <Show when={view.modified}>
                    <span class={styles.modifiedMarker} data-modified="true" aria-hidden="true" title="modified" />
                  </Show>
                </button>
              )}
            </For>
          </Show>
          <hr class={styles.separator} />
          <button
            class={`${styles.item} ${styles.footer}`}
            role="menuitem"
            onClick={handleOrganize}
          >
            Organize views…
          </button>
        </div>
      </Show>
    </div>
  );
};
