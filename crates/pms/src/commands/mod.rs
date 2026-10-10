mod auth;
mod catalog;

use anyhow::Result;

use crate::cli::Command;
use crate::ctx::Ctx;

pub async fn run(ctx: &Ctx, command: Command) -> Result<()> {
    match command {
        Command::Login(a) => auth::login(ctx, a).await,
        Command::Logout => auth::logout(ctx).await,
        Command::Whoami => auth::whoami(ctx).await,
        Command::Catalog(a) => catalog::show(ctx, a).await,
    }
}
