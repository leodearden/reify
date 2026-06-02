/**
 * Navigation history — a bounded, browser-style back/forward stack of editor positions.
 *
 * Pure closure factory with no SolidJS dependency; safe to unit-test without a DOM.
 * Used by Editor.tsx to implement F12 goto-definition history (Alt+← / Alt+→).
 */

export interface NavEntry {
  /** The document URI (file:// or bare path). */
  uri: string;
  /** Absolute CodeMirror document offset of the cursor position. */
  offset: number;
}

export interface NavHistory {
  /**
   * Push an entry onto the history stack.
   *
   * - If the pushed entry equals the current entry (same uri AND same offset),
   *   the push is a no-op (consecutive-dedupe).
   * - Otherwise, any forward entries are truncated, the entry is appended, and
   *   if the stack exceeds maxDepth the oldest entry is dropped.
   */
  push(entry: NavEntry): void;
  /**
   * Navigate back one step.
   * Returns the entry now active (the one you landed on), or null if already at
   * the oldest entry (index stays unchanged).
   */
  back(): NavEntry | null;
  /**
   * Navigate forward one step.
   * Returns the entry now active (the one you landed on), or null if already at
   * the newest entry (index stays unchanged).
   */
  forward(): NavEntry | null;
  /** Returns true if a back() call would yield a non-null result. */
  canGoBack(): boolean;
  /** Returns true if a forward() call would yield a non-null result. */
  canGoForward(): boolean;
  /** Returns the current entry, or null if the stack is empty. */
  current(): NavEntry | null;
  /** Returns the number of entries in the stack. */
  size(): number;
}

/**
 * Create a bounded navigation-history stack.
 *
 * @param maxDepth Maximum number of entries to retain (default 50).
 *                 When a push exceeds this limit, the oldest entry is dropped.
 */
export function createNavHistory(maxDepth = 50): NavHistory {
  const entries: NavEntry[] = [];
  let index = -1;

  function current(): NavEntry | null {
    return index >= 0 ? entries[index] : null;
  }

  function push(entry: NavEntry): void {
    // Consecutive-dedupe: no-op when the pushed entry equals the current entry.
    const cur = current();
    if (cur !== null && cur.uri === entry.uri && cur.offset === entry.offset) {
      return;
    }

    // Truncate any forward entries (entries after current index).
    entries.splice(index + 1);

    // Append the new entry.
    entries.push(entry);

    // Clamp to maxDepth by dropping the oldest entry from the front.
    if (entries.length > maxDepth) {
      entries.shift();
      // index stays at entries.length - 1 because we removed one from the front
      // and added one at the back: net offset is zero.
    } else {
      index = entries.length - 1;
    }

    // After a possible shift, index must always point to the last element.
    index = entries.length - 1;
  }

  function back(): NavEntry | null {
    if (index <= 0) return null;
    index--;
    return entries[index];
  }

  function forward(): NavEntry | null {
    if (index >= entries.length - 1) return null;
    index++;
    return entries[index];
  }

  function canGoBack(): boolean {
    return index > 0;
  }

  function canGoForward(): boolean {
    return index < entries.length - 1;
  }

  function size(): number {
    return entries.length;
  }

  return { push, back, forward, canGoBack, canGoForward, current, size };
}
