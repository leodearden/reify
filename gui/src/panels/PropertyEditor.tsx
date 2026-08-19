import { type Component, createSignal, createMemo, createEffect, For, Show } from 'solid-js';
import type { UnitLadderMap, UnitOption, ValueData } from '../types';
import styles from './PropertyEditor.module.css';
import { SelectionBreadcrumb } from './SelectionBreadcrumb';
import {
  buildQuantityRe,
  convertToUnit,
  formatDisplayNumber,
  ladderForDimension,
  normalizeUnitLabel,
  NUMBER_RE,
  quantityUnitAlphabet,
} from '../stores/unitLadder';
import { loadAllUnitPreferences, pruneUnitPreferences, saveUnitPreference } from '../stores/unitPreferences';

/**
 * Return a short glyph for the non-Final freshness variants.
 * Intermediate: "⟳" (in-progress); Pending: "⚠" (upstream blocked); Failed: "✕" (error).
 * Final is never passed here (the Show guard filters it out).
 */
function freshnessGlyph(freshness: string): string {
  switch (freshness) {
    case 'intermediate': return '⟳';
    case 'pending': return '⚠';
    case 'failed': return '✕';
    default: return freshness;
  }
}

/**
 * The value to DISPLAY for a cell (task #4739 γ). For a demand-pruned
 * (`freshness === 'pending'`) cell carrying a `last_substantive_value`, show
 * that prior good value instead of the current un-recomputed `value` — so the
 * displayed number equals the last good one (arch §8 prune-safety scenario 3).
 * Otherwise (final/intermediate/failed, or no prior value) show `value` as-is.
 */
function displayValue(val: ValueData): string {
  if (val.freshness === 'pending' && val.last_substantive_value != null) {
    return val.last_substantive_value;
  }
  return val.value;
}

export interface PropertyEditorProps {
  values: Record<string, ValueData>;
  selectedEntity: string | null;
  onSetParameter: (cellId: string, value: string) => void;
  onGroupDoubleClick?: (entityPath: string) => void;
  highlightedParams?: string[];
  /** Per-dimension display-unit ladders (task #5199), fetched once via `get_unit_ladders`. */
  unitLadders?: UnitLadderMap;
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

// Both halves of the accepted input grammar — the bare number (`NUMBER_RE`)
// and the number-plus-unit quantity (`buildQuantityRe`) — are defined ONCE, in
// `../stores/unitLadder`, and share their numeric form. No whitespace is
// allowed between number and unit, matching the .ri grammar (token.immediate);
// the backend parse_value_string is more lenient (accepts "5 mm") but the
// frontend intentionally enforces the stricter rule.

export const PropertyEditor: Component<PropertyEditorProps> = (props) => {
  const [filterText, setFilterText] = createSignal('');
  const [collapsedGroups, setCollapsedGroups] = createSignal<Set<string>>(new Set());
  const [editingCellId, setEditingCellId] = createSignal<string | null>(null);
  const [editValue, setEditValue] = createSignal('');
  let escapingRef = false;

  // Per-cell display-unit picker (task #5199). Keyed by cell_id in a single
  // signal (rather than one signal per row) so a picked unit survives the
  // <For> row potentially being recreated on an unrelated values update.
  const [selectedUnits, setSelectedUnits] = createSignal<Record<string, string>>({});

  // Persisted per-cell unit preferences, parsed ONCE at mount rather than
  // per-cell-per-render (task #5199 amend: `chosenOptionFor` previously
  // called `loadUnitPreference` — a fresh localStorage.getItem + JSON.parse
  // of the whole blob — on every invocation, at least twice per
  // picker-enabled row per render since it's read from both
  // `primaryDisplay`/`displayForPicker` and the `<select value={...}>`
  // binding). Any pick made THIS session goes through `selectedUnits`
  // (checked first below, and always up to date), so this snapshot only
  // needs to cover preferences saved before mount.
  //
  // Mount-scoped assumption (task #5199 amend, reviewer_comprehensive
  // reactivity finding): this only stays correct because PropertyEditor is a
  // single, non-keyed, long-lived component instance for the whole App
  // session — `gui/src/App.tsx`'s one `<PropertyEditor>` render site has no
  // `key` and no `<Show>`/conditional ancestor, so file loads update
  // `props.values` reactively rather than unmounting/remounting this
  // component (SolidJS component bodies run once per instance, unlike
  // React). If a future caller renders more than one PropertyEditor
  // instance, or starts remounting this one per file load, a preference
  // saved through a DIFFERENT instance/mount after this snapshot was taken
  // will NOT be reflected here until this instance itself remounts —
  // in-session picks made through THIS instance are unaffected, since those
  // always go through the always-fresh `selectedUnits` signal above.
  const persistedUnits = loadAllUnitPreferences();

  /**
   * The typed-quantity gate (task #6028), resolved PER CELL: the accepted unit
   * alphabet is the static floor unioned with just the ladder the cell's own
   * dimension advertises — so a Length cell takes the Length rungs and refuses
   * a Density literal, and a Volume cell the reverse. Before this it was a
   * hard-coded five-unit alternation that rejected every unit the backend
   * supports beyond those five; widening it to the union over ALL dimensions
   * would instead have let any cell accept any other dimension's literal, which
   * the backend then refuses on the worst-feedback path (see below).
   *
   * Cells with no dimension, and dimensions the ladder map does not cover,
   * fall back to the union — the non-narrowing choice, and byte-identical to
   * the pre-#6028 behaviour whenever the ladders are absent.
   *
   * Memoized so the alphabets and their regexes are rebuilt only when the
   * ladder map changes; the returned resolver caches per dimension, so a panel
   * of N cells over K dimensions builds at most K+1 regexes.
   *
   * ACCEPTED DEGRADATION: this gate still admits more than the commit path can
   * parse, because the ladders are the curated DISPLAY table while the commit
   * path has its own hard-coded suffix table. Per-cell scoping shrinks that
   * gap; it does not close it. The full account — cause, user-visible shape,
   * why it is accepted, and who owns the fix — is on `quantityUnitAlphabet` in
   * `../stores/unitLadder`, and it is pinned end-to-end by the `App.test.tsx`
   * block "ladder-derived units the backend cannot parse".
   */
  const quantityReFor = createMemo(() => {
    const ladders = props.unitLadders;
    const unionRe = buildQuantityRe(quantityUnitAlphabet(ladders));
    const byDimension = new Map<string, RegExp>();
    return (dimension: string | undefined): RegExp => {
      if (dimension === undefined) return unionRe;
      const cached = byDimension.get(dimension);
      if (cached !== undefined) return cached;
      const ladder = ladderForDimension(ladders ?? {}, dimension);
      if (ladder === undefined) return unionRe;
      const re = buildQuantityRe(quantityUnitAlphabet({ [dimension]: ladder }));
      byDimension.set(dimension, re);
      return re;
    };
  });

  /**
   * The selectable unit ladder for a cell, or `undefined` when the cell has
   * no dimension/si_value, its dimension has no ladder (<2 options), or the
   * cell is demand-pruned and showing a prior good value — in which case the
   * caller falls back to the static unit badge.
   */
  function pickerLadder(val: ValueData): UnitOption[] | undefined {
    // A demand-pruned (`freshness === 'pending'`) cell with a
    // `last_substantive_value` displays that STRING, formatted in the
    // default unit, with no SI magnitude of its own (task #4739 γ never
    // needed one). Converting to a non-default unit would have to use the
    // cell's live `si_value` instead — a different, possibly stale,
    // un-recomputed number — so the displayed magnitude would silently flip
    // between the last-good value (default unit) and a stale value (any
    // other unit) for the same cell (task #5199 amend, reviewer_comprehensive
    // correctness finding). Suppress the picker entirely for these cells.
    if (val.freshness === 'pending' && val.last_substantive_value != null) return undefined;
    if (!val.dimension || val.si_value == null) return undefined;
    const ladder = ladderForDimension(props.unitLadders ?? {}, val.dimension);
    if (!ladder || ladder.length < 2) return undefined;
    return ladder;
  }

  /**
   * The currently chosen unit option for a cell: in-session pick, else
   * persisted, else the ladder default.
   *
   * Resolution is exact-match first, then a match on the ASCII normal form of
   * BOTH sides (task #6028). Without that second attempt, task #5788's relabel
   * of the curated tables (`m³` -> `m^3`) silently invalidates every stored
   * preference written in the old spelling: the lookup misses and the cell
   * snaps back to the default rung, changing the displayed magnitude with no
   * user action and no notice.
   *
   * WHY at COMPARISON time rather than as a one-way rewrite of the persisted
   * blob (the obvious "migration"): a stored-form rewrite is ORDER-DEPENDENT.
   * Landing before #5788 — as this does — rewriting a stored `m³` to `m^3`
   * would stop it matching the still-superscript live ladder, inflicting the
   * exact harm being removed, only earlier. Normalizing both sides at
   * comparison time resolves in both eras and in both directions, so #6028 and
   * #5788 may land in either order. `gui/src/stores/unitPreferences.ts` is
   * therefore deliberately untouched: localStorage keeps the verbatim label.
   *
   * A one-time user-facing notice was also considered and rejected:
   * disproportionate UI for a pure relabel that preserves rung identity, and
   * moot in any case — with this fix there is no reset to announce.
   *
   * Note the fallback is a NORMALIZED equality, not a fuzzy one: a genuinely
   * unknown label still falls through to the `is_default` rung as before.
   */
  function chosenOptionFor(val: ValueData, ladder: UnitOption[]): UnitOption {
    const label = selectedUnits()[val.cell_id] ?? persistedUnits[val.cell_id] ?? undefined;
    let found = label !== undefined ? ladder.find((u) => u.label === label) : undefined;
    if (found === undefined && typeof label === 'string') {
      // Both sides are typed `string` but neither is guaranteed at runtime:
      // `label` is parsed out of localStorage and `u.label` arrives over IPC.
      // The exact-equality attempt above tolerates a non-string on either side
      // (it just misses); the normalized attempt would throw, so it is guarded
      // rather than trading a silent miss for a render-time crash.
      const normalized = normalizeUnitLabel(label);
      found = ladder.find((u) => typeof u.label === 'string' && normalizeUnitLabel(u.label) === normalized);
    }
    return found ?? ladder.find((u) => u.is_default) ?? ladder[0];
  }

  /** The magnitude to display for the picker: backend value verbatim at the default unit, else converted. */
  function displayForPicker(val: ValueData, ladder: UnitOption[]): string {
    const chosen = chosenOptionFor(val, ladder);
    if (chosen.is_default) return displayValue(val);
    // `pickerLadder` only ever hands back a ladder when `val.si_value !=
    // null`, and this function is only called with a ladder it returned —
    // so si_value is guaranteed non-null here. Assert instead of falling
    // back to `?? 0`: a `0` fallback would silently render a plausible-looking
    // value instead of surfacing the bug if that invariant were ever violated
    // (task #5199 amend, reviewer_comprehensive robustness finding).
    if (val.si_value == null) {
      throw new Error(
        `displayForPicker: cell ${val.cell_id} has an active unit ladder but no si_value`,
      );
    }
    return formatDisplayNumber(convertToUnit(val.si_value, chosen.si_scale));
  }

  /**
   * The magnitude to show in the row's primary value slot — the editable
   * input at rest and the read-only fallback span: the picker-converted
   * magnitude when the cell has an active unit ladder, else the canonical
   * `displayValue` (task #5199 amend). Previously only the badge's number
   * reflected the picked unit while this slot always showed the canonical
   * magnitude, so the row displayed two different numbers for one cell.
   */
  function primaryDisplay(val: ValueData): string {
    const ladder = pickerLadder(val);
    return ladder ? displayForPicker(val, ladder) : displayValue(val);
  }

  /**
   * The ladder's default (canonical) unit label, or `undefined` when the
   * cell has no active picker ladder.
   */
  function defaultUnitLabel(val: ValueData): string | undefined {
    const ladder = pickerLadder(val);
    return ladder?.find((u) => u.is_default)?.label ?? ladder?.[0]?.label;
  }

  /**
   * Whether this cell is CURRENTLY being edited while its picker is showing
   * a non-default unit (task #5199 amend, reviewer_comprehensive robustness
   * finding). Editing always operates on the canonical/default-unit
   * magnitude (see `handleFocus`) so an unmodified commit is a true no-op,
   * but the `<select>` keeps showing whichever unit was picked for at-rest
   * display — e.g. a Length cell displayed in "in" still reads "in" in the
   * picker while the input has just silently switched to the "mm" magnitude
   * underneath. Drives an explicit on-screen hint (below) instead of leaving
   * that switch silent: a user who did not notice it and types a fresh
   * number would otherwise believe they are entering a value in the picked
   * unit, when it is actually committed in the canonical unit.
   */
  function editingInDifferentUnit(val: ValueData): boolean {
    if (editingCellId() !== val.cell_id) return false;
    const ladder = pickerLadder(val);
    if (!ladder) return false;
    return !chosenOptionFor(val, ladder).is_default;
  }

  function handleUnitChange(cellId: string, label: string) {
    saveUnitPreference(cellId, label);
    setSelectedUnits((prev) => ({ ...prev, [cellId]: label }));
  }

  // Garbage-collect unit preferences for cells no longer present in
  // props.values — e.g. a file reload, or a member edited/removed (task
  // #5199 amend, reviewer_comprehensive resource_cleanup finding: both the
  // in-memory `selectedUnits` record and the persisted localStorage blob
  // otherwise only ever grow, accumulating one entry per cell_id ever seen
  // across every file opened in the session). Runs whenever the live
  // cell_id set changes; a no-op write-wise when nothing is stale.
  createEffect(() => {
    const liveCellIds = Object.keys(props.values);
    pruneUnitPreferences(liveCellIds);
    const valid = new Set(liveCellIds);
    setSelectedUnits((prev) => {
      let changed = false;
      const next: Record<string, string> = {};
      for (const [cellId, label] of Object.entries(prev)) {
        if (valid.has(cellId)) {
          next[cellId] = label;
        } else {
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  });

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
    setEditingCellId(cellId);
    // Seed the edit buffer from the canonical backend value, not whatever is
    // currently on screen: when a non-default unit is picked, the at-rest
    // display is `primaryDisplay` — a magnitude in a DIFFERENT unit than
    // `onSetParameter` expects on submit. Editing always operates in
    // canonical units so an unmodified commit is a true no-op instead of
    // silently rewriting the value by the picked unit's conversion factor
    // (task #5199 amend).
    const val = props.values[cellId];
    setEditValue(val ? displayValue(val) : (e.target as HTMLInputElement).value);
  }

  function handleInput(cellId: string, e: InputEvent) {
    const input = e.target as HTMLInputElement;
    setEditValue(input.value);
  }

  /**
   * Whether `value` is an acceptable literal for the cell `cellId`. The cell is
   * load-bearing, not decoration: the accepted unit alphabet is scoped to that
   * cell's dimension (see `quantityReFor`).
   */
  function isValidValue(value: string, cellId: string): boolean {
    if (value === '') return false;
    // NUMBER_RE gates bare numeric literals; isFinite catches overflow (e.g. 1e999 → Infinity)
    if (NUMBER_RE.test(value) && Number.isFinite(Number(value))) return true;
    // Group 1 is the whole signed numeric literal, so the overflow check reads
    // it directly — no second regex re-declaring the unit alternation to strip
    // the suffix, and so nothing left to keep in sync.
    const m = quantityReFor()(props.values[cellId]?.dimension).exec(value);
    if (m) return Number.isFinite(Number(m[1]));
    return false;
  }

  /** Trim, validate, submit. Returns true on success. */
  function submitValue(cellId: string, rawValue: string, input: HTMLInputElement): boolean {
    const trimmed = rawValue.trim();
    if (!isValidValue(trimmed, cellId)) {
      return false;
    }
    input.removeAttribute('data-invalid');
    props.onSetParameter(cellId, trimmed);
    setEditingCellId(null);
    return true;
  }

  function handleKeyDown(cellId: string, e: KeyboardEvent) {
    if (e.key === 'Enter') {
      const input = e.target as HTMLInputElement;
      if (!submitValue(cellId, input.value, input)) {
        input.setAttribute('data-invalid', '');
        return;
      }
      escapingRef = true;
      input.blur();
      escapingRef = false;
    } else if (e.key === 'Escape') {
      const input = e.target as HTMLInputElement;
      // Find the original prop value for this cell
      const propValue = props.values[cellId]?.value ?? '';
      input.removeAttribute('data-invalid');
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
    if (!submitValue(cellId, input.value, input)) {
      // Revert to prop value on blur with invalid input
      const propValue = props.values[cellId]?.value ?? '';
      input.value = propValue;
      input.removeAttribute('data-invalid');
      setEditingCellId(null);
    }
  }

  return (
    <div data-testid="property-editor" class={styles.container}>
      <div class="panel-title" data-testid="panel-title-parameters">Parameters</div>
      <SelectionBreadcrumb path={props.selectedEntity} />
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
                              <span class={styles.valueReadonly}>{primaryDisplay(val)}</span>
                            }
                          >
                            <input
                              type="text"
                              class={styles.valueInput}
                              value={editingCellId() === val.cell_id ? editValue() : primaryDisplay(val)}
                              title={primaryDisplay(val)}
                              onFocus={(e) => handleFocus(val.cell_id, e)}
                              onInput={(e) => handleInput(val.cell_id, e)}
                              onKeyDown={(e) => handleKeyDown(val.cell_id, e)}
                              onBlur={(e) => handleBlur(val.cell_id, e)}
                            />
                          </Show>
                          <Show when={editingInDifferentUnit(val)}>
                            <span
                              class={styles.unitEditHint}
                              data-testid={`unit-edit-hint-${val.cell_id}`}
                              title="Editing always uses the canonical unit, regardless of the unit picked for display."
                            >
                              ⚠ editing in {defaultUnitLabel(val)}
                            </span>
                          </Show>
                          <Show
                            when={pickerLadder(val)}
                            fallback={
                              <Show when={val.unit}>
                                <span class={styles.unitBadge}>{val.unit}</span>
                              </Show>
                            }
                          >
                            {(ladder) => (
                              <span class={styles.unitBadge}>
                                <select
                                  aria-label={`unit for ${val.name}`}
                                  data-testid={`unit-select-${val.cell_id}`}
                                  value={chosenOptionFor(val, ladder()).label}
                                  onChange={(e) => handleUnitChange(val.cell_id, e.currentTarget.value)}
                                >
                                  <For each={ladder()}>
                                    {(opt) => <option value={opt.label}>{opt.label}</option>}
                                  </For>
                                </select>
                              </span>
                            )}
                          </Show>
                          <span
                            class={styles.determinacyBadge}
                            data-determinacy={val.determinacy}
                          >
                            {val.determinacy}
                          </span>
                          <Show when={val.freshness !== 'final'}>
                            <span
                              class={styles.freshnessBadge}
                              data-freshness={val.freshness}
                              data-testid={`freshness-badge-${val.cell_id}`}
                              aria-label={`freshness ${val.freshness}`}
                              title={`freshness ${val.freshness}`}
                            >
                              {freshnessGlyph(val.freshness)}
                            </span>
                          </Show>
                          <Show when={val.reason}>
                            <span
                              class={styles.undefReason}
                              data-testid={`undef-reason-${val.cell_id}`}
                              title={`undef because: ${val.reason}`}
                            >
                              {val.reason}
                            </span>
                          </Show>
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
