import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createRoot } from 'solid-js';
import { createToast } from '../hooks/useToast';

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('createToast', () => {
  it('showToast sets message and type signals', () => {
    createRoot((dispose) => {
      const toast = createToast();
      expect(toast.toastMessage()).toBeNull();
      expect(toast.toastType()).toBe('info');

      toast.showToast('Hello', 'success');

      expect(toast.toastMessage()).toBe('Hello');
      expect(toast.toastType()).toBe('success');
      dispose();
    });
  });

  it('dismissToast clears message to null', () => {
    createRoot((dispose) => {
      const toast = createToast();
      toast.showToast('Error occurred', 'error');
      expect(toast.toastMessage()).toBe('Error occurred');

      toast.dismissToast();
      expect(toast.toastMessage()).toBeNull();
      dispose();
    });
  });

  it('auto-dismiss fires after 5000ms for success type', () => {
    createRoot((dispose) => {
      const toast = createToast();
      toast.showToast('Saved!', 'success');

      // Still visible before 5000ms
      vi.advanceTimersByTime(4999);
      expect(toast.toastMessage()).toBe('Saved!');

      // Dismissed at 5000ms
      vi.advanceTimersByTime(1);
      expect(toast.toastMessage()).toBeNull();
      dispose();
    });
  });

  it('auto-dismiss fires after 10000ms for error type', () => {
    createRoot((dispose) => {
      const toast = createToast();
      toast.showToast('Something failed', 'error');

      // Still visible before 10000ms
      vi.advanceTimersByTime(9999);
      expect(toast.toastMessage()).toBe('Something failed');

      // Dismissed at 10000ms
      vi.advanceTimersByTime(1);
      expect(toast.toastMessage()).toBeNull();
      dispose();
    });
  });

  it('auto-dismiss fires after 5000ms for info type (default)', () => {
    createRoot((dispose) => {
      const toast = createToast();
      toast.showToast('FYI message');

      vi.advanceTimersByTime(5000);
      expect(toast.toastMessage()).toBeNull();
      dispose();
    });
  });

  it('calling showToast again resets the auto-dismiss timer', () => {
    createRoot((dispose) => {
      const toast = createToast();
      toast.showToast('First', 'success');

      // Advance 3000ms then show new toast
      vi.advanceTimersByTime(3000);
      toast.showToast('Second', 'success');

      // After 3000ms more (6000ms total from first, 3000ms from second),
      // should still be visible since second timer has 5000ms
      vi.advanceTimersByTime(3000);
      expect(toast.toastMessage()).toBe('Second');

      // After 2000ms more (5000ms from second), should auto-dismiss
      vi.advanceTimersByTime(2000);
      expect(toast.toastMessage()).toBeNull();
      dispose();
    });
  });

  it('dismissToast cancels pending auto-dismiss timer', () => {
    createRoot((dispose) => {
      const toast = createToast();
      toast.showToast('Test', 'success');

      // Manually dismiss
      toast.dismissToast();
      expect(toast.toastMessage()).toBeNull();

      // Advance past auto-dismiss time — should not throw or set message
      vi.advanceTimersByTime(10000);
      expect(toast.toastMessage()).toBeNull();
      dispose();
    });
  });
});
