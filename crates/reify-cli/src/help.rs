//! The CLI's subcommand table: one entry per subcommand, naming the [`Command`] `main`
//! dispatches it to and rendering its help as the top-level list, as `reify <cmd> --help`,
//! and as the banner a command prints on a usage error.

use std::io::Write;

use crate::cache::{CACHE_SUBCOMMANDS, CACHE_USAGE, CacheSubcommand};
use crate::dev::DEV_USAGE;

pub(crate) const CHECK_USAGE: &str = "Usage: reify check [--strict] [--verbose] [--purpose <name>=<binding>]... [--cfg <key=value|flag>]... <file>";

pub(crate) const TEST_USAGE: &str = "Usage: reify test <file>";

pub(crate) const BUILD_USAGE: &str = "Usage: reify build <file.ri> [-o <output>] [--out-dir <dir>] [--verbose] [--cfg <key=value|flag>]...\n  \
    With -o:    write a single file in that format (imperative).\n  \
    Without -o: every `: Output` occurrence in the design drives its own file\n              \
    (declarative); each relative path resolves against the .ri file's\n              \
    directory, or against --out-dir when given.";

pub(crate) const EVAL_USAGE: &str = "Usage: reify eval [--explain-undef] [--verbose] [--cache-dir <path>] [--cfg <key=value|flag>]... <file>";

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

/// One row of a command's `Flags:` table: the flag as typed, with its argument, and what it does.
struct FlagHelp {
    synopsis: &'static str,
    summary: &'static str,
}

/// The one home of the `--cfg` row: a command that takes the flag lists this, never a copy.
const CFG_FLAG: FlagHelp = FlagHelp {
    synopsis: "--cfg <key=value|flag>",
    summary: "Set a cfg key or flag (`target=<v>` overrides the host target); repeatable",
};

enum HelpSection {
    Prose(&'static str),
    Flags(&'static [FlagHelp]),
    CacheSubcommands(&'static [CacheSubcommand]),
}

pub(crate) struct SubcommandHelp {
    name: &'static str,
    pub(crate) command: Command,
    /// The short form `reify --help` lists; `usage` is the full banner.
    synopsis: &'static str,
    usage: &'static str,
    summary: &'static str,
    /// What `reify <cmd> --help` prints after the summary, in order, one blank line before
    /// each section.
    details: &'static [HelpSection],
}

/// Listed in the order `reify --help` prints them.
const SUBCOMMANDS: &[SubcommandHelp] = &[
    SubcommandHelp {
        name: "check",
        command: Command::Check,
        synopsis: "check [--strict] <file>",
        usage: CHECK_USAGE,
        summary: "Resolve `auto` parameters and check every constraint",
        details: &[
            HelpSection::Prose("Prints one OK / VIOLATED / INDETERMINATE line per constraint."),
            HelpSection::Flags(&[
                FlagHelp {
                    synopsis: "--strict",
                    summary: "Also fail on any INDETERMINATE constraint",
                },
                FlagHelp {
                    synopsis: "--verbose",
                    summary: "Add a full-precision achieved-deviation readout",
                },
                FlagHelp {
                    synopsis: "--purpose <name>=<binding>",
                    summary: "Activate purpose <name> on <binding> (<entity>, or \
                              <param>:<entity>,...) and check its constraints; repeatable",
                },
                CFG_FLAG,
            ]),
        ],
    },
    SubcommandHelp {
        name: "test",
        command: Command::Test,
        synopsis: "test <file>",
        usage: TEST_USAGE,
        summary: "Run @test-annotated structures, resolving their `auto` parameters",
        details: &[],
    },
    SubcommandHelp {
        name: "build",
        command: Command::Build,
        synopsis: "build <file> [-o <output>]",
        usage: BUILD_USAGE,
        summary: "Resolve `auto` parameters, build geometry and export it",
        details: &[HelpSection::Flags(&[
            FlagHelp {
                synopsis: "--verbose",
                summary: "Print per-realization kernel provenance",
            },
            CFG_FLAG,
        ])],
    },
    SubcommandHelp {
        name: "run",
        command: Command::Eval,
        synopsis: "run <file>",
        usage: EVAL_USAGE,
        summary: "Alias for eval",
        details: &[],
    },
    SubcommandHelp {
        name: "eval",
        command: Command::Eval,
        synopsis: "eval <file>",
        usage: EVAL_USAGE,
        summary: "Evaluate and print every top-level value cell",
        details: &[HelpSection::Flags(&[
            FlagHelp {
                synopsis: "--explain-undef",
                summary: "Explain every undef cell, not only the printed ones",
            },
            FlagHelp {
                synopsis: "--verbose",
                summary: "Print a persistent-cache hit/miss summary",
            },
            FlagHelp {
                synopsis: "--cache-dir <path>",
                summary: "Use <path> as the persistent cache directory",
            },
            CFG_FLAG,
        ])],
    },
    SubcommandHelp {
        name: "report",
        command: Command::Report,
        synopsis: "report --bom <file>",
        usage: REPORT_USAGE,
        summary: "Roll up a BOM / cost / waste / provenance report",
        details: &[],
    },
    SubcommandHelp {
        name: "lsp",
        command: Command::Lsp,
        synopsis: "lsp",
        usage: LSP_USAGE,
        summary: "Start language server (stdin/stdout)",
        details: &[],
    },
    SubcommandHelp {
        name: "gui",
        command: Command::Gui,
        synopsis: "gui [--debug] <file>",
        usage: GUI_USAGE,
        summary: "Open file in GUI",
        details: &[HelpSection::Flags(&[FlagHelp {
            synopsis: "--debug, --mcp",
            summary: "Enable the MCP debug listener",
        }])],
    },
    SubcommandHelp {
        name: "gui-debug",
        command: Command::GuiDebug,
        synopsis: "gui-debug <file>",
        usage: GUI_USAGE,
        summary: "Alias for `gui --debug`",
        details: &[],
    },
    SubcommandHelp {
        name: "doc",
        command: Command::Doc,
        synopsis: "doc <file> [-o <path>]",
        usage: DOC_USAGE,
        summary: "Generate documentation",
        details: &[HelpSection::Flags(&[
            FlagHelp {
                synopsis: "-o, --out <path>",
                summary: "Output file, or directory with --split / --stdlib",
            },
            FlagHelp {
                synopsis: "--format html|markdown|json",
                summary: "Output format (default html)",
            },
            FlagHelp {
                synopsis: "--split",
                summary: "One file per item (markdown only; needs -o)",
            },
            FlagHelp {
                synopsis: "--compact",
                summary: "Compact output (json only)",
            },
            FlagHelp {
                synopsis: "--stdlib",
                summary: "Document the standard library (html; needs --out)",
            },
        ])],
    },
    SubcommandHelp {
        name: "explain",
        command: Command::Explain,
        synopsis: "explain <file>",
        usage: EXPLAIN_USAGE,
        summary: "Print per-cell objective provenance: which objective chose each `auto` value",
        details: &[],
    },
    SubcommandHelp {
        name: "fix",
        command: Command::Fix,
        synopsis: "fix --imports <file>...",
        usage: FIX_USAGE,
        summary: "Add the `import std.X` lines each file needs",
        details: &[
            HelpSection::Prose(
                "Refuses to write a file whose derived imports would leave a diagnostic the\n\
                full standard library does not.",
            ),
            HelpSection::Flags(&[
                FlagHelp {
                    synopsis: "--imports",
                    summary: "Add every missing `import std.X` line (required)",
                },
                FlagHelp {
                    synopsis: "--check",
                    summary: "Report missing imports without writing; fail if any are missing",
                },
            ]),
        ],
    },
    SubcommandHelp {
        name: "dev",
        command: Command::Dev,
        synopsis: "dev inspect-node <node-id>",
        usage: DEV_USAGE,
        summary: "Print a node's kind, traits, priority, policy, and overrides",
        details: &[],
    },
    SubcommandHelp {
        name: "cache",
        command: Command::Cache,
        synopsis: "cache export|import|stats|clear|gc",
        usage: CACHE_USAGE,
        summary: "Manage the persistent result cache",
        details: &[HelpSection::CacheSubcommands(CACHE_SUBCOMMANDS)],
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
    for section in entry.details {
        let _ = writeln!(out);
        match section {
            HelpSection::Prose(text) => {
                let _ = writeln!(out, "{text}");
            }
            HelpSection::Flags(flags) => write_flags(flags, out),
            HelpSection::CacheSubcommands(subcommands) => {
                let _ = writeln!(out, "Subcommands:");
                for subcommand in *subcommands {
                    let _ = writeln!(out, "  {}", subcommand.synopsis);
                    let _ = writeln!(out, "      {}", subcommand.summary);
                }
            }
        }
    }
}

/// The widest line a `Flags:` table prints, in characters.
const HELP_WIDTH: usize = 80;

/// Descriptions start two columns past the widest flag, and wrap onto continuation lines
/// indented to that column.
fn write_flags(flags: &[FlagHelp], out: &mut dyn Write) {
    let _ = writeln!(out, "Flags:");
    let width = flags
        .iter()
        .map(|flag| flag.synopsis.chars().count())
        .max()
        .unwrap_or(0);
    let column = 2 + width + 2;
    for flag in flags {
        let mut lines = wrap_words(flag.summary, HELP_WIDTH.saturating_sub(column)).into_iter();
        let first_line = lines.next().unwrap_or_default();
        let _ = writeln!(out, "  {:<width$}  {first_line}", flag.synopsis);
        for line in lines {
            let _ = writeln!(out, "{:column$}{line}", "");
        }
    }
}

/// Greedy word wrap: each line takes whole words while it stays within `width` characters.
/// A word wider than `width` is never split; it sits alone on its line.
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
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

    fn demo_entry(details: &'static [HelpSection]) -> SubcommandHelp {
        SubcommandHelp {
            name: "demo",
            command: Command::Check,
            synopsis: "demo",
            usage: "Usage: reify demo",
            summary: "Demo summary.",
            details,
        }
    }

    fn rendered(entry: &SubcommandHelp) -> String {
        let mut out = Vec::new();
        write_subcommand_help(entry, &mut out);
        String::from_utf8(out).expect("help text is UTF-8")
    }

    #[test]
    fn a_flags_section_aligns_every_description_past_the_widest_flag_and_wraps_within_80_columns() {
        let entry = demo_entry(&[
            HelpSection::Prose("Intro paragraph."),
            HelpSection::Flags(&[
                FlagHelp {
                    synopsis: "--quiet",
                    summary: "Print nothing but errors",
                },
                FlagHelp {
                    synopsis: "--long-flag <value>",
                    summary: "Sets the long flag to the given value for this run alone, \
                              overrides any default and each earlier setting; repeatable",
                },
            ]),
        ]);
        let expected = [
            "Usage: reify demo",
            "",
            "Demo summary.",
            "",
            "Intro paragraph.",
            "",
            "Flags:",
            "  --quiet              Print nothing but errors",
            "  --long-flag <value>  Sets the long flag to the given value for this run alone,",
            "                       overrides any default and each earlier setting;",
            "                       repeatable",
        ]
        .join("\n")
            + "\n";
        assert_eq!(rendered(&entry), expected);
    }

    #[test]
    fn a_word_wider_than_the_description_width_sits_alone_on_its_line_unsplit() {
        let entry = demo_entry(&[HelpSection::Flags(&[FlagHelp {
            synopsis: "--docs <url>",
            summary: "Read https://example.invalid/reify/reference/flags/the-complete-list.html \
                      for the details",
        }])]);
        let expected = [
            "Usage: reify demo",
            "",
            "Demo summary.",
            "",
            "Flags:",
            "  --docs <url>  Read",
            "                https://example.invalid/reify/reference/flags/the-complete-list.html",
            "                for the details",
        ]
        .join("\n")
            + "\n";
        assert_eq!(rendered(&entry), expected);
    }

    #[test]
    fn an_entry_with_no_sections_prints_only_usage_and_summary() {
        assert_eq!(
            rendered(&demo_entry(&[])),
            "Usage: reify demo\n\nDemo summary.\n"
        );
    }

    /// Every row of every subcommand's `Flags:` table, with the subcommand that lists it.
    fn listed_flags() -> impl Iterator<Item = (&'static str, &'static FlagHelp)> {
        SUBCOMMANDS.iter().flat_map(|entry| {
            entry
                .details
                .iter()
                .filter_map(|section| match section {
                    HelpSection::Flags(flags) => Some(*flags),
                    _ => None,
                })
                .flatten()
                .map(move |flag| (entry.name, flag))
        })
    }

    #[test]
    fn every_flag_row_in_the_subcommand_table_has_a_non_blank_summary() {
        assert!(
            listed_flags().next().is_some(),
            "no subcommand lists a flag, so there was nothing to check"
        );
        let undescribed: Vec<String> = listed_flags()
            .filter(|(_, flag)| flag.summary.trim().is_empty())
            .map(|(name, flag)| format!("`reify {name} --help`: {}", flag.synopsis))
            .collect();
        assert!(
            undescribed.is_empty(),
            "every `Flags:` row must say what its flag does, but these have no description: \
             {undescribed:?}"
        );
    }
}
