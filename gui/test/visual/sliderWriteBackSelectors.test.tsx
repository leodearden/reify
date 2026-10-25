/**
 * The live slider write-back gate (`./smoke_slider_write_back_e2e.mjs`) aims
 * its tools by `sliderSelector()` and `propInputSelector()`, and it cannot run
 * in CI. This suite renders the REAL MechanismPanel and PropertyEditor with the
 * gate's own ids, so a component whose DOM drifts away from either selector
 * goes red here rather than only on a live display.
 *
 * Each render carries a sibling of the target (a second joint, a second param)
 * so "exactly one match" also proves the selector is scoped, not merely present.
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";

import { MechanismPanel } from "../../src/panels/MechanismPanel";
import { PropertyEditor } from "../../src/panels/PropertyEditor";
import type { JointDescriptor, ValueData } from "../../src/types";
import {
  CELL_ID,
  JOINT_INDEX,
  MECHANISM_CELL_ID,
  propInputSelector,
  sliderSelector,
} from "./sliderWriteBackGate.mjs";

const SIBLING_JOINT_INDEX = JOINT_INDEX + 1;
const SIBLING_CELL_ID = `${CELL_ID}_sibling`;

function prismaticJoint(jointIndex: number, paramCellId: string): JointDescriptor {
  return {
    joint_index: jointIndex,
    kind: "prismatic",
    dimension: "length",
    range_lower_si: 0,
    range_upper_si: 0.2,
    axis: [1, 0, 0],
    driving_param_cell_id: paramCellId,
    current_value_si: 0.04,
    binding: { kind: "param_bound", param_cell_id: paramCellId, current_value_si: 0.04 },
  };
}

function lengthParam(cellId: string): ValueData {
  return {
    cell_id: cellId,
    name: cellId.split(".").pop()!,
    value: "40",
    unit: "mm",
    determinacy: "determined",
    entity_path: cellId,
    kind: "Param",
    freshness: "final",
    dimension: "Length",
    si_value: 0.04,
  };
}

afterEach(() => {
  cleanup();
});

describe("the live gate's selectors match the real components exactly once", () => {
  it("sliderSelector() finds the bound joint's range input", () => {
    render(() => (
      <MechanismPanel
        descriptors={[
          {
            cell_id: MECHANISM_CELL_ID,
            entity_path: MECHANISM_CELL_ID.split(".")[0],
            name: "m1",
            bodies_count: 3,
            joints: [
              prismaticJoint(JOINT_INDEX, CELL_ID),
              prismaticJoint(SIBLING_JOINT_INDEX, SIBLING_CELL_ID),
            ],
          },
        ]}
        onPreviewParameter={vi.fn()}
        onSetParameter={vi.fn()}
        onScrubLocal={vi.fn()}
      />
    ));

    expect(document.querySelectorAll('input[type="range"]')).toHaveLength(2);
    expect(document.querySelectorAll(sliderSelector())).toHaveLength(1);
  });

  it("propInputSelector() finds the param's PropertyEditor value input", () => {
    render(() => (
      <PropertyEditor
        values={{ [CELL_ID]: lengthParam(CELL_ID), [SIBLING_CELL_ID]: lengthParam(SIBLING_CELL_ID) }}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        unitLadders={undefined}
      />
    ));

    expect(document.querySelectorAll('[data-testid^="prop-row-"] input[type="text"]')).toHaveLength(2);
    expect(document.querySelectorAll(propInputSelector())).toHaveLength(1);
  });
});
