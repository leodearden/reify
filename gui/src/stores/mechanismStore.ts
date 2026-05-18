import { createStore, produce } from 'solid-js/store';
import type { MechanismDescriptor, JointDescriptor } from '../types';

// ---------------------------------------------------------------------------
// Binding-aware current-SI helper (Task 3788 η-frontend)
// ---------------------------------------------------------------------------

/**
 * Return the authoritative current-SI value for a joint using its `binding`
 * field.  Used by MechanismPanel for initial/effective slider values and by
 * `refresh()` for the optimistic-override equality check.
 *
 * - `param_bound`  → `binding.current_value_si` (falls back to legacy field)
 * - `literal_bound` → `binding.initial_value_si` (AST literal baseline; the
 *    engine never surfaces a post-scrub "current" value for LiteralBound)
 * - `coupling_derived` / `fixed_no_motion` → `null`
 */
export function jointCurrentSi(joint: Pick<JointDescriptor, 'binding' | 'current_value_si'>): number | null {
  const b = joint.binding;
  if (b.kind === 'param_bound') return b.current_value_si ?? joint.current_value_si;
  if (b.kind === 'literal_bound') return b.initial_value_si;
  return null;
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface MechanismStoreState {
  /** All non-errored mechanism descriptors from the latest backend refresh. */
  descriptors: MechanismDescriptor[];
  /**
   * Optimistic per-joint value overrides for in-flight slider scrubs.
   * Key format: `"${cell_id}:${joint_index}"`.
   * Cleared on refresh when the new descriptor's `current_value_si` matches
   * the override (commit confirmation).
   */
  optimistic: Record<string, number>;
}

export interface MechanismStoreDeps {
  /** Bridge function that fetches descriptor list from the Tauri backend. */
  getMechanismDescriptors: () => Promise<MechanismDescriptor[]>;
}

// ---------------------------------------------------------------------------
// Store factory
// ---------------------------------------------------------------------------

/**
 * Creates a SolidJS store that holds mechanism descriptors and tracks
 * optimistic slider overrides for in-flight parameter scrubs.
 *
 * Usage:
 *   const store = createMechanismStore({ getMechanismDescriptors: bridgeGetMechanismDescriptors });
 *   await store.refresh();
 *   store.setOptimistic(cellId, jointIndex, valueSi);
 *   const displayed = store.getEffectiveValueSi(cellId, jointIndex, descriptor.current_value_si);
 */
export function createMechanismStore(deps: MechanismStoreDeps) {
  const [state, setState] = createStore<MechanismStoreState>({
    descriptors: [],
    optimistic: {},
  });

  /**
   * Refresh mechanism descriptors from the backend.
   *
   * After updating `descriptors`, clears any optimistic overrides whose
   * committed `current_value_si` now matches the override value — indicating
   * the backend has accepted the parameter change.
   */
  async function refresh(): Promise<void> {
    const newDescriptors = await deps.getMechanismDescriptors();

    // Build a set of keys to clear: overrides that have been "committed"
    // (i.e. the backend's authoritative current-SI is within tolerance of the
    // optimistic value — strict === is avoided because the JS display→SI
    // conversion, e.g. 90deg → 90 * π/180, may not bit-match the Rust-side
    // parse of "90deg").
    //
    // Binding semantics:
    //   param_bound   → uses binding.current_value_si (set by engine on commit)
    //   literal_bound → uses binding.initial_value_si (AST baseline; the engine
    //                   never surfaces a post-scrub value for LiteralBound, so a
    //                   literal-bound override will NOT equal the baseline unless
    //                   the user explicitly scrubbed back, which is fine to clear)
    //   coupling/fixed → null → never cleared
    const toDelete: string[] = [];
    for (const desc of newDescriptors) {
      for (const joint of desc.joints) {
        const key = `${desc.cell_id}:${joint.joint_index}`;
        const override = state.optimistic[key];
        const committedSi = jointCurrentSi(joint);
        if (
          override !== undefined &&
          committedSi !== null &&
          Math.abs(committedSi - override) <= 1e-9 * (1 + Math.abs(override))
        ) {
          toDelete.push(key);
        }
      }
    }

    setState(
      produce((s) => {
        s.descriptors = newDescriptors;
        for (const key of toDelete) {
          delete s.optimistic[key];
        }
      }),
    );
  }

  /**
   * Record an optimistic override for a joint slider.
   * Call this immediately when the user drags the slider so the UI stays
   * responsive before the backend confirms the new value.
   */
  function setOptimistic(cellId: string, jointIndex: number, valueSi: number): void {
    setState('optimistic', `${cellId}:${jointIndex}`, valueSi);
  }

  /**
   * Return the effective SI value to display for a joint slider.
   *
   * Priority: optimistic override > fallback (descriptor's `current_value_si`).
   *
   * @param cellId      - Mechanism cell id (e.g. `"Kinematic.m"`).
   * @param jointIndex  - Zero-based joint index.
   * @param fallback    - The descriptor's `current_value_si` (may be null).
   */
  function getEffectiveValueSi(cellId: string, jointIndex: number, fallback: number | null): number | null {
    const key = `${cellId}:${jointIndex}`;
    const override = state.optimistic[key];
    return override !== undefined ? override : fallback;
  }

  /** Clear all optimistic overrides (e.g. on source edit that invalidates the mechanism). */
  function clearOptimistic(): void {
    setState(produce((s) => {
      s.optimistic = {};
    }));
  }

  return { state, refresh, setOptimistic, getEffectiveValueSi, clearOptimistic };
}

export type MechanismStore = ReturnType<typeof createMechanismStore>;
