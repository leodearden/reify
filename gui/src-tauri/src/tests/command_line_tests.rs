//! Tests for what reify-gui's command line asks for
//! (`crate::command_line::{parse, Launch}`), driven with argument lists, never
//! the real process argv.

use std::ffi::OsString;

use crate::command_line::{Launch, parse};

fn launch_for(args: &[&str]) -> Launch {
    parse(args.iter().map(OsString::from))
}

fn os_strings(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

#[test]
fn no_arguments_opens_the_gui_without_a_file() {
    assert_eq!(launch_for(&[]), Launch::Gui { file: None });
}

#[test]
fn a_file_argument_opens_the_gui_on_it() {
    assert_eq!(
        launch_for(&["m5.ri"]),
        Launch::Gui {
            file: Some(OsString::from("m5.ri"))
        }
    );
}

#[test]
fn help_flags_ask_for_help() {
    assert_eq!(launch_for(&["--help"]), Launch::Help);
    assert_eq!(launch_for(&["-h"]), Launch::Help);
}

#[test]
fn version_flags_ask_for_the_version() {
    assert_eq!(launch_for(&["--version"]), Launch::Version);
    assert_eq!(launch_for(&["-V"]), Launch::Version);
}

#[test]
fn print_webkit_env_flag_asks_for_the_env_report() {
    assert_eq!(
        launch_for(&[crate::webkit_env::PRINT_ENV_FLAG]),
        Launch::PrintWebkitEnv
    );
}

#[test]
fn cli_forwards_every_remaining_argument_verbatim() {
    assert_eq!(
        launch_for(&["cli", "check", "a b.ri"]),
        Launch::Cli {
            args: os_strings(&["check", "a b.ri"])
        }
    );
}

#[test]
fn bare_cli_forwards_no_arguments() {
    assert_eq!(launch_for(&["cli"]), Launch::Cli { args: Vec::new() });
}

#[test]
fn unknown_dash_arguments_are_unknown_options() {
    assert_eq!(
        launch_for(&["--no-such-option"]),
        Launch::UnknownOption(OsString::from("--no-such-option"))
    );
    assert_eq!(
        launch_for(&["-v"]),
        Launch::UnknownOption(OsString::from("-v"))
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_file_argument_opens_the_gui_on_it() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let file = OsStr::from_bytes(b"\xff.ri").to_os_string();
    assert_eq!(parse([file.clone()]), Launch::Gui { file: Some(file) });
}
