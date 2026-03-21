import { type Component, createSignal, createMemo, For, Show } from 'solid-js';
import type { ValueData } from '../types';
import styles from './PropertyEditor.module.css';

export interface PropertyEditorProps {
  values: Record<string, ValueData>;
  selectedEntity: string | null;
  onSetParameter: (cellId: string, value: string) => void;
  onGroupDoubleClick?: (entityPath: string) => void;
  highlightedParams?: string[];
}

/** Group values by the first dot-separated segment of entity_path. */
function groupByEntity(values: Record<string, ValueData>): Record<string, ValueData[]> {
  const groups: Record<string, ValueData[]> = {};
  for (const v of Object.values(values)) {
    const dotIdx = v.entity_path.indexOf('.');
    const groupName = dotIdx >= 0 ? v.entity_path.substring(0, dotIdx) : v.entity_path;
    if (!groups[groupName]) {
      groups[groupName] = [];
    }
    groups[groupName].push(v);
  }
  return groups;
}

export const PropertyEditor: Component<PropertyEditorProps> = (props) => {
  const [filterText, setFilterText] = createSignal('');
  const [collapsedGroups, setCollapsedGroups] = createSignal<Set<string>>(new Set());
  const [editingCellId, setEditingCellId] = createSignal<string | null>(null);
  const [editValue, setEditValue] = createSignal('');
  let escapingRef = false;

  const filteredGroups = createMemo(() => {
    const filter = filterText().toLowerCase();
    const allGroups = groupByEntity(props.values);
    const result: Record<string, ValueData[]> = {};

    for (const [groupName, values] of Object.entries(allGroups)) {
      const filtered = filter
        ? values.filter((v) => v.name.toLowerCase().includes(filter))
        : values;
      if (filtered.length > 0) {
        result[groupName] = filtered;
      }
    }
    return result;
  });

  const groupNames = createMemo(() => Object.keys(filteredGroups()).sort());

  const isEmpty = createMemo(() => Object.keys(filteredGroups()).length === 0);

  function toggleGroup(name: string) {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(name)) {
        next.delete(name);
      } else {
        next.add(name);
      }
      return next;
    });
  }

  function entityMatchesGroup(entity: string, groupName: string): boolean {
    return entity === groupName || entity.startsWith(groupName + '.');
  }

  function isGroupCollapsed(name: string): boolean {
    // If this group matches selectedEntity, force-expand it
    if (props.selectedEntity && entityMatchesGroup(props.selectedEntity, name)) {
      return false;
    }
    return collapsedGroups().has(name);
  }

  function isGroupSelected(name: string): boolean {
    return props.selectedEntity !== null && entityMatchesGroup(props.selectedEntity, name);
  }

  function handleFocus(cellId: string, e: FocusEvent) {
    const input = e.target as HTMLInputElement;
    setEditingCellId(cellId);
    setEditValue(input.value);
  }

  function handleInput(cellId: string, e: InputEvent) {
    const input = e.target as HTMLInputElement;
    setEditValue(input.value);
  }

  function isValidValue(value: string): boolean {
    return value.trim() !== '' && !isNaN(parseFloat(value));
  }

  function handleKeyDown(cellId: string, e: KeyboardEvent) {
    if (e.key === 'Enter') {
      const input = e.target as HTMLInputElement;
      if (!isValidValue(input.value)) {
        input.setAttribute('data-invalid', '');
        return;
      }
      input.removeAttribute('data-invalid');
      props.onSetParameter(cellId, input.value);
      setEditingCellId(null);
      escapingRef = true;
      input.blur();
      escapingRef = false;
    } else if (e.key === 'Escape') {
      const input = e.target as HTMLInputElement;
      // Find the original prop value for this cell
      const propValue = props.values[cellId]?.value ?? '';
      input.value = propValue;
      setEditValue(propValue);
      setEditingCellId(null);
      escapingRef = true;
      input.blur();
      escapingRef = false;
    }
  }

  function handleBlur(cellId: string, e: FocusEvent) {
    if (escapingRef) return;
    const input = e.target as HTMLInputElement;
    if (!isValidValue(input.value)) {
      // Revert to prop value on blur with invalid input
      const propValue = props.values[cellId]?.value ?? '';
      input.value = propValue;
      input.removeAttribute('data-invalid');
    } else {
      input.removeAttribute('data-invalid');
      props.onSetParameter(cellId, input.value);
    }
    setEditingCellId(null);
  }

  return (
    <div data-testid="property-editor" class={styles.container}>
      <input
        type="text"
        placeholder="Filter properties..."
        class={styles.filterInput}
        aria-label="Filter properties"
        value={filterText()}
        onInput={(e) => setFilterText(e.currentTarget.value)}
      />
      <Show when={isEmpty()}>
        <div class={styles.emptyState}>No properties</div>
      </Show>
      <Show when={!isEmpty()}>
        <div class={styles.groups} role="tree">
          <For each={groupNames()}>
            {(groupName) => (
              <div
                class={`${styles.group} ${isGroupSelected(groupName) ? styles.selected : ''}`}
                data-selected={isGroupSelected(groupName) || undefined}
                role="treeitem"
              >
                <button
                  class={styles.groupHeader}
                  onClick={() => toggleGroup(groupName)}
                  onDblClick={() => props.onGroupDoubleClick?.(groupName)}
                  aria-expanded={!isGroupCollapsed(groupName)}
                >
                  <span class={styles.collapseIcon}>
                    {isGroupCollapsed(groupName) ? '▶' : '▼'}
                  </span>
                  {groupName}
                </button>
                <Show when={!isGroupCollapsed(groupName)}>
                  <div class={styles.groupBody} role="group">
                    <For each={filteredGroups()[groupName]}>
                      {(val) => (
                        <div class={styles.row} data-testid={`prop-row-${val.cell_id}`} data-highlighted={props.highlightedParams?.includes(val.cell_id) || undefined}>
                          <span class={styles.paramName}>{val.name}</span>
                          <Show
                            when={val.determinacy === 'determined'}
                            fallback={
                              <span class={styles.valueReadonly}>{val.value}</span>
                            }
                          >
                            <input
                              type="text"
                              class={styles.valueInput}
                              value={editingCellId() === val.cell_id ? editValue() : val.value}
                              onFocus={(e) => handleFocus(val.cell_id, e)}
                              onInput={(e) => handleInput(val.cell_id, e)}
                              onKeyDown={(e) => handleKeyDown(val.cell_id, e)}
                              onBlur={(e) => handleBlur(val.cell_id, e)}
                            />
                          </Show>
                          <Show when={val.unit}>
                            <span class={styles.unitBadge}>{val.unit}</span>
                          </Show>
                          <span
                            class={styles.determinacyBadge}
                            data-determinacy={val.determinacy}
                          >
                            {val.determinacy}
                          </span>
                        </div>
                      )}
                    </For>
                  </div>
                </Show>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};
