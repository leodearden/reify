//! Shared tracing test utilities for reify crates.

use std::collections::HashMap;
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

// ── CountingSubscriberBuilder ─────────────────────────────────────────────────

/// Builder for a minimal [`tracing::Subscriber`] that counts events at
/// registered levels and optionally filters by target prefix.
///
/// # Example
///
/// ```rust,ignore
/// let (subscriber, counters) = CountingSubscriberBuilder::new()
///     .count_level(tracing::Level::WARN)
///     .count_level(tracing::Level::DEBUG)
///     .target_prefix("reify_constraints")
///     .build();
/// ```
pub struct CountingSubscriberBuilder {
    levels: Vec<tracing::Level>,
    target_prefix: Option<String>,
}

impl CountingSubscriberBuilder {
    /// Create a new builder with no registered levels and no target filter.
    pub fn new() -> Self {
        Self {
            levels: Vec::new(),
            target_prefix: None,
        }
    }

    /// Register a level to count.  May be called multiple times for different
    /// levels; each call adds an independent counter for that level.
    pub fn count_level(mut self, level: tracing::Level) -> Self {
        self.levels.push(level);
        self
    }

    /// Set an optional target-prefix filter.  When set, only events whose
    /// target starts with `prefix` are counted; all others are ignored inside
    /// `event()`.
    pub fn target_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.target_prefix = Some(prefix.into());
        self
    }

    /// Build the subscriber and return it alongside a map of counters keyed by
    /// level.  The returned `Arc<AtomicUsize>` values are shared with the
    /// subscriber so external reads observe internal increments.
    pub fn build(
        self,
    ) -> (
        impl tracing::Subscriber + Send + Sync,
        HashMap<tracing::Level, Arc<AtomicUsize>>,
    ) {
        let counters: HashMap<tracing::Level, Arc<AtomicUsize>> = self
            .levels
            .into_iter()
            .map(|level| (level, Arc::new(AtomicUsize::new(0))))
            .collect();

        let subscriber = CountingSubscriber {
            counters: counters.clone(),
            target_prefix: self.target_prefix,
            span_counter: AtomicU64::new(1),
        };

        (subscriber, counters)
    }
}

impl Default for CountingSubscriberBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ── CountingSubscriber (private) ──────────────────────────────────────────────

struct CountingSubscriber {
    counters: HashMap<tracing::Level, Arc<AtomicUsize>>,
    target_prefix: Option<String>,
    /// Monotonically increasing counter used to generate unique span IDs.
    ///
    /// # Span ID uniqueness
    ///
    /// Unlike a naïve implementation that returns `Id::from_u64(1)` for every
    /// span, this subscriber uses an [`AtomicU64`] to issue monotonically
    /// increasing IDs, avoiding the "all spans share the same ID" bug.
    span_counter: AtomicU64,
}

impl tracing::Subscriber for CountingSubscriber {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        // Accept only events at registered levels; reject everything else
        // at the gate so no unregistered events reach event().
        self.counters.contains_key(metadata.level())
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        // Relaxed ordering is correct: the only invariant is uniqueness, which
        // fetch_add guarantees atomically regardless of memory ordering.
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
        // Apply optional target-prefix filter before counting.
        if let Some(prefix) = &self.target_prefix
            && !event.metadata().target().starts_with(prefix.as_str())
        {
            return;
        }
        if let Some(counter) = self.counters.get(event.metadata().level()) {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
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

    fn event(&self, event: &tracing::Event<'_>) {
        // The tracing dispatcher only calls event() when enabled() returned
        // true; our enabled() accepts only WARN, so only WARN events reach
        // here. The debug_assert catches direct misuse (called outside the
        // dispatcher) loudly in debug builds.
        debug_assert_eq!(
            event.metadata().level(),
            &tracing::Level::WARN,
            "event() reached with non-WARN — enabled() contract violated"
        );
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

    /// Non-WARN events (DEBUG, INFO, ERROR) do not affect the warn counter.
    ///
    /// The subscriber implements two-level filtering as defense in depth:
    /// `enabled()` is the first gate (rejects non-WARN events before `event()` is
    /// called), and `event()` is the second gate (only increments the counter when
    /// the level is WARN).  This test validates end-to-end counting correctness —
    /// that the counter stays zero when only non-WARN events are emitted.
    ///
    /// See `error_events_rejected_by_enabled_filter` for the test that
    /// specifically validates the `enabled()` gate.
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
            "warn counter must remain zero when only non-WARN events are emitted"
        );
    }

    /// Two calls to new_span must produce distinct span IDs, and the non-zero
    /// invariant required by `tracing::span::Id::from_u64` must hold.
    ///
    /// # Invariants
    ///
    /// - **Non-zero**: `Id::from_u64` panics if passed zero, so the subscriber
    ///   must never issue an ID with underlying value 0.  The counter is
    ///   initialised to 1 to guarantee this; a regression that started the
    ///   counter at 0 would panic inside `new_span()` → `Id::from_u64(0)`
    ///   before control returns to this test, so the non-zero invariant is
    ///   enforced by construction rather than by a runtime assertion here.
    /// - **Uniqueness**: successive calls return distinct IDs.  This is
    ///   guaranteed by `AtomicU64::fetch_add` and asserted below.
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

        // Uniqueness invariant: successive new_span calls must not collide.
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

    /// `CountingSubscriberBuilder` with a single registered level (WARN) should
    /// count exactly one WARN event and leave the counter at 1.
    /// No target_prefix is set, exercising the no-filter path.
    #[test]
    fn counting_subscriber_counts_warn_events() {
        use tracing::Level;

        use crate::CountingSubscriberBuilder;

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(Level::WARN)
            .build();

        let warn_arc: Arc<AtomicUsize> = Arc::clone(&counters[&Level::WARN]);

        assert_eq!(
            warn_arc.load(Ordering::Relaxed),
            0,
            "counter should start at 0"
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("test warning");
        });

        assert_eq!(
            warn_arc.load(Ordering::Relaxed),
            1,
            "one WARN event should produce count=1"
        );
    }

    /// `CountingSubscriberBuilder` with `target_prefix` should only count events
    /// whose target starts with the given prefix.
    ///
    /// Emits one matching-target WARN event and one non-matching WARN event;
    /// asserts the counter reads exactly 1.
    #[test]
    fn counting_subscriber_filters_by_target_prefix() {
        use tracing::Level;

        use crate::CountingSubscriberBuilder;

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(Level::WARN)
            .target_prefix("reify_constraints")
            .build();

        let warn_arc: Arc<AtomicUsize> = Arc::clone(&counters[&Level::WARN]);

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(target: "reify_constraints::solver", "matching target");
            tracing::warn!(target: "argmin::core", "non-matching target");
        });

        assert_eq!(
            warn_arc.load(Ordering::Relaxed),
            1,
            "only the matching-target event should be counted"
        );
    }

    /// Two registered levels each maintain independent counters.
    ///
    /// Registers both DEBUG and WARN with no target prefix; emits one event at
    /// each level and asserts both counters read exactly 1.
    #[test]
    fn counting_subscriber_supports_multiple_levels() {
        use tracing::Level;

        use crate::CountingSubscriberBuilder;

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(Level::DEBUG)
            .count_level(Level::WARN)
            .build();

        let debug_arc: Arc<AtomicUsize> = Arc::clone(&counters[&Level::DEBUG]);
        let warn_arc: Arc<AtomicUsize> = Arc::clone(&counters[&Level::WARN]);

        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!("debug event");
            tracing::warn!("warn event");
        });

        assert_eq!(
            debug_arc.load(Ordering::Relaxed),
            1,
            "one DEBUG event should produce debug count=1"
        );
        assert_eq!(
            warn_arc.load(Ordering::Relaxed),
            1,
            "one WARN event should produce warn count=1"
        );
    }

    /// Unregistered levels are rejected at the `enabled()` gate — they never
    /// reach `event()`.
    ///
    /// Wraps a `CountingSubscriber` registered only for WARN in a thin
    /// `EventDispatchCounter` that increments a shared counter on each call to
    /// `event()`.  Emits ERROR and INFO events; asserts `dispatch_count` stays 0.
    #[test]
    fn counting_subscriber_enabled_rejects_unregistered_levels() {
        use tracing::Level;

        use crate::CountingSubscriberBuilder;

        struct EventDispatchCounter<S> {
            inner: S,
            dispatch_count: Arc<AtomicUsize>,
        }

        impl<S: tracing::Subscriber> tracing::Subscriber for EventDispatchCounter<S> {
            fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
                self.inner.enabled(metadata)
            }

            fn new_span(
                &self,
                span: &tracing::span::Attributes<'_>,
            ) -> tracing::span::Id {
                self.inner.new_span(span)
            }

            fn record(
                &self,
                span: &tracing::span::Id,
                values: &tracing::span::Record<'_>,
            ) {
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

        let dispatch_count = Arc::new(AtomicUsize::new(0));

        let (inner, _counters) = CountingSubscriberBuilder::new()
            .count_level(Level::WARN)
            .build();

        let subscriber = EventDispatchCounter {
            inner,
            dispatch_count: Arc::clone(&dispatch_count),
        };

        tracing::subscriber::with_default(subscriber, || {
            tracing::error!("error event — should be rejected at enabled()");
            tracing::info!("info event — should be rejected at enabled()");
        });

        assert_eq!(
            dispatch_count.load(Ordering::Relaxed),
            0,
            "ERROR and INFO events must be rejected at enabled(), never reaching event()"
        );
    }

    // debug_assert_eq! is compiled out in release builds, so this test would
    // incorrectly fail under #[should_panic] without the cfg gate.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "enabled() contract violated")]
    fn event_panics_on_non_warn_when_dispatcher_contract_violated() {
        // ForcedEventDispatcher bypasses the inner subscriber's enabled() filter
        // by always returning true, allowing non-WARN events to reach event().
        struct ForcedEventDispatcher<S> {
            inner: S,
        }

        impl<S: tracing::Subscriber> tracing::Subscriber for ForcedEventDispatcher<S> {
            fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
                // Always return true, bypassing the inner subscriber's filter.
                true
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
                self.inner.event(event)
            }

            fn enter(&self, span: &tracing::span::Id) {
                self.inner.enter(span)
            }

            fn exit(&self, span: &tracing::span::Id) {
                self.inner.exit(span)
            }
        }

        let (inner, _warn_count) = warn_counting_subscriber();
        let subscriber = ForcedEventDispatcher { inner };

        tracing::subscriber::with_default(subscriber, || {
            tracing::error!("error message bypassing filter");
        });
    }

    /// Two consecutive `new_span` calls on a `CountingSubscriber` must produce
    /// distinct span IDs (regression guard for the Id::from_u64(1) bug).
    #[test]
    fn counting_subscriber_produces_unique_span_ids() {
        use tracing::Level;

        use crate::CountingSubscriberBuilder;

        let (sub, _counters) = CountingSubscriberBuilder::new()
            .count_level(Level::WARN)
            .build();

        let (id_a, id_b) = tracing::subscriber::with_default(sub, || {
            let a = tracing::span!(Level::WARN, "span_a")
                .id()
                .expect("WARN span should be enabled by CountingSubscriber");
            let b = tracing::span!(Level::WARN, "span_b")
                .id()
                .expect("WARN span should be enabled by CountingSubscriber");
            (a, b)
        });

        assert_ne!(
            id_a, id_b,
            "successive new_span calls must return distinct IDs"
        );
    }
}
