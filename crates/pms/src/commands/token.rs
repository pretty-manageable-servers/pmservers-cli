use anyhow::Result;
use pms_client::tokens::CreatedApiToken;

use crate::cli::TokenCmd;
use crate::ctx::Ctx;
use crate::output::{print_kv, print_table, ts, ts_opt};

pub async fn run(ctx: &Ctx, cmd: TokenCmd) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    match cmd {
        TokenCmd::List => {
            let tokens = client.list_tokens(&project).await?;
            ctx.print(&tokens, |tokens| {
                print_table(
                    &["id", "name", "prefix", "expires", "last used", "created"],
                    tokens
                        .iter()
                        .map(|t| {
                            vec![
                                t.id.clone(),
                                t.name.clone(),
                                t.prefix.clone(),
                                expires(&t.expires_at),
                                ts_opt(&t.last_used_at),
                                ts(&t.created_at),
                            ]
                        })
                        .collect(),
                )
            });
        }
        TokenCmd::Create { name, expires } => {
            let created = client.create_token(&project, &name, expires.days()).await?;
            if !ctx.json() {
                eprintln!("Save the token now. It is shown only once and cannot be read again.");
            }
            ctx.print(&created, details);
        }
        TokenCmd::Revoke { token_id } => {
            ctx.confirm(&format!(
                "Revoke API token {token_id}? Anything that uses it stops working."
            ))?;
            client.revoke_token(&project, &token_id).await?;
            ctx.done(&format!("Revoked API token {token_id}."));
        }
    }
    Ok(())
}

fn expires(value: &Option<String>) -> String {
    value.as_deref().map(ts).unwrap_or_else(|| "never".into())
}

fn details(t: &CreatedApiToken) {
    print_kv(vec![
        ("ID", t.id.clone()),
        ("Name", t.name.clone()),
        ("Prefix", t.prefix.clone()),
        ("Expires", expires(&t.expires_at)),
        ("Created", ts(&t.created_at)),
        ("Token", t.token.clone()),
    ]);
}
