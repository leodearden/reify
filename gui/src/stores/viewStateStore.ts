import { createStore, produce } from 'solid-js/store';
import type { EntityTreeNode, ExplicitVisibility, VisibilityState } from '../types';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface ViewState {
  explicit: Record<string, ExplicitVisibility>;
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

/** Default visibility rule for a node when no ancestor has an explicit state. */
function defaultRuleFor(node: EntityTreeNode): VisibilityState {
  if (node.trait_geometry) return 'show';
  if (node.kind === 'let' && node.type_name?.includes('Solid')) return 'hidden';
  return 'show';
}

// ---------------------------------------------------------------------------
// Store factory
// ---------------------------------------------------------------------------

export function createViewStateStore() {
  const [state, setState] = createStore<ViewState>({
    explicit: {},
  });

  // Internal non-reactive maps (rebuilt on setTree).
  let nodeByPath = new Map<string, EntityTreeNode>();
  let parentByPath = new Map<string, string | null>();

  // ---------------------------------------------------------------------------
  // Tree registration
  // ---------------------------------------------------------------------------

  function setTree(nodes: EntityTreeNode[]): void {
    nodeByPath = new Map();
    parentByPath = new Map();
    buildMaps(nodes, nodeByPath, parentByPath, null);
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
  // Mutations (stubs — fully implemented in later steps)
  // ---------------------------------------------------------------------------

  function setVisibility(path: string, vs: VisibilityState, cascade = true): void {
    setState(
      produce((s) => {
        s.explicit[path] = vs;
        if (cascade) {
          for (const desc of walkDescendants(path, nodeByPath)) {
            s.explicit[desc] = null;
          }
        }
      }),
    );
  }

  function setVisibilityWithoutCascade(path: string, vs: VisibilityState): void {
    setState('explicit', path, vs);
  }

  function resetToInherit(path: string): void {
    setState(
      produce((s) => {
        s.explicit[path] = null;
        for (const desc of walkDescendants(path, nodeByPath)) {
          s.explicit[desc] = null;
        }
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
          s.explicit[anc] = null;
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
            s.explicit[desc] = null;
          }
        }
      }),
    );
  }

  function cycleCascading(path: string): void {
    const effective = getEffectiveVisibility(path);
    const next: VisibilityState =
      effective === 'show' ? 'ghost' : effective === 'ghost' ? 'hidden' : 'show';
    setVisibility(path, next, true);
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
  };
}
