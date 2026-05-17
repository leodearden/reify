/**
 * FeaCasePickerDropdown — multi-load-case FEA selector panel (task 3545, GR-016 η).
 *
 * Renders a `<select>` dropdown that lets the user choose the active load case
 * when a MultiCaseResult is present in CheckResult.values.
 *
 * Degrades gracefully: renders nothing when `availableCases` is empty
 * (the common state until task 3026 lands `solve_load_cases`).
 *
 * Per PRD §2.2 task η and docs/gui-event-channels/fea-case-changed.md.
 */

import { Show, For, createEffect, type JSX } from 'solid-js';
import type { FeaModeStore } from '../stores/feaModeStore';
import styles from './FeaCasePickerDropdown.module.css';

export interface FeaCasePickerDropdownProps {
  store: FeaModeStore;
  availableCases: string[];
}

/**
 * Dropdown for selecting the active FEA load case.
 *
 * When `availableCases` is empty (no MultiCaseResult observed), renders nothing.
 * When non-empty, renders a labelled `<select data-testid="fea-case-picker-dropdown">`.
 * The selected value is `store.state.activeCaseId` when set, falling back to
 * `availableCases[0]` as a defensive default when `activeCaseId` is null
 * (avoids a blank/hung select on first render).
 */
export function FeaCasePickerDropdown(props: FeaCasePickerDropdownProps): JSX.Element {
  // On mount and whenever availableCases/activeCaseId change: if activeCaseId is
  // null but cases are available, sync the store to availableCases[0] so the
  // visible dropdown default and the store source-of-truth stay consistent.
  // Without this, the select renders availableCases[0] via the ?? fallback but
  // store.state.activeCaseId remains null — causing a divergence when task 3026
  // wires a consumer that reads activeCaseId to drive displayed results.
  createEffect(() => {
    if (props.store.state.activeCaseId === null && props.availableCases.length > 0) {
      props.store.setActiveCaseId(props.availableCases[0]);
    }
  });

  return (
    <Show when={props.availableCases.length > 0}>
      <label class={styles.label}>
        <span class={styles.labelText}>Load case</span>
        <select
          data-testid="fea-case-picker-dropdown"
          class={styles.select}
          value={props.store.state.activeCaseId ?? props.availableCases[0]}
          onChange={(e) => props.store.setActiveCaseId(e.currentTarget.value)}
        >
          <For each={props.availableCases}>
            {(c) => <option value={c}>{c}</option>}
          </For>
        </select>
      </label>
    </Show>
  );
}
