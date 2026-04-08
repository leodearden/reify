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
});
