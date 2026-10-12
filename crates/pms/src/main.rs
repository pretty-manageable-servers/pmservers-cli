mod cli;
mod cloudflared;
mod commands;
mod config;
mod ctx;
mod envfile;
mod login;
mod output;
mod session;
mod ssh;
mod supabase;
mod wait;

use clap::{CommandFactory, FromArgMatches};

#[tokio::main]
async fn main() {
    let matches = cli::Cli::command().get_matches();
    let cli = cli::Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    let mut ctx = ctx::Ctx::new(cli.output, cli.yes, cli.api_url, cli.project);
    ctx.project_typed = cli::project_typed(&matches);
    if let Err(e) = commands::run(&ctx, cli.command).await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
