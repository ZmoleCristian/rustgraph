#![deny(dead_code, unused_imports, unused_mut, unused_variables)]

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};

use rustgraph::cli::Args;

#[cfg(unix)]
fn configure_sigpipe() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn configure_sigpipe() {}

fn parse_args_or_help() -> Args {
    match Args::try_parse() {
        Ok(args) => args,
        Err(e) => {

            match e.kind() {
                ErrorKind::DisplayHelp
                | ErrorKind::DisplayVersion
                | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
                    if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand) {
                        rustgraph::mcp_install::print_nudge_if_needed();
                    }
                    e.exit()
                }
                _ => {}
            }


            eprint!("{}", e);
            let raw_argv: Vec<String> = std::env::args().collect();
            let sub_name = raw_argv
                .iter()
                .skip(1)
                .find(|a| !a.starts_with('-'))
                .cloned();
            let mut cmd = Args::command();
            let help = if let Some(name) = sub_name.as_deref()
                && let Some(sub) = cmd.find_subcommand_mut(name)
            {
                sub.render_help()
            } else {
                cmd.render_help()
            };
            eprintln!();
            if sub_name.is_none() {
                rustgraph::mcp_install::print_nudge_if_needed();
            }
            eprintln!("{}", help);
            std::process::exit(e.exit_code());
        }
    }
}

/// Binary entry point. Dispatches MCP subcommands directly (bypassing the
/// `app::run` path) and delegates all other subcommands to [`rustgraph::app::run`].
///
/// On error, OS-level I/O errors get an actionable hint and exit code 2;
/// all other errors print the message and exit with code 1.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    configure_sigpipe();

    let args = parse_args_or_help();
    let use_color = if args.no_color { false } else { args.color };
    colored::control::set_override(use_color);


    if let Some(rustgraph::cli::ModeCommand::Mcp(mcp_cmd)) = &args.command {
        use rustgraph::cli::McpAction;
        match &mcp_cmd.action {
            None | Some(McpAction::List) => {
                let regs = rustgraph::mcp_install::list_all();
                println!("rustgraph MCP registration state:");
                rustgraph::mcp_install::print_report(&regs, false);
                std::process::exit(0);
            }
            Some(McpAction::Serve) => {
                colored::control::set_override(false);
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?;
                return rt.block_on(rustgraph::mcp::serve_stdio()).map_err(|e| {
                    let boxed: Box<dyn std::error::Error> = e.to_string().into();
                    boxed
                });
            }
            Some(McpAction::Install { quiet }) => {
                let regs = rustgraph::mcp_install::install_all();
                if !*quiet {
                    println!("rustgraph MCP install — checking detected AI clients:");
                }
                rustgraph::mcp_install::print_report(&regs, *quiet);
                let any_failed = regs.iter().any(|r| matches!(r.status, rustgraph::mcp_install::RegStatus::Failed(_)));
                std::process::exit(if any_failed { 1 } else { 0 });
            }
            Some(McpAction::Uninstall { quiet }) => {
                let regs = rustgraph::mcp_install::uninstall_all();
                if !*quiet {
                    println!("rustgraph MCP uninstall:");
                }
                rustgraph::mcp_install::print_report(&regs, *quiet);
                let any_failed = regs.iter().any(|r| matches!(r.status, rustgraph::mcp_install::RegStatus::Failed(_)));
                std::process::exit(if any_failed { 1 } else { 0 });
            }
        }
    }

    if let Some(rustgraph::cli::ModeCommand::Completions(c)) = &args.command {
        let mut app = Args::command();
        let name = app.get_name().to_string();
        clap_complete::generate(c.shell, &mut app, name, &mut std::io::stdout());
        std::process::exit(0);
    }

    if let Some(rustgraph::cli::ModeCommand::GenerateMan) = &args.command {
        let app = Args::command();
        let man = clap_mangen::Man::new(app);
        let mut buf: Vec<u8> = Vec::new();
        man.render(&mut buf)?;
        use std::io::Write;
        std::io::stdout().write_all(&buf)?;
        std::process::exit(0);
    }

    match rustgraph::app::run(args) {
        Ok(()) => Ok(()),
        Err(e) => {


            let msg = e.to_string();
            if msg.starts_with("Os {") || msg.starts_with("os error") {
                eprintln!("error: {}", msg);
                eprintln!("hint: this typically means rustgraph tried to read a source file at a path that no longer exists. \
                    If you passed --symbol-id or a path:LINE query, double-check the file is still there.");
                std::process::exit(2);
            }


            eprintln!("error: {}", msg);
            std::process::exit(1);
        }
    }
}
