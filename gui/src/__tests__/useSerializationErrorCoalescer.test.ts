import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createSerializationErrorCoalescer } from '../hooks/useSerializationErrorCoalescer';

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('createSerializationErrorCoalescer', () => {
  it('emits a detailed toast for a single error after the 500ms debounce window', () => {
    const showToast = vi.fn();
    const coalescer = createSerializationErrorCoalescer(showToast);

    coalescer.add({ item_type: 'mesh', item_id: 'Bracket.body', error: 'non-finite f32 value' });

    // Toast has NOT fired yet
    expect(showToast).not.toHaveBeenCalled();

    // Advance past the debounce window
    vi.advanceTimersByTime(500);

    expect(showToast).toHaveBeenCalledOnce();
    expect(showToast).toHaveBeenCalledWith(
      "Failed to serialize mesh 'Bracket.body': non-finite f32 value",
      'error',
    );
  });

  it('emits a single summary toast for multiple distinct errors after the window', () => {
    const showToast = vi.fn();
    const coalescer = createSerializationErrorCoalescer(showToast);

    coalescer.add({ item_type: 'mesh', item_id: 'Bracket.body', error: 'non-finite f32 value' });
    coalescer.add({ item_type: 'value', item_id: 'param.width', error: 'Infinity' });
    coalescer.add({ item_type: 'constraint', item_id: 'c42', error: 'NaN distance' });

    // No toast yet
    expect(showToast).not.toHaveBeenCalled();

    vi.advanceTimersByTime(500);

    // Exactly one call with the summary message
    expect(showToast).toHaveBeenCalledOnce();
    expect(showToast).toHaveBeenCalledWith('3 items failed to serialize', 'error');
  });

  it('deduplicates errors for the same (item_type, item_id) — single detailed toast', () => {
    const showToast = vi.fn();
    const coalescer = createSerializationErrorCoalescer(showToast);

    // Send 5 errors for the same mesh
    for (let i = 0; i < 5; i++) {
      coalescer.add({ item_type: 'mesh', item_id: 'Bracket.body', error: `error variant ${i}` });
    }

    vi.advanceTimersByTime(500);

    // Only one unique item → detailed toast, not a summary
    expect(showToast).toHaveBeenCalledOnce();
    expect(showToast).toHaveBeenCalledWith(
      expect.stringContaining("Failed to serialize mesh 'Bracket.body'"),
      'error',
    );
  });
});
