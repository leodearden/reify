import { describe, it, expect } from 'vitest';
import { createRoot } from 'solid-js';
import { createViewStateStore } from '../stores/viewStateStore';
import type { EntityTreeNode } from '../types';

// ---------------------------------------------------------------------------
// Local fixture builder
// ---------------------------------------------------------------------------

function makeNode(overrides: Partial<EntityTreeNode> & { entity_path: string }): EntityTreeNode {
  return {
    kind: 'structure',
    type_name: null,
    has_mesh: false,
    trait_geometry: false,
    children: [],
    ...overrides,
  };
}

describe('viewStateStore — default rules', () => {
  it('node with trait_geometry=true → getEffectiveVisibility returns "show"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree([makeNode({ entity_path: 'Root', trait_geometry: true })]);
      expect(store.getEffectiveVisibility('Root')).toBe('show');
      dispose();
    });
  });

  it('node with kind="let" and type_name containing "Solid" → "hidden"', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree([makeNode({ entity_path: 'Root.geo', kind: 'let', type_name: 'MySolid' })]);
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
      expect(store.state.explicit['Root.A.a1']).toBeNull();
      expect(store.state.explicit['Root.A.a1.a1x']).toBeNull();
      expect(store.state.explicit['Root.A.a2']).toBeNull();
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
      expect(store.state.explicit['Root.A.a1']).toBeNull();
      expect(store.getEffectiveVisibility('Root.A.a1')).toBe('ghost');
      dispose();
    });
  });
});

describe('viewStateStore — setVisibilityWithoutCascade and walk-up', () => {
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
        ],
      }),
    ];
  }

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
              makeNode({ entity_path: 'Root.A.a2', kind: 'param' }),
            ],
          }),
          makeNode({ entity_path: 'Root.B', kind: 'structure' }),
        ],
      }),
    ];
  }

  it('clears explicit[path] to null', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      store.setVisibility('Root.A', 'hidden', false);
      store.resetToInherit('Root.A');
      expect(store.state.explicit['Root.A']).toBeNull();
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
      expect(store.state.explicit['Root.A']).toBeNull();
      expect(store.state.explicit['Root.A.a1']).toBeNull();
      expect(store.state.explicit['Root.A.a2']).toBeNull();
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
});

describe('viewStateStore — showOnly', () => {
  function makeTree() {
    // Root { A { a1, a2 }, B { b1, b2 } }
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
              makeNode({ entity_path: 'Root.A.a2', kind: 'param' }),
            ],
          }),
          makeNode({
            entity_path: 'Root.B',
            kind: 'structure',
            children: [
              makeNode({ entity_path: 'Root.B.b1', kind: 'param' }),
              makeNode({ entity_path: 'Root.B.b2', kind: 'param' }),
            ],
          }),
        ],
      }),
    ];
  }

  it('showOnly(cascade=true): target has explicit show, all nodes not in {target, ancestors} are hidden', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      store.showOnly('Root.A.a1', true);
      // Target
      expect(store.state.explicit['Root.A.a1']).toBe('show');
      // Ancestors: Root and Root.A should be null (not hidden)
      expect(store.state.explicit['Root']).toBeNull();
      expect(store.state.explicit['Root.A']).toBeNull();
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
      store.setTree(makeTree());
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
      expect(store.state.explicit['Root.A.a1.x']).toBeNull();
      expect(store.getEffectiveVisibility('Root.A.a1.x')).toBe('show');
      dispose();
    });
  });

  it('showOnly(cascade=false): descendants of target are hidden (not cleared to null)', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      store.showOnly('Root.A', false);
      // cascade=false: a1 and a2 are set hidden by the universal-hide pass (not null)
      expect(store.state.explicit['Root.A.a1']).toBe('hidden');
      expect(store.state.explicit['Root.A.a2']).toBe('hidden');
      // B hidden too
      expect(store.state.explicit['Root.B']).toBe('hidden');
      // target is show
      expect(store.state.explicit['Root.A']).toBe('show');
      // ancestor is null
      expect(store.state.explicit['Root']).toBeNull();
      dispose();
    });
  });

  it('ancestors of target have explicit=null so they do not block target', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
      // Pre-set an ancestor to hidden
      store.setVisibility('Root.A', 'hidden', false);
      store.showOnly('Root.A.a1', true);
      // After showOnly, ancestor Root.A must be null (not hidden) so a1 can show
      expect(store.state.explicit['Root.A']).toBeNull();
      expect(store.getEffectiveVisibility('Root.A.a1')).toBe('show');
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
