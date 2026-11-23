use crate::workspace::WorkspaceError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandMode {
    Desktop,
    Serve,
    Init,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgs {
    pub command: CommandMode,
    pub dir: Option<String>,
    pub port: u16,
    pub open_browser: bool,
    pub help: bool,
}

pub fn parse_args(argv: &[String]) -> Result<CliArgs, WorkspaceError> {
    let mut command = CommandMode::Desktop;
    let mut dir: Option<String> = None;
    let mut port_val: i64 = 4173;
    let mut open_browser = false;
    let mut help = false;
    let mut positionals: Vec<String> = Vec::new();

    let mut i = 0;
    while i < argv.len() {
        let token = &argv[i];
        if token == "-h" || token == "--help" {
            help = true;
        } else if token == "--browser" {
            command = CommandMode::Serve;
            open_browser = true;
        } else if token == "--no-open" {
            open_browser = false;
        } else if token == "--port" {
            i += 1;
            let raw = argv.get(i).ok_or_else(|| WorkspaceError::bad_request("Missing value for --port"))?;
            port_val = raw
                .parse::<i64>()
                .map_err(|_| WorkspaceError::bad_request(format!("Invalid port: {raw}")))?;
        } else if let Some(raw) = token.strip_prefix("--port=") {
            port_val = raw
                .parse::<i64>()
                .map_err(|_| WorkspaceError::bad_request(format!("Invalid port: {raw}")))?;
        } else if token == "--dir" {
            i += 1;
            let val = argv.get(i).ok_or_else(|| WorkspaceError::bad_request("Missing value for --dir"))?;
            dir = Some(val.clone());
        } else if let Some(val) = token.strip_prefix("--dir=") {
            dir = Some(val.to_string());
        } else if token.starts_with('-') {
            return Err(WorkspaceError::bad_request(format!("Unknown flag: {token}")));
        } else {
            positionals.push(token.clone());
        }
        i += 1;
    }

    if let Some(first) = positionals.first() {
        match first.as_str() {
            "init" => {
                command = CommandMode::Init;
                if dir.is_none() && positionals.len() > 1 {
                    dir = Some(positionals[1].clone());
                }
            }
            "serve" => {
                command = CommandMode::Serve;
                if dir.is_none() && positionals.len() > 1 {
                    dir = Some(positionals[1].clone());
                }
            }
            "desktop" => {
                command = CommandMode::Desktop;
                if dir.is_none() && positionals.len() > 1 {
                    dir = Some(positionals[1].clone());
                }
            }
            "help" => {
                command = CommandMode::Help;
                help = true;
            }
            _ => {
                if dir.is_none() {
                    dir = Some(first.clone());
                }
            }
        }
    }

    if matches!(command, CommandMode::Serve) && argv.iter().any(|arg| arg == "--browser") {
        open_browser = true;
    }

    if !(1..=65535).contains(&port_val) {
        return Err(WorkspaceError::bad_request(format!("Invalid port: {port_val}")));
    }

    Ok(CliArgs {
        command,
        dir,
        port: port_val as u16,
        open_browser,
        help,
    })
}

pub fn help_text() -> &'static str {
    "ResuFlow — local web app for role-specific resumes

Install:
  Windows: download the installer from the product website.
  Linux (Arch): sudo pacman -U <release-url.pkg.tar.zst>, or download
  resuflow-linux-<arch>.pkg.tar.zst and run sudo pacman -U ./*.pkg.tar.zst
  from that folder. The same command upgrades an existing install.

Usage:
  resuflow [dir]               Open the desktop application
  resuflow desktop [dir]       Same as the default
  resuflow --browser [dir]     Start the server and open a web browser
  resuflow serve [dir]         Start the localhost server only
  resuflow init [dir]          Create a data workspace
  resuflow --help

Options:
  --dir <path>     Workspace directory
  --port <number>  Port for the Web UI (default 4173)
  --browser        Open in a web browser instead of the desktop window
  --no-open        Do not open a browser (serve only)

The desktop window and the browser both talk to http://127.0.0.1:4173/.
Use the supported update method for your platform. Uninstalling never removes resume workspaces."
}
