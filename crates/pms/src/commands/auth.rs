use std::io::IsTerminal;

use anyhow::{bail, Result};
use serde_json::json;

use crate::cli::LoginArgs;
use crate::ctx::Ctx;
use crate::login;
use crate::output::print_kv;
use crate::session::{self, Saved};
use crate::supabase::{Supabase, SupabaseConfig};

pub async fn login(ctx: &Ctx, args: LoginArgs) -> Result<()> {
    let cfg = SupabaseConfig::resolve()?;
    let session = if args.email {
        if !std::io::stdin().is_terminal() {
            bail!("`pms login --email` needs a terminal to ask for the password.");
        }
        let email: String = dialoguer::Input::new()
            .with_prompt("Email")
            .interact_text()?;
        let password = dialoguer::Password::new()
            .with_prompt("Password")
            .interact()?;
        login::password_login(cfg, email.trim(), &password).await?
    } else {
        login::browser_login(cfg, args.port, !args.no_browser).await?
    };
    let saved = session::save(&session)?;
    let place = match &saved {
        Saved::Keychain => "the OS keychain".to_owned(),
        Saved::File(p) => format!("{} (the OS keychain is not available)", p.display()),
    };
    if ctx.json() {
        crate::output::print_json(&json!({"email": session.email, "user_id": session.user_id}));
    } else {
        println!("Signed in as {}. Session saved in {place}.", session.email);
    }
    Ok(())
}

pub async fn logout(ctx: &Ctx) -> Result<()> {
    let Some(stored) = session::load()? else {
        ctx.done("You were not signed in.");
        return Ok(());
    };
    // Remove the local copy even if the server cannot be reached.
    let revoke = match SupabaseConfig::for_session(&stored.supabase_url) {
        Ok(cfg) => Supabase::new(cfg).logout(&stored.access_token).await,
        Err(e) => Err(e),
    };
    session::clear()?;
    if let Err(e) = revoke {
        eprintln!("warning: the session was removed here but could not be revoked: {e:#}");
    }
    ctx.done("Signed out.");
    Ok(())
}

pub async fn whoami(ctx: &Ctx) -> Result<()> {
    let me = ctx.client()?.me().await?;
    ctx.print(&me, |me| {
        print_kv(vec![
            ("Email", me.email.clone()),
            ("User ID", me.id.clone()),
            ("Plan", me.plan.clone()),
            ("API", ctx.api_url.clone()),
        ])
    });
    Ok(())
}
