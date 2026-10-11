mod access;
mod ai_key;
mod auth;
mod catalog;
mod container;
mod db;
mod events;
mod member;
mod project;
mod registry;
mod ssh_key;
mod token;
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
        Command::Access(c) => access::run(ctx, c).await,
        Command::Container(c) => container::run(ctx, c).await,
        Command::RegistryCredential(c) => registry::run(ctx, c).await,
        Command::Db(c) => db::run(ctx, c).await,
        Command::AiKey(c) => ai_key::run(ctx, c).await,
        Command::SshKey(c) => ssh_key::run(ctx, c).await,
        Command::Token(c) => token::run(ctx, c).await,
    }
}
