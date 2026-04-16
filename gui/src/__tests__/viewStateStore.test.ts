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
