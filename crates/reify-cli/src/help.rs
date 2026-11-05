//! The CLI's subcommand table: one entry per subcommand, naming the [`Command`] `main`
//! dispatches it to and rendering its help as the top-level list, as `reify <cmd> --help`,
//! and as the banner a command prints on a usage error.

use std::io::Write;

use crate::cache::{CACHE_SUBCOMMANDS, CACHE_USAGE, CacheSubcommand};
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

pub(crate) const FIX_USAGE: &str = "Usage: reify fix --imports [--check] <file>...";

/// What `main` runs for a subcommand. Only a [`SUBCOMMANDS`] entry constructs one, so a
/// command with no help entry is a never-constructed variant rather than a silent gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    Check,
    Test,
    Build,
    Eval,
    Report,
    Doc,
    Lsp,
    Gui,
    GuiDebug,
    Explain,
    Fix,
    Dev,
    Cache,
}

/// What `reify <cmd> --help` prints after the summary.
enum Details {
    None,
    Text(&'static str),
    CacheSubcommands(&'static [CacheSubcommand]),
}

pub(crate) struct SubcommandHelp {
    name: &'static str,
    pub(crate) command: Command,
    /// The short form `reify --help` lists; `usage` is the full banner.
    synopsis: &'static str,
    usage: &'static str,
    summary: &'static str,
    details: Details,
}

/// Listed in the order `reify --help` prints them.
const SUBCOMMANDS: &[SubcommandHelp] = &[
    SubcommandHelp {
        name: "check",
        command: Command::Check,
        synopsis: "check [--strict] <file>",
        usage: CHECK_USAGE,
        summary: "Resolve `auto` parameters and check every constraint",
        details: Details::Text(
            "Prints one OK / VIOLATED / INDETERMINATE line per constraint.\n\
            \n\
            Flags:\n  \
            --strict                    Also fail on any INDETERMINATE constraint\n  \
            --verbose                   Add a full-precision achieved-deviation readout\n  \
            --purpose <name>=<binding>  Activate purpose <name> on <binding> (<entity>, or\n                              \
            <param>:<entity>,...) and check its constraints; repeatable\n  \
            --cfg <key=value|flag>      Set a cfg key or flag (`target=<v>` overrides\n                              \
            the host target); repeatable",
        ),
    },
    SubcommandHelp {
        name: "test",
        command: Command::Test,
        synopsis: "test <file>",
        usage: TEST_USAGE,
        summary: "Run @test-annotated structures, resolving their `auto` parameters",
        details: Details::None,
    },
    SubcommandHelp {
        name: "build",
        command: Command::Build,
        synopsis: "build <file> [-o <output>]",
        usage: BUILD_USAGE,
        summary: "Resolve `auto` parameters, build geometry and export it",
        details: Details::Text(
            "Flags:\n  \
            --verbose  Print per-realization kernel provenance",
        ),
    },
    SubcommandHelp {
        name: "run",
        command: Command::Eval,
        synopsis: "run <file>",
        usage: EVAL_USAGE,
        summary: "Alias for eval",
        details: Details::None,
    },
    SubcommandHelp {
        name: "eval",
        command: Command::Eval,
        synopsis: "eval <file>",
        usage: EVAL_USAGE,
        summary: "Evaluate and print every top-level value cell",
        details: Details::Text(
            "Flags:\n  \
            --explain-undef     Explain every undef cell, not only the printed ones\n  \
            --verbose           Print a persistent-cache hit/miss summary\n  \
            --cache-dir <path>  Use <path> as the persistent cache directory",
        ),
    },
    SubcommandHelp {
        name: "report",
        command: Command::Report,
        synopsis: "report --bom <file>",
        usage: REPORT_USAGE,
        summary: "Roll up a BOM / cost / waste / provenance report",
        details: Details::None,
    },
    SubcommandHelp {
        name: "lsp",
        command: Command::Lsp,
        synopsis: "lsp",
        usage: LSP_USAGE,
        summary: "Start language server (stdin/stdout)",
        details: Details::None,
    },
    SubcommandHelp {
        name: "gui",
        command: Command::Gui,
        synopsis: "gui [--debug] <file>",
        usage: GUI_USAGE,
        summary: "Open file in GUI",
        details: Details::Text(
            "Flags:\n  \
            --debug, --mcp  Enable the MCP debug listener",
        ),
    },
    SubcommandHelp {
        name: "gui-debug",
        command: Command::GuiDebug,
        synopsis: "gui-debug <file>",
        usage: GUI_USAGE,
        summary: "Alias for `gui --debug`",
        details: Details::None,
    },
    SubcommandHelp {
        name: "doc",
        command: Command::Doc,
        synopsis: "doc <file> [-o <path>]",
        usage: DOC_USAGE,
        summary: "Generate documentation",
        details: Details::Text(
            "Flags:\n  \
            -o, --out <path>                Output file, or directory with --split / --stdlib\n  \
            --format html|markdown|json     Output format (default html)\n  \
            --split                         One file per item (markdown only; needs -o)\n  \
            --compact                       Compact output (json only)\n  \
            --stdlib                        Document the standard library (html; needs --out)",
        ),
    },
    SubcommandHelp {
        name: "explain",
        command: Command::Explain,
        synopsis: "explain <file>",
        usage: EXPLAIN_USAGE,
        summary: "Print per-cell objective provenance: which objective chose each `auto` value",
        details: Details::None,
    },
    SubcommandHelp {
        name: "fix",
        command: Command::Fix,
        synopsis: "fix --imports <file>...",
        usage: FIX_USAGE,
        summary: "Add the `import std.X` lines each file needs",
        details: Details::Text(
            "Refuses to write a file whose derived imports would leave a diagnostic the\n\
            full standard library does not.\n\
            \n\
            Flags:\n  \
            --imports  Add every missing `import std.X` line (required)\n  \
            --check    Report missing imports without writing; fail if any are missing",
        ),
    },
    SubcommandHelp {
        name: "dev",
        command: Command::Dev,
        synopsis: "dev inspect-node <node-id>",
        usage: DEV_USAGE,
        summary: "Print a node's kind, traits, priority, policy, and overrides",
        details: Details::None,
    },
    SubcommandHelp {
        name: "cache",
        command: Command::Cache,
        synopsis: "cache export|import|stats|clear|gc",
        usage: CACHE_USAGE,
        summary: "Manage the persistent result cache",
        details: Details::CacheSubcommands(CACHE_SUBCOMMANDS),
    },
];

pub(crate) fn lookup(name: &str) -> Option<&'static SubcommandHelp> {
    SUBCOMMANDS.iter().find(|entry| entry.name == name)
}

/// Whether the arguments after a subcommand's name ask for its help: `--help` or `-h`
/// anywhere among them.
pub(crate) fn requests_help(subcommand_args: &[String]) -> bool {
    subcommand_args
        .iter()
        .any(|arg| arg == "--help" || arg == "-h")
}

pub(crate) fn write_overview(out: &mut dyn Write) {
    let _ = writeln!(out, "Usage: reify <command> [options]");
    let _ = writeln!(out, "Commands:");
    let rows = SUBCOMMANDS
        .iter()
        .map(|entry| (entry.synopsis, entry.summary))
        .chain([("--version", "Print version"), ("--help", "Show this list")]);
    let width = rows
        .clone()
        .map(|(synopsis, _)| synopsis.len())
        .max()
        .unwrap_or(0);
    for (synopsis, summary) in rows {
        let _ = writeln!(out, "  {synopsis:<width$}  {summary}");
    }
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
    match entry.details {
        Details::None => {}
        Details::Text(text) => {
            let _ = writeln!(out);
            let _ = writeln!(out, "{text}");
        }
        Details::CacheSubcommands(subcommands) => {
            let _ = writeln!(out);
            let _ = writeln!(out, "Subcommands:");
            for subcommand in subcommands {
                let _ = writeln!(out, "  {}", subcommand.synopsis);
                let _ = writeln!(out, "      {}", subcommand.summary);
            }
        }
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
    fn only_a_listed_subcommand_is_looked_up() {
        assert_eq!(lookup("check").map(|e| e.command), Some(Command::Check));
        assert_eq!(lookup("run").map(|e| e.command), Some(Command::Eval));
        assert!(lookup("nope").is_none());
        assert!(lookup("--help").is_none());
    }

    #[test]
    fn help_is_requested_by_a_help_flag_anywhere_among_the_subcommand_arguments() {
        let argv = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        assert!(requests_help(&argv(&["file.ri", "-h"])));
        assert!(requests_help(&argv(&["--help"])));
        assert!(!requests_help(&argv(&["file.ri"])));
        assert!(!requests_help(&argv(&["--helpful"])));
    }
}
