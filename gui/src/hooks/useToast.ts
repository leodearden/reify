/**
 * createToast — centralizes toast message state with auto-dismiss timers.
 * Factory function following the SolidJS createX convention.
 */
import { createSignal } from 'solid-js';

export type ToastType = 'success' | 'error' | 'info';

const AUTO_DISMISS_MS: Record<ToastType, number> = {
  success: 5000,
  info: 5000,
  error: 10000,
};

export function createToast() {
  const [toastMessage, setToastMessage] = createSignal<string | null>(null);
  const [toastType, setToastType] = createSignal<ToastType>('info');
  let dismissTimer: ReturnType<typeof setTimeout> | undefined;

  function showToast(message: string, type: ToastType = 'info') {
    // Clear any pending auto-dismiss timer
    if (dismissTimer !== undefined) {
      clearTimeout(dismissTimer);
      dismissTimer = undefined;
    }

    setToastMessage(message);
    setToastType(type);

    // Start auto-dismiss timer
    dismissTimer = setTimeout(() => {
      setToastMessage(null);
      dismissTimer = undefined;
    }, AUTO_DISMISS_MS[type]);
  }

  function dismissToast() {
    if (dismissTimer !== undefined) {
      clearTimeout(dismissTimer);
      dismissTimer = undefined;
    }
    setToastMessage(null);
  }

  return {
    toastMessage,
    toastType,
    showToast,
    dismissToast,
  };
}
