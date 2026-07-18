/**
 * Per-cell display-unit ladder helpers (task #5199).
 *
 * Pure functions supporting the Parameters panel's per-cell unit picker:
 * converting a canonical SI magnitude into a chosen display unit, formatting
 * the result without float noise, and looking up the selectable unit ladder
 * for a cell's dimension. The ladder data itself is authoritative in Rust
 * (`reify_core::display_units`, re-exported through
 * `reify_gui::display_units`) and fetched once via the `get_unit_ladders`
 * Tauri command — these helpers only consume it.
 */

import type { UnitLadderMap, UnitOption } from '../types';

/**
 * Format a number for display, mirroring the Rust `format_display_number`
 * whole-number path plus a 12-significant-figure trim for everything else.
 *
 * The trim kills float noise from unit conversion (e.g. division by
 * `si_scale`) the same way task #5198 kills it on the backend's default-unit
 * path: round-tripping through `toPrecision(12)` discards noise beyond the
 * 12th significant figure, then `Number(...)` + `String(...)` drops
 * trailing zeros (`"0.300000000000"` -> `0.3` -> `"0.3"`).
 *
 * Non-finite values (`Infinity`, `NaN`) fall back to `String(v)` verbatim.
 */
export function formatDisplayNumber(v: number): string {
  if (!Number.isFinite(v)) return String(v);
  if (Number.isInteger(v) && Math.abs(v) < 1e15) {
    return String(Math.trunc(v));
  }
  return String(Number(v.toPrecision(12)));
}

/** Convert a canonical SI magnitude into a chosen display unit's magnitude. */
export function convertToUnit(siValue: number, siScale: number): number {
  return siValue / siScale;
}

/** Look up the selectable unit ladder for a canonical dimension name. */
export function ladderForDimension(
  map: UnitLadderMap,
  dimension: string,
): UnitOption[] | undefined {
  return map[dimension];
}
