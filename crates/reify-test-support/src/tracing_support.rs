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
pub fn warn_counting_subscriber(
) -> (impl tracing::Subscriber + Send + Sync, Arc<AtomicUsize>) {
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
        // Accept WARN and above (WARN, ERROR).  Reject DEBUG, TRACE, INFO.
        metadata.level() <= &tracing::Level::WARN
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let id = self.span_counter.fetch_add(1, Ordering::Relaxed);
        // Safety: Id::from_u64 requires a non-zero value; our counter starts at 1.
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(
        &self,
        _span: &tracing::span::Id,
        _follows: &tracing::span::Id,
    ) {
    }

    fn event(&self, event: &tracing::Event<'_>) {
        if event.metadata().level() == &tracing::Level::WARN {
            self.warn_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use crate::warn_counting_subscriber;

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

    /// Non-WARN events (DEBUG, INFO, ERROR) are NOT counted.
    #[test]
    fn non_warn_events_are_not_counted() {
        let (subscriber, warn_count) = warn_counting_subscriber();

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
    }

    /// Two calls to new_span must produce distinct span IDs (AtomicU64 uniqueness).
    #[test]
    fn new_span_ids_are_unique() {
        // Each subscriber issues IDs starting from 1; what matters is that
        // within a single subscriber the IDs advance and are not all the same.
        // We verify by calling new_span twice on the SAME subscriber instance.
        //
        // We use WARN-level spans because our subscriber's `enabled()` only
        // accepts WARN and above; trace_span! would produce disabled spans
        // (no ID) that never call new_span().
        let (sub, _count) = warn_counting_subscriber();
        let (id_a, id_b) = tracing::subscriber::with_default(sub, || {
            let a = tracing::span!(tracing::Level::WARN, "a").id();
            let b = tracing::span!(tracing::Level::WARN, "b").id();
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
