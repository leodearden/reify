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
} from '../stores';

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
