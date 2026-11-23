import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  createViewportDisplayStore,
  VIEWPORT_DISPLAY_STORAGE_KEY,
} from '../stores/viewportDisplayStore';

function stored(): unknown {
  const raw = localStorage.getItem(VIEWPORT_DISPLAY_STORAGE_KEY);
  return raw === null ? null : JSON.parse(raw);
}

describe('viewportDisplayStore', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('defaults edges to off when nothing is stored', () => {
    expect(createViewportDisplayStore().state.edges).toBe(false);
  });

  it('setEdges(true) updates the state, persists it, and a new store reads it back', () => {
    const store = createViewportDisplayStore();

    store.setEdges(true);

    expect(store.state.edges).toBe(true);
    expect(stored()).toEqual({ edges: true });
    expect(createViewportDisplayStore().state.edges).toBe(true);
  });

  it('toggleEdges() flips the value and persists every flip', () => {
    const store = createViewportDisplayStore();

    store.toggleEdges();
    expect(store.state.edges).toBe(true);
    expect(stored()).toEqual({ edges: true });

    store.toggleEdges();
    expect(store.state.edges).toBe(false);
    expect(stored()).toEqual({ edges: false });
  });

  it('reset() restores the default and persists it', () => {
    const store = createViewportDisplayStore();
    store.setEdges(true);

    store.reset();

    expect(store.state.edges).toBe(false);
    expect(stored()).toEqual({ edges: false });
  });

  it.each([
    ['corrupt JSON', '{edges:'],
    ['a non-object', '"edges"'],
    ['null', 'null'],
    ['a non-boolean edges field', JSON.stringify({ edges: 'yes' })],
  ])('reads the default when the stored value is %s', (_label, raw) => {
    localStorage.setItem(VIEWPORT_DISPLAY_STORAGE_KEY, raw);

    expect(createViewportDisplayStore().state.edges).toBe(false);
  });

  it('still updates the state when localStorage refuses the write', () => {
    const store = createViewportDisplayStore();
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('QuotaExceededError');
    });

    expect(() => store.setEdges(true)).not.toThrow();
    expect(store.state.edges).toBe(true);
  });
});
