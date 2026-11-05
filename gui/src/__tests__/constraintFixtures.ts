/**
 * Wire-format constraint fixtures shared by the suites that drive the verdict
 * boundary (`convertRawConstraint`) through its two ingress paths: the
 * whole-state snapshot (./types.test.ts) and the `constraint-update` delta
 * (./bridge.test.ts).
 *
 * The status parameter is a plain `string` because these fixtures exist to
 * feed the boundary out-of-contract tokens as well as the three accepted ones.
 * No `.test.` segment, so vitest does not collect this module as a suite.
 */
import type { RawConstraintData } from '../types';

export function makeRawConstraint(status: string): RawConstraintData {
  return {
    node_id: 'Box#constraint[0]',
    expression: 'w > 0',
    status,
    label: 'positive width',
    parameter_ids: ['Box.w'],
  };
}
