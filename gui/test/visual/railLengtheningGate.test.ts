/**
 * CI-gated backstop for the printer_v01 Y-rail lengthening integration gate
 * (task 5098; PRD docs/prds/v0_6/ai-native-editing.md §7 leaf ζ).
 *
 * The live half of that gate (`./smoke_rail_lengthening_e2e.mjs`) needs a real
 * webview, a real OCCT and a real `reify_set_parameter` round trip, so it can
 * never run in CI. This suite pins the part that can actually regress — the
 * decision function that turns four debug-MCP payloads into a pass/fail
 * verdict — as pure data, with no RPC and no GUI.
 *
 * THE SCENARIO, as measured on printer.ri (the truth table `RAIL_GATE_PHASES`
 * encodes; the reading was taken with `reify check`, and the same three states
 * are re-derived by the live driver through the GUI):
 *
 *   baseline          y_rail_len 800  rail_span_m 800   travel_avail 510
 *   after-y-rail      y_rail_len 1100 rail_span_m 800   travel_avail 510
 *   after-rail-span   y_rail_len 1100 rail_span_m 1100  travel_avail 810
 *
 * TWO THINGS THIS SUITE DELIBERATELY DOES NOT ASSERT, both because the measured
 * behaviour contradicts the obvious reading:
 *
 *   * "the pins go green again" is NOT a zero-violations claim. Lengthening the
 *     rail span moves AFrame.travel_avail 510 -> 810, which drags the ToolDock
 *     pinned literal `yh_min_today` off its `centre_y - travel_avail/2` target,
 *     so a SECOND pin legitimately goes red at `after-rail-span`. A
 *     zero-violations assertion would red on correct behaviour. The gate names
 *     the two pins it is about and asserts each one's own expected status —
 *     which makes the ToolDock flip a second, free constraint-status-sync
 *     signal rather than a nuisance.
 *   * a pin is never selected by its `node_id` index. `Printer#constraint[45]`
 *     is positional: adding a constraint anywhere above it in printer.ri would
 *     silently retarget an index-keyed assertion at a different predicate. The
 *     selector is a superset match on `parameter_ids` (which is
 *     `collect_value_refs(expr)`, engine.rs), so the pin is named by what it is
 *     about.
 *
 * ASSERT ON RECORDS, NOT PROSE — the house rule from `./meshCountParity.test.ts`.
 * Every gate returns a {gate, tool, field, observed, expected} record, so the
 * cases below check the observation itself rather than substring-matching an
 * English sentence. The rendered wording is pinned once, in the formatFailures
 * block at the end.
 */
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, it, expect } from "vitest";
import {
  PRINTER_RELPATH,
  RAIL_GATE_MIN_BODIES,
  RAIL_GATE_MIN_CONSTRAINTS,
  RAIL_GATE_MIN_DISPATCHES,
  RAIL_GATE_MIN_VALUES,
  RAIL_GATE_PHASES,
  PIN_ABSENT,
  PIN_RAIL_SPAN_CELL,
  PIN_TRAVEL_AVAIL_CELL,
  PIN_Y_RAIL_LEN_CELL,
  PIN_YH_MIN_TODAY_CELL,
  RAIL_GATE_LENGTH_TOLERANCE_MM,
  RAIL_SPAN_CELL,
  RAIL_SPAN_PIN,
  SUBJECT_BASENAME,
  TOOL_DOCK_PIN,
  TRAVEL_AVAIL_CELL,
  X_RAIL_LEN_CELL,
  YH_MIN_TODAY_CELL,
  Y_RAIL_LEN_CELL,
  checkFieldCoverage,
  checkIdempotentReload,
  checkRailLengtheningGate,
  checkRejectionAtomicity,
  checkSourceCanonical,
  extractGateInputs,
  findEditableParams,
  foldPinStatus,
  formatFailures,
  observeThenExtras,
  selectPinConstraints,
} from "./railLengtheningGate.mjs";

type Failure = {
  gate: string;
  tool: string;
  field?: string;
  observed: unknown;
  expected?: unknown;
};

type Verdict = { ok: boolean; failures: Failure[] };
type Extraction = { inputs: Record<string, unknown>; failures: Failure[] };

const SUBJECT_PATH = "/tmp/rail-gate-abc123/printer_v01/printer.ri";

/** One `engine_state` `values[]` entry, in the shape the live payload carries. */
function value(cellId: string, mm: number, freshness = "final") {
  const [entity, name] = cellId.split(".");
  return {
    cell_id: cellId,
    name,
    value: String(mm),
    unit: "mm",
    determinacy: "determined",
    entity_path: entity,
    kind: "Param",
    freshness,
    reason: null,
    last_substantive_value: null,
    si_value: mm / 1000,
    dimension: "Length",
  };
}

/** One `engine_state` `constraints[]` entry. */
function constraint(nodeId: string, status: string, parameterIds: string[]) {
  return {
    node_id: nodeId,
    expression: `<${nodeId}>`,
    status,
    label: null,
    parameter_ids: [...parameterIds].sort(),
  };
}

/**
 * The rail-span pin is a PAIR — printer.ri writes `x < y + slack` and
 * `x > y - slack` — and both halves carry the same `parameter_ids`, so the
 * selector matches both. `railSpanStatus` sets the `>` half, which is the one
 * that actually flips.
 */
function constraintsFor(railSpanStatus: string, toolDockStatus: string) {
  const railCells = [PIN_RAIL_SPAN_CELL, PIN_Y_RAIL_LEN_CELL, "Printer.o1_pin_slack"];
  const dockCells = [
    "Printer.a_frame.centre_y",
    PIN_TRAVEL_AVAIL_CELL,
    "Printer.o1_pin_slack",
    PIN_YH_MIN_TODAY_CELL,
  ];
  return [
    constraint("Printer#constraint[44]", "Satisfied", railCells),
    constraint("Printer#constraint[45]", railSpanStatus, railCells),
    constraint("Printer#constraint[88]", toolDockStatus, dockCells),
    constraint("Printer#constraint[89]", "Satisfied", dockCells),
    // An unrelated pin that must never be selected: it names ONE of the two
    // rail cells, so a match-any selector would pick it up.
    constraint("Printer#constraint[7]", "Satisfied", [
      PIN_RAIL_SPAN_CELL,
      "Printer.a_frame.brg_len_m",
    ]),
  ];
}

/** A complete, passing input for one phase — the shape the cases below perturb. */
function inputsFor(
  phase: string,
  yRailLenMm: number,
  railSpanMm: number,
  travelAvailMm: number,
  railSpanPinStatus: string,
  toolDockPinStatus: string,
) {
  return {
    phase,
    meshCount: 240,
    valueCount: 900,
    constraintCount: 120,
    dispatchCount: 40,
    activeFile: SUBJECT_PATH,
    source: "param y_rail_len : Length = 800mm\n",
    cells: {
      [Y_RAIL_LEN_CELL]: { mm: yRailLenMm, freshness: "final" },
      [RAIL_SPAN_CELL]: { mm: railSpanMm, freshness: "final" },
      [TRAVEL_AVAIL_CELL]: { mm: travelAvailMm, freshness: "final" },
    },
    railSpanPinStatus,
    toolDockPinStatus,
  };
}

const BASELINE = inputsFor("baseline", 800, 800, 510, "Satisfied", "Satisfied");
const AFTER_Y_RAIL = inputsFor("after-y-rail", 1100, 800, 510, "Violated", "Satisfied");
const AFTER_RAIL_SPAN = inputsFor("after-rail-span", 1100, 1100, 810, "Satisfied", "Violated");

/** Every failure record naming one field. */
const forField = (failures: Failure[], field: string) => failures.filter((f) => f.field === field);
/** Every failure record from one gate. */
const forGate = (failures: Failure[], gate: string) => failures.filter((f) => f.gate === gate);

// ─────────────────────────────────────────────────────────────────────────────

describe("RAIL_GATE_PHASES — the measured truth table", () => {
  it("names exactly the three states the scenario passes through", () => {
    expect(Object.keys(RAIL_GATE_PHASES)).toEqual([
      "baseline",
      "after-y-rail",
      "after-rail-span",
    ]);
  });

  it("pins the two edits and the derived travel each phase expects", () => {
    expect(RAIL_GATE_PHASES["baseline"]).toMatchObject({
      cells: { [Y_RAIL_LEN_CELL]: 800, [RAIL_SPAN_CELL]: 800, [TRAVEL_AVAIL_CELL]: 510 },
    });
    expect(RAIL_GATE_PHASES["after-y-rail"]).toMatchObject({
      cells: { [Y_RAIL_LEN_CELL]: 1100, [RAIL_SPAN_CELL]: 800, [TRAVEL_AVAIL_CELL]: 510 },
    });
    expect(RAIL_GATE_PHASES["after-rail-span"]).toMatchObject({
      cells: { [Y_RAIL_LEN_CELL]: 1100, [RAIL_SPAN_CELL]: 1100, [TRAVEL_AVAIL_CELL]: 810 },
    });
  });

  it("encodes the rail-span pin's Satisfied -> Violated -> Satisfied transition", () => {
    expect([
      RAIL_GATE_PHASES["baseline"]!.railSpanPinStatus,
      RAIL_GATE_PHASES["after-y-rail"]!.railSpanPinStatus,
      RAIL_GATE_PHASES["after-rail-span"]!.railSpanPinStatus,
    ]).toEqual(["Satisfied", "Violated", "Satisfied"]);
  });

  it("encodes the ToolDock cascade as EXPECTED, not as a failure to be tolerated", () => {
    // Lengthening the rail span moves travel_avail 510 -> 810, which puts the
    // ToolDock's pinned `yh_min_today` literal out of range. The gate demands
    // that flip; it does not merely permit it.
    expect([
      RAIL_GATE_PHASES["baseline"]!.toolDockPinStatus,
      RAIL_GATE_PHASES["after-y-rail"]!.toolDockPinStatus,
      RAIL_GATE_PHASES["after-rail-span"]!.toolDockPinStatus,
    ]).toEqual(["Satisfied", "Satisfied", "Violated"]);
  });
});

describe("checkRailLengtheningGate — (a) the three measured states pass", () => {
  it.each([
    ["baseline", BASELINE],
    ["after-y-rail", AFTER_Y_RAIL],
    ["after-rail-span", AFTER_RAIL_SPAN],
  ])("passes at %s", (_name, inputs) => {
    expect(checkRailLengtheningGate(inputs)).toEqual({ ok: true, failures: [] });
  });
});

describe("checkRailLengtheningGate — (b) the value gate", () => {
  it("reads travel_avail 510 at baseline and rejects the post-edit number there", () => {
    const { ok, failures } = checkRailLengtheningGate({
      ...BASELINE,
      cells: { ...BASELINE.cells, [TRAVEL_AVAIL_CELL]: { mm: 810, freshness: "final" } },
    }) as Verdict;
    expect(ok).toBe(false);
    expect(forField(failures, `values[${TRAVEL_AVAIL_CELL}].mm`)).toEqual([
      {
        gate: "value",
        tool: "engine_state",
        field: `values[${TRAVEL_AVAIL_CELL}].mm`,
        observed: 810,
        expected: 510,
      },
    ]);
  });

  it("reads travel_avail 810 after the rail-span edit and rejects the stale number", () => {
    const { failures } = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      cells: { ...AFTER_RAIL_SPAN.cells, [TRAVEL_AVAIL_CELL]: { mm: 510, freshness: "final" } },
    }) as Verdict;
    expect(forField(failures, `values[${TRAVEL_AVAIL_CELL}].mm`)).toEqual([
      {
        gate: "value",
        tool: "engine_state",
        field: `values[${TRAVEL_AVAIL_CELL}].mm`,
        observed: 510,
        expected: 810,
      },
    ]);
  });

  it("localises which of the three cells drifted", () => {
    const { failures } = checkRailLengtheningGate({
      ...AFTER_Y_RAIL,
      cells: { ...AFTER_Y_RAIL.cells, [Y_RAIL_LEN_CELL]: { mm: 800, freshness: "final" } },
    }) as Verdict;
    expect(failures.map((f) => f.field)).toEqual([`values[${Y_RAIL_LEN_CELL}].mm`]);
  });

  it("tolerates float noise in the SI round trip, and nothing larger", () => {
    // si_value arrives in metres and is scaled by 1000, so 0.51 m can land a few
    // ulps off 510. An exact === here would red on a correct run — and a
    // tolerance wide enough to swallow a real drift would be worse, so both
    // sides of RAIL_GATE_LENGTH_TOLERANCE_MM are pinned.
    const at = (drift: number) =>
      checkRailLengtheningGate({
        ...BASELINE,
        cells: { ...BASELINE.cells, [TRAVEL_AVAIL_CELL]: { mm: 510 + drift, freshness: "final" } },
      }) as Verdict;
    expect(at(RAIL_GATE_LENGTH_TOLERANCE_MM / 2).ok).toBe(true);
    expect(at(RAIL_GATE_LENGTH_TOLERANCE_MM * 10).ok).toBe(false);
  });

  it("names a cell whose reading is not a finite number as a shape failure, not a value one", () => {
    const { failures } = checkRailLengtheningGate({
      ...BASELINE,
      cells: { ...BASELINE.cells, [RAIL_SPAN_CELL]: { mm: undefined, freshness: "final" } },
    }) as Verdict;
    expect(failures).toEqual([
      {
        gate: "shape",
        tool: "engine_state",
        field: `values[${RAIL_SPAN_CELL}].mm`,
        observed: undefined,
        expected: "finite-number",
      },
    ]);
  });

  it("names a missing cell rather than reading it as zero", () => {
    const { failures } = checkRailLengtheningGate({ ...BASELINE, cells: {} }) as Verdict;
    expect(forGate(failures, "shape").map((f) => f.field)).toEqual([
      `values[${Y_RAIL_LEN_CELL}].mm`,
      `values[${RAIL_SPAN_CELL}].mm`,
      `values[${TRAVEL_AVAIL_CELL}].mm`,
    ]);
  });
});

describe("checkRailLengtheningGate — (c) the staleness gate", () => {
  it("refuses a pruned cell whose number is the last good value, not the live one", () => {
    // A demand-pruned cell reads freshness 'pending' and keeps its previous
    // value, so the value gate would PASS on a number the edit never produced.
    const { ok, failures } = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      cells: { ...AFTER_RAIL_SPAN.cells, [TRAVEL_AVAIL_CELL]: { mm: 810, freshness: "pending" } },
    }) as Verdict;
    expect(ok).toBe(false);
    expect(failures).toEqual([
      {
        gate: "stale",
        tool: "engine_state",
        field: `values[${TRAVEL_AVAIL_CELL}].freshness`,
        observed: "pending",
        expected: "final",
      },
    ]);
  });

  it("refuses a failed cell too", () => {
    const { failures } = checkRailLengtheningGate({
      ...BASELINE,
      cells: { ...BASELINE.cells, [Y_RAIL_LEN_CELL]: { mm: 800, freshness: "failed" } },
    }) as Verdict;
    expect(forGate(failures, "stale").map((f) => f.observed)).toEqual(["failed"]);
  });
});

describe("checkRailLengtheningGate — (d) the constraint gate", () => {
  it("demands the rail-span pin flip to Violated after the y_rail_len edit", () => {
    const { ok, failures } = checkRailLengtheningGate({
      ...AFTER_Y_RAIL,
      railSpanPinStatus: "Satisfied",
    }) as Verdict;
    expect(ok).toBe(false);
    expect(failures).toEqual([
      {
        gate: "constraint",
        tool: "engine_state",
        field: `constraints[${RAIL_SPAN_PIN}].status`,
        observed: "Satisfied",
        expected: "Violated",
      },
    ]);
  });

  it("demands the rail-span pin return to Satisfied after the rail_span_m edit", () => {
    const { failures } = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      railSpanPinStatus: "Violated",
    }) as Verdict;
    expect(forField(failures, `constraints[${RAIL_SPAN_PIN}].status`)).toEqual([
      {
        gate: "constraint",
        tool: "engine_state",
        field: `constraints[${RAIL_SPAN_PIN}].status`,
        observed: "Violated",
        expected: "Satisfied",
      },
    ]);
  });

  it("demands the ToolDock cascade, so a pin that stays green is a FAILURE", () => {
    const { ok, failures } = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      toolDockPinStatus: "Satisfied",
    }) as Verdict;
    expect(ok).toBe(false);
    expect(failures).toEqual([
      {
        gate: "constraint",
        tool: "engine_state",
        field: `constraints[${TOOL_DOCK_PIN}].status`,
        observed: "Satisfied",
        expected: "Violated",
      },
    ]);
  });

  it("reports an absent pin as a constraint failure naming PIN_ABSENT", () => {
    const { failures } = checkRailLengtheningGate({
      ...BASELINE,
      railSpanPinStatus: PIN_ABSENT,
    }) as Verdict;
    expect(forField(failures, `constraints[${RAIL_SPAN_PIN}].status`)![0]!.observed).toBe(
      PIN_ABSENT,
    );
  });

  it("never asserts a global 'no constraints violated' — only the two named pins", () => {
    // The scenario legitimately leaves a violated pin behind at the last phase,
    // and printer.ri carries eleven indeterminate constraints at baseline. Any
    // gate keyed on a global count would red on correct behaviour — so this
    // drives the whole extract-then-check path over a constraints list that is
    // genuinely full of red, none of it on either named pin.
    const payloads = payloadsFor("after-rail-span", 1100, 1100, 810, "Satisfied", "Violated");
    payloads.engineState.constraints = [
      ...payloads.engineState.constraints,
      ...new Array(11)
        .fill(0)
        .map((_, i) => constraint(`Printer#constraint[${200 + i}]`, "Indeterminate", ["Printer.env.build_z"])),
      constraint("Printer#constraint[300]", "Violated", ["Printer.d_toolhead.peak_accel"]),
    ];
    const { inputs, failures } = extractGateInputs(payloads) as Extraction;
    expect(failures).toEqual([]);
    expect(checkRailLengtheningGate(inputs)).toEqual({ ok: true, failures: [] });
  });
});

describe("checkRailLengtheningGate — (e) the vacuity gate", () => {
  it("rejects an empty scene: nothing loaded means the invariant was never tested", () => {
    const { ok, failures } = checkRailLengtheningGate({
      ...BASELINE,
      meshCount: 0,
      valueCount: 0,
      constraintCount: 0,
      dispatchCount: 0,
    }) as Verdict;
    expect(ok).toBe(false);
    expect(forGate(failures, "vacuity").map((f) => [f.field, f.observed, f.expected])).toEqual([
      ["meshes.length", 0, RAIL_GATE_MIN_BODIES],
      ["values.length", 0, RAIL_GATE_MIN_VALUES],
      ["constraints.length", 0, RAIL_GATE_MIN_CONSTRAINTS],
      ["dispatch_by_realization", 0, RAIL_GATE_MIN_DISPATCHES],
    ]);
  });

  it("rejects a PARTIAL load, not just a total one", () => {
    const { ok } = checkRailLengtheningGate({ ...BASELINE, meshCount: 3 }) as Verdict;
    expect(ok).toBe(false);
  });

  it("names demand_dispatch as the tool for the dispatch floor", () => {
    const { failures } = checkRailLengtheningGate({ ...BASELINE, dispatchCount: 0 }) as Verdict;
    expect(failures.map((f) => f.tool)).toEqual(["demand_dispatch"]);
  });
});

describe("checkRailLengtheningGate — (f) the subject gate", () => {
  it("rejects a run that graded some other file", () => {
    const { ok, failures } = checkRailLengtheningGate({
      ...BASELINE,
      activeFile: "/home/leo/src/reify/gui/test/fixtures/large_assembly.ri",
    }) as Verdict;
    expect(ok).toBe(false);
    expect(failures).toEqual([
      {
        gate: "subject",
        tool: "store_state",
        field: "editor.activeFile",
        observed: "/home/leo/src/reify/gui/test/fixtures/large_assembly.ri",
        expected: SUBJECT_BASENAME,
      },
    ]);
  });

  it.each([
    ["a mkdtemp copy under /tmp", "/tmp/reify-rail-lengthening-9Xq2/printer_v01/printer.ri"],
    ["the same copy with /tmp's symlink resolved", "/private/tmp/reify-x/printer_v01/printer.ri"],
    ["the bare basename", SUBJECT_BASENAME],
  ])("accepts %s — only the basename is stable", (_name, activeFile) => {
    // The driver never mutates the tracked design file; it drives a mkdtemp
    // copy, and canonicalize resolves /tmp's symlink, so the path that comes
    // back is not the one that went in.
    expect(checkRailLengtheningGate({ ...BASELINE, activeFile }).ok).toBe(true);
  });

  it("rejects a path that merely CONTAINS the basename mid-segment", () => {
    // `endsWith('/printer.ri')`, not `includes`: a sibling named
    // `printer.ri.bak` — or a directory of that name — is not the subject.
    expect(
      checkRailLengtheningGate({ ...BASELINE, activeFile: `/tmp/x/${SUBJECT_BASENAME}.bak` }).ok,
    ).toBe(false);
  });
});

describe("checkRailLengtheningGate — (g) never throws, for ANY argument", () => {
  it.each([
    ["null", null],
    ["undefined", undefined],
    ["a string", "engine_state"],
    ["a number", 7],
    ["an array", []],
    ["an in-band error envelope", { error: "engine not ready" }],
    ["an inputs object with a null cells map", { ...BASELINE, cells: null }],
  ])("returns a verdict for %s rather than throwing", (_name, arg) => {
    const verdict = checkRailLengtheningGate(arg) as Verdict;
    expect(verdict.ok).toBe(false);
    expect(Array.isArray(verdict.failures)).toBe(true);
    expect(verdict.failures.length).toBeGreaterThan(0);
  });

  it("names an unknown phase as a shape failure and still grades the rest", () => {
    const { failures } = checkRailLengtheningGate({
      ...BASELINE,
      phase: "after-the-third-edit",
      meshCount: 0,
    }) as Verdict;
    expect(forGate(failures, "shape")).toEqual([
      {
        gate: "shape",
        tool: "railLengtheningGate",
        field: "phase",
        observed: "after-the-third-edit",
        expected: "one-of-RAIL_GATE_PHASES",
      },
    ]);
    // The phase-independent gates are still evaluated — a bad phase must not
    // swallow the rest of the diagnosis.
    expect(forGate(failures, "vacuity").length).toBeGreaterThan(0);
  });
});

// ─── extractGateInputs ───────────────────────────────────────────────────────

/** A complete, healthy payload set for one phase. */
function payloadsFor(
  phase: string,
  yRailLenMm: number,
  railSpanMm: number,
  travelAvailMm: number,
  railSpanStatus: string,
  toolDockStatus: string,
) {
  return {
    phase,
    engineState: {
      meshes: new Array(240).fill({ entity_path: "Printer#realization[0]" }),
      values: [
        value(Y_RAIL_LEN_CELL, yRailLenMm),
        value(RAIL_SPAN_CELL, railSpanMm),
        value(TRAVEL_AVAIL_CELL, travelAvailMm),
        value(YH_MIN_TODAY_CELL, 145),
      ],
      constraints: constraintsFor(railSpanStatus, toolDockStatus),
    },
    storeState: { editor: { activeFile: SUBJECT_PATH, dirtyFiles: [], openFiles: [] } },
    openFile: { success: true, source: "param y_rail_len : Length = 800mm\n" },
    demandDispatch: {
      dispatch_by_realization: Object.fromEntries(
        new Array(40).fill(0).map((_, i) => [`Printer#realization[${i}]`, 1]),
      ),
      eval_set: [],
      full_scope: false,
    },
  };
}

describe("extractGateInputs — (h) folding live payloads into flat scalars", () => {
  it("extracts a complete, gradeable baseline with no failures", () => {
    const { inputs, failures } = extractGateInputs(
      payloadsFor("baseline", 800, 800, 510, "Satisfied", "Satisfied"),
    ) as Extraction;
    expect(failures).toEqual([]);
    expect(checkRailLengtheningGate(inputs)).toEqual({ ok: true, failures: [] });
  });

  it("scales si_value from metres to millimetres", () => {
    const { inputs } = extractGateInputs(
      payloadsFor("after-rail-span", 1100, 1100, 810, "Satisfied", "Violated"),
    ) as Extraction;
    expect((inputs["cells"] as Record<string, { mm: number }>)[TRAVEL_AVAIL_CELL]!.mm).toBeCloseTo(
      810,
      9,
    );
  });

  it("selects the pin by parameter_ids superset, never by node_id index", () => {
    const payloads = payloadsFor("after-y-rail", 1100, 800, 510, "Violated", "Satisfied");
    // Renumber every constraint. An index-keyed selector would now miss.
    payloads.engineState.constraints = payloads.engineState.constraints.map((c, i) => ({
      ...c,
      node_id: `Printer#constraint[${900 + i}]`,
    }));
    const { inputs, failures } = extractGateInputs(payloads) as Extraction;
    expect(failures).toEqual([]);
    expect(inputs["railSpanPinStatus"]).toBe("Violated");
  });

  it("folds the two halves of a pin pair: Violated on either half is Violated", () => {
    const { inputs } = extractGateInputs(
      payloadsFor("after-y-rail", 1100, 800, 510, "Violated", "Satisfied"),
    ) as Extraction;
    // Only `Printer#constraint[45]` is Violated; `[44]` shares the same
    // parameter_ids and is Satisfied.
    expect(inputs["railSpanPinStatus"]).toBe("Violated");
    expect(inputs["toolDockPinStatus"]).toBe("Satisfied");
  });

  it("folds an Indeterminate half as Indeterminate when no half is Violated", () => {
    const payloads = payloadsFor("baseline", 800, 800, 510, "Indeterminate", "Satisfied");
    const { inputs } = extractGateInputs(payloads) as Extraction;
    expect(inputs["railSpanPinStatus"]).toBe("Indeterminate");
  });

  it("reports an unmatched pin as 'absent' rather than silently passing", () => {
    const payloads = payloadsFor("baseline", 800, 800, 510, "Satisfied", "Satisfied");
    payloads.engineState.constraints = payloads.engineState.constraints.filter(
      (c) => !c.parameter_ids.includes(PIN_Y_RAIL_LEN_CELL),
    );
    const { inputs } = extractGateInputs(payloads) as Extraction;
    expect(inputs["railSpanPinStatus"]).toBe(PIN_ABSENT);
  });

  it("classifies an in-band tool error as an OUTAGE, not a shape problem", () => {
    // An outage means the invariant was never tested; a caller must be able to
    // tell that apart from a gate that was tested and violated.
    const payloads = payloadsFor("baseline", 800, 800, 510, "Satisfied", "Satisfied");
    const { failures } = extractGateInputs({
      ...payloads,
      engineState: { error: "engine session poisoned" },
    }) as Extraction;
    expect(forGate(failures, "outage")).toEqual([
      { gate: "outage", tool: "engine_state", observed: "engine session poisoned" },
    ]);
  });

  it("names each unreadable payload rather than reporting one blanket problem", () => {
    const { failures } = extractGateInputs({
      phase: "baseline",
      engineState: null,
      storeState: 42,
      openFile: [],
      demandDispatch: "nope",
    }) as Extraction;
    expect(forGate(failures, "shape").map((f) => f.tool)).toEqual([
      "engine_state",
      "store_state",
      "reify_open_file",
      "demand_dispatch",
    ]);
  });

  it("flags a non-array meshes/values/constraints field by name", () => {
    const payloads = payloadsFor("baseline", 800, 800, 510, "Satisfied", "Satisfied");
    const { failures } = extractGateInputs({
      ...payloads,
      engineState: { meshes: 240, values: null, constraints: "many" },
    }) as Extraction;
    expect(forGate(failures, "shape").map((f) => [f.field, f.expected])).toEqual([
      ["meshes", "array"],
      ["values", "array"],
      ["constraints", "array"],
    ]);
  });

  it("flags a missing reify_open_file source", () => {
    const payloads = payloadsFor("baseline", 800, 800, 510, "Satisfied", "Satisfied");
    const { failures } = extractGateInputs({
      ...payloads,
      openFile: { success: true },
    }) as Extraction;
    expect(forGate(failures, "shape").map((f) => [f.tool, f.field, f.expected])).toEqual([
      ["reify_open_file", "source", "non-empty-string"],
    ]);
  });

  it("never throws for a null argument — the shape a caller holds after a failed read", () => {
    const { inputs, failures } = extractGateInputs(null) as Extraction;
    expect(failures.length).toBeGreaterThan(0);
    expect(checkRailLengtheningGate(inputs).ok).toBe(false);
  });

  it("an extraction failure means the invariant was NEVER TESTED, so the verdict is not ok", () => {
    const payloads = payloadsFor("baseline", 800, 800, 510, "Satisfied", "Satisfied");
    const { inputs, failures } = extractGateInputs({
      ...payloads,
      engineState: { error: "boom" },
    }) as Extraction;
    expect(failures.length).toBeGreaterThan(0);
    expect(checkRailLengtheningGate(inputs).ok).toBe(false);
  });
});

describe("selectPinConstraints / foldPinStatus — (h2) the selector's own halves", () => {
  const pinCells = [PIN_RAIL_SPAN_CELL, PIN_Y_RAIL_LEN_CELL];
  const half = (status: string) =>
    constraint("Printer#constraint[45]", status, [...pinCells, "Printer.o1_pin_slack"]);

  it("matches on a SUPERSET of the named cells, never on one of them", () => {
    const matched = selectPinConstraints(
      [
        half("Satisfied"),
        constraint("Printer#constraint[7]", "Violated", [PIN_RAIL_SPAN_CELL]),
        constraint("Printer#constraint[8]", "Violated", [PIN_Y_RAIL_LEN_CELL]),
      ],
      pinCells,
    );
    expect(matched.map((c) => c.node_id)).toEqual(["Printer#constraint[45]"]);
  });

  it.each([
    ["a non-array constraints list", "many", pinCells],
    ["a null constraints list", null, pinCells],
    ["a non-array cells list", [half("Satisfied")], "Printer.a_frame.rail_span_m"],
    ["an entry that is not an object", [null, 7, "x"], pinCells],
    ["an entry whose parameter_ids is not an array", [{ node_id: "a", parameter_ids: "x" }], pinCells],
  ])("yields no matches for %s rather than throwing", (_name, constraints, cells) => {
    expect(selectPinConstraints(constraints as never, cells as never)).toEqual([]);
  });

  it.each([
    ["Violated wins over every other half", ["Satisfied", "Indeterminate", "Violated"], "Violated"],
    ["Indeterminate wins when no half is Violated", ["Satisfied", "Indeterminate"], "Indeterminate"],
    ["Satisfied only when EVERY half is", ["Satisfied", "Satisfied"], "Satisfied"],
  ])("folds a pin pair: %s", (_name, statuses, want) => {
    expect(foldPinStatus((statuses as string[]).map(half))).toBe(want);
  });

  it.each([
    ["a status outside the engine's vocabulary", [half("Unknown"), half("Satisfied")]],
    ["a status that is not a string at all", [{ ...half("Satisfied"), status: 7 }]],
    ["an entry that is not an object", [null]],
    ["no match at all", []],
    ["a non-array argument", "Satisfied"],
  ])("reports %s as PIN_ABSENT, never as a silent pass", (_name, matched) => {
    // `build_constraints` emits exactly Satisfied / Violated / Indeterminate, so
    // anything else is not a pin reading. Folding it to "Satisfied" by omission
    // would be the disarmed-assertion failure the whole selector exists to
    // avoid: the gate would go green on a payload it did not understand.
    expect(foldPinStatus(matched as never)).toBe(PIN_ABSENT);
  });
});

// ─── formatFailures ──────────────────────────────────────────────────────────

describe("formatFailures — the single site where a record becomes English", () => {
  it("renders one line per failure, in the order given", () => {
    const { failures } = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      meshCount: 0,
      railSpanPinStatus: "Violated",
    }) as Verdict;
    // ORDER, not length: `formatFailures` is an `Array.map`, so a length check
    // holds for any implementation at all. Each line must be the rendering of
    // the record at the SAME index, which is what a caller printing the list
    // alongside the records relies on.
    expect(failures.length).toBeGreaterThan(1);
    expect(formatFailures(failures)).toEqual(
      failures.map((f) => expect.stringContaining(f.field === undefined ? f.tool : f.field)),
    );
  });

  it("explains a constraint failure in terms of the pin, the phase and both statuses", () => {
    const { failures } = checkRailLengtheningGate({
      ...AFTER_Y_RAIL,
      railSpanPinStatus: "Satisfied",
    }) as Verdict;
    const line = formatFailures(failures)[0]!;
    expect(line).toContain(RAIL_SPAN_PIN);
    expect(line).toContain("Satisfied");
    expect(line).toContain("Violated");
  });

  it("explains a stale reading in terms of the cell and the freshness actually read", () => {
    const { failures } = checkRailLengtheningGate({
      ...BASELINE,
      cells: { ...BASELINE.cells, [TRAVEL_AVAIL_CELL]: { mm: 510, freshness: "pending" } },
    }) as Verdict;
    const line = formatFailures(failures)[0]!;
    expect(line).toContain(TRAVEL_AVAIL_CELL);
    expect(line).toContain("pending");
  });

  it("explains an outage in terms of the tool that failed and what it returned", () => {
    const line = formatFailures([
      { gate: "outage", tool: "engine_state", observed: "boom" },
    ] as Failure[])[0]!;
    expect(line).toContain("engine_state");
    expect(line).toContain("boom");
  });

  it.each([
    ["a boolean-valued coverage record", { gate: "coverage", tool: "engine_state", field: "stale", observed: true, expected: false }, ["true", "false"]],
    ["a boolean-valued canonical record", { gate: "canonical", tool: "reify_save_file", field: "success", observed: false, expected: true }, ["false", "true"]],
  ])("renders %s with both of its values, not with 'boolean'", (_name, record, wanted) => {
    // `describeValue` had no boolean branch, so it fell through to `typeof` and
    // the only diagnostic a live run prints for `stale` / `reload_error` /
    // `reify_save_file.success` read "is boolean, expected boolean" — both
    // numbers erased from the one sentence that had to carry them.
    const line = formatFailures([record] as Failure[])[0]!;
    for (const want of wanted) expect(line).toContain(want);
    expect(line).not.toContain("is boolean, expected boolean");
  });

  it("renders a shape token's prose without resolving an INHERITED object key", () => {
    // `SHAPE_PROSE[f.expected]` reached `Object.prototype`, so a record whose
    // `expected` happened to be `constructor` rendered the Object constructor's
    // source where a token's prose belongs — the opposite of the "a malformed
    // record degrades to a dump" contract directly below.
    const line = formatFailures([
      { gate: "shape", tool: "t", field: "f", observed: 1, expected: "constructor" },
    ] as Failure[])[0]!;
    expect(line).not.toContain("native code");
    expect(line).toContain("constructor");
  });

  it("degrades a malformed record to a dump rather than throwing", () => {
    expect(formatFailures([null, 7, { gate: "no-such-gate", tool: "x", observed: 1 }] as never)).
      toHaveLength(3);
  });

  it("returns an empty list for a non-array argument", () => {
    expect(formatFailures(null as never)).toEqual([]);
  });
});

// ─── findEditableParams ──────────────────────────────────────────────────────

/**
 * The STATIC PRECONDITION the AI write path needs at runtime.
 *
 * `reify_set_parameter` rewrites a parameter's DEFAULT LITERAL through alpha's
 * `resolve_param_default_span`, which returns None — and the write is refused —
 * for a cell that is not a `param` carrying a default. So the whole live
 * scenario hinges on a fact about the SOURCE, not about the engine, and that
 * fact is checkable in CI where the live run is not.
 *
 * Both directions matter. `CoreXY.y_rail_len` and `AFrame.rail_span_m` must be
 * editable, or edits 1 and 2 are refused before anything is measured. And
 * `CoreXY.x_rail_len` must stay a derived `let` — it is `y_rail_offset * 2`, the
 * formula that keeps the gantry spanning between the Y rails, and it is the B7
 * rejection subject: a plausible-looking cell id with no default literal, which
 * must produce a structured error and mutate nothing.
 */
describe("findEditableParams — (i) the write path's static precondition", () => {
  const SYNTHETIC = [
    "module demo",
    "",
    "pub structure Widget {",
    "    /// A doc comment must not confuse the scan.",
    "    param width : Length = 120mm",
    "    param depth : Length",
    "    let height = width * 2",
    "    realize solid {",
    "        let height = 9mm",
    "    }",
    "}",
    "",
    "pub structure Gadget {",
    "    let width = 4mm",
    "}",
  ].join("\n");

  it("reports a param carrying a default literal as editable", () => {
    expect(findEditableParams(SYNTHETIC, ["Widget.width"])).toEqual([
      { cell: "Widget.width", declared: "param", hasDefaultLiteral: true },
    ]);
  });

  it("reports a param with NO default as a param that cannot be written", () => {
    // `resolve_param_default_span` has no span to rewrite here, so the write is
    // refused even though the cell really is a parameter.
    expect(findEditableParams(SYNTHETIC, ["Widget.depth"])).toEqual([
      { cell: "Widget.depth", declared: "param", hasDefaultLiteral: false },
    ]);
  });

  it("reports a derived cell as a `let`", () => {
    expect(findEditableParams(SYNTHETIC, ["Widget.height"])).toEqual([
      { cell: "Widget.height", declared: "let", hasDefaultLiteral: true },
    ]);
  });

  it("reports an unknown member, and an unknown entity, as absent", () => {
    expect(findEditableParams(SYNTHETIC, ["Widget.nosuch", "Nosuch.width"])).toEqual([
      { cell: "Widget.nosuch", declared: "absent", hasDefaultLiteral: false },
      { cell: "Nosuch.width", declared: "absent", hasDefaultLiteral: false },
    ]);
  });

  it("scopes a member to its own entity — a same-named cell next door is not it", () => {
    expect(findEditableParams(SYNTHETIC, ["Gadget.width"])).toEqual([
      { cell: "Gadget.width", declared: "let", hasDefaultLiteral: true },
    ]);
  });

  it("ignores a declaration nested inside a block rather than reading it as a member", () => {
    // `Widget.height` is the top-level `let`, not the one inside `realize`.
    expect(findEditableParams(SYNTHETIC, ["Widget.height"])[0]!.hasDefaultLiteral).toBe(true);
    expect(findEditableParams(SYNTHETIC, ["Widget.solid"])).toEqual([
      { cell: "Widget.solid", declared: "absent", hasDefaultLiteral: false },
    ]);
  });

  it("returns one record per request, in the order asked", () => {
    expect(
      findEditableParams(SYNTHETIC, ["Widget.height", "Widget.width", "Widget.height"]).map(
        (r) => r.cell,
      ),
    ).toEqual(["Widget.height", "Widget.width", "Widget.height"]);
  });

  it.each([
    ["a null source", null, ["Widget.width"]],
    ["a non-string source", 7, ["Widget.width"]],
    ["a null cell list", SYNTHETIC, null],
    ["a non-string cell name", SYNTHETIC, [7]],
    ["a name with no dot", SYNTHETIC, ["width"]],
  ])("never throws for %s", (_name, source, cells) => {
    expect(() => findEditableParams(source as never, cells as never)).not.toThrow();
  });
});

describe("findEditableParams — (j) against the real prj/printer_v01/printer.ri", () => {
  const PRINTER_RI = fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", PRINTER_RELPATH),
    "utf8",
  );

  it("both edited cells are params carrying a default literal", () => {
    // Without this the live scenario never starts: `reify_set_parameter` refuses
    // a cell whose default literal has no span to rewrite, and the run would
    // report a write failure rather than the value-flow behaviour under test.
    expect(findEditableParams(PRINTER_RI, [Y_RAIL_LEN_CELL, RAIL_SPAN_CELL])).toEqual([
      { cell: Y_RAIL_LEN_CELL, declared: "param", hasDefaultLiteral: true },
      { cell: RAIL_SPAN_CELL, declared: "param", hasDefaultLiteral: true },
    ]);
  });

  it("the B7 rejection subject stays a derived `let`", () => {
    // `x_rail_len = y_rail_offset * 2` keeps the gantry spanning between the Y
    // rails. If a later refactor promoted it to a param, the B7 half of the gate
    // would silently start testing a WRITEABLE cell and stop testing rejection
    // at all — the disarmed-assertion failure mode, caught here instead.
    expect(findEditableParams(PRINTER_RI, [X_RAIL_LEN_CELL])).toEqual([
      { cell: X_RAIL_LEN_CELL, declared: "let", hasDefaultLiteral: true },
    ]);
  });
});

/**
 * THE CONSTANT-DRIFT GUARD the pin-selection cases above cannot be.
 *
 * `constraintsFor` builds its fixture `parameter_ids` from the very constants
 * the selector consumes, so every selection case passes identically if all four
 * spellings are wrong together — the trap `railLengtheningGate.mjs`'s
 * second-namespace docblock names in so many words ("a hand-built fixture
 * reproduces perfectly if it is written from the same wrong assumption"). It is
 * not hypothetical: this branch carries a commit fixing exactly that, and a
 * re-regression would surface only as PIN_ABSENT on a run that needs a GUI.
 *
 * So the spellings are re-derived from the SOURCE here. A constraint id
 * `Printer.<sub>.<member>` is well-formed only if printer.ri's `Printer` block
 * declares `sub <sub> = <Type>()` and pins `self.<sub>.<member>` — and `<Type>`
 * is what makes the values-namespace constant beside it right, which ties the
 * two namespaces to one reading of one file instead of to two assumptions.
 */
describe("the constraint-namespace constants — (j2) re-derived from printer.ri", () => {
  const PRINTER_RI = fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", PRINTER_RELPATH),
    "utf8",
  );

  /** The body of `pub structure <name> { … }`, by brace depth, comments stripped. */
  function entityBody(source: string, name: string): string[] {
    const lines = source.split("\n").map((l) => l.replace(/\/\/.*$/, ""));
    const start = lines.findIndex((l) =>
      new RegExp(`^\\s*(?:pub\\s+)?structure\\s+(?:def\\s+)?${name}\\b`).test(l),
    );
    expect(start, `printer.ri must declare structure ${name}`).toBeGreaterThanOrEqual(0);
    const body: string[] = [];
    let depth = 0;
    for (let i = start; i < lines.length; i += 1) {
      for (const ch of lines[i]!) {
        if (ch === "{") depth += 1;
        else if (ch === "}") depth -= 1;
      }
      if (i > start) body.push(lines[i]!);
      if (i > start && depth <= 0) break;
    }
    return body;
  }

  const PRINTER_BODY = entityBody(PRINTER_RI, "Printer");
  const CONSTRAINTS = PRINTER_BODY.filter((l) => /^\s*constraint\b/.test(l));

  /** `Printer.a_frame.rail_span_m` -> `self.a_frame.rail_span_m`. */
  const asSelfPath = (cell: string) => `self.${cell.slice("Printer.".length)}`;

  it("has a Printer block carrying constraints at all", () => {
    // Without this every assertion below is vacuous: an empty list satisfies no
    // `.some`, and `entityBody` returning nothing would look like a rename.
    expect(CONSTRAINTS.length).toBeGreaterThan(20);
  });

  it.each([
    ["the rail-span pin's A-frame half", PIN_RAIL_SPAN_CELL, "a_frame", "AFrame", RAIL_SPAN_CELL],
    ["the rail-span pin's motion half", PIN_Y_RAIL_LEN_CELL, "motion", "CoreXY", Y_RAIL_LEN_CELL],
    ["the ToolDock pin's travel half", PIN_TRAVEL_AVAIL_CELL, "a_frame", "AFrame", TRAVEL_AVAIL_CELL],
    ["the ToolDock pin's dock half", PIN_YH_MIN_TODAY_CELL, "tool_dock", "ToolDock", YH_MIN_TODAY_CELL],
  ])("%s is a real sub path, and its type matches the values-namespace constant", (
    _name,
    pinCell,
    sub,
    type,
    valuesCell,
  ) => {
    const [, member] = /^Printer\.(\w+)\.(\w+)$/.exec(pinCell)!.slice(1);
    expect(pinCell).toBe(`Printer.${sub}.${member}`);
    // The SUB really is declared, with the type the values-namespace constant
    // names — `build_values` keys on the TYPE, `build_constraints` on the sub.
    expect(PRINTER_BODY.some((l) => new RegExp(`^\\s*sub\\s+${sub}\\s*=\\s*${type}\\(`).test(l))).toBe(
      true,
    );
    expect(valuesCell).toBe(`${type}.${member}`);
  });

  it.each([
    ["the rail-span pin", [PIN_RAIL_SPAN_CELL, PIN_Y_RAIL_LEN_CELL]],
    ["the ToolDock pin", [PIN_YH_MIN_TODAY_CELL, PIN_TRAVEL_AVAIL_CELL]],
  ])("%s names a real ± slack constraint PAIR in the Printer block", (_name, cells) => {
    const paths = cells.map(asSelfPath);
    const matched = CONSTRAINTS.filter((l) => paths.every((path) => l.includes(path)));
    // A PAIR, not one line: printer.ri writes `x < y + slack` and
    // `x > y - slack`, which is why `foldPinStatus` reduces a list. One match
    // would mean the pin lost a half; none would mean a rename the live gate
    // could only report as PIN_ABSENT, on a run that needs a GUI.
    expect(matched).toHaveLength(2);
    expect(matched.filter((l) => l.includes("<"))).toHaveLength(1);
    expect(matched.filter((l) => l.includes(">"))).toHaveLength(1);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// The remaining PRD §7 rows. B4 is deliberately ABSENT from this file: the
// debug path discards the `StateDelta` by design (PRD §6.2 caveat (i), restated
// on `write_on_engine_and_refresh_baseline`), so no debug tool can return one
// and no JS predicate could ever be fed. It is asserted where the observable
// actually lives, driving that seam itself — `debug_server::tests::write_tools::
// write_helper_refreshes_the_delta_baseline`.
// ─────────────────────────────────────────────────────────────────────────────

/**
 * One complete B5 reading — every tracked cell and both pin statuses.
 *
 * Shared by the B5 block and the fold-into-one-verdict block below, because
 * `checkIdempotentReload` now demands a COMPLETE reading on both sides: a pair
 * of empty observations agrees on every key it was asked about, so grading them
 * clean was a pass on nothing at all.
 */
const SETTLED_READING = {
  cells: {
    [Y_RAIL_LEN_CELL]: { mm: 1100, freshness: "final" },
    [RAIL_SPAN_CELL]: { mm: 1100, freshness: "final" },
    [TRAVEL_AVAIL_CELL]: { mm: 810, freshness: "final" },
  },
  railSpanPinStatus: "Satisfied",
  toolDockPinStatus: "Violated",
};

/** The post-edit source the B2 cases read, with both params already rewritten. */
const EDITED_SOURCE = [
  "pub structure CoreXY {",
  "  param y_rail_len : Length = 1100mm",
  "  let x_rail_len = y_rail_offset * 2",
  "}",
  "pub structure AFrame {",
  "  param rail_span_m : Length = 1100mm",
  "}",
  "",
].join("\n");

const EDITED_LITERALS = {
  [Y_RAIL_LEN_CELL]: "1100mm",
  [RAIL_SPAN_CELL]: "1100mm",
};

describe("checkSourceCanonical — (k) B2, the edit is canonical ON DISK", () => {
  it("passes when both params carry the expected literal and the save is a no-op", () => {
    expect(
      checkSourceCanonical({
        source: EDITED_SOURCE,
        expected: EDITED_LITERALS,
        saveFile: { success: true },
        sourceAfterSave: EDITED_SOURCE,
      }),
    ).toEqual([]);
  });

  it("faults the cell whose default literal still reads the OLD value", () => {
    const stale = EDITED_SOURCE.replace("= 1100mm", "= 800mm");
    const failures = checkSourceCanonical({
      source: stale,
      expected: EDITED_LITERALS,
      saveFile: { success: true },
      sourceAfterSave: stale,
    }) as Failure[];
    expect(forGate(failures, "canonical")).toHaveLength(1);
    expect(failures[0]).toMatchObject({
      gate: "canonical",
      tool: "reify_open_file",
      field: `source[${Y_RAIL_LEN_CELL}]`,
      observed: "800mm",
      expected: "1100mm",
    });
  });

  it("is unit-preserving: the same magnitude in another unit is NOT canonical", () => {
    const metres = EDITED_SOURCE.replace(`param y_rail_len : Length = 1100mm`, "param y_rail_len : Length = 1.1m");
    const failures = checkSourceCanonical({
      source: metres,
      expected: EDITED_LITERALS,
      saveFile: { success: true },
      sourceAfterSave: metres,
    }) as Failure[];
    expect(forGate(failures, "canonical")).toHaveLength(1);
    expect(failures[0].observed).toBe("1.1m");
  });

  it("faults a cell the source declares as a `let` — it has no default to rewrite", () => {
    const failures = checkSourceCanonical({
      source: EDITED_SOURCE,
      expected: { [X_RAIL_LEN_CELL]: "900mm" },
      saveFile: { success: true },
      sourceAfterSave: EDITED_SOURCE,
    }) as Failure[];
    expect(failures[0]).toMatchObject({
      gate: "canonical",
      field: `source[${X_RAIL_LEN_CELL}]`,
      observed: "let",
    });
  });

  it("faults a save that CHANGED the source — B2's no-op half", () => {
    const failures = checkSourceCanonical({
      source: EDITED_SOURCE,
      expected: EDITED_LITERALS,
      saveFile: { success: true },
      sourceAfterSave: `${EDITED_SOURCE}// touched\n`,
    }) as Failure[];
    expect(forGate(failures, "canonical")).toHaveLength(1);
    expect(failures[0].field).toBe("source-after-save");
  });

  it.each([
    ["absent", undefined],
    ["null", null],
    ["an array", []],
    ["an object with no entries", {}],
  ])("faults an `expected` that is %s — a B2 reading with nothing to compare", (_name, expected) => {
    // Without this the whole literal-canonicity assertion — the POINT of B2 —
    // drops silently and the predicate returns clean having checked only the
    // save. `requires: ['sourceCanonical']` does not help: it asks whether the
    // reading is present, not whether it carries anything to grade.
    const failures = checkSourceCanonical({
      source: EDITED_SOURCE,
      expected,
      saveFile: { success: true },
      sourceAfterSave: EDITED_SOURCE,
    }) as Failure[];
    expect(forGate(failures, "shape")).toEqual([
      {
        gate: "shape",
        tool: "railLengtheningGate",
        field: "sourceCanonical.expected",
        observed: expected,
        expected: "object-with-at-least-one-entry",
      },
    ]);
  });

  it("reports a failed save as an OUTAGE, not as a canonical violation", () => {
    const failures = checkSourceCanonical({
      source: EDITED_SOURCE,
      expected: EDITED_LITERALS,
      saveFile: { error: "no active file" },
      sourceAfterSave: EDITED_SOURCE,
    }) as Failure[];
    expect(forGate(failures, "outage")).toHaveLength(1);
    expect(forGate(failures, "canonical")).toHaveLength(0);
    expect(failures[0].tool).toBe("reify_save_file");
  });
});

describe("checkFieldCoverage — (l) B3, fields beyond meshes/values stay live", () => {
  const covered = {
    meshes: [{}],
    values: [{}],
    constraints: [{ node_id: "a" }],
    files: [{ path: "printer.ri" }],
    compile_diagnostics: [],
    tessellation_diagnostics: [],
    stale: false,
    reload_error: null,
  };

  it("passes when every non-mesh/non-value field is present and the reload is clean", () => {
    expect(checkFieldCoverage(covered)).toEqual([]);
  });

  it("faults `stale: true` — the reload failed, so every field is LAST GOOD", () => {
    const failures = checkFieldCoverage({ ...covered, stale: true }) as Failure[];
    expect(forGate(failures, "coverage")).toHaveLength(1);
    expect(failures[0]).toMatchObject({
      gate: "coverage",
      tool: "engine_state",
      field: "stale",
      observed: true,
      expected: false,
    });
  });

  it("faults a non-null reload_error even when stale is false", () => {
    const failures = checkFieldCoverage({ ...covered, reload_error: "parse error" }) as Failure[];
    expect(forField(failures, "reload_error")).toHaveLength(1);
    expect(failures[0].observed).toBe("parse error");
  });

  it("faults an EMPTY constraints/files list — printer.ri realizes both", () => {
    const failures = checkFieldCoverage({ ...covered, constraints: [], files: [] }) as Failure[];
    expect(forField(failures, "constraints").length).toBe(1);
    expect(forField(failures, "files").length).toBe(1);
  });

  it("faults a diagnostics field that is missing entirely, as a shape problem", () => {
    const { tessellation_diagnostics: _drop, ...missing } = covered;
    const failures = checkFieldCoverage(missing) as Failure[];
    expect(forField(failures, "tessellation_diagnostics")).toHaveLength(1);
    expect(failures[0].gate).toBe("shape");
  });

  it("names more than one field when more than one is wrong — no early return", () => {
    const failures = checkFieldCoverage({ ...covered, stale: true, constraints: [] }) as Failure[];
    expect(failures.length).toBeGreaterThanOrEqual(2);
  });
});

describe("checkIdempotentReload — (m) B5, the watcher re-read adds no churn", () => {
  const settled = SETTLED_READING;

  it("passes when the post-debounce re-read is identical to the pre-debounce one", () => {
    expect(checkIdempotentReload({ before: settled, after: { ...settled } })).toEqual([]);
  });

  it.each([
    ["a reading carrying no cells at all", { before: { cells: {} }, after: { cells: {} } }],
    ["a reading with no `cells` key", { before: {}, after: {} }],
    ["one side missing a single tracked cell", {
      before: settled,
      after: { ...settled, cells: { [Y_RAIL_LEN_CELL]: settled.cells[Y_RAIL_LEN_CELL] } },
    }],
    ["a reading carrying no pin statuses", {
      before: { cells: settled.cells },
      after: { cells: settled.cells },
    }],
  ])("faults %s rather than grading two absences as agreement", (_name, observed) => {
    // THE VACUITY THIS ROW EXISTS TO REFUSE. Reading each key off two raw
    // objects compares `undefined` with `undefined` wherever a reading is
    // missing, so an empty pair agrees on everything and B5 passes having
    // observed nothing. Every fault here is `shape` — never tested — not
    // `reload`, which would claim the re-read moved something.
    const failures = checkIdempotentReload(observed) as Failure[];
    expect(failures.length).toBeGreaterThan(0);
    expect(forGate(failures, "reload")).toEqual([]);
    expect(failures.every((f) => f.gate === "shape")).toBe(true);
  });

  it("names the SIDE a missing reading is on, so the diagnosis is actionable", () => {
    const failures = checkIdempotentReload({
      before: settled,
      after: { ...settled, cells: {} },
    }) as Failure[];
    expect(failures.map((f) => f.field)).toEqual([
      `after.values[${Y_RAIL_LEN_CELL}]`,
      `after.values[${RAIL_SPAN_CELL}]`,
      `after.values[${TRAVEL_AVAIL_CELL}]`,
    ]);
  });

  it("faults a cell whose value MOVED across the debounce — a double-apply", () => {
    const drifted = {
      ...settled,
      cells: { ...settled.cells, [TRAVEL_AVAIL_CELL]: { mm: 1110, freshness: "final" } },
    };
    const failures = checkIdempotentReload({ before: settled, after: drifted }) as Failure[];
    expect(forGate(failures, "reload")).toHaveLength(1);
    expect(failures[0]).toMatchObject({
      gate: "reload",
      tool: "engine_state",
      field: `values[${TRAVEL_AVAIL_CELL}].mm`,
      observed: 1110,
      expected: 810,
    });
  });

  it("faults a pin that flipped across the debounce", () => {
    const flipped = { ...settled, railSpanPinStatus: "Violated" };
    const failures = checkIdempotentReload({ before: settled, after: flipped }) as Failure[];
    expect(failures[0]).toMatchObject({
      gate: "reload",
      field: `constraints[${RAIL_SPAN_PIN}].status`,
      observed: "Violated",
      expected: "Satisfied",
    });
  });

  it("faults a freshness that regressed across the debounce", () => {
    const stale = {
      ...settled,
      cells: { ...settled.cells, [Y_RAIL_LEN_CELL]: { mm: 1100, freshness: "stale" } },
    };
    const failures = checkIdempotentReload({ before: settled, after: stale }) as Failure[];
    expect(forField(failures, `values[${Y_RAIL_LEN_CELL}].freshness`)).toHaveLength(1);
  });

  it("reports churn in EVERY drifted cell, not just the first", () => {
    const both = {
      ...settled,
      cells: {
        ...settled.cells,
        [Y_RAIL_LEN_CELL]: { mm: 1, freshness: "final" },
        [RAIL_SPAN_CELL]: { mm: 2, freshness: "final" },
      },
    };
    expect((checkIdempotentReload({ before: settled, after: both }) as Failure[]).length).toBe(2);
  });
});

describe("checkRejectionAtomicity — (n) B7, a refused write mutates nothing", () => {
  const SRC = "param y_rail_len : Length = 1100mm\n";

  it("passes on a structured error with byte-identical source either side", () => {
    expect(
      checkRejectionAtomicity({
        tool: "reify_set_parameter",
        error: { error: "cell CoreXY.x_rail_len has no default literal to rewrite" },
        sourceBefore: SRC,
        sourceAfter: SRC,
      }),
    ).toEqual([]);
  });

  it("faults a write that SUCCEEDED where a rejection was required", () => {
    const failures = checkRejectionAtomicity({
      tool: "reify_set_parameter",
      error: { success: true, new_value: "900", unit: "mm" },
      sourceBefore: SRC,
      sourceAfter: SRC,
    }) as Failure[];
    expect(forGate(failures, "rejection")).toHaveLength(1);
    expect(failures[0].field).toBe("error");
  });

  it("faults a source that MOVED despite the rejection — a partial mutation", () => {
    const failures = checkRejectionAtomicity({
      tool: "reify_set_parameter",
      error: { error: "dimension mismatch" },
      sourceBefore: SRC,
      sourceAfter: "param y_rail_len : Length = 900mm\n",
    }) as Failure[];
    expect(forGate(failures, "rejection")).toHaveLength(1);
    expect(failures[0]).toMatchObject({
      gate: "rejection",
      field: "source",
      expected: SRC,
    });
  });

  it("faults an EMPTY error message — a rejection must say why", () => {
    const failures = checkRejectionAtomicity({
      tool: "reify_set_parameter",
      error: { error: "" },
      sourceBefore: SRC,
      sourceAfter: SRC,
    }) as Failure[];
    expect(forGate(failures, "rejection")).toHaveLength(1);
  });

  it.each([
    ["neither side", { sourceBefore: undefined, sourceAfter: undefined }],
    ["the before side", { sourceBefore: undefined, sourceAfter: SRC }],
    ["the after side", { sourceBefore: SRC, sourceAfter: undefined }],
    ["either side, when the file read back empty", { sourceBefore: "", sourceAfter: "" }],
  ])("faults a reading that observed the source on %s", (_name, sources) => {
    // `undefined !== undefined` is false, so a well-shaped `reify_open_file`
    // answer carrying no `source` string graded a full "the refused write
    // mutated nothing" pass having looked at the file on neither side. Every
    // sibling predicate here faults a non-string source by name; this one did
    // not, which made the omission inconsistent as well as vacuous.
    const failures = checkRejectionAtomicity({
      tool: "reify_set_parameter",
      error: { error: "no default literal" },
      ...sources,
    }) as Failure[];
    expect(failures.length).toBeGreaterThan(0);
    expect(failures.every((f) => f.gate === "shape" && f.expected === "non-empty-string")).toBe(
      true,
    );
  });

  it("reports BOTH a wrong verdict and a moved source in one call", () => {
    const failures = checkRejectionAtomicity({
      tool: "reify_set_parameter",
      error: { success: true },
      sourceBefore: SRC,
      sourceAfter: "changed\n",
    }) as Failure[];
    expect(failures.length).toBe(2);
  });
});

describe("the four new predicates never throw, for ANY argument", () => {
  const hostile = [
    null,
    undefined,
    0,
    "",
    [],
    {},
    { error: "boom" },
    { source: null, expected: null, saveFile: null, sourceAfterSave: null },
    { before: null, after: null },
    { cells: null, railSpanPinStatus: null, toolDockPinStatus: null },
  ];
  for (const [name, fn] of [
    ["checkSourceCanonical", checkSourceCanonical],
    ["checkFieldCoverage", checkFieldCoverage],
    ["checkIdempotentReload", checkIdempotentReload],
    ["checkRejectionAtomicity", checkRejectionAtomicity],
  ] as const) {
    it(`${name} returns an array of records instead of throwing`, () => {
      for (const arg of hostile) {
        const out = fn(arg as never);
        expect(Array.isArray(out)).toBe(true);
        expect(() => formatFailures(out as Failure[])).not.toThrow();
      }
    });
  }
});

describe("checkRailLengtheningGate — (o) the four rows fold into ONE verdict", () => {
  it("still passes the three measured states, which promise no extra rows", () => {
    for (const inputs of [BASELINE, AFTER_Y_RAIL, AFTER_RAIL_SPAN]) {
      expect((checkRailLengtheningGate(inputs) as Verdict).ok).toBe(true);
    }
  });

  it("grades a supplied reading and reports it in the ONE failures list", () => {
    const verdict = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      sourceCanonical: {
        source: EDITED_SOURCE.replace("= 1100mm", "= 800mm"),
        expected: EDITED_LITERALS,
        saveFile: { success: true },
        sourceAfterSave: EDITED_SOURCE.replace("= 1100mm", "= 800mm"),
      },
    }) as Verdict;
    expect(verdict.ok).toBe(false);
    expect(forGate(verdict.failures, "canonical")).toHaveLength(1);
  });

  it("faults a PROMISED row that was never exercised — the silent-skip hole", () => {
    const verdict = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      requires: ["rejectionAtomicity"],
    }) as Verdict;
    expect(verdict.ok).toBe(false);
    expect(forGate(verdict.failures, "vacuity")).toHaveLength(1);
    expect(verdict.failures[0]).toMatchObject({
      gate: "vacuity",
      field: "rejectionAtomicity",
      observed: undefined,
    });
  });

  it("faults an unrecognised name in `requires` rather than ignoring it", () => {
    const verdict = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      requires: ["noSuchRow"],
    }) as Verdict;
    expect(forField(verdict.failures, "requires")).toHaveLength(1);
    expect(verdict.failures[0].gate).toBe("shape");
  });

  it.each([
    ["a bare string", "rejectionAtomicity"],
    ["a number", 4],
    ["an object", { rejectionAtomicity: true }],
    ["null", null],
  ])("faults a `requires` that is %s rather than discarding it", (_name, requires) => {
    // Reading a malformed container as "promised nothing" reopens the exact
    // silent-skip hole `requires` exists to close, and does it wholesale: ONE
    // misspelling disarms every promise the run meant to make.
    const verdict = checkRailLengtheningGate({ ...AFTER_RAIL_SPAN, requires }) as Verdict;
    expect(verdict.ok).toBe(false);
    expect(forField(verdict.failures, "requires")).toEqual([
      {
        gate: "shape",
        tool: "railLengtheningGate",
        field: "requires",
        observed: requires,
        expected: "array",
      },
    ]);
  });

  it("refuses `readOrder` as a PROMISE — it grades the harness, not the subject", () => {
    // A healthy run parks no read-order marker, so promising the row would fail
    // every correct run. Naming it is a mistake about what `requires` means, and
    // is reported as one instead of becoming an unsatisfiable promise.
    const verdict = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      requires: ["readOrder"],
    }) as Verdict;
    expect(forField(verdict.failures, "requires")).toHaveLength(1);
    expect(verdict.failures[0]).toMatchObject({ gate: "shape", observed: "readOrder" });
    // …and the vacuity row it would otherwise have triggered did not fire.
    expect(forGate(verdict.failures, "vacuity")).toEqual([]);
  });

  it("grades EVERY B7 rejection when a list is supplied, not just the last", () => {
    const bad = { tool: "reify_set_parameter", error: { success: true }, sourceBefore: "a", sourceAfter: "a" };
    const verdict = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      requires: ["rejectionAtomicity"],
      rejectionAtomicity: [bad, bad],
    }) as Verdict;
    expect(forGate(verdict.failures, "rejection")).toHaveLength(2);
  });

  it("passes when a promised row IS supplied and clean", () => {
    const verdict = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      requires: ["sourceCanonical", "fieldCoverage", "idempotentReload", "rejectionAtomicity"],
      sourceCanonical: {
        source: EDITED_SOURCE,
        expected: EDITED_LITERALS,
        saveFile: { success: true },
        sourceAfterSave: EDITED_SOURCE,
      },
      fieldCoverage: {
        meshes: [{}],
        values: [{}],
        constraints: [{ node_id: "a" }],
        files: [{ path: "printer.ri" }],
        compile_diagnostics: [],
        tessellation_diagnostics: [],
        stale: false,
        reload_error: null,
      },
      idempotentReload: { before: SETTLED_READING, after: { ...SETTLED_READING } },
      rejectionAtomicity: [
        { tool: "reify_set_parameter", error: { error: "no default literal" }, sourceBefore: "a", sourceAfter: "a" },
      ],
    }) as Verdict;
    expect(verdict.failures).toEqual([]);
    expect(verdict.ok).toBe(true);
  });
});

describe("observeThenExtras — (p) the READ ORDER contract the driver cannot state", () => {
  /**
   * THE RULE: a phase's extra readings are taken only AFTER its own, and the
   * seam takes a thunk rather than an object so the caller cannot spell it
   * otherwise. Why the order decides PRD §7 B1 at all — and why getting it wrong
   * fails silently, in the direction that PASSES — is derived once, on
   * `observeThenExtras` in `./railLengtheningGate.mjs`.
   *
   * What this block adds is that the rule is a VALUE CI can execute, not a
   * comment in a driver CI can never run.
   */
  const trace: string[] = [];
  const recorded = (tag: string, value: unknown) => async () => {
    trace.push(`${tag}:start`);
    // A REAL suspension, not a bare `return`: an implementation that merely
    // calls the two thunks in source order and awaits both at the end still
    // interleaves here, and must fail.
    await Promise.resolve();
    trace.push(`${tag}:end`);
    return value;
  };

  it("resolves `observe` FULLY before `gatherExtras` is so much as invoked", async () => {
    trace.length = 0;
    await observeThenExtras(recorded("observe", BASELINE), recorded("extras", {}));
    expect(trace).toEqual(["observe:start", "observe:end", "extras:start", "extras:end"]);
  });

  it("merges the resolved extras OVER the observation, the shape gradePhase produced", async () => {
    const merged = (await observeThenExtras(
      async () => AFTER_RAIL_SPAN,
      async () => ({ requires: ["fieldCoverage"], fieldCoverage: { stale: true } }),
    )) as Record<string, unknown>;
    expect(merged).toMatchObject({
      phase: "after-rail-span",
      requires: ["fieldCoverage"],
      fieldCoverage: { stale: true },
    });
    // Merged OVER, so an extra wins a key collision — that is how a phase
    // re-reads a field the observation also carries.
    expect(
      ((await observeThenExtras(
        async () => ({ phase: "baseline", source: "old" }),
        async () => ({ source: "new" }),
      )) as Record<string, unknown>).source,
    ).toBe("new");
  });

  it("leaves the observation untouched when there are no extras (`undefined`)", async () => {
    expect(await observeThenExtras(async () => BASELINE, undefined)).toEqual(BASELINE);
    expect((checkRailLengtheningGate(await observeThenExtras(async () => BASELINE)) as Verdict).ok).toBe(
      true,
    );
  });

  it("REFUSES a plain object — the disarmed spelling — and reds the run instead", async () => {
    const disarmed = (await observeThenExtras(async () => AFTER_RAIL_SPAN, {
      requires: ["fieldCoverage"],
      fieldCoverage: { stale: true },
    } as never)) as Record<string, unknown>;
    // NOT merged: re-introducing the literal form must not quietly keep working.
    expect(disarmed.requires).toBeUndefined();
    expect(disarmed.fieldCoverage).toBeUndefined();

    const verdict = checkRailLengtheningGate(disarmed) as Verdict;
    expect(verdict.ok).toBe(false);
    const [failure, ...rest] = forGate(verdict.failures, "read-order");
    expect(rest).toEqual([]);
    expect(failure).toMatchObject({
      gate: "read-order",
      tool: "railLengtheningGate",
      field: "extras",
      observed: "object",
    });
  });

  it("surfaces a REJECTING thunk as a record, not as an exception", async () => {
    const out = await observeThenExtras(
      async () => AFTER_RAIL_SPAN,
      async () => {
        throw new Error("store_state went away");
      },
    );
    const verdict = checkRailLengtheningGate(out) as Verdict;
    expect(forGate(verdict.failures, "read-order")).toHaveLength(1);
    expect(forGate(verdict.failures, "read-order")[0]!.field).toBe("extras");
  });

  it("surfaces a non-thunk `observe` as a record naming THAT argument", async () => {
    const verdict = checkRailLengtheningGate(await observeThenExtras(null as never)) as Verdict;
    expect(forGate(verdict.failures, "read-order")).toHaveLength(1);
    expect(forGate(verdict.failures, "read-order")[0]!.field).toBe("observe");
  });

  it("never rejects, for ANY pair of arguments", async () => {
    const hostile = [null, undefined, 0, "", [], {}, { error: "boom" }, Symbol("x")];
    for (const observe of hostile) {
      for (const extras of hostile) {
        const out = await observeThenExtras(observe as never, extras as never);
        expect(out === null || typeof out !== "object").toBe(false);
        expect(() => formatFailures((checkRailLengtheningGate(out) as Verdict).failures)).not.toThrow();
      }
    }
  });

  it("renders the read-order record from its own fields, not from a fixed sentence", async () => {
    // The DATA reaches the line: the failure path the record identifies and the
    // `expected` token it carries. The surrounding explanation is prose owned by
    // `formatFailures` and pinned nowhere, so rewording it stays a one-line edit
    // — the same split every other rendering case in this file asserts on.
    const disarmed = await observeThenExtras(async () => AFTER_RAIL_SPAN, {} as never);
    const line = formatFailures(
      forGate((checkRailLengtheningGate(disarmed) as Verdict).failures, "read-order"),
    )[0]!;
    expect(line).toContain("railLengtheningGate.extras");
    expect(line).toContain("observeThenExtras");
  });
});
