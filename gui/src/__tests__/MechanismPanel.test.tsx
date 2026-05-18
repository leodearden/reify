import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@solidjs/testing-library';
import type { MechanismDescriptor, JointDescriptor, JointBinding } from '../types';
import { MechanismPanel } from '../panels/MechanismPanel';
import { createMechanismStore } from '../stores/mechanismStore';

// ── Fixture helpers ──────────────────────────────────────────────────────────

function makeJoint(overrides: Partial<JointDescriptor> & { joint_index: number }): JointDescriptor {
  const kind = overrides.kind ?? 'prismatic';
  const driving_param_cell_id = overrides.driving_param_cell_id !== undefined
    ? overrides.driving_param_cell_id
    : 'Kinematic.y_pos';
  const current_value_si = overrides.current_value_si !== undefined ? overrides.current_value_si : 0.1;

  // Derive binding from kind/driving_param_cell_id if not explicitly provided.
  const binding: JointBinding = overrides.binding ?? (
    driving_param_cell_id !== null
      ? { kind: 'param_bound', param_cell_id: driving_param_cell_id, current_value_si }
      : kind === 'coupling'
        ? { kind: 'coupling_derived', source_joint: '' }
        : kind === 'fixed'
          ? { kind: 'fixed_no_motion' }
          : { kind: 'literal_bound', synth_param_name: `__joint_${overrides.joint_index}_v`, initial_value_si: current_value_si, scrubbable: true }
  );

  return {
    joint_index: overrides.joint_index,
    kind,
    dimension: overrides.dimension ?? 'length',
    range_lower_si: overrides.range_lower_si ?? 0.0,
    range_upper_si: overrides.range_upper_si ?? 0.8,
    axis: overrides.axis !== undefined ? overrides.axis : [0, 1, 0],
    driving_param_cell_id,
    current_value_si,
    binding,
  };
}

function makeDescriptor(overrides: Partial<MechanismDescriptor> & { cell_id: string }): MechanismDescriptor {
  return {
    cell_id: overrides.cell_id,
    entity_path: overrides.entity_path ?? 'Kinematic',
    name: overrides.name ?? overrides.cell_id.split('.').at(-1) ?? 'm',
    bodies_count: overrides.bodies_count ?? 2,
    joints: overrides.joints ?? [makeJoint({ joint_index: 0 })],
  };
}

afterEach(() => {
  cleanup();
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe('MechanismPanel', () => {
  describe('(a) empty state', () => {
    it('renders with data-testid="mechanism-panel"', () => {
      render(() => (
        <MechanismPanel
          descriptors={[]}
          onSetParameter={vi.fn()}
          onScrubLocal={vi.fn()}
        />
      ));
      expect(screen.getByTestId('mechanism-panel')).toBeTruthy();
    });

    it('renders empty state message when descriptors=[]', () => {
      render(() => (
        <MechanismPanel
          descriptors={[]}
          onSetParameter={vi.fn()}
          onScrubLocal={vi.fn()}
        />
      ));
      expect(screen.getByText(/no mechanisms/i)).toBeTruthy();
    });
  });

  describe('(b) one section per mechanism', () => {
    it('renders one section per descriptor with mechanism name label', () => {
      const descriptors = [
        makeDescriptor({ cell_id: 'Kinematic.m', name: 'm', bodies_count: 2 }),
        makeDescriptor({ cell_id: 'Robot.arm', name: 'arm', bodies_count: 3, joints: [makeJoint({ joint_index: 0 })] }),
      ];
      render(() => (
        <MechanismPanel descriptors={descriptors} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      expect(screen.getByText('m')).toBeTruthy();
      expect(screen.getByText('arm')).toBeTruthy();
    });

    it('shows bodies count in each mechanism section', () => {
      const descriptors = [
        makeDescriptor({ cell_id: 'Kinematic.m', bodies_count: 4 }),
      ];
      render(() => (
        <MechanismPanel descriptors={descriptors} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      // Should show bodies count somehow (e.g. "4 bodies" or "bodies: 4")
      expect(screen.getByText(/4/)).toBeTruthy();
    });
  });

  describe('(c) one labelled slider per joint', () => {
    it('renders a labelled slider row per joint with kind and dimension', () => {
      const desc = makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({ joint_index: 0, kind: 'prismatic', dimension: 'length' }),
          makeJoint({ joint_index: 1, kind: 'revolute', dimension: 'angle', range_lower_si: 0, range_upper_si: Math.PI, driving_param_cell_id: 'Kinematic.theta' }),
        ],
      });
      render(() => (
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      // Check that kind labels appear
      expect(screen.getByText(/prismatic/i)).toBeTruthy();
      expect(screen.getByText(/revolute/i)).toBeTruthy();
    });

    it('renders one range input per joint with driving_param_cell_id', () => {
      const desc = makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({ joint_index: 0, kind: 'prismatic', driving_param_cell_id: 'Kinematic.y' }),
          makeJoint({ joint_index: 1, kind: 'revolute', driving_param_cell_id: 'Kinematic.theta' }),
        ],
      });
      render(() => (
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      const sliders = screen.getAllByRole('slider');
      expect(sliders).toHaveLength(2);
    });
  });

  describe('(d) slider range in display units', () => {
    it('prismatic slider min=range_lower_si*1000 (mm), max=range_upper_si*1000', () => {
      const desc = makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({
            joint_index: 0,
            kind: 'prismatic',
            dimension: 'length',
            range_lower_si: 0.0,
            range_upper_si: 0.8,
            driving_param_cell_id: 'Kinematic.y',
          }),
        ],
      });
      render(() => (
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      const slider = screen.getByRole('slider') as HTMLInputElement;
      expect(Number(slider.min)).toBeCloseTo(0);
      expect(Number(slider.max)).toBeCloseTo(800);
    });

    it('revolute slider min/max converted to degrees', () => {
      const deg = (r: number) => r * (180 / Math.PI);
      const lower = 0;
      const upper = Math.PI / 2; // 90 deg
      const desc = makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({
            joint_index: 0,
            kind: 'revolute',
            dimension: 'angle',
            range_lower_si: lower,
            range_upper_si: upper,
            driving_param_cell_id: 'Kinematic.theta',
          }),
        ],
      });
      render(() => (
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      const slider = screen.getByRole('slider') as HTMLInputElement;
      expect(Number(slider.min)).toBeCloseTo(deg(lower), 1);
      expect(Number(slider.max)).toBeCloseTo(deg(upper), 1);
    });
  });

  describe('(e) slider onChange fires onSetParameter', () => {
    it('prismatic slider fires onSetParameter with "Xmm" formatted value', () => {
      // Synchronously flush RAF so the callback runs immediately
      const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        cb(performance.now());
        return 1;
      });
      try {
        const onSetParameter = vi.fn();
        const desc = makeDescriptor({
          cell_id: 'Kinematic.m',
          joints: [
            makeJoint({
              joint_index: 0,
              kind: 'prismatic',
              driving_param_cell_id: 'Kinematic.y_pos',
              range_lower_si: 0,
              range_upper_si: 0.8,
            }),
          ],
        });
        render(() => (
          <MechanismPanel descriptors={[desc]} onSetParameter={onSetParameter} onScrubLocal={vi.fn()} />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.input(slider, { target: { value: '400' } });
        // onSetParameter should be called with (driving_param_cell_id, '<value>mm')
        expect(onSetParameter).toHaveBeenCalledWith('Kinematic.y_pos', expect.stringMatching(/mm$/));
      } finally {
        rafSpy.mockRestore();
      }
    });

    it('revolute slider fires onSetParameter with "Xdeg" formatted value', () => {
      const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        cb(performance.now());
        return 1;
      });
      try {
        const onSetParameter = vi.fn();
        const desc = makeDescriptor({
          cell_id: 'Kinematic.m',
          joints: [
            makeJoint({
              joint_index: 0,
              kind: 'revolute',
              dimension: 'angle',
              driving_param_cell_id: 'Kinematic.theta',
              range_lower_si: 0,
              range_upper_si: Math.PI,
            }),
          ],
        });
        render(() => (
          <MechanismPanel descriptors={[desc]} onSetParameter={onSetParameter} onScrubLocal={vi.fn()} />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.input(slider, { target: { value: '90' } });
        expect(onSetParameter).toHaveBeenCalledWith('Kinematic.theta', expect.stringMatching(/deg$/));
      } finally {
        rafSpy.mockRestore();
      }
    });
  });

  describe('(f) literal-bound joints render functional sliders; coupling/fixed do not', () => {
    it('literal_bound prismatic joint renders exactly one functional slider', () => {
      const desc = makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({
            joint_index: 0,
            kind: 'prismatic',
            driving_param_cell_id: null,
            current_value_si: null,
            binding: {
              kind: 'literal_bound',
              synth_param_name: '__joint_x_axis_v',
              initial_value_si: 0.1,
              scrubbable: true,
            },
          }),
        ],
      });
      render(() => (
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      const sliders = screen.getAllByRole('slider');
      expect(sliders).toHaveLength(1);
    });

    it('literal_bound revolute joint renders exactly one functional slider', () => {
      const desc = makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({
            joint_index: 0,
            kind: 'revolute',
            dimension: 'angle',
            range_lower_si: 0,
            range_upper_si: Math.PI,
            driving_param_cell_id: null,
            current_value_si: null,
            binding: {
              kind: 'literal_bound',
              synth_param_name: '__joint_theta_v',
              initial_value_si: 0.5,
              scrubbable: true,
            },
          }),
        ],
      });
      render(() => (
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      const sliders = screen.getAllByRole('slider');
      expect(sliders).toHaveLength(1);
    });

    it('coupling_derived joint still renders no slider (regression guard)', () => {
      const desc = makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({
            joint_index: 0,
            kind: 'coupling',
            dimension: 'dimensionless',
            axis: null,
            range_lower_si: null,
            range_upper_si: null,
            driving_param_cell_id: null,
            current_value_si: null,
            binding: { kind: 'coupling_derived', source_joint: '' },
          }),
        ],
      });
      render(() => (
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      expect(screen.queryAllByRole('slider')).toHaveLength(0);
    });

    it('fixed_no_motion joint still renders no slider (regression guard)', () => {
      const desc = makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({
            joint_index: 0,
            kind: 'fixed',
            dimension: 'dimensionless',
            axis: null,
            range_lower_si: null,
            range_upper_si: null,
            driving_param_cell_id: null,
            current_value_si: null,
            binding: { kind: 'fixed_no_motion' },
          }),
        ],
      });
      render(() => (
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      expect(screen.queryAllByRole('slider')).toHaveLength(0);
    });
  });

  describe('(g) coupling joint', () => {
    it('coupling joint shows "coupling" kind label and no slider', () => {
      const desc = makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({
            joint_index: 0,
            kind: 'coupling',
            dimension: 'dimensionless',
            axis: null,
            range_lower_si: null,
            range_upper_si: null,
            driving_param_cell_id: null,
            current_value_si: null,
          }),
        ],
      });
      render(() => (
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      // Check the kind label specifically (exact text "coupling")
      const kindLabels = screen.getAllByText('coupling');
      expect(kindLabels.length).toBeGreaterThanOrEqual(1);
      const sliders = screen.queryAllByRole('slider');
      expect(sliders).toHaveLength(0);
    });

    it('fixed joint shows "fixed" kind label and no slider', () => {
      const desc = makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({
            joint_index: 0,
            kind: 'fixed',
            dimension: 'dimensionless',
            axis: null,
            range_lower_si: null,
            range_upper_si: null,
            driving_param_cell_id: null,
            current_value_si: null,
          }),
        ],
      });
      render(() => (
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      // Check the kind label specifically (exact text "fixed")
      const kindLabels = screen.getAllByText('fixed');
      expect(kindLabels.length).toBeGreaterThanOrEqual(1);
      const sliders = screen.queryAllByRole('slider');
      expect(sliders).toHaveLength(0);
    });
  });

  describe('(h) RAF-coalesced scrub', () => {
    it('rapid input events dispatch at most one onSetParameter per RAF tick', () => {
      // Capture RAF callbacks
      const rafCallbacks: FrameRequestCallback[] = [];
      const originalRaf = globalThis.requestAnimationFrame;
      globalThis.requestAnimationFrame = (cb: FrameRequestCallback) => {
        rafCallbacks.push(cb);
        return rafCallbacks.length;
      };

      try {
        const onSetParameter = vi.fn();
        const desc = makeDescriptor({
          cell_id: 'Kinematic.m',
          joints: [
            makeJoint({
              joint_index: 0,
              kind: 'prismatic',
              driving_param_cell_id: 'Kinematic.y_pos',
            }),
          ],
        });
        render(() => (
          <MechanismPanel descriptors={[desc]} onSetParameter={onSetParameter} onScrubLocal={vi.fn()} />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;

        // Fire 5 rapid input events before any RAF fires
        fireEvent.input(slider, { target: { value: '100' } });
        fireEvent.input(slider, { target: { value: '200' } });
        fireEvent.input(slider, { target: { value: '300' } });
        fireEvent.input(slider, { target: { value: '400' } });
        fireEvent.input(slider, { target: { value: '500' } });

        // Before RAF fires: onSetParameter should not have been called yet
        // (or if it's called synchronously, at most once with the last value)
        const callsBefore = onSetParameter.mock.calls.length;

        // Flush one RAF tick
        const cb = rafCallbacks[0];
        if (cb) cb(performance.now());

        // After RAF: should have been called at most once more (with the last pending value)
        const callsAfter = onSetParameter.mock.calls.length;
        expect(callsAfter - callsBefore).toBeLessThanOrEqual(1);

        // If there was a call, it should use the last value ("500mm")
        if (callsAfter > callsBefore) {
          const lastCall = onSetParameter.mock.calls[callsAfter - 1];
          expect(lastCall[0]).toBe('Kinematic.y_pos');
          expect(lastCall[1]).toMatch(/500/);
        }
      } finally {
        globalThis.requestAnimationFrame = originalRaf;
      }
    });
  });

  describe('(i) onScrubLocal receives SI values', () => {
    it('(i.1) prismatic slider input of "400" mm invokes onScrubLocal with ~0.4 SI (m)', () => {
      const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        cb(performance.now());
        return 1;
      });
      try {
        const onScrubLocal = vi.fn();
        const desc = makeDescriptor({
          cell_id: 'Kinematic.m',
          joints: [
            makeJoint({
              joint_index: 0,
              kind: 'prismatic',
              dimension: 'length',
              driving_param_cell_id: 'Kinematic.y_pos',
              range_lower_si: 0,
              range_upper_si: 1.0,
            }),
          ],
        });
        render(() => (
          <MechanismPanel
            descriptors={[desc]}
            onSetParameter={vi.fn()}
            onScrubLocal={onScrubLocal}
          />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.input(slider, { target: { value: '400' } });

        expect(onScrubLocal).toHaveBeenCalled();
        // Third arg is valueSi: 400 mm → 0.4 m
        const thirdArg: number = onScrubLocal.mock.calls[0][2];
        expect(thirdArg).toBeCloseTo(0.4, 6);
      } finally {
        rafSpy.mockRestore();
      }
    });

    it('(i.2) revolute slider input of "90" deg invokes onScrubLocal with ~π/2 SI (rad)', () => {
      const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        cb(performance.now());
        return 1;
      });
      try {
        const onScrubLocal = vi.fn();
        const desc = makeDescriptor({
          cell_id: 'Kinematic.m',
          joints: [
            makeJoint({
              joint_index: 0,
              kind: 'revolute',
              dimension: 'angle',
              driving_param_cell_id: 'Kinematic.theta',
              range_lower_si: 0,
              range_upper_si: Math.PI,
            }),
          ],
        });
        render(() => (
          <MechanismPanel
            descriptors={[desc]}
            onSetParameter={vi.fn()}
            onScrubLocal={onScrubLocal}
          />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.input(slider, { target: { value: '90' } });

        expect(onScrubLocal).toHaveBeenCalled();
        // Third arg is valueSi: 90 deg → π/2 rad
        const thirdArg: number = onScrubLocal.mock.calls[0][2];
        expect(thirdArg).toBeCloseTo(Math.PI / 2, 6);
      } finally {
        rafSpy.mockRestore();
      }
    });

    it('(i.3) optimistic override is cleared after refresh confirms matching SI value', async () => {
      const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        cb(performance.now());
        return 1;
      });
      try {
        // Build a real store with a mock bridge
        let resolvedDescriptors: MechanismDescriptor[] = [];
        const mockGetDescriptors = vi.fn().mockImplementation(async () => resolvedDescriptors);
        const store = createMechanismStore({ getMechanismDescriptors: mockGetDescriptors });

        const desc = makeDescriptor({
          cell_id: 'Kinematic.m',
          joints: [
            makeJoint({
              joint_index: 0,
              kind: 'prismatic',
              dimension: 'length',
              driving_param_cell_id: 'Kinematic.y_pos',
              range_lower_si: 0,
              range_upper_si: 1.0,
              current_value_si: 0.1,
            }),
          ],
        });

        render(() => (
          <MechanismPanel
            descriptors={[desc]}
            onSetParameter={vi.fn()}
            onScrubLocal={(cellId, jointIndex, valueSi) => {
              if (cellId !== null) {
                store.setOptimistic(cellId, jointIndex, valueSi);
              }
            }}
          />
        ));

        const slider = screen.getByRole('slider') as HTMLInputElement;
        // Fire slider input: 400 mm → should store optimistic 0.4 SI (not 400)
        fireEvent.input(slider, { target: { value: '400' } });

        const key = 'Kinematic.m:0';
        // After the fix, optimistic should contain 0.4 (SI), not 400 (display)
        expect(store.state.optimistic[key]).toBeCloseTo(0.4, 6);

        // Simulate backend confirming the new value at 0.4 SI
        resolvedDescriptors = [{
          ...desc,
          joints: [{ ...desc.joints[0], current_value_si: 0.4 }],
        }];

        // After refresh, the equality check fires (0.4 === 0.4) and key is deleted
        await store.refresh();
        expect(store.state.optimistic[key]).toBeUndefined();
      } finally {
        rafSpy.mockRestore();
      }
    });
  });

  describe('(k) binding-aware initial value + visual distinction', () => {
    it('literal_bound joint with current_value_si:null uses binding.initial_value_si for slider init', () => {
      const desc = makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({
            joint_index: 0,
            kind: 'prismatic',
            driving_param_cell_id: null,
            current_value_si: null,
            range_lower_si: 0,
            range_upper_si: 1.0,
            binding: {
              kind: 'literal_bound',
              synth_param_name: '__joint_x_axis_v',
              initial_value_si: 0.25,
              scrubbable: true,
            },
          }),
        ],
      });
      render(() => (
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      const slider = screen.getByRole('slider') as HTMLInputElement;
      // 0.25 m → 250 mm display
      expect(Number(slider.value)).toBeCloseTo(250, 0);
    });

    it('literal_bound joint row carries data-binding="literal"', () => {
      const desc = makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({
            joint_index: 0,
            kind: 'prismatic',
            driving_param_cell_id: null,
            current_value_si: null,
            binding: {
              kind: 'literal_bound',
              synth_param_name: '__joint_x_axis_v',
              initial_value_si: 0.1,
              scrubbable: true,
            },
          }),
        ],
      });
      render(() => (
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      const row = screen.getByTestId('joint-row-0');
      expect(row.getAttribute('data-binding')).toBe('literal');
      // Slider should still be present and functional
      expect(screen.getAllByRole('slider')).toHaveLength(1);
    });

    it('param_bound joint row carries data-binding="param"', () => {
      const desc = makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({
            joint_index: 0,
            kind: 'prismatic',
            driving_param_cell_id: 'Kinematic.y_pos',
            current_value_si: 0.1,
            binding: {
              kind: 'param_bound',
              param_cell_id: 'Kinematic.y_pos',
              current_value_si: 0.1,
            },
          }),
        ],
      });
      render(() => (
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      const row = screen.getByTestId('joint-row-0');
      expect(row.getAttribute('data-binding')).toBe('param');
      // Slider should be present for param_bound
      expect(screen.getAllByRole('slider')).toHaveLength(1);
    });
  });

  describe('(j) literal-bound slider fires onSetParameter with synth param name', () => {
    it('literal_bound prismatic slider fires onSetParameter with synth_param_name and "Xmm" value', () => {
      const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        cb(performance.now());
        return 1;
      });
      try {
        const onSetParameter = vi.fn();
        const desc = makeDescriptor({
          cell_id: 'Kinematic.m',
          joints: [
            makeJoint({
              joint_index: 0,
              kind: 'prismatic',
              driving_param_cell_id: null,
              current_value_si: null,
              range_lower_si: 0,
              range_upper_si: 0.8,
              binding: {
                kind: 'literal_bound',
                synth_param_name: '__joint_x_axis_v',
                initial_value_si: 0.1,
                scrubbable: true,
              },
            }),
          ],
        });
        render(() => (
          <MechanismPanel descriptors={[desc]} onSetParameter={onSetParameter} onScrubLocal={vi.fn()} />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.input(slider, { target: { value: '400' } });

        // Must fire with synth param name, not null
        expect(onSetParameter).toHaveBeenCalledWith('__joint_x_axis_v', expect.stringMatching(/mm$/));
      } finally {
        rafSpy.mockRestore();
      }
    });

    it('literal_bound revolute slider fires onSetParameter with synth_param_name and "Xdeg" value', () => {
      const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        cb(performance.now());
        return 1;
      });
      try {
        const onSetParameter = vi.fn();
        const desc = makeDescriptor({
          cell_id: 'Kinematic.m',
          joints: [
            makeJoint({
              joint_index: 0,
              kind: 'revolute',
              dimension: 'angle',
              driving_param_cell_id: null,
              current_value_si: null,
              range_lower_si: 0,
              range_upper_si: Math.PI,
              binding: {
                kind: 'literal_bound',
                synth_param_name: '__joint_theta_v',
                initial_value_si: 0.5,
                scrubbable: true,
              },
            }),
          ],
        });
        render(() => (
          <MechanismPanel descriptors={[desc]} onSetParameter={onSetParameter} onScrubLocal={vi.fn()} />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.input(slider, { target: { value: '90' } });

        expect(onSetParameter).toHaveBeenCalledWith('__joint_theta_v', expect.stringMatching(/deg$/));
      } finally {
        rafSpy.mockRestore();
      }
    });

    it('param_bound joint still fires onSetParameter with param_cell_id (regression)', () => {
      const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        cb(performance.now());
        return 1;
      });
      try {
        const onSetParameter = vi.fn();
        const desc = makeDescriptor({
          cell_id: 'Kinematic.m',
          joints: [
            makeJoint({
              joint_index: 0,
              kind: 'prismatic',
              driving_param_cell_id: 'Kinematic.y_pos',
              range_lower_si: 0,
              range_upper_si: 0.8,
              binding: {
                kind: 'param_bound',
                param_cell_id: 'Kinematic.y_pos',
                current_value_si: 0.1,
              },
            }),
          ],
        });
        render(() => (
          <MechanismPanel descriptors={[desc]} onSetParameter={onSetParameter} onScrubLocal={vi.fn()} />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.input(slider, { target: { value: '400' } });

        expect(onSetParameter).toHaveBeenCalledWith('Kinematic.y_pos', expect.stringMatching(/mm$/));
      } finally {
        rafSpy.mockRestore();
      }
    });

    it('literal_bound prismatic onScrubLocal receives SI value (~0.4 m) not display value', () => {
      const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        cb(performance.now());
        return 1;
      });
      try {
        const onScrubLocal = vi.fn();
        const desc = makeDescriptor({
          cell_id: 'Kinematic.m',
          joints: [
            makeJoint({
              joint_index: 0,
              kind: 'prismatic',
              driving_param_cell_id: null,
              current_value_si: null,
              range_lower_si: 0,
              range_upper_si: 0.8,
              binding: {
                kind: 'literal_bound',
                synth_param_name: '__joint_x_axis_v',
                initial_value_si: 0.1,
                scrubbable: true,
              },
            }),
          ],
        });
        render(() => (
          <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={onScrubLocal} />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.input(slider, { target: { value: '400' } });

        expect(onScrubLocal).toHaveBeenCalled();
        const thirdArg: number = onScrubLocal.mock.calls[0][2];
        expect(thirdArg).toBeCloseTo(0.4, 6);
      } finally {
        rafSpy.mockRestore();
      }
    });
  });
});
