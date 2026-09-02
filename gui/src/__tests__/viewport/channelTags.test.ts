/**
 * Unit suite for the scalar-channel tag helpers — task #6185 step-7 RED.
 *
 * This file imports from ../../viewport/channelTags, which does not yet exist.
 * The suite MUST fail to import (module absent) in step-7 — that is the RED
 * state, the same shape as scalarRange.test.ts's own step-1 RED.
 */
import { describe, it, expect } from 'vitest';
import { channelUnit, isSignedChannel } from '../../viewport/channelTags';
import type { MeshData, ScalarChannelTag } from '../../types';

// ─── helpers ─────────────────────────────────────────────────────────────────

function makeMesh(
  channels: Record<string, number[]>,
  tags?: Record<string, ScalarChannelTag>,
): MeshData {
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
    ...(tags === undefined ? {} : { scalar_channel_tags: tags }),
  } as unknown as MeshData;
}

const PA: ScalarChannelTag = { unit: 'Pa', signed: false };
const RAD: ScalarChannelTag = { unit: 'rad', signed: true };

// ─── isSignedChannel ─────────────────────────────────────────────────────────

describe('isSignedChannel', () => {
  it('(a) treats an untagged channel as unsigned', () => {
    // Pre-tag payloads must keep today's behaviour exactly.
    const meshes = { m: makeMesh({ vonMises: [1, 2] }) };
    expect(isSignedChannel(meshes, 'vonMises')).toBe(false);
  });

  it('(b) reports false for an explicitly unsigned tag', () => {
    const meshes = { m: makeMesh({ vonMises: [1, 2] }, { vonMises: PA }) };
    expect(isSignedChannel(meshes, 'vonMises')).toBe(false);
  });

  it('(c) reports true for a signed tag', () => {
    const meshes = { m: makeMesh({ rotation: [-1, 2] }, { rotation: RAD }) };
    expect(isSignedChannel(meshes, 'rotation')).toBe(true);
  });

  it('(d) is ANY-signed: one mesh tagging it signed is enough', () => {
    // Safe direction: if any producer says values can be negative, dropping
    // them anywhere is data loss, whereas widening the scale is merely a
    // wider scale.
    const meshes = {
      tagged: makeMesh({ rotation: [-1, 2] }, { rotation: RAD }),
      untagged: makeMesh({ rotation: [0, 3] }),
    };
    expect(isSignedChannel(meshes, 'rotation')).toBe(true);
  });

  it('(e) reports false when the channel is absent from every mesh', () => {
    const meshes = { m: makeMesh({ vonMises: [1, 2] }, { vonMises: PA }) };
    expect(isSignedChannel(meshes, 'notAChannel')).toBe(false);
  });

  it('(f) reports false when the tags map tags a DIFFERENT channel', () => {
    const meshes = {
      m: makeMesh({ vonMises: [1, 2], rotation: [-1, 2] }, { rotation: RAD }),
    };
    expect(isSignedChannel(meshes, 'vonMises')).toBe(false);
  });
});

// ─── channelUnit ─────────────────────────────────────────────────────────────

describe('channelUnit', () => {
  it('(g) returns the unit of a single tagged mesh', () => {
    const meshes = { m: makeMesh({ vonMises: [1, 2] }, { vonMises: PA }) };
    expect(channelUnit(meshes, 'vonMises')).toBe('Pa');
  });

  it('(h) returns null for an untagged or absent channel', () => {
    const untagged = { m: makeMesh({ vonMises: [1, 2] }) };
    expect(channelUnit(untagged, 'vonMises')).toBeNull();

    const absent = { m: makeMesh({ vonMises: [1, 2] }, { vonMises: PA }) };
    expect(channelUnit(absent, 'notAChannel')).toBeNull();
  });

  it('(i) returns the unit when two meshes agree', () => {
    const meshes = {
      a: makeMesh({ vonMises: [1, 2] }, { vonMises: PA }),
      b: makeMesh({ vonMises: [3, 4] }, { vonMises: PA }),
    };
    expect(channelUnit(meshes, 'vonMises')).toBe('Pa');
  });

  it('(j) returns null when two meshes DISAGREE on the unit', () => {
    // Unanimous-or-null: never show a unit that is wrong for some of the
    // data feeding the same legend.
    const meshes = {
      a: makeMesh({ ch: [1, 2] }, { ch: PA }),
      b: makeMesh({ ch: [3, 4] }, { ch: RAD }),
    };
    expect(channelUnit(meshes, 'ch')).toBeNull();
  });

  it('(k) returns null for a tag carrying an empty-string unit', () => {
    const meshes = {
      m: makeMesh({ ch: [1, 2] }, { ch: { unit: '', signed: false } }),
    };
    expect(channelUnit(meshes, 'ch')).toBeNull();
  });

  it('(l) lets an untagged mesh abstain rather than veto', () => {
    const meshes = {
      tagged: makeMesh({ rotation: [-1, 2] }, { rotation: RAD }),
      untagged: makeMesh({ rotation: [0, 3] }),
    };
    expect(channelUnit(meshes, 'rotation')).toBe('rad');
  });
});
