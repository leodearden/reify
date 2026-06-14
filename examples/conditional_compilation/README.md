# examples/conditional_compilation

Demonstrates platform-variant cfg selection via `#cfg`-gated imports
(PRD `docs/prds/v0_6/conditional-compilation.md` §2 Slice E / §6 two-way table).

## What the example models

Two sibling modules each define a `Platform` structure differently:

| Module             | Core type   | `Platform` field |
|--------------------|-------------|-----------------|
| `platform_linux`   | `LinuxCore` | `core : LinuxCore` |
| `platform_wasm`    | `WasmCore`  | `core : WasmCore`  |

`main.ri` selects the correct sibling at compile time via `#cfg`-gated imports:

```
module main

#cfg(target = "linux")
import platform_linux

#cfg(target = "wasm")
import platform_wasm

structure def Entry {
    param p : Platform
}
```

The `module` declarations on all three files silence the incidental
`W_MODULE_DECL_MISSING` warning, leaving only the meaningful off-target gating
diagnostic in the output.

`param p : Platform` references `Platform` in **type position** — a type-position
reference is a hard compile error if unresolved, so exit 0 genuinely proves the
platform-correct variant was followed (not a lenient deferral to eval time).

## Running the example

Both targets resolve cleanly — each exits 0 and resolves the platform-correct
`Platform` variant:

```bash
# Linux target — follows platform_linux, gates out platform_wasm
reify check --cfg target=linux examples/conditional_compilation/main.ri

# Expected: exit 0
# stdout:   All constraints satisfied.
# stderr:   warning: import "platform_wasm" not resolved by this entry point
```

```bash
# Wasm target — follows platform_wasm, gates out platform_linux
reify check --cfg target=wasm examples/conditional_compilation/main.ri

# Expected: exit 0
# stdout:   All constraints satisfied.
# stderr:   warning: import "platform_linux" not resolved by this entry point
```

The `warning: import "<module>" not resolved by this entry point` line for the
gated-out module — present for the off-target, absent for the on-target — is the
robust two-way signal that the platform-correct variant was selected and the other
platform module is absent from the DAG.

## File layout

```
conditional_compilation/
├── main.ri            entry — cfg-gated imports + Entry { param p : Platform }
├── platform_linux.ri  linux sibling — LinuxCore + Platform { core : LinuxCore }
└── platform_wasm.ri   wasm sibling  — WasmCore  + Platform { core : WasmCore  }
```

## Note on the bulk examples smoke test

`crates/reify-compiler/tests/examples_smoke.rs` compiles every `examples/**/*.ri`
**single-file** (`compile_with_stdlib`, no cfg DAG).  `main.ri`'s `param p : Platform`
is resolvable only through the cfg-gated import, so single-file it is a hard
`unresolved type` error — `main.ri` is therefore listed in the smoke `SKIP_SET` with
a mandatory justification.

The siblings (`platform_linux.ri`, `platform_wasm.ri`) define their own types, compile
clean single-file, and are **not** skipped.

The full two-way behaviour is exercised end-to-end by
`crates/reify-cli/tests/cli_check_cfg_example.rs` running the real `reify` binary.
