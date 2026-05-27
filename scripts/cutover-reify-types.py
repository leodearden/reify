#!/usr/bin/env python3
"""
cutover-reify-types.py — task η one-shot cutover script.

Atomically retires crates/reify-types/ and the transient AST re-export block in
crates/reify-syntax/src/lib.rs by rewriting all downstream imports to point
directly at reify-core, reify-ast, or reify-ir.

Usage:
    python3 scripts/cutover-reify-types.py [--dry-run]

Run from the workspace root (/path/to/reify).
"""

import re
import os
import sys
import shutil
import subprocess
from pathlib import Path
from collections import defaultdict

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent
DRY_RUN = "--dry-run" in sys.argv

# ─── helpers ──────────────────────────────────────────────────────────────────

def abort(msg: str) -> None:
    print(f"\nABORT: {msg}", file=sys.stderr)
    sys.exit(1)


def crate_dir_for_file(path: Path) -> str | None:
    """Return the crate root dir name (e.g. 'reify-compiler') for a .rs file."""
    try:
        rel = path.relative_to(WORKSPACE_ROOT / "crates")
        return rel.parts[0]
    except ValueError:
        pass
    try:
        path.relative_to(WORKSPACE_ROOT / "gui" / "src-tauri")
        return "gui-src-tauri"
    except ValueError:
        return None


def is_test_file(path: Path) -> bool:
    """True if the file lives under tests/ or benches/ (not src/)."""
    parts = path.parts
    for p in parts:
        if p in ("tests", "benches"):
            return True
    return False


# ─── Step 1 & 2: parse reify-types/src/lib.rs ─────────────────────────────────

def parse_reify_types_lib() -> tuple[dict, dict]:
    """
    Returns:
        module_to_crate: {module_name: crate_name_with_underscores}
        flat_sym_to_crate: {symbol: crate_name_with_underscores}
    """
    lib_path = WORKSPACE_ROOT / "crates" / "reify-types" / "src" / "lib.rs"
    src = lib_path.read_text()

    module_to_crate: dict[str, str] = {}

    # Parse the three pub use <crate>::{...}; module re-export lines.
    # Pattern: pub use reify_xxx::{mod1, mod2, ...};  (possibly multiline)
    module_block_re = re.compile(
        r'pub use (reify_core|reify_ast|reify_ir)::\{([^}]+)\};',
        re.DOTALL,
    )
    # Also handle single-module: pub use reify_ast::ast;
    single_module_re = re.compile(
        r'pub use (reify_core|reify_ast|reify_ir)::([a-z_]+);'
    )

    for m in module_block_re.finditer(src):
        crate = m.group(1)
        mods = [s.strip().rstrip(',') for s in m.group(2).split(',')]
        for mod in mods:
            mod = mod.strip()
            if mod:
                module_to_crate[mod] = crate

    for m in single_module_re.finditer(src):
        crate = m.group(1)
        mod = m.group(2)
        module_to_crate[mod] = crate

    if not module_to_crate:
        abort("module_to_crate map is empty — failed to parse reify-types/src/lib.rs")

    flat_sym_to_crate: dict[str, str] = {}

    # Handle pub use identity::*; specially
    identity_rs = WORKSPACE_ROOT / "crates" / "reify-core" / "src" / "identity.rs"
    ident_src = identity_rs.read_text()
    pub_item_re = re.compile(
        r'^pub\s+(?:struct|enum|fn|const|type|trait|static)\s+([A-Za-z_][A-Za-z0-9_]*)',
        re.MULTILINE,
    )
    for m in pub_item_re.finditer(ident_src):
        flat_sym_to_crate[m.group(1)] = "reify_core"
    # Also pick up pub const strings that are &str (FIELD_ENTITY_PREFIX etc.)
    const_str_re = re.compile(
        r'^pub const ([A-Z_][A-Z0-9_]*):\s*&str', re.MULTILINE
    )
    for m in const_str_re.finditer(ident_src):
        flat_sym_to_crate[m.group(1)] = "reify_core"

    # Parse all other pub use <module>::{...}; and pub use <module>::sym; lines
    flat_multi_re = re.compile(
        r'pub use ([a-z_]+)::\{([^}]+)\};', re.DOTALL
    )
    flat_single_re = re.compile(
        r'pub use ([a-z_]+)::([A-Za-z_][A-Za-z0-9_]*);'
    )

    for m in flat_multi_re.finditer(src):
        mod = m.group(1)
        if mod not in module_to_crate:
            continue
        crate = module_to_crate[mod]
        syms_raw = m.group(2)
        for part in syms_raw.split(','):
            part = part.strip().rstrip(',').strip()
            if not part:
                continue
            # strip leading comments that got caught in multiline
            part = re.sub(r'//[^\n]*', '', part).strip()
            if not part:
                continue
            # handle "sym" or "sym as Alias" — we want the original sym name
            sym = part.split()[0]
            if sym:
                flat_sym_to_crate[sym] = crate

    for m in flat_single_re.finditer(src):
        mod = m.group(1)
        if mod not in module_to_crate:
            continue
        crate = module_to_crate[mod]
        sym = m.group(2)
        if sym and sym not in ('diagnostics', 'dimension', 'hash', 'identity',
                                'primitives', 'source_location', 'spanned_ident',
                                'ty', 'ast', 'annotation', 'boundary_attachment',
                                'constraint', 'expr', 'geometry', 'kernel_validation',
                                'node_traits', 'persistent', 'provenance', 'sampled',
                                'structure_registry', 'traits', 'value', 'warm',
                                'warm_registry'):
            flat_sym_to_crate[sym] = crate

    if not flat_sym_to_crate:
        abort("flat_sym_to_crate map is empty — failed to parse flat re-exports")

    return module_to_crate, flat_sym_to_crate


# ─── Step 3: parse reify-syntax/src/lib.rs for transient AST set ─────────────

def parse_syntax_ast_set() -> set[str]:
    lib_path = WORKSPACE_ROOT / "crates" / "reify-syntax" / "src" / "lib.rs"
    src = lib_path.read_text()

    # Find the transient re-exports block
    marker = "// Transient re-exports"
    idx = src.find(marker)
    if idx == -1:
        abort("Could not find '// Transient re-exports' in reify-syntax/src/lib.rs")

    # Find the closing }; after the pub use reify_ast::{
    block_start = src.find("pub use reify_ast::{", idx)
    if block_start == -1:
        abort("Could not find 'pub use reify_ast::{' in transient block")

    block_end = src.find("};", block_start)
    if block_end == -1:
        abort("Could not find closing '}; ' for transient reify_ast block")

    block = src[block_start:block_end + 2]

    # Extract symbol names from inside { ... }
    inner = re.search(r'pub use reify_ast::\{([^}]+)\}', block, re.DOTALL)
    if not inner:
        abort("Failed to extract symbols from transient reify_ast block")

    syms: set[str] = set()
    for part in inner.group(1).split(','):
        part = part.strip().rstrip(',').strip()
        part = re.sub(r'//[^\n]*', '', part).strip()
        if part:
            syms.add(part.split()[0])  # handle "sym" or "sym as Alias"

    # Ensure parse and parse_with_prelude_enums are NOT in the set
    syms.discard('parse')
    syms.discard('parse_with_prelude_enums')

    if not syms:
        abort("syntax_ast_set is empty — failed to parse transient block")

    return syms


# ─── Comment-aware line processor ─────────────────────────────────────────────

def apply_bare_path_subs_to_content(
    content: str,
    module_to_crate: dict,
    flat_sym_to_crate: dict,
    syntax_ast_set: set,
    file_path: Path,
) -> str:
    """Apply 4f, 4g, 4j substitutions line-by-line, skipping comment lines."""
    lines = content.split('\n')
    out_lines = []
    for line in lines:
        stripped = line.lstrip()
        if stripped.startswith('///') or stripped.startswith('//!') or stripped.startswith('//'):
            out_lines.append(line)
            continue

        # Split off inline comment
        code_part, comment_part = split_inline_comment(line)

        # 4f: reify_types::<module>:: → <crate>::<module>::
        def repl_module(m):
            mod = m.group(1)
            if mod in module_to_crate:
                return f"{module_to_crate[mod]}::{mod}::"
            return m.group(0)

        code_part = re.sub(r'\breify_types::([a-z_]+)::', repl_module, code_part)

        # 4g: reify_types::SYM (flat paths in code body, PascalCase or ALL_CAPS)
        def repl_flat(m):
            sym = m.group(1)
            if sym in flat_sym_to_crate:
                return f"{flat_sym_to_crate[sym]}::{sym}"
            # Unknown symbol — leave as-is (use statements will abort, bare refs warn)
            return m.group(0)

        code_part = re.sub(r'\breify_types::([A-Z_][A-Za-z0-9_]*)', repl_flat, code_part)

        # 4j: reify_syntax::Sym → reify_ast::Sym (for Sym in syntax_ast_set)
        def repl_syntax(m):
            sym = m.group(1)
            if sym in syntax_ast_set:
                return f"reify_ast::{sym}"
            return m.group(0)

        code_part = re.sub(r'\breify_syntax::([A-Z][A-Za-z0-9_]*)', repl_syntax, code_part)

        out_lines.append(code_part + comment_part)

    return '\n'.join(out_lines)


def split_inline_comment(line: str) -> tuple[str, str]:
    """Split a line into (code_part, '// comment') avoiding false positives in strings."""
    # Simple heuristic: find // not preceded by : (to avoid https://)
    idx = line.find('//')
    while idx != -1:
        if idx == 0 or line[idx - 1] != ':':
            return line[:idx], line[idx:]
        idx = line.find('//', idx + 2)
    return line, ''


# ─── Use-statement rewriter ───────────────────────────────────────────────────

def normalize_multiline_use_blocks(content: str) -> str:
    """
    Collapse multi-line `use reify_types::{...};` and `use reify_syntax::{...};`
    blocks into single lines. Preserves leading indentation of the first line.
    """
    result = []
    lines = content.split('\n')
    i = 0
    while i < len(lines):
        line = lines[i]
        # Match start of a multi-line use reify_types or reify_syntax block
        m = re.match(r'^(\s*)(pub\s+)?use (reify_types|reify_syntax)::\{', line)
        if m and '}' not in line:
            collected = [line]
            j = i + 1
            while j < len(lines):
                collected.append(lines[j])
                if re.search(r'\};', lines[j]):
                    break
                j += 1
            # Join into a single line
            joined = ' '.join(l.strip() for l in collected)
            # Normalize spaces around braces and commas
            joined = re.sub(r'\s+', ' ', joined)
            # Restore leading indentation
            indent = m.group(1)
            joined = indent + joined.lstrip()
            result.append(joined)
            i = j + 1
        else:
            result.append(line)
            i += 1
    return '\n'.join(result)


def parse_use_entries(entries_str: str) -> list[str]:
    """Parse comma-separated import entries from inside braces."""
    entries = []
    for part in entries_str.split(','):
        part = part.strip()
        if part:
            entries.append(part)
    return entries


def rewrite_use_reify_types_line(
    line: str,
    module_to_crate: dict,
    flat_sym_to_crate: dict,
    file_path: Path,
) -> str:
    """
    Rewrite a single-line `use reify_types::{...};` or `use reify_types::Sym;` statement.
    Returns the replacement string (may be multiple lines joined by \\n).
    """
    stripped = line.lstrip()
    indent = line[:len(line) - len(stripped)]

    # 4b: glob import
    glob_re = re.compile(r'^(pub\s+)?use reify_types::\*;')
    m = glob_re.match(stripped)
    if m:
        pub = m.group(1) or ''
        return f"{indent}{pub}use reify_core::*;\n{indent}{pub}use reify_ir::*;"

    # 4e: module-path import: use reify_types::<module>::...;
    modpath_re = re.compile(r'^(pub\s+)?use reify_types::([a-z_]+)::(.+);$')
    m = modpath_re.match(stripped)
    if m:
        pub = m.group(1) or ''
        mod = m.group(2)
        rest = m.group(3)
        if mod in module_to_crate:
            crate = module_to_crate[mod]
            return f"{indent}{pub}use {crate}::{mod}::{rest};"
        else:
            abort(f"Unknown module '{mod}' in reify_types path in {file_path}")

    # 4d: single flat symbol: use reify_types::Sym; or use reify_types::Sym as Alias;
    single_re = re.compile(r'^(pub\s+)?use reify_types::([A-Za-z_][A-Za-z0-9_]*)(\s+as\s+\w+)?;$')
    m = single_re.match(stripped)
    if m:
        pub = m.group(1) or ''
        sym = m.group(2)
        alias = m.group(3) or ''
        if sym in flat_sym_to_crate:
            crate = flat_sym_to_crate[sym]
            return f"{indent}{pub}use {crate}::{sym}{alias};"
        elif sym in module_to_crate:
            # It's a module re-export used as a single import
            crate = module_to_crate[sym]
            return f"{indent}{pub}use {crate}::{sym}{alias};"
        else:
            abort(f"Unknown symbol '{sym}' in `use reify_types::{sym}` in {file_path}")

    # 4c: multi-symbol: use reify_types::{A, B as X, module::Sym, ...};
    multi_re = re.compile(r'^(pub\s+)?use reify_types::\{([^}]+)\};$')
    m = multi_re.match(stripped)
    if m:
        pub = m.group(1) or ''
        entries_str = m.group(2)
        entries = parse_use_entries(entries_str)

        # Group by crate
        by_crate: dict[str, list[str]] = defaultdict(list)
        order: list[str] = []  # track crate insertion order

        for entry in entries:
            entry = entry.strip()
            if not entry:
                continue

            # Check if it's a module-path entry: module::Sym or module::Sym as Alias
            modentry_re = re.compile(r'^([a-z_]+)::([A-Za-z_][A-Za-z0-9_]*)(.*)$')
            me = modentry_re.match(entry)
            if me:
                mod = me.group(1)
                if mod in module_to_crate:
                    crate = module_to_crate[mod]
                    if crate not in by_crate:
                        order.append(crate)
                    by_crate[crate].append(entry)
                    continue
                else:
                    abort(f"Unknown module '{mod}' in use reify_types entry '{entry}' in {file_path}")

            # Flat symbol entry: Sym or Sym as Alias
            sym = entry.split()[0]
            if sym in flat_sym_to_crate:
                crate = flat_sym_to_crate[sym]
                if crate not in by_crate:
                    order.append(crate)
                by_crate[crate].append(entry)
            elif sym in module_to_crate:
                crate = module_to_crate[sym]
                if crate not in by_crate:
                    order.append(crate)
                by_crate[crate].append(entry)
            else:
                abort(f"Unknown symbol '{sym}' in use reify_types block in {file_path}")

        # Emit one use per crate (sorted by crate name)
        result_lines = []
        for crate in sorted(by_crate.keys()):
            syms = by_crate[crate]
            if len(syms) == 1:
                result_lines.append(f"{indent}{pub}use {crate}::{syms[0]};")
            else:
                syms_str = ', '.join(syms)
                result_lines.append(f"{indent}{pub}use {crate}::{{{syms_str}}};")
        return '\n'.join(result_lines)

    # Fallback: return unchanged
    return line


def rewrite_use_reify_syntax_line(
    line: str,
    syntax_ast_set: set,
) -> str:
    """
    Rewrite `use reify_syntax::{...};` or `use reify_syntax::Sym;` for AST type splitting.
    """
    stripped = line.lstrip()
    indent = line[:len(line) - len(stripped)]

    # 4i: single import
    single_re = re.compile(r'^(pub\s+)?use reify_syntax::([A-Za-z_][A-Za-z0-9_]*)(\s+as\s+\w+)?;$')
    m = single_re.match(stripped)
    if m:
        pub = m.group(1) or ''
        sym = m.group(2)
        alias = m.group(3) or ''
        if sym in syntax_ast_set:
            return f"{indent}{pub}use reify_ast::{sym}{alias};"
        return line  # leave unchanged

    # 4h: multi-symbol import
    multi_re = re.compile(r'^(pub\s+)?use reify_syntax::\{([^}]+)\};$')
    m = multi_re.match(stripped)
    if m:
        pub = m.group(1) or ''
        entries_str = m.group(2)
        entries = parse_use_entries(entries_str)

        ast_syms = []
        syntax_syms = []

        for entry in entries:
            sym = entry.strip().split()[0]
            if sym in syntax_ast_set:
                ast_syms.append(entry.strip())
            else:
                syntax_syms.append(entry.strip())

        if not ast_syms:
            return line  # no change needed

        result_lines = []
        if ast_syms:
            if len(ast_syms) == 1:
                result_lines.append(f"{indent}{pub}use reify_ast::{ast_syms[0]};")
            else:
                result_lines.append(f"{indent}{pub}use reify_ast::{{{', '.join(ast_syms)}}};")
        if syntax_syms:
            if len(syntax_syms) == 1:
                result_lines.append(f"{indent}{pub}use reify_syntax::{syntax_syms[0]};")
            else:
                result_lines.append(f"{indent}{pub}use reify_syntax::{{{', '.join(syntax_syms)}}};")
        return '\n'.join(result_lines)

    return line


# ─── Step 4: file walk and rewriting ─────────────────────────────────────────

def rewrite_rs_file(
    path: Path,
    module_to_crate: dict,
    flat_sym_to_crate: dict,
    syntax_ast_set: set,
) -> tuple[str | None, set[str], set[str]]:
    """
    Returns (new_content_or_None_if_unchanged, deps_introduced, dev_deps_introduced).
    Aborts if an unknown symbol is encountered.
    """
    original = path.read_text()
    content = original

    test_file = is_test_file(path)

    # Step 4a: normalize multi-line use reify_types blocks
    content = normalize_multiline_use_blocks(content)

    # Rewrite use reify_types::... statements line-by-line
    lines = content.split('\n')
    new_lines = []
    for line in lines:
        stripped = line.lstrip()
        # Skip doc comments
        if stripped.startswith('///') or stripped.startswith('//!') or stripped.startswith('//'):
            new_lines.append(line)
            continue

        if re.search(r'\buse reify_types::', line):
            rewritten = rewrite_use_reify_types_line(
                line, module_to_crate, flat_sym_to_crate, path
            )
            new_lines.append(rewritten)
        elif re.search(r'\buse reify_syntax::', line):
            rewritten = rewrite_use_reify_syntax_line(line, syntax_ast_set)
            new_lines.append(rewritten)
        else:
            new_lines.append(line)

    content = '\n'.join(new_lines)

    # Steps 4f, 4g, 4j: bare path substitutions (comment-aware)
    content = apply_bare_path_subs_to_content(
        content, module_to_crate, flat_sym_to_crate, syntax_ast_set, path
    )

    if content == original:
        return None, set(), set()

    # Determine which new crates were introduced by looking at the new content
    introduced = set()
    for crate in ('reify_core', 'reify_ast', 'reify_ir'):
        if re.search(rf'\buse {crate}::', content) or re.search(rf'\b{crate}::', content):
            introduced.add(crate)

    if test_file:
        return content, set(), introduced
    else:
        return content, introduced, set()


def collect_rs_files() -> list[Path]:
    """Collect all .rs files in scope."""
    paths = []
    for root, dirs, files in os.walk(WORKSPACE_ROOT / "crates"):
        # Exclude reify-types crate
        dirs[:] = [d for d in dirs if not (
            Path(root) == WORKSPACE_ROOT / "crates" and d == "reify-types"
        )]
        for f in files:
            if f.endswith('.rs'):
                paths.append(Path(root) / f)

    tauri_src = WORKSPACE_ROOT / "gui" / "src-tauri" / "src"
    if tauri_src.exists():
        for root, dirs, files in os.walk(tauri_src):
            for f in files:
                if f.endswith('.rs'):
                    paths.append(Path(root) / f)

    tauri_tests = WORKSPACE_ROOT / "gui" / "src-tauri" / "tests"
    if tauri_tests.exists():
        for root, dirs, files in os.walk(tauri_tests):
            for f in files:
                if f.endswith('.rs'):
                    paths.append(Path(root) / f)

    return paths


# ─── Step 5: Cargo.toml updates ───────────────────────────────────────────────

def crate_name_from_dir(crate_dir: str) -> str | None:
    """Return Cargo package name from crate directory name."""
    # For gui-src-tauri we need to read the Cargo.toml
    if crate_dir == "gui-src-tauri":
        cargo = WORKSPACE_ROOT / "gui" / "src-tauri" / "Cargo.toml"
    else:
        cargo = WORKSPACE_ROOT / "crates" / crate_dir / "Cargo.toml"
    if not cargo.exists():
        return None
    content = cargo.read_text()
    m = re.search(r'^name\s*=\s*"([^"]+)"', content, re.MULTILINE)
    return m.group(1) if m else None


def find_crate_for_rs_path(rs_path: Path) -> str | None:
    """Find the Cargo.toml for a given .rs file path."""
    p = rs_path.parent
    while p != WORKSPACE_ROOT and p != p.parent:
        if (p / "Cargo.toml").exists():
            return str(p / "Cargo.toml")
        p = p.parent
    return None


def scan_crate_for_new_deps(
    crate_root: Path,
) -> tuple[set[str], set[str]]:
    """
    Scan src/ and tests/ dirs of a crate for new reify_core/ast/ir uses.
    Returns (prod_deps, dev_deps) sets of crate names (with hyphens).
    """
    prod_deps: set[str] = set()
    dev_deps: set[str] = set()

    src_dir = crate_root / "src"
    tests_dir = crate_root / "tests"
    benches_dir = crate_root / "benches"

    def scan_dir(d: Path, dep_set: set):
        if not d.exists():
            return
        for root, _, files in os.walk(d):
            for f in files:
                if f.endswith('.rs'):
                    fpath = Path(root) / f
                    try:
                        txt = fpath.read_text()
                    except Exception:
                        continue
                    for crate in ('reify_core', 'reify_ast', 'reify_ir'):
                        if re.search(rf'\buse {crate}::', txt) or re.search(rf'\b{crate}::', txt):
                            dep_set.add(crate.replace('_', '-'))

    scan_dir(src_dir, prod_deps)
    scan_dir(tests_dir, dev_deps)
    scan_dir(benches_dir, dev_deps)

    return prod_deps, dev_deps


def existing_deps_in_cargo(content: str, section: str) -> set[str]:
    """Return set of dep names already present in a given [section]."""
    deps = set()
    in_section = False
    for line in content.split('\n'):
        stripped = line.strip()
        if stripped.startswith('['):
            in_section = stripped == f'[{section}]'
            continue
        if in_section:
            m = re.match(r'^([\w-]+)\s*[.=]', stripped)
            if m:
                deps.add(m.group(1))
    return deps


def has_features_tag(content: str, dep_name: str) -> bool:
    """Check if a dep entry in Cargo.toml has features = [...]."""
    m = re.search(
        rf'^{re.escape(dep_name)}\s*=\s*\{{[^}}]*features\s*=',
        content, re.MULTILINE
    )
    return m is not None


def update_cargo_toml(
    cargo_path: Path,
    prod_deps_needed: set[str],
    dev_deps_needed: set[str],
    has_features: bool,
    dry_run: bool,
) -> bool:
    """
    Remove reify-types entry and add needed replacement deps.
    Returns True if file was modified.
    """
    content = cargo_path.read_text()
    if 'reify-types' not in content:
        return False

    original = content

    is_syntax = cargo_path.parent.name == "reify-syntax"

    # Remove the reify-types line(s)
    if is_syntax:
        # Remove the comment block + reify-types line in [dev-dependencies]
        # Pattern: "# reify-types kept..." through "reify-types.workspace = true"
        comment_block_re = re.compile(
            r'# reify-types kept[^\n]*\n'
            r'(?:#[^\n]*\n)*'
            r'# REMOVE-AT[^\n]*\n'
            r'reify-types[^\n]*\n',
            re.MULTILINE,
        )
        content = comment_block_re.sub('', content)
    else:
        # Remove any reify-types line
        content = re.sub(r'^reify-types[^\n]*\n', '', content, flags=re.MULTILINE)
        content = re.sub(r'^reify-types\s*=\s*\{[^}]*\}\s*\n', '', content, flags=re.MULTILINE)

    # Build the dep string template
    def dep_str(crate_name: str) -> str:
        if has_features:
            return f'{crate_name} = {{ workspace = true, features = ["serde"] }}'
        return f'{crate_name}.workspace = true'

    # Add prod deps to [dependencies]
    if prod_deps_needed:
        existing_prod = existing_deps_in_cargo(content, 'dependencies')
        new_prod = sorted(prod_deps_needed - existing_prod)
        if new_prod:
            # Find the [dependencies] section and append before its end
            dep_lines = '\n'.join(dep_str(c) for c in new_prod)
            # Insert after [dependencies] header line
            content = re.sub(
                r'(\[dependencies\]\n)',
                r'\1' + dep_lines + '\n',
                content,
                count=1,
            )

    # Add dev deps to [dev-dependencies]
    if dev_deps_needed:
        existing_dev = existing_deps_in_cargo(content, 'dev-dependencies')
        new_dev = sorted(dev_deps_needed - existing_dev)
        if new_dev:
            dev_lines = '\n'.join(dep_str(c) for c in new_dev)
            if '[dev-dependencies]' in content:
                content = re.sub(
                    r'(\[dev-dependencies\]\n)',
                    r'\1' + dev_lines + '\n',
                    content,
                    count=1,
                )
            else:
                content += f'\n[dev-dependencies]\n{dev_lines}\n'

    # Clean up any double blank lines left by removal
    content = re.sub(r'\n{3,}', '\n\n', content)

    if content == original:
        return False

    if dry_run:
        print(f"  [DRY-RUN] would update {cargo_path.relative_to(WORKSPACE_ROOT)}")
    else:
        cargo_path.write_text(content)

    return True


# ─── Step 6: workspace Cargo.toml ────────────────────────────────────────────

def update_workspace_cargo_toml(dry_run: bool) -> None:
    cargo_path = WORKSPACE_ROOT / "Cargo.toml"
    content = cargo_path.read_text()
    original = content

    # Remove members entry
    content = re.sub(r'\s*"crates/reify-types",?\n', '\n', content)
    content = re.sub(r',\s*"crates/reify-types"', '', content)

    # Remove workspace.dependencies entry
    content = re.sub(r'^reify-types\s*=\s*\{[^}]*\}\s*\n', '', content, flags=re.MULTILINE)
    content = re.sub(r'^reify-types\s*=\s*[^\n]*\n', '', content, flags=re.MULTILINE)

    # Clean up double blank lines
    content = re.sub(r'\n{3,}', '\n\n', content)

    if content == original:
        print("  (workspace Cargo.toml: no changes needed)")
        return

    if dry_run:
        print("  [DRY-RUN] would update Cargo.toml (workspace root)")
    else:
        cargo_path.write_text(content)
        print("  updated Cargo.toml (workspace root)")


# ─── Step 7: remove transient block from reify-syntax/src/lib.rs ──────────────

def remove_syntax_transient_block(dry_run: bool) -> None:
    lib_path = WORKSPACE_ROOT / "crates" / "reify-syntax" / "src" / "lib.rs"
    content = lib_path.read_text()
    original = content

    # Find: "// Transient re-exports — retired by task η." through closing "};" plus optional blank line
    marker = "// Transient re-exports"
    idx = content.find(marker)
    if idx == -1:
        print("  (reify-syntax/src/lib.rs: transient block already removed)")
        return

    # Find the line start
    line_start = content.rfind('\n', 0, idx)
    if line_start == -1:
        line_start = 0
    else:
        line_start += 1  # skip the \n itself

    # Find the closing }; of the pub use reify_ast block
    block_end_idx = content.find('};', idx)
    if block_end_idx == -1:
        abort("Could not find closing '};' for transient block in reify-syntax/src/lib.rs")

    # Include the "};\n" and any immediately following blank line
    end_pos = block_end_idx + 2  # past "};"
    if end_pos < len(content) and content[end_pos] == '\n':
        end_pos += 1
    if end_pos < len(content) and content[end_pos] == '\n':
        end_pos += 1  # skip a blank line too

    content = content[:line_start] + content[end_pos:]

    if content == original:
        print("  (reify-syntax/src/lib.rs: no changes needed)")
        return

    if dry_run:
        print("  [DRY-RUN] would update crates/reify-syntax/src/lib.rs (remove transient block)")
    else:
        lib_path.write_text(content)
        print("  updated crates/reify-syntax/src/lib.rs (removed transient block)")


# ─── Main ─────────────────────────────────────────────────────────────────────

def main() -> None:
    if DRY_RUN:
        print("=== DRY RUN MODE — no files will be written ===\n")

    # ── 1 & 2: build symbol maps ──────────────────────────────────────────────
    print("Building module and symbol maps from reify-types/src/lib.rs...")
    module_to_crate, flat_sym_to_crate = parse_reify_types_lib()
    print(f"  {len(module_to_crate)} modules, {len(flat_sym_to_crate)} flat symbols")

    # ── 3: syntax AST set ─────────────────────────────────────────────────────
    print("Parsing transient AST set from reify-syntax/src/lib.rs...")
    syntax_ast_set = parse_syntax_ast_set()
    print(f"  {len(syntax_ast_set)} AST symbols in transient set")

    # ── 4: rewrite .rs files ──────────────────────────────────────────────────
    print("\nWalking .rs files...")
    rs_files = collect_rs_files()
    print(f"  Found {len(rs_files)} .rs files to process")

    files_rewritten = 0
    # Track per cargo-toml which deps are newly needed
    # cargo_toml_path -> (prod_deps_set, dev_deps_set, has_features_flag)
    cargo_deps: dict[str, tuple[set, set, bool]] = {}

    for rs_path in sorted(rs_files):
        new_content, prod_deps, dev_deps = rewrite_rs_file(
            rs_path, module_to_crate, flat_sym_to_crate, syntax_ast_set
        )
        if new_content is None:
            continue

        rel = rs_path.relative_to(WORKSPACE_ROOT)
        print(f"  rewriting {rel}")
        files_rewritten += 1

        if not DRY_RUN:
            rs_path.write_text(new_content)

        # Track which cargo.toml this belongs to
        cargo_toml_str = find_crate_for_rs_path(rs_path)
        if cargo_toml_str:
            if cargo_toml_str not in cargo_deps:
                cargo_deps[cargo_toml_str] = (set(), set(), False)
            existing_prod, existing_dev, has_feat = cargo_deps[cargo_toml_str]
            existing_prod.update(prod_deps)
            existing_dev.update(dev_deps)
            cargo_deps[cargo_toml_str] = (existing_prod, existing_dev, has_feat)

    # ── 5: Cargo.toml updates ─────────────────────────────────────────────────
    print("\nUpdating Cargo.toml files...")

    cargo_tomls_updated = 0

    # Find all Cargo.tomls that reference reify-types
    cargo_toml_paths: list[Path] = []
    for crate_dir in os.listdir(WORKSPACE_ROOT / "crates"):
        if crate_dir == "reify-types":
            continue
        cargo = WORKSPACE_ROOT / "crates" / crate_dir / "Cargo.toml"
        if cargo.exists():
            cargo_toml_paths.append(cargo)

    tauri_cargo = WORKSPACE_ROOT / "gui" / "src-tauri" / "Cargo.toml"
    if tauri_cargo.exists():
        cargo_toml_paths.append(tauri_cargo)

    for cargo_path in sorted(cargo_toml_paths):
        content = cargo_path.read_text()
        if 'reify-types' not in content:
            continue

        # Determine which new crates are needed by scanning the rewritten .rs files
        if cargo_path.parent.name == "src-tauri":
            crate_root = cargo_path.parent
        else:
            crate_root = cargo_path.parent

        prod_needed, dev_needed = scan_crate_for_new_deps(crate_root)

        has_features = has_features_tag(content, 'reify-types')

        modified = update_cargo_toml(
            cargo_path, prod_needed, dev_needed, has_features, DRY_RUN
        )
        if modified:
            rel = cargo_path.relative_to(WORKSPACE_ROOT)
            if not DRY_RUN:
                print(f"  updated {rel}")
            cargo_tomls_updated += 1

    # ── 6: workspace Cargo.toml ───────────────────────────────────────────────
    print("\nUpdating workspace Cargo.toml...")
    update_workspace_cargo_toml(DRY_RUN)

    # ── 7: remove transient block from reify-syntax ───────────────────────────
    print("\nRemoving transient block from reify-syntax/src/lib.rs...")
    remove_syntax_transient_block(DRY_RUN)

    # ── 8: delete reify-types crate ───────────────────────────────────────────
    types_crate = WORKSPACE_ROOT / "crates" / "reify-types"
    if DRY_RUN:
        print(f"\n  [DRY-RUN] would delete {types_crate}")
    else:
        print(f"\nDeleting {types_crate}...")
        shutil.rmtree(types_crate)
        print("  done")

    # ── 9: regenerate lockfile ────────────────────────────────────────────────
    if DRY_RUN:
        print("\n  [DRY-RUN] would run: cargo generate-lockfile")
    else:
        print("\nRegenerating Cargo.lock...")
        result = subprocess.run(
            ['cargo', 'generate-lockfile'],
            cwd=WORKSPACE_ROOT,
            check=False,
        )
        if result.returncode != 0:
            abort("cargo generate-lockfile failed — review errors above")
        print("  done")

    # ── Summary ───────────────────────────────────────────────────────────────
    print(f"\n{'DRY-RUN ' if DRY_RUN else ''}Summary: "
          f"{files_rewritten} files rewritten, "
          f"{cargo_tomls_updated} Cargo.tomls updated")

    if DRY_RUN:
        print("\nDry run complete — no files were written.")


if __name__ == '__main__':
    main()
