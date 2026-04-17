import { describe, it, expect } from 'vitest';
import {
  generateDefaultView,
  generateAllGeometryView,
  generatePurposeViews,
} from '../stores/autoViewGenerator';
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

// ---------------------------------------------------------------------------
// generateDefaultView
// ---------------------------------------------------------------------------

describe('generateDefaultView', () => {
  it('(a) single trait_geometry node → visibility "show"', () => {
    const tree = [makeNode({ entity_path: 'Root', trait_geometry: true })];
    const view = generateDefaultView(tree);
    expect(view.visibility['Root']).toBe('show');
  });

  it('(b) let-binding with type_name containing "Solid" → "hidden"', () => {
    const tree = [makeNode({ entity_path: 'Root.geo', kind: 'let', type_name: 'MySolid' })];
    const view = generateDefaultView(tree);
    expect(view.visibility['Root.geo']).toBe('hidden');
  });

  it('(b) let-binding with type_name containing "Surface" → "hidden"', () => {
    const tree = [makeNode({ entity_path: 'Root.surf', kind: 'let', type_name: 'MySurface' })];
    const view = generateDefaultView(tree);
    expect(view.visibility['Root.surf']).toBe('hidden');
  });

  it('(b) let-binding with type_name containing "Curve" → "hidden"', () => {
    const tree = [makeNode({ entity_path: 'Root.crv', kind: 'let', type_name: 'MyCurve' })];
    const view = generateDefaultView(tree);
    expect(view.visibility['Root.crv']).toBe('hidden');
  });

  it('(c) structure container node → "show"', () => {
    const tree = [makeNode({ entity_path: 'Root', kind: 'structure' })];
    const view = generateDefaultView(tree);
    expect(view.visibility['Root']).toBe('show');
  });

  it('(c) sub container node → "show"', () => {
    const tree = [makeNode({ entity_path: 'Root.sub', kind: 'sub' })];
    const view = generateDefaultView(tree);
    expect(view.visibility['Root.sub']).toBe('show');
  });

  it('(d) nested tree walk — Assembly > housing{geometry, bore_cutout} > flange{geometry, body, hole}', () => {
    const tree = [
      makeNode({
        entity_path: 'Assembly',
        kind: 'structure',
        children: [
          makeNode({
            entity_path: 'Assembly.housing',
            kind: 'structure',
            children: [
              makeNode({ entity_path: 'Assembly.housing.geometry', kind: 'param', trait_geometry: true }),
              makeNode({ entity_path: 'Assembly.housing.bore_cutout', kind: 'let', type_name: 'SolidCutout' }),
            ],
          }),
          makeNode({
            entity_path: 'Assembly.flange',
            kind: 'structure',
            children: [
              makeNode({ entity_path: 'Assembly.flange.geometry', kind: 'param', trait_geometry: true }),
              makeNode({ entity_path: 'Assembly.flange.body', kind: 'let', type_name: 'SolidBody' }),
              makeNode({ entity_path: 'Assembly.flange.hole', kind: 'let', type_name: 'SolidHole' }),
            ],
          }),
        ],
      }),
    ];
    const view = generateDefaultView(tree);

    // Every node is covered
    expect(view.visibility['Assembly']).toBe('show');
    expect(view.visibility['Assembly.housing']).toBe('show');
    expect(view.visibility['Assembly.housing.geometry']).toBe('show');
    expect(view.visibility['Assembly.housing.bore_cutout']).toBe('hidden');
    expect(view.visibility['Assembly.flange']).toBe('show');
    expect(view.visibility['Assembly.flange.geometry']).toBe('show');
    expect(view.visibility['Assembly.flange.body']).toBe('hidden');
    expect(view.visibility['Assembly.flange.hole']).toBe('hidden');
    expect(Object.keys(view.visibility)).toHaveLength(8);
  });

  it('(e) returns ViewDefinition with correct metadata', () => {
    const tree = [makeNode({ entity_path: 'Root' })];
    const view = generateDefaultView(tree);
    expect(view.id).toBe('auto:default');
    expect(view.name).toBe('Default');
    expect(view.auto).toBe(true);
    expect(view.modified).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// generateAllGeometryView
// ---------------------------------------------------------------------------

describe('generateAllGeometryView', () => {
  it('(a) single-node tree → visibility "show"', () => {
    const tree = [makeNode({ entity_path: 'Root' })];
    const view = generateAllGeometryView(tree);
    expect(view.visibility['Root']).toBe('show');
  });

  it('(b) nested tree — every node marked "show" regardless of trait_geometry / kind / type_name', () => {
    const tree = [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.geo', kind: 'let', type_name: 'MySolid' }),
          makeNode({ entity_path: 'Root.param', kind: 'param', trait_geometry: false }),
          makeNode({ entity_path: 'Root.mesh', kind: 'param', trait_geometry: true }),
        ],
      }),
    ];
    const view = generateAllGeometryView(tree);
    expect(view.visibility['Root']).toBe('show');
    expect(view.visibility['Root.geo']).toBe('show');
    expect(view.visibility['Root.param']).toBe('show');
    expect(view.visibility['Root.mesh']).toBe('show');
  });

  it('(c) returns ViewDefinition with id="auto:all-geometry", name="All geometry", auto=true, modified=false', () => {
    const tree = [makeNode({ entity_path: 'Root' })];
    const view = generateAllGeometryView(tree);
    expect(view.id).toBe('auto:all-geometry');
    expect(view.name).toBe('All geometry');
    expect(view.auto).toBe(true);
    expect(view.modified).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// generatePurposeViews
// ---------------------------------------------------------------------------

describe('generatePurposeViews', () => {
  it('(a) empty activePurposes → returns []', () => {
    const tree = [makeNode({ entity_path: 'Root' })];
    const views = generatePurposeViews(tree, []);
    expect(views).toEqual([]);
  });

  it('(b) one arbitrary purpose "foo" → returns single ViewDefinition with id="auto:purpose:foo", name="foo", auto=true, falling back to Default rules', () => {
    const tree = [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.geo', kind: 'let', type_name: 'MySolid' }),
          makeNode({ entity_path: 'Root.mesh', kind: 'param', trait_geometry: true }),
        ],
      }),
    ];
    const views = generatePurposeViews(tree, ['foo']);
    expect(views).toHaveLength(1);
    const view = views[0];
    expect(view.id).toBe('auto:purpose:foo');
    expect(view.name).toBe('foo');
    expect(view.auto).toBe(true);
    expect(view.modified).toBe(false);
    // Falls back to Default rules
    expect(view.visibility['Root.geo']).toBe('hidden');
    expect(view.visibility['Root.mesh']).toBe('show');
  });

  it('(c) "manufacturing_ready" heuristic: let Solid/Surface/Curve → "ghost", trait_geometry → "show", containers → "show", Material params → "show", non-material params → "ghost"', () => {
    const tree = [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.body', kind: 'let', type_name: 'SolidBody' }),
          makeNode({ entity_path: 'Root.skin', kind: 'let', type_name: 'Surface' }),
          makeNode({ entity_path: 'Root.edge', kind: 'let', type_name: 'CurveEdge' }),
          makeNode({ entity_path: 'Root.geometry', kind: 'param', trait_geometry: true }),
          makeNode({ entity_path: 'Root.mat', kind: 'param', type_name: 'Material', trait_geometry: false }),
          makeNode({ entity_path: 'Root.width', kind: 'param', type_name: null, trait_geometry: false }),
          makeNode({ entity_path: 'Root.housing', kind: 'structure' }),
        ],
      }),
    ];
    const views = generatePurposeViews(tree, ['manufacturing_ready']);
    expect(views).toHaveLength(1);
    const view = views[0];
    expect(view.id).toBe('auto:purpose:manufacturing_ready');
    // let Solid/Surface/Curve → ghost (still visible as context)
    expect(view.visibility['Root.body']).toBe('ghost');
    expect(view.visibility['Root.skin']).toBe('ghost');
    expect(view.visibility['Root.edge']).toBe('ghost');
    // trait_geometry → show
    expect(view.visibility['Root.geometry']).toBe('show');
    // containers → show
    expect(view.visibility['Root']).toBe('show');
    expect(view.visibility['Root.housing']).toBe('show');
    // Material params → show (distinct from the non-material param below)
    expect(view.visibility['Root.mat']).toBe('show');
    // Non-material, non-geometry param → ghost (proves the Material branch fires
    // independently of the final fallback)
    expect(view.visibility['Root.width']).toBe('ghost');
  });

  it('(d) multiple purposes produce multiple views in order', () => {
    const tree = [makeNode({ entity_path: 'Root' })];
    const views = generatePurposeViews(tree, ['alpha', 'beta', 'gamma']);
    expect(views).toHaveLength(3);
    expect(views[0].id).toBe('auto:purpose:alpha');
    expect(views[1].id).toBe('auto:purpose:beta');
    expect(views[2].id).toBe('auto:purpose:gamma');
  });

  it('(e) let node with type_name=null → "show" under both generateDefaultView and fallback purpose view (null guard regression)', () => {
    // Structurally-typed let bindings have no type_name — they must not be hidden.
    const tree = [makeNode({ entity_path: 'Root.untyped', kind: 'let', type_name: null })];
    // Default view
    expect(generateDefaultView(tree).visibility['Root.untyped']).toBe('show');
    // Generic purpose (falls back to defaultVisibilityFor)
    const [purposeView] = generatePurposeViews(tree, ['foo']);
    expect(purposeView.visibility['Root.untyped']).toBe('show');
  });
});
