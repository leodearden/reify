import { describe, it, expect, vi, afterEach } from 'vitest';
import { createRoot } from 'solid-js';
import { useKeyboardShortcuts, ID_TO_CALLBACK } from '../hooks/useKeyboardShortcuts';
import { SHORTCUTS, type ShortcutId } from '../shortcuts';

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

describe('ID_TO_CALLBACK wiring invariant', () => {
  it('every shortcut with a bind — minus known no-callback IDs — has an ID_TO_CALLBACK entry', () => {
    // Derive expected set directly from SHORTCUTS: every shortcut with a bind
    // that is not disabled. This way the test self-updates when shortcuts gain
    // or lose their `disabled` flag rather than requiring a parallel list.
    // Shortcuts with no `bind` at all (e.g. fitToView) are excluded by the
    // first predicate.
    const expected = SHORTCUTS
      .filter((s) => s.bind !== undefined && !s.disabled)
      .map((s) => s.id)
      .sort();
    const actual = Object.keys(ID_TO_CALLBACK).sort();
    expect(actual).toEqual(expected);
  });

  it('every ID_TO_CALLBACK key is a shortcut with a bind', () => {
    for (const id of Object.keys(ID_TO_CALLBACK) as ShortcutId[]) {
      const shortcut = SHORTCUTS.find((s) => s.id === id);
      expect(shortcut?.bind, `shortcut "${id}" should have a bind field`).toBeDefined();
    }
  });
});
