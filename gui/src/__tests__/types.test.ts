/**
 * Runtime tests for types.ts — specifically for convertRawMesh and convertRawGuiState.
 * Task 2959: verify the new optional scalar_channels and displaced_positions
 * fields are correctly converted from number[] → Float32Array.
 * Task 3229: verify compile_diagnostics is copied by convertRawGuiState.
 */
import { describe, it, expect } from 'vitest';
import { convertRawMesh, convertRawGuiState } from '../types';
import type { RawMeshData, RawGuiState, DiagnosticInfo, FeaDiagnosticInfo, FeaConvergenceInfo } from '../types';

describe('convertRawMesh', () => {
  it('converts scalar_channels number[] → Float32Array when present', () => {
    const raw: RawMeshData = {
      entity_path: 'FEA.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      scalar_channels: { vonMises: [10, 20, 30] },
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.scalar_channels).toBeDefined();
    expect(mesh.scalar_channels!['vonMises']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.scalar_channels!['vonMises'])).toEqual([10, 20, 30]);
  });

  it('converts multiple scalar channels independently', () => {
    const raw: RawMeshData = {
      entity_path: 'FEA.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      scalar_channels: {
        vonMises: [1, 2, 3],
        displacement_magnitude: [4, 5, 6],
      },
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.scalar_channels!['vonMises']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.scalar_channels!['vonMises'])).toEqual([1, 2, 3]);
    expect(mesh.scalar_channels!['displacement_magnitude']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.scalar_channels!['displacement_magnitude'])).toEqual([4, 5, 6]);
  });

  it('leaves scalar_channels undefined when absent from raw payload', () => {
    const raw: RawMeshData = {
      entity_path: 'Plain.body',
      vertices: [0, 0, 0],
      indices: [0],
      normals: null,
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.scalar_channels).toBeUndefined();
  });

  it('converts displaced_positions number[] → Float32Array when present', () => {
    const raw: RawMeshData = {
      entity_path: 'FEA.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      displaced_positions: [1, 2, 3, 4, 5, 6, 7, 8, 9],
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.displaced_positions).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.displaced_positions!)).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);
  });

  it('leaves displaced_positions undefined when absent from raw payload', () => {
    const raw: RawMeshData = {
      entity_path: 'Plain.body',
      vertices: [0, 0, 0],
      indices: [0],
      normals: null,
    };
    const mesh = convertRawMesh(raw);
    // undefined (not present) when field is absent from the raw payload
    expect(mesh.displaced_positions).toBeUndefined();
  });

  it('converts both scalar_channels and displaced_positions together', () => {
    const raw: RawMeshData = {
      entity_path: 'FEA.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      scalar_channels: { vonMises: [10, 20, 30] },
      displaced_positions: [1, 0, 0, 2, 0, 0, 3, 0, 0],
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.scalar_channels!['vonMises']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.scalar_channels!['vonMises'])).toEqual([10, 20, 30]);
    expect(mesh.displaced_positions).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.displaced_positions!)).toHaveLength(9);
  });

  // --- shell-extract fields (task 3597) ---

  it('converts vector_channels number[] → Float32Array when present', () => {
    const raw: RawMeshData = {
      entity_path: 'Shell.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      vector_channels: {
        shell_normal_per_face: [0, 0, 1],
        shell_tangent_per_vertex: [1, 0, 0, 0, 1, 0, 0, 0, 1],
      },
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.vector_channels).toBeDefined();
    expect(mesh.vector_channels!['shell_normal_per_face']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.vector_channels!['shell_normal_per_face'])).toEqual([0, 0, 1]);
    expect(mesh.vector_channels!['shell_tangent_per_vertex']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.vector_channels!['shell_tangent_per_vertex'])).toEqual([1, 0, 0, 0, 1, 0, 0, 0, 1]);
  });

  it('leaves vector_channels undefined when absent', () => {
    const raw: RawMeshData = {
      entity_path: 'Tet.body',
      vertices: [0, 0, 0],
      indices: [0],
      normals: null,
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.vector_channels).toBeUndefined();
  });

  it('converts element_kind number[] → Uint8Array when present', () => {
    const raw: RawMeshData = {
      entity_path: 'Shell.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1],
      indices: [0, 1, 2, 0, 2, 3],
      normals: null,
      element_kind: [0, 1],
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.element_kind).toBeInstanceOf(Uint8Array);
    expect(Array.from(mesh.element_kind!)).toEqual([0, 1]);
  });

  it('leaves element_kind undefined when absent', () => {
    const raw: RawMeshData = {
      entity_path: 'Tet.body',
      vertices: [0, 0, 0],
      indices: [0],
      normals: null,
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.element_kind).toBeUndefined();
  });

  it('converts region_tags number[] → Uint32Array when present', () => {
    const raw: RawMeshData = {
      entity_path: 'Shell.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      region_tags: [42],
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.region_tags).toBeInstanceOf(Uint32Array);
    expect(Array.from(mesh.region_tags!)).toEqual([42]);
  });

  it('leaves region_tags undefined when absent', () => {
    const raw: RawMeshData = {
      entity_path: 'Tet.body',
      vertices: [0, 0, 0],
      indices: [0],
      normals: null,
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.region_tags).toBeUndefined();
  });

  it('converts all three new shell-extract fields together', () => {
    const raw: RawMeshData = {
      entity_path: 'Shell.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      element_kind: [1],
      region_tags: [99],
      vector_channels: { shell_normal_per_face: [0, 0, 1] },
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.element_kind).toBeInstanceOf(Uint8Array);
    expect(Array.from(mesh.element_kind!)).toEqual([1]);
    expect(mesh.region_tags).toBeInstanceOf(Uint32Array);
    expect(Array.from(mesh.region_tags!)).toEqual([99]);
    expect(mesh.vector_channels!['shell_normal_per_face']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.vector_channels!['shell_normal_per_face'])).toEqual([0, 0, 1]);
  });

  // --- element_index field (task #4883) ---

  it('converts element_index number[] → Uint32Array when present', () => {
    const raw: RawMeshData = {
      entity_path: 'Shell.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1],
      indices: [0, 1, 2, 0, 2, 3],
      normals: null,
      element_index: [7, 9],
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.element_index).toBeInstanceOf(Uint32Array);
    expect(Array.from(mesh.element_index!)).toEqual([7, 9]);
  });

  it('leaves element_index undefined when absent', () => {
    const raw: RawMeshData = {
      entity_path: 'Tet.body',
      vertices: [0, 0, 0],
      indices: [0],
      normals: null,
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.element_index).toBeUndefined();
  });

  // --- appearance field (task 4770) ---

  it('carries appearance through when present in raw payload', () => {
    const raw: RawMeshData = {
      entity_path: 'Body.mesh',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      appearance: { color: [0.1, 0.2, 0.3, 1.0], metalness: 0.8, roughness: 0.4, finish: 1 },
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.appearance).toBeDefined();
    expect(mesh.appearance).toEqual({ color: [0.1, 0.2, 0.3, 1.0], metalness: 0.8, roughness: 0.4, finish: 1 });
  });

  it('leaves appearance undefined when absent from raw payload', () => {
    const raw: RawMeshData = {
      entity_path: 'Plain.body',
      vertices: [0, 0, 0],
      indices: [0],
      normals: null,
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.appearance).toBeUndefined();
  });
});

describe('convertRawGuiState', () => {
  it('copies compile_diagnostics from raw to converted state', () => {
    const diag: DiagnosticInfo = {
      file_path: 'test.ri',
      line: 5,
      column: 3,
      end_line: 5,
      end_column: 20,
      severity: 'Warning',
      message: "unknown port type 'Foo'",
      code: null,
    };
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [diag],
    };
    const state = convertRawGuiState(raw);
    expect(state.compile_diagnostics).toHaveLength(1);
    expect(state.compile_diagnostics[0].severity).toBe('Warning');
    expect(state.compile_diagnostics[0].message).toContain('unknown port type');
    expect(state.compile_diagnostics[0].file_path).toBe('test.ri');
  });

  // ── T0b: tensegrity_wires conversion tests ───────────────────────────────

  it('passes tensegrity_wires through from RawGuiState when present', () => {
    // RED until TensegrityWireData is added to types.ts and convertRawGuiState copies it.
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_wires: [
        { entity_path: 'TPrism', kind: 'strut', x1: 1.0, y1: 0.0, z1: 1.0, x2: 0.866, y2: 0.5, z2: 0.0 },
        { entity_path: 'TPrism', kind: 'cable', x1: 1.0, y1: 0.0, z1: 1.0, x2: -0.5, y2: 0.866, z2: 1.0 },
      ],
    };
    const state = convertRawGuiState(raw);
    expect(state.tensegrity_wires).toHaveLength(2);
    expect(state.tensegrity_wires[0].kind).toBe('strut');
    expect(state.tensegrity_wires[0].entity_path).toBe('TPrism');
    expect(state.tensegrity_wires[0].x1).toBe(1.0);
    expect(state.tensegrity_wires[0].x2).toBe(0.866);
    expect(state.tensegrity_wires[1].kind).toBe('cable');
  });

  it('yields tensegrity_wires: [] when the field is absent from RawGuiState', () => {
    // Forward-compat: older backend payloads without tensegrity_wires must not crash.
    // RED until convertRawGuiState uses the `?? []` default.
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      // tensegrity_wires intentionally omitted
    };
    const state = convertRawGuiState(raw);
    expect(state.tensegrity_wires).toEqual([]);
  });

  // ── β: tensegrity_surfaces conversion tests ──────────────────────────────

  it('passes tensegrity_surfaces through from RawGuiState when present', () => {
    // RED until TensegritySurfaceData is added to types.ts and convertRawGuiState copies it.
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      tensegrity_surfaces: [
        {
          entity_path: 'Patch',
          kind: 'membrane',
          i0: 0, i1: 1, i2: 2,
          x0: 0.0, y0: 0.0, z0: 0.0,
          x1: 1.0, y1: 0.0, z1: 0.0,
          x2: 0.5, y2: 0.866, z2: 0.0,
        },
        {
          entity_path: 'Patch',
          kind: 'membrane',
          i0: 1, i1: 3, i2: 2,
          x0: 1.0, y0: 0.0, z0: 0.0,
          x1: 1.5, y1: 0.866, z1: 0.0,
          x2: 0.5, y2: 0.866, z2: 0.0,
        },
      ],
    };
    const state = convertRawGuiState(raw);
    expect(state.tensegrity_surfaces).toHaveLength(2);
    expect(state.tensegrity_surfaces[0].kind).toBe('membrane');
    expect(state.tensegrity_surfaces[0].entity_path).toBe('Patch');
    expect(state.tensegrity_surfaces[0].i0).toBe(0);
    expect(state.tensegrity_surfaces[0].i1).toBe(1);
    expect(state.tensegrity_surfaces[0].i2).toBe(2);
    expect(state.tensegrity_surfaces[0].x0).toBe(0.0);
    expect(state.tensegrity_surfaces[0].y0).toBe(0.0);
    expect(state.tensegrity_surfaces[0].z0).toBe(0.0);
    expect(state.tensegrity_surfaces[0].x2).toBe(0.5);
    expect(state.tensegrity_surfaces[1].kind).toBe('membrane');
    expect(state.tensegrity_surfaces[1].i0).toBe(1);
  });

  it('yields tensegrity_surfaces: [] when the field is absent from RawGuiState', () => {
    // Forward-compat: older backend payloads without tensegrity_surfaces must not crash.
    // RED until convertRawGuiState uses the `?? []` default.
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      // tensegrity_surfaces intentionally omitted
    };
    const state = convertRawGuiState(raw);
    expect(state.tensegrity_surfaces).toEqual([]);
  });

  // ── PRD-3 γ: display_panes conversion tests ──────────────────────────────

  it('passes display_panes through from RawGuiState when present', () => {
    // RED until DisplayDirective interface + display_panes added to types.ts (step-8).
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      display_panes: [{ subject: 'S#realization[0]', pane: 1 }],
    };
    const state = convertRawGuiState(raw);
    expect(state.display_panes).toHaveLength(1);
    expect(state.display_panes[0].subject).toBe('S#realization[0]');
    expect(state.display_panes[0].pane).toBe(1);
  });

  it('yields display_panes: [] when the field is absent from RawGuiState', () => {
    // Forward-compat: older backend payloads without display_panes must not crash.
    // RED until convertRawGuiState uses the `?? []` default.
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      // display_panes intentionally omitted
    };
    const state = convertRawGuiState(raw);
    expect(state.display_panes).toEqual([]);
  });

  // ── PRD-2 γ: display_appearance conversion tests ─────────────────────────

  it('passes display_appearance through from RawGuiState when present', () => {
    // RED until DisplayStyleData + AppearanceDirective interfaces and
    // display_appearance are added to types.ts (step-6 GREEN).
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      display_appearance: [
        {
          subject: 'MyPart#realization[0]',
          style: {
            color: [0.96, 0.95, 0.88, 0.5] as [number, number, number, number],
            finish: 2,
            opacity: 0.5,
            wireframe: true,
          },
        },
      ],
    };
    const state = convertRawGuiState(raw);
    expect(state.display_appearance).toHaveLength(1);
    expect(state.display_appearance[0].subject).toBe('MyPart#realization[0]');
    expect(state.display_appearance[0].style.color).toEqual([0.96, 0.95, 0.88, 0.5]);
    expect(state.display_appearance[0].style.finish).toBe(2);
    expect(state.display_appearance[0].style.opacity).toBe(0.5);
    expect(state.display_appearance[0].style.wireframe).toBe(true);
  });

  it('yields display_appearance: [] when the field is absent from RawGuiState', () => {
    // Forward-compat: older backend payloads without display_appearance must not crash.
    // RED until convertRawGuiState uses the `?? []` default for display_appearance.
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      // display_appearance intentionally omitted
    };
    const state = convertRawGuiState(raw);
    expect(state.display_appearance).toEqual([]);
  });

  // ── #2966: fea_diagnostics conversion tests ──────────────────────────────

  it('maps Unconstrained fea_diagnostic from RawGuiState preserving rigid_body_modes', () => {
    // RED: FeaDiagnosticInfo type and fea_diagnostics field absent from types.ts.
    // fea_diagnostics rides only the full GuiState snapshot (#4818 wire contract).
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      fea_diagnostics: [
        { kind: 'Unconstrained', rigid_body_modes: ['TranslationX', 'TranslationY', 'RotationZ'] },
      ],
    };
    const state = convertRawGuiState(raw);
    const diags: FeaDiagnosticInfo[] = state.fea_diagnostics;
    expect(diags).toHaveLength(1);
    expect(diags[0].kind).toBe('Unconstrained');
    expect((diags[0] as { kind: 'Unconstrained'; rigid_body_modes: string[] }).rigid_body_modes).toEqual(
      ['TranslationX', 'TranslationY', 'RotationZ'],
    );
  });

  it('maps ProblemElements fea_diagnostic from RawGuiState preserving ids', () => {
    // RED: FeaDiagnosticInfo type and fea_diagnostics field absent from types.ts.
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      fea_diagnostics: [{ kind: 'ProblemElements', ids: [5, 12, 99] }],
    };
    const state = convertRawGuiState(raw);
    const diags: FeaDiagnosticInfo[] = state.fea_diagnostics;
    expect(diags).toHaveLength(1);
    expect(diags[0].kind).toBe('ProblemElements');
    expect((diags[0] as { kind: 'ProblemElements'; ids: number[] }).ids).toEqual([5, 12, 99]);
  });

  it('maps UnresolvedSelector fea_diagnostic from RawGuiState preserving selector_path', () => {
    // RED: FeaDiagnosticInfo type and fea_diagnostics field absent from types.ts.
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      fea_diagnostics: [{ kind: 'UnresolvedSelector', selector_path: 'Body.fea_load' }],
    };
    const state = convertRawGuiState(raw);
    const diags: FeaDiagnosticInfo[] = state.fea_diagnostics;
    expect(diags).toHaveLength(1);
    expect(diags[0].kind).toBe('UnresolvedSelector');
    expect((diags[0] as { kind: 'UnresolvedSelector'; selector_path: string }).selector_path).toBe(
      'Body.fea_load',
    );
  });

  it('preserves all six DofDirection bare strings through convertRawGuiState', () => {
    // Verifies wire shape: DofDirectionInfo serialises as bare strings (no wrapping).
    const modes = ['TranslationX', 'TranslationY', 'TranslationZ', 'RotationX', 'RotationY', 'RotationZ'];
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      // @ts-expect-error — rigid_body_modes: string[] is not assignable to DofDirectionInfo[]
      fea_diagnostics: [{ kind: 'Unconstrained', rigid_body_modes: modes }],
    };
    const state = convertRawGuiState(raw);
    const diags: FeaDiagnosticInfo[] = state.fea_diagnostics;
    const unc = diags[0] as { kind: 'Unconstrained'; rigid_body_modes: string[] };
    expect(unc.rigid_body_modes).toEqual(modes);
  });

  it('preserves input order for a mixed fea_diagnostics list', () => {
    // Verifies the mapping preserves ordering across variant types.
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      fea_diagnostics: [
        { kind: 'Unconstrained', rigid_body_modes: ['TranslationX'] },
        { kind: 'ProblemElements', ids: [7] },
        { kind: 'UnresolvedSelector', selector_path: 'some.path' },
      ],
    };
    const state = convertRawGuiState(raw);
    const diags: FeaDiagnosticInfo[] = state.fea_diagnostics;
    expect(diags).toHaveLength(3);
    expect(diags[0].kind).toBe('Unconstrained');
    expect(diags[1].kind).toBe('ProblemElements');
    expect(diags[2].kind).toBe('UnresolvedSelector');
  });

  it('yields fea_diagnostics: [] when field is absent from RawGuiState (forward-compat)', () => {
    // Mirrors the tensegrity_wires / display_panes forward-compat default.
    // RED: convertRawGuiState does not yet map fea_diagnostics.
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      // fea_diagnostics intentionally omitted
    };
    const state = convertRawGuiState(raw);
    expect(state.fea_diagnostics).toEqual([]);
  });

  // ── Task 3001 step-15: fea_convergence conversion tests ───────────────────

  it('maps fea_convergence from RawGuiState preserving converged + reason', () => {
    // RED: FeaConvergenceInfo type and fea_convergence field absent from types.ts.
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      fea_convergence: { converged: false, reason: 'MaxDofs' },
    };
    const state = convertRawGuiState(raw);
    const fc: FeaConvergenceInfo | null = state.fea_convergence;
    expect(fc).toEqual({ converged: false, reason: 'MaxDofs' });
  });

  it('maps a converged fea_convergence with no reason', () => {
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      fea_convergence: { converged: true },
    };
    const state = convertRawGuiState(raw);
    expect(state.fea_convergence).toEqual({ converged: true });
  });

  it('yields fea_convergence: null when field is absent from RawGuiState (forward-compat)', () => {
    // Mirrors the tensegrity_wires / display_panes forward-compat default, but
    // null (not []) since fea_convergence is a single optional object, not a list.
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
      // fea_convergence intentionally omitted
    };
    const state = convertRawGuiState(raw);
    expect(state.fea_convergence).toBeNull();
  });
});
