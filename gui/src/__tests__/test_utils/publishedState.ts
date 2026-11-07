import type { GuiState, PublishedState } from '../../types';

/** `state` as a whole-state command replies it: stamped with the generation it was published under. */
export function published(state: GuiState, generation = 1): PublishedState {
  return { generation, state };
}
