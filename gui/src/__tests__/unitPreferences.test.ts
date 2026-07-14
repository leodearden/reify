// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';
import {
  UNIT_PREFERENCES_KEY,
  loadUnitPreference,
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
});
