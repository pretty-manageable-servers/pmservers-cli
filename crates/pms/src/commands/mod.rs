mod auth;
mod catalog;
mod events;
mod member;
mod project;
mod ssh_key;
mod vm;

use anyhow::Result;

use crate::cli::Command;
use crate::ctx::Ctx;

pub async fn run(ctx: &Ctx, command: Command) -> Result<()> {
    match command {
        Command::Login(a) => auth::login(ctx, a).await,
        Command::Logout => auth::logout(ctx).await,
        Command::Whoami => auth::whoami(ctx).await,
        Command::Catalog(a) => catalog::show(ctx, a).await,
        Command::Project(c) => project::run(ctx, c).await,
        Command::Member(c) => member::run(ctx, c).await,
        Command::Vm(c) => vm::run(ctx, c).await,
        Command::Events(a) => events::run(ctx, a).await,
        Command::SshConfig(a) => vm::ssh_config(ctx, a).await,
        Command::SshKey(c) => ssh_key::run(ctx, c).await,
    }
}
