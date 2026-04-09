//! Shared tracing test utilities for reify crates.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Assert that `counter` has advanced by exactly `expected_delta` since the
/// `before` snapshot.
///
/// Computes the actual delta as `counter.load(Ordering::Acquire) - before`,
/// panicking first if the counter appears to have gone backwards (indicating a
/// stale or wrong `before` snapshot).  Uses `Acquire` ordering so that all
/// WARN event stores (which use `Release`) are visible to this load.
///
/// # Parameters
///
/// - `counter` — the warn counter returned by [`warn_counting_subscriber`] or
///   [`warn_counting_guard`].
/// - `before` — a snapshot of the counter taken before the code under test
///   ran.  Use `counter.load(Ordering::Acquire)` to take a snapshot.  Must
///   be a snapshot taken from the same counter before the code under test
///   ran; passing a value greater than the counter's current load (for
///   example, a stale snapshot from an unrelated counter or a reordered
///   read) will panic.
/// - `expected_delta` — how many WARN events you expect since the snapshot.
/// - `context` — included in the panic message for diagnostics.
///
/// # Panics
///
/// Panics if `before` is greater than the current counter value (backwards
/// counter — indicates a stale or wrong `before` snapshot).
///
/// Panics if the actual delta differs from `expected_delta`.
pub fn assert_warn_count_delta(
    counter: &AtomicUsize,
    before: usize,
    expected_delta: usize,
    context: &str,
) {
    let after = counter.load(Ordering::Acquire);
    assert!(
        after >= before,
        "warn counter went backwards (before={before}, after={after}): {context}"
    );
    let actual_delta = after - before;
    assert_eq!(
        actual_delta,
        expected_delta,
        "expected warn delta of {expected_delta} (before={before}, after={after}): {context}"
    );
}

/// Assert that `counter` equals `expected` (convenience wrapper for
/// [`assert_warn_count_delta`] with `before=0`).
///
/// Equivalent to `assert_warn_count_delta(counter, 0, expected, context)`.
/// Suited for tests where the subscriber is freshly installed and the counter
/// starts at zero.
///
/// # Panics
///
/// Panics if `counter.load(Acquire)` does not equal `expected`.
pub fn assert_warn_count(counter: &AtomicUsize, expected: usize, context: &str) {
    assert_warn_count_delta(counter, 0, expected, context);
}

/// Install a WARN-counting subscriber as the thread-default and return a RAII
/// guard alongside the shared counter.
///
/// This is a convenience wrapper around [`warn_counting_subscriber`] and
/// [`tracing::subscriber::set_default`] for tests that need a persistent
/// thread-default rather than a scoped `with_default` block — in particular
/// async tests on a `current_thread` runtime, where the entire test body
/// runs on one thread but cannot be wrapped by `with_default`.
///
/// # Returns
///
/// A `(guard, counter)` pair where:
/// - `guard` is a [`tracing::subscriber::DefaultGuard`].  When it drops, the
///   subscriber is removed and the previous default (if any) is restored.
/// - `counter` is the `Arc<AtomicUsize>` shared with the subscriber; loads
///   with `Ordering::Acquire` observe all WARN increments.
///
/// # Example
///
/// ```rust,ignore
/// let (_guard, counter) = warn_counting_guard();
/// tracing::warn!("oops");
/// assert_warn_count(&counter, 1, "must count the warning");
/// ```
pub fn warn_counting_guard() -> (tracing::subscriber::DefaultGuard, Arc<AtomicUsize>) {
    let (subscriber, counter) = warn_counting_subscriber();
    let guard = tracing::subscriber::set_default(subscriber);
    (guard, counter)
}

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
            // Release ordering pairs with Acquire loads in assertion helpers,
            // ensuring all memory written before the store is visible to threads
            // that observe the counter increment.
            counter.fetch_add(1, Ordering::Release);
        }
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}


// ── Contract violation marker ─────────────────────────────────────────────────

/// Canonical substring embedded in `debug_assert_eq!` panic messages when a
/// non-WARN event reaches `event()` in violation of the dispatcher contract.
///
/// # Release-mode asymmetry
///
/// Both `WarnCountingSubscriber` and `WarnCapturingSubscriber` rely entirely on
/// the dispatcher's `enabled()` contract: their `event()` implementations
/// perform no defensive level re-check.  In debug builds the `debug_assert_eq!`
/// that embeds this marker catches contract violations loudly.  In release
/// builds `debug_assert_eq!` is compiled out, so a violation would silently
/// miscount or miscapture — this is deliberate per the silent-defaults
/// alignment established by task 972, which favours minimal branches in the
/// hot path over defensive double-checks.
///
/// # Sync requirement
///
/// `#[should_panic(expected = ...)]` in
/// `tests::event_panics_on_non_warn_when_dispatcher_contract_violated`
/// (`WarnCountingSubscriber`) and
/// `tests::capturing_event_panics_on_non_warn_when_dispatcher_contract_violated`
/// (`WarnCapturingSubscriber`) both use the literal `"enabled() contract
/// violated"` — the same text as this const.  Because Rust requires a **string
/// literal** (not a const expression) in the `expected` parameter of
/// `#[should_panic]`, the sync cannot be enforced by the type system.  Instead
/// it is enforced by the `tests::contract_violation_marker_matches_panic_expected`
/// test, which asserts `CONTRACT_VIOLATION_MARKER == "enabled() contract
/// violated"` at runtime.  **Do not change this const without updating those
/// attributes.**
const CONTRACT_VIOLATION_MARKER: &str = "enabled() contract violated";

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
        // The tracing dispatcher only calls event() when enabled() returned true;
        // our enabled() accepts only WARN, so only WARN events should reach here.
        // See release-mode asymmetry note on `CONTRACT_VIOLATION_MARKER`.
        debug_assert_eq!(
            event.metadata().level(),
            &tracing::Level::WARN,
            "event() reached with non-WARN — {}",
            CONTRACT_VIOLATION_MARKER
        );
        // Release ordering pairs with Acquire loads in assertion helpers and
        // WarnCapture::count(), ensuring all prior memory writes are visible to
        // threads that observe the counter — especially important for async
        // tests on a current_thread runtime.
        self.warn_count.fetch_add(1, Ordering::Release);
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}

// ── WarnCapture (public) ──────────────────────────────────────────────────────

/// Captured output from [`warn_capturing_subscriber`]: event count and message
/// text for all WARN events.
pub struct WarnCapture {
    count: Arc<AtomicUsize>,
    messages: Arc<std::sync::Mutex<Vec<String>>>,
}

impl WarnCapture {
    /// Return the number of WARN events that have been emitted so far.
    pub fn count(&self) -> usize {
        // Acquire ordering pairs with the Release store in WarnCapturingSubscriber::event(),
        // ensuring the count is fully visible once observed.
        self.count.load(Ordering::Acquire)
    }

    /// Return a snapshot of all captured WARN event message strings.
    pub fn messages(&self) -> Vec<String> {
        self.messages.lock().unwrap().clone()
    }

    /// Assert that exactly `expected` WARN events were emitted.
    ///
    /// # Panics
    ///
    /// Panics if the count does not equal `expected`.
    pub fn assert_count(&self, expected: usize) {
        let n = self.count();
        assert_eq!(
            n, expected,
            "expected {expected} WARN events, got {n}"
        );
    }

    /// Assert that exactly `expected` WARN events were emitted **and** that at
    /// least one captured message contains `substring`.
    ///
    /// # Panics
    ///
    /// Panics if either condition fails.
    pub fn assert_count_and_any_message_contains(&self, expected: usize, substring: &str) {
        self.assert_count(expected);
        let msgs = self.messages();
        assert!(
            msgs.iter().any(|m| m.contains(substring)),
            "no WARN message contained {substring:?}; captured messages: {msgs:?}"
        );
    }
}

/// Build a minimal [`tracing::Subscriber`] that captures WARN-level events:
/// both the count and the formatted message text.
///
/// Returns a `(subscriber, capture)` pair.  The [`WarnCapture`] is shared via
/// [`Arc`] so callers can inspect results after the subscriber has been
/// installed and removed.
pub fn warn_capturing_subscriber() -> (impl tracing::Subscriber + Send + Sync, WarnCapture) {
    let count = Arc::new(AtomicUsize::new(0));
    let messages = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let capture = WarnCapture {
        count: Arc::clone(&count),
        messages: Arc::clone(&messages),
    };
    let subscriber = WarnCapturingSubscriber {
        count,
        messages,
        span_counter: AtomicU64::new(1),
    };
    (subscriber, capture)
}

// ── WarnCapturingSubscriber (private) ─────────────────────────────────────────

struct WarnCapturingSubscriber {
    count: Arc<AtomicUsize>,
    messages: Arc<std::sync::Mutex<Vec<String>>>,
    span_counter: AtomicU64,
}

/// A [`tracing::field::Visit`] implementation that extracts the formatted
/// `message` field from a tracing event.
struct MessageVisitor(String);

impl tracing::field::Visit for MessageVisitor {
    /// Intercept `&str` message values and store them directly, bypassing
    /// `record_debug`'s `{value:?}` formatting which would add surrounding
    /// double-quotes around string literals.
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_owned();
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl tracing::Subscriber for WarnCapturingSubscriber {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.level() == &tracing::Level::WARN
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let id = self.span_counter.fetch_add(1, Ordering::Relaxed);
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        // The tracing dispatcher only calls event() when enabled() returned true;
        // our enabled() accepts only WARN, so only WARN events should reach here.
        // See release-mode asymmetry note on `CONTRACT_VIOLATION_MARKER`.
        debug_assert_eq!(
            event.metadata().level(),
            &tracing::Level::WARN,
            "WarnCapturingSubscriber: event() reached with non-WARN — {}",
            CONTRACT_VIOLATION_MARKER
        );
        // Release ordering pairs with Acquire loads in assertion helpers and
        // WarnCapture::count(), ensuring all prior memory writes are visible.
        self.count.fetch_add(1, Ordering::Release);
        let mut visitor = MessageVisitor(String::new());
        event.record(&mut visitor);
        self.messages.lock().unwrap().push(visitor.0);
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tracing::Subscriber as _;

    use crate::warn_counting_subscriber;

    // ── ForwardingSubscriber ──────────────────────────────────────────────────

    /// A generic tracing subscriber that forwards all span bookkeeping to an
    /// inner subscriber and delegates `enabled()` and `event()` to caller-
    /// supplied closures.
    ///
    /// This eliminates the per-test boilerplate of repeating five identical
    /// forwarding methods (`new_span`, `record`, `record_follows_from`, `enter`,
    /// `exit`) every time a test needs to customise filtering or event handling.
    ///
    /// # Closure signatures
    ///
    /// - `EnabledFn(&S, &tracing::Metadata<'_>) -> bool` — receives a shared
    ///   reference to the inner subscriber so it can delegate if needed.
    /// - `EventFn(&S, &tracing::Event<'_>)` — same; can delegate to
    ///   `inner.event(event)` or perform custom side effects.
    ///
    /// External state (e.g. `Arc<AtomicUsize>` counters) is captured via
    /// `move` closures, consistent with the existing test patterns.
    struct ForwardingSubscriber<S, EnabledFn, EventFn>
    where
        S: tracing::Subscriber,
        EnabledFn: Fn(&S, &tracing::Metadata<'_>) -> bool + Send + Sync + 'static,
        EventFn: Fn(&S, &tracing::Event<'_>) + Send + Sync + 'static,
    {
        inner: S,
        enabled_fn: EnabledFn,
        event_fn: EventFn,
    }

    impl<S, EnabledFn, EventFn> tracing::Subscriber for ForwardingSubscriber<S, EnabledFn, EventFn>
    where
        S: tracing::Subscriber,
        EnabledFn: Fn(&S, &tracing::Metadata<'_>) -> bool + Send + Sync + 'static,
        EventFn: Fn(&S, &tracing::Event<'_>) + Send + Sync + 'static,
    {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            (self.enabled_fn)(&self.inner, metadata)
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
            (self.event_fn)(&self.inner, event)
        }

        fn enter(&self, span: &tracing::span::Id) {
            self.inner.enter(span)
        }

        fn exit(&self, span: &tracing::span::Id) {
            self.inner.exit(span)
        }
    }

    /// ERROR events should be rejected at the `enabled()` gate, not silently
    /// accepted and then discarded inside `event()`.
    ///
    /// We verify this by wrapping the real `WarnCountingSubscriber` in a thin
    /// `EventDispatchCounter` that increments `dispatch_count` each time the
    /// tracing framework calls `event()` on us.  Because the wrapper delegates
    /// `enabled()` to the inner subscriber, the tracing dispatcher only calls
    /// `event()` on the wrapper — and therefore on the inner — when the inner's
    /// `enabled()` returns `true`.  Our `enabled()` accepts only WARN, so an
    /// ERROR event is rejected at the gate and `dispatch_count` stays 0.
    #[test]
    fn error_events_rejected_by_enabled_filter() {
        let (inner, warn_count) = warn_counting_subscriber();
        let dispatch_count = Arc::new(AtomicUsize::new(0));
        let dispatch_count_clone = Arc::clone(&dispatch_count);

        // ForwardingSubscriber delegates enabled() to the inner subscriber so
        // its filter is exercised.  The event_fn increments dispatch_count
        // before delegating — it is only reached when enabled() returned true.
        let subscriber = ForwardingSubscriber {
            inner,
            enabled_fn: |s: &_, meta| s.enabled(meta),
            event_fn: move |s: &_, event| {
                dispatch_count_clone.fetch_add(1, Ordering::Relaxed);
                s.event(event);
            },
        };

        tracing::subscriber::with_default(subscriber, || {
            tracing::error!("error message");
        });

        // The dispatcher rejected the ERROR at enabled(), so event() was never
        // called on either wrapper or inner, and warn_count stays 0.
        assert_eq!(
            warn_count.load(Ordering::Acquire),
            0,
            "ERROR must not be counted as a WARN event"
        );

        // dispatch_count is 0 because the tracing dispatcher honoured the
        // enabled() rejection and never forwarded the ERROR event to event().
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
            warn_count.load(Ordering::Acquire),
            0,
            "counter should start at 0"
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("test warning");
        });

        assert_eq!(
            warn_count.load(Ordering::Acquire),
            1,
            "one WARN event should produce count=1"
        );
    }

    /// Non-WARN events (DEBUG, INFO, ERROR) do not affect the warn counter.
    ///
    /// Filtering is handled entirely at `enabled()`: it rejects non-WARN events
    /// before the tracing dispatcher ever calls `event()` on this subscriber,
    /// so the counter is never incremented for them.  `event()` itself no
    /// longer carries a runtime level check (see task 972); it relies on the
    /// dispatcher contract and a `debug_assert_eq!` backstop that panics in
    /// debug builds if the contract is violated.  This test validates
    /// end-to-end counting correctness — that the counter stays zero when only
    /// non-WARN events are emitted.
    ///
    /// See `error_events_rejected_by_enabled_filter` for the test that
    /// specifically validates the `enabled()` gate, and
    /// `event_panics_on_non_warn_when_dispatcher_contract_violated` for the
    /// debug-assert backstop.
    #[test]
    fn non_warn_events_are_not_counted() {
        let (subscriber, warn_count) = warn_counting_subscriber();

        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!("debug message");
            tracing::info!("info message");
            tracing::error!("error message");
        });

        assert_eq!(
            warn_count.load(Ordering::Acquire),
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

        assert_eq!(counter_clone.load(Ordering::Acquire), 2);
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
            warn_arc.load(Ordering::Acquire),
            0,
            "counter should start at 0"
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("test warning");
        });

        assert_eq!(
            warn_arc.load(Ordering::Acquire),
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
            warn_arc.load(Ordering::Acquire),
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
            debug_arc.load(Ordering::Acquire),
            1,
            "one DEBUG event should produce debug count=1"
        );
        assert_eq!(
            warn_arc.load(Ordering::Acquire),
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

        let dispatch_count = Arc::new(AtomicUsize::new(0));
        let dispatch_count_clone = Arc::clone(&dispatch_count);

        let (inner, _counters) = CountingSubscriberBuilder::new()
            .count_level(Level::WARN)
            .build();

        // ForwardingSubscriber delegates enabled() to the inner CountingSubscriber
        // (which only accepts WARN) and increments dispatch_count on event().
        let subscriber = ForwardingSubscriber {
            inner,
            enabled_fn: |s: &_, meta| s.enabled(meta),
            event_fn: move |s: &_, event| {
                dispatch_count_clone.fetch_add(1, Ordering::Relaxed);
                s.event(event);
            },
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

    /// Calling `event()` directly on `WarnCountingSubscriber` with a non-WARN
    /// event — bypassing the tracing dispatcher's `enabled()` gate — must panic
    /// loudly in debug builds rather than silently swallowing the event.
    ///
    /// We simulate this by wrapping the subscriber in a `ForwardingSubscriber`
    /// whose `enabled_fn` always returns `true`, causing the tracing framework to
    /// deliver an ERROR event directly into the inner subscriber's `event()`.
    /// The `debug_assert_eq!` inside `event()` detects the contract violation
    /// and panics with the full message:
    ///
    /// ```text
    /// event() reached with non-WARN — enabled() contract violated
    /// ```
    ///
    /// The `#[should_panic(expected = "enabled() contract violated")]` attribute
    /// performs a **substring match** against that message, using
    /// [`CONTRACT_VIOLATION_MARKER`] as the canonical anchor.  The test
    /// [`contract_violation_marker_matches_panic_expected`] enforces that the
    /// const value stays in sync with the literal in this attribute.
    ///
    /// # Release-build note
    ///
    /// `debug_assert_eq!` is compiled out in release builds, so the inner
    /// subscriber would silently accept the non-WARN event without panicking.
    /// The `#[cfg(debug_assertions)]` gate prevents the test from incorrectly
    /// failing under `#[should_panic]` in release mode.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "enabled() contract violated")]
    fn event_panics_on_non_warn_when_dispatcher_contract_violated() {
        let (inner, _warn_count) = warn_counting_subscriber();

        // enabled_fn ignores the inner subscriber and always returns true,
        // bypassing its WARN-only filter so non-WARN events reach inner.event().
        let subscriber = ForwardingSubscriber {
            inner,
            enabled_fn: |_s: &_, _meta| true,
            event_fn: |s: &_, event| s.event(event),
        };

        tracing::subscriber::with_default(subscriber, || {
            tracing::error!("non-WARN event delivered directly");
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

    // ── warn_capturing_subscriber tests ──────────────────────────────────────

    /// `WarnCapture::count()` returns the number of WARN events that were
    /// emitted while the capturing subscriber was active.
    #[test]
    fn warn_capturing_count_returns_warn_event_count() {
        use crate::warn_capturing_subscriber;

        let (subscriber, capture) = warn_capturing_subscriber();

        assert_eq!(capture.count(), 0, "count should start at 0");

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("first warning");
            tracing::warn!("second warning");
        });

        assert_eq!(capture.count(), 2, "count should be 2 after two WARN events");
    }

    /// `WarnCapture::messages()` captures the formatted text of each WARN event.
    #[test]
    fn warn_capturing_messages_captures_message_text() {
        use crate::warn_capturing_subscriber;

        let (subscriber, capture) = warn_capturing_subscriber();

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("hello from warn");
        });

        let msgs = capture.messages();
        assert_eq!(msgs.len(), 1, "should capture exactly one message");
        assert!(
            msgs[0].contains("hello from warn"),
            "captured message should contain 'hello from warn', got: {:?}",
            msgs[0]
        );
    }

    /// Non-WARN events are not counted and their messages are not captured.
    #[test]
    fn warn_capturing_ignores_non_warn_events() {
        use crate::warn_capturing_subscriber;

        let (subscriber, capture) = warn_capturing_subscriber();

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("info message");
            tracing::error!("error message");
            tracing::debug!("debug message");
        });

        assert_eq!(capture.count(), 0, "non-WARN events must not be counted");
        assert!(
            capture.messages().is_empty(),
            "non-WARN event messages must not be captured"
        );
    }

    /// Calling `event()` directly on `WarnCapturingSubscriber` with a non-WARN
    /// event — bypassing the tracing dispatcher's `enabled()` gate — must panic
    /// loudly in debug builds rather than silently capturing the event.
    ///
    /// Mirrors `event_panics_on_non_warn_when_dispatcher_contract_violated` but
    /// exercises `WarnCapturingSubscriber::event()`'s `debug_assert_eq!`.
    /// The `#[should_panic(expected = "enabled() contract violated")]` attribute
    /// uses [`CONTRACT_VIOLATION_MARKER`] as the canonical anchor substring.
    ///
    /// # Release-build note
    ///
    /// `debug_assert_eq!` is compiled out in release builds, so no panic would
    /// occur there.  The `#[cfg(debug_assertions)]` gate prevents this test from
    /// incorrectly failing under `#[should_panic]` in release mode.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "enabled() contract violated")]
    fn capturing_event_panics_on_non_warn_when_dispatcher_contract_violated() {
        use crate::warn_capturing_subscriber;

        let (inner, _capture) = warn_capturing_subscriber();

        // enabled_fn ignores the inner subscriber and always returns true,
        // bypassing its WARN-only filter so non-WARN events reach inner.event().
        let subscriber = ForwardingSubscriber {
            inner,
            enabled_fn: |_s: &_, _meta| true,
            event_fn: |s: &_, event| s.event(event),
        };

        tracing::subscriber::with_default(subscriber, || {
            tracing::error!("non-WARN event delivered directly");
        });
    }

    /// `WarnCapture::assert_count_and_any_message_contains` passes when the
    /// count matches and at least one message contains the given substring.
    #[test]
    fn warn_capture_assert_count_and_message_passes_on_match() {
        use crate::warn_capturing_subscriber;

        let (subscriber, capture) = warn_capturing_subscriber();

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("values RwLock poisoned, recovering: err");
        });

        // Should not panic
        capture.assert_count_and_any_message_contains(1, "values RwLock poisoned");
    }

    /// `WarnCapture::assert_count_and_any_message_contains` panics when no
    /// captured message contains the expected substring.
    #[test]
    #[should_panic(expected = "no WARN message contained")]
    fn warn_capture_assert_message_panics_on_missing_substring() {
        use crate::warn_capturing_subscriber;

        let (subscriber, capture) = warn_capturing_subscriber();

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("some other warning");
        });

        capture.assert_count_and_any_message_contains(1, "values RwLock poisoned");
    }

    /// `WarnCapture::assert_count_and_any_message_contains` panics when the
    /// count does not match, even if a message would otherwise match.
    #[test]
    #[should_panic(expected = "expected 2 WARN events")]
    fn warn_capture_assert_count_panics_on_wrong_count() {
        use crate::warn_capturing_subscriber;

        let (subscriber, capture) = warn_capturing_subscriber();

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("values RwLock poisoned, recovering");
        });

        capture.assert_count_and_any_message_contains(2, "values RwLock poisoned");
    }

    /// `tracing::warn!("msg")` sends `message` as `fmt::Arguments` through
    /// `record_debug`.  Because `fmt::Arguments`'s `Debug` impl delegates to
    /// `Display`, the captured text must equal the raw format string exactly —
    /// without any extra quotes or decorations — even after the `record_str`
    /// override was added.
    ///
    /// This is the regression guard for the `fmt::Arguments` path: the addition
    /// of `record_str` must not change the behaviour of the `record_debug` path.
    #[test]
    fn warn_capturing_format_args_message_unchanged() {
        use crate::warn_capturing_subscriber;

        let (subscriber, capture) = warn_capturing_subscriber();

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("format_args path");
        });

        let msgs = capture.messages();
        assert_eq!(msgs.len(), 1, "should capture exactly one message");
        assert_eq!(
            msgs[0], "format_args path",
            "fmt::Arguments message must be captured without extra formatting; got: {:?}",
            msgs[0]
        );
    }

    /// When the `message` field is a `&str` (e.g. `tracing::warn!(message =
    /// "literal")`), the captured text must equal the raw string exactly —
    /// without the surrounding double-quotes that `{value:?}` (Debug) would
    /// add for a `&str`.
    ///
    /// This is the failing test for the `record_str` fix: before the override
    /// is added, `record_str`'s default falls back to `record_debug`, which
    /// formats `&str` with `{:?}` and produces `"literal"` (with quotes) rather
    /// than `literal`.
    #[test]
    fn warn_capturing_str_field_has_no_debug_quotes() {
        use crate::warn_capturing_subscriber;

        let (subscriber, capture) = warn_capturing_subscriber();

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(message = "direct string value");
        });

        let msgs = capture.messages();
        assert_eq!(msgs.len(), 1, "should capture exactly one message");
        assert_eq!(
            msgs[0], "direct string value",
            "captured message must be the raw string without Debug quotes; got: {:?}",
            msgs[0]
        );
    }

    // ── ForwardingSubscriber tests ────────────────────────────────────────────

    /// `ForwardingSubscriber` correctly delegates `enabled()` to the closure.
    ///
    /// Wraps a `warn_counting_subscriber` (which accepts only WARN) in a
    /// `ForwardingSubscriber` whose `enabled_fn` delegates to `inner.enabled()`
    /// and whose `event_fn` delegates to `inner.event()`.  Emits one WARN and
    /// one ERROR event; asserts the inner counter reads 1 — the ERROR was
    /// rejected at the `enabled()` gate and never reached `event()`.
    #[test]
    fn forwarding_subscriber_delegates_enabled_to_closure() {
        let (inner, warn_count) = warn_counting_subscriber();

        let subscriber = ForwardingSubscriber {
            inner,
            enabled_fn: |s: &_, meta| s.enabled(meta),
            event_fn: |s: &_, event| s.event(event),
        };

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("warn event");
            tracing::error!("error event — should be rejected at enabled()");
        });

        assert_eq!(
            warn_count.load(Ordering::Acquire),
            1,
            "only the WARN event should be counted; ERROR must be rejected at enabled()"
        );
    }

    /// `ForwardingSubscriber` correctly delegates `event()` to the closure.
    ///
    /// Creates a `ForwardingSubscriber` whose `enabled_fn` always returns `true`
    /// and whose `event_fn` increments a shared counter without delegating to
    /// the inner subscriber.  Emits one WARN event; asserts the counter is 1.
    /// This validates that `event()` is driven by the closure, not hardcoded.
    #[test]
    fn forwarding_subscriber_delegates_event_to_closure() {
        let (inner, _warn_count) = warn_counting_subscriber();
        let custom_count = Arc::new(AtomicUsize::new(0));
        let custom_count_clone = Arc::clone(&custom_count);

        let subscriber = ForwardingSubscriber {
            inner,
            enabled_fn: |_s: &_, _meta| true,
            event_fn: move |_s: &_, _event| {
                custom_count_clone.fetch_add(1, Ordering::Relaxed);
            },
        };

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("warn event");
        });

        assert_eq!(
            custom_count.load(Ordering::Relaxed),
            1,
            "custom event_fn must be called exactly once for the WARN event"
        );
    }

    /// Verifies that `CONTRACT_VIOLATION_MARKER` matches the substring used in
    /// `#[should_panic(expected = ...)]` on the two contract-violation tests:
    ///
    /// - `event_panics_on_non_warn_when_dispatcher_contract_violated`
    ///   (`WarnCountingSubscriber`)
    /// - `capturing_event_panics_on_non_warn_when_dispatcher_contract_violated`
    ///   (`WarnCapturingSubscriber`)
    ///
    /// # Sync requirement
    ///
    /// `#[should_panic(expected = ...)]` requires a string literal; it cannot
    /// reference a const.  This test acts as the compile-time link: if the const
    /// value ever drifts from the literal in either attribute, this assertion will
    /// fail loudly before the drift can go unnoticed.
    #[test]
    fn contract_violation_marker_matches_panic_expected() {
        assert_eq!(
            super::CONTRACT_VIOLATION_MARKER,
            "enabled() contract violated",
            "CONTRACT_VIOLATION_MARKER must match the #[should_panic(expected = ...)] literal"
        );
    }

    // ── assert_warn_count_delta tests ─────────────────────────────────────────

    /// `assert_warn_count_delta` passes when the counter advanced by exactly
    /// `expected_delta` since the `before` snapshot.
    ///
    /// (a) After 2 WARN events with `before=0`, delta==2 → passes.
    /// (b) After 2 WARN events with `before=1`, delta==1 → passes (only the
    ///     increment since the snapshot counts).
    #[test]
    fn assert_warn_count_delta_passes_on_correct_delta() {
        use crate::assert_warn_count_delta;
        use crate::warn_counting_subscriber;

        let (subscriber, counter) = warn_counting_subscriber();

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("first warn");
            tracing::warn!("second warn");
        });

        // (a) before=0, delta=2: the counter went from 0 to 2
        assert_warn_count_delta(&counter, 0, 2, "two warns from zero");

        // Synthesise a mid-test snapshot: counter is at 2, before=1 means delta=1
        assert_warn_count_delta(&counter, 1, 1, "delta since snapshot at 1");
    }

    /// `assert_warn_count_delta` panics when the actual delta does not match.
    #[test]
    #[should_panic(expected = "expected warn delta")]
    fn assert_warn_count_delta_panics_on_wrong_delta() {
        use crate::assert_warn_count_delta;
        use crate::warn_counting_subscriber;

        let (subscriber, counter) = warn_counting_subscriber();

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("only one warn");
        });

        // counter is at 1, but we claim delta of 2 from before=0 → should panic
        assert_warn_count_delta(&counter, 0, 2, "expected warn delta");
    }

    /// Verifies `assert_warn_count_delta` panics with the `warn counter went
    /// backwards` message when `before` exceeds the current counter, which
    /// catches stale-snapshot bugs that the old `saturating_sub` implementation
    /// silently swallowed.
    #[test]
    #[should_panic(expected = "warn counter went backwards")]
    fn assert_warn_count_delta_panics_when_counter_went_backwards() {
        use crate::assert_warn_count_delta;
        use crate::warn_counting_subscriber;

        // Obtain a fresh counter at 0 — do NOT install the subscriber or emit
        // any warns.  The counter stays at 0.
        let (_subscriber, counter) = warn_counting_subscriber(); // subscriber intentionally unused — we only need the zero-valued counter handle

        // Passing before=5 against a counter at 0 is a backwards snapshot.
        // This must panic with "warn counter went backwards"; if it silently
        // returns 0 the should_panic expectation fails and this test is red.
        assert_warn_count_delta(&counter, 5, 0, "stale snapshot");
    }

    /// `assert_warn_count` (convenience wrapper with before=0) passes when
    /// counter equals `expected`.
    #[test]
    fn assert_warn_count_passes_on_correct_count() {
        use crate::assert_warn_count;
        use crate::warn_counting_subscriber;

        let (subscriber, counter) = warn_counting_subscriber();

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("one warn");
        });

        assert_warn_count(&counter, 1, "exactly one warn");
    }

    // ── warn_counting_guard tests ─────────────────────────────────────────────

    /// `warn_counting_guard()` installs a WARN-counting subscriber as the
    /// thread-default and returns a live guard + shared counter.
    ///
    /// While the guard is in scope, `tracing::warn!` events are counted.
    /// After the guard drops, the subscriber is removed.
    #[test]
    fn warn_counting_guard_captures_warn_events() {
        use crate::assert_warn_count;
        use crate::warn_counting_guard;

        let (_guard, counter) = warn_counting_guard();

        tracing::warn!("from guard subscriber");

        assert_warn_count(&counter, 1, "guard must count the WARN event");
    }

    /// `warn_counting_guard()` does not count WARN events after the guard drops.
    ///
    /// Once the guard is dropped the subscriber is removed, so events emitted
    /// outside the guard's lifetime are not reflected in the counter.
    ///
    /// The post-drop `tracing::warn!` below falls through to the global no-op
    /// fallback and leaves the counter at 1.  If the guard ever leaked its
    /// subscriber, that warn would be captured and bump the counter to 2,
    /// causing this assertion to fail.
    #[test]
    fn warn_counting_guard_stops_counting_after_drop() {
        use crate::assert_warn_count;
        use crate::warn_counting_guard;

        let (guard, counter) = warn_counting_guard();
        tracing::warn!("inside guard");
        drop(guard);
        // Emit a warn AFTER the guard drops.  The subscriber is now detached
        // so this event must NOT increment the counter.  If the subscriber had
        // leaked, the counter would reach 2 and the assertion below would catch
        // the regression.
        tracing::warn!("after drop");
        assert_warn_count(
            &counter,
            1,
            "post-drop warn must not be counted (subscriber should be detached)",
        );
    }
}
