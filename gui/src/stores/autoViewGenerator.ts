import type { EntityTreeNode, VisibilityState } from '../types';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/**
 * A view definition — either auto-generated or user-created.
 * - `id`: Unique identifier. Auto views use prefixes `auto:default`,
 *   `auto:all-geometry`, `auto:purpose:<name>`. User views use `user:<name>`.
 * - `name`: Human-readable label displayed in the UI.
 * - `auto`: True for generated views, false for user-created views.
 * - `modified`: True if the user has made local edits to an auto view.
 *   NOTE: `regenerateAutoViews` does not yet preserve edits when `modified=true`
 *   (future work). For now auto-view edits are always transient.
 * - `visibility`: Explicit per-node visibility state keyed by `entity_path`.
 *   For auto views this is a full assignment (every node has an entry).
 *   For user views the map may be sparse; unset paths fall through to
 *   `defaultRuleFor` via walk-up.
 *
 * **Live user-view mirror**: When a `user:*` view is the active view in
 * `viewStateStore`, every mutation (`setVisibility`, `setVisibilityWithoutCascade`,
 * `resetToInherit`, `showOnly`, `cycleCascading`) automatically rebuilds
 * `visibility` from the current `state.explicit`, dropping null (inherit-cleared)
 * entries.  This means `visibility` on an active user view always equals the
 * user's live explicit choices — it is not a stale seed from `seedView`.
 * Consumers that export or restore user views can treat `visibility` as the
 * complete, up-to-date state.
 */
export interface ViewDefinition {
  id: string;
  name: string;
  auto: boolean;
  modified: boolean;
  visibility: Record<string, VisibilityState>;
}

// ---------------------------------------------------------------------------
// Shared visibility rule
// ---------------------------------------------------------------------------

/**
 * Returns true when a node is a let-binding whose type is a geometry
 * intermediate (Solid, Surface, or Curve).  Used by both `defaultVisibilityFor`
 * and `manufacturingReadyVisibilityFor` so the two rules cannot drift.
 */
function isLetGeometryType(node: EntityTreeNode): boolean {
  return (
    node.kind === 'let' &&
    node.type_name != null &&
    (node.type_name.includes('Solid') ||
      node.type_name.includes('Surface') ||
      node.type_name.includes('Curve'))
  );
}

/**
 * Compute the default visibility state for a single node based on its
 * structural properties. This materialises the same rule that
 * `viewStateStore`'s `defaultRuleFor` uses for walk-up resolution, but
 * produces an explicit per-node entry rather than a fallback.
 *
 * Rule (in precedence order):
 * 1. `trait_geometry` → 'show'
 * 2. `kind === 'let'` AND `type_name` contains 'Solid' | 'Surface' | 'Curve' → 'hidden'
 * 3. Everything else (structure, sub, param, occurrence, auto, port, …) → 'show'
 */
export function defaultVisibilityFor(node: EntityTreeNode): VisibilityState {
  if (node.trait_geometry) return 'show';
  if (isLetGeometryType(node)) return 'hidden';
  return 'show';
}

// ---------------------------------------------------------------------------
// DFS tree walker (shared by all generators)
// ---------------------------------------------------------------------------

/**
 * Collect every node in the tree via a stack-based DFS.  Using `pop()` keeps
 * this O(n) rather than the O(n²) that `shift()` on a plain array would cause
 * on large assemblies.  Ordering within the returned array doesn't matter
 * because the callers key results by `entity_path`.
 */
function collectAllNodes(nodes: EntityTreeNode[]): EntityTreeNode[] {
  const result: EntityTreeNode[] = [];
  const stack: EntityTreeNode[] = [...nodes];
  while (stack.length > 0) {
    const node = stack.pop()!;
    result.push(node);
    stack.push(...node.children);
  }
  return result;
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

/**
 * Generate the `auto:default` view by walking the tree and applying
 * `defaultVisibilityFor` to each node.
 */
export function generateDefaultView(tree: EntityTreeNode[]): ViewDefinition {
  const visibility: Record<string, VisibilityState> = {};
  for (const node of collectAllNodes(tree)) {
    visibility[node.entity_path] = defaultVisibilityFor(node);
  }
  return {
    id: 'auto:default',
    name: 'Default',
    auto: true,
    modified: false,
    visibility,
  };
}

/**
 * Generate the `auto:all-geometry` view — every node is 'show' regardless
 * of kind, type_name, or trait_geometry.
 */
export function generateAllGeometryView(tree: EntityTreeNode[]): ViewDefinition {
  const visibility: Record<string, VisibilityState> = {};
  for (const node of collectAllNodes(tree)) {
    visibility[node.entity_path] = 'show';
  }
  return {
    id: 'auto:all-geometry',
    name: 'All geometry',
    auto: true,
    modified: false,
    visibility,
  };
}

// ---------------------------------------------------------------------------
// Manufacturing-ready heuristic
// ---------------------------------------------------------------------------

function manufacturingReadyVisibilityFor(node: EntityTreeNode): VisibilityState {
  // trait_geometry → show
  if (node.trait_geometry) return 'show';
  // let Solid/Surface/Curve → ghost (still visible as context, not fully hidden)
  if (isLetGeometryType(node)) return 'ghost';
  // Material params are specifically kept visible (material assignments matter
  // for manufacturing output).  Without this branch they would fall through to
  // the param→ghost rule below, which would incorrectly de-emphasise them.
  if (node.type_name != null && node.type_name.includes('Material')) return 'show';
  // Non-geometry, non-material params (dimensions, angles, …) are ghosted so
  // they don't clutter the manufacturing view.
  if (node.kind === 'param') return 'ghost';
  // Containers (structure, sub, occurrence, …) → show.
  return 'show';
}

// ---------------------------------------------------------------------------
// generatePurposeViews
// ---------------------------------------------------------------------------

/**
 * Generate one `ViewDefinition` per active purpose name.
 * - `manufacturing_ready`: applies the dedicated heuristic.
 * - All other purposes: fall back to `defaultVisibilityFor` per node (same
 *   as the Default view, but the view is distinctly labeled).
 *
 * Returns views in the same order as `activePurposes`.
 */
export function generatePurposeViews(
  tree: EntityTreeNode[],
  activePurposes: string[],
): ViewDefinition[] {
  if (activePurposes.length === 0) return [];

  const allNodes = collectAllNodes(tree);

  return activePurposes.map((purpose) => {
    const visibility: Record<string, VisibilityState> = {};
    const ruleFn =
      purpose === 'manufacturing_ready' ? manufacturingReadyVisibilityFor : defaultVisibilityFor;
    for (const node of allNodes) {
      visibility[node.entity_path] = ruleFn(node);
    }
    return {
      id: `auto:purpose:${purpose}`,
      name: purpose,
      auto: true,
      modified: false,
      visibility,
    };
  });
}
