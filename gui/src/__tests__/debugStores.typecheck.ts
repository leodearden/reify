/**
 * Compile-time type-contract file for DebugStores.
 * This file is NOT executed — it only needs to pass `tsc --noEmit`.
 * It verifies that DebugStores.selection exposes clearSelection/toggleSelect,
 * and that DebugStores.viewState is exactly ViewStateStore.
 */
import type { DebugStores } from '../debug/types';
import type { ViewStateStore } from '../stores/viewStateStore';
import { createSelectionStore } from '../stores/selectionStore';
import { createViewStateStore } from '../stores/viewStateStore';

// Equals<A,B> / AssertTrue<T> helpers (copied from types.typecheck.ts:288-290)
type Equals<A, B> =
  (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false;
type AssertTrue<T extends true> = T;

// (1) clearSelection must be a required member of DebugStores.selection
const _cs: DebugStores['selection']['clearSelection'] = () => {};
void _cs;

// (2) toggleSelect must be a required member of DebugStores.selection
const _ts: DebugStores['selection']['toggleSelect'] = (_p: string) => {};
void _ts;

// (3) Drift guard — real selection store must be assignable to DebugStores.selection.
// Compiles both before and after the fix (assignment widening; structural subtyping).
declare const realSel: ReturnType<typeof createSelectionStore>;
const _selAssign: DebugStores['selection'] = realSel;
void _selAssign;

// (4) DebugStores.viewState must be EXACTLY ViewStateStore (not a narrow subset).
type _VsExact = AssertTrue<Equals<DebugStores['viewState'], ViewStateStore>>;

// (5) Drift guard — real viewState store must be assignable to DebugStores.viewState.
// Compiles both before and after the fix (assignment widening).
declare const realVs: ReturnType<typeof createViewStateStore>;
const _vsAssign: DebugStores['viewState'] = realVs;
void _vsAssign;

// (6) switchView must be reachable through DebugStores.viewState
const _sw: DebugStores['viewState']['switchView'] = () => false;
void _sw;
