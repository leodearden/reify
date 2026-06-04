/**
 * Navigation orchestration module.
 * Pure functions that coordinate cross-panel navigation flows
 * with dependency-injected callbacks for testability.
 */
import type { SourceLocation, ConstraintData, ValueData } from './types';

// ── Viewport → Source navigation ────────────────────────────────────

export interface NavigateToSourceDeps {
  getSourceLocation: (entityPath: string) => Promise<SourceLocation>;
  scrollEditor: (location: SourceLocation) => void;
  selectEntity: (entityPath: string | null) => void;
}

/**
 * Navigate from viewport selection to editor source location.
 * Calls bridge.getSourceLocation, scrolls editor, and updates selection.
 */
export async function navigateToSource(
  entityPath: string,
  deps: NavigateToSourceDeps,
): Promise<void> {
  try {
    const location = await deps.getSourceLocation(entityPath);
    deps.scrollEditor(location);
    deps.selectEntity(entityPath);
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
