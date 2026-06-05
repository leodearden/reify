/**
 * Compile-time type-contract file for DebugStores.
 * This file is NOT executed — it only needs to pass `tsc --noEmit`.
 *
 * Load-bearing assertion: DebugStores.viewState must be EXACTLY ViewStateStore
 * (Equals<> fails if it becomes a narrow subset).
 * Drift guards: the real stores must remain assignable to DebugStores, catching
 * any future rename or removal of a required member.
 */
import type { DebugStores } from '../debug/types';
import type { ViewStateStore } from '../stores/viewStateStore';
import { createSelectionStore } from '../stores/selectionStore';
import { createViewStateStore } from '../stores/viewStateStore';

// Equals<A,B> / AssertTrue<T> helpers (copied from types.typecheck.ts:288-290)
type Equals<A, B> =
  (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false;
type AssertTrue<T extends true> = T;

// Drift guard: real selection store must remain assignable to DebugStores.selection.
// Catches future renames/removals of required members (clearSelection, toggleSelect, …).
declare const realSel: ReturnType<typeof createSelectionStore>;
const _selAssign: DebugStores['selection'] = realSel;
void _selAssign;

// DebugStores.viewState must be EXACTLY ViewStateStore (not a narrow subset).
// This is the key structural invariant — fails if viewState drifts to a Pick<>.
type _VsExact = AssertTrue<Equals<DebugStores['viewState'], ViewStateStore>>;

// Drift guard: real viewState store must remain assignable to DebugStores.viewState.
// Catches future ViewStateStore member changes that would break App.tsx:1127.
declare const realVs: ReturnType<typeof createViewStateStore>;
const _vsAssign: DebugStores['viewState'] = realVs;
void _vsAssign;
