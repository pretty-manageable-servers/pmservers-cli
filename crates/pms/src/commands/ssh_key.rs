use std::io::{IsTerminal, Read};

use anyhow::{bail, Context, Result};

use crate::cli::SshKeyCmd;
use crate::ctx::Ctx;
use crate::output::{print_table, ts};

pub async fn run(ctx: &Ctx, cmd: SshKeyCmd) -> Result<()> {
    let client = ctx.client()?;
    match cmd {
        SshKeyCmd::List => {
            let keys = client.ssh_keys().await?;
            ctx.print(&keys, |keys| {
                print_table(
                    &["id", "name", "fingerprint", "added"],
                    keys.iter()
                        .map(|k| {
                            vec![
                                k.id.clone(),
                                k.name.clone(),
                                k.fingerprint.clone(),
                                ts(&k.created_at),
                            ]
                        })
                        .collect(),
                )
            });
        }
        SshKeyCmd::Add { name, file, key } => {
            let text = match (key, file) {
                (Some(k), _) => k,
                (None, Some(path)) => std::fs::read_to_string(&path)
                    .with_context(|| format!("cannot read {}", path.display()))?,
                (None, None) if !std::io::stdin().is_terminal() => {
                    let mut s = String::new();
                    std::io::stdin().read_to_string(&mut s)?;
                    s
                }
                (None, None) => {
                    bail!("Give --file <path>, --key <text>, or pipe the key to standard input.")
                }
            };
            let saved = client.add_ssh_key(&name, text.trim()).await?;
            ctx.print(&saved, |k| {
                println!("Saved {} ({}).", k.name, k.fingerprint)
            });
        }
        SshKeyCmd::Delete { id } => {
            ctx.confirm(&format!("Delete SSH key {id}?"))?;
            client.delete_ssh_key(&id).await?;
            ctx.done(&format!("Deleted SSH key {id}."));
        }
    }
    Ok(())
}
