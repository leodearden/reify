# Per-Channel Event Spec: `eval-generation`

> **Source:** task 7853 — stamp whole-state command replies with the `EvalQueue` publish generation.
>
> **Inventory row:** [`docs/gui-event-channels.md`](../gui-event-channels.md) §1.

---

## 1. Channel name + Rust + TS file/symbol locations

- **Channel:** `eval-generation`
- **Rust emit site (announcement):** `gui/src-tauri/src/eval_queue.rs` — `EvalQueue::process` calls `EvalQueue::announce(generation)` → `EvalObserver::started(generation)` before running any entry that carries a generation.
- **Rust emit site (transport):** `gui/src-tauri/src/main.rs` — `TauriEvalObserver::started()` (calls `event_bus::emit_typed(&self.app, "eval-generation", &EvalGeneration { generation })`).
- **TS listen site:** `gui/src/bridge.ts` — `onEvalGeneration(callback: (generation: number) => void): Promise<UnlistenFn>`

---

## 2. Payload Rust struct + TS interface

Field names match exactly (§3.2 — no `#[serde(rename_all)]`).

```rust
/// The `eval-generation` payload: an edit or evaluation of `generation` is
/// about to run (mirrors frontend EvalGeneration interface).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalGeneration {
    pub generation: u64,
}
```

Source: `gui/src-tauri/src/types.rs` — `EvalGeneration`.

```typescript
export interface EvalGeneration {
  generation: number;
}
```

Source: `gui/src/types.ts` — `EvalGeneration`.

The same number stamps the whole-state replies of `get_initial_state` and
`open_file_engine`: both reply `PublishedState {generation, state}`
(`gui/src-tauri/src/eval_queue.rs::PublishedState`, serialized as
`{"generation": u64, "state": GuiState}`; TS `PublishedState` /
`RawPublishedState` in `gui/src/types.ts`). The queue is the only constructor of
a `PublishedState`, and its `state` is exactly the snapshot the queue published
under `generation`.

---

## 3. Producer site(s) and emission triggers

- **Emit site:** `gui/src-tauri/src/eval_queue.rs` — `EvalQueue::process`, on the queue's single drainer, before `entry.into_job().run(..)`.
- **Trigger:** an edit or evaluation (including an `EvalRequest::snapshot` whole-state request) is about to run. Engine calls carry no generation and announce nothing; an edit superseded before it runs, or refused by the ledger as late, never runs and announces nothing.
- **Frequency:** once per edit/evaluation that actually runs. Generations are issued at enqueue by `QueueState::issue_generation`, start at 1, and are announced in strictly increasing order.
- **Ordering contract:** everything an entry makes the frontend see follows its announcement. That covers its delta, and the events its engine emits from inside the job (for example `fea-diagnostics-changed` / `fea-convergence-changed`). Everything an older generation caused precedes it. This holds because only the single drainer calls `process`, and all events travel one FIFO event stream.

---

## 4. Consumer site(s) and unlisten lifecycle owner

- **bridge.ts wrapper:** `gui/src/bridge.ts` — `onEvalGeneration(callback): Promise<UnlistenFn>`. It uses the inline structural-shape-guard idiom (`listen<unknown>` + `isPlainObject` + `typeof p['generation'] === 'number'`), the same as `onFeaCaseChanged` / `onAutoResolveIteration`.
- **Subscribing store:** `gui/src/stores/engineStore.ts` — `subscribeToEvents` registers `onEvalGeneration(noteGeneration)`. `noteGeneration` raises the store's closure-local, non-reactive newest generation and never lowers it.
- **Unlisten lifecycle owner:** the cleanup function returned by `engineStore.subscribeToEvents`, which unlistens every channel it registered.
- **Subscription pattern:** global, via `engineStore` (PRD §7 convention).
- **Frontend rule:** `engineStore.applyPublishedState(published)` writes a whole-state reply's fields iff `published.generation >= newest`, where newest is the maximum of every announced generation and every applied snapshot's generation; applying the reply sets newest to its generation. A stale reply writes no snapshot field. Either way `onEngineReinitialized` fires, so the entity tree and mechanisms still refresh after an open. `App.tsx` routes both File→Open/New (`loadPathIntoStores`) and `initApp` through `applyPublishedState`.
- **Auto-resolve rule:** `autoResolve` is event-owned — the engine fires the whole `auto-resolve-start` / `-iteration` / `-complete` trio from inside each re-eval — so a reply clears it by generation, not by freshness. `beginAutoResolveLoop` records the newest generation when a loop begins, which is the generation whose re-eval fired it. A reply of generation `g` clears the loop iff an older generation began it (the previous file's loop). A loop that `g` itself or a newer generation began is kept, stale reply or not: it describes the reply's state or a newer one.
- **Why this is sound:** every event of generation `h` is emitted after `announce(h)`, and a snapshot's own delta (same generation) is a subset of the snapshot. A stale delta that arrives after a newer snapshot re-converges, because each item is a full value, so deltas need no guard.
- **Known limitation:** debug-server whole-state pushes go through `gui/src/debug/bridge.ts` → `engineStore.initFromState`. They happen outside the queue, carry no generation, and are not fenced. `initFromState` stays the unconditional path for them.

---

## 5. Versioning policy

Default per PRD §3.3 (no `version` field).

---

## 6. Error semantics

Default per PRD §5:

- **Malformed payload:** `console.warn('[eval-generation] malformed payload; dropping event', p)` + drop in `onEvalGeneration` (inline shape guard, see §4). Covered payloads: a non-object, a missing `generation`, and a non-number `generation`.
- **Emit failure:** `tracing::warn!` and continue. `TauriEvalObserver::started` logs `"eval-generation emit failed: {}"`, and the queue keeps running.
- **Observer panic:** `EvalQueue::notify` contains a panicking `started`, as it does for `activity`. The entry still runs, publishes and replies.

---

## 7. Test pointers

> Test pointers use symbol/function-name anchors rather than absolute line numbers (line ranges drift; symbol names are stable). Grep the cited file for the function name.

- **Rust serde roundtrip test:** `gui/src-tauri/src/tests/types_tests.rs` — `eval_generation_serializes_to_expected_json_shape`: `EvalGeneration { generation: 7 }` serializes to exactly `{"generation":7}` and round-trips (PRD §6.1 gate).
- **Rust announcement tests:** `gui/src-tauri/src/tests/eval_queue_tests.rs`
  - `each_edit_and_evaluation_announces_its_generation_before_its_delta`
  - `superseded_and_late_edits_announce_nothing`
  - `an_entry_is_announced_before_its_job_runs`
  - `an_observer_that_panics_on_started_does_not_stop_the_queue`
- **Rust snapshot/reply-stamp tests:** `gui/src-tauri/src/tests/eval_queue_tests.rs`
  - `a_snapshot_replies_the_state_it_published_stamped_with_its_generation`
  - `a_snapshot_is_announced_before_its_job_runs_under_the_generation_it_replies`
  - `a_failed_snapshot_replies_err_and_publishes_nothing`
  - `a_panicking_snapshot_replies_err_and_the_queue_carries_on`
  - `a_published_state_serializes_as_a_generation_and_state_envelope`
- **Rust command tests:** `gui/src-tauri/src/tests/commands_tests.rs` (`mod queued_requests`) — `a_whole_state_reply_queued_behind_an_edit_is_stamped_later_than_the_edit`; `gui/src-tauri/src/tests/main_helpers_tests.rs` — `begin_initial_file_load_is_served_before_a_later_initial_state_request`.
- **TS bridge shape test:** `gui/src/__tests__/bridge/evalGeneration.test.ts` — happy path plus malformed-payload drops with `console.warn` mentioning `eval-generation` (§6.2 + §6.3 gate).
- **Store guard tests:** `gui/src/__tests__/engineStore.test.ts` — `describe('engineStore publish-generation guard')`, including the auto-resolve rule (`a stale reply keeps the auto-resolve loop a newer generation began`, `a reply keeps the auto-resolve loop its own generation began`, `a reply clears an auto-resolve loop an older generation began, even when it is stale`).
- **App tests:** `gui/src/__tests__/App.test.tsx` — `describe('App whole-state replies honour the publish generation')`.
