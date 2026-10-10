#![allow(dead_code)] // removed in the release phase, once every command uses the helpers

mod cli;
mod cloudflared;
mod commands;
mod config;
mod console;
mod ctx;
mod envfile;
mod keygen;
mod login;
mod output;
mod session;
mod ssh;
mod supabase;
mod wait;

use clap::Parser;

#[tokio::main]
async fn main() {
    let cli = cli::Cli::parse();
    let ctx = ctx::Ctx::new(cli.output, cli.yes, cli.api_url, cli.project);
    if let Err(e) = commands::run(&ctx, cli.command).await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
