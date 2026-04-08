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
      "Failed to serialize mesh 'Bracket.body': error variant 4",
      'error',
    );
  });

  it('resets the debounce window on each new error arrival', () => {
    const showToast = vi.fn();
    const coalescer = createSerializationErrorCoalescer(showToast);

    // Send first error, advance 400ms (not yet fired)
    coalescer.add({ item_type: 'mesh', item_id: 'A', error: 'err1' });
    vi.advanceTimersByTime(400);
    expect(showToast).not.toHaveBeenCalled();

    // Send second error (different key), advance 400ms more (800ms from first, 400ms from second)
    coalescer.add({ item_type: 'mesh', item_id: 'B', error: 'err2' });
    vi.advanceTimersByTime(400);
    // Timer reset — still not fired (only 400ms from the second add)
    expect(showToast).not.toHaveBeenCalled();

    // Advance remaining 100ms (500ms from second add) — now fires
    vi.advanceTimersByTime(100);
    expect(showToast).toHaveBeenCalledOnce();
    expect(showToast).toHaveBeenCalledWith('2 items failed to serialize', 'error');
  });

  it('cleanup() clears the pending timer and buffer — showToast never called', () => {
    const showToast = vi.fn();
    const coalescer = createSerializationErrorCoalescer(showToast);

    coalescer.add({ item_type: 'mesh', item_id: 'Bracket.body', error: 'non-finite f32 value' });
    coalescer.add({ item_type: 'value', item_id: 'param.width', error: 'Infinity' });

    // Cleanup before the window elapses
    coalescer.cleanup();

    // Advance well past the window — timer was cancelled
    vi.advanceTimersByTime(1000);

    expect(showToast).not.toHaveBeenCalled();
  });

  it('add() after cleanup() starts a fresh coalescing cycle', () => {
    const showToast = vi.fn();
    const coalescer = createSerializationErrorCoalescer(showToast);

    // Add an error and cancel before the window elapses
    coalescer.add({ item_type: 'mesh', item_id: 'Bracket.body', error: 'first error' });
    coalescer.cleanup();

    // Verify cleanup suppressed the first toast
    vi.advanceTimersByTime(500);
    expect(showToast).not.toHaveBeenCalled();

    // Add a new error after cleanup — should start a fresh timer and buffer
    coalescer.add({ item_type: 'mesh', item_id: 'Bracket.body', error: 'second error' });
    vi.advanceTimersByTime(500);

    // Exactly one toast with only the new error (buffer was cleared by cleanup)
    expect(showToast).toHaveBeenCalledOnce();
    expect(showToast).toHaveBeenCalledWith(
      "Failed to serialize mesh 'Bracket.body': second error",
      'error',
    );
  });

  it('forces a flush at maxWaitMs ceiling when errors arrive faster than windowMs', () => {
    const showToast = vi.fn();
    // windowMs=100ms, maxWaitMs=500ms: errors every 80ms keep resetting the debounce
    const coalescer = createSerializationErrorCoalescer(showToast, 100, 500);

    // Add an error every 80ms for 6 iterations (t=0..480ms total after 6 advances)
    for (let i = 0; i < 6; i++) {
      coalescer.add({ item_type: 'mesh', item_id: `item-${i}`, error: 'err' });
      vi.advanceTimersByTime(80);
      expect(showToast).not.toHaveBeenCalled();
    }

    // At t=480ms: add the 7th error.
    // remaining = 500 - 480 = 20ms → timer should be min(100, 20) = 20ms, firing at t=500.
    // Without maxWaitMs the timer fires 100ms later at t=580.
    coalescer.add({ item_type: 'mesh', item_id: 'item-6', error: 'err' });

    vi.advanceTimersByTime(19);
    expect(showToast).not.toHaveBeenCalled();

    // Cross the 500ms ceiling — flush must fire now
    vi.advanceTimersByTime(1);
    expect(showToast).toHaveBeenCalledOnce();
    expect(showToast).toHaveBeenCalledWith('7 items failed to serialize', 'error');
  });

  it('caps the timer at remaining time when the next add is near the maxWaitMs boundary', () => {
    const showToast = vi.fn();
    // windowMs=200ms, maxWaitMs=500ms
    const coalescer = createSerializationErrorCoalescer(showToast, 200, 500);

    // Add errors every 100ms to keep resetting the debounce without letting any timer fire.
    // firstArrival is anchored at t=0 throughout.
    //
    // t=0  : add A, elapsed=0,   remaining=500, timer=min(200,500)=200ms → fires at t=200
    // t=100: add B, elapsed=100, remaining=400, timer=min(200,400)=200ms → fires at t=300
    // t=200: add C, elapsed=200, remaining=300, timer=min(200,300)=200ms → fires at t=400
    // t=300: add D, elapsed=300, remaining=200, timer=min(200,200)=200ms → fires at t=500
    // t=400: add E, elapsed=400, remaining=100, timer=min(200,100)=100ms → fires at t=500
    //                                                     ^^^  ← key: capped to remaining
    coalescer.add({ item_type: 'mesh', item_id: 'A', error: 'err' });
    vi.advanceTimersByTime(100);
    coalescer.add({ item_type: 'mesh', item_id: 'B', error: 'err' });
    vi.advanceTimersByTime(100);
    coalescer.add({ item_type: 'mesh', item_id: 'C', error: 'err' });
    vi.advanceTimersByTime(100);
    coalescer.add({ item_type: 'mesh', item_id: 'D', error: 'err' });
    vi.advanceTimersByTime(100);
    // t=400: elapsed=400, remaining=100 < windowMs=200 → timer set to 100ms
    coalescer.add({ item_type: 'mesh', item_id: 'E', error: 'err' });
    expect(showToast).not.toHaveBeenCalled();

    // Advance 99ms → t=499: flush has not fired yet
    vi.advanceTimersByTime(99);
    expect(showToast).not.toHaveBeenCalled();

    // Advance 1ms → t=500: flush fires (100ms after t=400, not 200ms)
    // Without Math.min the timer would fire at t=600; this assertion fails without it.
    vi.advanceTimersByTime(1);
    expect(showToast).toHaveBeenCalledOnce();
    expect(showToast).toHaveBeenCalledWith('5 items failed to serialize', 'error');
  });

  it('cleanup() prevents a pending max-wait flush from firing', () => {
    const showToast = vi.fn();
    // windowMs=100ms, maxWaitMs=300ms
    const coalescer = createSerializationErrorCoalescer(showToast, 100, 300);

    // Add errors every 80ms so the debounce window never fires on its own.
    // firstArrival is anchored at t=0.
    //
    // t=0  : add A, timer=min(100,300)=100ms → fires at t=100
    // t=80 : add B, elapsed=80, remaining=220, timer=100ms → fires at t=180
    // t=160: add C, elapsed=160, remaining=140, timer=100ms → fires at t=260
    coalescer.add({ item_type: 'mesh', item_id: 'A', error: 'err' });
    vi.advanceTimersByTime(80);
    coalescer.add({ item_type: 'mesh', item_id: 'B', error: 'err' });
    vi.advanceTimersByTime(80);
    coalescer.add({ item_type: 'mesh', item_id: 'C', error: 'err' });
    // t=160: timer fires at t=260 (100ms from now)
    expect(showToast).not.toHaveBeenCalled();

    // Call cleanup() before maxWaitMs (300ms) elapses
    coalescer.cleanup();

    // Advance well past both the pending timer (t=260) and the ceiling (t=300)
    vi.advanceTimersByTime(500);

    // The cleanup should have cleared the timer AND reset firstArrival,
    // so no flush should ever fire.
    expect(showToast).not.toHaveBeenCalled();
  });

  it('respects default maxWaitMs (3000ms) — flush fires at the ceiling under sustained errors', () => {
    const showToast = vi.fn();
    // Use default args: windowMs=500ms, maxWaitMs=3000ms
    const coalescer = createSerializationErrorCoalescer(showToast);

    // Send 8 errors, each 400ms apart. Without maxWaitMs the debounce keeps resetting
    // and the flush would not fire until 400ms after the last add (t=2800+500=t=3300ms).
    // With maxWaitMs=3000 the flush must fire at or before t=3000ms.
    //
    // t=0   : add 0, firstArrival=0, remaining=3000, timer=min(500,3000)=500ms
    // t=400 : add 1, elapsed=400,  remaining=2600, timer=500ms
    // t=800 : add 2, elapsed=800,  remaining=2200, timer=500ms
    // t=1200: add 3, elapsed=1200, remaining=1800, timer=500ms
    // t=1600: add 4, elapsed=1600, remaining=1400, timer=500ms
    // t=2000: add 5, elapsed=2000, remaining=1000, timer=500ms
    // t=2400: add 6, elapsed=2400, remaining=600,  timer=min(500,600)=500ms → fires at t=2900
    // t=2800: add 7, elapsed=2800, remaining=200,  timer=min(500,200)=200ms → fires at t=3000
    for (let i = 0; i < 8; i++) {
      coalescer.add({ item_type: 'mesh', item_id: `item-${i}`, error: 'err' });
      if (i < 7) {
        vi.advanceTimersByTime(400);
        expect(showToast).not.toHaveBeenCalled();
      }
    }

    // At t=2800, timer fires in 200ms (not 500ms). Advance 200ms → t=3000.
    // Without maxWaitMs the flush fires at t=3300; this assertion fails without the cap.
    vi.advanceTimersByTime(200);
    expect(showToast).toHaveBeenCalledOnce();
    expect(showToast).toHaveBeenCalledWith('8 items failed to serialize', 'error');
  });

  it('resets firstArrival after a max-wait force-flush — second batch runs independently', () => {
    const showToast = vi.fn();
    // windowMs=100ms, maxWaitMs=300ms
    const coalescer = createSerializationErrorCoalescer(showToast, 100, 300);

    // First batch: add errors every 80ms until maxWait ceiling fires
    // t=0: add, firstArrival=0, timer=min(100,300)=100ms → fires at t=100
    // t=80: add, elapsed=80, remaining=220, timer=min(100,220)=100ms → fires at t=180
    // t=160: add, elapsed=160, remaining=140, timer=min(100,140)=100ms → fires at t=260
    // t=240: add, elapsed=240, remaining=60, timer=min(100,60)=60ms → fires at t=300
    coalescer.add({ item_type: 'mesh', item_id: 'a1', error: 'e' });
    vi.advanceTimersByTime(80);
    coalescer.add({ item_type: 'mesh', item_id: 'a2', error: 'e' });
    vi.advanceTimersByTime(80);
    coalescer.add({ item_type: 'mesh', item_id: 'a3', error: 'e' });
    vi.advanceTimersByTime(80);
    coalescer.add({ item_type: 'mesh', item_id: 'a4', error: 'e' });
    // t=240: timer set to 60ms → fires at t=300
    expect(showToast).not.toHaveBeenCalled();

    // Advance to t=300 — first flush fires at the maxWait ceiling
    vi.advanceTimersByTime(60);
    expect(showToast).toHaveBeenCalledOnce();
    expect(showToast).toHaveBeenNthCalledWith(1, '4 items failed to serialize', 'error');

    // Second batch: starts fresh — firstArrival should be undefined after the flush
    coalescer.add({ item_type: 'value', item_id: 'b1', error: 'e2' });
    coalescer.add({ item_type: 'value', item_id: 'b2', error: 'e2' });
    // windowMs=100ms, timer fires 100ms from the last add
    vi.advanceTimersByTime(100);
    expect(showToast).toHaveBeenCalledTimes(2);
    expect(showToast).toHaveBeenNthCalledWith(2, '2 items failed to serialize', 'error');
  });

  it('resets after a flush — can accumulate and emit a second batch', () => {
    const showToast = vi.fn();
    const coalescer = createSerializationErrorCoalescer(showToast);

    // First batch
    coalescer.add({ item_type: 'mesh', item_id: 'A', error: 'err1' });
    vi.advanceTimersByTime(500);
    expect(showToast).toHaveBeenCalledOnce();
    expect(showToast).toHaveBeenNthCalledWith(
      1,
      "Failed to serialize mesh 'A': err1",
      'error',
    );

    // Second batch — coalescer should have reset
    coalescer.add({ item_type: 'mesh', item_id: 'B', error: 'err2' });
    coalescer.add({ item_type: 'value', item_id: 'C', error: 'err3' });
    vi.advanceTimersByTime(500);
    expect(showToast).toHaveBeenCalledTimes(2);
    expect(showToast).toHaveBeenNthCalledWith(2, '2 items failed to serialize', 'error');
  });

  it('flushes synchronously when Date.now() advances past maxWaitMs without the timer firing', () => {
    // This test covers the `if (remaining <= 0) { flush(); }` synchronous branch in add().
    //
    // In production this branch is reached when a browser tab is backgrounded and the
    // JS engine throttles setTimeout delivery (the clock keeps ticking but the callback
    // is delayed far beyond its scheduled time).  We reproduce it here by using
    // vi.setSystemTime(), which advances Date.now() WITHOUT dispatching pending timers.
    //
    // We deliberately do NOT use vi.advanceTimersByTime() for the clock jump: that API
    // fires the pending 100ms setTimeout, which calls flush(), resets firstArrival, and
    // starts a fresh cycle — making elapsed=0 on the next add() and making the sync
    // branch permanently unreachable.
    const showToast = vi.fn();
    const coalescer = createSerializationErrorCoalescer(showToast, 100, 300);

    // ── Warmup cycle ── flush+reset to reach a clean state (firstArrival=undefined)
    coalescer.add({ item_type: 'mesh', item_id: 'warm', error: 'e' });
    vi.advanceTimersByTime(100); // fires the 100ms timer → flush() → firstArrival=undefined
    expect(showToast).toHaveBeenCalledOnce();
    showToast.mockClear();

    // ── Sync-flush cycle ──
    // (a) anchor firstArrival and schedule a 100ms timer
    const t0 = Date.now();
    coalescer.add({ item_type: 'mesh', item_id: 'bg1', error: 'err' });

    // (b) move Date.now() forward by 400ms WITHOUT firing the pending 100ms timer
    vi.setSystemTime(t0 + 400);

    // The timer has not fired yet — no toast
    expect(showToast).not.toHaveBeenCalled();

    // (c) add a second error: elapsed = (t0+400) - t0 = 400 > maxWaitMs(300)
    //     remaining = 300 - 400 = -100 ≤ 0 → sync flush fires inline, no timer needed
    expect(vi.getTimerCount()).toBe(1);
    coalescer.add({ item_type: 'mesh', item_id: 'bg2', error: 'err' });

    // Assert the flush happened synchronously — no vi.advanceTimersByTime() call required
    expect(showToast).toHaveBeenCalledOnce();
    expect(showToast).toHaveBeenCalledWith('2 items failed to serialize', 'error');
  });
});
