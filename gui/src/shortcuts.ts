/**
 * Centralized keyboard shortcut registry.
 *
 * This is the single source of truth for shortcut definitions consumed by
 * both MenuBar (for hotkey annotations) and KeyboardHelp (for the overlay).
 */

/**
 * Structured keyboard binding with tri-state modifier fields.
 * - true  → modifier MUST be held
 * - false → modifier MUST NOT be held
 * - undefined → don't care (modifier state is ignored)
 *
 * Note: `ctrl` checks `event.ctrlKey` only. For macOS Cmd-key support, use
 * `meta: true` (checks `event.metaKey`). Combining both allows cross-platform
 * bindings without conflating the two modifier keys.
 */
export interface KeyBinding {
  /** The KeyboardEvent.key value to match */
  key: string;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  /** Tri-state for the Meta/Cmd key (event.metaKey). Useful for macOS bindings. */
  meta?: boolean;
}

/**
 * Returns true if the given KeyboardEvent matches the binding.
 *
 * Key comparison is case-insensitive for single-character keys.
 * Modifier fields use tri-state semantics: true/false enforce the state,
 * undefined means don't care.
 */
export function matchesEvent(bind: KeyBinding, event: KeyboardEvent): boolean {
  const isSingleChar = bind.key.length === 1;
  const bindKey = isSingleChar ? bind.key.toLowerCase() : bind.key;
  const eventKey = event.key.length === 1 ? event.key.toLowerCase() : event.key;
  if (bindKey !== eventKey) return false;
  if (bind.ctrl !== undefined && bind.ctrl !== event.ctrlKey) return false;
  if (bind.shift !== undefined && bind.shift !== event.shiftKey) return false;
  if (bind.alt !== undefined && bind.alt !== event.altKey) return false;
  if (bind.meta !== undefined && bind.meta !== event.metaKey) return false;
  return true;
}

export interface ShortcutDef {
  /** Unique identifier for the shortcut */
  id: string;
  /** Display key combination (e.g. "Ctrl+O", "F5", "?"). Empty string if no shortcut. */
  key: string;
  /** Human-readable description of the action */
  description: string;
  /** Optional grouping category shown in the keyboard-help dialog (e.g. "View", "File"). */
  category?: string;
  /** When true, the shortcut is defined but not currently functional */
  disabled?: boolean;
  /** Structured binding for event matching. Absent when there is no keyboard shortcut. */
  bind?: KeyBinding;
}

// Private definition uses `as const satisfies` to validate shape against ShortcutDef
// while preserving literal `id` types for ShortcutId derivation below. SHORTCUTS is
// exported as `readonly (ShortcutDef & { id: ShortcutId })[]`: the intersection narrows
// `.id` to the literal union while keeping optional fields (.disabled, .bind) accessible.
const _SHORTCUTS_DEF = [
  // shift: false on Ctrl-only bindings prevents them from firing on Ctrl+Shift+<letter>,
  // which produces an uppercase key that the case-insensitive comparison would otherwise
  // accept. This restores the behavior of the original per-key equality checks.
  { id: 'new',        key: 'Ctrl+N',       description: 'New file',             bind: { key: 'n', ctrl: true, shift: false } },
  { id: 'open',       key: 'Ctrl+O',       description: 'Open file',            bind: { key: 'o', ctrl: true, shift: false } },
  { id: 'save',       key: 'Ctrl+S',       description: 'Save file',            bind: { key: 's', ctrl: true, shift: false } },
  { id: 'export',     key: 'Ctrl+E',       description: 'Export',               bind: { key: 'e', ctrl: true, shift: false } },
  // shift: false makes the undo intent explicit and prevents it from shadowing redo
  // (which requires shift: true) if a callback were ever wired to undo in ID_TO_CALLBACK.
  { id: 'undo',       key: 'Ctrl+Z',       description: 'Undo',                 disabled: true, bind: { key: 'z', ctrl: true, shift: false } },
  { id: 'redo',       key: 'Ctrl+Shift+Z', description: 'Redo',                 disabled: true, bind: { key: 'z', ctrl: true, shift: true } },
  { id: 'reEvaluate', key: 'F5',           description: 'Re-evaluate',          bind: { key: 'F5' } },
  { id: 'fitToView',  key: '',             description: 'Fit to view' },
  { id: 'toggleChat', key: 'Ctrl+J',       description: 'Toggle chat panel',    bind: { key: 'j', ctrl: true, shift: false } },
  { id: 'reload',     key: 'Ctrl+Shift+R', description: 'Reload changed files', bind: { key: 'r', ctrl: true, shift: true } },
  { id: 'help',       key: '?',            description: 'Toggle this help',     bind: { key: '?', ctrl: false, alt: false } },
  // Display-only entry for the 1–9 view-switch shortcut.  No `bind` field: the "1-9" key
  // is a descriptive range shown in the help dialog, not a literal binding matched by
  // matchesEvent.  Actual dispatch is a special-case block in useKeyboardShortcuts
  // (mirroring the Escape handler pattern).
  { id: 'switchViewByIndex', key: '1-9', description: 'Switch to view N in the view selector', category: 'View' },
  // Display-only entries for CodeMirror structural folding.  No `bind` field: dispatch
  // is owned by the editor's foldKeymap (keymap.of(foldKeymap) in Editor.tsx).
  // useKeyboardShortcuts skips entries without a bind, and also bails when the event
  // target is contentEditable (the CM editor contentDOM), so fold keys in the editor
  // never reach the global handler.  These entries exist solely to surface the
  // keybindings in the ? overlay.
  // Platform note: key labels below reflect CM6 foldKeymap defaults for Linux/Windows.
  // On macOS CM6 overrides fold/unfold to Cmd-Alt-[ / Cmd-Alt-] (foldAll/unfoldAll
  // remain Ctrl-Alt-[ / Ctrl-Alt-] on all platforms).  If macOS support is added,
  // these display strings should be made platform-aware (Cmd vs Ctrl for fold/unfold).
  { id: 'fold',      key: 'Ctrl+Shift+[', description: 'Fold block at cursor', category: 'Editor' },
  { id: 'unfold',    key: 'Ctrl+Shift+]', description: 'Unfold block at cursor', category: 'Editor' },
  { id: 'foldAll',   key: 'Ctrl+Alt+[',   description: 'Fold all', category: 'Editor' },
  { id: 'unfoldAll', key: 'Ctrl+Alt+]',   description: 'Unfold all', category: 'Editor' },
] as const satisfies readonly ShortcutDef[];

/**
 * Union of all valid shortcut IDs, derived from the shortcut definitions so the two
 * cannot drift. Consumed by useKeyboardShortcuts to give compile-time safety to its
 * ID→callback map.
 */
export type ShortcutId = typeof _SHORTCUTS_DEF[number]['id'];

export const SHORTCUTS: readonly (ShortcutDef & { id: ShortcutId })[] = _SHORTCUTS_DEF;

/**
 * Look up a shortcut definition by id.
 * Returns `undefined` if no entry with that id exists.
 */
export function getShortcut(id: ShortcutId): ShortcutDef | undefined {
  return SHORTCUTS.find((s) => s.id === id);
}

/**
 * Returns the display key string for a shortcut id.
 * Returns an empty string if the id is not found or has no key.
 */
export function shortcutKey(id: ShortcutId): string {
  return getShortcut(id)?.key ?? '';
}
