/**
 * Unit suite for feaToolbarChannels — the base list plus its 'errorIndicator'
 * extension (task 3001), and the currentChannel-seed + PREFERRED_FEA_CHANNELS
 * widening policy (tasks 5669/5828, which moved that policy out of Viewport.tsx
 * so it could be pinned here instead of by a full <Viewport> render).
 */
import { describe, it, expect } from 'vitest';
import { feaToolbarChannels } from '../../viewport/feaToolbarChannels';
import type { MeshData } from '../../types';

// ─── helpers ─────────────────────────────────────────────────────────────────

/** Build a MeshData with the given scalar_channels (name → values). */
function makeMesh(channels: Record<string, number[]>): MeshData {
  const scalar_channels: Record<string, Float32Array> = {};
  for (const [name, values] of Object.entries(channels)) {
    scalar_channels[name] = new Float32Array(values);
  }
  return {
    entity_path: 'test',
    vertices: new Float32Array(0),
    indices: new Uint32Array(0),
    normals: null,
    scalar_channels,
  } as unknown as MeshData;
}

function makeMeshNoChannels(): MeshData {
  return {
    entity_path: 'test',
    vertices: new Float32Array(0),
    indices: new Uint32Array(0),
    normals: null,
  } as unknown as MeshData;
}

// ─── tests ────────────────────────────────────────────────────────────────────

describe('feaToolbarChannels', () => {
  it('(a) empty meshes record → base list only', () => {
    expect(feaToolbarChannels({})).toEqual(['vonMises', 'displacement_magnitude']);
  });

  it('(b) mesh with no scalar_channels → base list only', () => {
    const meshes = { m: makeMeshNoChannels() };
    expect(feaToolbarChannels(meshes)).toEqual(['vonMises', 'displacement_magnitude']);
  });

  it('(c) mesh with vonMises only (no errorIndicator) → base list only', () => {
    const meshes = { m: makeMesh({ vonMises: [1.0, 2.0] }) };
    expect(feaToolbarChannels(meshes)).toEqual(['vonMises', 'displacement_magnitude']);
  });

  it('(d) mesh with a non-empty errorIndicator channel → base list + errorIndicator appended', () => {
    const meshes = { m: makeMesh({ vonMises: [1.0], errorIndicator: [0.5, 0.6] }) };
    expect(feaToolbarChannels(meshes)).toEqual([
      'vonMises',
      'displacement_magnitude',
      'errorIndicator',
    ]);
  });

  it('(e) errorIndicator present but EMPTY on the only mesh → not appended', () => {
    const meshes = { m: makeMesh({ vonMises: [1.0], errorIndicator: [] }) };
    expect(feaToolbarChannels(meshes)).toEqual(['vonMises', 'displacement_magnitude']);
  });

  it('(f) errorIndicator empty on mesh A but non-empty on mesh B → appended (union semantics)', () => {
    const meshes = {
      a: makeMesh({ errorIndicator: [] }),
      b: makeMesh({ errorIndicator: [3.0] }),
    };
    expect(feaToolbarChannels(meshes)).toEqual([
      'vonMises',
      'displacement_magnitude',
      'errorIndicator',
    ]);
  });

  it('(g) multiple meshes each with non-empty errorIndicator → appended exactly once (no duplicates)', () => {
    const meshes = {
      a: makeMesh({ errorIndicator: [1.0] }),
      b: makeMesh({ errorIndicator: [2.0] }),
    };
    const result = feaToolbarChannels(meshes);
    expect(result).toEqual(['vonMises', 'displacement_magnitude', 'errorIndicator']);
    expect(result.filter((c) => c === 'errorIndicator')).toHaveLength(1);
  });

  it('(h) base list order is always ["vonMises", "displacement_magnitude", ...] regardless of channel insertion order', () => {
    const meshes = { m: makeMesh({ errorIndicator: [1.0], vonMises: [2.0] }) };
    expect(feaToolbarChannels(meshes)).toEqual([
      'vonMises',
      'displacement_magnitude',
      'errorIndicator',
    ]);
  });
});

// ─── widening policy (task 5828) ──────────────────────────────────────────────
//
// The option-list widening policy — the caller's current channel seeded in, plus
// a PREFERRED_FEA_CHANNELS-restricted scan of the live mesh set — used to live
// inline in Viewport.tsx's `feaChannelOptions` memo (task 5669). Task 5828 moved
// it into feaToolbarChannels so it can be pinned as pure unit cases here instead
// of costing a full <Viewport> render each. The 8 one-arg cases (a)-(h) above are
// deliberately untouched: they are the regression floor proving `currentChannel`
// stays optional and the base-segment computation is unchanged.

describe('feaToolbarChannels widening policy (currentChannel seed + PREFERRED_FEA_CHANNELS scan)', () => {
  it('(i) omitted/undefined currentChannel is a no-op', () => {
    expect(feaToolbarChannels({}, undefined)).toEqual(feaToolbarChannels({}));
    expect(feaToolbarChannels({}, undefined)).toEqual(['vonMises', 'displacement_magnitude']);
  });

  it('(j) seed admits a channel outside both the base list and PREFERRED_FEA_CHANNELS', () => {
    expect(feaToolbarChannels({}, 'temperature')).toEqual([
      'vonMises',
      'displacement_magnitude',
      'temperature',
    ]);
  });

  it('(k) seed already in the base list is not duplicated', () => {
    expect(feaToolbarChannels({}, 'vonMises')).toEqual(['vonMises', 'displacement_magnitude']);
  });

  it('(l) seed equal to errorIndicator on a mesh carrying it → appended exactly once, in the base segment', () => {
    const meshes = { m: makeMesh({ vonMises: [1.0], errorIndicator: [0.5] }) };
    const result = feaToolbarChannels(meshes, 'errorIndicator');
    expect(result).toEqual(['vonMises', 'displacement_magnitude', 'errorIndicator']);
    expect(result.filter((c) => c === 'errorIndicator')).toHaveLength(1);
  });

  it('(m) scan admits shell sub-channels with no currentChannel', () => {
    const meshes = { shell: makeMesh({ vonMises_top: [3], vonMises_bottom: [1] }) };
    expect(feaToolbarChannels(meshes)).toEqual([
      'vonMises',
      'displacement_magnitude',
      'vonMises_bottom',
      'vonMises_top',
    ]);
  });

  it('(n) scan is restricted to PREFERRED_FEA_CHANNELS (non-FEA vertex scalars are not admitted)', () => {
    const meshes = {
      solid: makeMesh({ vonMises: [1], displacement_magnitude: [2], temperature: [300] }),
    };
    // Regression guard against re-widening to every non-empty channel.
    expect(feaToolbarChannels(meshes, 'vonMises')).toEqual([
      'vonMises',
      'displacement_magnitude',
    ]);
  });

  it('(o) scan skips EMPTY preferred channels', () => {
    const meshes = { shell: makeMesh({ vonMises_top: [] }) };
    expect(feaToolbarChannels(meshes)).toEqual(['vonMises', 'displacement_magnitude']);
  });

  it('(p) scan is a union across meshes', () => {
    const meshes = {
      a: makeMesh({ vonMises_top: [] }),
      b: makeMesh({ vonMises_top: [1] }),
    };
    expect(feaToolbarChannels(meshes)).toEqual([
      'vonMises',
      'displacement_magnitude',
      'vonMises_top',
    ]);
  });

  it('(q) seed and scan extras are sorted together as ONE set (ordering contract)', () => {
    const meshes = {
      shell: makeMesh({ vonMises_top: [3], vonMises_mid: [2], vonMises_bottom: [1] }),
    };
    // 'temperature' sorts BEFORE the shell sub-channels — pinning that the seed
    // is not merely appended last after the scan extras.
    expect(feaToolbarChannels(meshes, 'temperature')).toEqual([
      'vonMises',
      'displacement_magnitude',
      'temperature',
      'vonMises_bottom',
      'vonMises_mid',
      'vonMises_top',
    ]);
  });

  it('(r) seed equal to a scanned channel is not duplicated', () => {
    const meshes = { shell: makeMesh({ vonMises_top: [3] }) };
    const result = feaToolbarChannels(meshes, 'vonMises_top');
    expect(result).toEqual(['vonMises', 'displacement_magnitude', 'vonMises_top']);
    expect(result.filter((c) => c === 'vonMises_top')).toHaveLength(1);
  });

  it('(s) seed applies to a mesh with no scalar_channels at all', () => {
    const meshes = { m: makeMeshNoChannels() };
    expect(feaToolbarChannels(meshes, 'temperature')).toEqual([
      'vonMises',
      'displacement_magnitude',
      'temperature',
    ]);
  });
});
