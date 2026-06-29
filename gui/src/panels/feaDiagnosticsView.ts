/**
 * Pure view-model formatter for FEA diagnostics (#2966, step-12).
 *
 * feaDiagnosticRows() maps each FeaDiagnosticInfo variant to a display row
 * {kind, label, detail} for rendering in FeaDiagnosticsPanel.
 *
 * Modeled on diagnosticsView.ts — pure string/array formatting, no SolidJS
 * or THREE.js dependencies. Unit-tested independently of the component.
 */

import type { FeaDiagnosticInfo } from '../types';

// ---------------------------------------------------------------------------
// Row type
// ---------------------------------------------------------------------------

/**
 * One display row for the FEA diagnostics panel.
 *
 * - `kind`: mirrors `FeaDiagnosticInfo['kind']` — used by the component for
 *   icon or styling differentiation.
 * - `label`: short primary description shown as the row header.
 * - `detail`: secondary description with the specific data payload.
 */
export interface FeaDiagnosticRow {
  kind: FeaDiagnosticInfo['kind'];
  label: string;
  detail: string;
}

// ---------------------------------------------------------------------------
// Formatter
// ---------------------------------------------------------------------------

/**
 * Map a list of FeaDiagnosticInfo values to display rows for the panel.
 *
 * Pure function — no side effects. Returns `[]` for an empty input.
 * Preserves input order.
 */
export function feaDiagnosticRows(diagnostics: FeaDiagnosticInfo[]): FeaDiagnosticRow[] {
  return diagnostics.map(formatDiagnostic);
}

function formatDiagnostic(diag: FeaDiagnosticInfo): FeaDiagnosticRow {
  switch (diag.kind) {
    case 'Unconstrained':
      return {
        kind: 'Unconstrained',
        label: 'Unconstrained body (rigid-body modes)',
        detail: diag.rigid_body_modes.join(', '),
      };

    case 'ProblemElements': {
      const n = diag.ids.length;
      return {
        kind: 'ProblemElements',
        label: `${n} problem element${n === 1 ? '' : 's'}`,
        detail: `Element IDs: ${diag.ids.join(', ')}`,
      };
    }

    case 'UnresolvedSelector':
      return {
        kind: 'UnresolvedSelector',
        label: 'Unresolved selector',
        detail: diag.selector_path,
      };
  }
}
