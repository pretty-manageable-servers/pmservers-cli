use std::io::{IsTerminal, Read};

use anyhow::{anyhow, bail, Result};
use pms_client::containers::{CreateRegistryCredential, RegistryCredential};
use pms_client::Client;

use crate::cli::RegistryCmd;
use crate::ctx::Ctx;
use crate::output::{print_table, ts};

pub async fn find(client: &Client, wanted: &str) -> Result<RegistryCredential> {
    client
        .registry_credentials()
        .await?
        .into_iter()
        .find(|c| c.id == wanted || c.name == wanted)
        .ok_or_else(|| {
            anyhow!("No registry credential {wanted:?}. Run `pms registry-credential list`.")
        })
}

fn read_token(from_stdin: bool) -> Result<String> {
    let token = if from_stdin {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        s.trim().to_owned()
    } else if std::io::stdin().is_terminal() {
        dialoguer::Password::new().with_prompt("Token").interact()?
    } else {
        bail!("Pass --token-stdin to read the token from standard input.");
    };
    if token.is_empty() {
        bail!("The token is empty.");
    }
    Ok(token)
}

pub async fn run(ctx: &Ctx, cmd: RegistryCmd) -> Result<()> {
    let client = ctx.client()?;
    match cmd {
        RegistryCmd::List => {
            let list = client.registry_credentials().await?;
            ctx.print(&list, |list| {
                print_table(
                    &["id", "name", "host", "username", "created"],
                    list.iter()
                        .map(|c| {
                            vec![
                                c.id.clone(),
                                c.name.clone(),
                                c.host.clone(),
                                c.username.clone(),
                                ts(&c.created_at),
                            ]
                        })
                        .collect(),
                )
            });
        }
        RegistryCmd::Add {
            name,
            host,
            username,
            token_stdin,
        } => {
            let token = read_token(token_stdin)?;
            let saved = client
                .add_registry_credential(&CreateRegistryCredential {
                    name,
                    host,
                    username,
                    token,
                })
                .await?;
            // The API never returns the token.
            ctx.print(&saved, |c| {
                println!("Saved {} for {} ({}).", c.name, c.host, c.id)
            });
        }
        RegistryCmd::Delete { credential } => {
            let c = find(&client, &credential).await?;
            ctx.confirm(&format!(
                "Delete registry credential {} ({})?",
                c.name, c.id
            ))?;
            client.delete_registry_credential(&c.id).await?;
            ctx.done(&format!("Deleted registry credential {}.", c.name));
        }
    }
    Ok(())
}
