# Capability manifest — cargo-native-harness-layout

PRD: `docs/prds/v0_6/cargo-native-harness-layout.md`. Machine-readable twin:
`docs/prds/v0_6/cargo-native-harness-layout.capability-manifest.yaml`, stamped by `commit_planning`. Evidence was
measured on main `3f9015b3b5` (2026-10-18) by the session's seats:

- S1: inventory;
- S2/S4: the measure and dep-info, the latter reconciled by the lead;
- S3: the step-B substrate probe, real cargo 1.96 / nextest 0.9.136 runs in a scratch workspace.

Every mechanical delivered check below FAILs on main and PASSes once its producer lands.

**Decompose verification.** `scripts/prd-decompose-verify.mjs` was not run. Its probe vectors (tree-sitter
parse, `reify check`, `reify eval`, value probe) observe `.ri`-language premises, and no leaf here asserts one. This
stream's substrate premises are cargo and rustc behaviours. S3 probed them directly (§2.4–§2.5 of the PRD), and
the greps and paths below bind them.

Verdict key: **PASS** means the binding held at authoring time. "producer-self" means the capability is the
leaf's own deliverable.

## α — dual-shape guard, contract doc's layout half

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| guard-blind-to-main-rs-today | root globs `tests/harness_*.rs`; unit walk uses `${root%.rs}`; `main.rs` absent from the lib | PASS | — |
| contract-doc-upstream | producer: #8700, upstream | PASS | — |
| lib-learns-main-rs | producer-self; no delivered check, because ε₁/ε₂ delete those functions (a check there would flip back); BT2 is the cover | PASS | — |
| subtopic-split-rule | producer-self (R3) | PASS | grep `harness_<topic>_<subtopic>` present in `docs/contributor/harness-layout-contract.md` |

## β₁–β₅ — step A per crate

| leaf | capability | evidence | verdict | delivered check |
|---|---|---|---|---|
| β₁ | selector-parity | S3: target name, binary id, test names and binary hash byte-identical across the move | PASS | — |
| β₁ | escaping-include-form | S3 M3: `#[path = "../common/mod.rs"]` compiles from `main.rs`; harness_cli has 1 bare `mod common;` | PASS | — |
| β₁ | root-moved / old-root-gone | producer-self | PASS | path `crates/reify-cli/tests/harness_cli/main.rs` present; `…/harness_cli.rs` absent |
| β₂ | root-moved / old-root-gone | producer-self (harness_syntax_lowering, since γ may split harness_syntax) | PASS | path `crates/reify-syntax/tests/harness_syntax_lowering/main.rs` present; `.rs` absent |
| β₃ | root-moved / old-root-gone | producer-self; 1 bare `mod common;` | PASS | path `crates/reify-kernel-occt/tests/harness_occt_measurement/main.rs` present; `.rs` absent |
| β₄ | root-moved / old-root-gone | producer-self; 12 `common/mod.rs` includes | PASS | path `crates/reify-compiler/tests/harness_units/main.rs` present; `.rs` absent |
| β₅ | heavy-filter-atom-test-names-root | `test_heavy_filter_atoms.sh` asserts the old root path | PASS | — |
| β₅ | root-moved | producer-self | PASS | path `crates/reify-eval/tests/harness_engine/main.rs` present |
| β₅ | escalation-cites-gone | `esc-5056-11` in 2 reify-eval roots, 0 members | PASS | grep `esc-5056-11` absent in `crates/reify-eval/tests` |

## γ — harness_syntax subtopic split

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| split-needed | harness_syntax 12,777 code lines > 12,000 (R2) | PASS | — |
| split-achievable | 55 members, 12,620 code lines; the declarations theme alone > 2,000 | PASS | — |
| units-under-advisory-line | behavioural | PASS | manual: the subtopic name is chosen in-leaf; ε₁'s cap check is the mechanical cover |

## δ — step C

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| no-test-tables-today | no `autotests` / `[[test]]` anywhere | PASS | — |
| standalones-match-baseline | reify-eval 220 = 218 rows + 2 override stems | PASS | — |
| planner-upstream | producers #9112 (ζ) and #9097 (β), upstream | PASS | — |
| fail-open-today | `_derive_added_from_git` failure → PASS `added=0` | PASS | — |
| autotests-off | producer-self | PASS | grep `^autotests = false` present in `crates/reify-eval/Cargo.toml` |
| baseline-retired | producer-self | PASS | path `tests/infra/harness-layout-baseline.manifest` absent |
| diff-gate-retired | producer-self | PASS | path `scripts/check-harness-baseline-registration.sh` absent |
| registration-fails-closed | producer-self | PASS | grep `def registration_violations` present in `scripts/harness_layout.py` |
| registration-step-emitted | producer-self (S18 seam, after ζ) | PASS | grep `check-registration` present in `scripts/reify_verify` |
| registered-check | producer-self (PRD §2.6 1) | PASS | grep `def registered_violations` present in `scripts/harness_layout.py` |

## ε₁ — measure, freshness, undeclared member (B folded; high, gate #9171)

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| dep-info-present-and-agreeing | 4–17 `.d` per unit; the two newest agree 45/45; clippy `.d` = test `.d` on `.rs` | PASS | — |
| dep-info-fresh-after-nextest | CORRECTED (R8): test and lint run as concurrent processes and the warm-lane relink makes seeded `.d` stale; the debug nextest pass rebuilds every in-scope test target, so a planner step right after it reads dep-info; the engine still proves freshness (D10) | PASS | — |
| one-lexer | `RUST_LEXER_AWK`; the lead's spot-check shows string interiors lex empty | PASS | — |
| cap-floor | after γ, max 11,215 code lines (93%) < 12,000; in-leaf split if one crosses | PASS | — |
| lexer-hoisted | producer-self | PASS | path `scripts/rust_lexed_source.py` present |
| awk-resolver-deleted | producer-self (fk-b904603b81f2) | PASS | grep `_harness_layout_mod_decls` absent in `tests/infra/harness-layout-lib.sh` |
| post-nextest-step | producer-self (R8) | PASS | grep `check-dep-info` present in `scripts/reify_verify` |
| code-line-cap | producer-self (R2; the number lives only in the engine) | PASS | grep `^CAP_CODE_LINES = ` present in `scripts/harness_layout.py` |
| undeclared-member-check | producer-self (R1) | PASS | grep `def dep_info_violations` present in `scripts/harness_layout.py` |

## ε₂ — stale selectors, bash guard deleted (high, gate #9242)

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| live-selector-atoms | 10 nextest.toml + 8 heavy-filter `binary()` atoms | PASS | — |
| bash-test-deleted | producer-self (fk-53a9afc0d2b9) | PASS | path `tests/infra/test_harness_kloc_cap.sh` absent |
| stale-selector-check | producer-self | PASS | grep `def stale_selector_violations` present in `scripts/harness_layout.py` |

## Ω — PRD close

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| terminal-stamp | producer-self | PASS | grep `^\*\*Status:\*\* SHIPPED` present in the PRD |
