# reify.toml schema notes

Brief reference for the `reify.toml` manifest schema. The authoritative schema
documentation lives in the `reify-config` crate rustdoc (`crates/reify-config/src/lib.rs`
`# Schema` section).

## `[kernels]`

Maps kernel ids to pinned versions. Supported ids: `occt`, `manifold`, `fidget`,
`openvdb`, `gmsh`. Each pin is either an inline string or a `{ version = "..." }` table.

## `[auto_type_params]`

Optional. Controls the `auto:` type-parameter resolution algorithm.
Fields: `max_depth` (default 6), `max_cross_product_size` (default 100 000).

## `[[node_overrides]]`

Array-of-tables. Each entry declares a per-node commitment-policy override,
the config-file level of the five-level precedence chain
(`docs/prds/v0_3/node-traits-unification.md` §6). `reify-config` parses and
validates the entries, and `NodePolicyOverrides::from_config_overrides` in
`crates/reify-runtime/src/commitment.rs` converts them into the override maps
that `NodePolicyOverrides::resolve_with_traits` resolves. See **Precedence**
below for how a converted entry ranks.

### Status: parsed and validated, not enforced

1. **No shipped binary reads `[[node_overrides]]`.** No binary loads
   `reify.toml`, and `from_config_overrides` has no production caller.
2. **No scheduler consults a resolved commitment policy.** The concurrent
   scheduler was deleted in c1b8dba3f7 (task ο, #5065). The enforcement code,
   `check_commitment` and `CommitmentTracker`, is kept, but nothing in
   production references it (tests still exercise it). The task that owns its
   future is cited on those items' `// G-allow:`
   markers in `crates/reify-runtime/src/commitment.rs`.
3. **`reify dev inspect-node` does not read `reify.toml`.** Its
   `instance override` and `type override` lines always print `(none)`, and its
   `derived policy` line is the kind+traits default (level 4). It reports a
   policy; it applies nothing.

So an entry is accepted and then has no effect. The rest of this section
describes what the library computes if a consumer is ever wired. It does not
describe current runtime behaviour.

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `node_id_pattern` | string | Selector — see below. Surrounding whitespace trimmed; empty rejected. |
| `commitment_policy` | enum | One of `commit_if_slow`, `always_cancel_when_stale`, `only_run_on_final_inputs`. |

### Selector forms

Two forms are accepted (no glob expansion — exact matches only):

- **Kind selector** — exact NodeKind name (case-insensitive):
  `value`, `constraint`, `compute`, `realization`, `resolution`.
  Sets a type-level override for all nodes of that kind.
- **Instance selector** — `Entity.member` (a single `.`, non-empty halves).
  Maps to the `Value` kind's `NodeId::Value(ValueCellId::new(entity, member))`.
  Sets an instance-level override for that specific node.

Glob expansion over concrete node-ids requires the compiled graph and is not
yet implemented (future enhancement; noted in `NodePolicyOverrides::from_config_overrides`).

### Example

```toml
[[node_overrides]]
node_id_pattern = "compute"
commitment_policy = "always_cancel_when_stale"

[[node_overrides]]
node_id_pattern = "Bracket.width"
commitment_policy = "only_run_on_final_inputs"
```

Pick a `commitment_policy` that differs from the node's level-4 default (see
**Precedence**), or the entry is a no-op even in the library: `compute`
defaults to `commit_if_slow`, so the entry above changes the resolved policy,
whereas `value` + `always_cancel_when_stale` would not.

### Precedence

Override priority (highest → lowest), in PRD §6's numbering. The chain is owned
by `NodePolicyOverrides::resolve_with_traits`'s rustdoc; this is the reify.toml
author's view of it:
1. Instance override — the `set_instance` map
2. Type override — the `set_type` map, where kind selectors land
3. Config-file `[[node_overrides]]` — no distinct slot; entries land in
   levels 1 and 2 (below)
4. Kind+traits default (`default_overrides`) — absent `COMMITTABLE` →
   `always_cancel_when_stale`, present → `commit_if_slow`
5. Hard default — PRD §6's floor, `commit_if_slow`

Neither level 3 nor level 5 has a branch in `resolve_with_traits`. Level 3 has
no slot of its own, and no task owns one: GR-007 (#3464, done) chose to convert
config entries into the level-1 and level-2 maps instead. Level 4 always
returns, so nothing reaches the level-5 floor.

`from_config_overrides` puts each entry straight into the level-1 or level-2
map: an instance selector becomes a `set_instance` entry, a kind selector a
`set_type` entry. A config entry is therefore indistinguishable from a
programmatic override of the same granularity. They share one map, so the last
write wins and neither source outranks the other. What the library guarantees
for a converted entry is the rest of the chain: an entry beats the kind+traits
default at level 4, and an instance selector beats a kind selector. Full chain
rationale: PRD §6.
