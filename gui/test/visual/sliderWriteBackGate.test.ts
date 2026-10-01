/**
 * The CI half of the slider/edit-box write-back gate (task 7680): every
 * decision the live driver `./smoke_slider_write_back_e2e.mjs` makes, as pure
 * data, plus a static premise check on the committed fixture.
 *
 * Failure records are asserted on their stable `code`, never on message text.
 */
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, it, expect } from "vitest";
import { findEditableParams } from "./railLengtheningGate.mjs";
import {
  CELL_ID,
  DEFAULT_LITERAL,
  EDIT_BLUR_VALUE,
  EDIT_ENTER_FRAMES,
  EDIT_ENTER_VALUE,
  FIXTURE_RELPATH,
  JOINT_INDEX,
  JOINT_RANGE_DECL,
  JOINT_RANGE_MAX_MM,
  JOINT_RANGE_MIN_MM,
  LENGTH_TOLERANCE_MM,
  MECHANISM_CELL_ID,
  SLIDER_GATE_CODES,
  SLIDER_HOLD_FRAMES,
  SLIDER_HOLD_VALUE,
  SLIDER_RELEASE_VALUE,
  STL_EXTENT_TOLERANCE_MM,
  cellMm,
  checkCommitRewroteLiteral,
  checkExportExtent,
  checkHeldGestureLeftDiskUntouched,
  expectedSourceAfterCommit,
  formatFailures,
  paramDeclaration,
  parseBinaryStl,
  propInputSelector,
  sliderSelector,
} from "./sliderWriteBackGate.mjs";

const BASELINE = [
  "structure SliderWriteBack {",
  `    ${paramDeclaration(DEFAULT_LITERAL)}`,
  "    let plate = box(width, 20mm, 5mm)",
  "}",
  "",
].join("\n");

function occurrences(text: string, needle: string): number {
  return text.split(needle).length - 1;
}

const codes = (failures: Array<{ code: string }>) => failures.map((f) => f.code);

describe("the one-span splice expectation", () => {
  it("replaces exactly the one declaration's literal", () => {
    expect(expectedSourceAfterCommit(BASELINE, DEFAULT_LITERAL, "120mm")).toBe(
      BASELINE.replace(paramDeclaration(DEFAULT_LITERAL), paramDeclaration("120mm")),
    );
  });

  it("a baseline without the declaration is declaration-absent", () => {
    const result = expectedSourceAfterCommit("structure S {}\n", DEFAULT_LITERAL, "120mm");
    expect(result).toMatchObject({ code: SLIDER_GATE_CODES.declarationAbsent });
  });

  it("a baseline with it twice is declaration-ambiguous", () => {
    const twice = BASELINE + `// ${paramDeclaration(DEFAULT_LITERAL)}\n`;
    const result = expectedSourceAfterCommit(twice, DEFAULT_LITERAL, "120mm");
    expect(result).toMatchObject({ code: SLIDER_GATE_CODES.declarationAmbiguous, count: 2 });
  });
});

describe("checkCommitRewroteLiteral", () => {
  const spliced = BASELINE.replace(paramDeclaration(DEFAULT_LITERAL), paramDeclaration("120mm"));
  const check = (after: string) =>
    checkCommitRewroteLiteral({
      before: BASELINE,
      after,
      fromLiteral: DEFAULT_LITERAL,
      toLiteral: "120mm",
    });

  it("an exact splice passes", () => {
    expect(check(spliced)).toEqual([]);
  });

  it("an untouched disk is commit-not-written", () => {
    expect(codes(check(BASELINE))).toEqual([SLIDER_GATE_CODES.commitNotWritten]);
  });

  it("the splice plus any other change is unexpected-diff, carrying both texts", () => {
    const failures = check(spliced + " ");
    expect(codes(failures)).toEqual([SLIDER_GATE_CODES.unexpectedDiff]);
    expect(failures[0]).toMatchObject({ expected: spliced, after: spliced + " " });
  });

  it("a before-text without the from-declaration reports that premise, not a diff", () => {
    const failures = checkCommitRewroteLiteral({
      before: BASELINE,
      after: spliced,
      fromLiteral: "999mm",
      toLiteral: "120mm",
    });
    expect(codes(failures)).toEqual([SLIDER_GATE_CODES.declarationAbsent]);
  });
});

describe("checkHeldGestureLeftDiskUntouched", () => {
  const held = Number(SLIDER_HOLD_VALUE);

  it("a landed preview with an untouched disk passes", () => {
    expect(
      checkHeldGestureLeftDiskUntouched({
        baseline: BASELINE,
        afterHold: BASELINE,
        engineMm: held + LENGTH_TOLERANCE_MM / 2,
        heldMm: held,
      }),
    ).toEqual([]);
  });

  it("a changed disk is preview-wrote-disk", () => {
    const failures = checkHeldGestureLeftDiskUntouched({
      baseline: BASELINE,
      afterHold: BASELINE + "x",
      engineMm: held,
      heldMm: held,
    });
    expect(codes(failures)).toEqual([SLIDER_GATE_CODES.previewWroteDisk]);
  });

  // The vacuity guard: a tool that did nothing also leaves the disk untouched.
  it.each([
    ["no engine reading", undefined],
    ["a reading off by more than the tolerance", held + 2 * LENGTH_TOLERANCE_MM],
    ["the default still in force", Number.parseFloat(DEFAULT_LITERAL)],
  ])("%s is preview-not-landed", (_label, engineMm) => {
    const failures = checkHeldGestureLeftDiskUntouched({
      baseline: BASELINE,
      afterHold: BASELINE,
      engineMm,
      heldMm: held,
    });
    expect(codes(failures)).toEqual([SLIDER_GATE_CODES.previewNotLanded]);
  });
});

describe("cellMm", () => {
  it("scales the cell's si_value to millimetres", () => {
    const state = { values: [{ cell_id: "Other.x", si_value: 1 }, { cell_id: CELL_ID, si_value: 0.09 }] };
    expect(cellMm(state, CELL_ID)).toBeCloseTo(90, 9);
  });

  it.each([
    ["a missing cell", { values: [{ cell_id: "Other.x", si_value: 1 }] }],
    ["a non-finite si_value", { values: [{ cell_id: CELL_ID, si_value: null }] }],
    ["values not a list", { values: {} }],
    ["an {error} envelope", { error: "engine not ready" }],
    ["null", null],
    ["a string", "engine_state"],
  ])("%s reads undefined, never throws", (_label, state) => {
    expect(cellMm(state, CELL_ID)).toBeUndefined();
  });
});

/** A binary STL: 80-byte header, u32 LE count, 50 bytes per triangle. */
function binaryStl(triangles: number[][][]): Uint8Array {
  const bytes = new Uint8Array(84 + 50 * triangles.length);
  const view = new DataView(bytes.buffer);
  view.setUint32(80, triangles.length, true);
  triangles.forEach((vertices, t) => {
    const base = 84 + 50 * t + 12;
    vertices.flat().forEach((x, k) => view.setFloat32(base + 4 * k, x, true));
  });
  return bytes;
}

const TWO_TRIANGLES = [
  [
    [-20, -10, 0],
    [20, -10, 0],
    [20, 10, 2.5],
  ],
  [
    [-20, -10, 0],
    [20, 10, 2.5],
    [-20, 10, -2.5],
  ],
];

describe("parseBinaryStl", () => {
  it("reads the triangle count and the exact bounding box", () => {
    expect(parseBinaryStl(binaryStl(TWO_TRIANGLES))).toEqual({
      triangles: 2,
      min: [-20, -10, -2.5],
      max: [20, 10, 2.5],
    });
  });

  it("a length that is not 84 + 50n is stl-malformed (an ASCII STL included)", () => {
    const ascii = new TextEncoder().encode("solid plate\n" + " ".repeat(200) + "endsolid plate\n");
    expect(parseBinaryStl(ascii)).toMatchObject({ code: SLIDER_GATE_CODES.stlMalformed });
    const truncated = binaryStl(TWO_TRIANGLES).subarray(0, 84 + 50 + 7);
    expect(parseBinaryStl(truncated)).toMatchObject({ code: SLIDER_GATE_CODES.stlMalformed });
    expect(parseBinaryStl(new Uint8Array(10))).toMatchObject({ code: SLIDER_GATE_CODES.stlMalformed });
  });

  it("a zero-triangle file is stl-empty", () => {
    expect(parseBinaryStl(binaryStl([]))).toMatchObject({ code: SLIDER_GATE_CODES.stlEmpty });
  });
});

describe("checkExportExtent", () => {
  it("an X extent within tolerance passes", () => {
    expect(
      checkExportExtent({ bytes: binaryStl(TWO_TRIANGLES), expectedMm: 40 + STL_EXTENT_TOLERANCE_MM / 2 }),
    ).toEqual([]);
  });

  it("an X extent outside tolerance is export-extent-mismatch, naming both", () => {
    const failures = checkExportExtent({ bytes: binaryStl(TWO_TRIANGLES), expectedMm: 120 });
    expect(codes(failures)).toEqual([SLIDER_GATE_CODES.exportExtentMismatch]);
    expect(failures[0]).toMatchObject({ observedMm: 40, expectedMm: 120 });
  });

  it("an unparseable file reports the parse failure", () => {
    expect(codes(checkExportExtent({ bytes: new Uint8Array(3), expectedMm: 40 }))).toEqual([
      SLIDER_GATE_CODES.stlMalformed,
    ]);
  });
});

describe("formatFailures", () => {
  it("renders one line per failure, each naming its code", () => {
    const lines = formatFailures([
      { code: SLIDER_GATE_CODES.commitNotWritten, literal: "120mm" },
      { code: SLIDER_GATE_CODES.exportExtentMismatch, observedMm: 40, expectedMm: 120 },
    ]).split("\n");
    expect(lines).toHaveLength(2);
    expect(lines[0]).toContain(SLIDER_GATE_CODES.commitNotWritten);
    expect(lines[1]).toContain(SLIDER_GATE_CODES.exportExtentMismatch);
  });
});

describe("the strict selectors", () => {
  it("address the bound joint's slider inside its own mechanism section", () => {
    expect(sliderSelector()).toBe(
      `[data-testid="mechanism-section-${MECHANISM_CELL_ID}"] [data-testid="joint-row-${JOINT_INDEX}"] input[type="range"]`,
    );
  });

  it("address the param's PropertyEditor value input", () => {
    expect(propInputSelector()).toBe(`[data-testid="prop-row-${CELL_ID}"] input[type="text"]`);
  });
});

describe("the committed fixture holds every premise the live gate relies on", () => {
  const FIXTURE = fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", FIXTURE_RELPATH),
    "utf8",
  );

  it("declares the default literal exactly once, so the splice is unambiguous", () => {
    expect(occurrences(FIXTURE, paramDeclaration(DEFAULT_LITERAL))).toBe(1);
  });

  it("declares the cell as a param with a default literal (the INV-GUI-3 write-back precondition)", () => {
    expect(findEditableParams(FIXTURE, [CELL_ID])).toEqual([
      { cell: CELL_ID, declared: "param", hasDefaultLiteral: true },
    ]);
  });

  it("declares the joint range exactly once", () => {
    expect(occurrences(FIXTURE, JOINT_RANGE_DECL)).toBe(1);
  });

  it("every slider value is an integer mm inside the joint range (the slider's step is 1)", () => {
    for (const v of [...SLIDER_HOLD_FRAMES, SLIDER_HOLD_VALUE, SLIDER_RELEASE_VALUE]) {
      expect(v, v).toMatch(/^\d+$/);
      expect(Number(v), v).toBeGreaterThanOrEqual(JOINT_RANGE_MIN_MM);
      expect(Number(v), v).toBeLessThanOrEqual(JOINT_RANGE_MAX_MM);
    }
  });

  it("the edit-box literals are <number>mm", () => {
    for (const v of [EDIT_ENTER_VALUE, EDIT_BLUR_VALUE]) expect(v).toMatch(/^\d+(\.\d+)?mm$/);
    expect(EDIT_ENTER_FRAMES.every((f) => typeof f === "string")).toBe(true);
  });

  it("every phase moves the literal, so an unwritten commit cannot pass as a splice", () => {
    const chain = [DEFAULT_LITERAL, `${SLIDER_HOLD_VALUE}mm`, `${SLIDER_RELEASE_VALUE}mm`, EDIT_ENTER_VALUE, EDIT_BLUR_VALUE];
    for (let i = 1; i < chain.length; i += 1) expect(chain[i]).not.toBe(chain[i - 1]);
    expect(new Set(chain.slice(2)).has(DEFAULT_LITERAL)).toBe(false);
  });
});
