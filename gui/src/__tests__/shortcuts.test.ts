import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { SHORTCUTS, getShortcut, shortcutKey, matchesEvent, type KeyBinding, type ShortcutDef } from '../shortcuts';

const SRC = readFileSync(join(__dirname, '../shortcuts.ts'), 'utf-8');

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
    // @ts-expect-error - unknown id must be rejected by ShortcutId narrowing
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
    // @ts-expect-error - unknown id must be rejected by ShortcutId narrowing
    expect(shortcutKey('nonexistent-id')).toBe('');
  });

  it('getShortcut("undo") has disabled === true', () => {
    const entry = getShortcut('undo');
    expect(entry).toBeDefined();
    expect(entry?.disabled).toBe(true);
  });

  it('getShortcut("redo") has disabled === true', () => {
    const entry = getShortcut('redo');
    expect(entry).toBeDefined();
    expect(entry?.disabled).toBe(true);
  });

  it('all non-disabled shortcuts with keys do NOT have disabled set to true', () => {
    for (const s of SHORTCUTS.filter((sh) => !sh.disabled && sh.key)) {
      const entry = getShortcut(s.id);
      expect(entry).toBeDefined();
      expect(entry?.disabled).not.toBe(true);
    }
  });
});

describe('matchesEvent', () => {
  function makeEvent(init: KeyboardEventInit): KeyboardEvent {
    return new KeyboardEvent('keydown', init);
  }

  it('matches Ctrl+letter when ctrl:true and ctrlKey:true in event', () => {
    const bind: KeyBinding = { key: 'o', ctrl: true };
    expect(matchesEvent(bind, makeEvent({ key: 'o', ctrlKey: true }))).toBe(true);
  });

  it('does not match Ctrl+letter when ctrl:true but ctrlKey:false', () => {
    const bind: KeyBinding = { key: 'o', ctrl: true };
    expect(matchesEvent(bind, makeEvent({ key: 'o', ctrlKey: false }))).toBe(false);
  });

  it('does not match Ctrl+letter when ctrlKey is missing from event', () => {
    const bind: KeyBinding = { key: 'o', ctrl: true };
    expect(matchesEvent(bind, makeEvent({ key: 'o' }))).toBe(false);
  });

  it('matches Ctrl+Shift+letter combo', () => {
    const bind: KeyBinding = { key: 'r', ctrl: true, shift: true };
    expect(matchesEvent(bind, makeEvent({ key: 'r', ctrlKey: true, shiftKey: true }))).toBe(true);
  });

  it('does not match Ctrl+Shift+letter when shiftKey is not held', () => {
    const bind: KeyBinding = { key: 'r', ctrl: true, shift: true };
    expect(matchesEvent(bind, makeEvent({ key: 'r', ctrlKey: true, shiftKey: false }))).toBe(false);
  });

  it('matches function key F5 with no modifiers', () => {
    const bind: KeyBinding = { key: 'F5' };
    expect(matchesEvent(bind, makeEvent({ key: 'F5' }))).toBe(true);
  });

  it('matches ? with ctrl:false and alt:false against bare ? keydown', () => {
    const bind: KeyBinding = { key: '?', ctrl: false, alt: false };
    expect(matchesEvent(bind, makeEvent({ key: '?' }))).toBe(true);
  });

  it('rejects ? with ctrl:false when ctrlKey is held', () => {
    const bind: KeyBinding = { key: '?', ctrl: false, alt: false };
    expect(matchesEvent(bind, makeEvent({ key: '?', ctrlKey: true }))).toBe(false);
  });

  it('rejects ? with alt:false when altKey is held', () => {
    const bind: KeyBinding = { key: '?', ctrl: false, alt: false };
    expect(matchesEvent(bind, makeEvent({ key: '?', altKey: true }))).toBe(false);
  });

  it('matches single-char key case-insensitively (bind lowercase, event uppercase)', () => {
    const bind: KeyBinding = { key: 'r', ctrl: true, shift: true };
    expect(matchesEvent(bind, makeEvent({ key: 'R', ctrlKey: true, shiftKey: true }))).toBe(true);
  });

  it('matches single-char key case-insensitively (bind uppercase, event lowercase)', () => {
    const bind: KeyBinding = { key: 'R', ctrl: true };
    expect(matchesEvent(bind, makeEvent({ key: 'r', ctrlKey: true }))).toBe(true);
  });

  it('returns false for wrong key', () => {
    const bind: KeyBinding = { key: 'o', ctrl: true };
    expect(matchesEvent(bind, makeEvent({ key: 'p', ctrlKey: true }))).toBe(false);
  });

  it('unspecified modifiers are ignored (don\'t-care semantics)', () => {
    // F5 with ctrl unspecified — matches whether ctrl is held or not
    const bind: KeyBinding = { key: 'F5' };
    expect(matchesEvent(bind, makeEvent({ key: 'F5', ctrlKey: true }))).toBe(true);
  });

  it('returns false for matching key but a required modifier is held when it must not be', () => {
    // ctrl must be false, but ctrlKey is true in the event
    const bind: KeyBinding = { key: '?', ctrl: false };
    expect(matchesEvent(bind, makeEvent({ key: '?', ctrlKey: true }))).toBe(false);
  });

  // meta field (suggestion 3 — cross-platform Cmd key support)
  it('matches with meta:true when metaKey is held', () => {
    const bind: KeyBinding = { key: 's', meta: true };
    expect(matchesEvent(bind, makeEvent({ key: 's', metaKey: true }))).toBe(true);
  });

  it('does not match with meta:true when metaKey is not held', () => {
    const bind: KeyBinding = { key: 's', meta: true };
    expect(matchesEvent(bind, makeEvent({ key: 's', metaKey: false }))).toBe(false);
  });

  it('meta and ctrl are independent: meta:true does not fire when only ctrlKey is held', () => {
    // ctrlKey is held but metaKey is not — meta:true requires event.metaKey, not event.ctrlKey
    const bind: KeyBinding = { key: 's', meta: true };
    expect(matchesEvent(bind, makeEvent({ key: 's', ctrlKey: true }))).toBe(false);
  });
});

describe('SHORTCUTS bind fields', () => {
  it('every entry with a non-empty key display string has a bind field', () => {
    for (const s of SHORTCUTS) {
      if (s.key !== '') {
        // switchViewByIndex uses a descriptive range "1-9" as its display key, not a
        // literal binding — the actual dispatch is a special-case block in
        // useKeyboardShortcuts (mirroring how Escape is handled).
        if (s.id === 'switchViewByIndex') continue;
        // fold/unfold/foldAll/unfoldAll are display-only: dispatch is handled by
        // CodeMirror's foldKeymap inside the editor; useKeyboardShortcuts skips
        // them because the CM contentDOM is contentEditable (bails before matching).
        if (s.id === 'fold' || s.id === 'unfold' || s.id === 'foldAll' || s.id === 'unfoldAll') continue;
        expect(s.bind, `shortcut "${s.id}" has a display key but no bind field`).toBeDefined();
      }
    }
  });

  it('open bind matches Ctrl+O keydown event', () => {
    const entry = getShortcut('open');
    expect(entry?.bind).toBeDefined();
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'o', ctrlKey: true }))).toBe(true);
    // wrong modifier must not match
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'o', ctrlKey: false }))).toBe(false);
    // Ctrl+Shift+O must not match (shift:false prevents the case-insensitive 'O' from firing)
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'O', ctrlKey: true, shiftKey: true }))).toBe(false);
  });

  it('save bind matches Ctrl+S keydown event', () => {
    const entry = getShortcut('save');
    expect(entry?.bind).toBeDefined();
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 's', ctrlKey: true }))).toBe(true);
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 's', ctrlKey: false }))).toBe(false);
    // Ctrl+Shift+S must not match
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'S', ctrlKey: true, shiftKey: true }))).toBe(false);
  });

  it('export bind matches Ctrl+E keydown event', () => {
    const entry = getShortcut('export');
    expect(entry?.bind).toBeDefined();
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'e', ctrlKey: true }))).toBe(true);
    // Ctrl+Shift+E must not match
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'E', ctrlKey: true, shiftKey: true }))).toBe(false);
  });

  it('undo bind matches Ctrl+Z keydown event but NOT Ctrl+Shift+Z', () => {
    const entry = getShortcut('undo');
    expect(entry?.bind).toBeDefined();
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'z', ctrlKey: true }))).toBe(true);
    // shift:false makes the non-overlap with redo explicit
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'z', ctrlKey: true, shiftKey: true }))).toBe(false);
  });

  it('redo bind matches Ctrl+Shift+Z keydown event', () => {
    const entry = getShortcut('redo');
    expect(entry?.bind).toBeDefined();
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'z', ctrlKey: true, shiftKey: true }))).toBe(true);
    // without shift must not match
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'z', ctrlKey: true, shiftKey: false }))).toBe(false);
  });

  it('reEvaluate bind matches F5 keydown event', () => {
    const entry = getShortcut('reEvaluate');
    expect(entry?.bind).toBeDefined();
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'F5' }))).toBe(true);
  });

  it('toggleChat bind matches Ctrl+J keydown event', () => {
    const entry = getShortcut('toggleChat');
    expect(entry?.bind).toBeDefined();
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'j', ctrlKey: true }))).toBe(true);
    // Ctrl+Shift+J must not match
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'J', ctrlKey: true, shiftKey: true }))).toBe(false);
  });

  it('reload bind matches Ctrl+Shift+R keydown event (case-insensitive)', () => {
    const entry = getShortcut('reload');
    expect(entry?.bind).toBeDefined();
    // uppercase R
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'R', ctrlKey: true, shiftKey: true }))).toBe(true);
    // lowercase r
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'r', ctrlKey: true, shiftKey: true }))).toBe(true);
    // without shift
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'R', ctrlKey: true, shiftKey: false }))).toBe(false);
  });

  it('help bind matches bare ? keydown (no modifiers)', () => {
    const entry = getShortcut('help');
    expect(entry?.bind).toBeDefined();
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: '?' }))).toBe(true);
  });

  it('help bind rejects Ctrl+? keydown', () => {
    const entry = getShortcut('help');
    expect(entry?.bind).toBeDefined();
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: '?', ctrlKey: true }))).toBe(false);
  });

  it('fitToView has no bind field (empty key, no shortcut)', () => {
    const entry = getShortcut('fitToView');
    expect(entry?.bind).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// switchViewByIndex display-only shortcut entry (VM-6)
// ---------------------------------------------------------------------------

describe('shortcuts — switchViewByIndex entry', () => {
  it('SHORTCUTS registry contains a switchViewByIndex entry', () => {
    const entry = SHORTCUTS.find((s) => s.id === 'switchViewByIndex');
    expect(entry).toBeDefined();
  });

  it('switchViewByIndex entry has display key "1-9"', () => {
    const entry = SHORTCUTS.find((s) => s.id === 'switchViewByIndex');
    expect(entry?.key).toBe('1-9');
  });

  it('switchViewByIndex entry has category "View"', () => {
    const entry = SHORTCUTS.find((s) => s.id === 'switchViewByIndex');
    // category is a new optional field added to ShortcutDef in step-18
    expect((entry as ShortcutDef & { category?: string })?.category).toBe('View');
  });

  it('ShortcutId union includes "switchViewByIndex" — verified via getShortcut lookup', () => {
    // ShortcutId is derived from _SHORTCUTS_DEF; once 'switchViewByIndex' is added
    // the literal type flows into ShortcutId and getShortcut accepts it without cast.
    const entry = getShortcut('switchViewByIndex');
    expect(entry).toBeDefined();
  });
});

// ---------------------------------------------------------------------------
// 'new' shortcut entry (task-3209)
// ---------------------------------------------------------------------------

describe('shortcuts — new entry', () => {
  it('SHORTCUTS registry contains a new entry', () => {
    const entry = SHORTCUTS.find((s) => s.id === 'new');
    expect(entry).toBeDefined();
  });

  it('new entry has key "Ctrl+N" and non-empty description', () => {
    const entry = SHORTCUTS.find((s) => s.id === 'new');
    expect(entry?.key).toBe('Ctrl+N');
    expect(entry?.description).toBeTruthy();
  });

  it('new entry is not disabled', () => {
    const entry = SHORTCUTS.find((s) => s.id === 'new');
    expect(entry?.disabled).not.toBe(true);
  });

  it('new bind matches Ctrl+N keydown event', () => {
    const entry = SHORTCUTS.find((s) => s.id === 'new');
    expect(entry?.bind).toBeDefined();
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'n', ctrlKey: true }))).toBe(true);
  });

  it('new bind does NOT match Ctrl+Shift+N (shift: false convention)', () => {
    const entry = SHORTCUTS.find((s) => s.id === 'new');
    expect(entry?.bind).toBeDefined();
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'N', ctrlKey: true, shiftKey: true }))).toBe(false);
  });

  it('new bind does NOT match bare N without Ctrl', () => {
    const entry = SHORTCUTS.find((s) => s.id === 'new');
    expect(entry?.bind).toBeDefined();
    expect(matchesEvent(entry!.bind!, new KeyboardEvent('keydown', { key: 'n', ctrlKey: false }))).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Fold shortcut entries (task-4205)
// ---------------------------------------------------------------------------

describe('shortcuts — fold entries', () => {
  it('SHORTCUTS registry contains foldAll entry', () => {
    expect(SHORTCUTS.find((s) => s.id === 'foldAll')).toBeDefined();
  });

  it('SHORTCUTS registry contains unfoldAll entry', () => {
    expect(SHORTCUTS.find((s) => s.id === 'unfoldAll')).toBeDefined();
  });

  it('SHORTCUTS registry contains fold entry', () => {
    expect(SHORTCUTS.find((s) => s.id === 'fold')).toBeDefined();
  });

  it('SHORTCUTS registry contains unfold entry', () => {
    expect(SHORTCUTS.find((s) => s.id === 'unfold')).toBeDefined();
  });

  it('foldAll entry has key "Ctrl+Alt+[" and truthy description', () => {
    const entry = SHORTCUTS.find((s) => s.id === 'foldAll');
    expect(entry?.key).toBe('Ctrl+Alt+[');
    expect(entry?.description).toBeTruthy();
  });

  it('unfoldAll entry has key "Ctrl+Alt+]" and truthy description', () => {
    const entry = SHORTCUTS.find((s) => s.id === 'unfoldAll');
    expect(entry?.key).toBe('Ctrl+Alt+]');
    expect(entry?.description).toBeTruthy();
  });

  it('fold entry has key "Ctrl+Shift+[" and truthy description', () => {
    const entry = SHORTCUTS.find((s) => s.id === 'fold');
    expect(entry?.key).toBe('Ctrl+Shift+[');
    expect(entry?.description).toBeTruthy();
  });

  it('unfold entry has key "Ctrl+Shift+]" and truthy description', () => {
    const entry = SHORTCUTS.find((s) => s.id === 'unfold');
    expect(entry?.key).toBe('Ctrl+Shift+]');
    expect(entry?.description).toBeTruthy();
  });

  it('all four fold entries are not disabled', () => {
    for (const id of ['fold', 'unfold', 'foldAll', 'unfoldAll'] as const) {
      const entry = SHORTCUTS.find((s) => s.id === id);
      expect(entry?.disabled, `${id} should not be disabled`).not.toBe(true);
    }
  });

  it('all four fold entries have category "Editor"', () => {
    for (const id of ['fold', 'unfold', 'foldAll', 'unfoldAll'] as const) {
      const entry = SHORTCUTS.find((s) => s.id === id);
      expect((entry as ShortcutDef & { category?: string })?.category, `${id} should have category Editor`).toBe('Editor');
    }
  });

  it('getShortcut("foldAll") is defined (id flows into ShortcutId union)', () => {
    // ShortcutId is derived from _SHORTCUTS_DEF literal ids; this line would fail
    // to compile (ts-expect-error) if the id were not in the union.
    expect(getShortcut('foldAll')).toBeDefined();
  });
});

describe('shortcuts.ts source documentation', () => {
  it('does not contain brittle KeyboardHelp.tsx: file:line reference', () => {
    expect(SRC).not.toContain('KeyboardHelp.tsx:');
  });

  it('does not contain brittle useKeyboardShortcuts.ts: file:line reference', () => {
    expect(SRC).not.toContain('useKeyboardShortcuts.ts:');
  });

  it('does not contain brittle shortcuts.test.ts: file:line reference', () => {
    expect(SRC).not.toContain('shortcuts.test.ts:');
  });

  it('contains no "Filename.ext:N" file:line patterns anywhere', () => {
    expect(SRC).not.toMatch(/\b\w+\.tsx?:\d+\b/);
  });

  it('comment block immediately before _SHORTCUTS_DEF is at most 5 lines', () => {
    const defIdx = SRC.indexOf('\nconst _SHORTCUTS_DEF');
    const before = SRC.slice(0, defIdx);
    const lines = before.split('\n');
    let commentCount = 0;
    for (let i = lines.length - 1; i >= 0; i--) {
      if (lines[i].trim().startsWith('//')) {
        commentCount++;
      } else {
        break;
      }
    }
    expect(commentCount, 'comment block before _SHORTCUTS_DEF exceeds 5 lines').toBeLessThanOrEqual(5);
  });
});
