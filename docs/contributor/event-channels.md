# Adding Tauri Event Channels

This note covers what to do when you add a new Tauri event channel between the Rust backend and the TypeScript frontend. The **canonical, machine-grep-friendly source of truth** for all registered channel names is [`docs/gui-event-channels.md`](../gui-event-channels.md). The source PRD that owns the convention is [`docs/prds/v0_3/gui-event-channel-inventory.md`](../prds/v0_3/gui-event-channel-inventory.md) §3.

## Convention digest

- **Names**: kebab-case ASCII only (e.g. `mesh-update`, `evaluation-status`). One channel per logical event family; lifecycle trios (start / iteration / complete) use sibling channels.
- **Lockstep commits**: The Rust `app.emit("name", …)` call and the TypeScript `listen<T>("name", cb)` subscription must land in the **same commit**. Splitting them creates a window where one side fires into a void.
- **Payload shape**: Declare the payload type in `reify_gui::types` (Rust) and `gui/src/types.ts` (TypeScript), field-for-field. Hand-shaped payloads must call `validatePayload(name, payload, REQUIRED_KEYS_ARRAY)` on the TypeScript side.
- **Consumer tests**: Use the `mockTauriEvent` utility in `gui/src/__tests__/test_utils/mockEvents.ts` to drive event listeners in unit tests — do not fake `window.__TAURI__` by hand.

## When you add a channel

1. Add `app.emit("your-channel", payload)` in Rust and `listen<YourPayload>("your-channel", cb)` in TypeScript.
2. Add a row to `docs/gui-event-channels.md` **and** to `docs/prds/v0_3/gui-event-channel-inventory.md` §2 in the same commit (lockstep with the code change).
3. Run `scripts/check_event_inventory.sh` locally to confirm no orphans before pushing.

## Lint script

`scripts/check_event_inventory.sh` greps `gui/src-tauri/` for literal `.emit("name", …)` call sites and warns if any name is absent from `docs/gui-event-channels.md`. It exits 0 by default (warning mode per [PRD §11 Q4](../prds/v0_3/gui-event-channel-inventory.md)). The `--strict` flag promotes it to exit 1, enabling future CI enforcement once a release cycle of drift observation has passed.

Dynamic emit-sites (`app.emit(&name, …)`) are intentionally skipped by the forward pass — their channel names are validated by the lockstep-commit convention, not by this regex lint.

Pass `--bidirectional` to also run a reverse pass: for each channel registered in §1 of the inventory, the script verifies that a quoted string literal `"channel-name"` appears somewhere in `gui/src-tauri/**/*.rs`. The scan is permissive (not restricted to `.emit("…")` form), so dynamic-emit channels whose names appear as `.to_string()` or `emitter("…")` literals are naturally covered without a hardcoded allowlist. The reverse pass is scoped to §1 only — §2 (FICTION → WIRED) rows are pre-implementation and would produce phantom-channel noise; they are excluded until they graduate to §1 (per esc-3552-52).
