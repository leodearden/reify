import { describe, it, expect, vi } from 'vitest';
import { createRoot } from 'solid-js';
import type { MechanismDescriptor, JointDescriptor } from '../types';
import { createMechanismStore } from '../stores/mechanismStore';
import * as _mechanismStoreModule from '../stores/mechanismStore';

// ── Fixture helpers ──────────────────────────────────────────────────────────

function makeDescriptor(overrides: Partial<MechanismDescriptor> & { cell_id: string }): MechanismDescriptor {
  return {
    cell_id: overrides.cell_id,
    entity_path: overrides.entity_path ?? 'Kinematic',
    name: overrides.name ?? overrides.cell_id.split('.').at(-1) ?? 'm',
    bodies_count: overrides.bodies_count ?? 2,
    joints: overrides.joints ?? [
      {
        joint_index: 0,
        kind: 'prismatic',
        dimension: 'length',
        range_lower_si: 0.0,
        range_upper_si: 0.8,
        axis: [0, 1, 0],
        driving_param_cell_id: 'Kinematic.y_pos',
        current_value_si: 0.1,
        binding: { kind: 'param_bound', param_cell_id: 'Kinematic.y_pos', current_value_si: 0.1 },
      },
    ],
  };
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('createMechanismStore', () => {
  describe('initial state', () => {
    it('has empty descriptors', () => {
      createRoot((dispose) => {
        const getMechanismDescriptors = vi.fn().mockResolvedValue([]);
        const store = createMechanismStore({ getMechanismDescriptors });
        expect(store.state.descriptors).toEqual([]);
        dispose();
      });
    });

    it('has empty optimistic overrides', () => {
      createRoot((dispose) => {
        const getMechanismDescriptors = vi.fn().mockResolvedValue([]);
        const store = createMechanismStore({ getMechanismDescriptors });
        expect(store.state.optimistic).toEqual({});
        dispose();
      });
    });
  });

  describe('refresh()', () => {
    it('calls getMechanismDescriptors and populates state.descriptors', async () => {
      await new Promise<void>((resolve) => {
        createRoot(async (dispose) => {
          const desc = makeDescriptor({ cell_id: 'Kinematic.m' });
          const getMechanismDescriptors = vi.fn().mockResolvedValue([desc]);
          const store = createMechanismStore({ getMechanismDescriptors });

          await store.refresh();

          expect(getMechanismDescriptors).toHaveBeenCalledTimes(1);
          expect(store.state.descriptors).toHaveLength(1);
          expect(store.state.descriptors[0].cell_id).toBe('Kinematic.m');
          dispose();
          resolve();
        });
      });
    });

    it('replaces previous descriptors on subsequent refresh', async () => {
      await new Promise<void>((resolve) => {
        createRoot(async (dispose) => {
          const desc1 = makeDescriptor({ cell_id: 'A.m' });
          const desc2 = makeDescriptor({ cell_id: 'B.m' });
          let callCount = 0;
          const getMechanismDescriptors = vi.fn().mockImplementation(async () => {
            callCount++;
            return callCount === 1 ? [desc1] : [desc2];
          });
          const store = createMechanismStore({ getMechanismDescriptors });

          await store.refresh();
          expect(store.state.descriptors[0].cell_id).toBe('A.m');

          await store.refresh();
          expect(store.state.descriptors[0].cell_id).toBe('B.m');
          expect(store.state.descriptors).toHaveLength(1);
          dispose();
          resolve();
        });
      });
    });
  });

  describe('setOptimistic() and getEffectiveValueSi()', () => {
    it('records an optimistic override keyed by cellId:jointIndex', () => {
      createRoot((dispose) => {
        const store = createMechanismStore({ getMechanismDescriptors: vi.fn().mockResolvedValue([]) });
        store.setOptimistic('Kinematic.m', 0, 0.45);
        expect(store.state.optimistic['Kinematic.m:0']).toBe(0.45);
        dispose();
      });
    });

    it('getEffectiveValueSi returns optimistic override over descriptor current_value_si', () => {
      createRoot((dispose) => {
        const store = createMechanismStore({ getMechanismDescriptors: vi.fn().mockResolvedValue([]) });
        store.setOptimistic('Kinematic.m', 0, 0.45);
        const effective = store.getEffectiveValueSi('Kinematic.m', 0, 0.1);
        expect(effective).toBe(0.45);
        dispose();
      });
    });

    it('getEffectiveValueSi returns fallback when no optimistic override exists', () => {
      createRoot((dispose) => {
        const store = createMechanismStore({ getMechanismDescriptors: vi.fn().mockResolvedValue([]) });
        const effective = store.getEffectiveValueSi('Kinematic.m', 0, 0.1);
        expect(effective).toBe(0.1);
        dispose();
      });
    });

    it('getEffectiveValueSi returns null when no override and fallback is null', () => {
      createRoot((dispose) => {
        const store = createMechanismStore({ getMechanismDescriptors: vi.fn().mockResolvedValue([]) });
        const effective = store.getEffectiveValueSi('Kinematic.m', 0, null);
        expect(effective).toBeNull();
        dispose();
      });
    });
  });

  describe('refresh() clears committed optimistic overrides', () => {
    it('clears overrides whose value matches new descriptor current_value_si (commit confirmation)', async () => {
      await new Promise<void>((resolve) => {
        createRoot(async (dispose) => {
          const descAfter = makeDescriptor({
            cell_id: 'Kinematic.m',
            joints: [
              {
                joint_index: 0,
                kind: 'prismatic',
                dimension: 'length',
                range_lower_si: 0.0,
                range_upper_si: 0.8,
                axis: [0, 1, 0],
                driving_param_cell_id: 'Kinematic.y_pos',
                current_value_si: 0.45, // matches the optimistic override
                binding: { kind: 'param_bound', param_cell_id: 'Kinematic.y_pos', current_value_si: 0.45 },
              },
            ],
          });
          const getMechanismDescriptors = vi.fn().mockResolvedValue([descAfter]);
          const store = createMechanismStore({ getMechanismDescriptors });

          // Set optimistic override
          store.setOptimistic('Kinematic.m', 0, 0.45);
          expect(store.state.optimistic['Kinematic.m:0']).toBe(0.45);

          // After refresh, current_value_si matches override → should be cleared
          await store.refresh();
          expect(store.state.optimistic['Kinematic.m:0']).toBeUndefined();
          dispose();
          resolve();
        });
      });
    });

    it('retains overrides whose value does NOT match new descriptor current_value_si (still in flight)', async () => {
      await new Promise<void>((resolve) => {
        createRoot(async (dispose) => {
          const descAfter = makeDescriptor({
            cell_id: 'Kinematic.m',
            joints: [
              {
                joint_index: 0,
                kind: 'prismatic',
                dimension: 'length',
                range_lower_si: 0.0,
                range_upper_si: 0.8,
                axis: [0, 1, 0],
                driving_param_cell_id: 'Kinematic.y_pos',
                current_value_si: 0.1, // does NOT match the optimistic override (0.45)
                binding: { kind: 'param_bound', param_cell_id: 'Kinematic.y_pos', current_value_si: 0.1 },
              },
            ],
          });
          const getMechanismDescriptors = vi.fn().mockResolvedValue([descAfter]);
          const store = createMechanismStore({ getMechanismDescriptors });

          store.setOptimistic('Kinematic.m', 0, 0.45);
          await store.refresh();

          // Override still pending — keep it
          expect(store.state.optimistic['Kinematic.m:0']).toBe(0.45);
          dispose();
          resolve();
        });
      });
    });
  });

  describe('clearOptimistic()', () => {
    it('removes all optimistic overrides', () => {
      createRoot((dispose) => {
        const store = createMechanismStore({ getMechanismDescriptors: vi.fn().mockResolvedValue([]) });
        store.setOptimistic('Kinematic.m', 0, 0.45);
        store.setOptimistic('Kinematic.m', 1, 1.2);
        store.clearOptimistic();
        expect(store.state.optimistic).toEqual({});
        dispose();
      });
    });
  });

  // ── Step-7: synth-virtual-param round-trip ────────────────────────────────
  // These tests verify that literal-bound joints are treated as session-only
  // (scrub overrides survive routine refresh) and that a binding-aware
  // current-SI accessor is exported from the module.

  describe('literal-bound session-only persistence (§8.3)', () => {
    it('literal_bound override is RETAINED after refresh that still reports null current_value_si', async () => {
      // A literal_bound joint: current_value_si is always null in the descriptor
      // (the engine surfaces no post-scrub "current" for LiteralBound).
      // After setOptimistic(), a routine refresh whose descriptor still shows
      // the literal baseline must NOT clear the override.
      await new Promise<void>((resolve) => {
        createRoot(async (dispose) => {
          const literalJoint: JointDescriptor = {
            joint_index: 0,
            kind: 'prismatic',
            dimension: 'length',
            range_lower_si: 0.0,
            range_upper_si: 0.8,
            axis: [0, 1, 0],
            driving_param_cell_id: null,
            current_value_si: null, // literal-bound: always null
            binding: {
              kind: 'literal_bound',
              synth_param_name: '__joint_K_m_v',
              initial_value_si: 0.1,
              scrubbable: true,
            },
          };
          const desc: MechanismDescriptor = {
            cell_id: 'K.m',
            entity_path: 'K',
            name: 'm',
            bodies_count: 2,
            joints: [literalJoint],
          };
          const getMechanismDescriptors = vi.fn().mockResolvedValue([desc]);
          const store = createMechanismStore({ getMechanismDescriptors });

          // Scrub the literal-bound slider: store an optimistic override
          store.setOptimistic('K.m', 0, 0.4);
          expect(store.state.optimistic['K.m:0']).toBe(0.4);

          // Simulate routine descriptor refresh: descriptor still shows
          // current_value_si:null and initial_value_si:0.1 (the literal baseline)
          await store.refresh();

          // MUST retain the override — literal-bound overrides are session-only
          expect(store.state.optimistic['K.m:0']).toBe(0.4);
          dispose();
          resolve();
        });
      });
    });

    it('param_bound clear-on-commit still works (regression guard)', async () => {
      // Ensure the literal-bound retention logic does NOT break param_bound clearing.
      await new Promise<void>((resolve) => {
        createRoot(async (dispose) => {
          const desc = makeDescriptor({
            cell_id: 'Kinematic.m',
            joints: [
              {
                joint_index: 0,
                kind: 'prismatic',
                dimension: 'length',
                range_lower_si: 0.0,
                range_upper_si: 0.8,
                axis: [0, 1, 0],
                driving_param_cell_id: 'Kinematic.y_pos',
                current_value_si: 0.45,
                binding: { kind: 'param_bound', param_cell_id: 'Kinematic.y_pos', current_value_si: 0.45 },
              },
            ],
          });
          const getMechanismDescriptors = vi.fn().mockResolvedValue([desc]);
          const store = createMechanismStore({ getMechanismDescriptors });

          store.setOptimistic('Kinematic.m', 0, 0.45);
          await store.refresh();

          // param_bound with matching value: must be cleared
          expect(store.state.optimistic['Kinematic.m:0']).toBeUndefined();
          dispose();
          resolve();
        });
      });
    });

    it('clearOptimistic() clears a literal-bound override (file-reload reset path)', () => {
      createRoot((dispose) => {
        const store = createMechanismStore({ getMechanismDescriptors: vi.fn().mockResolvedValue([]) });
        store.setOptimistic('K.m', 0, 0.4);
        expect(store.state.optimistic['K.m:0']).toBe(0.4);
        store.clearOptimistic();
        expect(store.state.optimistic['K.m:0']).toBeUndefined();
        dispose();
      });
    });
  });

  describe('jointCurrentSi() binding-aware current-SI accessor', () => {
    // ── FAILING TEST (step-7 RED): jointCurrentSi is not yet exported ──
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const jcs = (_mechanismStoreModule as any).jointCurrentSi as
      ((joint: JointDescriptor) => number | null) | undefined;

    it('returns binding.initial_value_si for literal_bound joints', () => {
      const joint: JointDescriptor = {
        joint_index: 0,
        kind: 'prismatic',
        dimension: 'length',
        range_lower_si: 0,
        range_upper_si: 0.8,
        axis: [0, 1, 0],
        driving_param_cell_id: null,
        current_value_si: null,
        binding: { kind: 'literal_bound', synth_param_name: '__joint_v', initial_value_si: 0.25, scrubbable: true },
      };
      // Fails until step-8 exports jointCurrentSi from mechanismStore
      expect(jcs).toBeDefined();
      expect(jcs!(joint)).toBe(0.25);
    });

    it('returns binding.current_value_si for param_bound joints', () => {
      const joint: JointDescriptor = {
        joint_index: 0,
        kind: 'prismatic',
        dimension: 'length',
        range_lower_si: 0,
        range_upper_si: 0.8,
        axis: [0, 1, 0],
        driving_param_cell_id: 'K.y_pos',
        current_value_si: 0.1,
        binding: { kind: 'param_bound', param_cell_id: 'K.y_pos', current_value_si: 0.3 },
      };
      expect(jcs).toBeDefined();
      // binding.current_value_si (0.3) takes precedence over legacy current_value_si (0.1)
      expect(jcs!(joint)).toBe(0.3);
    });

    it('falls back to legacy current_value_si when binding.current_value_si is null', () => {
      const joint: JointDescriptor = {
        joint_index: 0,
        kind: 'prismatic',
        dimension: 'length',
        range_lower_si: 0,
        range_upper_si: 0.8,
        axis: [0, 1, 0],
        driving_param_cell_id: 'K.y_pos',
        current_value_si: 0.1,
        binding: { kind: 'param_bound', param_cell_id: 'K.y_pos', current_value_si: null },
      };
      expect(jcs).toBeDefined();
      // binding.current_value_si is null → falls back to legacy current_value_si
      expect(jcs!(joint)).toBe(0.1);
    });

    it('returns null for coupling_derived joints', () => {
      const joint: JointDescriptor = {
        joint_index: 0,
        kind: 'coupling',
        dimension: 'dimensionless',
        range_lower_si: null,
        range_upper_si: null,
        axis: null,
        driving_param_cell_id: null,
        current_value_si: null,
        binding: { kind: 'coupling_derived', source_joint: '' },
      };
      expect(jcs).toBeDefined();
      expect(jcs!(joint)).toBeNull();
    });

    it('returns null for fixed_no_motion joints', () => {
      const joint: JointDescriptor = {
        joint_index: 0,
        kind: 'fixed',
        dimension: 'dimensionless',
        range_lower_si: null,
        range_upper_si: null,
        axis: null,
        driving_param_cell_id: null,
        current_value_si: null,
        binding: { kind: 'fixed_no_motion' },
      };
      expect(jcs).toBeDefined();
      expect(jcs!(joint)).toBeNull();
    });
  });
});
