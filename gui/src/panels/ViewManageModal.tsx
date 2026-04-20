/**
 * ViewManageModal — modal dialog for managing user-created named views.
 *
 * Lists all user views (in userViewOrder) with inline rename, delete,
 * duplicate, and reorder (up/down) affordances.  Auto views are NOT listed.
 *
 * Pattern: mirrors ExportDialog.tsx — `role="dialog"`, `aria-modal="true"`,
 * focus trap on open, overlay-click close, Escape close.
 */
import { createSignal, For, Show, onMount } from 'solid-js';
import type { Component } from 'solid-js';
import type { ViewStateStore } from '../stores/viewStateStore';
import styles from './ViewManageModal.module.css';

export interface ViewManageModalProps {
  store: ViewStateStore;
  open: boolean;
  onClose: () => void;
}

const FOCUSABLE_SELECTOR =
  'button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex="-1"])';

export const ViewManageModal: Component<ViewManageModalProps> = (props) => {
  let dialogRef: HTMLDivElement | undefined;

  function setupFocusTrap(el: HTMLDivElement) {
    queueMicrotask(() => {
      const focusable = el.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR);
      if (focusable.length > 0) focusable[0].focus();
    });
  }

  function handleOverlayKeyDown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      props.onClose();
      return;
    }

    // Tab focus trap
    if (e.key === 'Tab') {
      const dialog = dialogRef;
      if (!dialog) return;
      const focusable = dialog.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR);
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (e.shiftKey) {
        if (document.activeElement === first) {
          e.preventDefault();
          last.focus();
        }
      } else {
        if (document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    }
  }

  function handleReorder(fromIndex: number, toIndex: number) {
    const order = [...props.store.state.userViewOrder];
    const [moved] = order.splice(fromIndex, 1);
    order.splice(toIndex, 0, moved);
    props.store.reorderUserViews(order);
  }

  return (
    <Show when={props.open}>
      <div
        class={styles.overlay}
        data-testid="view-manage-overlay"
        onClick={() => props.onClose()}
        onKeyDown={handleOverlayKeyDown}
      >
        <div
          ref={(el) => {
            dialogRef = el;
            setupFocusTrap(el);
          }}
          class={styles.dialog}
          role="dialog"
          aria-modal="true"
          aria-labelledby="view-manage-title"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div class={styles.header}>
            <h2 id="view-manage-title" class={styles.title}>Manage Views</h2>
            <button
              class={styles.closeBtn}
              aria-label="Close"
              onClick={() => props.onClose()}
            >
              ✕
            </button>
          </div>

          {/* Body */}
          <div class={styles.body}>
            <Show
              when={props.store.state.userViewOrder.length > 0}
              fallback={<div class={styles.empty}>No custom views yet.</div>}
            >
              <ul class={styles.viewList} role="list">
                <For each={props.store.state.userViewOrder}>
                  {(viewId, index) => {
                    const view = () => props.store.state.views[viewId];
                    const [draftName, setDraftName] = createSignal(view()?.name ?? '');

                    function commitRename() {
                      const trimmed = draftName().trim();
                      if (trimmed && trimmed !== view()?.name) {
                        props.store.renameView(viewId, trimmed);
                      }
                      // Reset draft to current (committed or rejected) name
                      setDraftName(props.store.state.views[viewId]?.name ?? '');
                    }

                    function revertRename() {
                      setDraftName(view()?.name ?? '');
                    }

                    return (
                      <li
                        class={styles.viewRow}
                        data-view-id={viewId}
                      >
                        <input
                          class={styles.nameInput}
                          type="text"
                          value={draftName()}
                          onInput={(e) => setDraftName(e.currentTarget.value)}
                          onKeyDown={(e) => {
                            if (e.key === 'Enter') {
                              e.preventDefault();
                              commitRename();
                            } else if (e.key === 'Escape') {
                              e.stopPropagation(); // prevent modal close
                              revertRename();
                            }
                          }}
                          onBlur={commitRename}
                          aria-label={`Rename view ${view()?.name ?? ''}`}
                        />
                        {/* Move up */}
                        <button
                          class={styles.actionBtn}
                          data-action="move-up"
                          disabled={index() === 0}
                          onClick={() => handleReorder(index(), index() - 1)}
                          aria-label="Move up"
                          title="Move up"
                        >
                          ↑
                        </button>
                        {/* Move down */}
                        <button
                          class={styles.actionBtn}
                          data-action="move-down"
                          disabled={index() === props.store.state.userViewOrder.length - 1}
                          onClick={() => handleReorder(index(), index() + 1)}
                          aria-label="Move down"
                          title="Move down"
                        >
                          ↓
                        </button>
                        {/* Duplicate */}
                        <button
                          class={styles.actionBtn}
                          data-action="duplicate"
                          onClick={() => props.store.duplicateView(viewId)}
                          aria-label={`Duplicate view ${view()?.name ?? ''}`}
                          title="Duplicate"
                        >
                          ⧉
                        </button>
                        {/* Delete */}
                        <button
                          class={styles.actionBtn}
                          data-action="delete"
                          onClick={() => props.store.deleteView(viewId)}
                          aria-label={`Delete view ${view()?.name ?? ''}`}
                          title="Delete"
                        >
                          ✕
                        </button>
                      </li>
                    );
                  }}
                </For>
              </ul>
            </Show>
          </div>
        </div>
      </div>
    </Show>
  );
};
