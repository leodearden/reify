/**
 * Tests for fuzzyPathMatcher — suffix + structural heuristics for matching
 * stale entity paths to renamed/moved paths in a new entity tree.
 *
 * Written in TDD order:
 *  step-13 — suffix-match tests (fail until step-14 creates suffixMatch)
 *  step-15 — structural + combined findFuzzyCandidate tests (fail until step-16)
 */
import { describe, it, expect } from 'vitest';
import { suffixMatch, structuralMatch, findFuzzyCandidate } from '../stores/fuzzyPathMatcher';
import type { EntityTreeNode } from '../types';
import type { StalePathMetadata } from '../stores/fuzzyPathMatcher';

// ---------------------------------------------------------------------------
// Tree builder helpers
// ---------------------------------------------------------------------------

/** Build a leaf node with no children. */
function leaf(entity_path: string): EntityTreeNode {
  return { entity_path, kind: 'occurrence', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children: [] };
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
    freshness: 'final',
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
        freshness: 'final',
        children: [
          {
            entity_path: 'Assembly.bolt_flange',
            kind: 'occurrence',
            type_name: 'Flange',
            has_mesh: false,
            trait_geometry: false,
            freshness: 'final',
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
        freshness: 'final',
        children: [
          {
            entity_path: 'Assembly.new_parent',
            kind: 'occurrence',
            type_name: null,
            has_mesh: false,
            trait_geometry: false,
            freshness: 'final',
            children: [
              {
                entity_path: 'Assembly.new_parent.bolt',
                kind: 'occurrence',
                type_name: null,
                has_mesh: false,
                trait_geometry: false,
                freshness: 'final',
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
        freshness: 'final',
        children: [
          {
            entity_path: 'Assembly.part1',
            kind: 'occurrence',
            type_name: null,
            has_mesh: false,
            trait_geometry: false,
            freshness: 'final',
            children: [leaf('Assembly.part1.geometry')],
          },
          {
            entity_path: 'Assembly.part2',
            kind: 'occurrence',
            type_name: null,
            has_mesh: false,
            trait_geometry: false,
            freshness: 'final',
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
        freshness: 'final',
        children: [
          {
            entity_path: 'Assembly.flange',
            kind: 'occurrence',
            type_name: null,
            has_mesh: false,
            trait_geometry: false,
            freshness: 'final',
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

// ---------------------------------------------------------------------------
// structuralMatch — step-15 tests (fail until step-16)
// ---------------------------------------------------------------------------

describe('structuralMatch — structural heuristic (step-15)', () => {
  /**
   * Tree used by all structural tests:
   *
   *   Assembly (structure, type=null)
   *     └─ bolt_flange (occurrence, type='Flange', 3 children)
   *          ├─ geometry  (leaf)
   *          ├─ width     (leaf)
   *          └─ depth     (leaf)
   *     └─ different_flange (occurrence, type='Different', 2 children)
   *          ├─ geometry  (leaf)
   *          └─ width     (leaf)
   */
  function buildStructuralTree(): EntityTreeNode[] {
    return [
      {
        entity_path: 'Assembly',
        kind: 'structure',
        type_name: null,
        has_mesh: false,
        trait_geometry: false,
        freshness: 'final',
        children: [
          {
            entity_path: 'Assembly.bolt_flange',
            kind: 'occurrence',
            type_name: 'Flange',
            has_mesh: false,
            trait_geometry: false,
            freshness: 'final',
            children: [
              { entity_path: 'Assembly.bolt_flange.geometry', kind: 'occurrence', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children: [] },
              { entity_path: 'Assembly.bolt_flange.width',    kind: 'occurrence', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children: [] },
              { entity_path: 'Assembly.bolt_flange.depth',    kind: 'occurrence', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children: [] },
            ],
          },
          {
            entity_path: 'Assembly.different_flange',
            kind: 'occurrence',
            type_name: 'Different',
            has_mesh: false,
            trait_geometry: false,
            freshness: 'final',
            children: [
              { entity_path: 'Assembly.different_flange.geometry', kind: 'occurrence', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children: [] },
              { entity_path: 'Assembly.different_flange.width',    kind: 'occurrence', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children: [] },
            ],
          },
        ],
      },
    ];
  }

  it('(a) matches when type_name, parent type_name, and child-count within ±1 all agree', () => {
    // bolt_flange has type='Flange', parent type=null, 3 children
    // metadata from stale: type='Flange', parentType=null, childCount=2 (within ±1 of 3)
    const metadata: StalePathMetadata = {
      typeName: 'Flange',
      parentTypeName: null,
      childCount: 2,
    };
    const testTree = buildStructuralTree();
    expect(structuralMatch('Assembly.bolt_flange', metadata, testTree)).toBe(true);
  });

  it('(b) rejects when parent type_name differs', () => {
    // bolt_flange parent is Assembly with type_name=null
    // metadata says parent type is 'SomeOtherType'
    const metadata: StalePathMetadata = {
      typeName: 'Flange',
      parentTypeName: 'SomeOtherType',
      childCount: 3,
    };
    const testTree = buildStructuralTree();
    expect(structuralMatch('Assembly.bolt_flange', metadata, testTree)).toBe(false);
  });

  it('(c) rejects when child-count differs by more than 1', () => {
    // bolt_flange has 3 children; metadata says 10 children → |10-3| = 7 > 1
    const metadata: StalePathMetadata = {
      typeName: 'Flange',
      parentTypeName: null,
      childCount: 10,
    };
    const testTree = buildStructuralTree();
    expect(structuralMatch('Assembly.bolt_flange', metadata, testTree)).toBe(false);
  });

  it('rejects when type_name does not match', () => {
    const metadata: StalePathMetadata = {
      typeName: 'Washer', // stale thought it was a 'Washer'
      parentTypeName: null,
      childCount: 3,
    };
    const testTree = buildStructuralTree();
    expect(structuralMatch('Assembly.bolt_flange', metadata, testTree)).toBe(false);
  });

  it('returns false for a path not present in the tree', () => {
    const metadata: StalePathMetadata = { typeName: 'Flange', parentTypeName: null, childCount: 3 };
    const testTree = buildStructuralTree();
    expect(structuralMatch('Assembly.nonexistent', metadata, testTree)).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// findFuzzyCandidate — step-15 tests (fail until step-16)
// ---------------------------------------------------------------------------

describe('findFuzzyCandidate — combined suffix + structural ranking (step-15)', () => {
  it('returns null when there are 2+ suffix candidates (ambiguous — no metadata)', () => {
    // Stale: "Assembly.flange.geometry"
    // Tree: two paths ending in "geometry" under different parents → ambiguous
    const testTree: EntityTreeNode[] = [
      {
        entity_path: 'Assembly',
        kind: 'structure',
        type_name: null,
        has_mesh: false,
        trait_geometry: false,
        freshness: 'final',
        children: [
          {
            entity_path: 'Assembly.part1',
            kind: 'occurrence',
            type_name: null,
            has_mesh: false,
            trait_geometry: false,
            freshness: 'final',
            children: [
              { entity_path: 'Assembly.part1.geometry', kind: 'occurrence', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children: [] },
            ],
          },
          {
            entity_path: 'Assembly.part2',
            kind: 'occurrence',
            type_name: null,
            has_mesh: false,
            trait_geometry: false,
            freshness: 'final',
            children: [
              { entity_path: 'Assembly.part2.geometry', kind: 'occurrence', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children: [] },
            ],
          },
        ],
      },
    ];

    const result = findFuzzyCandidate('Assembly.flange.geometry', null, testTree);
    expect(result).toBeNull();
  });

  it('returns { path } when exactly one unambiguous candidate exists (no metadata)', () => {
    const testTree: EntityTreeNode[] = [
      {
        entity_path: 'Assembly',
        kind: 'structure',
        type_name: null,
        has_mesh: false,
        trait_geometry: false,
        freshness: 'final',
        children: [
          {
            entity_path: 'Assembly.bolt_flange',
            kind: 'occurrence',
            type_name: 'Flange',
            has_mesh: false,
            trait_geometry: false,
            freshness: 'final',
            children: [
              { entity_path: 'Assembly.bolt_flange.geometry', kind: 'occurrence', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children: [] },
            ],
          },
        ],
      },
    ];

    const result = findFuzzyCandidate('Assembly.flange.geometry', null, testTree);
    expect(result).toEqual({ path: 'Assembly.bolt_flange.geometry' });
  });

  it('returns null when no suffix candidates exist', () => {
    const result = findFuzzyCandidate('Assembly.flange.geometry', null, []);
    expect(result).toBeNull();
  });

  it('returns null when 2 suffix candidates are narrowed to 0 by structural filter', () => {
    // Both suffix candidates fail structural match
    const testTree: EntityTreeNode[] = [
      {
        entity_path: 'Assembly',
        kind: 'structure',
        type_name: null,
        has_mesh: false,
        trait_geometry: false,
        freshness: 'final',
        children: [
          {
            entity_path: 'Assembly.part1',
            kind: 'occurrence',
            type_name: 'Washer', // wrong type
            has_mesh: false,
            trait_geometry: false,
            freshness: 'final',
            children: [
              { entity_path: 'Assembly.part1.geometry', kind: 'occurrence', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children: [] },
            ],
          },
          {
            entity_path: 'Assembly.part2',
            kind: 'occurrence',
            type_name: 'Washer', // wrong type
            has_mesh: false,
            trait_geometry: false,
            freshness: 'final',
            children: [
              { entity_path: 'Assembly.part2.geometry', kind: 'occurrence', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children: [] },
            ],
          },
        ],
      },
    ];

    const metadata: StalePathMetadata = { typeName: null, parentTypeName: 'Flange', childCount: 0 };
    const result = findFuzzyCandidate('Assembly.flange.geometry', metadata, testTree);
    expect(result).toBeNull();
  });

  it('returns { path } when structural filter narrows 2 candidates to 1', () => {
    // Two suffix candidates; only one matches structural metadata
    const testTree: EntityTreeNode[] = [
      {
        entity_path: 'Assembly',
        kind: 'structure',
        type_name: null,
        has_mesh: false,
        trait_geometry: false,
        freshness: 'final',
        children: [
          {
            entity_path: 'Assembly.bolt_flange',
            kind: 'occurrence',
            type_name: 'Flange', // matches metadata
            has_mesh: false,
            trait_geometry: false,
            freshness: 'final',
            children: [
              { entity_path: 'Assembly.bolt_flange.geometry', kind: 'occurrence', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children: [] },
            ],
          },
          {
            entity_path: 'Assembly.other_part',
            kind: 'occurrence',
            type_name: 'Washer', // doesn't match
            has_mesh: false,
            trait_geometry: false,
            freshness: 'final',
            children: [
              { entity_path: 'Assembly.other_part.geometry', kind: 'occurrence', type_name: null, has_mesh: false, trait_geometry: false, freshness: 'final', children: [] },
            ],
          },
        ],
      },
    ];

    // stalePath = "Assembly.flange.geometry" → both .bolt_flange.geometry and .other_part.geometry
    // are suffix candidates (both end in "geometry").
    // Metadata says the PARENT had type_name='Flange' and 1 child → only bolt_flange matches.
    const metadata: StalePathMetadata = { typeName: null, parentTypeName: 'Flange', childCount: 1 };
    const result = findFuzzyCandidate('Assembly.flange.geometry', metadata, testTree);
    expect(result).toEqual({ path: 'Assembly.bolt_flange.geometry' });
  });
});
