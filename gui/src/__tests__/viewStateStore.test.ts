import { describe, it, expect, expectTypeOf } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { createRoot, createComputed } from 'solid-js';
import { createViewStateStore } from '../stores/viewStateStore';
import type { ViewDefinition } from '../stores/autoViewGenerator';
import type { ViewStateStore } from '../stores';
import type { PersistentViewState } from '../types';
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

describe('viewStateStore — setTree stale-entry preservation', () => {
  // NOTE: Prior to step-18 (task 1749) this describe block was titled
  // "setTree pruning" and asserted that stale explicit entries were deleted on
  // tree change.  Step-18 removed that pruning loop per PRD §8.2: stale
  // entries must survive for undo/branch-switch restoration.  Tests updated
  // accordingly.

  it('stale explicit entries for removed paths are PRESERVED when setTree is called', () => {
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

      // Replace tree with only B — A becomes stale.
      store.setTree([nodeB]);
      // Stale entry for Root.A must be preserved (PRD §8.2).
      expect(store.state.explicit['Root.A']).toBe('hidden');
      // Root.B's explicit is preserved since it still exists.
      expect(store.state.explicit['Root.B']).toBe('ghost');

      dispose();
    });
  });

  it('re-introducing a previously-removed path re-surfaces its prior explicit state', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const nodeA = makeNode({ entity_path: 'Root.A' });
      const nodeB = makeNode({ entity_path: 'Root.B' });

      store.setTree([nodeA, nodeB]);
      store.setVisibility('Root.A', 'hidden', false);

      // Remove A (becomes stale), then re-introduce it.
      store.setTree([nodeB]);
      store.setTree([nodeA, nodeB]);

      // Re-introduced A should inherit its prior explicit state (PRD §8.2).
      expect(store.state.explicit['Root.A']).toBe('hidden');
      expect(store.getEffectiveVisibility('Root.A')).toBe('hidden');

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

// ---------------------------------------------------------------------------
// COW — copy-on-write via setVisibility
// ---------------------------------------------------------------------------

describe('viewStateStore — COW on setVisibility', () => {
  function makeTree() {
    return [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.A', kind: 'param' }),
          makeNode({ entity_path: 'Root.B', kind: 'param' }),
        ],
      }),
    ];
  }

  it('(a) with auto view active, setVisibility creates a new user view named "{autoName} (modified)"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      // activeViewId defaults to auto:default
      expect(store.state.activeViewId).toBe('auto:default');

      store.setVisibility('Root.A', 'hidden');
      // A new user view should have been created
      const userViewIds = Object.keys(store.state.views).filter((k) => k.startsWith('user:'));
      expect(userViewIds).toHaveLength(1);
      const cowView = store.state.views[userViewIds[0]];
      expect(cowView.name).toBe('Default (modified)');
      expect(cowView.modified).toBe(true);
      dispose();
    });
  });

  it('(a) with auto view active, setVisibility switches to the new COW user view', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());

      store.setVisibility('Root.A', 'hidden');
      const activeId = store.state.activeViewId;
      expect(activeId).toMatch(/^user:/);
      expect(store.state.views[activeId].name).toBe('Default (modified)');
      dispose();
    });
  });

  it('(a) the mutation is recorded in the COW view visibility', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());

      store.setVisibility('Root.A', 'hidden');
      const activeId = store.state.activeViewId;
      expect(store.state.views[activeId].visibility['Root.A']).toBe('hidden');
      dispose();
    });
  });

  it('(b) subsequent setVisibility calls on the now-active user view do NOT create additional COW views', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());

      store.setVisibility('Root.A', 'hidden');
      const firstCowId = store.state.activeViewId;
      const userViewCountAfterFirst = Object.keys(store.state.views).filter((k) =>
        k.startsWith('user:'),
      ).length;
      expect(userViewCountAfterFirst).toBe(1);

      // Second mutation — must NOT create another user view
      store.setVisibility('Root.B', 'ghost');
      const userViewCountAfterSecond = Object.keys(store.state.views).filter((k) =>
        k.startsWith('user:'),
      ).length;
      expect(userViewCountAfterSecond).toBe(1);
      // Still the same user view
      expect(store.state.activeViewId).toBe(firstCowId);
      dispose();
    });
  });

  it('(c) the original auto view is untouched after COW', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      const originalAutoVisibility = { ...store.state.views['auto:default'].visibility };

      store.setVisibility('Root.A', 'hidden');
      // The auto:default visibility must not have changed
      expect(store.state.views['auto:default'].visibility).toEqual(originalAutoVisibility);
      dispose();
    });
  });

  it('(d) collision: if "{autoName} (modified)" already exists, uses "{autoName} (modified 2)"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());

      // First COW: creates "Default (modified)"
      store.setVisibility('Root.A', 'hidden');
      expect(store.state.views[store.state.activeViewId].name).toBe('Default (modified)');

      // Switch back to auto:default and trigger another COW
      store.setActiveView('auto:default');
      store.setVisibility('Root.B', 'ghost');
      // "Default (modified)" is already taken → should use "Default (modified 2)"
      expect(store.state.views[store.state.activeViewId].name).toBe('Default (modified 2)');
      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// COW — copy-on-write via remaining mutation entry points
// ---------------------------------------------------------------------------

describe('viewStateStore — COW on setVisibilityWithoutCascade, resetToInherit, showOnly, cycleCascading', () => {
  function makeTree() {
    return [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.A', kind: 'param' }),
          makeNode({ entity_path: 'Root.B', kind: 'param' }),
        ],
      }),
    ];
  }

  /**
   * Common assertion: after calling a mutation while an auto view is active,
   * exactly one user view should exist, it should be named "{autoName} (modified)",
   * it should have modified: true, and it should be the active view.
   */
  function assertCowProduced(store: ReturnType<typeof createViewStateStore>, autoName = 'Default') {
    const userViewIds = Object.keys(store.state.views).filter((k) => k.startsWith('user:'));
    expect(userViewIds).toHaveLength(1);
    const cowView = store.state.views[userViewIds[0]];
    expect(cowView.name).toBe(`${autoName} (modified)`);
    expect(cowView.modified).toBe(true);
    expect(store.state.activeViewId).toBe(userViewIds[0]);
  }

  it('setVisibilityWithoutCascade triggers COW when an auto view is active', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      expect(store.state.activeViewId).toBe('auto:default');

      store.setVisibilityWithoutCascade('Root.A', 'hidden');
      assertCowProduced(store);
      // Mutation is recorded in the COW view
      expect(store.state.views[store.state.activeViewId].visibility['Root.A']).toBe('hidden');
      dispose();
    });
  });

  it('setVisibilityWithoutCascade does NOT trigger COW when a user view is already active', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      const uid = store.createView('My View');
      store.switchView(uid);

      store.setVisibilityWithoutCascade('Root.A', 'hidden');
      // Still only one user view (the one we created, no new COW)
      const userViewIds = Object.keys(store.state.views).filter((k) => k.startsWith('user:'));
      expect(userViewIds).toHaveLength(1);
      expect(store.state.activeViewId).toBe(uid);
      dispose();
    });
  });

  it('resetToInherit triggers COW when an auto view is active', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      expect(store.state.activeViewId).toBe('auto:default');

      store.resetToInherit('Root.A');
      assertCowProduced(store);
      // After COW + resetToInherit, Root.A should have no explicit entry
      expect(store.state.explicit['Root.A']).toBeUndefined();
      dispose();
    });
  });

  it('resetToInherit does NOT trigger COW when a user view is already active', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      const uid = store.createView('My View');
      store.switchView(uid);

      store.resetToInherit('Root.A');
      const userViewIds = Object.keys(store.state.views).filter((k) => k.startsWith('user:'));
      expect(userViewIds).toHaveLength(1);
      expect(store.state.activeViewId).toBe(uid);
      dispose();
    });
  });

  it('showOnly triggers COW when an auto view is active', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      expect(store.state.activeViewId).toBe('auto:default');

      store.showOnly('Root.A', true);
      assertCowProduced(store);
      // Target is explicit 'show' in the COW view
      expect(store.state.views[store.state.activeViewId].visibility['Root.A']).toBe('show');
      dispose();
    });
  });

  it('showOnly does NOT trigger COW when a user view is already active', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      const uid = store.createView('My View');
      store.switchView(uid);

      store.showOnly('Root.A', true);
      const userViewIds = Object.keys(store.state.views).filter((k) => k.startsWith('user:'));
      expect(userViewIds).toHaveLength(1);
      expect(store.state.activeViewId).toBe(uid);
      dispose();
    });
  });

  it('cycleCascading triggers COW when an auto view is active', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      expect(store.state.activeViewId).toBe('auto:default');

      // Root.A default effective is 'show'; cycle → 'ghost'
      store.cycleCascading('Root.A');
      assertCowProduced(store);
      expect(store.state.views[store.state.activeViewId].visibility['Root.A']).toBe('ghost');
      dispose();
    });
  });

  it('cycleCascading does NOT trigger COW when a user view is already active', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      const uid = store.createView('My View');
      store.switchView(uid);

      store.cycleCascading('Root.A');
      const userViewIds = Object.keys(store.state.views).filter((k) => k.startsWith('user:'));
      expect(userViewIds).toHaveLength(1);
      expect(store.state.activeViewId).toBe(uid);
      dispose();
    });
  });

  it('original auto view is untouched after COW via setVisibilityWithoutCascade', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      const originalAutoVisibility = { ...store.state.views['auto:default'].visibility };

      store.setVisibilityWithoutCascade('Root.A', 'hidden');
      expect(store.state.views['auto:default'].visibility).toEqual(originalAutoVisibility);
      dispose();
    });
  });

  it('original auto view is untouched after COW via showOnly', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      const originalAutoVisibility = { ...store.state.views['auto:default'].visibility };

      store.showOnly('Root.A', true);
      expect(store.state.views['auto:default'].visibility).toEqual(originalAutoVisibility);
      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// reorderUserViews
// ---------------------------------------------------------------------------

describe('viewStateStore — reorderUserViews', () => {
  it('(a) replaces state.userViewOrder when given a valid permutation', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id1 = store.createView('First');
      const id2 = store.createView('Second');
      const id3 = store.createView('Third');
      // Reorder: put Third first
      const result = store.reorderUserViews([id3, id1, id2]);
      expect(result).toBe(true);
      expect(store.state.userViewOrder).toEqual([id3, id1, id2]);
      dispose();
    });
  });

  it('(b) rejects if any current user-view id is missing from the argument', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id1 = store.createView('First');
      const id2 = store.createView('Second');
      const before = [...store.state.userViewOrder];
      // Missing id2
      const result = store.reorderUserViews([id1]);
      expect(result).toBe(false);
      expect(store.state.userViewOrder).toEqual(before);
      dispose();
    });
  });

  it('(b) rejects if the argument contains unknown ids', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id1 = store.createView('First');
      const before = [...store.state.userViewOrder];
      const result = store.reorderUserViews([id1, 'user:nonexistent']);
      expect(result).toBe(false);
      expect(store.state.userViewOrder).toEqual(before);
      dispose();
    });
  });

  it('(b) rejects if the argument has duplicates', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id1 = store.createView('First');
      const before = [...store.state.userViewOrder];
      const result = store.reorderUserViews([id1, id1]);
      expect(result).toBe(false);
      expect(store.state.userViewOrder).toEqual(before);
      dispose();
    });
  });

  it('(b) rejects if the argument contains auto view ids', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id1 = store.createView('First');
      store.regenerateAutoViews([makeNode({ entity_path: 'Root' })]);
      const before = [...store.state.userViewOrder];
      // auto:default is not a user view
      const result = store.reorderUserViews(['auto:default', id1]);
      expect(result).toBe(false);
      expect(store.state.userViewOrder).toEqual(before);
      dispose();
    });
  });

  it('(a) empty array is valid when there are no user views', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      // No user views created yet
      const result = store.reorderUserViews([]);
      expect(result).toBe(true);
      expect(store.state.userViewOrder).toEqual([]);
      dispose();
    });
  });

  it('(f) after reorderUserViews, every id returned by getOrderedViewIds resolves to a view in state.views (transactional invariant)', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews([makeNode({ entity_path: 'Root' })]);
      const id1 = store.createView('First');
      const id2 = store.createView('Second');
      const id3 = store.createView('Third');

      const result = store.reorderUserViews([id3, id1, id2]);
      expect(result).toBe(true);

      const ordered = store.getOrderedViewIds();
      // Every id must resolve to a view in state.views
      for (const id of ordered) {
        expect(store.state.views[id]).toBeDefined();
      }
      // User-view suffix should reflect the new order
      const userSuffix = ordered.filter((id) => !id.startsWith('auto:'));
      expect(userSuffix).toEqual([id3, id1, id2]);
      // Auto views still precede user views — every auto id must come before every user id
      const autoIndices = ordered
        .map((id, i) => (id.startsWith('auto:') ? i : -1))
        .filter((i) => i >= 0);
      const userIndices = ordered
        .map((id, i) => (!id.startsWith('auto:') ? i : -1))
        .filter((i) => i >= 0);
      expect(autoIndices.length).toBeGreaterThan(0);
      expect(Math.max(...autoIndices)).toBeLessThan(Math.min(...userIndices));
      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// duplicateView
// ---------------------------------------------------------------------------

describe('viewStateStore — duplicateView', () => {
  function makeTree() {
    return [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.A', kind: 'param' }),
          makeNode({ entity_path: 'Root.B', kind: 'param' }),
        ],
      }),
    ];
  }

  it('(a) auto→user duplication produces a user view with auto: false, modified: false, and visibility snapshot from source', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      const autoView = store.state.views['auto:default'];

      const newId = store.duplicateView('auto:default');
      expect(newId).not.toBeNull();
      expect(newId!).toMatch(/^user:/);

      const dupView = store.state.views[newId!];
      expect(dupView.auto).toBe(false);
      expect(dupView.modified).toBe(false);
      // Visibility snapshot equals source auto view's visibility
      expect(dupView.visibility).toEqual(autoView.visibility);
      dispose();
    });
  });

  it('(b) user→user duplication copies visibility verbatim', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      // Use seedView to set up a user view with non-empty visibility; direct
      // property assignment on SolidJS store state proxies does not trigger
      // reactive updates, so the proper store mutation path must be used.
      const sourceId = 'user:source';
      store.seedView({
        id: sourceId,
        name: 'Source',
        auto: false,
        visibility: { 'Root': 'hidden', 'Root.A': 'show' },
      });

      const newId = store.duplicateView(sourceId);
      expect(newId).not.toBeNull();
      const dupView = store.state.views[newId!];
      expect(dupView.visibility).toEqual({ 'Root': 'hidden', 'Root.A': 'show' });
      dispose();
    });
  });

  it('(c) default name is {sourceName} (copy)', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());

      const newId = store.duplicateView('auto:default');
      expect(store.state.views[newId!].name).toBe('Default (copy)');
      dispose();
    });
  });

  it('(c) counter-suffix collision: if "{sourceName} (copy)" already exists, uses "{sourceName} (copy 2)"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());

      const id1 = store.duplicateView('auto:default');
      expect(store.state.views[id1!].name).toBe('Default (copy)');

      // Duplicate again — "Default (copy)" is taken, should get "Default (copy 2)"
      const id2 = store.duplicateView('auto:default');
      expect(store.state.views[id2!].name).toBe('Default (copy 2)');

      // Duplicate a third time
      const id3 = store.duplicateView('auto:default');
      expect(store.state.views[id3!].name).toBe('Default (copy 3)');
      dispose();
    });
  });

  it('(c) explicit newName overrides the default copy name', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());

      const newId = store.duplicateView('auto:default', 'My Snapshot');
      expect(store.state.views[newId!].name).toBe('My Snapshot');
      dispose();
    });
  });

  it('(d) returns the new id; unknown source returns null', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const result = store.duplicateView('user:nonexistent');
      expect(result).toBeNull();
      dispose();
    });
  });

  it('(d) new id is appended to state.userViewOrder', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeTree());
      const newId = store.duplicateView('auto:default');
      expect(store.state.userViewOrder).toContain(newId!);
      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// deleteView
// ---------------------------------------------------------------------------

describe('viewStateStore — deleteView', () => {
  it('(a) removes the view from state.views and state.userViewOrder', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id = store.createView('To Delete');
      expect(store.state.views[id]).toBeDefined();
      expect(store.state.userViewOrder).toContain(id);

      const result = store.deleteView(id);
      expect(result).toBe(true);
      expect(store.state.views[id]).toBeUndefined();
      expect(store.state.userViewOrder).not.toContain(id);
      dispose();
    });
  });

  it('(b) rejects auto views — returns false, no state change', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews([makeNode({ entity_path: 'Root' })]);
      const result = store.deleteView('auto:default');
      expect(result).toBe(false);
      expect(store.state.views['auto:default']).toBeDefined();
      dispose();
    });
  });

  it('(c) rejects unknown ids — returns false', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const result = store.deleteView('user:nonexistent');
      expect(result).toBe(false);
      dispose();
    });
  });

  it('(d) deleting the active user view falls back to activeViewId === "auto:default"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews([
        makeNode({
          entity_path: 'Root',
          kind: 'structure',
          children: [makeNode({ entity_path: 'Root.A', kind: 'param' })],
        }),
      ]);

      const id = store.createView('Active View');
      store.switchView(id);
      expect(store.state.activeViewId).toBe(id);

      const result = store.deleteView(id);
      expect(result).toBe(true);
      // Falls back to auto:default
      expect(store.state.activeViewId).toBe('auto:default');
      // Explicit state is re-seeded from auto:default
      expect(store.state.explicit).toEqual(store.state.views['auto:default'].visibility);
      dispose();
    });
  });

  it('(d) deleting a non-active user view does NOT change activeViewId', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews([makeNode({ entity_path: 'Root' })]);
      const id1 = store.createView('Active');
      const id2 = store.createView('Inactive');
      store.switchView(id1);
      expect(store.state.activeViewId).toBe(id1);

      store.deleteView(id2);
      // activeViewId should not change
      expect(store.state.activeViewId).toBe(id1);
      dispose();
    });
  });

  it('(e) after deleteView, every id returned by getOrderedViewIds resolves to a view in state.views (transactional invariant)', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews([makeNode({ entity_path: 'Root' })]);
      const idA = store.createView('A');
      const idB = store.createView('B');
      const idC = store.createView('C');

      store.deleteView(idB);

      const ordered = store.getOrderedViewIds();
      // Deleted id must not appear
      expect(ordered).not.toContain(idB);
      // Every remaining id must resolve to a view in state.views
      for (const id of ordered) {
        expect(store.state.views[id]).toBeDefined();
      }
      // The surviving user views are still present
      expect(ordered).toContain(idA);
      expect(ordered).toContain(idC);
      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// renameView
// ---------------------------------------------------------------------------

describe('viewStateStore — renameView', () => {
  function makeStoreWithUserView() {
    let store: ReturnType<typeof createViewStateStore>;
    let id: string;
    createRoot((dispose) => {
      store = createViewStateStore();
      id = store.createView('Original Name');
      // keep root alive — tests call dispose() themselves
    });
    return { store: store!, id: id! };
  }

  it('(a) updates views[id].name when given a valid new name', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id = store.createView('Original');
      const result = store.renameView(id, 'Updated');
      expect(result).toBe(true);
      expect(store.state.views[id].name).toBe('Updated');
      dispose();
    });
  });

  it('(b) rejects empty name — returns false, no state change', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id = store.createView('Original');
      const result = store.renameView(id, '');
      expect(result).toBe(false);
      expect(store.state.views[id].name).toBe('Original');
      dispose();
    });
  });

  it('(b) rejects whitespace-only name — returns false, no state change', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id = store.createView('Original');
      const result = store.renameView(id, '   ');
      expect(result).toBe(false);
      expect(store.state.views[id].name).toBe('Original');
      dispose();
    });
  });

  it('(c) rejects duplicate name (case-insensitive) — returns false, no state change', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id1 = store.createView('Alpha');
      const id2 = store.createView('Beta');
      // Try to rename id2 to "alpha" (duplicate of id1, case-insensitive)
      const result = store.renameView(id2, 'alpha');
      expect(result).toBe(false);
      expect(store.state.views[id2].name).toBe('Beta');
      dispose();
    });
  });

  it('(c) allows renaming to same name (case-insensitive same-id is not a collision)', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const id = store.createView('Alpha');
      // Renaming Alpha to "Alpha" (same) should succeed
      const result = store.renameView(id, 'Alpha');
      expect(result).toBe(true);
      expect(store.state.views[id].name).toBe('Alpha');
      dispose();
    });
  });

  it('(d) rejects renaming auto views — returns false', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews([makeNode({ entity_path: 'Root' })]);
      const result = store.renameView('auto:default', 'My Default');
      expect(result).toBe(false);
      // auto view name unchanged
      expect(store.state.views['auto:default'].name).toBe('Default');
      dispose();
    });
  });

  it('(d) rejects renaming unknown ids — returns false', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      const result = store.renameView('user:nonexistent', 'New Name');
      expect(result).toBe(false);
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

// ---------------------------------------------------------------------------
// Stale path preservation — step-17 tests (fail until step-18 removes prunes)
// ---------------------------------------------------------------------------

describe('viewStateStore — stale path preservation (step-17)', () => {
  /** Build a single-root tree with an optional child path. */
  function treeWith(child?: string) {
    if (!child) return [makeNode({ entity_path: 'Root', kind: 'structure' })];
    return [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [makeNode({ entity_path: child, kind: 'occurrence' })],
      }),
    ];
  }

  it('(a) regenerateAutoViews with a tree omitting the path preserves explicit entry and user-view visibility', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();

      // Generate auto views with the path present
      store.regenerateAutoViews(treeWith('Root.moving_part'));

      // Switch to a user view so the explicit map is preserved across regen
      const viewId = store.createView('My View');
      store.switchView(viewId);

      // Record an explicit override on the path
      store.setVisibility('Root.moving_part', 'hidden', false);
      expect(store.state.explicit['Root.moving_part']).toBe('hidden');
      expect(store.state.views[viewId].visibility['Root.moving_part']).toBe('hidden');

      // Regenerate with a tree that omits the path
      store.regenerateAutoViews(treeWith());

      // After step-18: stale entry must survive (currently FAILS — it's pruned)
      expect(store.state.explicit['Root.moving_part']).toBe('hidden');
      expect(store.state.views[viewId].visibility['Root.moving_part']).toBe('hidden');

      dispose();
    });
  });

  it('(b) regenerateAutoViews again with the path restored re-surfaces the persisted visibility', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();

      store.regenerateAutoViews(treeWith('Root.moving_part'));
      const viewId = store.createView('My View');
      store.switchView(viewId);
      store.setVisibility('Root.moving_part', 'hidden', false);

      // Remove the path from the tree
      store.regenerateAutoViews(treeWith());

      // Restore the path — the 'hidden' visibility should re-surface
      store.regenerateAutoViews(treeWith('Root.moving_part'));

      // After step-18: getEffectiveVisibility should return 'hidden' (preserved value)
      expect(store.getEffectiveVisibility('Root.moving_part')).toBe('hidden');

      dispose();
    });
  });

  it('(c) setTree alone preserves explicit entry for a path no longer in the new tree', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();

      // Set the tree with the path — then add an explicit override
      store.setTree(treeWith('Root.moving_part'));
      store.setVisibility('Root.moving_part', 'hidden', false);
      expect(store.state.explicit['Root.moving_part']).toBe('hidden');

      // Replace tree WITHOUT the path — currently prunes the entry
      store.setTree(treeWith());

      // After step-18: entry must survive
      expect(store.state.explicit['Root.moving_part']).toBe('hidden');

      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// getStalePaths — step-19 tests (fail until step-20 adds the accessor)
// ---------------------------------------------------------------------------

describe('viewStateStore — getStalePaths (step-19)', () => {
  function treeWith(child?: string) {
    if (!child) return [makeNode({ entity_path: 'Root', kind: 'structure' })];
    return [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [makeNode({ entity_path: child, kind: 'occurrence' })],
      }),
    ];
  }

  it('(a) returns paths present in state.explicit but absent from the current tree', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();

      store.setTree(treeWith('Root.going_away'));
      store.setVisibility('Root.going_away', 'hidden', false);

      // Remove the path
      store.setTree(treeWith());

      // getStalePaths should report it
      expect((store as any).getStalePaths()).toContain('Root.going_away');

      dispose();
    });
  });

  it('(b) returns [] when the tree is empty (no nodeByPath entries)', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      // No setTree call → nodeByPath is empty
      store.setVisibility('Some.path', 'hidden', false);

      expect((store as any).getStalePaths()).toEqual([]);

      dispose();
    });
  });

  it('(c) does not include paths that are currently in the tree', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();

      store.setTree(treeWith('Root.present'));
      store.setVisibility('Root.present', 'hidden', false);

      const stale = (store as any).getStalePaths() as string[];
      expect(stale).not.toContain('Root.present');

      dispose();
    });
  });

  it('(d) stale path disappears from results when the tree re-includes it', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();

      store.setTree(treeWith('Root.movable'));
      store.setVisibility('Root.movable', 'ghost', false);

      // Path leaves tree → becomes stale
      store.setTree(treeWith());
      expect((store as any).getStalePaths()).toContain('Root.movable');

      // Path returns → no longer stale
      store.setTree(treeWith('Root.movable'));
      expect((store as any).getStalePaths()).not.toContain('Root.movable');

      dispose();
    });
  });
});

// applyPersistedState / serializePersistedState — step-21 tests (fail until step-22 adds the methods)
describe('viewStateStore — applyPersistedState / serializePersistedState (step-21)', () => {
  /** Helper: a minimal valid persisted state with one user view. */
  function makePersistedState(
    overrides: Partial<Omit<PersistentViewState, 'viewportCameras' | 'timestamp'>> = {},
  ): Omit<PersistentViewState, 'viewportCameras' | 'timestamp'> {
    const userView: ViewDefinition = {
      id: 'user:saved-abc',
      name: 'My Saved View',
      auto: false,
      modified: false,
      visibility: { 'Root.geo': 'show', 'Root.strut': 'hidden' },
    };
    return {
      version: '2',
      activeViewId: 'user:saved-abc',
      userViews: [userView],
      explicit: { 'Root.geo': 'show', 'Root.strut': 'hidden' },
      ...overrides,
    };
  }

  it('(a) apply seeds userViews into state.views without clobbering auto views', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      // Prime auto views first
      store.regenerateAutoViews([makeNode({ entity_path: 'Root', kind: 'structure' })]);
      expect(store.state.views['auto:default']).toBeTruthy();

      const persisted = makePersistedState();
      (store as any).applyPersistedState(persisted);

      // User view was seeded
      expect(store.state.views['user:saved-abc']).toBeTruthy();
      expect(store.state.views['user:saved-abc'].name).toBe('My Saved View');
      // Auto view is still present
      expect(store.state.views['auto:default']).toBeTruthy();

      dispose();
    });
  });

  it('(b) apply sets activeViewId and restores explicit map from the persisted user view', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();

      const persisted = makePersistedState();
      (store as any).applyPersistedState(persisted);

      expect(store.state.activeViewId).toBe('user:saved-abc');
      expect(store.state.explicit['Root.geo']).toBe('show');
      expect(store.state.explicit['Root.strut']).toBe('hidden');

      dispose();
    });
  });

  it('(c) apply ignores persisted entries whose id starts with "auto:" (auto views regenerated separately)', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();

      const autoView: ViewDefinition = {
        id: 'auto:default',
        name: 'Default',
        auto: true,
        modified: false,
        visibility: {},
      };
      const persisted = makePersistedState({
        userViews: [
          // Mix of auto: (should be ignored) and user: (should be seeded)
          autoView,
          {
            id: 'user:legitimate',
            name: 'Legit',
            auto: false,
            modified: false,
            visibility: {},
          },
        ],
        activeViewId: 'user:legitimate',
        explicit: {},
      });

      (store as any).applyPersistedState(persisted);

      // auto:default from the persisted list must NOT overwrite real auto views
      // (the store started with no auto views here; if we did seed it would have
      //  auto: false overrides which is wrong — we simply must not seed it at all)
      expect(store.state.views['user:legitimate']).toBeTruthy();
      // The store should NOT have seeded the persisted autoView as a view at all
      // (it may or may not exist from regenerateAutoViews, but the persisted one
      //  should be dropped — we check name to distinguish)
      const maybeAuto = store.state.views['auto:default'];
      if (maybeAuto) {
        // It was there before (shouldn't be here in this test), or was seeded legitimately —
        // the key point is the persisted auto view (with auto:true, name="Default") was not
        // applied incorrectly. Since we didn't call regenerateAutoViews, any auto:default
        // present must have come from the persisted list — which is the bug. Verify it didn't.
        expect(maybeAuto).toBeUndefined();
      }

      dispose();
    });
  });

  it('(d) serialize returns only user views (filters auto), activeViewId, explicit snapshot, and version "2"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();

      // Prime with auto views + a user view
      store.regenerateAutoViews([makeNode({ entity_path: 'Root', kind: 'structure' })]);
      const userId = store.createView('Manual');
      store.switchView(userId);
      store.setVisibility('Root', 'ghost', false);

      const serialized: Omit<PersistentViewState, 'viewportCameras' | 'timestamp'> =
        (store as any).serializePersistedState();

      expect(serialized.version).toBe('2');
      expect(serialized.activeViewId).toBe(userId);

      // Only user views in userViews — no auto:*
      expect(serialized.userViews.every((v: ViewDefinition) => !v.id.startsWith('auto:'))).toBe(true);
      expect(serialized.userViews.some((v: ViewDefinition) => v.id === userId)).toBe(true);

      // explicit snapshot captured
      expect(serialized.explicit['Root']).toBe('ghost');

      // No viewportCameras or timestamp (those are composed at App.tsx layer)
      expect('viewportCameras' in serialized).toBe(false);
      expect('timestamp' in serialized).toBe(false);

      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// T6 acceptance: default-hidden aux + outline toggle (step-7)
// ---------------------------------------------------------------------------

describe('viewStateStore — T6 default-hidden aux realization + toggle', () => {
  it('getAllEffective: product realization visible, aux realization hidden by default; toggle reveals it', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();

      // Tree: Asm { part sub (product realization) + jig sub (aux realization) }
      const productPath = 'Asm.part#realization[0]';
      const auxPath = 'Asm.jig#realization[0]';
      const tree = [
        makeNode({
          entity_path: 'Asm',
          kind: 'structure',
          children: [
            makeNode({ entity_path: 'Asm.part', kind: 'sub',
              children: [
                makeNode({ entity_path: productPath, kind: 'realization', default_visible: true }),
              ],
            }),
            makeNode({ entity_path: 'Asm.jig', kind: 'sub',
              children: [
                makeNode({ entity_path: auxPath, kind: 'realization', default_visible: false }),
              ],
            }),
          ],
        }),
      ];

      store.regenerateAutoViews(tree);

      // Product child is visible; aux child is hidden by default.
      const allEffective = store.getAllEffective();
      expect(allEffective[productPath]).toBe('show');
      expect(allEffective[auxPath]).toBe('hidden');

      // Simulate outline toggle: user explicitly shows the aux entity.
      store.setVisibility(auxPath, 'show');
      // No rebuild — explicit override reveals it immediately.
      expect(store.getEffectiveVisibility(auxPath)).toBe('show');

      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// resetToDefaultView — step-1 tests (fail until step-2 adds the method)
// ---------------------------------------------------------------------------

describe('viewStateStore — resetToDefaultView (step-1)', () => {
  /**
   * Build a minimal tree that reproduces the stale-visibility bug scenario:
   *   CapstanDrive (structure)
   *     └─ CapstanDrive.capstan (structure)
   *          └─ CapstanDrive.capstan#realization[0]  (realization, trait_geometry=true)
   */
  function makeRealizationTree() {
    return [
      makeNode({
        entity_path: 'CapstanDrive',
        kind: 'structure',
        children: [
          makeNode({
            entity_path: 'CapstanDrive.capstan',
            kind: 'structure',
            children: [
              makeNode({
                entity_path: 'CapstanDrive.capstan#realization[0]',
                kind: 'realization',
                trait_geometry: true,
              }),
            ],
          }),
        ],
      }),
    ];
  }

  const REALIZATION_PATH = 'CapstanDrive.capstan#realization[0]';

  it('(pre-condition) after setVisibility("hidden") the mesh is hidden and activeViewId is a user:* view', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeRealizationTree());

      // Confirm baseline: mesh is visible on a clean store.
      expect(store.getEffectiveVisibility(REALIZATION_PATH)).toBe('show');
      expect(store.state.activeViewId).toBe('auto:default');

      // COW: setVisibility hides the mesh, auto:default gets COW'd into a user view.
      store.setVisibility(REALIZATION_PATH, 'hidden');

      expect(store.getEffectiveVisibility(REALIZATION_PATH)).toBe('hidden');
      expect(store.state.activeViewId).toMatch(/^user:/);

      dispose();
    });
  });

  it('(a) resetToDefaultView sets activeViewId back to "auto:default"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeRealizationTree());
      store.setVisibility(REALIZATION_PATH, 'hidden'); // drives into user:* view

      (store as any).resetToDefaultView();

      expect(store.state.activeViewId).toBe('auto:default');
      dispose();
    });
  });

  it('(b) resetToDefaultView clears all explicit overrides', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeRealizationTree());
      store.setVisibility(REALIZATION_PATH, 'hidden');
      expect(Object.keys(store.state.explicit).length).toBeGreaterThan(0);

      (store as any).resetToDefaultView();

      expect(Object.keys(store.state.explicit).length).toBe(0);
      dispose();
    });
  });

  it('(c) resetToDefaultView makes the previously-hidden live mesh visible again via defaultVisibilityFor walk-up', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeRealizationTree());
      store.setVisibility(REALIZATION_PATH, 'hidden');
      expect(store.getEffectiveVisibility(REALIZATION_PATH)).toBe('hidden');

      (store as any).resetToDefaultView();

      expect(store.getEffectiveVisibility(REALIZATION_PATH)).toBe('show');
      dispose();
    });
  });

  it('(d) resetToDefaultView preserves user-view definitions in state.views and getOrderedViewIds', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.regenerateAutoViews(makeRealizationTree());
      store.setVisibility(REALIZATION_PATH, 'hidden'); // creates a user:* view definition

      // Capture the user view id that was created by COW.
      const userViewId = store.state.activeViewId;
      expect(userViewId).toMatch(/^user:/);
      expect(store.state.views[userViewId]).toBeDefined();

      (store as any).resetToDefaultView();

      // User-view definition must still exist after reset.
      expect(store.state.views[userViewId]).toBeDefined();
      expect(store.getOrderedViewIds()).toContain(userViewId);
      dispose();
    });
  });
});
