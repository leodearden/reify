import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { GuiState, RawGuiState, EvaluationStatus, MechanismDescriptor } from '../types';

// Mock Tauri API modules
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: vi.fn(),
  open: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  getInitialState,
  setParameter,
  saveFile,
  updateSource,
  exportGeometry,
  refreshFullState,
  getEntityTree,
  onMeshUpdate,
  onEvaluationStatus,
  onSerializationError,
  onTessellationDiagnostics,
  onCompileDiagnostics,
  pickOpenPath,
  onFocusEntity,
  onNavigateToSource,
  getKernelStatus,
  onKernelStatus,
  readViewSidecar,
  writeViewSidecar,
  getMechanismDescriptors,
  onAutoResolveIteration,
  onFileRemoved,
} from '../bridge';
import type { PersistentViewState } from '../types';
import type { KernelStatus } from '../bridge';
import { open } from '@tauri-apps/plugin-dialog';

const mockOpen = vi.mocked(open);

const mockInvoke = vi.mocked(invoke);
const mockListen = vi.mocked(listen);

beforeEach(() => {
  vi.clearAllMocks();
});

describe('bridge commands', () => {
  it('getInitialState calls invoke with correct command', async () => {
    const mockState: GuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
    };
    mockInvoke.mockResolvedValue(mockState);

    const result = await getInitialState();

    expect(mockInvoke).toHaveBeenCalledWith('get_initial_state');
    expect(result).toEqual(mockState);
  });

  it('setParameter calls invoke with cellId and value', async () => {
    const rawState: RawGuiState = { meshes: [], values: [], constraints: [], files: [], tessellation_diagnostics: [], compile_diagnostics: [] };
    mockInvoke.mockResolvedValue(rawState);

    await setParameter('cell_001', '42.0');

    expect(mockInvoke).toHaveBeenCalledWith('set_parameter', {
      cellId: 'cell_001',
      value: '42.0',
    });
  });

  it('saveFile calls invoke with both path and content', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await saveFile('/project/bracket.ri', 'structure Bracket {}');

    expect(mockInvoke).toHaveBeenCalledWith('save_file', {
      path: '/project/bracket.ri',
      content: 'structure Bracket {}',
    });
  });

  it('exportGeometry calls invoke with format and path', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await exportGeometry('step', '/tmp/output.step');

    expect(mockInvoke).toHaveBeenCalledWith('export', {
      format: 'step',
      path: '/tmp/output.step',
    });
  });

  // S5: setParameter should return a converted GuiState (not void)
  it('setParameter returns a GuiState with typed arrays', async () => {
    const rawState: RawGuiState = {
      meshes: [{ entity_path: 'Box.body', vertices: [0, 1, 2], indices: [0, 1, 2], normals: null }],
      values: [{ cell_id: 'c1', name: 'w', value: '10', unit: 'mm', determinacy: 'determined', entity_path: 'Box.w', kind: 'parameter', freshness: 'final' }],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
    };
    mockInvoke.mockResolvedValue(rawState);

    const result = await setParameter('c1', '42.0');

    expect(mockInvoke).toHaveBeenCalledWith('set_parameter', { cellId: 'c1', value: '42.0' });
    // result should be a converted GuiState with typed arrays
    expect(result).toBeDefined();
    expect(result.meshes[0].vertices).toBeInstanceOf(Float32Array);
    expect(result.meshes[0].indices).toBeInstanceOf(Uint32Array);
    expect(result.values).toHaveLength(1);
  });

  // S6: updateSource should return a converted GuiState (not void)
  it('updateSource returns a GuiState with typed arrays', async () => {
    const rawState: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [{ node_id: 'n1', expression: 'x > 0', status: 'satisfied', label: null, parameter_ids: [] }],
      files: [{ path: 'main.ri', content: 'updated' }],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
    };
    mockInvoke.mockResolvedValue(rawState);

    const result = await updateSource('main.ri', 'updated');

    expect(mockInvoke).toHaveBeenCalledWith('update_source', { path: 'main.ri', content: 'updated' });
    expect(result).toBeDefined();
    expect(result.constraints).toHaveLength(1);
    expect(result.files).toHaveLength(1);
  });

  it('getEntityTree calls invoke with get_entity_tree and returns payload', async () => {
    const sampleTree = [
      {
        entity_path: 'Bracket',
        kind: 'structure',
        type_name: null,
        has_mesh: false,
        trait_geometry: false,
        children: [],
      },
    ];
    mockInvoke.mockResolvedValue(sampleTree);

    const result = await getEntityTree();

    expect(mockInvoke).toHaveBeenCalledWith('get_entity_tree');
    expect(result).toEqual(sampleTree);
  });

  // S7: refreshFullState should call get_initial_state and return a converted GuiState
  it('refreshFullState calls get_initial_state and returns converted GuiState', async () => {
    const rawState: RawGuiState = {
      meshes: [{ entity_path: 'Box.body', vertices: [1, 2, 3], indices: [0, 1, 2], normals: null }],
      values: [],
      constraints: [],
      files: [{ path: 'main.ri', content: 'content' }],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
    };
    mockInvoke.mockResolvedValue(rawState);

    const result = await refreshFullState();

    expect(mockInvoke).toHaveBeenCalledWith('get_initial_state');
    expect(result).toBeDefined();
    expect(result.meshes[0].vertices).toBeInstanceOf(Float32Array);
    expect(result.meshes[0].indices).toBeInstanceOf(Uint32Array);
    expect(result.files).toHaveLength(1);
  });
});

describe('pickOpenPath', () => {
  it('calls Tauri dialog open() and returns selected path string', async () => {
    mockOpen.mockResolvedValue('/home/user/project/bracket.ri' as any);

    const result = await pickOpenPath();

    expect(mockOpen).toHaveBeenCalledWith({
      filters: [{ name: 'Reify files', extensions: ['ri'] }],
    });
    expect(result).toBe('/home/user/project/bracket.ri');
  });

  it('returns null when user cancels the dialog', async () => {
    mockOpen.mockResolvedValue(null as any);

    const result = await pickOpenPath();

    expect(result).toBeNull();
  });
});

describe('bridge event listeners', () => {
  it('onMeshUpdate calls listen with mesh-update event', async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const callback = vi.fn();
    const result = await onMeshUpdate(callback);

    expect(mockListen).toHaveBeenCalledWith('mesh-update', expect.any(Function));
    expect(result).toBe(unlisten);
  });

  it('onMeshUpdate extracts payload from event and calls callback with typed arrays', async () => {
    const unlisten = vi.fn();
    mockListen.mockImplementation(async (_event, handler) => {
      // Simulate Tauri calling the handler with raw wire-format data
      const rawMesh = {
        entity_path: 'Bracket.body',
        vertices: [0, 1, 2],
        indices: [0, 1, 2],
        normals: null,
      };
      (handler as (event: { payload: unknown }) => void)({ payload: rawMesh });
      return unlisten;
    });

    const callback = vi.fn();
    await onMeshUpdate(callback);

    const received = callback.mock.calls[0][0];
    expect(received.entity_path).toBe('Bracket.body');
    expect(received.vertices).toBeInstanceOf(Float32Array);
    expect(received.indices).toBeInstanceOf(Uint32Array);
    expect(received.normals).toBeNull();
  });

  it('onEvaluationStatus calls listen with evaluation-status event', async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const callback = vi.fn();
    await onEvaluationStatus(callback);

    expect(mockListen).toHaveBeenCalledWith('evaluation-status', expect.any(Function));
  });

  it('onMeshUpdate converts wire-format number[] arrays to typed arrays', async () => {
    const unlisten = vi.fn();
    mockListen.mockImplementation(async (_event, handler) => {
      // Simulate Tauri delivering raw JSON wire format (number[] arrays)
      const rawPayload = {
        entity_path: 'Bracket.body',
        vertices: [0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
        indices: [0, 1, 2],
        normals: [0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
      };
      (handler as (event: { payload: unknown }) => void)({ payload: rawPayload });
      return unlisten;
    });

    const callback = vi.fn();
    await onMeshUpdate(callback);

    const received = callback.mock.calls[0][0];
    expect(received.entity_path).toBe('Bracket.body');
    expect(received.vertices).toBeInstanceOf(Float32Array);
    expect(received.indices).toBeInstanceOf(Uint32Array);
    expect(received.normals).toBeInstanceOf(Float32Array);
    expect(Array.from(received.vertices)).toEqual([0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    expect(Array.from(received.indices)).toEqual([0, 1, 2]);
    expect(Array.from(received.normals)).toEqual([0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
  });

  it('onSerializationError subscribes to serialization-error event', async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const callback = vi.fn();
    const result = await onSerializationError(callback);

    expect(mockListen).toHaveBeenCalledWith('serialization-error', expect.any(Function));
    expect(result).toBe(unlisten);
  });

  it('onSerializationError passes payload to callback', async () => {
    const unlisten = vi.fn();
    mockListen.mockImplementation(async (_event, handler) => {
      const payload = { item_type: 'mesh', item_id: 'Bracket.body', error: 'non-finite f32' };
      (handler as (event: { payload: unknown }) => void)({ payload });
      return unlisten;
    });

    const callback = vi.fn();
    await onSerializationError(callback);

    expect(callback).toHaveBeenCalledWith({ item_type: 'mesh', item_id: 'Bracket.body', error: 'non-finite f32' });
  });

  it('onMeshUpdate converts null normals correctly', async () => {
    const unlisten = vi.fn();
    mockListen.mockImplementation(async (_event, handler) => {
      const rawPayload = {
        entity_path: 'Mount.body',
        vertices: [1.0, 2.0, 3.0],
        indices: [0, 1, 2],
        normals: null,
      };
      (handler as (event: { payload: unknown }) => void)({ payload: rawPayload });
      return unlisten;
    });

    const callback = vi.fn();
    await onMeshUpdate(callback);

    const received = callback.mock.calls[0][0];
    expect(received.vertices).toBeInstanceOf(Float32Array);
    expect(received.indices).toBeInstanceOf(Uint32Array);
    expect(received.normals).toBeNull();
  });

  it('onTessellationDiagnostics subscribes to tessellation-diagnostics event', async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const callback = vi.fn();
    const result = await onTessellationDiagnostics(callback);

    expect(mockListen).toHaveBeenCalledWith('tessellation-diagnostics', expect.any(Function));
    expect(result).toBe(unlisten);
  });

  it('onTessellationDiagnostics passes payload array to callback', async () => {
    const unlisten = vi.fn();
    mockListen.mockImplementation(async (_event, handler) => {
      const payload = [
        { file_path: '<unknown>', line: 1, column: 1, end_line: 1, end_column: 1,
          severity: 'Error', message: 'geometry error: kernel failure', code: null },
      ];
      (handler as (event: { payload: unknown }) => void)({ payload });
      return unlisten;
    });

    const callback = vi.fn();
    await onTessellationDiagnostics(callback);

    expect(callback).toHaveBeenCalledWith(
      expect.arrayContaining([
        expect.objectContaining({ severity: 'Error', message: 'geometry error: kernel failure' }),
      ])
    );
  });

  it("onCompileDiagnostics subscribes to 'compile-diagnostics' event", async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const callback = vi.fn();
    const result = await onCompileDiagnostics(callback);

    expect(mockListen).toHaveBeenCalledWith('compile-diagnostics', expect.any(Function));
    expect(result).toBe(unlisten);
  });

  it('onCompileDiagnostics passes DiagnosticInfo[] payload to callback', async () => {
    const unlisten = vi.fn();
    mockListen.mockImplementation(async (_event, handler) => {
      const payload = [
        { file_path: 'helper.ri', line: 3, column: 1, end_line: 3, end_column: 10,
          severity: 'Warning', message: "unknown port type 'Foo'", code: null },
      ];
      (handler as (event: { payload: unknown }) => void)({ payload });
      return unlisten;
    });

    const callback = vi.fn();
    await onCompileDiagnostics(callback);

    expect(callback).toHaveBeenCalledWith(
      expect.arrayContaining([
        expect.objectContaining({ severity: 'Warning', message: "unknown port type 'Foo'" }),
      ])
    );
  });

  it("onFocusEntity calls listen with 'focus-entity' event", async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const callback = vi.fn();
    const result = await onFocusEntity(callback);

    expect(mockListen).toHaveBeenCalledWith('focus-entity', expect.any(Function));
    expect(result).toBe(unlisten);
  });

  it('onFocusEntity passes string payload (entity_path) to callback', async () => {
    const unlisten = vi.fn();
    mockListen.mockImplementation(async (_name, handler) => {
      (handler as (event: { payload: unknown }) => void)({ payload: 'Bracket' });
      return unlisten;
    });

    const callback = vi.fn();
    await onFocusEntity(callback);

    expect(callback).toHaveBeenCalledWith('Bracket');
  });

  it("onNavigateToSource calls listen with 'navigate-to-source' event", async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const callback = vi.fn();
    const result = await onNavigateToSource(callback);

    expect(mockListen).toHaveBeenCalledWith('navigate-to-source', expect.any(Function));
    expect(result).toBe(unlisten);
  });

  it('onNavigateToSource passes {file, line, column, end_line, end_column} payload to callback', async () => {
    const unlisten = vi.fn();
    mockListen.mockImplementation(async (_name, handler) => {
      (handler as (event: { payload: unknown }) => void)({
        payload: { file: 'bracket.ri', line: 5, column: 3, end_line: 20, end_column: 7 },
      });
      return unlisten;
    });

    const callback = vi.fn();
    await onNavigateToSource(callback);

    expect(callback).toHaveBeenCalledWith({ file: 'bracket.ri', line: 5, column: 3, end_line: 20, end_column: 7 });
  });

  it("onKernelStatus calls listen with 'kernel-status' event", async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const callback = vi.fn();
    const result = await onKernelStatus(callback);

    expect(mockListen).toHaveBeenCalledWith('kernel-status', expect.any(Function));
    expect(result).toBe(unlisten);
  });

  it('onKernelStatus passes KernelStatus payload to callback', async () => {
    const unlisten = vi.fn();
    const sample: KernelStatus = { available: false, message: 'Geometry kernel not available — OCCT not linked' };
    mockListen.mockImplementation(async (_name, handler) => {
      (handler as (event: { payload: KernelStatus }) => void)({ payload: sample });
      return unlisten;
    });

    const callback = vi.fn();
    await onKernelStatus(callback);

    expect(callback).toHaveBeenCalledWith(sample);
  });
});

describe('bridge kernel commands', () => {
  it('getKernelStatus calls invoke with get_kernel_status and returns payload', async () => {
    const sample: KernelStatus = { available: false, message: 'Geometry kernel not available — OCCT not linked' };
    mockInvoke.mockResolvedValue(sample);

    const result = await getKernelStatus();

    expect(mockInvoke).toHaveBeenCalledWith('get_kernel_status');
    expect(result).toEqual(sample);
  });
});

describe('bridge def-preview commands', () => {
  it('getContainingDefinition invokes get_containing_definition with {line, col} and resolves to DefInfo', async () => {
    const mockDefInfo = { name: 'BoltFlange', kind: 'structure', span: { start: 0, end: 42 } };
    mockInvoke.mockResolvedValue(mockDefInfo);

    const { getContainingDefinition } = await import('../bridge');
    const result = await getContainingDefinition(7, 12);

    expect(mockInvoke).toHaveBeenCalledWith('get_containing_definition', { line: 7, col: 12 });
    expect(result).toEqual(mockDefInfo);
  });

  it('getContainingDefinition resolves to null when backend returns null', async () => {
    mockInvoke.mockResolvedValue(null);

    const { getContainingDefinition } = await import('../bridge');
    const result = await getContainingDefinition(1, 1);

    expect(result).toBeNull();
  });

  it('getEntityAtSourceLocation invokes get_entity_at_source_location with {line, col} and resolves to string', async () => {
    mockInvoke.mockResolvedValue('Bracket.width');

    const { getEntityAtSourceLocation } = await import('../bridge');
    const result = await getEntityAtSourceLocation(7, 12);

    expect(mockInvoke).toHaveBeenCalledWith('get_entity_at_source_location', { line: 7, col: 12 });
    expect(result).toEqual('Bracket.width');
  });

  it('getEntityAtSourceLocation resolves to null when backend returns null', async () => {
    mockInvoke.mockResolvedValue(null);

    const { getEntityAtSourceLocation } = await import('../bridge');
    const result = await getEntityAtSourceLocation(1, 1);

    expect(result).toBeNull();
  });

  it('getDefPreview invokes get_def_preview with {defName} and resolves to a converted GuiState', async () => {
    const rawState: RawGuiState = {
      meshes: [{ entity_path: 'BoltFlange.body', vertices: [0, 1, 2], indices: [0, 1, 2], normals: null }],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
    };
    mockInvoke.mockResolvedValue(rawState);

    const { getDefPreview } = await import('../bridge');
    const result = await getDefPreview('BoltFlange');

    expect(mockInvoke).toHaveBeenCalledWith('get_def_preview', { defName: 'BoltFlange' });
    expect(result.meshes[0].vertices).toBeInstanceOf(Float32Array);
    expect(result.meshes[0].indices).toBeInstanceOf(Uint32Array);
    expect(result.meshes[0].entity_path).toBe('BoltFlange.body');
  });
});

// --- View sidecar bridge commands (step-9) ---

const samplePersistentState: PersistentViewState = {
  version: '2',
  activeViewId: 'auto:default',
  userViews: [],
  explicit: {},
  viewportCameras: {},
  timestamp: '2026-01-01T00:00:00Z',
};

describe('bridge view sidecar commands', () => {
  it('readViewSidecar invokes read_view_sidecar with { riPath }', async () => {
    mockInvoke.mockResolvedValue(samplePersistentState);

    await readViewSidecar('/project/bracket.ri');

    expect(mockInvoke).toHaveBeenCalledWith('read_view_sidecar', {
      riPath: '/project/bracket.ri',
    });
  });

  it('readViewSidecar returns null when invoke resolves null', async () => {
    mockInvoke.mockResolvedValue(null);

    const result = await readViewSidecar('/project/bracket.ri');

    expect(result).toBeNull();
  });

  it('readViewSidecar returns parsed PersistentViewState when invoke resolves a valid payload', async () => {
    mockInvoke.mockResolvedValue(samplePersistentState);

    const result = await readViewSidecar('/project/bracket.ri');

    expect(result).toEqual(samplePersistentState);
  });

  it('writeViewSidecar invokes write_view_sidecar with { riPath, state }', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await writeViewSidecar('/project/bracket.ri', samplePersistentState);

    expect(mockInvoke).toHaveBeenCalledWith('write_view_sidecar', {
      riPath: '/project/bracket.ri',
      state: samplePersistentState,
    });
  });

  it('writeViewSidecar propagates invoke rejections', async () => {
    mockInvoke.mockRejectedValue(new Error('disk full'));

    await expect(
      writeViewSidecar('/project/bracket.ri', samplePersistentState),
    ).rejects.toThrow('disk full');
  });
});

// --- Mechanism descriptor bridge commands (step-15) ---

describe('bridge mechanism commands', () => {
  it('getMechanismDescriptors invokes the Tauri command and returns the typed array', async () => {
    const sampleDescriptors: MechanismDescriptor[] = [
      {
        cell_id: 'Kinematic.m',
        entity_path: 'Kinematic',
        name: 'm',
        bodies_count: 2,
        joints: [
          {
            joint_index: 0,
            kind: 'prismatic',
            dimension: 'length',
            range_lower_si: 0.0,
            range_upper_si: 0.8,
            axis: [0, 1, 0],
            driving_param_cell_id: 'Kinematic.y_pos',
            current_value_si: 0.1,
            binding: { kind: 'param_bound' as const, param_cell_id: 'Kinematic.y_pos', current_value_si: 0.1 },
          },
        ],
      },
    ];
    mockInvoke.mockResolvedValue(sampleDescriptors);

    const result = await getMechanismDescriptors();

    expect(mockInvoke).toHaveBeenCalledWith('get_mechanism_descriptors');
    expect(result).toEqual(sampleDescriptors);
    expect(result[0].joints[0].joint_index).toBe(0);
    expect(result[0].joints[0].driving_param_cell_id).toBe('Kinematic.y_pos');
  });

  it('getMechanismDescriptors returns empty array when no mechanisms exist', async () => {
    mockInvoke.mockResolvedValue([]);

    const result = await getMechanismDescriptors();

    expect(mockInvoke).toHaveBeenCalledWith('get_mechanism_descriptors');
    expect(result).toEqual([]);
  });
});

// --- onAutoResolveIteration malformed payload rejection (task-3407) ---

describe('onAutoResolveIteration malformed payload', () => {
  let warnSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
  });

  afterEach(() => {
    warnSpy.mockRestore();
  });

  function makeHandler(payload: unknown) {
    return async (_name: unknown, handler: unknown) => {
      (handler as (event: { payload: unknown }) => void)({ payload });
      return vi.fn();
    };
  }

  it('drops null payload', async () => {
    mockListen.mockImplementation(makeHandler(null) as any);
    const cb = vi.fn();
    await onAutoResolveIteration(cb);
    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
  });

  it('drops primitive (number) payload', async () => {
    mockListen.mockImplementation(makeHandler(42) as any);
    const cb = vi.fn();
    await onAutoResolveIteration(cb);
    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
  });

  it('drops payload missing iteration field', async () => {
    mockListen.mockImplementation(makeHandler({ parameters: {}, constraints: {} }) as any);
    const cb = vi.fn();
    await onAutoResolveIteration(cb);
    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
  });

  it('drops payload with array-shaped parameters', async () => {
    mockListen.mockImplementation(
      makeHandler({ iteration: 0, parameters: [], constraints: {} }) as any,
    );
    const cb = vi.fn();
    await onAutoResolveIteration(cb);
    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
  });

  it('drops payload with array-shaped constraints', async () => {
    mockListen.mockImplementation(
      makeHandler({ iteration: 0, parameters: {}, constraints: [] }) as any,
    );
    const cb = vi.fn();
    await onAutoResolveIteration(cb);
    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
  });

  it('drops payload with primitive-string parameters', async () => {
    mockListen.mockImplementation(
      makeHandler({ iteration: 0, parameters: 'not-an-object', constraints: {} }) as any,
    );
    const cb = vi.fn();
    await onAutoResolveIteration(cb);
    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
  });

  it('drops payload with non-numeric iteration', async () => {
    mockListen.mockImplementation(
      makeHandler({ iteration: '0', parameters: {}, constraints: {} }) as any,
    );
    const cb = vi.fn();
    await onAutoResolveIteration(cb);
    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
  });

  it('does NOT warn and calls callback for a well-formed payload', async () => {
    const wellFormed = {
      iteration: 0,
      parameters: { p1: { value: 1, unit: 'mm' } },
      constraints: {},
    };
    mockListen.mockImplementation(makeHandler(wellFormed) as any);
    const cb = vi.fn();
    await onAutoResolveIteration(cb);
    expect(cb).toHaveBeenCalledWith(wellFormed);
    expect(warnSpy).not.toHaveBeenCalled();
  });
});

// ─── onFileRemoved (step-21) ──────────────────────────────────────────────────

describe('onFileRemoved', () => {
  it('(a) subscribes to the file-removed Tauri event channel', async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const callback = vi.fn();
    const result = await onFileRemoved(callback);

    expect(mockListen).toHaveBeenCalledWith('file-removed', expect.any(Function));
    expect(result).toBe(unlisten);
  });

  it('(b) forwards { path } payload to the callback', async () => {
    const unlisten = vi.fn();
    mockListen.mockImplementation(async (_event, handler) => {
      (handler as (event: { payload: unknown }) => void)({ payload: { path: '/a/foo.ri' } });
      return unlisten;
    });

    const callback = vi.fn();
    await onFileRemoved(callback);

    expect(callback).toHaveBeenCalledWith({ path: '/a/foo.ri' });
  });

  it('(c) returned UnlistenFn is the one returned by listen()', async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const callback = vi.fn();
    const result = await onFileRemoved(callback);

    expect(result).toBe(unlisten);
  });
});
