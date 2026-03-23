/**
 * Chat panel persistence via localStorage.
 * Pure functions for loading and saving chat panel height and open state.
 */

export const CHAT_HEIGHT_KEY = 'claudePanelHeight';
export const CHAT_OPEN_KEY = 'claudePanelOpen';

/** Load persisted chat panel height. Returns null if missing, invalid, or non-number. */
export function loadChatPanelHeight(): number | null {
  try {
    const raw = localStorage.getItem(CHAT_HEIGHT_KEY);
    if (raw === null) return null;

    const parsed = JSON.parse(raw);
    if (typeof parsed === 'number') {
      return parsed;
    }
    return null;
  } catch {
    return null;
  }
}

/** Save chat panel height to localStorage. */
export function saveChatPanelHeight(height: number): void {
  try {
    localStorage.setItem(CHAT_HEIGHT_KEY, JSON.stringify(height));
  } catch {
    // Silently ignore — localStorage may be full or unavailable
  }
}

/** Load persisted chat panel open state. Returns null if missing, invalid, or non-boolean. */
export function loadChatPanelOpen(): boolean | null {
  try {
    const raw = localStorage.getItem(CHAT_OPEN_KEY);
    if (raw === null) return null;

    const parsed = JSON.parse(raw);
    if (typeof parsed === 'boolean') {
      return parsed;
    }
    return null;
  } catch {
    return null;
  }
}

/** Save chat panel open state to localStorage. */
export function saveChatPanelOpen(open: boolean): void {
  try {
    localStorage.setItem(CHAT_OPEN_KEY, JSON.stringify(open));
  } catch {
    // Silently ignore — localStorage may be full or unavailable
  }
}
