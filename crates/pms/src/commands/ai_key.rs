use anyhow::{anyhow, bail, Result};
use pms_client::ai_keys::{AiKey, CreateAiKey, UpdateAiKey};
use pms_client::Client;

use crate::cli::AiKeyCmd;
use crate::ctx::Ctx;
use crate::output::{print_kv, print_table, ts, warn_one_time};

pub async fn run(ctx: &Ctx, cmd: AiKeyCmd) -> Result<()> {
    let project = ctx.project_id().await?;
    let client = ctx.client()?;
    match cmd {
        AiKeyCmd::List => {
            let keys = client.ai_keys(&project).await?;
            ctx.print(&keys, |keys| {
                print_table(
                    &[
                        "id",
                        "name",
                        "status",
                        "prefix",
                        "models",
                        "spend / budget",
                        "created",
                    ],
                    keys.iter()
                        .map(|k| {
                            vec![
                                k.id.clone(),
                                k.name.clone(),
                                k.status.clone(),
                                k.prefix.clone(),
                                k.models.join(","),
                                format!("{} / {}", usd(k.spend_usd), usd(k.budget_usd)),
                                ts(&k.created_at),
                            ]
                        })
                        .collect(),
                )
            });
        }
        AiKeyCmd::Create {
            name,
            models,
            budget,
        } => {
            let key = client
                .create_ai_key(
                    &project,
                    &CreateAiKey {
                        name,
                        models,
                        budget_usd: budget,
                    },
                )
                .await?;
            print_secret(ctx, &key);
        }
        AiKeyCmd::Show { key } => {
            let key = find(&client, &project, &key).await?;
            ctx.print(&key, details);
        }
        AiKeyCmd::Edit {
            key,
            models,
            budget,
        } => {
            if models.is_empty() && budget.is_none() {
                bail!("Nothing to change. Pass --model or --budget.");
            }
            let key = find(&client, &project, &key).await?;
            let update = UpdateAiKey {
                models: (!models.is_empty()).then_some(models),
                budget_usd: budget,
            };
            let key = client.update_ai_key(&project, &key.id, &update).await?;
            ctx.print(&key, details);
        }
        AiKeyCmd::Rotate { key } => {
            let key = find(&client, &project, &key).await?;
            ctx.confirm(&format!(
                "Make a new secret for key {}? The old secret stops working.",
                key.name
            ))?;
            let rotated = client.rotate_ai_key(&project, &key.id).await?;
            print_secret(ctx, &rotated);
        }
        AiKeyCmd::Delete { key } => {
            let key = find(&client, &project, &key).await?;
            ctx.confirm(&format!(
                "Delete AI key {} ({})? Apps that use it stop working.",
                key.name, key.id
            ))?;
            client.delete_ai_key(&project, &key.id).await?;
            ctx.done(&format!("Deleted AI key {}.", key.name));
        }
    }
    Ok(())
}

async fn find(client: &Client, project: &str, wanted: &str) -> Result<AiKey> {
    if wanted.starts_with("key_") {
        return Ok(client.ai_key(project, wanted).await?);
    }
    client
        .ai_keys(project)
        .await?
        .into_iter()
        .find(|k| k.name == wanted)
        .ok_or_else(|| anyhow!("No AI key named {wanted:?}. Run `pms ai-key list`."))
}

fn usd(v: f64) -> String {
    format!("${v:.2}")
}

fn details(k: &AiKey) {
    let mut rows = vec![
        ("ID", k.id.clone()),
        ("Name", k.name.clone()),
        ("Status", k.status.clone()),
        ("Prefix", k.prefix.clone()),
        ("Models", k.models.join(", ")),
        ("Budget", usd(k.budget_usd)),
        ("Spend", usd(k.spend_usd)),
        (
            "Rate limit",
            k.rate_limit_rpm
                .map(|r| format!("{r} requests per minute"))
                .unwrap_or_else(|| "-".into()),
        ),
        ("Created", ts(&k.created_at)),
    ];
    if let Some(s) = &k.secret {
        rows.push(("Secret", s.clone()));
    }
    print_kv(rows);
}

fn print_secret(ctx: &Ctx, key: &AiKey) {
    if !ctx.json() {
        warn_one_time("secret");
    }
    ctx.print(key, details);
}
