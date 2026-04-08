import type { SerializationError } from '../types';
import type { ToastMessage } from '../types';

type ShowToast = (message: string, type: ToastMessage['type']) => void;

export interface SerializationErrorCoalescer {
  add(error: SerializationError): void;
  cleanup(): void;
}

/**
 * Creates a coalescer that buffers SerializationError events over a debounce
 * window and emits a single toast:
 *   - 1 unique error  → detailed message: "Failed to serialize {type} '{id}': {error}"
 *   - N unique errors → summary: "N items failed to serialize"
 *
 * Deduplication is by (item_type, item_id) key — the last error for a given key
 * wins within the window.
 */
export function createSerializationErrorCoalescer(
  showToast: ShowToast,
  windowMs = 500,
): SerializationErrorCoalescer {
  const buffer = new Map<string, SerializationError>();
  let timer: ReturnType<typeof setTimeout> | undefined;

  function flush(): void {
    const errors = Array.from(buffer.values());
    buffer.clear();
    timer = undefined;

    if (errors.length === 0) return;

    if (errors.length === 1) {
      const { item_type, item_id, error } = errors[0];
      showToast(`Failed to serialize ${item_type} '${item_id}': ${error}`, 'error');
    } else {
      showToast(`${errors.length} items failed to serialize`, 'error');
    }
  }

  function add(error: SerializationError): void {
    const key = `${error.item_type}:${error.item_id}`;
    buffer.set(key, error);
    clearTimeout(timer);
    timer = setTimeout(flush, windowMs);
  }

  function cleanup(): void {
    clearTimeout(timer);
    timer = undefined;
    buffer.clear();
  }

  return { add, cleanup };
}
