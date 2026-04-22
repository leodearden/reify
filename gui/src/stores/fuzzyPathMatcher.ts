/**
 * Fuzzy path matcher for stale entity paths.
 *
 * When the entity tree changes (e.g. after a rename or structural refactor),
 * previously-persisted view-state entries may reference paths that no longer
 * exist.  This module provides heuristics to suggest candidate replacements:
 *
 *  - `suffixMatch`       — pure suffix comparison (last-N segments match)
 *  - `structuralMatch`   — type_name + parent type_name + child-count guard
 *  - `findFuzzyCandidate` — combinator that ranks and gates on uniqueness
 *
 * **Never auto-applies** — callers are responsible for presenting suggestions
 * to the user and applying only on explicit confirmation (PRD §8.5).
 */
import type { EntityTreeNode } from '../types';

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/**
 * Collect all entity paths in the tree via a stack-based DFS.
 * Mirrors `collectAllNodes` in autoViewGenerator but returns paths only.
 */
function collectAllPaths(nodes: EntityTreeNode[]): string[] {
  const result: string[] = [];
  const stack: EntityTreeNode[] = [...nodes];
  while (stack.length > 0) {
    const node = stack.pop()!;
    result.push(node.entity_path);
    stack.push(...node.children);
  }
  return result;
}

/**
 * Build a lookup map from entity_path → EntityTreeNode for a whole tree.
 */
function buildNodeMap(nodes: EntityTreeNode[]): Map<string, EntityTreeNode> {
  const map = new Map<string, EntityTreeNode>();
  const stack: EntityTreeNode[] = [...nodes];
  while (stack.length > 0) {
    const node = stack.pop()!;
    map.set(node.entity_path, node);
    stack.push(...node.children);
  }
  return map;
}

/**
 * Compute the length of the longest common suffix (by segment) between two
 * dot-separated path segment arrays.
 */
function maxCommonSuffixLength(a: string[], b: string[]): number {
  let count = 0;
  let ai = a.length - 1;
  let bi = b.length - 1;
  while (ai >= 0 && bi >= 0 && a[ai] === b[bi]) {
    count++;
    ai--;
    bi--;
  }
  return count;
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Return all tree paths that share a non-trivial suffix with `stalePath` but
 * differ from it (i.e. possible rename/restructure candidates).
 *
 * A candidate path P is included when:
 *   1. P ≠ stalePath (exact match is not a rename candidate).
 *   2. The longest common suffix from the right is ≥ 1 segment.
 *   3. That suffix is strictly shorter than `stalePath` (so there is at least
 *      one differing ancestor segment — "differently-named parent" criterion).
 *
 * Returns all candidates; uniqueness filtering is the caller's responsibility
 * (see `findFuzzyCandidate`).
 */
export function suffixMatch(stalePath: string, tree: EntityTreeNode[]): string[] {
  const staleSegs = stalePath.split('.');
  const allPaths = collectAllPaths(tree);
  const candidates: string[] = [];

  for (const treePath of allPaths) {
    if (treePath === stalePath) continue; // not a rename candidate
    const treeSegs = treePath.split('.');
    const commonSuffix = maxCommonSuffixLength(staleSegs, treeSegs);
    // Require at least 1 matching segment, but fewer than the full stale path
    // (there must be at least one differing ancestor on the stale side).
    if (commonSuffix > 0 && commonSuffix < staleSegs.length) {
      candidates.push(treePath);
    }
  }

  return candidates;
}

/**
 * Metadata about the original stale path used by `structuralMatch` to
 * narrow candidates beyond suffix similarity.
 */
export interface StalePathMetadata {
  /** `type_name` of the stale node at the time it was recorded. */
  typeName: string | null;
  /** `type_name` of the stale node's parent, or null if root. */
  parentTypeName: string | null;
  /** Number of children the stale node had. */
  childCount: number;
}

/**
 * Return true when `treePath` is a plausible structural match for the
 * original stale node described by `metadata`.
 *
 * Conditions (all must hold):
 *   - `type_name` of the tree node equals `metadata.typeName`
 *   - `type_name` of the tree node's parent equals `metadata.parentTypeName`
 *   - Child-count of the tree node is within ±1 of `metadata.childCount`
 */
export function structuralMatch(
  treePath: string,
  metadata: StalePathMetadata,
  tree: EntityTreeNode[],
): boolean {
  const nodeMap = buildNodeMap(tree);
  const node = nodeMap.get(treePath);
  if (!node) return false;

  // type_name must match exactly (including null)
  if (node.type_name !== metadata.typeName) return false;

  // child-count within ±1
  if (Math.abs(node.children.length - metadata.childCount) > 1) return false;

  // parent type_name
  const segs = treePath.split('.');
  if (segs.length >= 2) {
    const parentPath = segs.slice(0, -1).join('.');
    const parentNode = nodeMap.get(parentPath);
    const parentTypeName = parentNode?.type_name ?? null;
    if (parentTypeName !== metadata.parentTypeName) return false;
  } else {
    // root node — parent is conceptually null
    if (metadata.parentTypeName !== null) return false;
  }

  return true;
}

/**
 * Find a single unambiguous rename candidate for `stalePath` in `tree`.
 *
 * Algorithm:
 *   1. Run `suffixMatch` to get all suffix-based candidates.
 *   2. If `metadata` is provided, further filter by `structuralMatch`.
 *   3. If exactly one candidate survives, return `{ path: candidate }`.
 *   4. Otherwise (0 or 2+), return null (no suggestion or ambiguous).
 *
 * Per PRD §8.5: never auto-applies; callers must present the suggestion
 * to the user.
 */
export function findFuzzyCandidate(
  stalePath: string,
  metadata: StalePathMetadata | null | undefined,
  tree: EntityTreeNode[],
): { path: string } | null {
  const suffixCandidates = suffixMatch(stalePath, tree);

  const candidates =
    metadata != null
      ? suffixCandidates.filter(p => structuralMatch(p, metadata, tree))
      : suffixCandidates;

  if (candidates.length === 1) {
    return { path: candidates[0] };
  }
  return null;
}
