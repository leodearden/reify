import { describe, it, expect, vi, afterEach } from 'vitest';
import { createRoot } from 'solid-js';
import { useKeyboardShortcuts, hasCallbackWiring, paletteCommands, runCommand } from '../hooks/useKeyboardShortcuts';
import { SHORTCUTS } from '../shortcuts';

describe('useKeyboardShortcuts', () => {
  let dispose: () => void;

  afterEach(() => {
    dispose?.();
  });

  it('dispatching Ctrl+O keydown on document calls onOpen callback', () => {
    const onOpen = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onOpen });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'o', ctrlKey: true, bubbles: true }),
    );
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it('dispatching F5 keydown calls onReEvaluate', () => {
    const onReEvaluate = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onReEvaluate });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F5', bubbles: true }),
    );
    expect(onReEvaluate).toHaveBeenCalledTimes(1);
  });

  it('dispatching Ctrl+E keydown calls onExportDialog', () => {
    const onExportDialog = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onExportDialog });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'e', ctrlKey: true, bubbles: true }),
    );
    expect(onExportDialog).toHaveBeenCalledTimes(1);
  });

  it('dispatching Ctrl+O when target is a contenteditable element does NOT call onOpen', () => {
    // JSDOM does not fully implement `isContentEditable` for programmatically created
    // contenteditable divs (contentEditable = 'true' does not flip isContentEditable
    // in JSDOM's non-rendering context). Use Object.defineProperty to simulate what
    // a real browser returns so this test exercises the hook's isContentEditable guard.
    const onOpen = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onOpen });
      return d;
    });

    const div = document.createElement('div');
    div.contentEditable = 'true';
    Object.defineProperty(div, 'isContentEditable', { get: () => true, configurable: true });
    document.body.appendChild(div);
    try {
      div.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'o', ctrlKey: true, bubbles: true }),
      );
      expect(onOpen).not.toHaveBeenCalled();
    } finally {
      document.body.removeChild(div);
    }
  });

  it('dispatching Ctrl+O when target is an <input> does NOT call onOpen', () => {
    const onOpen = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onOpen });
      return d;
    });

    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      input.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'o', ctrlKey: true, bubbles: true }),
      );
      expect(onOpen).not.toHaveBeenCalled();
    } finally {
      document.body.removeChild(input);
    }
  });

  it('dispatching ? key calls onHelp callback', () => {
    const onHelp = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onHelp });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: '?', bubbles: true }),
    );
    expect(onHelp).toHaveBeenCalledTimes(1);
  });

  it('dispatching ? when target is an <input> does NOT call onHelp', () => {
    const onHelp = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onHelp });
      return d;
    });

    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      input.dispatchEvent(
        new KeyboardEvent('keydown', { key: '?', bubbles: true }),
      );
      expect(onHelp).not.toHaveBeenCalled();
    } finally {
      document.body.removeChild(input);
    }
  });

  it('dispatching ? with Ctrl held does NOT call onHelp', () => {
    const onHelp = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onHelp });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: '?', ctrlKey: true, bubbles: true }),
    );
    expect(onHelp).not.toHaveBeenCalled();
  });

  it('after cleanup, dispatching shortcuts does nothing (listeners removed)', () => {
    const onOpen = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onOpen });
      return d;
    });

    // Dispose (triggers onCleanup)
    dispose();

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'o', ctrlKey: true, bubbles: true }),
    );
    expect(onOpen).not.toHaveBeenCalled();
  });

  it('dispatching Ctrl+Shift+R calls onReloadShortcut', () => {
    const onReloadShortcut = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onReloadShortcut });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'R', ctrlKey: true, shiftKey: true, bubbles: true }),
    );
    expect(onReloadShortcut).toHaveBeenCalledTimes(1);
  });

  it('dispatching Escape calls onDismissReload', () => {
    const onDismissReload = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onDismissReload });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }),
    );
    expect(onDismissReload).toHaveBeenCalledTimes(1);
  });

  it('dispatching Ctrl+J calls onToggleChatPanel callback', () => {
    const onToggleChatPanel = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onToggleChatPanel });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'j', ctrlKey: true, bubbles: true }),
    );
    expect(onToggleChatPanel).toHaveBeenCalledTimes(1);
  });

  it('Ctrl+J is skipped when target is an input element', () => {
    const onToggleChatPanel = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onToggleChatPanel });
      return d;
    });

    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      input.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'j', ctrlKey: true, bubbles: true }),
      );
      expect(onToggleChatPanel).not.toHaveBeenCalled();
    } finally {
      document.body.removeChild(input);
    }
  });

  it('Ctrl+J is skipped when target is a textarea element', () => {
    const onToggleChatPanel = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onToggleChatPanel });
      return d;
    });

    const textarea = document.createElement('textarea');
    document.body.appendChild(textarea);
    try {
      textarea.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'j', ctrlKey: true, bubbles: true }),
      );
      expect(onToggleChatPanel).not.toHaveBeenCalled();
    } finally {
      document.body.removeChild(textarea);
    }
  });

  it('Ctrl+Shift+R is skipped when target is an input element', () => {
    const onReloadShortcut = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onReloadShortcut });
      return d;
    });

    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      input.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'R', ctrlKey: true, shiftKey: true, bubbles: true }),
      );
      expect(onReloadShortcut).not.toHaveBeenCalled();
    } finally {
      document.body.removeChild(input);
    }
  });

  it('dispatching Ctrl+S keydown on document calls onSave callback', () => {
    const onSave = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onSave });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 's', ctrlKey: true, bubbles: true }),
    );
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it('Ctrl+S is skipped when target is an input element', () => {
    const onSave = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onSave });
      return d;
    });

    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      input.dispatchEvent(
        new KeyboardEvent('keydown', { key: 's', ctrlKey: true, bubbles: true }),
      );
      expect(onSave).not.toHaveBeenCalled();
    } finally {
      document.body.removeChild(input);
    }
  });

  it('dispatching Ctrl+N keydown on document calls onNew callback', () => {
    const onNew = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onNew });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true }),
    );
    expect(onNew).toHaveBeenCalledTimes(1);
  });

  it('Ctrl+N is skipped when target is an <input>', () => {
    const onNew = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onNew });
      return d;
    });

    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      input.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true }),
      );
      expect(onNew).not.toHaveBeenCalled();
    } finally {
      document.body.removeChild(input);
    }
  });

  it('dispatching Escape with onClearSelection provided invokes it', () => {
    const onClearSelection = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onClearSelection });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }),
    );
    expect(onClearSelection).toHaveBeenCalledTimes(1);
  });

  it('Escape invokes onDismissReload first then onClearSelection in sequence', () => {
    const callOrder: string[] = [];
    const onDismissReload = vi.fn(() => callOrder.push('dismiss'));
    const onClearSelection = vi.fn(() => callOrder.push('clear'));
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onDismissReload, onClearSelection });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }),
    );
    expect(onDismissReload).toHaveBeenCalledTimes(1);
    expect(onClearSelection).toHaveBeenCalledTimes(1);
    expect(callOrder).toEqual(['dismiss', 'clear']);
  });

  it('Escape in an input element does NOT invoke onClearSelection', () => {
    const onClearSelection = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onClearSelection });
      return d;
    });

    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      input.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }),
      );
      expect(onClearSelection).not.toHaveBeenCalled();
    } finally {
      document.body.removeChild(input);
    }
  });

  it('Escape in a textarea element does NOT invoke onClearSelection', () => {
    const onClearSelection = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onClearSelection });
      return d;
    });

    const textarea = document.createElement('textarea');
    document.body.appendChild(textarea);
    try {
      textarea.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }),
      );
      expect(onClearSelection).not.toHaveBeenCalled();
    } finally {
      document.body.removeChild(textarea);
    }
  });
});

describe('useKeyboardShortcuts — onSwitchViewByIndex number-key dispatch (VM-6)', () => {
  let dispose: () => void;

  afterEach(() => {
    dispose?.();
  });

  it('pressing "1" calls onSwitchViewByIndex with index 0', () => {
    const onSwitchViewByIndex = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onSwitchViewByIndex });
      return d;
    });

    document.dispatchEvent(new KeyboardEvent('keydown', { key: '1', bubbles: true }));
    expect(onSwitchViewByIndex).toHaveBeenCalledOnce();
    expect(onSwitchViewByIndex).toHaveBeenCalledWith(0);
  });

  it('pressing "5" calls onSwitchViewByIndex with index 4', () => {
    const onSwitchViewByIndex = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onSwitchViewByIndex });
      return d;
    });

    document.dispatchEvent(new KeyboardEvent('keydown', { key: '5', bubbles: true }));
    expect(onSwitchViewByIndex).toHaveBeenCalledOnce();
    expect(onSwitchViewByIndex).toHaveBeenCalledWith(4);
  });

  it('pressing "9" calls onSwitchViewByIndex with index 8', () => {
    const onSwitchViewByIndex = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onSwitchViewByIndex });
      return d;
    });

    document.dispatchEvent(new KeyboardEvent('keydown', { key: '9', bubbles: true }));
    expect(onSwitchViewByIndex).toHaveBeenCalledOnce();
    expect(onSwitchViewByIndex).toHaveBeenCalledWith(8);
  });

  it('Ctrl+1 does NOT call onSwitchViewByIndex (modifier guard)', () => {
    const onSwitchViewByIndex = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onSwitchViewByIndex });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: '1', ctrlKey: true, bubbles: true }),
    );
    expect(onSwitchViewByIndex).not.toHaveBeenCalled();
  });

  it('Shift+1 does NOT call onSwitchViewByIndex (modifier guard)', () => {
    const onSwitchViewByIndex = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onSwitchViewByIndex });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: '1', shiftKey: true, bubbles: true }),
    );
    expect(onSwitchViewByIndex).not.toHaveBeenCalled();
  });

  it('Alt+1 does NOT call onSwitchViewByIndex (modifier guard)', () => {
    const onSwitchViewByIndex = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onSwitchViewByIndex });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: '1', altKey: true, bubbles: true }),
    );
    expect(onSwitchViewByIndex).not.toHaveBeenCalled();
  });

  it('Meta+1 does NOT call onSwitchViewByIndex (modifier guard)', () => {
    const onSwitchViewByIndex = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onSwitchViewByIndex });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: '1', metaKey: true, bubbles: true }),
    );
    expect(onSwitchViewByIndex).not.toHaveBeenCalled();
  });

  it('number key in an <input> does NOT call onSwitchViewByIndex (isTypingContext guard)', () => {
    const onSwitchViewByIndex = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onSwitchViewByIndex });
      return d;
    });

    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      input.dispatchEvent(new KeyboardEvent('keydown', { key: '1', bubbles: true }));
      expect(onSwitchViewByIndex).not.toHaveBeenCalled();
    } finally {
      document.body.removeChild(input);
    }
  });

  it('number key in a <textarea> does NOT call onSwitchViewByIndex (isTypingContext guard)', () => {
    const onSwitchViewByIndex = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onSwitchViewByIndex });
      return d;
    });

    const textarea = document.createElement('textarea');
    document.body.appendChild(textarea);
    try {
      textarea.dispatchEvent(new KeyboardEvent('keydown', { key: '3', bubbles: true }));
      expect(onSwitchViewByIndex).not.toHaveBeenCalled();
    } finally {
      document.body.removeChild(textarea);
    }
  });
});

describe('hasCallbackWiring invariant', () => {
  it('every enabled shortcut with a bind has a callback wiring', () => {
    // Derive expected set directly from SHORTCUTS: every shortcut with a bind
    // that is not disabled. This way the test self-updates when shortcuts gain
    // or lose their `disabled` flag rather than requiring a parallel list.
    // Shortcuts with no `bind` at all (e.g. fitToView) are excluded by the
    // first predicate.
    const expected = SHORTCUTS
      .filter((s) => s.bind !== undefined && !s.disabled)
      .map((s) => s.id)
      .sort();
    const actual = SHORTCUTS.filter((s) => hasCallbackWiring(s.id)).map((s) => s.id).sort();
    expect(actual).toEqual(expected);
  });

  it('every wired shortcut has a bind', () => {
    for (const s of SHORTCUTS) {
      if (!hasCallbackWiring(s.id)) continue;
      expect(s.bind, `wired shortcut "${s.id}" must have a bind`).toBeDefined();
    }
  });
});

// ---------------------------------------------------------------------------
// Command palette + symbol-jump shortcut dispatch (task-4208)
// ---------------------------------------------------------------------------

describe('useKeyboardShortcuts — command palette shortcuts', () => {
  let dispose: () => void;

  afterEach(() => {
    dispose?.();
  });

  it('Ctrl+Shift+P calls onCommandPalette', () => {
    const onCommandPalette = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onCommandPalette });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'p', ctrlKey: true, shiftKey: true, bubbles: true }),
    );
    expect(onCommandPalette).toHaveBeenCalledTimes(1);
  });

  it('Ctrl+Shift+O calls onSymbolJump', () => {
    const onSymbolJump = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onSymbolJump });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'o', ctrlKey: true, shiftKey: true, bubbles: true }),
    );
    expect(onSymbolJump).toHaveBeenCalledTimes(1);
  });

  it('Ctrl+Shift+P on an <input> STILL calls onCommandPalette (typing-context exempt)', () => {
    const onCommandPalette = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onCommandPalette });
      return d;
    });

    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      input.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'p', ctrlKey: true, shiftKey: true, bubbles: true }),
      );
      expect(onCommandPalette).toHaveBeenCalledTimes(1);
    } finally {
      document.body.removeChild(input);
    }
  });

  it('Ctrl+Shift+O on an <input> STILL calls onSymbolJump (typing-context exempt)', () => {
    const onSymbolJump = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onSymbolJump });
      return d;
    });

    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      input.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'o', ctrlKey: true, shiftKey: true, bubbles: true }),
      );
      expect(onSymbolJump).toHaveBeenCalledTimes(1);
    } finally {
      document.body.removeChild(input);
    }
  });

  it('Ctrl+Shift+P on a contentEditable element STILL calls onCommandPalette (typing-context exempt)', () => {
    const onCommandPalette = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onCommandPalette });
      return d;
    });

    const div = document.createElement('div');
    div.contentEditable = 'true';
    Object.defineProperty(div, 'isContentEditable', { get: () => true, configurable: true });
    document.body.appendChild(div);
    try {
      div.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'p', ctrlKey: true, shiftKey: true, bubbles: true }),
      );
      expect(onCommandPalette).toHaveBeenCalledTimes(1);
    } finally {
      document.body.removeChild(div);
    }
  });

  it('Ctrl+O on an <input> does NOT call onOpen (regression guard: non-palette shortcuts stay guarded)', () => {
    const onOpen = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onOpen });
      return d;
    });

    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      input.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'o', ctrlKey: true, bubbles: true }),
      );
      expect(onOpen).not.toHaveBeenCalled();
    } finally {
      document.body.removeChild(input);
    }
  });
});

// ---------------------------------------------------------------------------
// paletteCommands() and runCommand() (task-4208)
// ---------------------------------------------------------------------------

describe('paletteCommands', () => {
  it('returns an array of objects with id, title, and key fields', () => {
    const cmds = paletteCommands();
    expect(Array.isArray(cmds)).toBe(true);
    for (const cmd of cmds) {
      expect(typeof cmd.id).toBe('string');
      expect(typeof cmd.title).toBe('string');
      expect(typeof cmd.key).toBe('string');
    }
  });

  it('includes the save command', () => {
    const cmds = paletteCommands();
    expect(cmds.some((c) => c.id === 'save')).toBe(true);
  });

  it('includes the open command', () => {
    const cmds = paletteCommands();
    expect(cmds.some((c) => c.id === 'open')).toBe(true);
  });

  it('excludes commandPalette and symbolJump from the list (palette-control ids)', () => {
    const cmds = paletteCommands();
    expect(cmds.some((c) => c.id === 'commandPalette')).toBe(false);
    expect(cmds.some((c) => c.id === 'symbolJump')).toBe(false);
  });

  it('excludes disabled shortcuts (undo, redo)', () => {
    const cmds = paletteCommands();
    expect(cmds.some((c) => c.id === 'undo')).toBe(false);
    expect(cmds.some((c) => c.id === 'redo')).toBe(false);
  });

  it('excludes entries with no bind (fitToView)', () => {
    const cmds = paletteCommands();
    expect(cmds.some((c) => c.id === 'fitToView')).toBe(false);
  });
});

describe('runCommand', () => {
  it('runCommand("save", {onSave}) invokes onSave once', () => {
    const onSave = vi.fn();
    runCommand('save', { onSave });
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it('runCommand for an unwired id is a no-op', () => {
    // 'fitToView' has no bind and no wiring
    const spy = vi.fn();
    expect(() => runCommand('fitToView', { onSave: spy })).not.toThrow();
    expect(spy).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// toggleDiagnostics keyboard shortcut dispatch (task-4401)
// ---------------------------------------------------------------------------

describe('useKeyboardShortcuts — toggleDiagnostics shortcut', () => {
  let dispose: () => void;

  afterEach(() => {
    dispose?.();
  });

  it('dispatching Ctrl+Shift+M calls onToggleDiagnostics callback', () => {
    const onToggleDiagnostics = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onToggleDiagnostics });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'm', ctrlKey: true, shiftKey: true, bubbles: true }),
    );
    expect(onToggleDiagnostics).toHaveBeenCalledTimes(1);
  });

  it('Ctrl+M (without shift) does NOT call onToggleDiagnostics', () => {
    const onToggleDiagnostics = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onToggleDiagnostics });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'm', ctrlKey: true, shiftKey: false, bubbles: true }),
    );
    expect(onToggleDiagnostics).not.toHaveBeenCalled();
  });
});
