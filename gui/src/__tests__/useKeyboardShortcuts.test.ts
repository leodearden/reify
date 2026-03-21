import { describe, it, expect, vi, afterEach } from 'vitest';
import { createRoot } from 'solid-js';
import { useKeyboardShortcuts } from '../hooks/useKeyboardShortcuts';

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
});
