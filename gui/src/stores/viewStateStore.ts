import { createStore, produce } from 'solid-js/store';
import type { EntityTreeNode, ExplicitVisibility, VisibilityState } from '../types';
import {
  generateDefaultView,
  generateAllGeometryView,
  generatePurposeViews,
  defaultVisibilityFor,
} from './autoViewGenerator';
import type { ViewDefinition } from './autoViewGenerator';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface ViewState {
  /**
   * Absence of a key means "no override" (inherits from nearest explicit
   * ancestor, or falls back to the default rule).  Entries are **deleted**
   * on clear; `null` is never stored at runtime even though
   * `ExplicitVisibility` admits it as a type.
   */
  explicit: Record<string, ExplicitVisibility>;
  views: Record<string, ViewDefinition>;
  activeViewId: string;
  /**
   * Ordered list of user view ids for display in the ViewSelector and
   * ViewManageModal. Auto views always appear before user views in the
   * selector; this array controls the order of the user-view segment.
   * Re-initialized to `[]` each session; persistence is handled by a
   * future task.
   */
  userViewOrder: string[];
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/** Recursively build parent and node lookup maps from a tree. */
function buildMaps(
  nodes: EntityTreeNode[],
  nodeByPath: Map<string, EntityTreeNode>,
  parentByPath: Map<string, string | null>,
  parentPath: string | null = null,
): void {
  for (const node of nodes) {
    nodeByPath.set(node.entity_path, node);
    parentByPath.set(node.entity_path, parentPath);
    buildMaps(node.children, nodeByPath, parentByPath, node.entity_path);
  }
}

/** Collect all descendant paths of a given path (not including the path itself). */
function walkDescendants(path: string, nodeByPath: Map<string, EntityTreeNode>): string[] {
  const result: string[] = [];
  const node = nodeByPath.get(path);
  if (!node) return result;
  const queue = [...node.children];
  while (queue.length > 0) {
    const child = queue.shift()!;
    result.push(child.entity_path);
    queue.push(...child.children);
  }
  return result;
}

/** Default visibility rule for a node when no ancestor has an explicit state.
 *  Delegates to autoViewGenerator.defaultVisibilityFor so the walk-up fallback
 *  and the per-node materialisation in generateDefaultView always agree.
 */
function defaultRuleFor(node: EntityTreeNode): VisibilityState {
  return defaultVisibilityFor(node);
}

// ---------------------------------------------------------------------------
// Store factory
// ---------------------------------------------------------------------------

export function createViewStateStore() {
  const [state, setState] = createStore<ViewState>({
    explicit: {},
    views: {},
    activeViewId: 'auto:default',
    userViewOrder: [],
  });

  // Internal non-reactive maps (rebuilt on setTree / rebuildTreeMaps).
  let nodeByPath = new Map<string, EntityTreeNode>();
  let parentByPath = new Map<string, string | null>();

  // ---------------------------------------------------------------------------
  // Tree registration
  // ---------------------------------------------------------------------------

  /**
   * Internal helper: refresh nodeByPath / parentByPath without touching reactive
   * state.  Used by regenerateAutoViews so the map rebuild does not produce a
   * separate setState call; the stale-explicit prune is instead folded into the
   * same produce block as the view replacement.
   */
  function rebuildTreeMaps(nodes: EntityTreeNode[]): void {
    nodeByPath = new Map();
    parentByPath = new Map();
    buildMaps(nodes, nodeByPath, parentByPath, null);
  }

  function setTree(nodes: EntityTreeNode[]): void {
    rebuildTreeMaps(nodes);
    // Prune explicit overrides for paths that no longer exist in the tree.
    // Stale entries can accumulate when nodes are deleted or renamed upstream,
    // causing hasOverride / getEffectiveVisibility to return stale values for
    // removed paths, and re-introduced paths would silently inherit old state.
    setState(
      produce((s) => {
        for (const path of Object.keys(s.explicit)) {
          if (!nodeByPath.has(path)) {
            delete s.explicit[path];
          }
        }
      }),
    );
  }

  // ---------------------------------------------------------------------------
  // Effective visibility resolution
  // ---------------------------------------------------------------------------

  function getEffectiveVisibility(path: string): VisibilityState {
    if (nodeByPath.size === 0) return 'show';

    // Walk up the ancestor chain looking for the first non-null explicit state.
    let current: string | null = path;
    while (current !== null) {
      const exp = state.explicit[current];
      if (exp != null) return exp;
      current = parentByPath.get(current) ?? null;
    }

    // No ancestor has an explicit state — apply default rule for this node.
    const node = nodeByPath.get(path);
    if (!node) return 'show';
    return defaultRuleFor(node);
  }

  // ---------------------------------------------------------------------------
  // Bulk effective map (for viewport)
  // ---------------------------------------------------------------------------

  function getAllEffective(): Record<string, VisibilityState> {
    if (nodeByPath.size === 0) return {};
    const result: Record<string, VisibilityState> = {};
    for (const path of nodeByPath.keys()) {
      result[path] = getEffectiveVisibility(path);
    }
    return result;
  }

  // ---------------------------------------------------------------------------
  // User-view mirror helper
  // ---------------------------------------------------------------------------

  /**
   * When the active view is a user view (`activeViewId` starts with `'user:'`),
   * mirror the current entries in `s.explicit` (cleared keys are simply absent)
   * back into `s.views[activeViewId].visibility`.
   *
   * Call this at the END of every mutation's `produce(s => ...)` block so that
   * the stored user view always reflects what the user currently sees.
   *
   * Early-returns for auto:* and unknown active views — those are not targets
   * of the live mirror.
   */
  function mirrorExplicitToActiveUserView(s: ViewState): void {
    if (!s.activeViewId.startsWith('user:')) return;
    const view = s.views[s.activeViewId];
    if (!view) return;
    const mirrored: Record<string, VisibilityState> = {};
    for (const [path, val] of Object.entries(s.explicit)) {
      // Type narrowing: ExplicitVisibility admits null at the type level; runtime never stores null (see ViewState.explicit JSDoc).
      if (val != null) {
        mirrored[path] = val;
      }
    }
    view.visibility = mirrored;
  }

  // ---------------------------------------------------------------------------
  // Mutations
  // ---------------------------------------------------------------------------

  function setVisibility(path: string, vs: VisibilityState, cascade = true): void {
    setState(
      produce((s) => {
        s.explicit[path] = vs;
        if (cascade) {
          for (const desc of walkDescendants(path, nodeByPath)) {
            delete s.explicit[desc];
          }
        }
        mirrorExplicitToActiveUserView(s);
      }),
    );
  }

  function setVisibilityWithoutCascade(path: string, vs: VisibilityState): void {
    // Use produce so the mirror step runs in the same reactive notification
    // rather than introducing a separate setState call.
    setState(
      produce((s) => {
        s.explicit[path] = vs;
        mirrorExplicitToActiveUserView(s);
      }),
    );
  }

  function resetToInherit(path: string): void {
    setState(
      produce((s) => {
        delete s.explicit[path];
        for (const desc of walkDescendants(path, nodeByPath)) {
          delete s.explicit[desc];
        }
        mirrorExplicitToActiveUserView(s);
      }),
    );
  }

  function showOnly(path: string, cascade = true): void {
    // Compute ancestor chain of `path`
    const ancestors = new Set<string>();
    let cur = parentByPath.get(path) ?? null;
    while (cur !== null) {
      ancestors.add(cur);
      cur = parentByPath.get(cur) ?? null;
    }

    setState(
      produce((s) => {
        // Clear all ancestors so they don't override-hide the target.
        for (const anc of ancestors) {
          delete s.explicit[anc];
        }
        // Hide everything not in {target} ∪ ancestors.
        for (const p of nodeByPath.keys()) {
          if (p !== path && !ancestors.has(p)) {
            s.explicit[p] = 'hidden';
          }
        }
        // Set target visible; if cascade, descendants will be cleared by setVisibility.
        s.explicit[path] = 'show';
        if (cascade) {
          for (const desc of walkDescendants(path, nodeByPath)) {
            delete s.explicit[desc];
          }
        }
        mirrorExplicitToActiveUserView(s);
      }),
    );
  }

  function cycleCascading(path: string): void {
    const effective = getEffectiveVisibility(path);
    const next: VisibilityState =
      effective === 'show' ? 'ghost' : effective === 'ghost' ? 'hidden' : 'show';
    setVisibility(path, next, true);
  }

  // ---------------------------------------------------------------------------
  // View management
  // ---------------------------------------------------------------------------

  /**
   * @internal Exposed on the returned store for test-only use; UI callers
   * should use regenerateAutoViews so views go through the standard
   * reconciliation path.
   *
   * Seed a view into state.views (used by regenerateAutoViews and tests).
   * Overwrites any existing entry with the same id.
   *
   * NOTE: When a `user:*` view is the active view, subsequent mutations
   * (`setVisibility`, `setVisibilityWithoutCascade`, `resetToInherit`,
   * `showOnly`, `cycleCascading`) will overwrite whatever this seed writes
   * via the live-mirror mechanism.  Callers that need to preserve seeded
   * state should switch away from a user view before calling mutations.
   */
  function seedView(view: ViewDefinition): void {
    setState(produce((s) => {
      s.views[view.id] = view;
    }));
  }

  /**
   * Switch the active view.  If the view doesn't exist in state.views this
   * is a no-op (the caller should ensure views are populated before
   * switching, e.g. via regenerateAutoViews).
   *
   * When the view exists its `visibility` map is copied into `state.explicit`
   * so that `getEffectiveVisibility` / `getAllEffective` reflect the view.
   *
   * NOTE on null semantics: `ViewDefinition.visibility` only carries concrete
   * `VisibilityState` values ('show' | 'ghost' | 'hidden') — never `null`.
   * Activating a view therefore replaces `state.explicit` wholesale and
   * destroys any prior explicit-null (inherit) markers the user may have set
   * via `resetToInherit`.  If null/inherit semantics ever need to survive a
   * view switch they would have to be stored in the `ViewDefinition` itself.
   */
  function setActiveView(viewId: string): void {
    const view = state.views[viewId];
    if (!view) return;
    setState(produce((s) => {
      s.activeViewId = viewId;
      // Replace explicit with the view's full visibility map.
      s.explicit = { ...view.visibility };
    }));
  }

  /**
   * Create a new empty user view with the given name.
   *
   * The view is added to `state.views` with `auto: false`, `modified: false`,
   * and an empty `visibility` map.  Its id is appended to `state.userViewOrder`.
   * The new view does NOT become active automatically — the caller should follow
   * up with `switchView(id)` if activation is desired.
   *
   * @returns The new view's id (format: `user:<uuid-fragment>`).
   */
  function createView(name: string): string {
    // Generate a short unique id using the high-res timestamp + random bits.
    const id = `user:${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
    setState(
      produce((s) => {
        s.views[id] = {
          id,
          name,
          auto: false,
          modified: false,
          visibility: {},
        };
        s.userViewOrder.push(id);
      }),
    );
    return id;
  }

  /**
   * Switch the active view, returning `true` on success or `false` if the view
   * id is unknown (does not exist in `state.views`).
   *
   * This is a boolean-returning wrapper over the existing `setActiveView` so
   * that callers (the number-key dispatcher, ViewSelector) can silently ignore
   * out-of-range indices without inspecting store internals.
   */
  function switchView(viewId: string): boolean {
    if (!state.views[viewId]) return false;
    setActiveView(viewId);
    return true;
  }

  /**
   * Delete a user view.  Validation rules:
   * - id must exist in `state.views`
   * - id must not start with `auto:` (auto views cannot be deleted)
   *
   * If the deleted view is currently active, falls back to `auto:default`
   * and copies its `visibility` map into `state.explicit` so the viewport
   * immediately reflects the fallback.
   *
   * @returns `true` on success, `false` on any validation failure (no state change).
   */
  function deleteView(id: string): boolean {
    const view = state.views[id];
    if (!view) return false;
    if (id.startsWith('auto:')) return false;

    const isActive = state.activeViewId === id;
    setState(
      produce((s) => {
        delete s.views[id];
        const idx = s.userViewOrder.indexOf(id);
        if (idx !== -1) s.userViewOrder.splice(idx, 1);

        if (isActive) {
          s.activeViewId = 'auto:default';
          const fallback = s.views['auto:default'];
          s.explicit = fallback ? { ...fallback.visibility } : {};
        }
      }),
    );
    return true;
  }

  /**
   * Rename a user view.  Validation rules (all return `false` on failure):
   * - id must exist in `state.views`
   * - id must not start with `auto:` (auto views cannot be renamed)
   * - new name must not be empty or whitespace-only
   * - new name must not duplicate an existing user view name (case-insensitive),
   *   except when renaming the view to its own current name (identity rename)
   *
   * @returns `true` on success, `false` on any validation failure (no state change).
   */
  function renameView(id: string, newName: string): boolean {
    // id must exist
    const view = state.views[id];
    if (!view) return false;
    // auto views cannot be renamed
    if (id.startsWith('auto:')) return false;
    // name must not be empty/whitespace
    const trimmed = newName.trim();
    if (trimmed.length === 0) return false;
    // check for name collision against other user views (case-insensitive)
    const lowerNew = trimmed.toLowerCase();
    for (const [otherId, otherView] of Object.entries(state.views)) {
      if (otherId === id) continue; // skip self
      if (otherView.name.toLowerCase() === lowerNew) return false;
    }
    setState(
      produce((s) => {
        s.views[id].name = trimmed;
      }),
    );
    return true;
  }

  /**
   * Regenerate all `auto:*` views from the current tree and active purposes,
   * preserve any `user:*` views, then reconcile the active view:
   *
   * - active is `auto:*` and view still exists → copy its visibility into explicit.
   * - active is `auto:*` and view was removed (purpose deactivated) → fall back
   *   to `auto:default` and copy its visibility.
   * - active is `user:*` and view exists → keep explicit entries for paths that
   *   are still in the tree; leave NEW paths unset so defaultRuleFor handles them;
   *   then mirror the pruned explicit back into the user view's stored visibility.
   * - active references a missing view → fall back to `auto:default`.
   *
   * This function keeps the internal nodeByPath / parentByPath maps in sync with
   * the provided tree so callers do not need a separate `setTree` call.
   *
   * NOTE: This function performs exactly one `setState` — all work (stale-explicit
   * prune, auto-view replacement, active-view reconciliation, user-view mirror) is
   * batched into a single reactive notification.  Do NOT add a `setTree(tree)` call
   * here; use `rebuildTreeMaps` instead to keep the map refresh non-reactive.
   *
   * NOTE on stale-explicit pruning: the prune loop runs only for the `user:*`
   * branch (the only branch that preserves `s.explicit` rather than replacing it
   * wholesale).  Auto:* and unknown branches overwrite `s.explicit` with a fresh
   * view map, so a prior prune pass would be wasted work.
   */
  function regenerateAutoViews(tree: EntityTreeNode[], activePurposes: string[] = []): void {
    // Rebuild the internal maps without triggering a reactive setState so that
    // the stale-explicit prune below can be folded into the same produce block
    // as the view replacement — one reactive notification total.
    rebuildTreeMaps(tree);

    const freshDefault = generateDefaultView(tree);
    const freshAllGeo = generateAllGeometryView(tree);
    const freshPurpose = generatePurposeViews(tree, activePurposes);

    // Build set of all paths in the new tree — used by the user:* prune loop.
    const treePathSet = new Set(Object.keys(freshDefault.visibility));

    setState(produce((s) => {
      // ------------------------------------------------------------------
      // 1. Replace all auto:* views with fresh ones.
      // ------------------------------------------------------------------
      for (const key of Object.keys(s.views)) {
        if (key.startsWith('auto:')) {
          delete s.views[key];
        }
      }
      s.views[freshDefault.id] = freshDefault;
      s.views[freshAllGeo.id] = freshAllGeo;
      for (const pv of freshPurpose) {
        s.views[pv.id] = pv;
      }
      // user:* views are left untouched.

      // ------------------------------------------------------------------
      // 2. Reconcile active view.
      // ------------------------------------------------------------------
      const activeId = s.activeViewId;

      if (activeId.startsWith('auto:')) {
        // Active is an auto view. If it still exists, apply it; otherwise fall back.
        const target = s.views[activeId] ?? s.views['auto:default'];
        const targetId = s.views[activeId] ? activeId : 'auto:default';
        s.activeViewId = targetId;
        s.explicit = { ...target.visibility };

      } else if (activeId.startsWith('user:')) {
        // Active is a user view. Keep entries for paths still in the tree;
        // remove entries for paths that are no longer in the tree;
        // leave new paths unset so defaultRuleFor applies via walk-up.
        // NOTE: this is the only branch that preserves s.explicit rather than
        // replacing it wholesale, so stale-path pruning is only needed here.
        const userView = s.views[activeId];
        if (!userView) {
          // User view was somehow deleted — fall back to default.
          s.activeViewId = 'auto:default';
          s.explicit = { ...freshDefault.visibility };
        } else {
          // Prune explicit entries for paths no longer in tree.
          for (const path of Object.keys(s.explicit)) {
            if (!treePathSet.has(path)) {
              delete s.explicit[path];
            }
          }
          // Do NOT add entries for new paths — leave them unset.
        }

      } else {
        // Unknown active view — fall back to default.
        s.activeViewId = 'auto:default';
        s.explicit = { ...freshDefault.visibility };
      }

      // ------------------------------------------------------------------
      // 3. Mirror user-view if applicable.
      //    Runs after reconcile so that a user view's stored visibility stays
      //    in sync with the (possibly pruned) explicit map after a tree change.
      //    Early-returns for auto:* and unknown active views.
      // ------------------------------------------------------------------
      mirrorExplicitToActiveUserView(s);
    }));
  }

  function hasOverride(path: string): boolean {
    const exp = state.explicit[path];
    if (exp == null) return false;
    // Compute what the node would inherit if it had no explicit state.
    const parent = parentByPath.get(path) ?? null;
    let wouldInherit: VisibilityState;
    if (parent !== null) {
      // Temporarily read parent effective without considering this node
      wouldInherit = getEffectiveVisibility(parent);
    } else {
      const node = nodeByPath.get(path);
      wouldInherit = node ? defaultRuleFor(node) : 'show';
    }
    return exp !== wouldInherit;
  }

  return {
    state,
    // Tree
    setTree,
    // Queries
    getEffectiveVisibility,
    getAllEffective,
    hasOverride,
    // Mutations
    setVisibility,
    setVisibilityWithoutCascade,
    resetToInherit,
    showOnly,
    cycleCascading,
    // View management
    seedView,
    setActiveView,
    regenerateAutoViews,
    createView,
    switchView,
    renameView,
    deleteView,
  };
}

export type ViewStateStore = ReturnType<typeof createViewStateStore>;
