import { describe, it, expect } from 'vitest';
import { createNavHistory, type NavEntry } from '../hooks/useNavHistory';

describe('createNavHistory', () => {
  it('(a) fresh history has null current, canGoBack===false, canGoForward===false', () => {
    const h = createNavHistory();
    expect(h.current()).toBeNull();
    expect(h.canGoBack()).toBe(false);
    expect(h.canGoForward()).toBe(false);
    expect(h.size()).toBe(0);
  });

  it('(b) push A then push B: back() returns A, then forward() returns B', () => {
    const h = createNavHistory();
    const A: NavEntry = { uri: 'file:///a.ri', offset: 10 };
    const B: NavEntry = { uri: 'file:///a.ri', offset: 20 };

    h.push(A);
    h.push(B);

    expect(h.current()).toEqual(B);
    expect(h.canGoBack()).toBe(true);
    expect(h.canGoForward()).toBe(false);

    const backResult = h.back();
    expect(backResult).toEqual(A);
    expect(h.current()).toEqual(A);
    expect(h.canGoBack()).toBe(false);
    expect(h.canGoForward()).toBe(true);

    const fwdResult = h.forward();
    expect(fwdResult).toEqual(B);
    expect(h.current()).toEqual(B);
    expect(h.canGoBack()).toBe(true);
    expect(h.canGoForward()).toBe(false);
  });

  it('(c) push C at back position truncates forward entries', () => {
    const h = createNavHistory();
    const A: NavEntry = { uri: 'file:///a.ri', offset: 10 };
    const B: NavEntry = { uri: 'file:///a.ri', offset: 20 };
    const C: NavEntry = { uri: 'file:///a.ri', offset: 30 };

    h.push(A);
    h.push(B);
    h.back(); // current is A, B is in forward

    h.push(C); // truncates B, appends C

    expect(h.current()).toEqual(C);
    expect(h.canGoForward()).toBe(false);
    expect(h.forward()).toBeNull();
    expect(h.canGoBack()).toBe(true);
    expect(h.back()).toEqual(A);
    expect(h.size()).toBe(2); // A and C
  });

  it('(d) consecutive-dedupe: pushing current entry is a no-op', () => {
    const h = createNavHistory();
    const A: NavEntry = { uri: 'file:///a.ri', offset: 10 };

    h.push(A);
    const sizeBefore = h.size();
    h.push({ uri: 'file:///a.ri', offset: 10 }); // same {uri, offset}
    expect(h.size()).toBe(sizeBefore);
    expect(h.current()).toEqual(A);
  });

  it('(d) consecutive-dedupe: different uri is NOT a no-op', () => {
    const h = createNavHistory();
    const A: NavEntry = { uri: 'file:///a.ri', offset: 10 };
    const B: NavEntry = { uri: 'file:///b.ri', offset: 10 };

    h.push(A);
    h.push(B);
    expect(h.size()).toBe(2);
  });

  it('(d) consecutive-dedupe: different offset is NOT a no-op', () => {
    const h = createNavHistory();
    const A: NavEntry = { uri: 'file:///a.ri', offset: 10 };
    const B: NavEntry = { uri: 'file:///a.ri', offset: 11 };

    h.push(A);
    h.push(B);
    expect(h.size()).toBe(2);
  });

  it('(e) bounded depth: push 60 entries with maxDepth=50 → size===50, oldest dropped', () => {
    const h = createNavHistory(50);
    const entries: NavEntry[] = [];
    for (let i = 0; i < 60; i++) {
      const e: NavEntry = { uri: 'file:///a.ri', offset: i };
      entries.push(e);
      h.push(e);
    }

    expect(h.size()).toBe(50);

    // Current should be the last pushed entry (offset=59)
    expect(h.current()).toEqual({ uri: 'file:///a.ri', offset: 59 });

    // Navigate all the way back — should stop at the 11th pushed (offset=10)
    let lastEntry: NavEntry | null = null;
    let backCount = 0;
    while (h.canGoBack()) {
      lastEntry = h.back();
      backCount++;
    }
    // We navigated back 49 times (from index 49 to index 0)
    expect(backCount).toBe(49);
    // The earliest reachable entry should be the 11th pushed (offset=10, i=10)
    expect(h.current()).toEqual({ uri: 'file:///a.ri', offset: 10 });
    // canGoBack is false at the oldest entry
    expect(h.canGoBack()).toBe(false);
    // back() at the end returns null without mutating
    expect(h.back()).toBeNull();
    // current is still the oldest
    expect(h.current()).toEqual({ uri: 'file:///a.ri', offset: 10 });
  });

  it('(f) back() at start returns null without mutating index', () => {
    const h = createNavHistory();
    const A: NavEntry = { uri: 'file:///a.ri', offset: 5 };
    h.push(A);

    expect(h.back()).toBeNull();
    expect(h.current()).toEqual(A);
    expect(h.canGoBack()).toBe(false);
  });

  it('(f) forward() at end returns null without mutating index', () => {
    const h = createNavHistory();
    const A: NavEntry = { uri: 'file:///a.ri', offset: 5 };
    const B: NavEntry = { uri: 'file:///a.ri', offset: 15 };
    h.push(A);
    h.push(B);

    expect(h.forward()).toBeNull();
    expect(h.current()).toEqual(B);
    expect(h.canGoForward()).toBe(false);
  });

  it('(f) forward() on empty history returns null', () => {
    const h = createNavHistory();
    expect(h.forward()).toBeNull();
    expect(h.current()).toBeNull();
  });

  it('all offsets are exact integers', () => {
    const h = createNavHistory();
    h.push({ uri: 'file:///a.ri', offset: 0 });
    h.push({ uri: 'file:///a.ri', offset: 100 });
    h.push({ uri: 'file:///b.ri', offset: 42 });

    expect(h.current()?.offset).toBe(42);
    h.back();
    expect(h.current()?.offset).toBe(100);
    h.back();
    expect(h.current()?.offset).toBe(0);
  });

  it('default maxDepth is 50', () => {
    const h = createNavHistory(); // no argument
    for (let i = 0; i < 60; i++) {
      h.push({ uri: 'file:///a.ri', offset: i });
    }
    expect(h.size()).toBe(50);
  });
});
