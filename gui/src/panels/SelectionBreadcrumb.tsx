import { type Component, For, Show } from 'solid-js';
import styles from './SelectionBreadcrumb.module.css';

export interface SelectionBreadcrumbProps {
  path: string | null;
}

/**
 * Renders a selected entity path as a breadcrumb trail.
 * Splits on '.' only (keeping '#realization[N]'-style suffixes on the leaf).
 * Shows a muted "No selection" placeholder when path is null/empty.
 */
export const SelectionBreadcrumb: Component<SelectionBreadcrumbProps> = (props) => {
  const segments = () => {
    const p = props.path;
    if (!p) return [];
    return p.split('.');
  };

  return (
    <div class={styles.breadcrumb} data-testid="selection-breadcrumb">
      <Show
        when={segments().length > 0}
        fallback={
          <span class={styles.placeholder}>No selection</span>
        }
      >
        <For each={segments()}>
          {(seg, idx) => {
            const isLast = () => idx() === segments().length - 1;
            return (
              <>
                <Show when={idx() > 0}>
                  <span class={styles.separator} aria-hidden="true">›</span>
                </Show>
                <span
                  class={isLast() ? styles.leaf : styles.crumb}
                  data-testid={isLast() ? 'breadcrumb-leaf' : `breadcrumb-crumb-${idx()}`}
                  data-leaf={isLast() ? 'true' : undefined}
                >
                  {seg}
                </span>
              </>
            );
          }}
        </For>
      </Show>
    </div>
  );
};
