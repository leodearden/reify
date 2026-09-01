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
 *
 * SOURCE OF TRUTH: `gui/src-tauri/src/engine.rs::normalize_unit_label` is
 * this function's same-shape Rust twin — the same total substitution over
 * the same two glyphs — and its doc block is the canonical account of the
 * cross-language contract: two mirror-image goldens, one per side (this
 * file's test block is the TypeScript one), that leave an accidental
 * one-sided drift uncaught.
 *
 * `reify_core::display_units::ascii_label_spelling`
 * (crates/reify-core/src/display_units.rs) separately owns contract C2's
 * underlying U+00B2/U+00B3 mapping rule (it returns `Option<String>`, a
 * different shape). Both describe the curated ladders served to this file
 * over the `get_unit_ladders` Tauri command; the duplication itself is
 * unavoidable because TypeScript cannot call across the language boundary.
 *
 * The gate that fires when the curated alphabet grows a glyph — e.g. the
 * `·` separator half, leaf κ of
 * docs/prds/v0_6/angle-units-surface-convergence.md (#5784) — is
 * `curated_unit_labels_carry_no_glyph_outside_the_shared_normalizer_alphabet`
 * (gui/src-tauri/src/tests/engine_tests.rs), which sweeps the live tables
 * and names this function in its failure message.
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
 * The DIMENSIONS {@link BASE_UNIT_LABELS} can express — its other half, and the
 * floor {@link acceptsBareNumber} gates on (task #5757 amendment).
 *
 * `mm`/`cm`/`m` are Length, `deg`/`rad` are Angle; that is the whole content of
 * this constant, and it must be re-derived by hand if the label floor above ever
 * changes. It cannot be computed from the labels, because mapping a label to a
 * dimension is exactly what needs the ladder data the ladder-less path does not
 * have.
 *
 * WHY A FLOOR IS NEEDED HERE AT ALL. The two ends of this seam degrade
 * differently when `get_unit_ladders` has not resolved (or has failed — see
 * `App.tsx`'s one-shot fetch, which logs, toasts, and leaves the map `{}`). This
 * side loses the ladder map entirely; the ENGINE does not, because its
 * `LADDER_COVERAGE` is built from the Rust-authored curated table in-process and
 * is always populated. So a rule that reads only the fetched map disagrees with
 * the engine on that path — and for the dimensions this floor names, in the
 * harmful direction: the panel would accept `80` in a Length cell, the engine
 * would refuse it, and the typed text would be discarded behind exactly the
 * async toast {@link acceptsBareNumber} exists to avoid.
 *
 * It is a floor in the same sense as the labels: `Length`/`Angle` are the
 * dimensions the panel can still describe with no ladder data, so gating them is
 * gating precisely what it can still tell the user how to fix. The backend's
 * coverage is a strict superset of it, pinned by
 * `every_dimension_the_frontend_floor_gates_is_gated_here_too` in
 * `gui/src-tauri/src/tests/engine_tests.rs`.
 *
 * Naming DIMENSIONS is not the #5788 D6 hazard that naming curated unit labels
 * would be: these are two canonical dimension names, not a mirror of the curated
 * rung table, and they follow the hand-written floor above rather than the
 * ladders.
 */
export const BASE_UNIT_DIMENSIONS: readonly string[] = ['Length', 'Angle'];

/**
 * The unit alphabet the typed-quantity gate accepts, derived from the LIVE
 * ladders unioned with {@link BASE_UNIT_LABELS} (task #6028).
 *
 * THE CANONICAL RATIONALE FOR THE WHOLE SEAM LIVES HERE. Callers
 * (`quantityReFor` in `../panels/PropertyEditor.tsx`, the `App.test.tsx`
 * block "ladder-derived units the backend cannot parse") point at this block
 * rather than restating it — three copies of the same argument, each naming
 * its own Rust line numbers, was three sets of rot sites with no gate to catch
 * them. For the same reason everything below names Rust SYMBOLS, never line
 * numbers, and describes sets by RULE, never by enumeration: a hand-mirrored
 * snapshot of the curated table in comment form is the very drift defect this
 * function exists to avoid in data.
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
 * WHAT "ADVERTISED" NOW MEANS — the gap this used to document is CLOSED
 * (task #5757). Until then the commit path — `handleSetParameter` (App.tsx) ->
 * `bridge.setParameter` -> `EngineSession::set_parameter` ->
 * `parse_value_string` (both in gui/src-tauri/src/engine.rs) — matched a
 * hard-coded five-entry suffix table whose entries were exactly
 * {@link BASE_UNIT_LABELS}, so every curated label outside that floor was
 * admitted here and then refused on commit with `Cannot parse value '<input>'`,
 * discarding the typed text behind an async toast.
 *
 * `parse_value_string` now scans an index composed from THIS SAME
 * `unit_ladders()` table unioned with `reify_core::BUILTIN_UNITS`, so the two
 * ends are two readers of one table rather than two tables kept in lockstep.
 * The consequence that matters HERE: that index registers every rung under both
 * its raw superscript spelling and its {@link normalizeUnitLabel} form, while
 * this gate admits only the ASCII one — so it is a strict SUPERSET of what this
 * alphabet can produce and no label admitted here can be refused on commit,
 * compound labels (`mm^3`, `kg/m^3`, `g/cm^3`) included. HOW that index is
 * composed and what it deliberately excludes is argued once, on
 * `COMPOSED_UNIT_INDEX` in gui/src-tauri/src/engine.rs; restating it here would
 * be a second hand-maintained copy of one argument, which is the prose form of
 * the drift defect this function exists to avoid in data.
 *
 * THE ONE ASYMMETRY THAT REMAINS runs the other way and is deliberate: the
 * backend also accepts spellings this gate refuses — raw superscripts, the SI
 * bases no ladder carries (`s`, `K`, `A`, `mol`, `cd`), and a label belonging
 * to a dimension other than the cell's. The last of those is not silently
 * taken: it resolves to its own dimension and is then refused by reify-eval
 * with a `DimensionMismatch` naming both. Callers still scope this alphabet to
 * the cell's own dimension, so the panel rejects a cross-dimension literal
 * inline and the backend path is only reached by callers that bypass this gate
 * (`MechanismPanel`, via `handleSetParameter`).
 *
 * Separately, {@link acceptsBareNumber} decides whether a cell may be given a
 * bare number at all — the same coverage question this alphabet asks, asked of
 * one cell; its own doc carries that rule. The `App.test.tsx` block that used
 * to pin the degradation above now pins the reconciled contract in both
 * directions.
 *
 * Still deferred, and NOT this: compound unit EXPRESSIONS in .ri source at the
 * `bind(joint, <quantity>)` site (`UnitExpr::Mul`/`Div`/`Pow`), which are task
 * γ (#3803); and the compiler's per-module `UnitRegistry` spellings (`km`,
 * `ft`, `psi`, `degC`, …), which this gate rejects outright and which are
 * therefore unreachable from the property editor.
 */
export function quantityUnitAlphabet(ladders: UnitLadderMap | undefined): string[] {
  const alphabet = new Set<string>(BASE_UNIT_LABELS);
  for (const options of Object.values(ladders ?? {})) {
    // `options` and `opt.label` are typed non-null/string, but this data
    // crosses an IPC boundary (`get_unit_ladders`), so the types are a claim
    // about the backend's serde shape rather than a runtime guarantee. Before
    // #6028 a malformed payload could only degrade the unit PICKER — the
    // label lookup simply missed. Now the same payload feeds typed-value
    // validation, so an unguarded `label.replace` would throw out of the
    // `quantityRe` memo on EVERY Enter/blur in the panel, for every cell.
    // Skipping the bad entry keeps the blast radius where it was.
    for (const opt of options ?? []) {
      if (typeof opt?.label === 'string') alphabet.add(normalizeUnitLabel(opt.label));
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
 * A bare numeric literal — the same grammar as a quantity's numeric part, with
 * no unit suffix (task #6028).
 *
 * Built from {@link QUANTITY_NUMBER} rather than restated, so the unit-less and
 * unit-suffixed input paths cannot diverge: a future decision about the numeric
 * form (accept a leading `+`, accept digit separators, …) is made once and
 * lands on both. `PropertyEditor` used to carry a byte-for-byte copy of this
 * regex next to its quantity check, which is the same sync obligation #6028
 * removed from the unit alternation.
 *
 * As with {@link buildQuantityRe} there is no numeric range check, so callers
 * must still guard with `Number.isFinite(Number(value))` — `1e999` matches and
 * converts to `Infinity`.
 */
export const NUMBER_RE = new RegExp(`^(${QUANTITY_NUMBER})$`);

/**
 * Whether a cell with this dimension accepts a BARE number as typed input
 * (task #5757).
 *
 * THE RULE: a cell needs a unit exactly when one CAN be typed for it. `20` in a
 * `Volume` cell is ambiguous — 20 what? — and the engine used to resolve that
 * ambiguity by silently reading it as 20 CUBIC METRES, the same 1000× hazard
 * the .ri geometry-argument gate rejects with "pass a dimensioned length such
 * as `5mm`". This is the panel's inline mirror of that rule.
 *
 * EXPRESSIBILITY IS THE KEY, NOT DIMENSIONEDNESS. That ambiguity presupposes a
 * unit COULD have been typed: `20` is ambiguous in a Volume cell precisely
 * because `20mm^3` and `20L` were both on offer. For a dimension off the
 * {@link BASE_UNIT_DIMENSIONS} floor that no curated ladder covers, the picker
 * offers nothing and {@link quantityUnitAlphabet} admits nothing, so refusing the
 * bare number disambiguates nothing — it removes the cell's LAST accepted input
 * and bricks the row.
 *
 * The concrete case this was breaking is `Money`, whose cells accepted neither
 * `6` nor `6USD` and so could not be edited at all. WHY `USD` is out of reach
 * is the engine's to say: `COMPOSED_UNIT_INDEX` in gui/src-tauri/src/engine.rs
 * states which tables it composes and which it excludes, and this doc points
 * there rather than keeping a second copy of that argument in sync.
 *
 * Not a weakening, and the guarantee that replaces the old one is stronger for
 * being checkable: GATED ⟺ the dimension is on the {@link BASE_UNIT_DIMENSIONS}
 * floor OR a rung exists in this cell's own ladder ⟺ {@link quantityUnitAlphabet}
 * can express it. Those two disjuncts are exactly the two the alphabet unions, so
 * the gate and the alphabet beside it stay ONE rule read twice: a cell is told to
 * supply a unit precisely when the alphabet can accept one for it.
 *
 * The ladder half reuses {@link ladderForDimension} rather than indexing the map,
 * so "what counts as covered" has ONE definition, shared with `pickerLadder`,
 * `editSeedUnitLabel` and `quantityReFor`. It still enumerates no unit STRINGS —
 * map membership plus two canonical dimension names — so the standing #5788 D6
 * prohibition documented on {@link quantityUnitAlphabet} is untouched.
 *
 * THE BACKEND IS THE AUTHORITATIVE GATE: `parse_value_string_for_cell` in
 * `gui/src-tauri/src/engine.rs` refuses a `Value::Int`/`Value::Real` only for a
 * dimension its `LADDER_COVERAGE` table records, and does so for every caller
 * of `set_parameter` — including `MechanismPanel`, which reaches
 * `handleSetParameter` without passing through `PropertyEditor`'s gate. This
 * predicate exists to make the refusal INLINE, keeping the typed text on screen
 * for correction instead of discarding it behind an async error toast.
 *
 * THE TWO ENDS KEY ON DIFFERENT FACTS, and the residual gap runs ONE way and IS
 * reachable from this panel. The backend reads the cell's DECLARED type; this
 * reads `ValueData.dimension`, which `format_determined_cell` derives from the
 * cell's CURRENT VALUE via `display_scalar` — the empty string for `Undef`,
 * `Option(None)`, or any non-Scalar. For a Scalar-valued cell the two coincide.
 *
 * The live case is a `none`-valued `Option<Length>`: `display_scalar` returns
 * `None`, the dimension serialises as `''`, this gate admits the bare number,
 * and the backend — which unwraps `Type::Option` before gating — refuses it
 * behind exactly the async toast this predicate exists to avoid. The user still
 * gets the actionable "expects Length, got the bare number '120'" rather than a
 * generic type error, so the outcome is correct and only the INLINE-ness is
 * lost. An `Option(Some(80mm))` cell surfaces `'Length'` and is gated inline as
 * usual, so the divergence is confined to the `none` state.
 *
 * Closing it properly means surfacing the DECLARED dimension on `ValueData` as a
 * field of its own, so both ends read one fact; until then it is recorded here
 * rather than claimed away.
 *
 * IT FAILS OPEN ONLY BELOW THE FLOOR. With `ladders` undefined or empty — the
 * `get_unit_ladders` fetch not resolved, or failed — nothing beyond
 * {@link BASE_UNIT_DIMENSIONS} is expressible, so nothing beyond it is gated.
 * That is the safe direction for the dimensions this side genuinely cannot
 * describe: the backend stays authoritative, and over-rejecting there would
 * discard input the engine would have accepted.
 *
 * It is the WRONG direction for the floor, which is why the floor is not part of
 * it. The engine's `LADDER_COVERAGE` is built in-process from the Rust-authored
 * curated table and is ALWAYS populated, so it keeps gating Length and Angle
 * whatever happens to the fetch. Failing open on them made the two ends disagree
 * exactly on that path — the panel accepting `80` in a Length cell, `editSeed`
 * seeding the bare magnitude, and the engine then refusing with "expects Length,
 * got the bare number '80'" behind the async toast this predicate exists to
 * avoid. Keeping the floor gated instead means `editSeed` still seeds `80mm`
 * (via `editSeedUnitLabel`'s `?? val.unit` fallback, since there is no ladder to
 * read a default rung from) and the inline gate still matches the engine.
 *
 * The floor is applied unconditionally rather than only when the map is missing,
 * which is the same shape {@link quantityUnitAlphabet} already has: it unions
 * {@link BASE_UNIT_LABELS} in on EVERY path, not just the ladder-less one. A
 * populated map covers Length and Angle anyway, so the two forms differ only for
 * a present-but-partial payload — where matching the alphabet's own floor is
 * what keeps the gate and the alphabet beside it reading one rule.
 *
 * IT DOES NOT COST THE ORDINARY EDIT A UNIT KEYSTROKE. `PropertyEditor`'s
 * `editSeed` seeds a COVERED cell's input with a unit-bearing literal —
 * magnitude + the cell's default ladder rung, or its unit badge on the
 * ladder-less floor path — so committing an untouched row is still a no-op and
 * changing only the digits still submits a united literal.
 * A predicate like this one is only safe to add alongside a seed like that. An
 * UNCOVERED cell seeds the bare magnitude, which is now consistent rather than
 * a degradation: that seed is a literal both ends accept.
 *
 * THE ASYMMETRY THAT REMAINS runs the documented safe direction only — the
 * backend accepts spellings this gate refuses (raw superscripts, the SI bases
 * no ladder carries: `s`, `K`, `A`, `mol`, `cd`, and cross-dimension labels).
 * A COMPOSED dimension is no longer part of it: the backend serialises one as
 * the empty string, indistinguishable here from dimensionless, and since the
 * coverage-conditional rule the backend does not gate it either — so the two
 * ends now AGREE on it.
 */
export function acceptsBareNumber(
  dimension: string | undefined,
  ladders: UnitLadderMap | undefined,
): boolean {
  if (!dimension) return true;
  if (BASE_UNIT_DIMENSIONS.includes(dimension)) return false;
  return ladderForDimension(ladders ?? {}, dimension) === undefined;
}

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
