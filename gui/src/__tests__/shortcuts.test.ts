import { describe, it, expect } from 'vitest';
import { SHORTCUTS, getShortcut, shortcutKey } from '../shortcuts';

describe('shortcuts', () => {
  it('SHORTCUTS array contains entries for all expected shortcut ids', () => {
    const ids = SHORTCUTS.map((s) => s.id);
    expect(ids).toContain('open');
    expect(ids).toContain('save');
    expect(ids).toContain('export');
    expect(ids).toContain('reEvaluate');
    expect(ids).toContain('fitToView');
    expect(ids).toContain('toggleChat');
    expect(ids).toContain('help');
  });

  it('each SHORTCUTS entry has id, key, and description fields', () => {
    for (const s of SHORTCUTS) {
      expect(typeof s.id).toBe('string');
      expect(typeof s.description).toBe('string');
      // key may be empty string for entries without a shortcut
      expect(typeof s.key).toBe('string');
    }
  });

  it('getShortcut("open") returns the open entry', () => {
    const entry = getShortcut('open');
    expect(entry).toBeDefined();
    expect(entry?.id).toBe('open');
    expect(entry?.key).toBe('Ctrl+O');
    expect(entry?.description).toBeTruthy();
  });

  it('getShortcut("save") returns the save entry', () => {
    const entry = getShortcut('save');
    expect(entry).toBeDefined();
    expect(entry?.id).toBe('save');
    expect(entry?.key).toBe('Ctrl+S');
  });

  it('getShortcut("reEvaluate") returns the reEvaluate entry with key F5', () => {
    const entry = getShortcut('reEvaluate');
    expect(entry).toBeDefined();
    expect(entry?.key).toBe('F5');
  });

  it('getShortcut("help") returns the help entry with key ?', () => {
    const entry = getShortcut('help');
    expect(entry).toBeDefined();
    expect(entry?.key).toBe('?');
  });

  it('getShortcut for unknown id returns undefined', () => {
    expect(getShortcut('nonexistent-id')).toBeUndefined();
  });

  it('shortcutKey("open") returns "Ctrl+O"', () => {
    expect(shortcutKey('open')).toBe('Ctrl+O');
  });

  it('shortcutKey("save") returns "Ctrl+S"', () => {
    expect(shortcutKey('save')).toBe('Ctrl+S');
  });

  it('shortcutKey("export") returns "Ctrl+E"', () => {
    expect(shortcutKey('export')).toBe('Ctrl+E');
  });

  it('shortcutKey("reEvaluate") returns "F5"', () => {
    expect(shortcutKey('reEvaluate')).toBe('F5');
  });

  it('shortcutKey("toggleChat") returns "Ctrl+J"', () => {
    expect(shortcutKey('toggleChat')).toBe('Ctrl+J');
  });

  it('shortcutKey("help") returns "?"', () => {
    expect(shortcutKey('help')).toBe('?');
  });

  it('shortcutKey for unknown id returns empty string', () => {
    expect(shortcutKey('nonexistent-id')).toBe('');
  });
});
