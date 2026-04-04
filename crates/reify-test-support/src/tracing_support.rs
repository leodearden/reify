//! Shared tracing test utilities for reify crates.

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
        let (sub, _count) = warn_counting_subscriber();
        let (id_a, id_b) = tracing::subscriber::with_default(sub, || {
            let a = tracing::trace_span!("a").id();
            let b = tracing::trace_span!("b").id();
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
