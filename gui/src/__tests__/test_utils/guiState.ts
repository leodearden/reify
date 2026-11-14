import type { GuiState } from '../../types';

/** An empty GuiState snapshot, with `overrides` replacing whole fields. */
export function makeGuiState(overrides: Partial<GuiState> = {}): GuiState {
  return {
    meshes: [],
    values: [],
    constraints: [],
    files: [],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
    tensegrity_wires: [],
    tensegrity_surfaces: [],
    display_panes: [],
    display_appearance: [],
    fea_diagnostics: [],
    fea_convergence: null,
    ...overrides,
  };
}
