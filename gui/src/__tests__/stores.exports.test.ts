/**
 * Step-39: Smoke test that all new public exports from gui/src/stores
 * (the barrel index.ts) resolve to callable functions.
 *
 * This test will fail until step-40 adds the missing re-exports to
 * gui/src/stores/index.ts.
 */
import { describe, it, expect } from 'vitest';

// Import the barrel — step-40 must add these to stores/index.ts
import {
  // viewPersistence exports
  loadViewPersistence,
  saveViewPersistence,
  createDebouncedSaver,
  STORAGE_KEY_PREFIX,
  // sidecarPersistence exports (already in index.ts from step-12)
  loadSidecar,
  saveSidecar,
  // fuzzyPathMatcher exports
  findFuzzyCandidate,
  suffixMatch,
  structuralMatch,
  // feaModeStore exports (task 2961, step-9)
  createFeaModeStore,
  // probeStore exports (task 2964, step-23/24)
  createProbeStore,
} from '../stores';
import type { FeaModeStore } from '../stores';
import type { ProbeStore, PinnedProbe, ProbeSample, BarycentricUV } from '../stores';

// Type-level assertion: FeaModeStore type re-exports and is assignable.
const _feaModeStoreTypeCheck: FeaModeStore | null = null;
void _feaModeStoreTypeCheck;

describe('stores barrel — new public exports resolve (step-39)', () => {
  it('loadViewPersistence is a function', () => {
    expect(typeof loadViewPersistence).toBe('function');
  });

  it('saveViewPersistence is a function', () => {
    expect(typeof saveViewPersistence).toBe('function');
  });

  it('createDebouncedSaver is a function', () => {
    expect(typeof createDebouncedSaver).toBe('function');
  });

  it('STORAGE_KEY_PREFIX is the expected string', () => {
    expect(STORAGE_KEY_PREFIX).toBe('reify:views:');
  });

  it('loadSidecar is a function', () => {
    expect(typeof loadSidecar).toBe('function');
  });

  it('saveSidecar is a function', () => {
    expect(typeof saveSidecar).toBe('function');
  });

  it('findFuzzyCandidate is a function', () => {
    expect(typeof findFuzzyCandidate).toBe('function');
  });

  it('suffixMatch is a function', () => {
    expect(typeof suffixMatch).toBe('function');
  });

  it('structuralMatch is a function', () => {
    expect(typeof structuralMatch).toBe('function');
  });
});

describe('stores barrel — feaModeStore exports (task 2961, step-9)', () => {
  it('createFeaModeStore is callable', () => {
    expect(typeof createFeaModeStore).toBe('function');
  });

  it('createFeaModeStore() returns an object with state and mutations', () => {
    const store = createFeaModeStore();
    expect(store).toHaveProperty('state');
    expect(typeof store.setEnabled).toBe('function');
    expect(typeof store.setChannel).toBe('function');
    expect(typeof store.setPalette).toBe('function');
    expect(typeof store.setRange).toBe('function');
    expect(typeof store.lockCurrent).toBe('function');
    expect(typeof store.tryAutoEnable).toBe('function');
  });
});

// Type-level assertions: ProbeStore/PinnedProbe/ProbeSample/BarycentricUV re-export and are assignable.
const _probeStoreTypeCheck: ProbeStore | null = null;
void _probeStoreTypeCheck;
const _pinnedProbeTypeCheck: PinnedProbe | null = null;
void _pinnedProbeTypeCheck;
const _probeSampleTypeCheck: ProbeSample | null = null;
void _probeSampleTypeCheck;
const _barycentricUVTypeCheck: BarycentricUV | null = null;
void _barycentricUVTypeCheck;

describe('stores barrel — probeStore exports (task 2964, step-23/24)', () => {
  it('createProbeStore is a function', () => {
    expect(typeof createProbeStore).toBe('function');
  });

  it('createProbeStore() returns an object with state and mutation methods', () => {
    const store = createProbeStore();
    expect(store).toHaveProperty('state');
    expect(Array.isArray(store.state.probes)).toBe(true);
    expect(typeof store.addProbe).toBe('function');
    expect(typeof store.removeProbe).toBe('function');
    expect(typeof store.clear).toBe('function');
    expect(typeof store.updateSample).toBe('function');
    expect(typeof store.markStale).toBe('function');
  });
});
