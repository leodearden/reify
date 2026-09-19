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

/**
 * Replace `requestAnimationFrame`/`cancelAnimationFrame` with a manually driven
 * queue, so a test can observe a frame while it is still PENDING — the state a
 * drag's preview is in when the gesture ends and the commit races it.
 */
function installManualRaf() {
  const originalRequest = globalThis.requestAnimationFrame;
  const originalCancel = globalThis.cancelAnimationFrame;
  const pending = new Map<number, FrameRequestCallback>();
  let nextId = 0;

  globalThis.requestAnimationFrame = (cb: FrameRequestCallback): number => {
    nextId += 1;
    pending.set(nextId, cb);
    return nextId;
  };
  globalThis.cancelAnimationFrame = (id: number): void => {
    pending.delete(id);
  };

  return {
    pendingFrames: (): number => pending.size,
    flush(): void {
      const due = [...pending.values()];
      pending.clear();
      for (const cb of due) cb(performance.now());
    },
    restore(): void {
      globalThis.requestAnimationFrame = originalRequest;
      globalThis.cancelAnimationFrame = originalCancel;
    },
  };
}

/** Yield past a macrotask boundary, draining every settled promise continuation. */
function flushPendingPromises(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
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
          onSetParameter={vi.fn()} onPreviewParameter={vi.fn()}
          onScrubLocal={vi.fn()}
        />
      ));
      expect(screen.getByTestId('mechanism-panel')).toBeTruthy();
    });

    it('renders empty state message when descriptors=[]', () => {
      render(() => (
        <MechanismPanel
          descriptors={[]}
          onSetParameter={vi.fn()} onPreviewParameter={vi.fn()}
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
        <MechanismPanel descriptors={descriptors} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      expect(screen.getByText('m')).toBeTruthy();
      expect(screen.getByText('arm')).toBeTruthy();
    });

    it('shows bodies count in each mechanism section', () => {
      const descriptors = [
        makeDescriptor({ cell_id: 'Kinematic.m', bodies_count: 4 }),
      ];
      render(() => (
        <MechanismPanel descriptors={descriptors} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
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
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
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
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
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
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
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
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      const slider = screen.getByRole('slider') as HTMLInputElement;
      expect(Number(slider.min)).toBeCloseTo(deg(lower), 1);
      expect(Number(slider.max)).toBeCloseTo(deg(upper), 1);
    });
  });

  describe('(e) slider input previews; slider change commits', () => {
    it('prismatic slider input previews "Xmm" and commits nothing', () => {
      // Synchronously flush RAF so the callback runs immediately
      const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        cb(performance.now());
        return 1;
      });
      try {
        const onSetParameter = vi.fn();
        const onPreviewParameter = vi.fn();
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
          <MechanismPanel
            descriptors={[desc]}
            onSetParameter={onSetParameter}
            onPreviewParameter={onPreviewParameter}
            onScrubLocal={vi.fn()}
          />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.input(slider, { target: { value: '400' } });

        expect(onPreviewParameter).toHaveBeenCalledWith(
          'Kinematic.y_pos',
          expect.stringMatching(/mm$/),
        );
        expect(onSetParameter).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
      }
    });

    it('revolute slider input previews "Xdeg" and commits nothing', () => {
      const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        cb(performance.now());
        return 1;
      });
      try {
        const onSetParameter = vi.fn();
        const onPreviewParameter = vi.fn();
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
            onSetParameter={onSetParameter}
            onPreviewParameter={onPreviewParameter}
            onScrubLocal={vi.fn()}
          />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.input(slider, { target: { value: '90' } });

        expect(onPreviewParameter).toHaveBeenCalledWith(
          'Kinematic.theta',
          expect.stringMatching(/deg$/),
        );
        expect(onSetParameter).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
      }
    });

    it('a burst of drag frames commits nothing — the whole drag stays transient', () => {
      const raf = installManualRaf();
      try {
        const onSetParameter = vi.fn();
        const onPreviewParameter = vi.fn();
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
          <MechanismPanel
            descriptors={[desc]}
            onSetParameter={onSetParameter}
            onPreviewParameter={onPreviewParameter}
            onScrubLocal={vi.fn()}
          />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;

        for (const value of ['100', '200', '300', '400', '500']) {
          fireEvent.input(slider, { target: { value } });
          raf.flush();
        }

        expect(onPreviewParameter).toHaveBeenCalledTimes(5);
        expect(onSetParameter).not.toHaveBeenCalled();
      } finally {
        raf.restore();
      }
    });

    it('prismatic slider change commits the final value once, formatted like a preview', async () => {
      const raf = installManualRaf();
      try {
        const onSetParameter = vi.fn();
        const onPreviewParameter = vi.fn();
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
          <MechanismPanel
            descriptors={[desc]}
            onSetParameter={onSetParameter}
            onPreviewParameter={onPreviewParameter}
            onScrubLocal={vi.fn()}
          />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;

        fireEvent.input(slider, { target: { value: '400' } });
        raf.flush();
        fireEvent.change(slider, { target: { value: '400' } });
        await flushPendingPromises();

        expect(onSetParameter).toHaveBeenCalledTimes(1);
        expect(onSetParameter).toHaveBeenCalledWith('Kinematic.y_pos', '400mm');
        expect(onSetParameter.mock.calls[0][1]).toBe(onPreviewParameter.mock.calls[0][1]);
      } finally {
        raf.restore();
      }
    });

    it('a burst of change events commits the first and then one trailing write', async () => {
      // `change` fires once per pointer release AND once per arrow key, and
      // auto-repeat delivers roughly 30 of those a second. Each one is a full
      // recompile plus an atomic `.ri` rewrite, so keyboard scrubbing was the
      // one gesture still routing the write cadence the preview/commit split
      // exists to prevent straight through.
      vi.useFakeTimers();
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
          <MechanismPanel
            descriptors={[desc]}
            onSetParameter={onSetParameter}
            onPreviewParameter={vi.fn()}
            onScrubLocal={vi.fn()}
          />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;

        for (const value of ['100', '110', '120']) {
          fireEvent.change(slider, { target: { value } });
        }
        await vi.advanceTimersByTimeAsync(0);

        // Leading edge: the first release-or-keypress is durable immediately,
        // so no gesture can end with its write merely scheduled.
        expect(onSetParameter).toHaveBeenCalledTimes(1);
        expect(onSetParameter).toHaveBeenCalledWith('Kinematic.y_pos', '100mm');

        // Trailing edge: the rest of the burst collapses to its LAST value,
        // the only one the slider still sits on.
        await vi.advanceTimersByTimeAsync(1000);
        expect(onSetParameter).toHaveBeenCalledTimes(2);
        expect(onSetParameter).toHaveBeenLastCalledWith('Kinematic.y_pos', '120mm');
      } finally {
        vi.useRealTimers();
      }
    });

    it('a coalesced write still lands when the row unmounts before its window closes', async () => {
      // The asymmetry with a pending PREVIEW, which unmount cancels: a preview
      // is transient by definition, while a durable write the user has already
      // made is theirs whether or not this row survives to see it land.
      vi.useFakeTimers();
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
        const { unmount } = render(() => (
          <MechanismPanel
            descriptors={[desc]}
            onSetParameter={onSetParameter}
            onPreviewParameter={vi.fn()}
            onScrubLocal={vi.fn()}
          />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;

        fireEvent.change(slider, { target: { value: '100' } });
        fireEvent.change(slider, { target: { value: '120' } });
        await vi.advanceTimersByTimeAsync(0);
        expect(onSetParameter).toHaveBeenCalledTimes(1);

        unmount();
        await vi.advanceTimersByTimeAsync(0);

        expect(onSetParameter).toHaveBeenCalledTimes(2);
        expect(onSetParameter).toHaveBeenLastCalledWith('Kinematic.y_pos', '120mm');
      } finally {
        vi.useRealTimers();
      }
    });

    it('revolute slider change commits "Xdeg"', async () => {
      const raf = installManualRaf();
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
          <MechanismPanel
            descriptors={[desc]}
            onSetParameter={onSetParameter}
            onPreviewParameter={vi.fn()}
            onScrubLocal={vi.fn()}
          />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;

        fireEvent.change(slider, { target: { value: '90' } });
        await flushPendingPromises();

        expect(onSetParameter).toHaveBeenCalledTimes(1);
        expect(onSetParameter).toHaveBeenCalledWith('Kinematic.theta', '90deg');
      } finally {
        raf.restore();
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
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
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
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
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
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
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
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
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
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
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
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      // Check the kind label specifically (exact text "fixed")
      const kindLabels = screen.getAllByText('fixed');
      expect(kindLabels.length).toBeGreaterThanOrEqual(1);
      const sliders = screen.queryAllByRole('slider');
      expect(sliders).toHaveLength(0);
    });
  });

  describe('(h) RAF-coalesced preview, and the commit that ends the gesture', () => {
    /** A prismatic joint driven by `Kinematic.y_pos` — the canonical drag subject. */
    const draggableDescriptor = () =>
      makeDescriptor({
        cell_id: 'Kinematic.m',
        joints: [
          makeJoint({
            joint_index: 0,
            kind: 'prismatic',
            driving_param_cell_id: 'Kinematic.y_pos',
          }),
        ],
      });

    it('rapid input events dispatch at most one onPreviewParameter per RAF tick, with the last value', () => {
      const raf = installManualRaf();
      try {
        const onPreviewParameter = vi.fn();
        render(() => (
          <MechanismPanel
            descriptors={[draggableDescriptor()]}
            onSetParameter={vi.fn()}
            onPreviewParameter={onPreviewParameter}
            onScrubLocal={vi.fn()}
          />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;

        for (const value of ['100', '200', '300', '400', '500']) {
          fireEvent.input(slider, { target: { value } });
        }
        expect(onPreviewParameter).not.toHaveBeenCalled();
        expect(raf.pendingFrames()).toBe(1);

        raf.flush();

        expect(onPreviewParameter).toHaveBeenCalledTimes(1);
        expect(onPreviewParameter).toHaveBeenCalledWith('Kinematic.y_pos', '500mm');
      } finally {
        raf.restore();
      }
    });

    it('a commit cancels the preview frame still pending, and lands last', async () => {
      const raf = installManualRaf();
      try {
        const order: string[] = [];
        const onPreviewParameter = vi.fn(() => {
          order.push('preview');
        });
        const onSetParameter = vi.fn(() => {
          order.push('commit');
        });
        render(() => (
          <MechanismPanel
            descriptors={[draggableDescriptor()]}
            onSetParameter={onSetParameter}
            onPreviewParameter={onPreviewParameter}
            onScrubLocal={vi.fn()}
          />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;

        fireEvent.input(slider, { target: { value: '200' } });
        raf.flush();
        await flushPendingPromises();
        const previewsMidDrag = onPreviewParameter.mock.calls.length;
        expect(previewsMidDrag).toBe(1);

        fireEvent.input(slider, { target: { value: '300' } });
        expect(raf.pendingFrames()).toBe(1);

        fireEvent.change(slider, { target: { value: '300' } });
        expect(raf.pendingFrames()).toBe(0);

        raf.flush();
        await flushPendingPromises();

        expect(onPreviewParameter).toHaveBeenCalledTimes(previewsMidDrag);
        expect(onSetParameter).toHaveBeenCalledTimes(1);
        expect(onSetParameter).toHaveBeenCalledWith('Kinematic.y_pos', '300mm');
        expect(order.at(-1)).toBe('commit');
      } finally {
        raf.restore();
      }
    });

    it('a commit waits for the in-flight preview it follows', async () => {
      const raf = installManualRaf();
      try {
        let landPreview!: () => void;
        const previewInFlight = new Promise<void>((resolve) => {
          landPreview = resolve;
        });
        const onPreviewParameter = vi.fn(() => previewInFlight);
        const onSetParameter = vi.fn();
        render(() => (
          <MechanismPanel
            descriptors={[draggableDescriptor()]}
            onSetParameter={onSetParameter}
            onPreviewParameter={onPreviewParameter}
            onScrubLocal={vi.fn()}
          />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;

        fireEvent.input(slider, { target: { value: '250' } });
        raf.flush();
        expect(onPreviewParameter).toHaveBeenCalledTimes(1);

        fireEvent.change(slider, { target: { value: '250' } });
        await flushPendingPromises();
        expect(onSetParameter).not.toHaveBeenCalled();

        landPreview();
        await flushPendingPromises();

        expect(onSetParameter).toHaveBeenCalledWith('Kinematic.y_pos', '250mm');
      } finally {
        raf.restore();
      }
    });

    it('unmounting mid-drag strands no preview frame', () => {
      const raf = installManualRaf();
      try {
        const onPreviewParameter = vi.fn();
        const { unmount } = render(() => (
          <MechanismPanel
            descriptors={[draggableDescriptor()]}
            onSetParameter={vi.fn()}
            onPreviewParameter={onPreviewParameter}
            onScrubLocal={vi.fn()}
          />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.input(slider, { target: { value: '400' } });
        expect(raf.pendingFrames()).toBe(1);

        unmount();
        expect(raf.pendingFrames()).toBe(0);

        raf.flush();
        expect(onPreviewParameter).not.toHaveBeenCalled();
      } finally {
        raf.restore();
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
            onSetParameter={vi.fn()} onPreviewParameter={vi.fn()}
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
            onSetParameter={vi.fn()} onPreviewParameter={vi.fn()}
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
            onSetParameter={vi.fn()} onPreviewParameter={vi.fn()}
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

        // Simulate backend confirming the new value at 0.4 SI.
        // In a real backend response, binding.current_value_si is also updated
        // (the engine mirrors current_value_si to binding for param_bound joints).
        resolvedDescriptors = [{
          ...desc,
          joints: [{
            ...desc.joints[0],
            current_value_si: 0.4,
            binding: { kind: 'param_bound', param_cell_id: 'Kinematic.y_pos', current_value_si: 0.4 },
          }],
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
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
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
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
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
        <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={vi.fn()} />
      ));
      const row = screen.getByTestId('joint-row-0');
      expect(row.getAttribute('data-binding')).toBe('param');
      // Slider should be present for param_bound
      expect(screen.getAllByRole('slider')).toHaveLength(1);
    });
  });

  describe('(j) literal-bound slider previews and commits under its synth param name', () => {
    it('literal_bound prismatic slider previews under synth_param_name with an "Xmm" value', () => {
      const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        cb(performance.now());
        return 1;
      });
      try {
        const onPreviewParameter = vi.fn();
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
          <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={onPreviewParameter} onScrubLocal={vi.fn()} />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.input(slider, { target: { value: '400' } });

        // Must preview under the synth param name, not null
        expect(onPreviewParameter).toHaveBeenCalledWith('__joint_x_axis_v', expect.stringMatching(/mm$/));
      } finally {
        rafSpy.mockRestore();
      }
    });

    it('literal_bound revolute slider previews under synth_param_name with an "Xdeg" value', () => {
      const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        cb(performance.now());
        return 1;
      });
      try {
        const onPreviewParameter = vi.fn();
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
          <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={onPreviewParameter} onScrubLocal={vi.fn()} />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.input(slider, { target: { value: '90' } });

        expect(onPreviewParameter).toHaveBeenCalledWith('__joint_theta_v', expect.stringMatching(/deg$/));
      } finally {
        rafSpy.mockRestore();
      }
    });

    it('param_bound joint still previews under param_cell_id (regression)', () => {
      const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((cb) => {
        cb(performance.now());
        return 1;
      });
      try {
        const onPreviewParameter = vi.fn();
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
          <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={onPreviewParameter} onScrubLocal={vi.fn()} />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.input(slider, { target: { value: '400' } });

        expect(onPreviewParameter).toHaveBeenCalledWith('Kinematic.y_pos', expect.stringMatching(/mm$/));
      } finally {
        rafSpy.mockRestore();
      }
    });

    it('literal_bound slider change commits under synth_param_name', async () => {
      const raf = installManualRaf();
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
          <MechanismPanel
            descriptors={[desc]}
            onSetParameter={onSetParameter}
            onPreviewParameter={vi.fn()}
            onScrubLocal={vi.fn()}
          />
        ));
        const slider = screen.getByRole('slider') as HTMLInputElement;
        fireEvent.change(slider, { target: { value: '400' } });
        await flushPendingPromises();

        expect(onSetParameter).toHaveBeenCalledWith('__joint_x_axis_v', '400mm');
      } finally {
        raf.restore();
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
          <MechanismPanel descriptors={[desc]} onSetParameter={vi.fn()} onPreviewParameter={vi.fn()} onScrubLocal={onScrubLocal} />
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
