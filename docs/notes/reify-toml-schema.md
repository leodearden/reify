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

Array-of-tables. Each entry declares a per-node commitment-policy override that
fills the "Level 3" slot in the five-level precedence chain
(`docs/prds/v0_3/node-traits-unification.md` §6; implemented in
`crates/reify-runtime/src/commitment.rs`).

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
node_id_pattern = "value"
commitment_policy = "always_cancel_when_stale"

[[node_overrides]]
node_id_pattern = "Bracket.width"
commitment_policy = "only_run_on_final_inputs"
```

### Precedence

Override priority (highest → lowest) in `NodePolicyOverrides::resolve`:
1. Instance override (set_instance)
2. Type override (set_type) — kind selectors land here
3. Default (`CommitIfSlow`)

`resolve_with_traits` adds levels 3–5; config-file overrides fill Level 3 per PRD §6.
