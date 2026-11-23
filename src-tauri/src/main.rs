#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

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
use workspace::Workspace;

const HELP: &str = "ResuFlow: local-first resumes for you and your AI agent.

Usage:
  resuflow                         Open the desktop app
  resuflow mcp                     Run the MCP server on stdio (for AI agents)
  resuflow mcp-config              Print MCP client config for this install
  resuflow list                    List resumes
  resuflow check SLUG              Run resume checks
  resuflow render SLUG [-o FILE]   Render print-ready HTML
  resuflow pdf SLUG -o FILE        Save as PDF (Linux)
  resuflow install-skills claude|agents   Copy built-in skills into an agent
  resuflow --help | --version

Data lives in your app-data folder (Linux: ~/.local/share/resuflow).
--dir DIR or $RESUFLOW_DIR use another folder (for development).";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let dir = take_flag(&mut args, "--dir").map(PathBuf::from);
    let out = take_flag(&mut args, "-o").or_else(|| take_flag(&mut args, "--out"));
    let cmd = args.first().cloned().unwrap_or_default();
    let arg = |i: usize| args.get(i).cloned();

    let result: Result<(), String> = match cmd.as_str() {
        "-h" | "--help" | "help" => {
            println!("{HELP}");
            Ok(())
        }
        "-V" | "--version" => {
            println!("resuflow {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "mcp" => mcp::serve(dir).map_err(|e| e.to_string()),
        "mcp-config" => {
            println!("{}", serde_json::to_string_pretty(&app::mcp_config()).unwrap());
            Ok(())
        }
        "list" => open(dir).and_then(|ws| {
            for r in ws.list_resumes().map_err(|e| e.0)? {
                let tags = if r.tags.is_empty() { String::new() } else { format!("  [{}]", r.tags.join(", ")) };
                println!("{:<32} {:<9} {}{}", r.slug, r.status, r.title, tags);
            }
            Ok(())
        }),
        "check" => open(dir).and_then(|ws| {
            let slug = arg(1).ok_or("usage: resuflow check SLUG")?;
            let r = ws.check(&slug).map_err(|e| e.0)?;
            println!("{} words · {} bullets ({} quantified) · ~{} pages", r.words, r.bullets, r.bullets_with_numbers, r.est_pages);
            for f in &r.findings {
                let at = f.line.map(|l| format!("resume.md:{l}  ")).unwrap_or_default();
                println!("{:<5}  {at}{}", f.level, f.message);
            }
            if r.findings.iter().any(|f| f.level == "error") { Err(String::new()) } else { Ok(()) }
        }),
        "render" => open(dir).and_then(|ws| {
            let slug = arg(1).ok_or("usage: resuflow render SLUG [-o FILE]")?;
            let html = ws.render(&slug).map_err(|e| e.0)?;
            match out {
                Some(path) => std::fs::write(&path, html).map_err(|e| e.to_string()),
                None => {
                    print!("{html}");
                    Ok(())
                }
            }
        }),
        "pdf" => match (arg(1), out) {
            (Some(slug), Some(file)) => app::export_pdf(dir, slug, PathBuf::from(file)),
            _ => Err("usage: resuflow pdf SLUG -o FILE".into()),
        },
        "install-skills" => {
            match arg(1) {
                Some(agent) => skills::install(&agent)
                    .map(|p| println!("Installed {} skills into {}", skills::list().len(), p.display())),
                None => Err("usage: resuflow install-skills claude|agents".into()),
            }
        }
        _ if cmd.starts_with('-') => Err(format!("unknown option {cmd}\n\n{HELP}")),
        "" => {
            app::run(dir);
            Ok(())
        }
        _ => Err(format!("unknown command '{cmd}'\n\n{HELP}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("resuflow: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

fn open(dir: Option<PathBuf>) -> Result<Workspace, String> {
    config::workspace(dir).map_err(|e| e.0)
}

fn take_flag(args: &mut Vec<String>, flag: &str) -> Option<String> {
    let i = args.iter().position(|a| a == flag)?;
    if i + 1 >= args.len() {
        args.remove(i);
        return None;
    }
    args.remove(i);
    Some(args.remove(i))
}
