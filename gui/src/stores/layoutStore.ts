/**
 * Panel layout store. LAYOUT_DIMENSIONS is the single source of the layout's
 * keys, defaults, saved-form validation, state, setters, persistence and reset().
 */

import { createEffect, onCleanup } from 'solid-js';
import { createStore } from 'solid-js/store';
import { readStoredPanelLayout, savePanelLayout } from '../hooks/useLayoutPersistence';

type LayoutDimension =
  | { key: string; kind: 'px'; default: number; requiredInSaved?: true }
  | { key: string; kind: 'flag'; default: boolean; requiredInSaved?: true };

export const LAYOUT_DIMENSIONS = [
  { key: 'editorWidth',       kind: 'px',   default: 300, requiredInSaved: true },
  { key: 'sideWidth',         kind: 'px',   default: 300, requiredInSaved: true },
  { key: 'designTreeHeight',  kind: 'px',   default: 160 },
  { key: 'propertyHeight',    kind: 'px',   default: 200 },
  { key: 'constraintHeight',  kind: 'px',   default: 140 },
  { key: 'problemsHeight',    kind: 'px',   default: 160 },
  { key: 'problemsCollapsed', kind: 'flag', default: true },
] as const satisfies readonly LayoutDimension[];

type Row = (typeof LAYOUT_DIMENSIONS)[number];
export type LayoutKey = Row['key'];
export type PanelLayout = {
  [K in LayoutKey]: Extract<Row, { key: K }>['kind'] extends 'px' ? number : boolean;
};

type Updater<T> = T | ((prev: T) => T);
type LayoutSetters = {
  [K in LayoutKey as `set${Capitalize<K>}`]: (v: Updater<PanelLayout[K]>) => void;
};

export type LayoutStore = {
  readonly state: PanelLayout;
  set<K extends LayoutKey>(key: K, v: Updater<PanelLayout[K]>): void;
  reset(): void;
} & LayoutSetters;

const SAVED_TYPEOF = { px: 'number', flag: 'boolean' } as const;

const isRequiredInSaved = (row: LayoutDimension) => row.requiredInSaved === true;

/**
 * The saved layout's well-typed rows, or null when nothing usable is stored or
 * a requiredInSaved row is absent or mistyped. Other mistyped rows are omitted.
 */
export function loadPanelLayout(): Partial<PanelLayout> | null {
  const stored = readStoredPanelLayout();
  if (typeof stored !== 'object' || stored === null) return null;
  const saved = stored as Record<string, unknown>;

  const layout: Record<string, number | boolean> = {};
  for (const row of LAYOUT_DIMENSIONS) {
    const value = saved[row.key];
    if (typeof value === SAVED_TYPEOF[row.kind]) layout[row.key] = value as number | boolean;
    else if (isRequiredInSaved(row)) return null;
  }
  return layout as Partial<PanelLayout>;
}

function defaultLayout(): PanelLayout {
  return Object.fromEntries(LAYOUT_DIMENSIONS.map((row) => [row.key, row.default])) as PanelLayout;
}

const SAVE_DEBOUNCE_MS = 300;

export function createLayoutStore(): LayoutStore {
  const [state, setState] = createStore<PanelLayout>({ ...defaultLayout(), ...loadPanelLayout() });

  const set: LayoutStore['set'] = (key, v) => setState(key, v);
  const setterFor = <K extends LayoutKey>(key: K) => (v: Updater<PanelLayout[K]>) => set(key, v);
  const namedSetters = Object.fromEntries(
    LAYOUT_DIMENSIONS.map((row) => [
      `set${row.key[0].toUpperCase()}${row.key.slice(1)}`,
      setterFor(row.key),
    ]),
  ) as LayoutSetters;

  const reset = () => setState(defaultLayout());

  let saveTimeout: ReturnType<typeof setTimeout> | undefined;

  createEffect(() => {
    const snapshot = Object.fromEntries(LAYOUT_DIMENSIONS.map((row) => [row.key, state[row.key]]));
    clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => savePanelLayout(snapshot), SAVE_DEBOUNCE_MS);
  });

  onCleanup(() => clearTimeout(saveTimeout));

  return { state, set, reset, ...namedSetters };
}
