# examples/module_visibility

Demonstrates the module-declaration + priv-visibility chain end-to-end via
`reify check` (PRD `docs/prds/v0_6/module-and-visibility-hardening.md` §6
two-way boundary table).

## What the example models

A producer module exposes a `pub structure def` with a mix of private and
default-visible parameters:

| Member          | Visibility | Behaviour from importer             |
|-----------------|------------|-------------------------------------|
| `rated_torque`  | `priv`     | Hidden — access emits E_PRIV_MEMBER_ACCESS |
| `shaft_diameter`| (default)  | Visible — access resolves cleanly   |

`consumer.ri` imports the producer and attempts to access both members, proving
the two-way signal:

- `m.shaft_diameter` resolves without error (default-visible).
- `m.rated_torque` triggers `E_PRIV_MEMBER_ACCESS` (private member hidden from
  importer).

`mismatch_variant.ri` shows what happens when the `module` declaration does not
agree with the file's location-derived stem — the CLI emits `E_MODULE_PATH_MISMATCH`.

All three files carry a `module` declaration so the results are unambiguous:

| File                 | `module` decl          | Stem           | Match? |
|----------------------|------------------------|----------------|--------|
| `producer.ri`        | `module producer`      | `producer`     | ✓      |
| `consumer.ri`        | `module consumer`      | `consumer`     | ✓      |
| `mismatch_variant.ri`| `module wrong.path.here`| `mismatch_variant` | ✗  |

## §6 two-way boundary rows covered

| §6 row                             | File                   | `reify check` result                  |
|------------------------------------|------------------------|---------------------------------------|
| declared path matches location     | `producer.ri`          | exit 0, no E_MODULE_PATH_MISMATCH     |
| priv param hidden from importer    | `consumer.ri`          | exit 1, E_PRIV_MEMBER_ACCESS (rated_torque) |
| default-visible param still works  | `consumer.ri`          | shaft_diameter NOT flagged in stderr  |
| declared path mismatches           | `mismatch_variant.ri`  | exit 1, E_MODULE_PATH_MISMATCH        |

## Running the examples

### producer.ri — clean module decl + pub/priv def (exit 0)

```bash
reify check examples/module_visibility/producer.ri

# Expected: exit 0
# stdout:   All constraints satisfied.
# stderr:   (empty — module decl matches file stem, no diagnostics)
```

### consumer.ri — priv-param access blocked, visible-param resolves (exit 1)

```bash
reify check examples/module_visibility/consumer.ri

# Expected: exit 1
# stdout:   (none)
# stderr:   error: E_PRIV_MEMBER_ACCESS: member 'rated_torque' of structure 'Motor' is private
#            (shaft_diameter is NOT mentioned — it resolved cleanly)
```

### mismatch_variant.ri — module-path mismatch (exit 1)

```bash
reify check examples/module_visibility/mismatch_variant.ri

# Expected: exit 1
# stderr:   error: E_MODULE_PATH_MISMATCH: declared module path 'wrong.path.here'
#                  does not match expected path 'mismatch_variant' (derived from file location)
```

## File layout

```
module_visibility/
├── producer.ri        pub structure def Motor { priv param rated_torque; param shaft_diameter }
├── consumer.ri        imports producer; accesses both members (one priv, one visible)
└── mismatch_variant.ri  module wrong.path.here — intentional path mismatch demo
```

## Note on the bulk examples smoke test

`crates/reify-compiler/tests/examples_smoke.rs` compiles every `examples/**/*.ri`
**single-file** (`compile_with_stdlib`, no module DAG).

- `producer.ri` — self-contained; compiles clean single-file. **Not skipped.**
- `mismatch_variant.ri` — the module-path check is CLI-only (`attach_module_path_diag`);
  single-file compile does not run it, so the file compiles clean. **Not skipped.**
- `consumer.ri` — two reasons it cannot pass the single-file smoke: (1) its
  `import producer` edge cannot be followed (Motor unresolved), and (2) the
  `m.rated_torque` access is a by-design `E_PRIV_MEMBER_ACCESS` Error. It is
  therefore listed in the smoke `SKIP_SET` with a mandatory justification.

The full two-way behaviour is exercised end-to-end by
`crates/reify-cli/tests/cli_module_visibility_example.rs` running the real
`reify` binary.
