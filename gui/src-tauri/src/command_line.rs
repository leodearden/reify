//! What reify-gui's command line asks for, decided before any window exists.

use std::ffi::OsString;

use crate::webkit_env::PRINT_ENV_FLAG;

/// What one reify-gui invocation asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// Open the GUI, on `file` when one was given, else on the Start view.
    Gui { file: Option<OsString> },
    /// Run the reify CLI shipped beside this GUI with `args`.
    Cli { args: Vec<OsString> },
    /// Print [`USAGE`].
    Help,
    /// Print the version.
    Version,
    /// Print the WebKit environment report (a diagnostic seam).
    PrintWebkitEnv,
    /// A dash-prefixed first argument reify-gui does not know.
    UnknownOption(OsString),
}

/// The subcommand that forwards every remaining argument to the bundled CLI.
const CLI_SUBCOMMAND: &str = "cli";

/// The `--help` text, newline-terminated, for `print!`.
pub const USAGE: &str = "\
Usage: reify-gui [FILE.ri]
       reify-gui cli [ARGS...]

  FILE.ri        open this design; without it the Start view opens
  cli [ARGS...]  run the reify CLI shipped beside this GUI with ARGS;
                 `reify-gui cli --help` lists its commands
  -h, --help     print this help and exit
  -V, --version  print the version and exit

From the AppImage, run the AppImage file in place of reify-gui.
";

/// The [`Launch`] that `args`, the arguments after the program name, ask for.
/// The first argument decides; extra arguments after a file are ignored.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Launch {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Launch::Gui { file: None };
    };
    match first.to_str() {
        Some("--help" | "-h") => Launch::Help,
        Some("--version" | "-V") => Launch::Version,
        Some(flag) if flag == PRINT_ENV_FLAG => Launch::PrintWebkitEnv,
        Some(CLI_SUBCOMMAND) => Launch::Cli {
            args: args.collect(),
        },
        _ if first.as_encoded_bytes().starts_with(b"-") => Launch::UnknownOption(first),
        _ => Launch::Gui { file: Some(first) },
    }
}
