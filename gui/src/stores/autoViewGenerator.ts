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
 * - `visibility`: Explicit per-node visibility state keyed by `entity_path`.
 *   For auto views this is a full assignment (every node has an entry).
 *   For user views the map may be sparse; unset paths fall through to
 *   `defaultRuleFor` via walk-up.
 * - `modified`: Optional flag set to `true` when this user view was created via
 *   copy-on-write (COW) from an auto view. Pristine auto views leave this unset
 *   (`undefined`). The UI may surface a visual indicator for COW-created views.
 *   Semantically equivalent to `false` when absent.
 *
 * **Live user-view mirror**: When a `user:*` view is the active view in
 * `viewStateStore`, every explicit-state mutation (`setVisibility`,
 * `setVisibilityWithoutCascade`, `resetToInherit`, `showOnly`, `cycleCascading`)
 * and every tree regeneration (`regenerateAutoViews`) automatically rebuilds
 * `visibility` from the current `state.explicit`, omitting cleared (absent) keys.
 * This means `visibility` on an active user view always equals the
 * user's live explicit choices — it is not a stale seed from `seedView`.
 * Consumers that export or restore user views can treat `visibility` as the
 * complete, up-to-date state.
 */
export interface ViewDefinition {
  id: string;
  name: string;
  auto: boolean;
  visibility: Record<string, VisibilityState>;
  /** Set to `true` on COW-created user views (auto→user on first edit). Absent on pristine auto views. */
  modified?: boolean;
}

// ---------------------------------------------------------------------------
// Shared visibility rule
// ---------------------------------------------------------------------------

/**
 * Matches the canonical geometry type tokens (Solid, Surface, Curve) at word
 * boundaries so generic wrappers like `Option<Solid>` or `List<Curve>` are
 * recognised while substring-only matches like `MySolid`, `Solidarity`, or
 * `SolidBody` are correctly rejected.
 *
 * Note: the backend type system does not emit digit-suffix variants such as
 * `Solid3D` or `Curve2D` (`Solid` resolves to `Type::Geometry` and emits
 * `"Geometry"`; `Surface`/`Curve` are `StructureRef` types with fixed names).
 * If that ever changes, extend this pattern accordingly.
 */
const GEOMETRY_TYPE_NAME_RE = /\b(?:Solid|Surface|Curve)\b/;

/**
 * Matches the canonical Material token at a word boundary, accepting wrappers
 * such as `List<Material>` while rejecting `MaterialReference`.
 */
const MATERIAL_TYPE_NAME_RE = /\bMaterial\b/;

/**
 * Returns true when a node is a let-binding whose type matches
 * `\b(Solid|Surface|Curve)\b` (anchored on word boundaries).  Generic wrappers
 * such as `Option<Solid>` are recognised; substring-only names such as `MySolid`
 * or `SolidBody` are not.  Used by both `defaultVisibilityFor` and
 * `manufacturingReadyVisibilityFor` so the two rules cannot drift.
 */
function isLetGeometryType(node: EntityTreeNode): boolean {
  return (
    node.kind === 'let' &&
    node.type_name != null &&
    GEOMETRY_TYPE_NAME_RE.test(node.type_name)
  );
}

/**
 * Compute the default visibility state for a single node based on its
 * structural properties. This materialises the same rule that
 * `viewStateStore`'s `defaultRuleFor` uses for walk-up resolution, but
 * produces an explicit per-node entry rather than a fallback.
 *
 * Rule (in precedence order):
 * 0. `default_visible === false` → 'hidden' (aux body / aux-subtree hidden-by-default;
 *    takes precedence over trait_geometry so an aux realization that happens to be
 *    trait_geometry is still hidden until the user toggles it on).
 * 1. `trait_geometry` → 'show'
 * 2. `kind === 'let'` AND `type_name` matches `\b(Solid|Surface|Curve)\b` (anchored) → 'hidden'
 * 3. Everything else (structure, sub, param, occurrence, auto, port, …) → 'show'
 */
export function defaultVisibilityFor(node: EntityTreeNode): VisibilityState {
  // Rule 0: aux hidden-by-default (T6). Strict === false so absent/true nodes
  // fall through to the existing rules unchanged (backward-compatible).
  if (node.default_visible === false) return 'hidden';
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
    visibility,
  };
}

// ---------------------------------------------------------------------------
// Manufacturing-ready heuristic
// ---------------------------------------------------------------------------

function manufacturingReadyVisibilityFor(node: EntityTreeNode): VisibilityState {
  // trait_geometry → show
  if (node.trait_geometry) return 'show';
  // let node matching \b(Solid|Surface|Curve)\b → ghost (still visible as context)
  if (isLetGeometryType(node)) return 'ghost';
  // Material params (type_name matches \bMaterial\b, including wrappers such as
  // List<Material>) are specifically kept visible (material assignments matter
  // for manufacturing output).  Without this branch they would fall through to
  // the param→ghost rule below, which would incorrectly de-emphasise them.
  if (node.type_name != null && MATERIAL_TYPE_NAME_RE.test(node.type_name)) return 'show';
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
      visibility,
    };
  });
}
