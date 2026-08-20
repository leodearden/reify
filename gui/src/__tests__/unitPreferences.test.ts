// @vitest-environment jsdom
import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  UNIT_PREFERENCES_KEY,
  loadAllUnitPreferences,
  loadUnitPreference,
  pruneUnitPreferences,
  saveUnitPreference,
} from '../stores/unitPreferences';

describe('unitPreferences', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('round-trips a saved preference', () => {
    saveUnitPreference('Tank.capacity', 'L');
    expect(loadUnitPreference('Tank.capacity')).toBe('L');
  });

  it('returns null for a cell with no stored preference', () => {
    expect(loadUnitPreference('Missing.cell')).toBeNull();
  });

  it('keys preferences per-cell — saving one cell leaves another unaffected', () => {
    saveUnitPreference('Tank.capacity', 'L');
    expect(loadUnitPreference('Tank.height')).toBeNull();
  });

  it('returns null for corrupted JSON in the store key, without throwing', () => {
    localStorage.setItem(UNIT_PREFERENCES_KEY, '{broken json!!!');
    expect(() => loadUnitPreference('Tank.capacity')).not.toThrow();
    expect(loadUnitPreference('Tank.capacity')).toBeNull();
  });

  it('returns null when the store key holds valid JSON that is not an object', () => {
    localStorage.setItem(UNIT_PREFERENCES_KEY, JSON.stringify('not an object'));
    expect(loadUnitPreference('Tank.capacity')).toBeNull();

    localStorage.setItem(UNIT_PREFERENCES_KEY, JSON.stringify([1, 2, 3]));
    expect(loadUnitPreference('Tank.capacity')).toBeNull();
  });

  it('returns null when the cell entry is present but not a string', () => {
    localStorage.setItem(UNIT_PREFERENCES_KEY, JSON.stringify({ 'Tank.capacity': 123 }));
    expect(loadUnitPreference('Tank.capacity')).toBeNull();
  });

  // loadAllUnitPreferences (task #5199 amend: bulk-load helper so callers with
  // many cells parse the localStorage blob once instead of per-cell).
  describe('loadAllUnitPreferences', () => {
    it('returns {} when the store key is absent', () => {
      expect(loadAllUnitPreferences()).toEqual({});
    });

    it('returns every saved cell -> label pair in one map', () => {
      saveUnitPreference('Tank.capacity', 'L');
      saveUnitPreference('Tank.height', 'mm');
      expect(loadAllUnitPreferences()).toEqual({
        'Tank.capacity': 'L',
        'Tank.height': 'mm',
      });
    });

    it('drops non-string entries but keeps the valid ones', () => {
      localStorage.setItem(
        UNIT_PREFERENCES_KEY,
        JSON.stringify({ 'Tank.capacity': 'L', 'Tank.height': 123 }),
      );
      expect(loadAllUnitPreferences()).toEqual({ 'Tank.capacity': 'L' });
    });

    it('returns {} for corrupted JSON, without throwing', () => {
      localStorage.setItem(UNIT_PREFERENCES_KEY, '{broken json!!!');
      expect(() => loadAllUnitPreferences()).not.toThrow();
      expect(loadAllUnitPreferences()).toEqual({});
    });

    it('returns {} when the store key holds valid JSON that is not an object', () => {
      localStorage.setItem(UNIT_PREFERENCES_KEY, JSON.stringify([1, 2, 3]));
      expect(loadAllUnitPreferences()).toEqual({});
    });

    it('stays consistent with loadUnitPreference for the same store contents', () => {
      saveUnitPreference('Tank.capacity', 'L');
      const all = loadAllUnitPreferences();
      expect(all['Tank.capacity']).toBe(loadUnitPreference('Tank.capacity'));
      expect(all['Missing.cell']).toBeUndefined();
      expect(loadUnitPreference('Missing.cell')).toBeNull();
    });
  });

  // pruneUnitPreferences (task #5199 amend, reviewer_comprehensive
  // resource_cleanup finding: the persisted blob is keyed by cell_id and
  // previously only ever grew across files/sessions).
  describe('pruneUnitPreferences', () => {
    it('drops entries whose cell_id is not in the valid set', () => {
      saveUnitPreference('Tank.capacity', 'L');
      saveUnitPreference('Tank.height', 'mm');
      pruneUnitPreferences(['Tank.capacity']);
      expect(loadAllUnitPreferences()).toEqual({ 'Tank.capacity': 'L' });
      expect(loadUnitPreference('Tank.height')).toBeNull();
    });

    it('accepts a Set as well as an array for the valid-id argument', () => {
      saveUnitPreference('Tank.capacity', 'L');
      saveUnitPreference('Tank.height', 'mm');
      pruneUnitPreferences(new Set(['Tank.height']));
      expect(loadAllUnitPreferences()).toEqual({ 'Tank.height': 'mm' });
    });

    it('keeps every entry when all cell_ids are still valid', () => {
      saveUnitPreference('Tank.capacity', 'L');
      saveUnitPreference('Tank.height', 'mm');
      pruneUnitPreferences(['Tank.capacity', 'Tank.height', 'Unrelated.cell']);
      expect(loadAllUnitPreferences()).toEqual({
        'Tank.capacity': 'L',
        'Tank.height': 'mm',
      });
    });

    it('drops every entry when the valid set is empty', () => {
      saveUnitPreference('Tank.capacity', 'L');
      pruneUnitPreferences([]);
      expect(loadAllUnitPreferences()).toEqual({});
    });

    it('is a no-op (no localStorage write) when nothing is stale', () => {
      saveUnitPreference('Tank.capacity', 'L');
      const setItemSpy = vi.spyOn(Storage.prototype, 'setItem');
      setItemSpy.mockClear();
      pruneUnitPreferences(['Tank.capacity']);
      expect(setItemSpy).not.toHaveBeenCalled();
      setItemSpy.mockRestore();
    });

    it('does nothing when the store key is absent', () => {
      expect(() => pruneUnitPreferences(['Tank.capacity'])).not.toThrow();
      expect(loadAllUnitPreferences()).toEqual({});
    });

    it('does not throw for corrupted JSON in the store key', () => {
      localStorage.setItem(UNIT_PREFERENCES_KEY, '{broken json!!!');
      expect(() => pruneUnitPreferences(['Tank.capacity'])).not.toThrow();
    });
  });
});
