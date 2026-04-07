//! Shared tracing test utilities for reify crates.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Build a minimal [`tracing::Subscriber`] that counts WARN-level events.
///
/// Returns a `(subscriber, counter)` pair.  The `counter` is shared via
/// [`Arc`] so callers can read the count after the subscriber has been
/// installed and removed.
///
/// # Span ID uniqueness
///
/// Unlike a naïve implementation that returns `Id::from_u64(1)` for every
/// span, this subscriber uses an [`AtomicU64`] to issue monotonically
/// increasing IDs, avoiding the "all spans share the same ID" bug.
pub fn warn_counting_subscriber() -> (impl tracing::Subscriber + Send + Sync, Arc<AtomicUsize>) {
    let warn_count = Arc::new(AtomicUsize::new(0));
    let warn_count_clone = Arc::clone(&warn_count);
    (WarnCountingSubscriber::new(warn_count_clone), warn_count)
}

// ── private implementation ────────────────────────────────────────────────────

struct WarnCountingSubscriber {
    warn_count: Arc<AtomicUsize>,
    /// Monotonically increasing counter used to generate unique span IDs.
    span_counter: AtomicU64,
}

impl WarnCountingSubscriber {
    fn new(warn_count: Arc<AtomicUsize>) -> Self {
        Self {
            warn_count,
            span_counter: AtomicU64::new(1),
        }
    }
}

impl tracing::Subscriber for WarnCountingSubscriber {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        // Accept only WARN events.  Reject all other levels (ERROR, INFO, DEBUG, TRACE).
        metadata.level() == &tracing::Level::WARN
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        // Relaxed ordering is correct: the only invariant is uniqueness, which
        // fetch_add guarantees atomically regardless of memory ordering.  No
        // synchronisation with other memory operations is required here because
        // tracing stabilises span IDs before they are shared across threads.
        let id = self.span_counter.fetch_add(1, Ordering::Relaxed);
        // Safety: Id::from_u64 requires a non-zero value; our counter starts at 1.
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, _event: &tracing::Event<'_>) {
        // enabled() guarantees only WARN events are dispatched here.
        self.warn_count.fetch_add(1, Ordering::Relaxed);
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::warn_counting_subscriber;

    /// ERROR events should be rejected at the `enabled()` gate, not silently
    /// accepted and then discarded inside `event()`.
    ///
    /// We verify this by wrapping the real `WarnCountingSubscriber` in a thin
    /// `EventDispatchCounter` that increments `dispatch_count` each time the
    /// tracing framework calls `event()` on us.  Because the wrapper delegates
    /// `enabled()` to the inner subscriber, `event()` is only reached when the
    /// inner subscriber's `enabled()` returns `true`.
    ///
    /// Under the current `<=` filter ERROR passes `enabled()`, so
    /// `dispatch_count` ends up as 1 and this test **fails**.
    /// After the fix (`==`), ERROR is rejected at `enabled()` and
    /// `dispatch_count` stays 0.
    #[test]
    fn error_events_rejected_by_enabled_filter() {
        // ── thin wrapper ────────────────────────────────────────────────────
        struct EventDispatchCounter<S> {
            inner: S,
            dispatch_count: Arc<AtomicUsize>,
        }

        impl<S: tracing::Subscriber> tracing::Subscriber for EventDispatchCounter<S> {
            fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
                // Delegate to the inner subscriber so its filter is exercised.
                self.inner.enabled(metadata)
            }

            fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
                self.inner.new_span(span)
            }

            fn record(&self, span: &tracing::span::Id, values: &tracing::span::Record<'_>) {
                self.inner.record(span, values)
            }

            fn record_follows_from(&self, span: &tracing::span::Id, follows: &tracing::span::Id) {
                self.inner.record_follows_from(span, follows)
            }

            fn event(&self, event: &tracing::Event<'_>) {
                // Reached only when enabled() returned true.
                self.dispatch_count.fetch_add(1, Ordering::Relaxed);
                self.inner.event(event)
            }

            fn enter(&self, span: &tracing::span::Id) {
                self.inner.enter(span)
            }

            fn exit(&self, span: &tracing::span::Id) {
                self.inner.exit(span)
            }
        }

        // ── test body ───────────────────────────────────────────────────────
        let warn_count = Arc::new(AtomicUsize::new(0));
        let dispatch_count = Arc::new(AtomicUsize::new(0));

        let inner = super::WarnCountingSubscriber::new(Arc::clone(&warn_count));
        let subscriber = EventDispatchCounter {
            inner,
            dispatch_count: Arc::clone(&dispatch_count),
        };

        tracing::subscriber::with_default(subscriber, || {
            tracing::error!("error message");
        });

        // warn_count is 0 even under the current code because event() checks
        // level == WARN before incrementing.  This assertion passes both before
        // and after the fix.
        assert_eq!(
            warn_count.load(Ordering::Relaxed),
            0,
            "ERROR must not be counted as a WARN event"
        );

        // dispatch_count is 0 only when enabled() rejected the ERROR event.
        // Under the current `<=` filter dispatch_count == 1 → this FAILS.
        // After the fix (`==` filter) dispatch_count == 0 → this PASSES.
        assert_eq!(
            dispatch_count.load(Ordering::Relaxed),
            0,
            "ERROR events must be rejected at enabled(), not reach event()"
        );
    }

    /// warn_counting_subscriber returns a working (subscriber, counter) pair.
    /// WARN events increment the counter; the counter starts at 0.
    #[test]
    fn warn_events_increment_counter() {
        let (subscriber, warn_count) = warn_counting_subscriber();
        assert_eq!(
            warn_count.load(Ordering::Relaxed),
            0,
            "counter should start at 0"
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("test warning");
        });

        assert_eq!(
            warn_count.load(Ordering::Relaxed),
            1,
            "one WARN event should produce count=1"
        );
    }

    /// Non-WARN events (DEBUG, INFO, ERROR) are rejected at the `enabled()` gate
    /// and never dispatched to `event()`.
    ///
    /// Uses an `EventDispatchCounter` wrapper to verify that `event()` is never
    /// called for DEBUG/INFO/ERROR events, confirming gate-rejection rather than
    /// silent discard inside `event()`.
    #[test]
    fn non_warn_events_are_not_counted() {
        // ── thin wrapper ────────────────────────────────────────────────────
        struct EventDispatchCounter<S> {
            inner: S,
            dispatch_count: Arc<AtomicUsize>,
        }

        impl<S: tracing::Subscriber> tracing::Subscriber for EventDispatchCounter<S> {
            fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
                // Delegate to the inner subscriber so its filter is exercised.
                self.inner.enabled(metadata)
            }

            fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
                self.inner.new_span(span)
            }

            fn record(&self, span: &tracing::span::Id, values: &tracing::span::Record<'_>) {
                self.inner.record(span, values)
            }

            fn record_follows_from(
                &self,
                span: &tracing::span::Id,
                follows: &tracing::span::Id,
            ) {
                self.inner.record_follows_from(span, follows)
            }

            fn event(&self, event: &tracing::Event<'_>) {
                // Reached only when enabled() returned true.
                self.dispatch_count.fetch_add(1, Ordering::Relaxed);
                self.inner.event(event)
            }

            fn enter(&self, span: &tracing::span::Id) {
                self.inner.enter(span)
            }

            fn exit(&self, span: &tracing::span::Id) {
                self.inner.exit(span)
            }
        }

        // ── test body ───────────────────────────────────────────────────────
        let warn_count = Arc::new(AtomicUsize::new(0));
        let dispatch_count = Arc::new(AtomicUsize::new(0));

        let inner = super::WarnCountingSubscriber::new(Arc::clone(&warn_count));
        let subscriber = EventDispatchCounter {
            inner,
            dispatch_count: Arc::clone(&dispatch_count),
        };

        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!("debug message");
            tracing::info!("info message");
            tracing::error!("error message");
        });

        assert_eq!(
            warn_count.load(Ordering::Relaxed),
            0,
            "DEBUG/INFO/ERROR events must not increment the WARN counter"
        );

        assert_eq!(
            dispatch_count.load(Ordering::Relaxed),
            0,
            "DEBUG/INFO/ERROR events must be rejected at enabled(), not reach event()"
        );
    }

    /// Two calls to new_span must produce distinct span IDs (AtomicU64 uniqueness).
    #[test]
    fn new_span_ids_are_unique() {
        // Each subscriber issues IDs starting from 1; what matters is that
        // within a single subscriber the IDs advance and are not all the same.
        // We verify by calling new_span twice on the SAME subscriber instance.
        //
        // We use WARN-level spans because our subscriber's `enabled()` only
        // accepts WARN; trace_span! would produce disabled spans
        // (no ID) that never call new_span().
        let (sub, _count) = warn_counting_subscriber();
        let (id_a, id_b) = tracing::subscriber::with_default(sub, || {
            let a = tracing::span!(tracing::Level::WARN, "a")
                .id()
                .expect("WARN span should be enabled by WarnCountingSubscriber");
            let b = tracing::span!(tracing::Level::WARN, "b")
                .id()
                .expect("WARN span should be enabled by WarnCountingSubscriber");
            (a, b)
        });

        assert_ne!(
            id_a, id_b,
            "successive new_span calls must return distinct IDs"
        );
    }

    /// Counter is shared via Arc — the caller can observe increments from outside.
    #[test]
    fn counter_is_observable_via_arc() {
        let (subscriber, warn_count) = warn_counting_subscriber();
        let counter_clone: Arc<std::sync::atomic::AtomicUsize> = Arc::clone(&warn_count);

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("w1");
            tracing::warn!("w2");
        });

        assert_eq!(counter_clone.load(Ordering::Relaxed), 2);
    }
}
