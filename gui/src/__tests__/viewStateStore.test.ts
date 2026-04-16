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

describe('viewStateStore — getAllEffective', () => {
  function makeTree() {
    return [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({
            entity_path: 'Root.A',
            kind: 'structure',
            trait_geometry: true,
            children: [
              makeNode({ entity_path: 'Root.A.a1', kind: 'param' }),
            ],
          }),
          makeNode({ entity_path: 'Root.B', kind: 'structure' }),
        ],
      }),
    ];
  }

  it('returns Record covering every node with their resolved effective state', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());
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
      store.setTree(makeTree());
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
      store.setTree(makeTree());
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
      expect(store.state.explicit['Root.A.a1']).toBeNull();
      // a1 inherits ghost from Root.A
      expect(store.getEffectiveVisibility('Root.A.a1')).toBe('ghost');
      dispose();
    });
  });
});

describe('viewStateStore — hasOverride', () => {
  function makeTree() {
    // Root(show-by-default) > A(explicit='hidden') > a1(explicit=null)
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
  // Tree: Root { A { a1, a2 }, B { b1, b2 } }
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

  it('simulates the full 4-step PRD scenario', () => {
    createRoot((dispose) => {
      const store = createViewStateStore();
      store.setTree(makeTree());

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
      expect(store.state.explicit['Root.A.a1']).toBeNull();
      expect(store.getEffectiveVisibility('Root.A.a1')).toBe('hidden');
      expect(store.getEffectiveVisibility('Root.B.b2')).toBe('hidden');

      // Step 4: resetToInherit('Root')
      // → root cleared; everything reverts to default rule ('show' for param nodes)
      store.resetToInherit('Root');
      expect(store.state.explicit['Root']).toBeNull();
      expect(store.state.explicit['Root.A']).toBeNull();
      expect(store.state.explicit['Root.A.a1']).toBeNull();
      expect(store.getEffectiveVisibility('Root')).toBe('show');
      expect(store.getEffectiveVisibility('Root.A.a1')).toBe('show');
      expect(store.getEffectiveVisibility('Root.B.b2')).toBe('show');

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
