use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::config::DEFAULT_API_URL;
use crate::output::Format;

#[derive(Parser)]
#[command(
    name = "pms",
    version,
    about = "Command line tool for PMServers",
    propagate_version = true
)]
pub struct Cli {
    /// API address. Use http://localhost for a local OrbStack stack.
    #[arg(long, global = true, env = "PMS_API_URL", default_value = DEFAULT_API_URL)]
    pub api_url: String,

    /// Output format. Use json in scripts.
    #[arg(short, long, global = true, env = "PMS_OUTPUT", value_enum, default_value_t = Format::Table)]
    pub output: Format,

    /// Do not ask for confirmation.
    #[arg(short, long, global = true)]
    pub yes: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Sign in. Opens the browser for GitHub, or use --email.
    Login(LoginArgs),
    /// Sign out and remove the saved session.
    Logout,
    /// Show the signed-in user.
    Whoami,
    /// Show sizes, images and AI models you can use.
    Catalog(CatalogArgs),
}

#[derive(Args)]
pub struct LoginArgs {
    /// Sign in with email and password. The CLI asks for both.
    #[arg(long)]
    pub email: bool,
    /// Listen on this port for the browser callback. Use it if Supabase does not allow a random port.
    #[arg(long, value_name = "PORT", conflicts_with = "email")]
    pub port: Option<u16>,
    /// Print the sign-in URL and do not open the browser.
    #[arg(long, conflicts_with = "email")]
    pub no_browser: bool,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum CatalogKind {
    All,
    Sizes,
    Images,
    Models,
}

#[derive(Args)]
pub struct CatalogArgs {
    #[arg(value_enum, default_value_t = CatalogKind::All)]
    pub what: CatalogKind,
}
