//! The CLI's help surface: one entry per subcommand, rendered as the top-level list, as
//! `reify <cmd> --help`, and as the banner a command prints on a usage error.

use std::io::Write;

use crate::cache::CACHE_USAGE;
use crate::dev::DEV_USAGE;

pub(crate) const CHECK_USAGE: &str = "Usage: reify check [--strict] [--verbose] [--purpose <name>=<binding>]... [--cfg <key=value|flag>]... <file>";

pub(crate) const TEST_USAGE: &str = "Usage: reify test <file>";

pub(crate) const BUILD_USAGE: &str = "Usage: reify build <file.ri> [-o <output>] [--out-dir <dir>] [--verbose]\n  \
    With -o:    write a single file in that format (imperative).\n  \
    Without -o: every `: Output` occurrence in the design drives its own file\n              \
    (declarative); each relative path resolves against the .ri file's\n              \
    directory, or against --out-dir when given.";

pub(crate) const EVAL_USAGE: &str =
    "Usage: reify eval [--explain-undef] [--verbose] [--cache-dir <path>] <file>";

pub(crate) const REPORT_USAGE: &str = "Usage: reify report --bom <file>";

pub(crate) const DOC_USAGE: &str = "Usage: reify doc <input.ri> [-o <path>] [--format html|markdown|json] [--split] [--compact]\n       reify doc --stdlib --out <dir>";

const LSP_USAGE: &str = "Usage: reify lsp";

pub(crate) const GUI_USAGE: &str = "Usage: reify gui [--debug] <file>";

pub(crate) const EXPLAIN_USAGE: &str = "Usage: reify explain <file>";

pub(crate) struct SubcommandHelp {
    name: &'static str,
    /// The short form `reify --help` lists; `usage` is the full banner.
    synopsis: &'static str,
    usage: &'static str,
    summary: &'static str,
    details: &'static str,
}

/// Listed in the order `reify --help` prints them.
const SUBCOMMANDS: &[SubcommandHelp] = &[
    SubcommandHelp {
        name: "check",
        synopsis: "check [--strict] <file>",
        usage: CHECK_USAGE,
        summary: "Resolve `auto` parameters and check every constraint",
        details: "Prints one OK / VIOLATED / INDETERMINATE line per constraint.\n\
            \n\
            Flags:\n  \
            --strict                    Also fail on any INDETERMINATE constraint\n  \
            --verbose                   Add a full-precision achieved-deviation readout\n  \
            --purpose <name>=<binding>  Activate purpose <name> on <binding> (<entity>, or\n                              \
            <param>:<entity>,...) and check its constraints; repeatable\n  \
            --cfg <key=value|flag>      Set a cfg key or flag (`target=<v>` overrides\n                              \
            the host target); repeatable",
    },
    SubcommandHelp {
        name: "test",
        synopsis: "test <file>",
        usage: TEST_USAGE,
        summary: "Run @test-annotated structures, resolving their `auto` parameters",
        details: "",
    },
    SubcommandHelp {
        name: "build",
        synopsis: "build <file> [-o <output>]",
        usage: BUILD_USAGE,
        summary: "Resolve `auto` parameters, build geometry and export it",
        details: "Flags:\n  \
            --verbose  Print per-realization kernel provenance",
    },
    SubcommandHelp {
        name: "run",
        synopsis: "run <file>",
        usage: EVAL_USAGE,
        summary: "Alias for eval",
        details: "",
    },
    SubcommandHelp {
        name: "eval",
        synopsis: "eval <file>",
        usage: EVAL_USAGE,
        summary: "Evaluate and print every top-level value cell",
        details: "Flags:\n  \
            --explain-undef     Explain every undef cell, not only the printed ones\n  \
            --verbose           Print a persistent-cache hit/miss summary\n  \
            --cache-dir <path>  Use <path> as the persistent cache directory",
    },
    SubcommandHelp {
        name: "report",
        synopsis: "report --bom <file>",
        usage: REPORT_USAGE,
        summary: "Roll up a BOM / cost / waste / provenance report",
        details: "",
    },
    SubcommandHelp {
        name: "lsp",
        synopsis: "lsp",
        usage: LSP_USAGE,
        summary: "Start language server (stdin/stdout)",
        details: "",
    },
    SubcommandHelp {
        name: "gui",
        synopsis: "gui [--debug] <file>",
        usage: GUI_USAGE,
        summary: "Open file in GUI",
        details: "Flags:\n  \
            --debug, --mcp  Enable the MCP debug listener",
    },
    SubcommandHelp {
        name: "gui-debug",
        synopsis: "gui-debug <file>",
        usage: GUI_USAGE,
        summary: "Alias for `gui --debug`",
        details: "",
    },
    SubcommandHelp {
        name: "doc",
        synopsis: "doc <file> [-o <path>]",
        usage: DOC_USAGE,
        summary: "Generate documentation",
        details: "Flags:\n  \
            -o, --out <path>                Output file, or directory with --split / --stdlib\n  \
            --format html|markdown|json     Output format (default html)\n  \
            --split                         One file per item (markdown only; needs -o)\n  \
            --compact                       Compact output (json only)\n  \
            --stdlib                        Document the standard library (html; needs --out)",
    },
    SubcommandHelp {
        name: "explain",
        synopsis: "explain <file>",
        usage: EXPLAIN_USAGE,
        summary: "Print per-cell objective provenance: which objective chose each `auto` value",
        details: "",
    },
    SubcommandHelp {
        name: "dev",
        synopsis: "dev inspect-node <node-id>",
        usage: DEV_USAGE,
        summary: "Print a node's kind, traits, priority, policy, and overrides",
        details: "",
    },
    SubcommandHelp {
        name: "cache",
        synopsis: "cache export|import|stats|clear|gc",
        usage: CACHE_USAGE,
        summary: "Manage the persistent result cache",
        details: "Subcommands:\n  \
            export <hash>  Write a single cache entry to stdout as a tarball\n  \
            import         Read a cache tarball from stdin into the local cache\n  \
            stats          Print cache directory, entry count, total size, and top-N largest entries\n  \
            clear [--engine-version <hash>] --yes\n                 \
            Empty the cache (or one engine-version subdir); --yes required\n  \
            gc             Force LRU eviction down to the configured cache cap (live engine version only)",
    },
];

fn lookup(name: &str) -> Option<&'static SubcommandHelp> {
    SUBCOMMANDS.iter().find(|entry| entry.name == name)
}

/// The entry whose help `command_args` (argv without the program name) asks for:
/// a subcommand name followed, anywhere later, by `--help` or `-h`.
pub(crate) fn requested_subcommand_help(
    command_args: &[String],
) -> Option<&'static SubcommandHelp> {
    let (name, rest) = command_args.split_first()?;
    let entry = lookup(name)?;
    rest.iter()
        .any(|arg| arg == "--help" || arg == "-h")
        .then_some(entry)
}

pub(crate) fn write_overview(out: &mut dyn Write) {
    let _ = writeln!(out, "Usage: reify <command> [options]");
    let _ = writeln!(out, "Commands:");
    for entry in SUBCOMMANDS {
        let _ = writeln!(out, "  {:<34} {}", entry.synopsis, entry.summary);
    }
    let _ = writeln!(out, "  {:<34} Print version", "--version");
    let _ = writeln!(out, "  {:<34} Show this list", "--help");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Run `reify <command> --help` for a command's usage and flags."
    );
}

pub(crate) fn write_subcommand_help(entry: &SubcommandHelp, out: &mut dyn Write) {
    let _ = writeln!(out, "{}", entry.usage);
    let _ = writeln!(out);
    let _ = writeln!(out, "{}", entry.summary);
    if !entry.details.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", entry.details);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subcommand_names_are_unique_so_lookup_never_shadows_an_entry() {
        for (i, entry) in SUBCOMMANDS.iter().enumerate() {
            assert!(
                SUBCOMMANDS[..i]
                    .iter()
                    .all(|earlier| earlier.name != entry.name),
                "duplicate help entry `{}`",
                entry.name
            );
        }
    }

    #[test]
    fn help_is_requested_only_by_a_known_subcommand_followed_by_a_help_flag() {
        let argv = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        assert_eq!(
            requested_subcommand_help(&argv(&["check", "file.ri", "-h"])).map(|e| e.name),
            Some("check")
        );
        assert!(requested_subcommand_help(&argv(&["check", "file.ri"])).is_none());
        assert!(requested_subcommand_help(&argv(&["nope", "--help"])).is_none());
        assert!(requested_subcommand_help(&argv(&["--help", "check"])).is_none());
    }
}
