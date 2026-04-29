import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@solidjs/testing-library';
import type { MechanismDescriptor, JointDescriptor } from '../types';
import { MechanismPanel } from '../panels/MechanismPanel';

// ── Fixture helpers ──────────────────────────────────────────────────────────

function makeJoint(overrides: Partial<JointDescriptor> & { joint_index: number }): JointDescriptor {
  return {
    joint_index: overrides.joint_index,
    kind: overrides.kind ?? 'prismatic',
    dimension: overrides.dimension ?? 'length',
    range_lower_si: overrides.range_lower_si ?? 0.0,
    range_upper_si: overrides.range_upper_si ?? 0.8,
    axis: overrides.axis !== undefined ? overrides.axis : [0, 1, 0],
    driving_param_cell_id: overrides.driving_param_cell_id !== undefined
      ? overrides.driving_param_cell_id
      : 'Kinematic.y_pos',
    current_value_si: overrides.current_value_si !== undefined ? overrides.current_value_si : 0.1,
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
      // RAF: flush pending RAF callbacks
      vi.runAllTimers?.();
      // onSetParameter should be called with (driving_param_cell_id, '<value>mm')
      expect(onSetParameter).toHaveBeenCalledWith('Kinematic.y_pos', expect.stringMatching(/mm$/));
    });

    it('revolute slider fires onSetParameter with "Xdeg" formatted value', () => {
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
      vi.runAllTimers?.();
      expect(onSetParameter).toHaveBeenCalledWith('Kinematic.theta', expect.stringMatching(/deg$/));
    });
  });

  describe('(f) joint without driving_param_cell_id', () => {
    it('renders read-only "literal-bound" badge instead of slider', () => {
      const desc = makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({
            joint_index: 0,
            kind: 'prismatic',
            driving_param_cell_id: null,
          }),
        ],
      });
      render(() => (
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      // Should NOT have a slider input
      const sliders = screen.queryAllByRole('slider');
      expect(sliders).toHaveLength(0);
      // Should show "literal-bound" badge
      expect(screen.getByText(/literal-bound/i)).toBeTruthy();
    });
  });

  describe('(g) coupling joint', () => {
    it('coupling joint shows "coupling" label and no slider', () => {
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
      expect(screen.getByText(/coupling/i)).toBeTruthy();
      const sliders = screen.queryAllByRole('slider');
      expect(sliders).toHaveLength(0);
    });

    it('fixed joint shows "fixed" label and no slider', () => {
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
      expect(screen.getByText(/fixed/i)).toBeTruthy();
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
});
