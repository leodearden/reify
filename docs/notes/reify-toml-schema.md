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

Array-of-tables. Each entry declares a per-node commitment-policy override —
"Level 3" of the five-level precedence chain
(`docs/prds/v0_3/node-traits-unification.md` §6), read by
`NodePolicyOverrides::resolve_with_traits` in
`crates/reify-runtime/src/commitment.rs`. See **Precedence** below for how an
entry materialises today.

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

Override priority (highest → lowest), in PRD §6's numbering, resolved by
`NodePolicyOverrides::resolve_with_traits`:
1. Instance override — the `set_instance` map
2. Type override — the `set_type` map, where kind selectors land
3. Config-file `[[node_overrides]]` — reserved; not yet a distinct slot
4. Kind+traits default (`default_overrides`) — absent `COMMITTABLE` →
   `always_cancel_when_stale`, present → `commit_if_slow`
5. Hard default — `commit_if_slow`, `NodeCommitmentOverride`'s `Default`

Level 3 has no branch in `resolve_with_traits` today (task 3578 owns it), and
level 5 is a floor rather than a branch: level 4 always returns, so nothing
falls past it.

Until level 3 lands, `from_config_overrides` materialises each entry straight
into the level-1 or level-2 map — an instance selector becomes a `set_instance`
entry, a kind selector a `set_type` entry. A config entry is therefore
indistinguishable from a programmatic override of the same granularity: they
share one map, so the last write wins and neither source outranks the other.
What a `reify.toml` author can rely on is the rest of the chain — an entry beats
the kind+traits default at level 4, and an instance selector beats a kind
selector. Full chain rationale: PRD §6.
