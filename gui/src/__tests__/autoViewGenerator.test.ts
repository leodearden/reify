import { describe, it, expect, expectTypeOf, vi } from 'vitest';
import {
  generateDefaultView,
  generateAllGeometryView,
  generatePurposeViews,
  defaultVisibilityFor,
} from '../stores/autoViewGenerator';
import type { ViewDefinition } from '../stores/autoViewGenerator';
import type { DisplayDirective, EntityTreeNode, MeshData } from '../types';

// `computePaneGroups` lives in App.tsx, whose module graph reaches the Tauri
// bridge and (via `./viewport`) three.js at import time. Mocked as App.test.tsx
// does so this store-level suite can compose the two halves of the #5195
// routing path (visibility map × pane bucketing) without rendering App.
// The viewport stub covers exactly App.tsx's imports from './viewport'
// (`DualViewport`, `MultiViewport`); without it three's lottie_canvas module
// calls `HTMLCanvasElement.getContext` at import and jsdom throws.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue({ meshes: [], values: [], constraints: [], files: [] }),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('../viewport', () => ({
  Viewport: () => document.createElement('div'),
  DualViewport: () => document.createElement('div'),
  MultiViewport: () => document.createElement('div'),
}));
// eslint-disable-next-line import/first
import { computePaneGroups } from '../App';

// ---------------------------------------------------------------------------
// Local fixture builder
// ---------------------------------------------------------------------------

function makeNode(overrides: Partial<EntityTreeNode> & { entity_path: string }): EntityTreeNode {
  return {
    kind: 'structure',
    type_name: null,
    has_mesh: false,
    trait_geometry: false,
    freshness: 'final',
    children: [],
    ...overrides,
  };
}

/** Minimal `MeshData`; mirrors App.test.tsx's `makeMesh`. */
function makeMesh(entityPath: string): MeshData {
  return {
    entity_path: entityPath,
    vertices: new Float32Array([0, 0, 0]),
    indices: new Uint32Array([0]),
    normals: null,
  };
}

// ---------------------------------------------------------------------------
// ViewDefinition shape contract (compile-time, single authoritative check)
// ---------------------------------------------------------------------------

describe('ViewDefinition shape contract', () => {
  it('keyset is pinned to {auto, id, modified, name, visibility}', () => {
    expectTypeOf<keyof ViewDefinition>().toEqualTypeOf<'id' | 'name' | 'auto' | 'visibility' | 'modified'>();
  });

  it('generateDefaultView returns a view with modified === undefined for pristine auto views', () => {
    const tree = [makeNode({ entity_path: 'Root' })];
    const view = generateDefaultView(tree);
    expect(view.modified).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// generateDefaultView
// ---------------------------------------------------------------------------

describe('generateDefaultView', () => {
  it('(a) single trait_geometry node → visibility "show"', () => {
    const tree = [makeNode({ entity_path: 'Root', trait_geometry: true })];
    const view = generateDefaultView(tree);
    expect(view.visibility['Root']).toBe('show');
  });

  it('(b) let-binding with type_name "Solid" → "hidden"', () => {
    const tree = [makeNode({ entity_path: 'Root.geo', kind: 'let', type_name: 'Solid' })];
    const view = generateDefaultView(tree);
    expect(view.visibility['Root.geo']).toBe('hidden');
  });

  it('(b) let-binding with type_name "Surface" → "hidden"', () => {
    const tree = [makeNode({ entity_path: 'Root.surf', kind: 'let', type_name: 'Surface' })];
    const view = generateDefaultView(tree);
    expect(view.visibility['Root.surf']).toBe('hidden');
  });

  it('(b) let-binding with type_name "Curve" → "hidden"', () => {
    const tree = [makeNode({ entity_path: 'Root.crv', kind: 'let', type_name: 'Curve' })];
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
              makeNode({ entity_path: 'Assembly.housing.bore_cutout', kind: 'let', type_name: 'Solid' }),
            ],
          }),
          makeNode({
            entity_path: 'Assembly.flange',
            kind: 'structure',
            children: [
              makeNode({ entity_path: 'Assembly.flange.geometry', kind: 'param', trait_geometry: true }),
              makeNode({ entity_path: 'Assembly.flange.body', kind: 'let', type_name: 'Solid' }),
              makeNode({ entity_path: 'Assembly.flange.hole', kind: 'let', type_name: 'Option<Solid>' }),
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
          makeNode({ entity_path: 'Root.geo', kind: 'let', type_name: 'Solid' }),
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

  it('(c) returns ViewDefinition with id="auto:all-geometry", name="All geometry", auto=true', () => {
    const tree = [makeNode({ entity_path: 'Root' })];
    const view = generateAllGeometryView(tree);
    expect(view.id).toBe('auto:all-geometry');
    expect(view.name).toBe('All geometry');
    expect(view.auto).toBe(true);
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
          makeNode({ entity_path: 'Root.geo', kind: 'let', type_name: 'Solid' }),
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
          makeNode({ entity_path: 'Root.body', kind: 'let', type_name: 'Solid' }),
          makeNode({ entity_path: 'Root.skin', kind: 'let', type_name: 'Surface' }),
          makeNode({ entity_path: 'Root.edge', kind: 'let', type_name: 'Curve' }),
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

// ---------------------------------------------------------------------------
// anchored type-name matching (regression for substring-match bug)
// ---------------------------------------------------------------------------

describe('anchored type-name matching (regression for substring-match bug)', () => {
  // Regression guards for the old substring-match bug: these must stay green to
  // ensure we don't regress to `.includes()`.

  it('defaultVisibilityFor: let with type_name="Solidarity" → "show" (not a geometry type)', () => {
    const node = makeNode({ entity_path: 'Root.x', kind: 'let', type_name: 'Solidarity' });
    expect(defaultVisibilityFor(node)).toBe('show');
  });

  it('defaultVisibilityFor: let with type_name="SurfaceTreatment" → "show"', () => {
    const node = makeNode({ entity_path: 'Root.x', kind: 'let', type_name: 'SurfaceTreatment' });
    expect(defaultVisibilityFor(node)).toBe('show');
  });

  it('defaultVisibilityFor: let with type_name="CurveBall" → "show"', () => {
    const node = makeNode({ entity_path: 'Root.x', kind: 'let', type_name: 'CurveBall' });
    expect(defaultVisibilityFor(node)).toBe('show');
  });

  it('generateDefaultView: MySolid and SolidBody let-nodes should both be "show" (not hidden)', () => {
    const tree = [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.a', kind: 'let', type_name: 'MySolid' }),
          makeNode({ entity_path: 'Root.b', kind: 'let', type_name: 'SolidBody' }),
        ],
      }),
    ];
    const view = generateDefaultView(tree);
    expect(view.visibility['Root.a']).toBe('show');
    expect(view.visibility['Root.b']).toBe('show');
  });

  it('generatePurposeViews manufacturing_ready: param with type_name="MaterialReference" → "ghost" (param fallthrough, not Material branch)', () => {
    const tree = [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.mat', kind: 'param', type_name: 'MaterialReference', trait_geometry: false }),
        ],
      }),
    ];
    const [view] = generatePurposeViews(tree, ['manufacturing_ready']);
    expect(view.visibility['Root.mat']).toBe('ghost');
  });

  it('generatePurposeViews manufacturing_ready: let with type_name="SolidBody" → "show" (no longer classified as let-geometry)', () => {
    const tree = [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.body', kind: 'let', type_name: 'SolidBody' }),
        ],
      }),
    ];
    const [view] = generatePurposeViews(tree, ['manufacturing_ready']);
    expect(view.visibility['Root.body']).toBe('show');
  });

  // --- Positive regression guards (PASS under both old and new code) ---

  it('defaultVisibilityFor: let with type_name="Solid" → "hidden" (exact match)', () => {
    const node = makeNode({ entity_path: 'Root.x', kind: 'let', type_name: 'Solid' });
    expect(defaultVisibilityFor(node)).toBe('hidden');
  });

  it('defaultVisibilityFor: let with type_name="Option<Solid>" → "hidden" (wrapper tolerance)', () => {
    const node = makeNode({ entity_path: 'Root.x', kind: 'let', type_name: 'Option<Solid>' });
    expect(defaultVisibilityFor(node)).toBe('hidden');
  });

  it('defaultVisibilityFor: let with type_name="List<Curve>" → "hidden" (wrapper tolerance)', () => {
    const node = makeNode({ entity_path: 'Root.x', kind: 'let', type_name: 'List<Curve>' });
    expect(defaultVisibilityFor(node)).toBe('hidden');
  });

  it('generatePurposeViews manufacturing_ready: param with type_name="List<Material>" → "show" (Material wrapper tolerance)', () => {
    const tree = [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.mat', kind: 'param', type_name: 'List<Material>', trait_geometry: false }),
        ],
      }),
    ];
    const [view] = generatePurposeViews(tree, ['manufacturing_ready']);
    expect(view.visibility['Root.mat']).toBe('show');
  });

  it('generatePurposeViews manufacturing_ready: let with type_name="Option<Surface>" → "ghost" (let-geometry wrapper still detected)', () => {
    const tree = [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.surf', kind: 'let', type_name: 'Option<Surface>' }),
        ],
      }),
    ];
    const [view] = generatePurposeViews(tree, ['manufacturing_ready']);
    expect(view.visibility['Root.surf']).toBe('ghost');
  });
});

// ---------------------------------------------------------------------------
// #5195 step-5: "Geometry" is the spelling the backend actually emits
// ---------------------------------------------------------------------------

/**
 * The `Solid` source spelling and the `Geometry` source spelling BOTH resolve
 * to the backend's `Type::Geometry`, whose `Display` emits the literal string
 * `"Geometry"` — never `"Solid"`. So the geometry-type rule could not fire on
 * any real value-cell node until `Geometry` joined the pattern.
 */
describe('defaultVisibilityFor — "Geometry" type_name (#5195)', () => {
  it('let with type_name="Geometry" → "hidden" (the spelling Type::Geometry actually Displays)', () => {
    const node = makeNode({ entity_path: 'Root.body', kind: 'let', type_name: 'Geometry' });
    expect(defaultVisibilityFor(node)).toBe('hidden');
  });

  it('let with type_name="Option<Geometry>" → "hidden" (wrapper tolerance)', () => {
    const node = makeNode({ entity_path: 'Root.body', kind: 'let', type_name: 'Option<Geometry>' });
    expect(defaultVisibilityFor(node)).toBe('hidden');
  });

  it('let with type_name="GeometryReference" → "show" (word boundary rejects substring)', () => {
    const node = makeNode({ entity_path: 'Root.ref', kind: 'let', type_name: 'GeometryReference' });
    expect(defaultVisibilityFor(node)).toBe('show');
  });

  it('generateDefaultView hides a "Geometry"-typed let while leaving a "GeometryReference" let shown', () => {
    const tree = [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.body', kind: 'let', type_name: 'Geometry' }),
          makeNode({ entity_path: 'Root.ref', kind: 'let', type_name: 'GeometryReference' }),
        ],
      }),
    ];
    const view = generateDefaultView(tree);
    expect(view.visibility['Root.body']).toBe('hidden');
    expect(view.visibility['Root.ref']).toBe('show');
  });

  it('generatePurposeViews manufacturing_ready: let with type_name="Geometry" → "ghost" (shared helper stays in sync)', () => {
    const tree = [
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Root.body', kind: 'let', type_name: 'Geometry' }),
        ],
      }),
    ];
    const [view] = generatePurposeViews(tree, ['manufacturing_ready']);
    expect(view.visibility['Root.body']).toBe('ghost');
  });
});

// ---------------------------------------------------------------------------
// default_visible: aux hidden-by-default (step-5 T6)
// ---------------------------------------------------------------------------

describe('defaultVisibilityFor — default_visible field (T6 aux hidden-by-default)', () => {
  it('realization node with default_visible:false → "hidden" (aux body hidden by default)', () => {
    const node = makeNode({ entity_path: 'Struct#realization[0]', kind: 'realization', default_visible: false });
    expect(defaultVisibilityFor(node)).toBe('hidden');
  });

  it('realization node with default_visible:true → "show" (product body visible)', () => {
    const node = makeNode({ entity_path: 'Struct#realization[0]', kind: 'realization', default_visible: true });
    expect(defaultVisibilityFor(node)).toBe('show');
  });

  it('realization node with default_visible omitted → "show" (backward-compat: absent treated as visible)', () => {
    const node = makeNode({ entity_path: 'Struct#realization[0]', kind: 'realization' });
    expect(defaultVisibilityFor(node)).toBe('show');
  });

  it('let node with type_name "Solid" and default_visible omitted → still "hidden" (existing rule unaffected)', () => {
    const node = makeNode({ entity_path: 'Root.geo', kind: 'let', type_name: 'Solid' });
    expect(defaultVisibilityFor(node)).toBe('hidden');
  });

  it('trait_geometry node with default_visible:false → "hidden" (aux overrides trait_geometry — aux rule is first)', () => {
    const node = makeNode({ entity_path: 'Struct#realization[0]', kind: 'realization', trait_geometry: true, default_visible: false });
    expect(defaultVisibilityFor(node)).toBe('hidden');
  });

  it('generateDefaultView honors default_visible:false on realization node — aux body listed hidden', () => {
    const tree = [
      makeNode({
        entity_path: 'Asm',
        kind: 'structure',
        children: [
          makeNode({ entity_path: 'Asm#realization[0]', kind: 'realization', default_visible: true }),
          makeNode({ entity_path: 'Asm#realization[1]', kind: 'realization', default_visible: false }),
        ],
      }),
    ];
    const view = generateDefaultView(tree);
    expect(view.visibility['Asm#realization[0]']).toBe('show');
    expect(view.visibility['Asm#realization[1]']).toBe('hidden');
  });
});

// ---------------------------------------------------------------------------
// #5195 step-7: DisplayOutput explicit routing
// ---------------------------------------------------------------------------

/**
 * When a design declares one or more `DisplayOutput`s, the author has named
 * something they definitely want to see. Routing is therefore ADDITIVE: a
 * named subject is forced to 'show' (outranking even its own
 * `default_visible === false`), but a realization that is NOT a subject keeps
 * its own default rules — routing never hides anything.
 *
 * It must be additive because `DisplayOutput` is OVERLOADED: the same
 * occurrence carries layer-3 appearance overrides AND multi-pane routing, and
 * `collect_display_routing` (engine.rs:4080-4086) emits a `DisplayDirective`
 * for EVERY DisplayDeferred occurrence, reading `pane` from the hydrated
 * instance where it is ALWAYS present (defaulted to 0). A directive therefore
 * proves nothing about visibility intent — an appearance-only
 * `DisplayOutput(subject:, style:)` is byte-identical on the wire to an
 * explicit `pane: 0`. An exhaustive `subject ? 'show' : 'hidden'` rule made
 * styling one body delete every other body from the viewport.
 *
 * Scoped to realization nodes (the things that carry meshes) and to the
 * auto:default view; the All-Geometry escape hatch and user views are
 * untouched.
 */
describe('DisplayOutput explicit routing (#5195)', () => {
  const subjectTree = () => [
    makeNode({
      entity_path: 'P',
      kind: 'structure',
      children: [
        makeNode({ entity_path: 'P.width', kind: 'param', type_name: 'Length' }),
        makeNode({ entity_path: 'P#realization[0]', kind: 'realization', default_visible: true }),
        makeNode({ entity_path: 'P#realization[1]', kind: 'realization', default_visible: true }),
      ],
    }),
  ];

  it('defaultVisibilityFor: subject realization → "show"', () => {
    const node = makeNode({ entity_path: 'P#realization[0]', kind: 'realization', default_visible: true });
    expect(defaultVisibilityFor(node, new Set(['P#realization[0]']))).toBe('show');
  });

  it('defaultVisibilityFor: non-subject realization keeps its own default → "show"', () => {
    const node = makeNode({ entity_path: 'P#realization[1]', kind: 'realization', default_visible: true });
    expect(defaultVisibilityFor(node, new Set(['P#realization[0]']))).toBe('show');
  });

  it('defaultVisibilityFor: routing outranks default_visible:false on a subject', () => {
    const node = makeNode({ entity_path: 'P#realization[0]', kind: 'realization', default_visible: false });
    expect(defaultVisibilityFor(node, new Set(['P#realization[0]']))).toBe('show');
  });

  it('defaultVisibilityFor: routing never reaches a non-realization node, even one named as a subject', () => {
    // The subject set contains the node's OWN path, so the path lookup in rule
    // -1 succeeds and `node.kind === 'realization'` is the only thing left
    // holding it back. Both nodes below resolve to 'hidden' under the normal
    // rules, so deleting the kind guard would flip them to 'show' — an
    // arrangement where the subject set and the node path never intersect
    // cannot detect that, since a param resolves to 'show' either way.
    const letGeometry = makeNode({
      entity_path: 'P.body',
      kind: 'let',
      type_name: 'Geometry',
    });
    expect(defaultVisibilityFor(letGeometry, new Set(['P.body']))).toBe('hidden');

    const hiddenParam = makeNode({
      entity_path: 'P.width',
      kind: 'param',
      type_name: 'Length',
      default_visible: false,
    });
    expect(defaultVisibilityFor(hiddenParam, new Set(['P.width']))).toBe('hidden');
  });

  it('generateDefaultView: subject forced show; non-subject keeps its own rule', () => {
    const view = generateDefaultView(subjectTree(), new Set(['P#realization[0]']));
    expect(view.visibility['P#realization[0]']).toBe('show');
    // ADDITIVE: realization[1] is not a subject, but its own default_visible is
    // true, so routing leaves it alone rather than deleting it from the view.
    expect(view.visibility['P#realization[1]']).toBe('show');
    // Non-realization nodes keep their normal rules.
    expect(view.visibility['P']).toBe('show');
    expect(view.visibility['P.width']).toBe('show');
  });

  it('generateDefaultView: an EMPTY subject set means no DisplayOutputs → normal rules', () => {
    const view = generateDefaultView(subjectTree(), new Set());
    expect(view.visibility['P#realization[0]']).toBe('show');
    expect(view.visibility['P#realization[1]']).toBe('show');
  });

  it('generateDefaultView: an omitted subject set behaves exactly as before', () => {
    const view = generateDefaultView(subjectTree());
    expect(view.visibility['P#realization[0]']).toBe('show');
    expect(view.visibility['P#realization[1]']).toBe('show');
  });

  it('generateAllGeometryView stays the escape hatch — routing does not reach it', () => {
    const view = generateAllGeometryView(subjectTree());
    expect(view.visibility['P#realization[0]']).toBe('show');
    expect(view.visibility['P#realization[1]']).toBe('show');
  });

  // -------------------------------------------------------------------------
  // Additive-routing regressions (#5195 step-12)
  // -------------------------------------------------------------------------

  /**
   * Mirrors the real `examples/appearance_viewport_egress.ri`: a top-level
   * `AppearanceViewportEgress` whose own `param geometry` is the RAL9001-styled
   * DisplayOutput subject, plus an INDEPENDENT `sub raw = RawEgress()` with its
   * own geometry realization that no DisplayOutput ever names.
   *
   * `collect_display_routing` (engine.rs:4080-4086) pushes a `DisplayDirective`
   * for that appearance-only `DisplayOutput(subject:, style:)` — it has no
   * `pane:` argument at all, yet arrives on the wire with `pane: 0` because the
   * hydrated instance always carries a defaulted `pane`. The paired engine-side
   * pin is engine_tests.rs::…display_output_defaults_pane_and_raw_stays_visible.
   */
  it('appearance-only DisplayOutput does not delete sibling bodies (examples/appearance_viewport_egress.ri shape)', () => {
    const tree = [
      makeNode({
        entity_path: 'AppearanceViewportEgress',
        kind: 'structure',
        children: [
          // B1/B3: the steel body carrying the RAL9001 layer-3 override — the subject.
          makeNode({
            entity_path: 'AppearanceViewportEgress#realization[0]',
            kind: 'realization',
            has_mesh: true,
            default_visible: true,
          }),
          // B2: the material-less raw box, never named by any DisplayOutput.
          makeNode({
            entity_path: 'AppearanceViewportEgress.raw',
            kind: 'sub',
            children: [
              makeNode({
                entity_path: 'AppearanceViewportEgress.raw#realization[0]',
                kind: 'realization',
                has_mesh: true,
                default_visible: true,
              }),
            ],
          }),
        ],
      }),
    ];
    const view = generateDefaultView(
      tree,
      new Set(['AppearanceViewportEgress#realization[0]']),
    );
    expect(view.visibility['AppearanceViewportEgress#realization[0]']).toBe('show');
    // The regression: styling one body must not delete the other.
    expect(view.visibility['AppearanceViewportEgress.raw#realization[0]']).toBe('show');
  });

  it('a non-subject consumed intermediate stays hidden (primary #5195 observable survives)', () => {
    // Guards the inverse over-correction: making routing additive must not
    // un-hide the engine-classified intermediates (body/hole/holes on the m5
    // flange). Those carry default_visible:false and rule 0 still owns them.
    const node = makeNode({
      entity_path: 'BoltFlange#realization[0]',
      kind: 'realization',
      has_mesh: true,
      default_visible: false,
    });
    expect(defaultVisibilityFor(node, new Set(['BoltFlange#realization[3]']))).toBe('hidden');
  });

  it('a subject routed to pane >= 1 does not blank pane 0 (generateDefaultView ∘ computePaneGroups)', () => {
    // The COMPOSITION is the thing that broke, so this test runs it rather than
    // re-asserting `generateDefaultView` alone: every pane shares ONE visibility
    // map (App.tsx `get entityVisibility()`), while computePaneGroups buckets
    // unrouted meshes into pane 0 via `subjectMap.get(path) ?? 0` (App.tsx:213).
    // Under the old exhaustive rule, routing realization[0] to pane 1 hid
    // realization[1] — which is exactly the mesh computePaneGroups puts in pane
    // 0 — so design-main rendered empty.
    const displayPanes: DisplayDirective[] = [{ subject: 'P#realization[0]', pane: 1 }];
    const meshes: Record<string, MeshData> = {
      'P#realization[0]': makeMesh('P#realization[0]'),
      'P#realization[1]': makeMesh('P#realization[1]'),
    };

    const view = generateDefaultView(
      subjectTree(),
      new Set(displayPanes.map((d) => d.subject)),
    );
    const { groups, dropped } = computePaneGroups(displayPanes, meshes);
    expect(dropped).toEqual([]);

    // Pane 0 (design-main) holds the UNROUTED body …
    const pane0 = groups.find((g) => g.pane === 0);
    expect(pane0).toBeDefined();
    expect(Object.keys(pane0!.meshes)).toEqual(['P#realization[1]']);
    // … and the shared visibility map must not have hidden it. This pairing is
    // the regression: a non-empty pane-0 bucket whose only member is 'hidden'
    // renders as a blank viewport.
    expect(view.visibility['P#realization[1]']).toBe('show');

    // The routed subject lands in pane 1 and is shown there.
    const pane1 = groups.find((g) => g.pane === 1);
    expect(pane1).toBeDefined();
    expect(Object.keys(pane1!.meshes)).toEqual(['P#realization[0]']);
    expect(view.visibility['P#realization[0]']).toBe('show');
  });
});
