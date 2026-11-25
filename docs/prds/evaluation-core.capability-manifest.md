# Evaluation core: capability manifest

**PRD:** `docs/prds/evaluation-core.md`. Written when the PRD was decomposed, on 2026-11-25. This file is generated from its twin, `docs/prds/evaluation-core.capability-manifest.yaml`, which the scheduler reads; change the twin and generate this file again, never this file alone.

Every capability here is built by the leaf that lists it: the crate does not exist before leaf α. So each row's evidence is "built here", and what can be checked mechanically is that the leaf's named tests exist. A check is a pattern searched for with `git grep -E` under the paths given, or a file that must exist. A row marked "manual" has no mechanical check, for the reason given; it does not gate anything. The check says a test is there, not that it passes: the merge gate runs the tests.

The leaf whose completion delivers the PRD's goal is φ.

## α (#10553) — The crate, the body interface and the executor

| Capability | Evidence | Check |
|---|---|---|
| crate-registered-in-the-workspace | built here: `crates/reify-eval-core` is a member of the workspace, with the registrations of PRD 5.3 | pattern `"crates/reify-eval-core"` present under `Cargo.toml` |
| body-interface-and-executor-signal | built here: the eight clauses of the leaf's signal, each a test of the crate | manual: the PRD states this signal in prose and gives its tests no names, so no pattern can be written before the leaf chooses them |

## χ (#10554) — Programs that must not compile

| Capability | Evidence | Check |
|---|---|---|
| must-not-compile-programs-run-in-the-gate | built here: one `trybuild` test over the five programs that must fail and the two that must compile (PRD 5.2, decision D9) | pattern `TestCases::new\(\)` present under `crates/reify-eval-core/tests` |

## β (#10555) — Structure by declaration

| Capability | Evidence | Check |
|---|---|---|
| a_body_learns_its_declaration_only_through_recorded_reads | built here: the test `a_body_learns_its_declaration_only_through_recorded_reads` of the crate, an entry of the catalogue under this leaf | pattern `fn a_body_learns_its_declaration_only_through_recorded_reads\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## γ (#10556) — The reference interpreter and random sequences

| Capability | Evidence | Check |
|---|---|---|
| reference-interpreter-and-random-sequences | built here: test code only (the reference interpreter, the generator, the shrinker, the printer); G0 over `cell`, `scope`, `guard` and `collection` | manual: the leaf is test code whose items the PRD does not name (question 1 of PRD section 15 leaves their form to the leaf), so no pattern can be written before it |

## δ (#10557) — The store

| Capability | Evidence | Check |
|---|---|---|
| a_body_never_observes_an_unready_read | built here: the test `a_body_never_observes_an_unready_read` of the crate, an entry of the catalogue under this leaf | pattern `fn a_body_never_observes_an_unready_read\(` present under `crates/reify-eval-core` |
| values_of_different_types_never_share_a_content | built here: the test `values_of_different_types_never_share_a_content` of the crate, an entry of the catalogue under this leaf | pattern `fn values_of_different_types_never_share_a_content\(` present under `crates/reify-eval-core` |
| two_places_check_catches_resource_minted_id_in_result | built here: the test `two_places_check_catches_resource_minted_id_in_result` of the crate, an entry of the catalogue under this leaf | pattern `fn two_places_check_catches_resource_minted_id_in_result\(` present under `crates/reify-eval-core` |
| order_check_detects_result_mutated_through_exclusive_resource | built here: the test `order_check_detects_result_mutated_through_exclusive_resource` of the crate, an entry of the catalogue under this leaf | pattern `fn order_check_detects_result_mutated_through_exclusive_resource\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## ε (#10558) — Cycles

| Capability | Evidence | Check |
|---|---|---|
| a_binding_to_a_sibling_place_is_not_a_read_by_the_declaring_scope | built here: the test `a_binding_to_a_sibling_place_is_not_a_read_by_the_declaring_scope` of the crate, an entry of the catalogue under this leaf | pattern `fn a_binding_to_a_sibling_place_is_not_a_read_by_the_declaring_scope\(` present under `crates/reify-eval-core` |
| two_cell_ring_answers_cycle_on_both_whatever_is_demanded_first | built here: the test `two_cell_ring_answers_cycle_on_both_whatever_is_demanded_first` of the crate, an entry of the catalogue under this leaf | pattern `fn two_cell_ring_answers_cycle_on_both_whatever_is_demanded_first\(` present under `crates/reify-eval-core` |
| conditional_mutual_reference_without_a_ring_of_running_bodies_is_not_a_cycle | built here: the test `conditional_mutual_reference_without_a_ring_of_running_bodies_is_not_a_cycle` of the crate, an entry of the catalogue under this leaf | pattern `fn conditional_mutual_reference_without_a_ring_of_running_bodies_is_not_a_cycle\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## η (#10559) — Limits as settings, and the paths budget

| Capability | Evidence | Check |
|---|---|---|
| depth_limit_failure_is_a_fact_about_the_path_not_shared_by_content | built here: the test `depth_limit_failure_is_a_fact_about_the_path_not_shared_by_content` of the crate, an entry of the catalogue under this leaf | pattern `fn depth_limit_failure_is_a_fact_about_the_path_not_shared_by_content\(` present under `crates/reify-eval-core` |
| shared_exponential_tree_ends_by_paths_budget_not_node_budget | built here: the test `shared_exponential_tree_ends_by_paths_budget_not_node_budget` of the crate, an entry of the catalogue under this leaf | pattern `fn shared_exponential_tree_ends_by_paths_budget_not_node_budget\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## θ (#10560) — Children declared by a running body

| Capability | Evidence | Check |
|---|---|---|
| children_declared_in_a_loop_get_distinct_paths | built here: the test `children_declared_in_a_loop_get_distinct_paths` of the crate, an entry of the catalogue under this leaf | pattern `fn children_declared_in_a_loop_get_distinct_paths\(` present under `crates/reify-eval-core` |
| one_inner_call_site_reached_from_two_outer_calls_gets_two_paths | built here: the test `one_inner_call_site_reached_from_two_outer_calls_gets_two_paths` of the crate, an entry of the catalogue under this leaf | pattern `fn one_inner_call_site_reached_from_two_outer_calls_gets_two_paths\(` present under `crates/reify-eval-core` |
| body_declared_child_is_shared_and_resolvable_without_rerunning_its_parent | built here: the test `body_declared_child_is_shared_and_resolvable_without_rerunning_its_parent` of the crate, an entry of the catalogue under this leaf | pattern `fn body_declared_child_is_shared_and_resolvable_without_rerunning_its_parent\(` present under `crates/reify-eval-core` |
| unbounded_body_declared_recursion_stops_at_the_depth_limit_with_one_failure | built here: the test `unbounded_body_declared_recursion_stops_at_the_depth_limit_with_one_failure` of the crate, an entry of the catalogue under this leaf | pattern `fn unbounded_body_declared_recursion_stops_at_the_depth_limit_with_one_failure\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## ι (#10561) — Views

| Capability | Evidence | Check |
|---|---|---|
| a_substitution_under_an_absent_parent_answers_absent | built here: the test `a_substitution_under_an_absent_parent_answers_absent` of the crate, an entry of the catalogue under this leaf | pattern `fn a_substitution_under_an_absent_parent_answers_absent\(` present under `crates/reify-eval-core` |
| an_unassigned_unknown_inside_its_own_solve_never_reaches_the_solve | built here: the test `an_unassigned_unknown_inside_its_own_solve_never_reaches_the_solve` of the crate, an entry of the catalogue under this leaf | pattern `fn an_unassigned_unknown_inside_its_own_solve_never_reaches_the_solve\(` present under `crates/reify-eval-core` |
| solve_reached_again_under_its_own_view_is_a_cycle_and_terminates | built here: the test `solve_reached_again_under_its_own_view_is_a_cycle_and_terminates` of the crate, an entry of the catalogue under this leaf | pattern `fn solve_reached_again_under_its_own_view_is_a_cycle_and_terminates\(` present under `crates/reify-eval-core` |
| read_outside_view_after_opening_it_fails_the_run | built here: the test `read_outside_view_after_opening_it_fails_the_run` of the crate, an entry of the catalogue under this leaf | pattern `fn read_outside_view_after_opening_it_fails_the_run\(` present under `crates/reify-eval-core` |
| view_reports_which_answers_depended_on_substitution | built here: the test `view_reports_which_answers_depended_on_substitution` of the crate, an entry of the catalogue under this leaf | pattern `fn view_reports_which_answers_depended_on_substitution\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## κ (#10562) — What a view-opener's trials rested on

| Capability | Evidence | Check |
|---|---|---|
| solve_rests_on_ordinary_values_its_constraints_read_under_trial | built here: the test `solve_rests_on_ordinary_values_its_constraints_read_under_trial` of the crate, an entry of the catalogue under this leaf | pattern `fn solve_rests_on_ordinary_values_its_constraints_read_under_trial\(` present under `crates/reify-eval-core` |
| rests_on_member_with_changed_content_invalidates_stored_solve | built here: the test `rests_on_member_with_changed_content_invalidates_stored_solve` of the crate, an entry of the catalogue under this leaf | pattern `fn rests_on_member_with_changed_content_invalidates_stored_solve\(` present under `crates/reify-eval-core` |
| solve_rests_on_inputs_its_trial_nodes_read_after_edit_and_across_places | built here: the test `solve_rests_on_inputs_its_trial_nodes_read_after_edit_and_across_places` of the crate, an entry of the catalogue under this leaf | pattern `fn solve_rests_on_inputs_its_trial_nodes_read_after_edit_and_across_places\(` present under `crates/reify-eval-core` |
| rests_on_check_evaluates_members_as_ordinary_answers_of_the_snapshot | built here: the test `rests_on_check_evaluates_members_as_ordinary_answers_of_the_snapshot` of the crate, an entry of the catalogue under this leaf | pattern `fn rests_on_check_evaluates_members_as_ordinary_answers_of_the_snapshot\(` present under `crates/reify-eval-core` |
| rests_on_check_stops_at_first_changed_member_in_first_read_order | built here: the test `rests_on_check_stops_at_first_changed_member_in_first_read_order` of the crate, an entry of the catalogue under this leaf | pattern `fn rests_on_check_stops_at_first_changed_member_in_first_read_order\(` present under `crates/reify-eval-core` |
| rests_on_check_that_waits_on_a_running_reader_misses_instead_of_cycling | built here: the test `rests_on_check_that_waits_on_a_running_reader_misses_instead_of_cycling` of the crate, an entry of the catalogue under this leaf | pattern `fn rests_on_check_that_waits_on_a_running_reader_misses_instead_of_cycling\(` present under `crates/reify-eval-core` |
| final_state_finds_trial_results_when_solver_substitutes_final_content | built here: the test `final_state_finds_trial_results_when_solver_substitutes_final_content` of the crate, an entry of the catalogue under this leaf | pattern `fn final_state_finds_trial_results_when_solver_substitutes_final_content\(` present under `crates/reify-eval-core` |
| change_and_change_back_refinds_a_view_opening_node | built here: the test `change_and_change_back_refinds_a_view_opening_node` of the crate, an entry of the catalogue under this leaf | pattern `fn change_and_change_back_refinds_a_view_opening_node\(` present under `crates/reify-eval-core` |
| stored_solve_is_not_found_when_a_rested_on_member_comes_to_depend_on_a_substituted_path | built here: the test `stored_solve_is_not_found_when_a_rested_on_member_comes_to_depend_on_a_substituted_path` of the crate, an entry of the catalogue under this leaf | pattern `fn stored_solve_is_not_found_when_a_rested_on_member_comes_to_depend_on_a_substituted_path\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## λ (#10563) — A view inside a view

| Capability | Evidence | Check |
|---|---|---|
| guard_that_reads_an_unknown_is_evaluated_per_trial_not_a_cycle | built here: the test `guard_that_reads_an_unknown_is_evaluated_per_trial_not_a_cycle` of the crate, an entry of the catalogue under this leaf | pattern `fn guard_that_reads_an_unknown_is_evaluated_per_trial_not_a_cycle\(` present under `crates/reify-eval-core` |
| mutually_dependent_solves_fail_as_one_cycle_whatever_the_demand_order | built here: the test `mutually_dependent_solves_fail_as_one_cycle_whatever_the_demand_order` of the crate, an entry of the catalogue under this leaf | pattern `fn mutually_dependent_solves_fail_as_one_cycle_whatever_the_demand_order\(` present under `crates/reify-eval-core` |
| solves_coupled_through_arguments_are_a_plain_ring | built here: the test `solves_coupled_through_arguments_are_a_plain_ring` of the crate, an entry of the catalogue under this leaf | pattern `fn solves_coupled_through_arguments_are_a_plain_ring\(` present under `crates/reify-eval-core` |
| inner_solve_with_outer_dependent_inputs_follows_each_outer_trial_without_cycle | built here: the test `inner_solve_with_outer_dependent_inputs_follows_each_outer_trial_without_cycle` of the crate, an entry of the catalogue under this leaf | pattern `fn inner_solve_with_outer_dependent_inputs_follows_each_outer_trial_without_cycle\(` present under `crates/reify-eval-core` |
| element_solve_independent_of_the_count_answers_under_every_count_trial | built here: the test `element_solve_independent_of_the_count_answers_under_every_count_trial` of the crate, an entry of the catalogue under this leaf | pattern `fn element_solve_independent_of_the_count_answers_under_every_count_trial\(` present under `crates/reify-eval-core` |
| inner_solve_follows_outer_trial_when_its_view_reads_another_part_of_the_outer_substitution | built here: the test `inner_solve_follows_outer_trial_when_its_view_reads_another_part_of_the_outer_substitution` of the crate, an entry of the catalogue under this leaf | pattern `fn inner_solve_follows_outer_trial_when_its_view_reads_another_part_of_the_outer_substitution\(` present under `crates/reify-eval-core` |
| inner_solve_infeasible_under_all_but_one_outer_trial_is_found_by_the_outer_solve | built here: the test `inner_solve_infeasible_under_all_but_one_outer_trial_is_found_by_the_outer_solve` of the crate, an entry of the catalogue under this leaf | pattern `fn inner_solve_infeasible_under_all_but_one_outer_trial_is_found_by_the_outer_solve\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## μ₁ (#10564) — What a new snapshot inherits

| Capability | Evidence | Check |
|---|---|---|
| absent_answers_are_indexed_with_their_ancestor_reads | built here: the test `absent_answers_are_indexed_with_their_ancestor_reads` of the crate, an entry of the catalogue under this leaf | pattern `fn absent_answers_are_indexed_with_their_ancestor_reads\(` present under `crates/reify-eval-core` |
| scope_gaining_a_member_compares_one_declaration_per_child_and_touches_nothing_below | built here: the test `scope_gaining_a_member_compares_one_declaration_per_child_and_touches_nothing_below` of the crate, an entry of the catalogue under this leaf | pattern `fn scope_gaining_a_member_compares_one_declaration_per_child_and_touches_nothing_below\(` present under `crates/reify-eval-core` |
| work_after_an_edit_stops_where_content_is_unchanged | built here: the test `work_after_an_edit_stops_where_content_is_unchanged` of the crate, an entry of the catalogue under this leaf | pattern `fn work_after_an_edit_stops_where_content_is_unchanged\(` present under `crates/reify-eval-core` |
| repointing_a_passed_along_binding_reaches_the_child | built here: the test `repointing_a_passed_along_binding_reaches_the_child` of the crate, an entry of the catalogue under this leaf | pattern `fn repointing_a_passed_along_binding_reaches_the_child\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## μ₂ (#10565) — Inheriting a node whose body declared children

| Capability | Evidence | Check |
|---|---|---|
| child_a_rerun_body_stops_declaring_becomes_absent | built here: the test `child_a_rerun_body_stops_declaring_becomes_absent` of the crate, an entry of the catalogue under this leaf | pattern `fn child_a_rerun_body_stops_declaring_becomes_absent\(` present under `crates/reify-eval-core` |
| child_of_a_parent_answered_from_the_store_is_absent_when_not_declared | built here: the test `child_of_a_parent_answered_from_the_store_is_absent_when_not_declared` of the crate, an entry of the catalogue under this leaf | pattern `fn child_of_a_parent_answered_from_the_store_is_absent_when_not_declared\(` present under `crates/reify-eval-core` |
| branch_left_by_an_edit_is_not_evaluated_before_its_reader_is_rechecked | built here: the test `branch_left_by_an_edit_is_not_evaluated_before_its_reader_is_rechecked` of the crate, an entry of the catalogue under this leaf | pattern `fn branch_left_by_an_edit_is_not_evaluated_before_its_reader_is_rechecked\(` present under `crates/reify-eval-core` |
| child_appearing_at_a_path_already_answered_absent_is_found | built here: the test `child_appearing_at_a_path_already_answered_absent_is_found` of the crate, an entry of the catalogue under this leaf | pattern `fn child_appearing_at_a_path_already_answered_absent_is_found\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## ν (#10566) — Inheriting a node that opened a view

| Capability | Evidence | Check |
|---|---|---|
| independent_solve_groups_fail_and_resolve_independently | built here: the test `independent_solve_groups_fail_and_resolve_independently` of the crate, an entry of the catalogue under this leaf | pattern `fn independent_solve_groups_fail_and_resolve_independently\(` present under `crates/reify-eval-core` |
| edit_that_routes_a_rested_on_member_through_the_unknown_is_a_miss_not_a_cycle | built here: the test `edit_that_routes_a_rested_on_member_through_the_unknown_is_a_miss_not_a_cycle` of the crate, an entry of the catalogue under this leaf | pattern `fn edit_that_routes_a_rested_on_member_through_the_unknown_is_a_miss_not_a_cycle\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## ξ₁ (#10567) — Inheriting from a snapshot whose work was unfinished

| Capability | Evidence | Check |
|---|---|---|
| edit_made_while_the_parent_is_still_evaluating_never_inherits_a_stale_answer | built here: the test `edit_made_while_the_parent_is_still_evaluating_never_inherits_a_stale_answer` of the crate, an entry of the catalogue under this leaf | pattern `fn edit_made_while_the_parent_is_still_evaluating_never_inherits_a_stale_answer\(` present under `crates/reify-eval-core` |
| entry_inherited_from_an_unfinished_walk_is_reconfirmed_before_it_is_served | built here: the test `entry_inherited_from_an_unfinished_walk_is_reconfirmed_before_it_is_served` of the crate, an entry of the catalogue under this leaf | pattern `fn entry_inherited_from_an_unfinished_walk_is_reconfirmed_before_it_is_served\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## ξ₂ (#10568) — A changed demand

| Capability | Evidence | Check |
|---|---|---|
| path_dropped_from_demand_is_not_inherited_stale | built here: the test `path_dropped_from_demand_is_not_inherited_stale` of the crate, an entry of the catalogue under this leaf | pattern `fn path_dropped_from_demand_is_not_inherited_stale\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## ξ₃ (#10569) — A changed limit setting

| Capability | Evidence | Check |
|---|---|---|
| changing_a_limit_setting_reaches_every_path_whose_check_changes | built here: the test `changing_a_limit_setting_reaches_every_path_whose_check_changes` of the crate, an entry of the catalogue under this leaf | pattern `fn changing_a_limit_setting_reaches_every_path_whose_check_changes\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## ζ (#10570) — Joining a run in flight

| Capability | Evidence | Check |
|---|---|---|
| first_evaluation_of_a_fold_over_n_elements_runs_its_body_once | built here: the test `first_evaluation_of_a_fold_over_n_elements_runs_its_body_once` of the crate, an entry of the catalogue under this leaf | pattern `fn first_evaluation_of_a_fold_over_n_elements_runs_its_body_once\(` present under `crates/reify-eval-core` |
| inner_solve_independent_of_the_outer_trial_runs_once | built here: the test `inner_solve_independent_of_the_outer_trial_runs_once` of the crate, an entry of the catalogue under this leaf | pattern `fn inner_solve_independent_of_the_outer_trial_runs_once\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## ο (#10571) — Determinism and the nonce

| Capability | Evidence | Check |
|---|---|---|
| seeded_reader_of_nondet_result_not_found_after_remake | built here: the test `seeded_reader_of_nondet_result_not_found_after_remake` of the crate, an entry of the catalogue under this leaf | pattern `fn seeded_reader_of_nondet_result_not_found_after_remake\(` present under `crates/reify-eval-core` |
| debug_mode_catches_a_false_determinism_claim | built here: the test `debug_mode_catches_a_false_determinism_claim` of the crate, an entry of the catalogue under this leaf | pattern `fn debug_mode_catches_a_false_determinism_claim\(` present under `crates/reify-eval-core` |
| answers_downstream_of_a_nondeterministic_result_equal_a_fresh_evaluation_seeded_with_it | built here: the test `answers_downstream_of_a_nondeterministic_result_equal_a_fresh_evaluation_seeded_with_it` of the crate, an entry of the catalogue under this leaf | pattern `fn answers_downstream_of_a_nondeterministic_result_equal_a_fresh_evaluation_seeded_with_it\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## π (#10572) — What was there last time

| Capability | Evidence | Check |
|---|---|---|
| equal_roots_share_results_but_not_place | built here: the test `equal_roots_share_results_but_not_place` of the crate, an entry of the catalogue under this leaf | pattern `fn equal_roots_share_results_but_not_place\(` present under `crates/reify-eval-core` |
| identical_lookups_in_flight_run_the_body_once | built here: the test `identical_lookups_in_flight_run_the_body_once` of the crate, an entry of the catalogue under this leaf | pattern `fn identical_lookups_in_flight_run_the_body_once\(` present under `crates/reify-eval-core` |
| previous_is_last_final_answer_outside_views_and_changed_covers_only_reads_made | built here: the test `previous_is_last_final_answer_outside_views_and_changed_covers_only_reads_made` of the crate, an entry of the catalogue under this leaf | pattern `fn previous_is_last_final_answer_outside_views_and_changed_covers_only_reads_made\(` present under `crates/reify-eval-core` |
| previous_found_at_body_declared_child_after_argument_change | built here: the test `previous_found_at_body_declared_child_after_argument_change` of the crate, an entry of the catalogue under this leaf | pattern `fn previous_found_at_body_declared_child_after_argument_change\(` present under `crates/reify-eval-core` |
| kept_state_reaches_next_run_at_same_path_after_input_change | built here: the test `kept_state_reaches_next_run_at_same_path_after_input_change` of the crate, an entry of the catalogue under this leaf | pattern `fn kept_state_reaches_next_run_at_same_path_after_input_change\(` present under `crates/reify-eval-core` |
| returning_the_previous_result_keeps_its_content_and_spares_its_readers | built here: the test `returning_the_previous_result_keeps_its_content_and_spares_its_readers` of the crate, an entry of the catalogue under this leaf | pattern `fn returning_the_previous_result_keeps_its_content_and_spares_its_readers\(` present under `crates/reify-eval-core` |
| warm_started_hashable_result_with_unchanged_value_keeps_content | built here: the test `warm_started_hashable_result_with_unchanged_value_keeps_content` of the crate, an entry of the catalogue under this leaf | pattern `fn warm_started_hashable_result_with_unchanged_value_keeps_content\(` present under `crates/reify-eval-core` |
| warm_start_never_changes_whether_a_solve_succeeds | built here: the test `warm_start_never_changes_whether_a_solve_succeeds` of the crate, an entry of the catalogue under this leaf | pattern `fn warm_start_never_changes_whether_a_solve_succeeds\(` present under `crates/reify-eval-core` |
| warm_started_trials_file_an_alternative_rested_on_set_beside_the_cold_one | built here: the test `warm_started_trials_file_an_alternative_rested_on_set_beside_the_cold_one` of the crate, an entry of the catalogue under this leaf | pattern `fn warm_started_trials_file_an_alternative_rested_on_set_beside_the_cold_one\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## ρ (#10573) — Ownership, trimming and release

| Capability | Evidence | Check |
|---|---|---|
| looking_inside_a_passed_opaque_value_is_a_recorded_read | built here: the test `looking_inside_a_passed_opaque_value_is_a_recorded_read` of the crate, an entry of the catalogue under this leaf | pattern `fn looking_inside_a_passed_opaque_value_is_a_recorded_read\(` present under `crates/reify-eval-core` |
| a_nondeterministic_answer_is_pinned_for_every_reader_in_its_snapshot | built here: the test `a_nondeterministic_answer_is_pinned_for_every_reader_in_its_snapshot` of the crate, an entry of the catalogue under this leaf | pattern `fn a_nondeterministic_answer_is_pinned_for_every_reader_in_its_snapshot\(` present under `crates/reify-eval-core` |
| remade_nondeterministic_result_gets_new_content_and_downstream_reruns | built here: the test `remade_nondeterministic_result_gets_new_content_and_downstream_reruns` of the crate, an entry of the catalogue under this leaf | pattern `fn remade_nondeterministic_result_gets_new_content_and_downstream_reruns\(` present under `crates/reify-eval-core` |
| results_reached_only_from_trials_are_trimmed_first_and_released | built here: the test `results_reached_only_from_trials_are_trimmed_first_and_released` of the crate, an entry of the catalogue under this leaf | pattern `fn results_reached_only_from_trials_are_trimmed_first_and_released\(` present under `crates/reify-eval-core` |
| trimmed_shapes_released_once_and_slider_session_stays_bounded | built here: the test `trimmed_shapes_released_once_and_slider_session_stays_bounded` of the crate, an entry of the catalogue under this leaf | pattern `fn trimmed_shapes_released_once_and_slider_session_stays_bounded\(` present under `crates/reify-eval-core` |
| every_shape_produced_for_a_dropped_body_is_released | built here: the test `every_shape_produced_for_a_dropped_body_is_released` of the crate, an entry of the catalogue under this leaf | pattern `fn every_shape_produced_for_a_dropped_body_is_released\(` present under `crates/reify-eval-core` |
| trimming_a_nonce_result_leaves_its_dependents_reclaimable | built here: the test `trimming_a_nonce_result_leaves_its_dependents_reclaimable` of the crate, an entry of the catalogue under this leaf | pattern `fn trimming_a_nonce_result_leaves_its_dependents_reclaimable\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## σ (#10574) — Freshness and progress

| Capability | Evidence | Check |
|---|---|---|
| exclusive_section_handle_offers_cancelled_progress_emit_without_waiting | built here: the test `exclusive_section_handle_offers_cancelled_progress_emit_without_waiting` of the crate, an entry of the catalogue under this leaf | pattern `fn exclusive_section_handle_offers_cancelled_progress_emit_without_waiting\(` present under `crates/reify-eval-core` |
| reader_of_an_emitted_partial_is_recomputed_when_the_input_becomes_final | built here: the test `reader_of_an_emitted_partial_is_recomputed_when_the_input_becomes_final` of the crate, an entry of the catalogue under this leaf | pattern `fn reader_of_an_emitted_partial_is_recomputed_when_the_input_becomes_final\(` present under `crates/reify-eval-core` |
| intermediate_answer_becomes_final_in_its_snapshot_and_descendants | built here: the test `intermediate_answer_becomes_final_in_its_snapshot_and_descendants` of the crate, an entry of the catalogue under this leaf | pattern `fn intermediate_answer_becomes_final_in_its_snapshot_and_descendants\(` present under `crates/reify-eval-core` |
| a_transient_failure_does_not_survive_into_a_later_snapshot | built here: the test `a_transient_failure_does_not_survive_into_a_later_snapshot` of the crate, an entry of the catalogue under this leaf | pattern `fn a_transient_failure_does_not_survive_into_a_later_snapshot\(` present under `crates/reify-eval-core` |
| progress_reaches_watcher_and_never_changes_result_or_key | built here: the test `progress_reaches_watcher_and_never_changes_result_or_key` of the crate, an entry of the catalogue under this leaf | pattern `fn progress_reaches_watcher_and_never_changes_result_or_key\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## τ (#10575) — Scheduling and budgets

| Capability | Evidence | Check |
|---|---|---|
| cold_reads_between_nodes_of_one_kind_do_not_deadlock_and_identical_runs_are_shared | built here: the test `cold_reads_between_nodes_of_one_kind_do_not_deadlock_and_identical_runs_are_shared` of the crate, an entry of the catalogue under this leaf | pattern `fn cold_reads_between_nodes_of_one_kind_do_not_deadlock_and_identical_runs_are_shared\(` present under `crates/reify-eval-core` |
| budget_exhaustion_is_one_request_error_and_stores_nothing | built here: the test `budget_exhaustion_is_one_request_error_and_stores_nothing` of the crate, an entry of the catalogue under this leaf | pattern `fn budget_exhaustion_is_one_request_error_and_stores_nothing\(` present under `crates/reify-eval-core` |
| solver_budget_exhaustion_is_stored_and_request_budget_exhaustion_is_not | built here: the test `solver_budget_exhaustion_is_stored_and_request_budget_exhaustion_is_not` of the crate, an entry of the catalogue under this leaf | pattern `fn solver_budget_exhaustion_is_stored_and_request_budget_exhaustion_is_not\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## υ (#10576) — What an answer read, and what read it

| Capability | Evidence | Check |
|---|---|---|
| read_all_of_child_declarations_starts_every_child_before_waiting | built here: the test `read_all_of_child_declarations_starts_every_child_before_waiting` of the crate, an entry of the catalogue under this leaf | pattern `fn read_all_of_child_declarations_starts_every_child_before_waiting\(` present under `crates/reify-eval-core` |
| signal-clauses-in-prose | built here: the clauses of the leaf's signal that the PRD states in prose (G0 with this leaf's generator feature, counter assertions), each a test of the crate | manual: the PRD gives these tests no names; the named tests above stand for the leaf at the dispatch gate |

## φ (#10577) — Section 11 at size, and the boundary scenarios

| Capability | Evidence | Check |
|---|---|---|
| scenario-K1 | built here: the test of boundary scenario K1 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k01_` present under `crates/reify-eval-core` |
| scenario-K2 | built here: the test of boundary scenario K2 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k02_` present under `crates/reify-eval-core` |
| scenario-K3 | built here: the test of boundary scenario K3 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k03_` present under `crates/reify-eval-core` |
| scenario-K4 | built here: the test of boundary scenario K4 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k04_` present under `crates/reify-eval-core` |
| scenario-K5 | built here: the test of boundary scenario K5 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k05_` present under `crates/reify-eval-core` |
| scenario-K6 | built here: the test of boundary scenario K6 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k06_` present under `crates/reify-eval-core` |
| scenario-K7 | built here: the test of boundary scenario K7 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k07_` present under `crates/reify-eval-core` |
| scenario-K8 | built here: the test of boundary scenario K8 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k08_` present under `crates/reify-eval-core` |
| scenario-K9 | built here: the test of boundary scenario K9 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k09_` present under `crates/reify-eval-core` |
| scenario-K10 | built here: the test of boundary scenario K10 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k10_` present under `crates/reify-eval-core` |
| scenario-K11 | built here: the test of boundary scenario K11 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k11_` present under `crates/reify-eval-core` |
| scenario-K12 | built here: the test of boundary scenario K12 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k12_` present under `crates/reify-eval-core` |
| scenario-K13 | built here: the test of boundary scenario K13 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k13_` present under `crates/reify-eval-core` |
| scenario-K14 | built here: the test of boundary scenario K14 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k14_` present under `crates/reify-eval-core` |
| scenario-K15 | built here: the test of boundary scenario K15 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_k15_` present under `crates/reify-eval-core` |
| scenario-H1 | built here: the test of boundary scenario H1 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_h01_` present under `crates/reify-eval-core` |
| scenario-H2 | built here: the test of boundary scenario H2 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_h02_` present under `crates/reify-eval-core` |
| scenario-H3 | built here: the test of boundary scenario H3 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_h03_` present under `crates/reify-eval-core` |
| scenario-H4 | built here: the test of boundary scenario H4 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_h04_` present under `crates/reify-eval-core` |
| scenario-H5 | built here: the test of boundary scenario H5 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_h05_` present under `crates/reify-eval-core` |
| scenario-H6 | built here: the test of boundary scenario H6 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_h06_` present under `crates/reify-eval-core` |
| scenario-H7 | built here: the test of boundary scenario H7 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_h07_` present under `crates/reify-eval-core` |
| scenario-H8 | built here: the test of boundary scenario H8 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_h08_` present under `crates/reify-eval-core` |
| scenario-H9 | built here: the test of boundary scenario H9 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_h09_` present under `crates/reify-eval-core` |
| scenario-H10 | built here: the test of boundary scenario H10 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_h10_` present under `crates/reify-eval-core` |
| scenario-H11 | built here: the test of boundary scenario H11 (PRD section 10), named by the prefix PRD 12.4 fixes | pattern `fn scenario_h11_` present under `crates/reify-eval-core` |
| growth-report-committed | built here: `docs/prds/evaluation-core.growth-report.md`, with the four counters for each model and each N | file exists: `docs/prds/evaluation-core.growth-report.md` |
| growth-tests-g1-to-g4 | built here: the growth tests on every synthetic model of contract section 11 at N = 100, 1,000 and 10,000 | manual: the PRD gives the growth tests no names; the scenario checks and the report check stand for the leaf |

## ω (#10578) — Close

| Capability | Evidence | Check |
|---|---|---|
| prd-stamped-shipped | built here: the PRD's status line carries the terminal word, with the landed task ids; the contract loses its provisional status | pattern `^\*\*Status[^A-Za-z]*SHIPPED` present under `docs/prds/evaluation-core.md` |
