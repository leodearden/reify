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
