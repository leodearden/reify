import { describe, it, expect, expectTypeOf } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { createRoot, createComputed } from 'solid-js';
import { createViewStateStore } from '../stores/viewStateStore';
import type { ViewDefinition } from '../stores/autoViewGenerator';
import type { ViewStateStore } from '../stores';
import { makeNode, makeTree, makeTreeWithTwoSubtrees, makeTreeWithGeometryA } from './test-utils';

describe('viewStateStore — default rules', () => {
  it('node with trait_geometry=true → getEffectiveVisibility returns "show"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree([makeNode({ entity_path: 'Root', trait_geometry: true })]);
      expect(store.getEffectiveVisibility('Root')).toBe('show');
      dispose();
    });
  });

  it('node with kind="let" and type_name "Solid" → "hidden"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree([makeNode({ entity_path: 'Root.geo', kind: 'let', type_name: 'Solid' })]);
      expect(store.getEffectiveVisibility('Root.geo')).toBe('hidden');
      dispose();
    });
  });

  it('node with kind="param", trait_geometry=false → "show"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree([makeNode({ entity_path: 'Root.w', kind: 'param', trait_geometry: false })]);
      expect(store.getEffectiveVisibility('Root.w')).toBe('show');
      dispose();
    });
  });

  it('node with kind="structure" → "show"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree([makeNode({ entity_path: 'Root', kind: 'structure' })]);
      expect(store.getEffectiveVisibility('Root')).toBe('show');
      dispose();
    });
  });
});

describe('viewStateStore — setVisibility with cascade=true', () => {
  function makeTree() {
    // Root { A { a1 { a1x }, a2 }, B }
    return [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({
            entity_path: 'Root.A',
            kind: 'structure',
            children: [
              makeNode({
                entity_path: 'Root.A.a1',
                kind: 'param',
                children: [makeNode({ entity_path: 'Root.A.a1.a1x', kind: 'param' })],
              }),
              makeNode({ entity_path: 'Root.A.a2', kind: 'param' }),
            ],
          }),
          makeNode({ entity_path: 'Root.B', kind: 'structure' }),
        ],
      }),
    ];
  }

  it('sets target explicit to the given state', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      store.setVisibility('Root.A', 'ghost', true);
      expect(store.state.explicit['Root.A']).toBe('ghost');
      dispose();
    });
  });

  it('clears explicit on every descendant at every depth', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      // Prime descendants with explicit overrides
      store.setVisibility('Root.A.a1', 'show', false);
      store.setVisibility('Root.A.a1.a1x', 'hidden', false);
      store.setVisibility('Root.A.a2', 'show', false);
      // Now cascade-set A
      store.setVisibility('Root.A', 'ghost', true);
      expect(store.state.explicit['Root.A.a1']).toBeUndefined();
      expect(store.state.explicit['Root.A.a1.a1x']).toBeUndefined();
      expect(store.state.explicit['Root.A.a2']).toBeUndefined();
      dispose();
    });
  });

  it('does NOT touch siblings of target', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      store.setVisibility('Root.B', 'hidden', false);
      store.setVisibility('Root.A', 'ghost', true);
      // B is a sibling — its explicit stays 'hidden'
      expect(store.state.explicit['Root.B']).toBe('hidden');
      dispose();
    });
  });

  it('cascade clears a1 explicit so it inherits ghost from Root.A', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      // Prime a1 with explicit 'show'
      store.setVisibility('Root.A.a1', 'show', false);
      expect(store.state.explicit['Root.A.a1']).toBe('show');
      // Now cascade ghost onto Root.A
      store.setVisibility('Root.A', 'ghost', true);
      expect(store.state.explicit['Root.A.a1']).toBeUndefined();
      expect(store.getEffectiveVisibility('Root.A.a1')).toBe('ghost');
      dispose();
    });
  });
});

describe('viewStateStore — setVisibilityWithoutCascade and walk-up', () => {
  it('setVisibilityWithoutCascade does not clear any descendant explicit state', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      store.setVisibility('Root.A.a1', 'hidden', false);
      store.setVisibilityWithoutCascade('Root.A', 'ghost');
      // a1 must still have its own explicit 'hidden'
      expect(store.state.explicit['Root.A.a1']).toBe('hidden');
      dispose();
    });
  });

  it('after priming a1 with hidden then setVisibility(A, show, false), a1 remains hidden (effective)', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      store.setVisibility('Root.A.a1', 'hidden', false);
      // no-cascade variant — a1 keeps its own explicit
      store.setVisibility('Root.A', 'show', false);
      expect(store.getEffectiveVisibility('Root.A.a1')).toBe('hidden');
      dispose();
    });
  });

  it('getEffectiveVisibility walks up and returns first non-null ancestor explicit state', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      // Root has explicit 'ghost'; A.a1 has no explicit
      store.setVisibility('Root', 'ghost', false);
      expect(store.getEffectiveVisibility('Root.A.a1')).toBe('ghost');
      dispose();
    });
  });
});

describe('viewStateStore — resetToInherit', () => {
  it('clears explicit[path] to null', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      store.setVisibility('Root.A', 'hidden', false);
      store.resetToInherit('Root.A');
      expect(store.state.explicit['Root.A']).toBeUndefined();
      dispose();
    });
  });

  it('clears explicit on all descendants too', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      store.setVisibility('Root.A', 'hidden', false);
      store.setVisibility('Root.A.a1', 'show', false);
      store.setVisibility('Root.A.a2', 'ghost', false);
      store.resetToInherit('Root.A');
      expect(store.state.explicit['Root.A']).toBeUndefined();
      expect(store.state.explicit['Root.A.a1']).toBeUndefined();
      expect(store.state.explicit['Root.A.a2']).toBeUndefined();
      dispose();
    });
  });

  it('effective visibility after reset returns ancestor effective or default rule', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      store.setVisibility('Root', 'ghost', false);
      store.setVisibility('Root.A', 'hidden', false);
      store.resetToInherit('Root.A');
      // Root still has 'ghost', so A inherits ghost
      expect(store.getEffectiveVisibility('Root.A')).toBe('ghost');
      dispose();
    });
  });

  it('does not touch siblings or ancestors', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      store.setVisibility('Root', 'ghost', false);
      store.setVisibility('Root.B', 'hidden', false);
      store.setVisibility('Root.A', 'show', false);
      store.resetToInherit('Root.A');
      // Root and B must be unchanged
      expect(store.state.explicit['Root']).toBe('ghost');
      expect(store.state.explicit['Root.B']).toBe('hidden');
      dispose();
    });
  });

  it('has NO key in explicit for cleared path after resetToInherit', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      store.setVisibility('Root.A', 'hidden', false);
      store.resetToInherit('Root.A');
      expect('Root.A' in store.state.explicit).toBe(false);
      dispose();
    });
  });
});

describe('viewStateStore — showOnly', () => {
  it('showOnly(cascade=true): target has explicit show, all nodes not in {target, ancestors} are hidden', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTreeWithTwoSubtrees());
      store.showOnly('Root.A.a1', true);
      // Target
      expect(store.state.explicit['Root.A.a1']).toBe('show');
      // Ancestors: Root and Root.A should be null (not hidden)
      expect(store.state.explicit['Root']).toBeUndefined();
      expect(store.state.explicit['Root.A']).toBeUndefined();
      // Non-ancestors: B, b1, b2, a2 should be hidden
      expect(store.state.explicit['Root.A.a2']).toBe('hidden');
      expect(store.state.explicit['Root.B']).toBe('hidden');
      expect(store.state.explicit['Root.B.b1']).toBe('hidden');
      expect(store.state.explicit['Root.B.b2']).toBe('hidden');
      dispose();
    });
  });

  it('showOnly(cascade=true): descendants of target have explicit=null (inherit show)', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTreeWithTwoSubtrees());
      // Prime a1's child (add a deeper node)
      const tree = [
        makeNode({
          entity_path: 'Root',
          kind: 'structure',
          children: [
            makeNode({
              entity_path: 'Root.A',
              kind: 'structure',
              children: [
                makeNode({
                  entity_path: 'Root.A.a1',
                  kind: 'param',
                  children: [makeNode({ entity_path: 'Root.A.a1.x', kind: 'param' })],
                }),
              ],
            }),
          ],
        }),
      ];
      store.setTree(tree);
      store.setVisibility('Root.A.a1.x', 'hidden', false);
      store.showOnly('Root.A.a1', true);
      // cascade=true: descendants of a1 are cleared to null
      expect(store.state.explicit['Root.A.a1.x']).toBeUndefined();
      expect(store.getEffectiveVisibility('Root.A.a1.x')).toBe('show');
      dispose();
    });
  });

  it('showOnly(cascade=false): descendants of target are hidden (not cleared to null)', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTreeWithTwoSubtrees());
      store.showOnly('Root.A', false);
      // cascade=false: a1 and a2 are set hidden by the universal-hide pass (not null)
      expect(store.state.explicit['Root.A.a1']).toBe('hidden');
      expect(store.state.explicit['Root.A.a2']).toBe('hidden');
      // B hidden too
      expect(store.state.explicit['Root.B']).toBe('hidden');
      // target is show
      expect(store.state.explicit['Root.A']).toBe('show');
      // ancestor is null
      expect(store.state.explicit['Root']).toBeUndefined();
      dispose();
    });
  });

  it('ancestors of target have explicit=null so they do not block target', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTreeWithTwoSubtrees());
      // Pre-set an ancestor to hidden
      store.setVisibility('Root.A', 'hidden', false);
      store.showOnly('Root.A.a1', true);
      // After showOnly, ancestor Root.A must be null (not hidden) so a1 can show
      expect(store.state.explicit['Root.A']).toBeUndefined();
      expect(store.getEffectiveVisibility('Root.A.a1')).toBe('show');
      dispose();
    });
  });
});

describe('viewStateStore — getAllEffective', () => {
  it('returns Record covering every node with their resolved effective state', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTreeWithGeometryA());
      const all = store.getAllEffective();
      expect(Object.keys(all)).toHaveLength(4);
      expect(all['Root']).toBe('show');
      expect(all['Root.A']).toBe('show');
      expect(all['Root.A.a1']).toBe('show');
      expect(all['Root.B']).toBe('show');
      dispose();
    });
  });

  it('after setVisibility(hidden, cascade=true) every descendant appears as hidden', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTreeWithGeometryA());
      store.setVisibility('Root.A', 'hidden', true);
      const all = store.getAllEffective();
      expect(all['Root.A']).toBe('hidden');
      expect(all['Root.A.a1']).toBe('hidden');
      // sibling B unaffected
      expect(all['Root.B']).toBe('show');
      dispose();
    });
  });

  it('sibling with trait_geometry=true appears as show when no explicit set', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTreeWithGeometryA());
      store.setVisibility('Root.A', 'hidden', true);
      const all = store.getAllEffective();
      // Root.A has trait_geometry but is hidden by explicit
      expect(all['Root.A']).toBe('hidden');
      // Root.B has no explicit and no trait_geometry → default 'show'
      expect(all['Root.B']).toBe('show');
      dispose();
    });
  });
});

describe('viewStateStore — cycleCascading', () => {
  it('show → ghost', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      // Root.A default effective is 'show'
      store.cycleCascading('Root.A');
      expect(store.state.explicit['Root.A']).toBe('ghost');
      dispose();
    });
  });

  it('ghost → hidden', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      store.setVisibility('Root.A', 'ghost', false);
      store.cycleCascading('Root.A');
      expect(store.state.explicit['Root.A']).toBe('hidden');
      dispose();
    });
  });

  it('hidden → show', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      store.setVisibility('Root.A', 'hidden', false);
      store.cycleCascading('Root.A');
      expect(store.state.explicit['Root.A']).toBe('show');
      dispose();
    });
  });

  it('cycle cascades — descendants explicit becomes null', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      // Prime a1 with explicit
      store.setVisibility('Root.A.a1', 'hidden', false);
      // Cycle Root.A from show → ghost with cascade
      store.cycleCascading('Root.A');
      expect(store.state.explicit['Root.A']).toBe('ghost');
      expect(store.state.explicit['Root.A.a1']).toBeUndefined();
      // a1 inherits ghost from Root.A
      expect(store.getEffectiveVisibility('Root.A.a1')).toBe('ghost');
      dispose();
    });
  });
});

describe('viewStateStore — hasOverride', () => {
  it('explicit=null → false', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      expect(store.hasOverride('Root.A.a1')).toBe(false);
      dispose();
    });
  });

  it('root node has no explicit → false', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      expect(store.hasOverride('Root')).toBe(false);
      dispose();
    });
  });

  it('explicit differs from would-inherit → true', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      // Root default is 'show', A's parent (Root) effective is 'show'
      // Setting A to 'hidden' means A differs from parent's effective 'show'
      store.setVisibility('Root.A', 'hidden', false);
      expect(store.hasOverride('Root.A')).toBe(true);
      dispose();
    });
  });

  it('explicit matches would-inherit → false', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      // Root default is 'show'; set A to 'show' (same as would-inherit)
      store.setVisibility('Root.A', 'show', false);
      expect(store.hasOverride('Root.A')).toBe(false);
      dispose();
    });
  });

  it('a1 with explicit=null after cascade clear → false', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      // Set A to hidden cascade — a1's explicit cleared to null
      store.setVisibility('Root.A', 'hidden', true);
      expect(store.hasOverride('Root.A.a1')).toBe(false);
      dispose();
    });
  });
});

describe('viewStateStore — skeleton', () => {
  it('has empty explicit map on creation', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      expect(store.state.explicit).toEqual({});
      dispose();
    });
  });

  it('getEffectiveVisibility returns "show" when no tree is set (graceful fallback)', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      expect(store.getEffectiveVisibility('Root.A')).toBe('show');
      dispose();
    });
  });

  it('getAllEffective returns {} when no tree is set', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      expect(store.getAllEffective()).toEqual({});
      dispose();
    });
  });

  it('setTree populates internal maps — getEffectiveVisibility works for a known path', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const root = makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.A', kind: 'param', trait_geometry: false }),
        ],
      });
      store.setTree([root]);
      // With no explicit overrides, a plain param node uses default rule → 'show'
      expect(store.getEffectiveVisibility('Root.A')).toBe('show');
      dispose();
    });
  });
});

describe('viewStateStore — full PRD integration scenario', () => {
  it('simulates the full 4-step PRD scenario', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTreeWithTwoSubtrees());

      // Step 1: setVisibility('Root.A', 'ghost', true)
      // → A, a1, a2 all effective 'ghost'; B, b1, b2 still 'show'
      store.setVisibility('Root.A', 'ghost', true);
      expect(store.getEffectiveVisibility('Root.A')).toBe('ghost');
      expect(store.getEffectiveVisibility('Root.A.a1')).toBe('ghost');
      expect(store.getEffectiveVisibility('Root.A.a2')).toBe('ghost');
      expect(store.getEffectiveVisibility('Root.B')).toBe('show');
      expect(store.getEffectiveVisibility('Root.B.b1')).toBe('show');
      expect(store.getEffectiveVisibility('Root.B.b2')).toBe('show');

      // Step 2: setVisibilityWithoutCascade('Root.A.a1', 'show')
      // → a1 effective 'show', a2 still 'ghost'
      store.setVisibilityWithoutCascade('Root.A.a1', 'show');
      expect(store.getEffectiveVisibility('Root.A.a1')).toBe('show');
      expect(store.getEffectiveVisibility('Root.A.a2')).toBe('ghost');

      // Step 3: setVisibility('Root', 'hidden', true)
      // → cascade clears a1's override; everything hidden including a1
      store.setVisibility('Root', 'hidden', true);
      expect(store.getEffectiveVisibility('Root')).toBe('hidden');
      expect(store.state.explicit['Root.A.a1']).toBeUndefined();
      expect(store.getEffectiveVisibility('Root.A.a1')).toBe('hidden');
      expect(store.getEffectiveVisibility('Root.B.b2')).toBe('hidden');

      // Step 4: resetToInherit('Root')
      // → root cleared; everything reverts to default rule ('show' for param nodes)
      store.resetToInherit('Root');
      expect(store.state.explicit['Root']).toBeUndefined();
      expect(store.state.explicit['Root.A']).toBeUndefined();
      expect(store.state.explicit['Root.A.a1']).toBeUndefined();
      expect(store.getEffectiveVisibility('Root')).toBe('show');
      expect(store.getEffectiveVisibility('Root.A.a1')).toBe('show');
      expect(store.getEffectiveVisibility('Root.B.b2')).toBe('show');

      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// Source-text regression guard
// ---------------------------------------------------------------------------

describe('viewStateStore — source comments', () => {
  // Anchored to the first mutation function (setVisibility) so the guard detects
  // if the entire mutations block is deleted, not merely that the section header
  // is present. If a future refactor legitimately inserts a helper or comment
  // between the Mutations divider and setVisibility, update this regex and the
  // synthetic-source self-test below.
  const GUARD_REGEX = /-{5,}\n\s*\/\/ Mutations\n\s*\/\/ -{5,}\n\s*\n\s*function setVisibility\(/;

  it('section header uses bare // Mutations form without stale "stubs" phrasing', () => {
    const SRC_PATH = join(__dirname, '../stores/viewStateStore.ts');
    const src = readFileSync(SRC_PATH, 'utf-8');
    // Guard against re-introduction of the historical scaffolding comment
    // "Mutations (stubs — fully implemented in later steps)".
    // The regex anchors against surrounding divider lines to avoid false positives
    // from incidental occurrences of '// Mutations' elsewhere in the file.
    expect(src).not.toContain('stubs — fully implemented in later steps');
    expect(src).toMatch(GUARD_REGEX);
  });

  it('GUARD_REGEX requires setVisibility immediately after Mutations divider', () => {
    const synthetic = '// ---------\n  // Mutations\n  // ---------\n\n  // nothing here\n';
    const syntheticWithFn = '// ---------\n  // Mutations\n  // ---------\n\n  function setVisibility(path: string) {}\n';
    expect(GUARD_REGEX.test(synthetic)).toBe(false);
    expect(GUARD_REGEX.test(syntheticWithFn)).toBe(true);
  });
});

describe('viewStateStore — views map and activeViewId skeleton', () => {
  it('(a) on creation state.views === {} and state.activeViewId === "auto:default"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      expect(store.state.views).toEqual({});
      expect(store.state.activeViewId).toBe('auto:default');
      dispose();
    });
  });

  it('(b) setActiveView("auto:default") when view absent is a no-op — does not crash, does not touch explicit', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree([makeNode({ entity_path: 'Root' })]);
      store.setVisibility('Root', 'ghost', false);
      // No views seeded — setActiveView should not throw and should not clear explicit
      store.setActiveView('auto:default');
      expect(store.state.explicit['Root']).toBe('ghost');
      dispose();
    });
  });

  it('(c) after manually seeding state.views, setActiveView(id) copies visibility into state.explicit and updates activeViewId', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const testView: ViewDefinition = {
        id: 'auto:default',
        name: 'Default',
        auto: true,
        visibility: { 'Root': 'show', 'Root.geo': 'hidden' },
      };
      store.seedView(testView);
      store.setActiveView('auto:default');
      expect(store.state.activeViewId).toBe('auto:default');
      expect(store.state.explicit['Root']).toBe('show');
      expect(store.state.explicit['Root.geo']).toBe('hidden');
      dispose();
    });
  });
});

describe('viewStateStore — regenerateAutoViews — populate and preserve', () => {
  function makeSimpleTree() {
    return [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.geo', kind: 'param', trait_geometry: true }),
          makeNode({ entity_path: 'Root.body', kind: 'let', type_name: 'Solid' }),
        ],
      }),
    ];
  }

  it('(a) regenerateAutoViews(tree) populates state.views["auto:default"] and state.views["auto:all-geometry"]', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeSimpleTree());
      expect(store.state.views['auto:default']).toBeDefined();
      expect(store.state.views['auto:default'].id).toBe('auto:default');
      expect(store.state.views['auto:all-geometry']).toBeDefined();
      expect(store.state.views['auto:all-geometry'].id).toBe('auto:all-geometry');
      dispose();
    });
  });

  it('(b) regenerateAutoViews(tree, ["foo"]) also populates state.views["auto:purpose:foo"]', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeSimpleTree(), ['foo']);
      expect(store.state.views['auto:purpose:foo']).toBeDefined();
      expect(store.state.views['auto:purpose:foo'].id).toBe('auto:purpose:foo');
      dispose();
    });
  });

  it('(c) calling regenerateAutoViews twice removes stale auto:purpose:* entries', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeSimpleTree(), ['alpha']);
      expect(store.state.views['auto:purpose:alpha']).toBeDefined();
      // Second call with different purpose — alpha should be gone
      store.regenerateAutoViews(makeSimpleTree(), ['beta']);
      expect(store.state.views['auto:purpose:alpha']).toBeUndefined();
      expect(store.state.views['auto:purpose:beta']).toBeDefined();
      // default and all-geometry still present
      expect(store.state.views['auto:default']).toBeDefined();
      expect(store.state.views['auto:all-geometry']).toBeDefined();
      dispose();
    });
  });

  it('(d) a pre-existing "user:my-view" entry is preserved unchanged across regenerateAutoViews', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const userView: ViewDefinition = {
        id: 'user:my-view',
        name: 'My view',
        auto: false,
        visibility: { 'Root': 'hidden' },
      };
      store.seedView(userView);
      store.regenerateAutoViews(makeSimpleTree());
      expect(store.state.views['user:my-view']).toBeDefined();
      expect(store.state.views['user:my-view'].visibility['Root']).toBe('hidden');
      dispose();
    });
  });
});

describe('viewStateStore — regenerateAutoViews — active view reconciliation', () => {
  function makeTree() {
    return [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.geo', kind: 'param', trait_geometry: true }),
          makeNode({ entity_path: 'Root.body', kind: 'let', type_name: 'Solid' }),
        ],
      }),
    ];
  }

  it('(a) activeViewId="auto:default", regenerateAutoViews(tree) → state.explicit equals default view visibility', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      // activeViewId defaults to 'auto:default'
      store.regenerateAutoViews(makeTree());
      const defaultView = store.state.views['auto:default'];
      expect(store.state.explicit['Root']).toBe(defaultView.visibility['Root']);
      expect(store.state.explicit['Root.geo']).toBe(defaultView.visibility['Root.geo']);
      expect(store.state.explicit['Root.body']).toBe(defaultView.visibility['Root.body']);
      dispose();
    });
  });

  it('(b) after tree change removing an entity, regenerateAutoViews reapplies new default view and removed path is gone from explicit', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      // Now remove Root.body from the tree
      const reducedTree = [
        makeNode({
          entity_path: 'Root',
          kind: 'structure',
          children: [
            makeNode({ entity_path: 'Root.geo', kind: 'param', trait_geometry: true }),
          ],
        }),
      ];
      store.regenerateAutoViews(reducedTree);
      // Root.body no longer in tree → should not be in explicit
      expect(store.state.explicit['Root.body']).toBeUndefined();
      expect(store.state.explicit['Root.geo']).toBe('show');
      dispose();
    });
  });

  it('(c) activeViewId="auto:purpose:foo", regenerateAutoViews(tree, []) (purpose deactivated) → falls back to "auto:default"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      // First populate with foo purpose active
      store.regenerateAutoViews(makeTree(), ['foo']);
      store.setActiveView('auto:purpose:foo');
      expect(store.state.activeViewId).toBe('auto:purpose:foo');
      // Now regenerate without the purpose
      store.regenerateAutoViews(makeTree(), []);
      // Should fall back to auto:default
      expect(store.state.activeViewId).toBe('auto:default');
      const defaultView = store.state.views['auto:default'];
      expect(store.state.explicit['Root.body']).toBe(defaultView.visibility['Root.body']);
      dispose();
    });
  });

  it('(d) activeViewId="user:mine" — user explicit retained, new path Root.C not set in explicit', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const userView: ViewDefinition = {
        id: 'user:mine',
        name: 'Mine',
        auto: false,
        visibility: { 'Root.A': 'hidden' },
      };
      store.seedView(userView);
      store.setActiveView('user:mine');
      expect(store.state.activeViewId).toBe('user:mine');
      // Tree with Root.A and new Root.C
      const tree = [
        makeNode({
          entity_path: 'Root',
          kind: 'structure',
          children: [
            makeNode({ entity_path: 'Root.A', kind: 'param' }),
            makeNode({ entity_path: 'Root.C', kind: 'param' }),
          ],
        }),
      ];
      store.regenerateAutoViews(tree);
      // User view stays active
      expect(store.state.activeViewId).toBe('user:mine');
      // Existing path Root.A retains its user-view explicit state
      expect(store.state.explicit['Root.A']).toBe('hidden');
      // NEW path Root.C is NOT set (so defaultRuleFor handles it via walk-up)
      expect(store.state.explicit['Root.C']).toBeUndefined();
      dispose();
    });
  });
});

describe('viewStateStore — setTree pruning', () => {
  it('stale explicit entries for removed paths are pruned when setTree is called', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const nodeA = makeNode({ entity_path: 'Root.A' });
      const nodeB = makeNode({ entity_path: 'Root.B' });

      // Set up initial tree with A and B.
      store.setTree([nodeA, nodeB]);
      store.setVisibility('Root.A', 'hidden', false);
      store.setVisibility('Root.B', 'ghost', false);
      expect(store.state.explicit['Root.A']).toBe('hidden');
      expect(store.state.explicit['Root.B']).toBe('ghost');

      // Replace tree with only B — A is removed.
      store.setTree([nodeB]);
      // Stale entry for Root.A must be pruned.
      expect(store.state.explicit['Root.A']).toBeUndefined();
      // Root.B's explicit is preserved since it still exists.
      expect(store.state.explicit['Root.B']).toBe('ghost');

      dispose();
    });
  });

  it('re-introducing a previously-removed path does not inherit old explicit state', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const nodeA = makeNode({ entity_path: 'Root.A' });
      const nodeB = makeNode({ entity_path: 'Root.B' });

      store.setTree([nodeA, nodeB]);
      store.setVisibility('Root.A', 'hidden', false);

      // Remove A, then re-introduce it.
      store.setTree([nodeB]);
      store.setTree([nodeA, nodeB]);

      // Re-introduced A should have no explicit state (fresh inherit).
      expect(store.state.explicit['Root.A']).toBeUndefined();
      expect(store.getEffectiveVisibility('Root.A')).toBe('show');

      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// defaultRuleFor parity with defaultVisibilityFor (no flicker between views)
// ---------------------------------------------------------------------------

describe('viewStateStore — defaultRuleFor parity with defaultVisibilityFor (no flicker between views)', () => {
  it('(a) let node with type_name="Surface" and no explicit entry → getEffectiveVisibility returns "hidden"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const leaf = 'Root.surf';
      store.setTree([
        makeNode({
          entity_path: 'Root',
          kind: 'structure',
          children: [
            makeNode({ entity_path: leaf, kind: 'let', type_name: 'Surface' }),
          ],
        }),
      ]);
      // No explicit entry — falls through to defaultRuleFor.
      expect(store.getEffectiveVisibility(leaf)).toBe('hidden');
      dispose();
    });
  });

  it('(b) let node with type_name="Curve" and no explicit entry → getEffectiveVisibility returns "hidden"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const leaf = 'Root.crv';
      store.setTree([
        makeNode({
          entity_path: 'Root',
          kind: 'structure',
          children: [
            makeNode({ entity_path: leaf, kind: 'let', type_name: 'Curve' }),
          ],
        }),
      ]);
      expect(store.getEffectiveVisibility(leaf)).toBe('hidden');
      dispose();
    });
  });

  it('(c) let node with type_name="Solid" and no explicit entry → "hidden" (regression guard)', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const leaf = 'Root.solid';
      store.setTree([
        makeNode({
          entity_path: 'Root',
          kind: 'structure',
          children: [
            makeNode({ entity_path: leaf, kind: 'let', type_name: 'Solid' }),
          ],
        }),
      ]);
      expect(store.getEffectiveVisibility(leaf)).toBe('hidden');
      dispose();
    });
  });

  it('(d) no flicker when switching from auto:default to a sparse user view for a let-Surface node', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const leaf = 'Root.surf';
      const tree = [
        makeNode({
          entity_path: 'Root',
          kind: 'structure',
          children: [
            makeNode({ entity_path: leaf, kind: 'let', type_name: 'Surface' }),
          ],
        }),
      ];
      store.setTree(tree);

      // Seed an auto:default view that explicitly marks the leaf as 'hidden'.
      const defaultView: ViewDefinition = {
        id: 'auto:default',
        name: 'Default',
        auto: true,
        visibility: { Root: 'show', [leaf]: 'hidden' },
      };
      store.seedView(defaultView);
      store.setActiveView('auto:default');
      // explicit is now { Root: 'show', [leaf]: 'hidden' } → 'hidden'
      expect(store.getEffectiveVisibility(leaf)).toBe('hidden');

      // Now switch to a sparse user view with NO entry for the leaf.
      const userView: ViewDefinition = {
        id: 'user:sparse',
        name: 'Sparse',
        auto: false,
        visibility: {}, // intentionally no entry for leaf
      };
      store.seedView(userView);
      store.setActiveView('user:sparse');
      // explicit is now {} → falls through to defaultRuleFor.
      // defaultRuleFor now delegates to defaultVisibilityFor (anchored regex), so
      // 'let Surface' → 'hidden' (same as above, no flicker).
      expect(store.getEffectiveVisibility(leaf)).toBe('hidden');

      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// regenerateAutoViews — internal setTree contract
// ---------------------------------------------------------------------------

describe('viewStateStore — regenerateAutoViews — internal setTree contract', () => {
  it('regenerateAutoViews without a prior setTree call still populates nodeByPath so getEffectiveVisibility works', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      // Do NOT call setTree first — regenerateAutoViews should handle it internally.
      const tree = [
        makeNode({
          entity_path: 'Root',
          kind: 'structure',
          children: [
            makeNode({ entity_path: 'Root.geo', kind: 'param', trait_geometry: true }),
            makeNode({ entity_path: 'Root.body', kind: 'let', type_name: 'Solid' }),
          ],
        }),
      ];
      store.regenerateAutoViews(tree);
      // Internal maps populated — walk-up and default-rule both work.
      expect(store.getEffectiveVisibility('Root.geo')).toBe('show');
      expect(store.getEffectiveVisibility('Root.body')).toBe('hidden');
      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// Integration sanity: generator↔store wiring
// ---------------------------------------------------------------------------

describe('viewStateStore — auto view generators — integration sanity', () => {
  /**
   * Realistic tree: Assembly (structure)
   *   └─ Physical (structure)
   *        ├─ geometry  (param, trait_geometry=true)  → Default: show
   *        ├─ body1     (let, Solid)                  → Default: hidden
   *        └─ body2     (let, Option<Solid>)          → Default: hidden
   */
  function makeRealisticTree() {
    return [
      makeNode({
        entity_path: 'Assembly',
        kind: 'structure',
        children: [
          makeNode({
            entity_path: 'Assembly.Physical',
            kind: 'structure',
            children: [
              makeNode({ entity_path: 'Assembly.Physical.geometry', kind: 'param', trait_geometry: true }),
              makeNode({ entity_path: 'Assembly.Physical.body1', kind: 'let', type_name: 'Solid' }),
              makeNode({ entity_path: 'Assembly.Physical.body2', kind: 'let', type_name: 'Option<Solid>' }),
            ],
          }),
        ],
      }),
    ];
  }

  it('end-to-end: geometry shows + intermediates hidden under auto:default; all show under auto:all-geometry; auto:default restores hidden', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const tree = makeRealisticTree();

      // Wire both tree and views together as production code would.
      store.setTree(tree);
      store.regenerateAutoViews(tree);

      // (i) geometry param shows under auto:default
      expect(store.getAllEffective()['Assembly.Physical.geometry']).toBe('show');

      // (ii) let-Solid intermediates are hidden under auto:default
      expect(store.getAllEffective()['Assembly.Physical.body1']).toBe('hidden');
      expect(store.getAllEffective()['Assembly.Physical.body2']).toBe('hidden');

      // (iii) after setActiveView('auto:all-geometry') intermediates become visible
      store.setActiveView('auto:all-geometry');
      expect(store.state.activeViewId).toBe('auto:all-geometry');
      expect(store.getAllEffective()['Assembly.Physical.geometry']).toBe('show');
      expect(store.getAllEffective()['Assembly.Physical.body1']).toBe('show');
      expect(store.getAllEffective()['Assembly.Physical.body2']).toBe('show');

      // (iv) setActiveView('auto:default') restores intermediates to hidden
      store.setActiveView('auto:default');
      expect(store.state.activeViewId).toBe('auto:default');
      expect(store.getAllEffective()['Assembly.Physical.geometry']).toBe('show');
      expect(store.getAllEffective()['Assembly.Physical.body1']).toBe('hidden');
      expect(store.getAllEffective()['Assembly.Physical.body2']).toBe('hidden');

      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// regenerateAutoViews — atomicity (single reactive notification)
// ---------------------------------------------------------------------------

describe('regenerateAutoViews — atomicity (single reactive notification)', () => {
  // treeA: has Root and Root.stale — explicit entries seeded here become stale
  //         when regenerateAutoViews is called with treeB.
  // treeB: has Root and Root.fresh (no Root.stale) — the "new" tree used for
  //         the regenerateAutoViews call under test.
  function makeTreeA() {
    return [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [makeNode({ entity_path: 'Root.stale', kind: 'param' })],
      }),
    ];
  }

  function makeTreeB() {
    return [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [makeNode({ entity_path: 'Root.fresh', kind: 'param' })],
      }),
    ];
  }

  it('(a) auto:default branch: regenerateAutoViews fires exactly one reactive notification', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      // Prime a stale explicit entry so setTree's setState changes explicit.
      store.setTree(makeTreeA());
      store.setVisibility('Root.stale', 'hidden', false);

      // Counter starts at -1 to account for the initial createComputed run.
      let updateCount = -1;
      createComputed(() => {
        // Subscribe to the full state snapshot: any setState() call that touches
        // explicit, views, or activeViewId registers as exactly one notification.
        // Using store.state (rather than two independent sub-object reads) ensures
        // the count is stable across Solid version changes in access-tracking.
        JSON.stringify(store.state);
        updateCount++;
      });
      // Initial run sets counter to 0.
      expect(updateCount).toBe(0);

      // activeViewId defaults to 'auto:default'.
      store.regenerateAutoViews(makeTreeB());

      // Before fix: 2  — setTree's setState prunes explicit (notify 1);
      //                   second setState updates views + replaces explicit (notify 2).
      // After  fix: 1  — single batched setState does all work.
      expect(updateCount).toBe(1);
      dispose();
    });
  });

  it('(b) user:* branch: regenerateAutoViews fires exactly one reactive notification', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTreeA());

      // Seed and activate a user view so we enter the user:* reconciliation branch.
      const userView: ViewDefinition = {
        id: 'user:mine',
        name: 'Mine',
        auto: false,
        visibility: { 'Root.stale': 'hidden' },
      };
      store.seedView(userView);
      store.setActiveView('user:mine');
      // explicit is now { 'Root.stale': 'hidden' }

      let updateCount = -1;
      createComputed(() => {
        JSON.stringify(store.state);
        updateCount++;
      });
      expect(updateCount).toBe(0);

      store.regenerateAutoViews(makeTreeB());

      // Before fix: 2  — setTree prunes explicit (notify 1);
      //                   second setState refreshes auto:* views (notify 2).
      // After  fix: 1  — single batched setState.
      expect(updateCount).toBe(1);
      dispose();
    });
  });

  it('(c) unknown-view fallback branch: regenerateAutoViews fires exactly one reactive notification', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTreeA());

      // Seed and activate a view whose id starts with neither 'auto:' nor 'user:'.
      // This exercises the else-branch fallback inside regenerateAutoViews.
      const customView: ViewDefinition = {
        id: 'custom:test',
        name: 'Custom',
        auto: false,
        visibility: { 'Root.stale': 'hidden' },
      };
      store.seedView(customView);
      store.setActiveView('custom:test');
      // explicit is now { 'Root.stale': 'hidden' }

      let updateCount = -1;
      createComputed(() => {
        JSON.stringify(store.state);
        updateCount++;
      });
      expect(updateCount).toBe(0);

      store.regenerateAutoViews(makeTreeB());

      // Before fix: 2  — setTree prunes explicit (notify 1);
      //                   second setState updates views + replaces explicit (notify 2).
      // After  fix: 1  — single batched setState.
      expect(updateCount).toBe(1);
      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// setVisibility — user-view write-back
// ---------------------------------------------------------------------------

describe('setVisibility — user-view write-back', () => {
  // Tree: Root { Root.A { Root.A.a1 }, Root.B }
  function makeTree() {
    return [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({
            entity_path: 'Root.A',
            kind: 'structure',
            children: [
              makeNode({ entity_path: 'Root.A.a1', kind: 'param' }),
            ],
          }),
          makeNode({ entity_path: 'Root.B', kind: 'structure' }),
        ],
      }),
    ];
  }

  it('(a) non-cascade setVisibility mirrors to state.views[activeViewId].visibility', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());

      // Seed and activate a user view with a starting visibility entry.
      const userView: ViewDefinition = {
        id: 'user:mine',
        name: 'Mine',
        auto: false,
        visibility: { Root: 'show' },
      };
      store.seedView(userView);
      store.setActiveView('user:mine');

      // Mutate via setVisibility (no cascade).
      store.setVisibility('Root.A', 'hidden', false);

      // Mirror: state.views['user:mine'].visibility must reflect the mutation.
      expect(store.state.views['user:mine'].visibility['Root.A']).toBe('hidden');
      // The original entry should be preserved.
      expect(store.state.views['user:mine'].visibility['Root']).toBe('show');

      dispose();
    });
  });

  it('(b) cascade setVisibility mirrors non-null entries; null (inherit) entries are NOT mirrored', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());

      // Seed a user view with some existing entries.
      const userView: ViewDefinition = {
        id: 'user:mine',
        name: 'Mine',
        auto: false,
        visibility: { 'Root.A': 'ghost', 'Root.A.a1': 'show' },
      };
      store.seedView(userView);
      store.setActiveView('user:mine');

      // Cascade-set Root.A to 'hidden' — this writes null to all descendants.
      store.setVisibility('Root.A', 'hidden', true);

      // 'Root.A' is a non-null write → must be mirrored.
      expect(store.state.views['user:mine'].visibility['Root.A']).toBe('hidden');
      // 'Root.A.a1' got null in explicit (cascade-clear) → must NOT appear in the view.
      expect(store.state.views['user:mine'].visibility['Root.A.a1']).toBeUndefined();

      dispose();
    });
  });

  it('(c) when active view is auto:*, setVisibility does NOT mutate state.views[activeViewId].visibility', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());

      // Seed an auto view and activate it.
      const autoView: ViewDefinition = {
        id: 'auto:default',
        name: 'Default',
        auto: true,
        visibility: { Root: 'show', 'Root.A': 'show', 'Root.A.a1': 'show', 'Root.B': 'show' },
      };
      store.seedView(autoView);
      store.setActiveView('auto:default');

      const viewBefore = { ...store.state.views['auto:default'].visibility };

      // Mutate — active view is auto:*, mirror must NOT apply.
      store.setVisibility('Root.A', 'hidden', false);

      // The stored auto view's visibility must be unchanged.
      expect(store.state.views['auto:default'].visibility).toEqual(viewBefore);

      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// other mutations — user-view mirror
// ---------------------------------------------------------------------------

describe('other mutations — user-view mirror to active user view', () => {
  // Tree: Root { Root.A { Root.A.a1 }, Root.B }
  function makeTree() {
    return [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({
            entity_path: 'Root.A',
            kind: 'structure',
            children: [
              makeNode({ entity_path: 'Root.A.a1', kind: 'param' }),
            ],
          }),
          makeNode({ entity_path: 'Root.B', kind: 'structure' }),
        ],
      }),
    ];
  }

  function seedAndActivateUserView(store: ReturnType<typeof createViewStateStore>) {
    const userView: ViewDefinition = {
      id: 'user:mine',
      name: 'Mine',
      auto: false,
      visibility: {},
    };
    store.seedView(userView);
    store.setActiveView('user:mine');
  }

  it('(a) resetToInherit on user:* view — null-cleared path must NOT appear in mirrored view', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      seedAndActivateUserView(store);

      // Prime Root.A as hidden first (mirrors to user:mine).
      store.setVisibility('Root.A', 'hidden', false);
      expect(store.state.views['user:mine'].visibility['Root.A']).toBe('hidden');

      // Now reset Root.A to inherit — explicit['Root.A'] becomes null.
      store.resetToInherit('Root.A');

      // After reset, 'Root.A' is null in explicit → must NOT appear in mirrored view.
      expect(store.state.views['user:mine'].visibility['Root.A']).toBeUndefined();

      dispose();
    });
  });

  it('(b) showOnly on user:* view — target "show" mirrored; non-null "hidden" mirrored; null ancestors omitted', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      seedAndActivateUserView(store);

      store.showOnly('Root.A.a1', true);

      // Target must be mirrored as 'show'.
      expect(store.state.views['user:mine'].visibility['Root.A.a1']).toBe('show');
      // Non-ancestor non-target nodes get hidden → mirrored.
      expect(store.state.views['user:mine'].visibility['Root.B']).toBe('hidden');
      // Ancestors of target get null (cleared) in explicit → NOT mirrored (undefined).
      expect(store.state.views['user:mine'].visibility['Root']).toBeUndefined();
      expect(store.state.views['user:mine'].visibility['Root.A']).toBeUndefined();

      dispose();
    });
  });

  it('(c) setVisibilityWithoutCascade on user:* view — mirrors to stored view', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      seedAndActivateUserView(store);

      store.setVisibilityWithoutCascade('Root.A', 'ghost');

      expect(store.state.views['user:mine'].visibility['Root.A']).toBe('ghost');

      dispose();
    });
  });

  it('(d) cycleCascading on user:* view — mirrors the cycled state (via setVisibility)', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      seedAndActivateUserView(store);

      // Default effective for Root.A is 'show'; cycle → 'ghost'.
      store.cycleCascading('Root.A');

      expect(store.state.views['user:mine'].visibility['Root.A']).toBe('ghost');

      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// setActiveView — wholesale explicit replacement after resetToInherit
// ---------------------------------------------------------------------------

describe('setActiveView — wholesale explicit replacement after resetToInherit', () => {
  it('setActiveView replaces state.explicit wholesale, destroying prior resetToInherit absent-key markers', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();

      const vA: ViewDefinition = {
        id: 'user:vA',
        name: 'View A',
        auto: false,
        visibility: { 'Root': 'hidden', 'Root.A': 'ghost' },
      };
      const vB: ViewDefinition = {
        id: 'user:vB',
        name: 'View B',
        auto: false,
        visibility: { 'Root': 'show', 'Root.A': 'show', 'Root.B': 'hidden' },
      };

      store.seedView(vA);
      store.seedView(vB);
      store.setActiveView('user:vA');

      // vA is active; Root.A enters explicit as 'ghost'.
      expect(store.state.activeViewId).toBe('user:vA');
      expect(store.state.explicit['Root.A']).toBe('ghost');

      // resetToInherit deletes the key — absence-of-key invariant.
      store.resetToInherit('Root.A');
      expect('Root.A' in store.state.explicit).toBe(false);

      // Switching to vB must wholesale-replace explicit, restoring Root.A = 'show'
      // and destroying the prior absent-key (inherit) marker for Root.A.
      store.setActiveView('user:vB');
      expect(store.state.explicit).toEqual(vB.visibility);
      expect(store.state.explicit['Root.A']).toBe('show');

      dispose();
    });
  });
});

describe('viewStateStore — ViewStateStore type export', () => {
  it('ViewStateStore type alias is structurally identical to ReturnType<typeof createViewStateStore>', () => {
    expectTypeOf<ViewStateStore>().toEqualTypeOf<ReturnType<typeof createViewStateStore>>();
  });
});

// ---------------------------------------------------------------------------
// createView + switchView
// ---------------------------------------------------------------------------

describe('viewStateStore — createView', () => {
  it('(a) returns a new id of the form user:<slug>', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id = store.createView('My View');
      expect(id).toMatch(/^user:/);
      dispose();
    });
  });

  it('(b) adds the view to state.views with auto: false, modified: false, empty visibility', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id = store.createView('My View');
      const view = store.state.views[id];
      expect(view).toBeDefined();
      expect(view.auto).toBe(false);
      expect(view.modified).toBe(false);
      expect(view.visibility).toEqual({});
      dispose();
    });
  });

  it('(c) appends the new id to state.userViewOrder', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id1 = store.createView('First');
      const id2 = store.createView('Second');
      expect(store.state.userViewOrder).toContain(id1);
      expect(store.state.userViewOrder).toContain(id2);
      // id1 should come before id2 (appended in order)
      expect(store.state.userViewOrder.indexOf(id1)).toBeLessThan(
        store.state.userViewOrder.indexOf(id2),
      );
      dispose();
    });
  });

  it('(d) the view does NOT become active automatically', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const previousActiveId = store.state.activeViewId;
      store.createView('New View');
      // activeViewId should not change
      expect(store.state.activeViewId).toBe(previousActiveId);
      dispose();
    });
  });

  it('(e) name is reflected in the stored view', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id = store.createView('Custom Name');
      expect(store.state.views[id].name).toBe('Custom Name');
      dispose();
    });
  });

  it('(f) state.userViewOrder is empty on fresh store creation', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      expect(store.state.userViewOrder).toEqual([]);
      dispose();
    });
  });
});

describe('viewStateStore — switchView', () => {
  it('(a) sets activeViewId to the given id and returns true', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews([makeNode({ entity_path: 'Root' })]);
      const result = store.switchView('auto:default');
      expect(result).toBe(true);
      expect(store.state.activeViewId).toBe('auto:default');
      dispose();
    });
  });

  it('(b) returns false and does NOT change activeViewId for an unknown view id', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews([makeNode({ entity_path: 'Root' })]);
      const prevActive = store.state.activeViewId;
      const result = store.switchView('nonexistent:view');
      expect(result).toBe(false);
      expect(store.state.activeViewId).toBe(prevActive);
      dispose();
    });
  });

  it('(c) switching to a user view via switchView makes it active', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews([makeNode({ entity_path: 'Root' })]);
      const id = store.createView('My View');
      const result = store.switchView(id);
      expect(result).toBe(true);
      expect(store.state.activeViewId).toBe(id);
      dispose();
    });
  });
});
