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

/**
 * The ASCII normal form of a CURATED unit label (task #6028).
 *
 * Rewrites the two superscript exponent glyphs that appear in the curated
 * ladder tables — U+00B2 -> `^2`, U+00B3 -> `^3` — and touches nothing else.
 *
 * WHY it exists: task #5788 relabels the curated tables from the superscript
 * spelling (`mm³`) to the ASCII spelling (`mm^3`) per its contract C2 —
 * "accept what we cannot enumerate; normalize what we curate". A label
 * compared by raw string equality across that relabel stops matching, so a
 * user's persisted unit preference would silently snap back to the ladder
 * default. Normalizing BOTH sides before comparing makes the comparison
 * survive the relabel in EITHER direction, which is what lets #6028 land
 * before or after #5788 without an ordering hazard.
 *
 * It deliberately does NOT touch the `×10ⁿ` engineering-notation superscript
 * digits produced by `reify-ir`'s value formatting (task #5788 PRD §10 /
 * addendum L3): those format the MAGNITUDE, not the unit, and are explicitly
 * out of scope. This is a pure glyph substitution with no exceptions, so it
 * must only ever be handed a unit label.
 */
export function normalizeUnitLabel(label: string): string {
  return label.replace(/²/g, '^2').replace(/³/g, '^3');
}

/**
 * The unit labels the typed-quantity gate accepts when no ladder data is
 * available (task #6028) — the five that were hard-coded into
 * `PropertyEditor`'s `QUANTITY_RE` before the alphabet became ladder-derived.
 *
 * This is a FLOOR, not the alphabet: `get_unit_ladders` is a best-effort
 * one-shot fetch that can fail (`App.test.tsx` pins a toast for that path),
 * and validation must not silently narrow when it does. Keeping exactly these
 * five also keeps every ladder-less caller byte-identical to the pre-#6028
 * behaviour.
 */
export const BASE_UNIT_LABELS: readonly string[] = ['mm', 'cm', 'm', 'deg', 'rad'];

/**
 * The unit alphabet the typed-quantity gate accepts, derived from the LIVE
 * ladders unioned with {@link BASE_UNIT_LABELS} (task #6028).
 *
 * WHY derived rather than a hand-written mirror of the Rust curated table: a
 * TS copy would be a FIFTH curated label table, which is the exact drift
 * defect task #5788 decision D6 exists to prevent. Deriving it from the data
 * the backend already advertises means this needs no follow-up edit when
 * #5788 relabels — the alphabet just follows.
 *
 * WHY the labels are normalized here: the typed-input gate should accept what
 * the **.ri grammar** accepts, and superscript spellings have never been
 * parseable (task #5788 probe evidence). So `mm^3` becomes accepted while
 * `mm³` stays rejected, in both the pre- and post-#5788 eras.
 *
 * Note the accepted transient while #5788 is unlanded: with ladders loaded,
 * `L` passes this gate but the backend still answers `error: unknown unit: L`
 * (#5788 is what adds `pub unit L : Volume`). The gate is advisory — the
 * backend remains the validator — so the degradation is a backend diagnostic
 * instead of an inline `data-invalid`.
 */
export function quantityUnitAlphabet(ladders: UnitLadderMap | undefined): string[] {
  const alphabet = new Set<string>(BASE_UNIT_LABELS);
  for (const options of Object.values(ladders ?? {})) {
    for (const opt of options) {
      alphabet.add(normalizeUnitLabel(opt.label));
    }
  }
  return [...alphabet];
}

/**
 * Escape every regex metacharacter in a unit label so it matches literally.
 *
 * `^` is the one that matters and the one that bites: inside an alternation
 * branch an unescaped `^` is a START-OF-INPUT ANCHOR, so a `mm^3` branch would
 * compile to "mm, then start-of-input, then 3" — a perfectly valid regex that
 * can never match. Nothing throws and nothing warns; the gate just silently
 * rejects every `mm^3` a user types. `/` matters too, for compound labels like
 * `kg/m^3`.
 */
function escapeForRegex(literal: string): string {
  return literal.replace(/[.*+?^${}()|[\]\\/-]/g, '\\$&');
}

/** The signed numeric literal of a quantity: sign, mantissa, optional exponent. */
const QUANTITY_NUMBER = '-?(?:\\d+\\.?\\d*|\\.\\d+)(?:[eE][+-]?\\d+)?';

/**
 * Build the typed-quantity regex for a unit alphabet (task #6028) — the ONE
 * definition of the quantity grammar in the frontend. Before this, the
 * five-unit alternation was written four times across
 * `PropertyEditor.tsx` and `PropertyEditor.test.tsx`, each copy carrying its
 * own obligation to stay in sync.
 *
 * **Capture group 1 is the whole signed numeric literal** — sign, mantissa and
 * exponent. That is what collapses the old pair of regexes into one: the
 * caller reads `m[1]` for the overflow check instead of re-declaring the
 * alternation to strip the unit suffix.
 *
 * Labels are sorted longest-first with a lexicographic tiebreak purely for
 * DETERMINISM — the output must not depend on ladder iteration order. Match
 * correctness itself comes from the `$` anchor, not from branch order.
 *
 * Grammar notes, all deliberate and all inherited from the regex this
 * replaces: no whitespace is allowed between the number and the unit,
 * mirroring the .ri grammar's `token.immediate`
 * (`tree-sitter-reify/grammar.js`) — the backend's `parse_value_string` is
 * more lenient (it accepts `"5 mm"`) but the frontend intentionally enforces
 * the stricter rule. A leading `+` is rejected because the grammar defines
 * only unary minus for number literals, even though the exponent does accept
 * a sign (`1e+3mm` is valid). There is no numeric range check, so callers must
 * still guard with `Number.isFinite(Number(m[1]))`.
 */
export function buildQuantityRe(labels: readonly string[]): RegExp {
  const ordered = [...labels].sort((a, b) => b.length - a.length || (a < b ? -1 : a > b ? 1 : 0));
  const alternation = ordered.map(escapeForRegex).join('|');
  return new RegExp(`^(${QUANTITY_NUMBER})(?:${alternation})$`);
}

/** Look up the selectable unit ladder for a canonical dimension name. */
export function ladderForDimension(
  map: UnitLadderMap,
  dimension: string,
): UnitOption[] | undefined {
  return map[dimension];
}
