import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { GuiState, EvaluationStatus } from '../types';

// Mock Tauri API modules
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  getInitialState,
  setParameter,
  saveFile,
  exportGeometry,
  onMeshUpdate,
  onEvaluationStatus,
} from '../bridge';

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
    };
    mockInvoke.mockResolvedValue(mockState);

    const result = await getInitialState();

    expect(mockInvoke).toHaveBeenCalledWith('get_initial_state');
    expect(result).toEqual(mockState);
  });

  it('setParameter calls invoke with cellId and value', async () => {
    mockInvoke.mockResolvedValue(undefined);

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
});
