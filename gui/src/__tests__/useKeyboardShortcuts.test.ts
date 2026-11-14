import { describe, it, expect, vi, afterEach, type Mock } from 'vitest';
import { createRoot, createSignal } from 'solid-js';
import {
  useKeyboardShortcuts,
  hasCallbackWiring,
  paletteCommands,
  runCommand,
  ID_TO_CALLBACK,
  type KeyboardShortcutCallbacks,
} from '../hooks/useKeyboardShortcuts';
import { SHORTCUTS, matchesEvent } from '../shortcuts';

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

  it('dispatching ? with Shift held (the Shift+/ keystroke) calls onHelp callback', () => {
    const onHelp = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onHelp });
      return d;
    });

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: '?', shiftKey: true, bubbles: true }),
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

describe('useKeyboardShortcuts — toggleEdges shortcut', () => {
  let dispose: () => void;

  afterEach(() => {
    dispose?.();
  });

  function wire() {
    const onToggleEdges = vi.fn();
    const onExportDialog = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onToggleEdges, onExportDialog });
      return d;
    });
    return { onToggleEdges, onExportDialog };
  }

  it('a bare e calls onToggleEdges once', () => {
    const { onToggleEdges } = wire();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'e', bubbles: true }));
    expect(onToggleEdges).toHaveBeenCalledTimes(1);
  });

  it('Ctrl+E stays Export and does not toggle edges', () => {
    const { onToggleEdges, onExportDialog } = wire();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'e', ctrlKey: true, bubbles: true }));
    expect(onExportDialog).toHaveBeenCalledTimes(1);
    expect(onToggleEdges).not.toHaveBeenCalled();
  });

  it('an e typed into an <input> does not toggle edges', () => {
    const { onToggleEdges } = wire();
    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'e', bubbles: true }));
      expect(onToggleEdges).not.toHaveBeenCalled();
    } finally {
      document.body.removeChild(input);
    }
  });

  it('an e pressed in a focused <select> neither toggles edges nor is swallowed', () => {
    const { onToggleEdges } = wire();
    const select = document.createElement('select');
    document.body.appendChild(select);
    try {
      const event = new KeyboardEvent('keydown', { key: 'e', bubbles: true, cancelable: true });
      select.dispatchEvent(event);
      expect(onToggleEdges).not.toHaveBeenCalled();
      expect(event.defaultPrevented).toBe(false);
    } finally {
      document.body.removeChild(select);
    }
  });

  it('a held E toggles once: its auto-repeat keydowns are swallowed', () => {
    const { onToggleEdges } = wire();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'e', bubbles: true }));
    const repeat = new KeyboardEvent('keydown', { key: 'e', repeat: true, bubbles: true, cancelable: true });
    document.dispatchEvent(repeat);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'e', repeat: true, bubbles: true }));

    expect(onToggleEdges).toHaveBeenCalledTimes(1);
    expect(repeat.defaultPrevented).toBe(true);
  });

  it('is offered in the command palette', () => {
    expect(paletteCommands().some((c) => c.id === 'toggleEdges')).toBe(true);
  });
});

describe('useKeyboardShortcuts — a focused <select> blocks only its type-ahead keys', () => {
  let dispose: () => void;
  let select: HTMLSelectElement;

  afterEach(() => {
    dispose?.();
    select?.remove();
  });

  function focusedSelect(callbacks: Parameters<typeof useKeyboardShortcuts>[0]) {
    dispose = createRoot((d) => {
      useKeyboardShortcuts(callbacks);
      return d;
    });
    select = document.createElement('select');
    document.body.appendChild(select);
    return select;
  }

  it('Ctrl+S still saves', () => {
    const onSave = vi.fn();
    focusedSelect({ onSave }).dispatchEvent(
      new KeyboardEvent('keydown', { key: 's', ctrlKey: true, bubbles: true }),
    );
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it('F5 still re-evaluates', () => {
    const onReEvaluate = vi.fn();
    focusedSelect({ onReEvaluate }).dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F5', bubbles: true }),
    );
    expect(onReEvaluate).toHaveBeenCalledTimes(1);
  });

  it('Escape still clears the selection', () => {
    const onClearSelection = vi.fn();
    focusedSelect({ onClearSelection }).dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }),
    );
    expect(onClearSelection).toHaveBeenCalledTimes(1);
  });

  it('a digit is type-ahead, not a view switch, and is not prevented', () => {
    const onSwitchViewByIndex = vi.fn();
    const event = new KeyboardEvent('keydown', { key: '2', bubbles: true, cancelable: true });
    focusedSelect({ onSwitchViewByIndex }).dispatchEvent(event);
    expect(onSwitchViewByIndex).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
  });

  it('a ? is type-ahead, not the help toggle, and is not prevented', () => {
    const onHelp = vi.fn();
    const event = new KeyboardEvent('keydown', { key: '?', shiftKey: true, bubbles: true, cancelable: true });
    focusedSelect({ onHelp }).dispatchEvent(event);
    expect(onHelp).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
  });
});

describe('useKeyboardShortcuts — toggle shortcuts ignore auto-repeat', () => {
  let dispose: () => void;

  afterEach(() => {
    dispose?.();
  });

  it('a held Ctrl+J toggles the chat panel once', () => {
    const onToggleChatPanel = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onToggleChatPanel });
      return d;
    });

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'j', ctrlKey: true, bubbles: true }));
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'j', ctrlKey: true, repeat: true, bubbles: true }));

    expect(onToggleChatPanel).toHaveBeenCalledTimes(1);
  });

  it('a held F5 still re-evaluates on every repeat (not a toggle)', () => {
    const onReEvaluate = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts({ onReEvaluate });
      return d;
    });

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F5', bubbles: true }));
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F5', repeat: true, bubbles: true }));

    expect(onReEvaluate).toHaveBeenCalledTimes(2);
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

describe('useKeyboardShortcuts — no document loaded (Start view)', () => {
  let dispose: () => void;

  afterEach(() => {
    dispose?.();
  });

  function wire(documentLoaded: () => boolean = () => false) {
    const callbacks = {} as Record<keyof KeyboardShortcutCallbacks, Mock>;
    for (const key of Object.values(ID_TO_CALLBACK)) callbacks[key] = vi.fn();
    callbacks.onDismissReload = vi.fn();
    callbacks.onClearSelection = vi.fn();
    callbacks.onSwitchViewByIndex = vi.fn();
    dispose = createRoot((d) => {
      useKeyboardShortcuts(callbacks, { documentLoaded });
      return d;
    });
    return callbacks;
  }

  it('Ctrl+N and Ctrl+O still reach onNew and onOpen', () => {
    const callbacks = wire();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true }));
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'o', ctrlKey: true, bubbles: true }));
    expect(callbacks.onNew).toHaveBeenCalledTimes(1);
    expect(callbacks.onOpen).toHaveBeenCalledTimes(1);
  });

  it('every other wired shortcut is swallowed: no callback runs and the default is prevented', () => {
    const callbacks = wire();
    const iterated: string[] = [];
    for (const s of SHORTCUTS) {
      if (!hasCallbackWiring(s.id) || s.worksWithoutDocument) continue;
      iterated.push(s.id);
      const bind = s.bind!;
      const event = new KeyboardEvent('keydown', {
        key: bind.key,
        ctrlKey: bind.ctrl ?? false,
        shiftKey: bind.shift ?? false,
        altKey: bind.alt ?? false,
        metaKey: bind.meta ?? false,
        bubbles: true,
        cancelable: true,
      });
      expect(matchesEvent(bind, event)).toBe(true);
      document.dispatchEvent(event);
      expect(callbacks[ID_TO_CALLBACK[s.id]!], s.id).not.toHaveBeenCalled();
      expect(event.defaultPrevented, s.id).toBe(true);
    }
    expect(callbacks.onNew).not.toHaveBeenCalled();
    expect(callbacks.onOpen).not.toHaveBeenCalled();
    expect(iterated).toEqual(
      expect.arrayContaining(['help', 'export', 'toggleEdges', 'toggleChat', 'commandPalette']),
    );
  });

  it('Escape and the 1-9 view keys do nothing', () => {
    const callbacks = wire();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    document.dispatchEvent(new KeyboardEvent('keydown', { key: '1', bubbles: true }));
    expect(callbacks.onDismissReload).not.toHaveBeenCalled();
    expect(callbacks.onClearSelection).not.toHaveBeenCalled();
    expect(callbacks.onSwitchViewByIndex).not.toHaveBeenCalled();
  });

  it('the document predicate is read on every keydown', () => {
    const [loaded, setLoaded] = createSignal(false);
    const callbacks = wire(loaded);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: '?', bubbles: true }));
    expect(callbacks.onHelp).not.toHaveBeenCalled();
    setLoaded(true);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: '?', bubbles: true }));
    expect(callbacks.onHelp).toHaveBeenCalledTimes(1);
  });

  it('a key typed into an <input> with no document still reaches the input', () => {
    const callbacks = wire();
    const input = document.createElement('input');
    document.body.appendChild(input);
    try {
      const event = new KeyboardEvent('keydown', { key: '?', bubbles: true, cancelable: true });
      input.dispatchEvent(event);
      expect(callbacks.onHelp).not.toHaveBeenCalled();
      expect(event.defaultPrevented).toBe(false);
    } finally {
      document.body.removeChild(input);
    }
  });
});
