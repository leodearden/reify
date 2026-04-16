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
 */
export interface KeyBinding {
  /** The KeyboardEvent.key value to match */
  key: string;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
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
  return true;
}

export interface ShortcutDef {
  /** Unique identifier for the shortcut */
  id: string;
  /** Display key combination (e.g. "Ctrl+O", "F5", "?"). Empty string if no shortcut. */
  key: string;
  /** Human-readable description of the action */
  description: string;
  /** Structured binding for event matching. Absent when there is no keyboard shortcut. */
  bind?: KeyBinding;
}

export const SHORTCUTS: ShortcutDef[] = [
  { id: 'open',       key: 'Ctrl+O',       description: 'Open file',            bind: { key: 'o', ctrl: true } },
  { id: 'save',       key: 'Ctrl+S',       description: 'Save file',            bind: { key: 's', ctrl: true } },
  { id: 'export',     key: 'Ctrl+E',       description: 'Export',               bind: { key: 'e', ctrl: true } },
  { id: 'undo',       key: 'Ctrl+Z',       description: 'Undo',                 bind: { key: 'z', ctrl: true } },
  { id: 'redo',       key: 'Ctrl+Shift+Z', description: 'Redo',                 bind: { key: 'z', ctrl: true, shift: true } },
  { id: 'reEvaluate', key: 'F5',           description: 'Re-evaluate',          bind: { key: 'F5' } },
  { id: 'fitToView',  key: '',             description: 'Fit to view' },
  { id: 'toggleChat', key: 'Ctrl+J',       description: 'Toggle chat panel',    bind: { key: 'j', ctrl: true } },
  { id: 'reload',     key: 'Ctrl+Shift+R', description: 'Reload changed files', bind: { key: 'r', ctrl: true, shift: true } },
  { id: 'help',       key: '?',            description: 'Toggle this help',     bind: { key: '?', ctrl: false, alt: false } },
];

/**
 * Look up a shortcut definition by id.
 * Returns `undefined` if no entry with that id exists.
 */
export function getShortcut(id: string): ShortcutDef | undefined {
  return SHORTCUTS.find((s) => s.id === id);
}

/**
 * Returns the display key string for a shortcut id.
 * Returns an empty string if the id is not found or has no key.
 */
export function shortcutKey(id: string): string {
  return getShortcut(id)?.key ?? '';
}
