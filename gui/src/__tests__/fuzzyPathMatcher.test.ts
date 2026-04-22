/**
 * Tests for fuzzyPathMatcher — suffix + structural heuristics for matching
 * stale entity paths to renamed/moved paths in a new entity tree.
 *
 * Written in TDD order:
 *  step-13 — suffix-match tests (fail until step-14 creates suffixMatch)
 *  step-15 — structural + combined findFuzzyCandidate tests (fail until step-16)
 */
import { describe, it, expect } from 'vitest';
import { suffixMatch } from '../stores/fuzzyPathMatcher';
import type { EntityTreeNode } from '../types';

// ---------------------------------------------------------------------------
// Tree builder helpers
// ---------------------------------------------------------------------------

/** Build a leaf node with no children. */
function leaf(entity_path: string): EntityTreeNode {
  return { entity_path, kind: 'occurrence', type_name: null, has_mesh: false, trait_geometry: false, children: [] };
}

/**
 * Build a node with children.  The entity_path of each child is constructed as
 * `${parentPath}.${childSegment}` to mirror real trees.
 */
function node(entity_path: string, childSegments: string[], type_name: string | null = null): EntityTreeNode {
  const parts = entity_path.split('.');
  return {
    entity_path,
    kind: 'structure',
    type_name,
    has_mesh: false,
    trait_geometry: false,
    children: childSegments.map(seg => leaf(`${entity_path}.${seg}`)),
  };
}

/** Build a tree root with named child sub-trees passed in directly. */
function tree(...roots: EntityTreeNode[]): EntityTreeNode[] {
  return roots;
}

// ---------------------------------------------------------------------------
// suffixMatch — step-13 tests (fail until step-14)
// ---------------------------------------------------------------------------

describe('suffixMatch — suffix-only matching (step-13)', () => {
  it('(a) matches stale leaf to tree node with same leaf but different parent', () => {
    // stale: "Assembly.flange.geometry"
    // tree:  "Assembly.bolt_flange.geometry"
    // suffix: "geometry" (1 segment), parent changed flange → bolt_flange
    const testTree: EntityTreeNode[] = [
      {
        entity_path: 'Assembly',
        kind: 'structure',
        type_name: null,
        has_mesh: false,
        trait_geometry: false,
        children: [
          {
            entity_path: 'Assembly.bolt_flange',
            kind: 'occurrence',
            type_name: 'Flange',
            has_mesh: false,
            trait_geometry: false,
            children: [
              leaf('Assembly.bolt_flange.geometry'),
            ],
          },
        ],
      },
    ];

    const candidates = suffixMatch('Assembly.flange.geometry', testTree);

    expect(candidates).toEqual(['Assembly.bolt_flange.geometry']);
  });

  it('(b) matches when tail-2 segments are identical but the parent name differs', () => {
    // stale: "Assembly.old_parent.bolt.geometry"
    // tree:  "Assembly.new_parent.bolt.geometry"
    // suffix: "bolt.geometry" (2 segments), parent changed old_parent → new_parent
    const testTree: EntityTreeNode[] = [
      {
        entity_path: 'Assembly',
        kind: 'structure',
        type_name: null,
        has_mesh: false,
        trait_geometry: false,
        children: [
          {
            entity_path: 'Assembly.new_parent',
            kind: 'occurrence',
            type_name: null,
            has_mesh: false,
            trait_geometry: false,
            children: [
              {
                entity_path: 'Assembly.new_parent.bolt',
                kind: 'occurrence',
                type_name: null,
                has_mesh: false,
                trait_geometry: false,
                children: [
                  leaf('Assembly.new_parent.bolt.geometry'),
                ],
              },
            ],
          },
        ],
      },
    ];

    const candidates = suffixMatch('Assembly.old_parent.bolt.geometry', testTree);

    expect(candidates).toEqual(['Assembly.new_parent.bolt.geometry']);
  });

  it('(c) returns multiple candidates when suffix matches more than one tree path (ambiguous)', () => {
    // stale: "Assembly.flange.geometry"
    // tree has: "Assembly.part1.geometry" AND "Assembly.part2.geometry"
    // Both share suffix "geometry" under different parents → 2 candidates (ambiguous)
    const testTree: EntityTreeNode[] = [
      {
        entity_path: 'Assembly',
        kind: 'structure',
        type_name: null,
        has_mesh: false,
        trait_geometry: false,
        children: [
          {
            entity_path: 'Assembly.part1',
            kind: 'occurrence',
            type_name: null,
            has_mesh: false,
            trait_geometry: false,
            children: [leaf('Assembly.part1.geometry')],
          },
          {
            entity_path: 'Assembly.part2',
            kind: 'occurrence',
            type_name: null,
            has_mesh: false,
            trait_geometry: false,
            children: [leaf('Assembly.part2.geometry')],
          },
        ],
      },
    ];

    const candidates = suffixMatch('Assembly.flange.geometry', testTree);

    // Both paths are candidates — caller is responsible for refusing ambiguous results
    expect(candidates).toHaveLength(2);
    expect(candidates).toContain('Assembly.part1.geometry');
    expect(candidates).toContain('Assembly.part2.geometry');
  });

  it('(d) returns empty array when tree is empty', () => {
    const candidates = suffixMatch('Assembly.flange.geometry', []);
    expect(candidates).toEqual([]);
  });

  it('does not return the stale path itself if it still exists in the tree', () => {
    // If the stale path is present, it should NOT be a candidate (it's not "stale" in this context)
    const testTree: EntityTreeNode[] = [
      {
        entity_path: 'Assembly',
        kind: 'structure',
        type_name: null,
        has_mesh: false,
        trait_geometry: false,
        children: [
          {
            entity_path: 'Assembly.flange',
            kind: 'occurrence',
            type_name: null,
            has_mesh: false,
            trait_geometry: false,
            children: [leaf('Assembly.flange.geometry')],
          },
        ],
      },
    ];

    const candidates = suffixMatch('Assembly.flange.geometry', testTree);

    // The stale path itself must not appear as a "rename candidate"
    expect(candidates).not.toContain('Assembly.flange.geometry');
  });
});
