/**
 * Navigation orchestration module.
 * Pure functions that coordinate cross-panel navigation flows
 * with dependency-injected callbacks for testability.
 */
import type { SourceLocation, ConstraintData, ValueData, FileData } from './types';
import { isSameFile } from './utils/pathUtils';
import { errorMessage } from './utils/errorClassifier';

// ── Location → Editor navigation ────────────────────────────────────

/** The slice of the editor store that location navigation reads and drives. */
export interface NavigationEditorStore {
  state: {
    activeFile: string | null;
    openFiles: Pick<FileData, 'path'>[];
  };
  setActiveFile: (path: string) => void;
  openFile: (file: FileData) => void;
}

export interface NavigateToLocationDeps {
  store: NavigationEditorStore;
  openFile: (path: string) => Promise<FileData>;
  setScrollToLocation: (location: SourceLocation) => void;
  reportOpenFailure: (message: string) => void;
}

/**
 * Reveal a source location in the editor: activate its tab — opening the file
 * from disk when no tab holds it — then scroll. The scroll names the store key
 * the tab is active under, so the Editor's same-file guard accepts it whatever
 * spelling the producer used. When the file cannot be opened the failure goes
 * to `reportOpenFailure` and nothing scrolls.
 */
export async function navigateToLocation(
  location: SourceLocation,
  deps: NavigateToLocationDeps,
): Promise<void> {
  const key = await activateFile(location.file_path, deps);
  if (key) deps.setScrollToLocation({ ...location, file_path: key });
}

/** Make `path` the active tab; returns the active store key, or null if it could not be opened. */
async function activateFile(path: string, deps: NavigateToLocationDeps): Promise<string | null> {
  const { store } = deps;
  const active = store.state.activeFile;
  if (active && isSameFile(path, active)) return active;

  const open = findOpenFile(store, path);
  if (open) {
    store.setActiveFile(open.path);
    return store.state.activeFile;
  }

  let fileData: FileData;
  try {
    fileData = await deps.openFile(path);
  } catch (err) {
    deps.reportOpenFailure(`Could not open ${path}: ${errorMessage(err)}`);
    return null;
  }
  // The producer's spelling may resolve to a tab that is already open; reopening
  // it would reload a clean buffer or flag a dirty one as externally changed.
  const reopened = findOpenFile(store, fileData.path);
  if (reopened) store.setActiveFile(reopened.path);
  else store.openFile(fileData);
  return store.state.activeFile;
}

function findOpenFile(store: NavigationEditorStore, path: string): Pick<FileData, 'path'> | undefined {
  return store.state.openFiles.find((f) => isSameFile(f.path, path));
}

// ── Viewport → Source navigation ────────────────────────────────────

export interface NavigateToSourceDeps {
  getSourceLocation: (entityPath: string) => Promise<SourceLocation>;
  scrollEditor: (location: SourceLocation) => void;
  selectEntity: (entityPath: string | null) => void;
}

/**
 * Navigate from viewport selection to editor source location.
 *
 * Selection is updated unconditionally — the entity is always selected
 * regardless of whether it has a resolvable source span.
 *
 * Editor source-scroll is best-effort: it is silently skipped when
 * getSourceLocation rejects (e.g. realized/derived geometry — boolean
 * results, patterns, auto-generated sub-entities — that has no 1:1
 * source span). A console.error is logged for diagnostic purposes.
 */
export async function navigateToSource(
  entityPath: string,
  deps: NavigateToSourceDeps,
): Promise<void> {
  // selectEntity is a synchronous SolidJS store setter that cannot throw,
  // so it is intentionally outside the try — the fallible step is the
  // getSourceLocation await + scrollEditor inside the try block below.
  deps.selectEntity(entityPath);
  try {
    const location = await deps.getSourceLocation(entityPath);
    deps.scrollEditor(location);
  } catch (err) {
    console.error('Failed to navigate to source:', err);
  }
}

// ── Source/Panel → Viewport navigation ──────────────────────────────

export interface NavigateToEntityDeps {
  focusEntity: (entityPath: string) => Promise<void>;
}

/**
 * Trigger the backend focus_entity command.
 * The focus-entity event listener in App.tsx handles viewport fly + selection
 * for both MCP-originated and user-initiated (double-click) paths.
 *
 * Deliberately kept as a named function rather than inlining `bridgeFocusEntity`
 * directly at the call site: it provides a single extension seam (e.g. optimistic
 * UI, analytics, or pre-flight guards) and centralises the error-catch/log that
 * would otherwise need to be duplicated at every call site.
 */
export async function navigateToEntity(
  entityPath: string,
  deps: NavigateToEntityDeps,
): Promise<void> {
  try {
    await deps.focusEntity(entityPath);
  } catch (err) {
    console.error('Failed to navigate to entity:', err);
  }
}

// ── Constraint → Panels navigation ─────────────────────────────────

/**
 * Dependencies for navigateFromConstraint.
 *
 * ORDERING CONTRACT: `selectEntity` MUST be called before `setHighlightedParams`.
 * `selectEntity` (→ selectSingle / clearSelection) clears `highlightedParams` as
 * part of every ordinary selection change, so the highlight must be applied AFTER
 * the entity selection to ensure it survives.  Callers that provide a custom
 * `selectEntity` implementation should honour the same clearing convention, or the
 * constraint-highlight feature will silently stop working without a test failure
 * (the integration test in navigation.test.ts pins the observable invariant).
 */
export interface NavigateFromConstraintDeps {
  selectEntity: (entityPath: string | null) => void;
  setHighlightedParams: (ids: string[]) => void;
}

/**
 * Navigate from a constraint selection to highlight related parameters
 * and select the associated entity.
 */
export function navigateFromConstraint(
  constraint: ConstraintData,
  values: ValueData[],
  deps: NavigateFromConstraintDeps,
): void {
  const { parameter_ids } = constraint;

  // Select the entity FIRST so that selectEntity (→ selectSingle/clearSelection) clears
  // highlightedParams before we set the new constraint highlight.  Setting the highlight
  // LAST ensures it survives the selection-change clearing logic.
  const matchingValue = values.find((v) => parameter_ids.includes(v.cell_id));
  deps.selectEntity(matchingValue?.entity_path ?? null);

  deps.setHighlightedParams(parameter_ids);
}
