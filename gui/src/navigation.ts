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
  flyToEntity: (entityPath: string) => void;
  selectEntity: (entityPath: string | null) => void;
}

/**
 * Navigate to an entity in the viewport.
 * Calls bridge.focusEntity, flies viewport camera, and updates selection.
 */
export async function navigateToEntity(
  entityPath: string,
  deps: NavigateToEntityDeps,
): Promise<void> {
  try {
    await deps.focusEntity(entityPath);
    deps.flyToEntity(entityPath);
    deps.selectEntity(entityPath);
  } catch (err) {
    console.error('Failed to navigate to entity:', err);
  }
}

// ── Constraint → Panels navigation ─────────────────────────────────

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
  deps.setHighlightedParams(parameter_ids);

  // Find the entity_path from the first matching value
  const matchingValue = values.find((v) => parameter_ids.includes(v.cell_id));
  deps.selectEntity(matchingValue?.entity_path ?? null);
}
