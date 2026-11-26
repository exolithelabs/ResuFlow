#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod about;
mod app;
mod config;
mod doc;
mod lint;
mod mcp;
mod pdf;
mod render;
mod skills;
mod workspace;

use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "ResuFlow: local-first resumes for you and your AI agent.

Usage:
  resuflow [--dir DIR]       Open the desktop app
  resuflow mcp [--dir DIR]   Run the MCP server on stdio (for AI agents)
  resuflow --help | --version

Data lives in your app-data folder (Linux: ~/.local/share/resuflow).
--dir DIR or $RESUFLOW_DIR use another folder (for development).";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let dir = match take_flag(&mut args, "--dir") {
        Ok(dir) => dir.map(PathBuf::from),
        Err(e) => return fail(&e),
    };

    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] => {
            app::run(dir);
            ExitCode::SUCCESS
        }
        ["mcp"] => match mcp::serve(dir) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => fail(&e.to_string()),
        },
        ["-h" | "--help" | "help"] => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        ["-V" | "--version"] => {
            println!("resuflow {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        [cmd, ..] if cmd.starts_with('-') => fail(&format!("unknown option {cmd}\n\n{HELP}")),
        [cmd] => fail(&format!("unknown command '{cmd}'\n\n{HELP}")),
        _ => fail(&format!("too many arguments\n\n{HELP}")),
    }
}

fn fail(e: &str) -> ExitCode {
    eprintln!("resuflow: {e}");
    ExitCode::FAILURE
}

fn take_flag(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, String> {
    let Some(i) = args.iter().position(|a| a == flag) else { return Ok(None) };
    args.remove(i);
    if i >= args.len() {
        return Err(format!("{flag} needs a folder"));
    }
    Ok(Some(args.remove(i)))
}
